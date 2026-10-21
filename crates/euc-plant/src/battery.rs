//! Battery: parallel strings of series cell groups, each with an equivalent circuit.
//!
//! One pack = `series` cell groups in series, each group an open-circuit voltage OCV(SOC), an
//! ohmic resistance R0 and one RC pair (R1, τ) for polarisation. Packs sit in parallel on the DC
//! bus and share the load by their impedance. Positive current discharges.
//!
//! Charge bounds delimit model validity, not a measured BMS cutoff. A step that would cross
//! either bound is rejected without changing state. Regeneration can recharge an empty pack.
//! See `docs/models/battery-validity.md` for the engine stop contract and model limitations.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Battery parameters (`[battery]` in `plant.toml`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatteryParams {
    /// Packs in parallel.
    pub packs: u8,
    /// Cell groups in series per pack (the cells a BMS reports).
    pub series: u8,
    /// Capacity of one pack, ampere-hours.
    pub capacity_ah: f64,
    /// Ohmic resistance of one cell group, ohm.
    pub r0_ohm: f64,
    /// Polarisation resistance of one cell group, ohm.
    pub r1_ohm: f64,
    /// Polarisation time constant, seconds.
    pub tau_s: f64,
    /// Open-circuit voltage of a cell group against state of charge: `[soc, volt]` pairs with
    /// `soc` rising from 0 to 1.
    pub ocv: Vec<[f64; 2]>,
    /// State of charge at power-on, 0..=1.
    pub soc: f64,
    /// Temperature sensors per pack (what the BMS reports).
    pub temp_sensors: u8,
    /// Heat capacity of one pack, joule per kelvin.
    pub heat_capacity_j_per_k: f64,
    /// Heat transfer of one pack to ambient, watt per kelvin.
    pub cooling_w_per_k: f64,
}

/// A parameter set that cannot describe a battery.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BatteryError {
    /// No pack or no cell.
    #[error("a battery needs at least one pack of at least one cell")]
    Empty,
    /// The OCV table lacks finite, positive voltages or rising SOC points covering 0 through 1.
    #[error("the OCV table needs finite positive voltages and rising SOC points from 0 through 1")]
    OcvTable,
    /// A circuit or thermal parameter is nonfinite or outside its mathematical domain.
    #[error("{0} is outside the battery model's finite parameter domain")]
    InvalidParameter(&'static str),
}

/// A boundary of the equivalent-circuit model, not a physical BMS trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BatteryLimit {
    /// A pack is empty or the requested step needs more charge than the pack contains.
    #[error("battery model exhausted at pack {pack}; no further discharge step is valid")]
    Exhausted {
        /// Zero-based pack index.
        pack: usize,
    },
    /// A charging step exceeds the pack's modeled capacity.
    #[error("battery model charge capacity exceeded at pack {pack}")]
    ChargeSaturated {
        /// Zero-based pack index.
        pack: usize,
    },
    /// Nonfinite inputs/results or a nonpositive loaded voltage invalidate the circuit solve.
    #[error("battery step is outside the finite, positive-voltage circuit domain")]
    InvalidStep,
}

impl BatteryParams {
    /// Checks the parameters.
    pub fn validate(&self) -> Result<(), BatteryError> {
        if self.packs == 0 || self.series == 0 {
            return Err(BatteryError::Empty);
        }
        if self.ocv.len() < 2
            || self.ocv.iter().any(|&[soc, volts]| !(0.0..=1.0).contains(&soc) || !volts.is_finite() || volts <= 0.0)
            || self.ocv.first().is_none_or(|point| point[0] > 0.0)
            || self.ocv.last().is_none_or(|point| point[0] < 1.0)
            || self.ocv.windows(2).any(|w| w[1][0] <= w[0][0])
        {
            return Err(BatteryError::OcvTable);
        }
        for (name, value) in [
            ("capacity_ah", self.capacity_ah),
            ("capacity_as", self.capacity_ah * 3600.0),
            ("r0_ohm", self.r0_ohm),
            ("r1_ohm", self.r1_ohm),
            ("tau_s", self.tau_s),
            ("heat_capacity_j_per_k", self.heat_capacity_j_per_k),
            ("cooling_w_per_k", self.cooling_w_per_k),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(BatteryError::InvalidParameter(name));
            }
        }
        if !(0.0..=1.0).contains(&self.soc) {
            return Err(BatteryError::InvalidParameter("soc"));
        }
        Ok(())
    }

    /// Open-circuit voltage of one cell group at `soc`, linear between table points and held at
    /// the ends.
    pub fn cell_ocv(&self, soc: f64) -> f64 {
        let table = &self.ocv;
        let i = table.partition_point(|p| p[0] < soc).clamp(1, table.len() - 1);
        let ([s0, v0], [s1, v1]) = (table[i - 1], table[i]);
        let t = ((soc - s0) / (s1 - s0)).clamp(0.0, 1.0);
        v0 + t * (v1 - v0)
    }
}

/// What is wrong with one pack (faults); a healthy pack has factor 1 and no cell drops.
#[derive(Debug, Clone, PartialEq)]
pub struct PackCondition {
    /// Multiplies the pack's R0 and R1 (aged cells, a loose connector).
    pub resistance_factor: f64,
    /// Volts each cell group's open-circuit voltage sits below the curve (a weak cell).
    pub cell_drop_v: Vec<f64>,
}

impl PackCondition {
    /// A healthy pack of `series` cell groups.
    pub fn healthy(series: u8) -> Self {
        Self { resistance_factor: 1.0, cell_drop_v: vec![0.0; usize::from(series)] }
    }
}

/// One pack's state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pack {
    /// State of charge, 0..=1.
    pub soc: f64,
    /// Voltage across the polarisation RC of the whole pack, volt.
    pub polarisation_v: f64,
    /// Current, ampere (positive = discharge).
    pub current_a: f64,
    /// Temperature, °C.
    pub temp_c: f64,
}

/// What a smart BMS reads from one pack.
#[derive(Debug, Clone, PartialEq)]
pub struct PackReading {
    /// Cell group voltages, millivolt.
    pub cells_mv: Vec<u16>,
    /// Temperature sensors, °C.
    pub temps_c: Vec<f64>,
    /// Pack current, ampere (positive = discharge).
    pub current_a: f64,
}

/// The battery: packs in parallel on the DC bus.
#[derive(Debug, Clone)]
pub struct Battery {
    params: BatteryParams,
    packs: Vec<Pack>,
    conditions: Vec<PackCondition>,
    terminal_v: f64,
}

impl Battery {
    /// A rested battery at the parameters' state of charge and `ambient_c`.
    pub fn new(params: BatteryParams, ambient_c: f64) -> Result<Self, BatteryError> {
        params.validate()?;
        if !ambient_c.is_finite() {
            return Err(BatteryError::InvalidParameter("ambient_c"));
        }
        let pack = Pack { soc: params.soc, polarisation_v: 0.0, current_a: 0.0, temp_c: ambient_c };
        let packs = vec![pack; usize::from(params.packs)];
        let conditions = vec![PackCondition::healthy(params.series); usize::from(params.packs)];
        let mut battery = Self { params, packs, conditions, terminal_v: 0.0 };
        battery.terminal_v = battery.solve(0.0).0;
        Ok(battery)
    }

    /// Terminal (bus) voltage at the last step's load.
    pub const fn voltage(&self) -> f64 {
        self.terminal_v
    }

    /// The packs.
    pub fn packs(&self) -> &[Pack] {
        &self.packs
    }

    /// The parameters.
    pub const fn params(&self) -> &BatteryParams {
        &self.params
    }

    /// Whether every pack contains charge for positive discharge.
    ///
    /// This conservative validity boundary assumes no unmodeled pack isolation or switching.
    /// It does not prohibit a separate regenerative [`step`](Self::step) from restoring charge.
    pub fn discharge_validity(&self) -> Result<(), BatteryLimit> {
        match self.packs.iter().position(|pack| pack.soc <= 0.0) {
            Some(pack) => Err(BatteryLimit::Exhausted { pack }),
            None => Ok(()),
        }
    }

    /// Changes pack `index`'s state of charge by `delta` (clamped to 0..=1).
    pub fn shift_soc(&mut self, index: usize, delta: f64) {
        if let Some(pack) = self.packs.get_mut(index) {
            pack.soc = (pack.soc + delta).clamp(0.0, 1.0);
        }
    }

    /// What is wrong with each pack.
    pub fn conditions(&self) -> &[PackCondition] {
        &self.conditions
    }

    /// Sets what is wrong with each pack (one entry per pack; missing packs stay as they are).
    pub fn set_conditions(&mut self, conditions: &[PackCondition]) {
        for (slot, condition) in self.conditions.iter_mut().zip(conditions) {
            slot.clone_from(condition);
        }
    }

    /// Ohmic resistance of pack `index`, ohm.
    fn r0(&self, index: usize) -> f64 {
        f64::from(self.params.series) * self.params.r0_ohm * self.conditions[index].resistance_factor
    }

    /// Open-circuit voltage of cell group `cell` of pack `index`, volt.
    fn cell_emf(&self, index: usize, cell: usize) -> f64 {
        self.params.cell_ocv(self.packs[index].soc) - self.conditions[index].cell_drop_v[cell]
    }

    /// Terminal voltage and pack currents for a bus load of `load_a`.
    fn solve(&self, load_a: f64) -> (f64, Vec<f64>) {
        let series = usize::from(self.params.series);
        let emf: Vec<f64> = (0..self.packs.len())
            .map(|k| (0..series).map(|c| self.cell_emf(k, c)).sum::<f64>() - self.packs[k].polarisation_v)
            .collect();
        let conductance: f64 = (0..emf.len()).map(|k| 1.0 / self.r0(k)).sum();
        // Subtract a reference before summation so identical rested packs give exactly zero
        // circulation at zero load, including at the charge boundaries.
        let reference = emf[0];
        let v = reference
            + (emf.iter().enumerate().map(|(k, e)| (e - reference) / self.r0(k)).sum::<f64>() - load_a) / conductance;
        let currents = emf.iter().enumerate().map(|(k, e)| (e - v) / self.r0(k)).collect();
        (v, currents)
    }

    /// Advances `h` seconds with the bus drawing `load_a` (negative = regeneration).
    ///
    /// Rejects the whole step before mutation if any pack's charge crosses `0..=1` or the
    /// solve leaves its finite, positive-voltage domain. Empty packs cannot supply positive
    /// discharge, but they can accept charging current. A zero-duration step changes nothing.
    /// The caller must stop or explicitly handle errors; voltage alone cannot prove validity.
    pub fn step(&mut self, load_a: f64, ambient_c: f64, h: f64) -> Result<(), BatteryLimit> {
        if !load_a.is_finite() || !ambient_c.is_finite() || !h.is_finite() || h < 0.0 {
            return Err(BatteryLimit::InvalidStep);
        }
        if h == 0.0 {
            return Ok(());
        }
        if load_a > 0.0 {
            self.discharge_validity()?;
        }
        let (v, currents) = self.solve(load_a);
        if !v.is_finite() || v <= 0.0 || currents.iter().any(|current| !current.is_finite()) {
            return Err(BatteryLimit::InvalidStep);
        }
        let series = f64::from(self.params.series);
        let relaxation = -(-h / self.params.tau_s).exp_m1();
        let capacity_as = self.params.capacity_ah * 3600.0;
        let heat_relaxation = -(-h * self.params.cooling_w_per_k / self.params.heat_capacity_j_per_k).exp_m1();
        // Prepare all packs before committing any of them. Never discard charge with an SOC clamp.
        let mut next = Vec::with_capacity(self.packs.len());
        for (k, current) in currents.into_iter().enumerate() {
            let mut pack = self.packs[k];
            let charge_delta = current / capacity_as * h;
            if current > 0.0 && (pack.soc <= 0.0 || charge_delta > pack.soc) {
                return Err(BatteryLimit::Exhausted { pack: k });
            }
            if current < 0.0 && (pack.soc >= 1.0 || -charge_delta > 1.0 - pack.soc) {
                return Err(BatteryLimit::ChargeSaturated { pack: k });
            }
            let factor = self.conditions[k].resistance_factor;
            let (r0, r1) = (series * self.params.r0_ohm * factor, series * self.params.r1_ohm * factor);
            pack.current_a = current;
            pack.polarisation_v += relaxation * (r1 * current - pack.polarisation_v);
            pack.soc -= charge_delta;
            let heat = current * current * r0 + pack.polarisation_v * pack.polarisation_v / r1;
            pack.temp_c += heat_relaxation * (ambient_c - pack.temp_c + heat / self.params.cooling_w_per_k);
            if !pack.soc.is_finite() || !pack.polarisation_v.is_finite() || !pack.temp_c.is_finite() {
                return Err(BatteryLimit::InvalidStep);
            }
            next.push(pack);
        }
        self.packs = next;
        self.terminal_v = v;
        Ok(())
    }

    /// What each pack's BMS reads now. Each cell group shows its open-circuit voltage less its
    /// share of the pack's ohmic and polarisation drops, so a pack's cells add up to the bus.
    pub fn readings(&self) -> Vec<PackReading> {
        let series = usize::from(self.params.series);
        let mv = |v: f64| (v * 1000.0).round_ties_even().clamp(0.0, 65_535.0) as u16;
        (0..self.packs.len())
            .map(|k| {
                let pack = &self.packs[k];
                let drop = (pack.current_a * self.r0(k) + pack.polarisation_v) / series as f64;
                PackReading {
                    cells_mv: (0..series).map(|c| mv(self.cell_emf(k, c) - drop)).collect(),
                    temps_c: vec![pack.temp_c; usize::from(self.params.temp_sensors)],
                    current_a: pack.current_a,
                }
            })
            .collect()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Two 32S2P packs of 5 Ah cells (see `profiles/begode-extreme/plant.toml`).
    pub(crate) fn extreme() -> BatteryParams {
        BatteryParams {
            packs: 2,
            series: 32,
            capacity_ah: 10.0,
            r0_ohm: 0.007,
            r1_ohm: 0.003,
            tau_s: 30.0,
            ocv: vec![
                [0.0, 3.00],
                [0.05, 3.30],
                [0.1, 3.42],
                [0.2, 3.52],
                [0.3, 3.60],
                [0.4, 3.66],
                [0.5, 3.72],
                [0.6, 3.80],
                [0.7, 3.88],
                [0.8, 3.97],
                [0.9, 4.07],
                [1.0, 4.20],
            ],
            soc: 0.9,
            temp_sensors: 6,
            heat_capacity_j_per_k: 6000.0,
            cooling_w_per_k: 3.0,
        }
    }

    #[test]
    fn ocv_interpolates_and_holds_at_the_ends() {
        let p = extreme();
        assert!((p.cell_ocv(0.85) - 4.02).abs() < 1e-12);
        assert_eq!((p.cell_ocv(-0.1), p.cell_ocv(1.5)), (3.00, 4.20));
        let rested = Battery::new(p, 25.0).unwrap();
        assert!((rested.voltage() - 32.0 * 4.07).abs() < 1e-9, "130.24 V at 90 %");
    }

    #[test]
    fn sag_and_recovery() {
        let mut b = Battery::new(extreme(), 25.0).unwrap();
        b.step(100.0, 25.0, 1e-3).unwrap();
        let sag = 130.24 - b.voltage();
        // Two packs of 32 × 7 mΩ in parallel: 0.112 Ω, 11.2 V at 100 A.
        assert!((sag - 11.2).abs() < 0.01, "sag {sag}");
        assert!(b.packs().iter().all(|p| (p.current_a - 50.0).abs() < 1e-9), "shared equally");
        for _ in 0..10_000 {
            b.step(100.0, 25.0, 1e-2).unwrap();
        }
        let polarised = b.voltage();
        assert!(polarised < 130.24 - 11.2 - 3.0, "polarisation adds sag: {polarised}");
        b.step(0.0, 25.0, 1e-3).unwrap();
        assert!(b.voltage() > polarised + 11.0, "ohmic part recovers at once");
    }

    #[test]
    fn charge_balance_and_regeneration() {
        let mut b = Battery::new(extreme(), 25.0).unwrap();
        // 20 A for 180 s draws 1 Ah: 5 % of each 10 Ah pack.
        for _ in 0..18_000 {
            b.step(20.0, 25.0, 1e-2).unwrap();
        }
        assert!(b.packs().iter().all(|p| (p.soc - 0.85).abs() < 1e-9));
        let before = b.voltage();
        b.step(-40.0, 25.0, 1e-3).unwrap();
        assert!(b.voltage() > before + 4.0, "regeneration lifts the bus");
        assert!(b.packs()[0].current_a < 0.0);
    }

    #[test]
    fn unequal_packs_share_unequally() {
        let mut b = Battery::new(extreme(), 25.0).unwrap();
        b.packs[1].soc = 0.5;
        b.step(30.0, 25.0, 1e-3).unwrap();
        let [a, low] = [b.packs()[0].current_a, b.packs()[1].current_a];
        assert!(a > low && (a + low - 30.0).abs() < 1e-9, "{a} {low}");
    }

    #[test]
    fn a_weak_cell_and_a_resistive_pack() {
        let mut b = Battery::new(extreme(), 25.0).unwrap();
        let mut weak = PackCondition::healthy(32);
        weak.cell_drop_v[5] = 0.4;
        let resistive = PackCondition { resistance_factor: 3.0, ..PackCondition::healthy(32) };
        b.set_conditions(&[weak, resistive]);
        b.step(60.0, 25.0, 1e-3).unwrap();
        let r = b.readings();
        // The weak pack's cell 5 reads about 0.4 V low; its other cells carry the rest of the bus.
        assert!(f64::from(r[0].cells_mv[4]) - f64::from(r[0].cells_mv[5]) > 390.0, "{:?}", &r[0].cells_mv[..6]);
        for pack in &r {
            let sum: f64 = pack.cells_mv.iter().map(|&mv| f64::from(mv)).sum();
            assert!((sum / 1000.0 - b.voltage()).abs() < 0.02, "cells add up to the bus");
        }
        // Three times the resistance: about a quarter of the load.
        assert!((r[0].current_a + r[1].current_a - 60.0).abs() < 1e-9);
        assert!(r[1].current_a < 30.0, "{} A", r[1].current_a);
        b.shift_soc(1, -0.3);
        assert!((b.packs()[1].soc - 0.6).abs() < 1e-4);
    }

    #[test]
    fn heating_and_readings() {
        let mut b = Battery::new(extreme(), 25.0).unwrap();
        // 300 s stays below the 432 s discharge boundary at 75 A per pack.
        for _ in 0..30_000 {
            b.step(150.0, 25.0, 1e-2).unwrap();
        }
        let readings = b.readings();
        assert_eq!((readings.len(), readings[0].cells_mv.len(), readings[0].temps_c.len()), (2, 32, 6));
        assert!(readings[0].temps_c[0] > 30.0, "{}", readings[0].temps_c[0]);
        assert!((f64::from(readings[0].cells_mv[0]) - b.voltage() / 32.0 * 1000.0).abs() <= 0.5);
        assert_eq!(
            Battery::new(BatteryParams { ocv: vec![[0.0, 3.0]], ..extreme() }, 25.0).err(),
            Some(BatteryError::OcvTable)
        );
    }

    fn assert_unchanged(battery: &Battery, before: &Battery) {
        assert_eq!(battery.packs(), before.packs());
        assert_eq!(battery.voltage(), before.voltage());
        assert_eq!(battery.conditions(), before.conditions());
        assert_eq!(battery.readings(), before.readings());
    }

    #[test]
    fn empty_battery_rejects_discharge_without_supplying_charge() {
        let mut battery = Battery::new(BatteryParams { soc: 0.0, ..extreme() }, 25.0).unwrap();
        let before = battery.clone();
        assert!(battery.voltage() > 0.0, "OCV alone cannot prove validity");
        assert_eq!(battery.discharge_validity(), Err(BatteryLimit::Exhausted { pack: 0 }));
        for load in [f64::EPSILON, 1.0, 100.0] {
            for h in [5e-6, 1.0, 1e6] {
                assert_eq!(battery.step(load, 25.0, h), Err(BatteryLimit::Exhausted { pack: 0 }));
                assert_unchanged(&battery, &before);
            }
        }
        let removed_charge: f64 = before
            .packs()
            .iter()
            .zip(battery.packs())
            .map(|(old, new)| (old.soc - new.soc) * battery.params().capacity_ah * 3600.0)
            .sum();
        assert_eq!(removed_charge, 0.0);
    }

    #[test]
    fn rested_charge_endpoints_have_no_false_circulation() {
        for soc in [0.0, 1.0] {
            let mut battery = Battery::new(BatteryParams { soc, ..extreme() }, 25.0).unwrap();
            let before = battery.clone();
            for h in [5e-6, 1.0, 1000.0] {
                battery.step(0.0, 25.0, h).unwrap();
                assert_unchanged(&battery, &before);
            }
            battery.step(f64::MAX, -20.0, 0.0).unwrap();
            assert_unchanged(&battery, &before);
        }
    }

    #[test]
    fn regeneration_restores_empty_battery_discharge_validity() {
        let mut battery = Battery::new(BatteryParams { soc: 0.0, ..extreme() }, 25.0).unwrap();
        battery.step(-20.0, 25.0, 1.0).unwrap();
        assert_eq!(battery.discharge_validity(), Ok(()));
        for pack in battery.packs() {
            assert!(pack.current_a < 0.0);
            assert!((pack.soc - 10.0 / 36_000.0).abs() < 1e-12);
        }
        let recovered = battery.clone();
        battery.step(1.0, 25.0, 1e-3).unwrap();
        for (old, pack) in recovered.packs().iter().zip(battery.packs()) {
            assert!(pack.current_a > 0.0);
            assert!(pack.soc > 0.0 && pack.soc < old.soc);
            assert!((old.soc - pack.soc - pack.current_a * 1e-3 / 36_000.0).abs() < 1e-15);
        }
    }

    #[test]
    fn full_battery_rejects_regeneration_but_accepts_discharge() {
        let mut battery = Battery::new(BatteryParams { soc: 1.0, ..extreme() }, 25.0).unwrap();
        let before = battery.clone();
        for h in [5e-6, 1.0, 1000.0] {
            assert_eq!(battery.step(-20.0, 25.0, h), Err(BatteryLimit::ChargeSaturated { pack: 0 }));
            assert_unchanged(&battery, &before);
        }
        assert_eq!(battery.discharge_validity(), Ok(()));
        battery.step(20.0, 25.0, 1.0).unwrap();
        assert!(battery.packs().iter().all(|pack| pack.soc < 1.0 && pack.current_a > 0.0));
    }

    #[test]
    fn steps_can_land_exactly_on_charge_endpoints() {
        for (soc, load, endpoint) in [(0.25, 2.0, 0.0), (0.75, -2.0, 1.0)] {
            // Synthetic constant OCV: each pack carries exactly +/-1 A for 900 s.
            let params = BatteryParams {
                series: 1,
                capacity_ah: 1.0,
                r0_ohm: 1.0,
                ocv: vec![[0.0, 4.0], [1.0, 4.0]],
                soc,
                ..extreme()
            };
            let mut battery = Battery::new(params, 25.0).unwrap();
            battery.step(load, 25.0, 900.0).unwrap();
            for pack in battery.packs() {
                assert_eq!(pack.soc, endpoint, "the step lands exactly on the boundary");
            }
            let before = battery.clone();
            let limit = if endpoint == 0.0 {
                BatteryLimit::Exhausted { pack: 0 }
            } else {
                BatteryLimit::ChargeSaturated { pack: 0 }
            };
            assert_eq!(battery.step(load, 25.0, 5e-6), Err(limit));
            assert_unchanged(&battery, &before);
        }
    }

    #[test]
    fn unequal_pack_oversteps_reject_every_pack_atomically() {
        for (soc, load, limit) in [
            (1e-5, 200.0, BatteryLimit::Exhausted { pack: 1 }),
            (1.0 - 1e-5, -200.0, BatteryLimit::ChargeSaturated { pack: 1 }),
        ] {
            let mut battery = Battery::new(extreme(), 25.0).unwrap();
            battery.step(20.0, 25.0, 0.1).unwrap();
            battery.packs[0].soc = 0.5;
            battery.packs[1].soc = soc;
            battery.conditions[1].resistance_factor = 2.0;
            let h = 0.1;
            let (_, currents) = battery.solve(load);
            assert!((currents.iter().sum::<f64>() - load).abs() < 1e-9);
            assert!((currents[0] - currents[1]).abs() > 1.0);
            let candidate_soc: Vec<f64> =
                battery.packs().iter().zip(currents).map(|(pack, current)| pack.soc - current * h / 36_000.0).collect();
            assert!((0.0..=1.0).contains(&candidate_soc[0]));
            assert!(!((0.0..=1.0).contains(&candidate_soc[1])));
            let before = battery.clone();
            assert_eq!(battery.step(load, 30.0, h), Err(limit));
            assert_unchanged(&battery, &before);
            assert!(battery.packs().iter().all(|pack| pack.soc > 0.0 && pack.soc < 1.0));
        }
    }

    #[test]
    fn one_empty_pack_invalidates_discharge_despite_positive_mean_soc() {
        for empty in 0..2 {
            let mut battery = Battery::new(extreme(), 25.0).unwrap();
            battery.shift_soc(empty, -1.0);
            let mean = battery.packs().iter().map(|pack| pack.soc).sum::<f64>() / 2.0;
            assert!(mean > 0.4);
            assert_eq!(battery.discharge_validity(), Err(BatteryLimit::Exhausted { pack: empty }));
            let before = battery.clone();
            assert_eq!(battery.step(1.0, 25.0, 5e-6), Err(BatteryLimit::Exhausted { pack: empty }));
            assert_unchanged(&battery, &before);
        }
    }

    #[test]
    fn circulating_current_checks_pack_bounds_even_with_zero_bus_load() {
        for (soc, limit) in [
            ([1.0, 1.0 - 1e-5], BatteryLimit::ChargeSaturated { pack: 1 }),
            ([1e-5, 0.0], BatteryLimit::Exhausted { pack: 0 }),
        ] {
            let mut battery = Battery::new(extreme(), 25.0).unwrap();
            for (pack, soc) in battery.packs.iter_mut().zip(soc) {
                pack.soc = soc;
            }
            let (_, currents) = battery.solve(0.0);
            assert!(currents[0] > 0.0 && currents[1] < 0.0);
            assert!(currents.iter().sum::<f64>().abs() < 1e-9);
            let before = battery.clone();
            assert_eq!(battery.step(0.0, 25.0, 1000.0), Err(limit));
            assert_unchanged(&battery, &before);
        }
    }

    #[test]
    fn invalid_step_inputs_and_loaded_voltage_are_atomic() {
        let mut battery = Battery::new(extreme(), 25.0).unwrap();
        battery.step(20.0, 25.0, 0.1).unwrap();
        let before = battery.clone();
        for (load, ambient, h) in [
            (f64::NAN, 25.0, 1.0),
            (f64::INFINITY, 25.0, 1.0),
            (f64::NEG_INFINITY, 25.0, 1.0),
            (1.0, f64::NAN, 1.0),
            (1.0, f64::INFINITY, 1.0),
            (1.0, 25.0, f64::NAN),
            (1.0, 25.0, f64::INFINITY),
            (1.0, 25.0, -1.0),
            (10_000.0, 25.0, 5e-6),
        ] {
            assert_eq!(battery.step(load, ambient, h), Err(BatteryLimit::InvalidStep));
            assert_unchanged(&battery, &before);
        }
        battery.step(100.0, -20.0, 0.0).unwrap();
        assert_unchanged(&battery, &before);
    }
}
