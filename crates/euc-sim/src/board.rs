//! The simulator's own components: the board logic around the controller and the DWT.

use std::collections::{HashMap, VecDeque};

use euc_core::{Ctx, Peripheral, Registry, ResetKind, Scheduler, Signal};

/// Port numbers of [`BoardLogic`].
mod port {
    pub(super) const BLE_RX: u8 = 0;
    pub(super) const BLE_TX: u8 = 1;
    pub(super) const LATCH: u8 = 2;
    pub(super) const BUTTON: u8 = 3;
    pub(super) const HANDLE: u8 = 4;
    pub(super) const HEADLIGHT: u8 = 5;
}

/// Maximum number of headlight edges retained for the rolling duty measurement.
const LIGHT_EDGES: usize = 256;

/// Something the board does at a set time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// A phone sends these bytes over BLE.
    Inject(Vec<u8>),
    /// The rider lets go of the power button.
    ReleaseButton,
    /// The battery is switched off and on.
    PowerCycle,
}

/// Everything around the controller that the firmware talks to through pins and the BLE UART:
/// the phone (capturing what the controller sends, injecting commands), the power button and
/// latch. Timed actions run in `(cycle, order)` order, like the reference harness's timeline,
/// which is why this component is the last one on the board. (The hall lines come from the
/// plant.)
pub struct BoardLogic {
    button_active_low: bool,
    release_after: u64,
    button_pressed: bool,
    handle_pressed: bool,
    /// Interactive GPIO levels, independent of the boot hold and simulated-time pulses.
    held_buttons: [bool; 2],
    pulse_until: [Option<u64>; 2],
    boot_release_at: Option<u64>,
    latch_time: Option<u64>,
    latch_level: Option<bool>,
    /// Actual output edges, not the firmware's requested light mode or duty variable.
    light_edges: VecDeque<(u64, Option<bool>)>,
    actions: Scheduler<Action>,
    /// Bytes the controller sent over BLE, with the cycle each started on the wire.
    pub ble_tx: Vec<(u64, u8)>,
}

pub(crate) const TIMELINE: u32 = 0;

impl BoardLogic {
    /// Board logic whose rider lets go of the button `release_after` cycles after the firmware
    /// sets the power latch.
    pub fn new(button_active_low: bool, release_after: u64) -> Self {
        Self {
            button_active_low,
            release_after,
            button_pressed: true,
            handle_pressed: false,
            held_buttons: [false; 2],
            pulse_until: [None; 2],
            boot_release_at: None,
            latch_time: None,
            latch_level: None,
            light_edges: VecDeque::new(),
            actions: Scheduler::new(),
            ble_tx: Vec::new(),
        }
    }

    /// Schedules `action` at cycle `at` (before the run starts).
    pub fn at(&mut self, at: u64, action: Action) {
        self.actions.schedule(at, action);
    }

    fn arm(&mut self, ctx: &mut Ctx<'_>) {
        let next = self.actions.next_at().into_iter().chain(self.pulse_until.iter().flatten().copied()).min();
        match next {
            Some(at) => ctx.schedule(at, TIMELINE),
            None => ctx.cancel(TIMELINE),
        }
    }

    /// Drives the button pin.
    fn apply_inputs(&self, ctx: &mut Ctx<'_>) {
        ctx.emit(port::BUTTON, Signal::Level((self.button_pressed || self.held_buttons[0]) != self.button_active_low));
        // PA12 is low while the handle button is pressed (board.h and control.c).
        ctx.emit(port::HANDLE, Signal::Level(!(self.handle_pressed || self.held_buttons[1])));
    }

    /// Presses a physical input for 1..=10000 simulated milliseconds, then releases it.
    ///
    /// Call through the machine with a real component context so GPIO and timer wiring update.
    /// Different buttons can overlap; a second pulse on the same button is refused.
    /// Firmware owns all gesture interpretation. No firmware RAM variable is changed here.
    pub fn pulse_button(&mut self, button: &str, duration_ms: u32, ctx: &mut Ctx<'_>) -> Result<(), String> {
        if !(1..=10_000).contains(&duration_ms) {
            return Err("Button duration must be 1 through 10000 ms".into());
        }
        let index = match button {
            "power" => 0,
            "handle" => 1,
            _ => return Err("Unknown physical button".into()),
        };
        if self.latch_time.is_none() || self.boot_release_at.is_some() || (self.button_pressed && index == 0) {
            return Err("Wait for the firmware to finish the initial power-button hold".into());
        }
        if self.latch_level != Some(true) {
            return Err("The power latch is not high; use the live Power input to turn the controller on".into());
        }
        if self.pulse_until[index].is_some() || self.held_buttons[index] {
            return Err("This button already has an active pulse".into());
        }
        let cycles = ctx.hclk().saturating_mul(u64::from(duration_ms)).div_ceil(1000).max(1);
        let until = ctx.now().checked_add(cycles).ok_or("Button release time overflow")?;
        self.pulse_until[index] = Some(until);
        if index == 0 {
            self.button_pressed = true;
        } else {
            self.handle_pressed = true;
        }
        ctx.log(format!("{button} button pressed for {duration_ms} ms"));
        self.apply_inputs(ctx);
        self.arm(ctx);
        Ok(())
    }

    /// Applies a live level at the current simulated cycle. The desktop runner owns its wall lease.
    /// Repeated pressed states leave the pin held; they never schedule or extend a pulse.
    pub fn set_button(&mut self, button: &str, pressed: bool, ctx: &mut Ctx<'_>) -> Result<(), String> {
        let index = match button {
            "power" => 0,
            "handle" => 1,
            _ => return Err("Unknown physical button".into()),
        };
        if pressed {
            if !(if index == 0 { self.power_button_ready() } else { self.buttons_ready() }) {
                return Err("This physical input is not ready".into());
            }
            if self.pulse_until[index].is_some() {
                return Err("This button already has an active pulse".into());
            }
        }
        if self.held_buttons[index] != pressed {
            self.held_buttons[index] = pressed;
            ctx.log(format!("{button} button {}", if pressed { "pressed" } else { "released" }));
            self.apply_inputs(ctx);
        }
        Ok(())
    }

    /// Releases desktop pulses and live holds without changing the initial boot hold or advancing the CPU.
    pub fn release_button_pulses(&mut self, ctx: &mut Ctx<'_>) {
        self.held_buttons = [false; 2];
        if self.pulse_until[0].take().is_some() {
            self.button_pressed = false;
        }
        if self.pulse_until[1].take().is_some() {
            self.handle_pressed = false;
        }
        self.apply_inputs(ctx);
        self.arm(ctx);
    }

    /// Whether the initial power hold finished and firmware currently drives the latch high.
    pub fn buttons_ready(&self) -> bool {
        self.latch_time.is_some() && self.latch_level == Some(true) && self.boot_release_at.is_none()
    }

    /// Whether a live Power input can start or renew. Power can wake the unpowered controller;
    /// Handle cannot. The initial synthetic boot hold remains unavailable to interactive input.
    pub fn power_button_ready(&self) -> bool {
        !self.button_pressed && (self.held_buttons[0] || self.buttons_ready() || !self.supply_powered())
    }

    /// Controller-rail enable in this board model, not the battery or the firmware latch alone.
    /// INFERRED wiring: the momentary Power contact and the MCU latch independently sustain it.
    /// The boot contract in `firmware/apps/begode-extreme/main.c::power_on_hold` supports this;
    /// no rail decay, capacitance, or measured transistor circuit is claimed.
    pub fn supply_powered(&self) -> bool {
        self.button_pressed || self.held_buttons[0] || self.latch_level == Some(true)
    }

    /// Starts an actual user-held power-on. Preserve the contact; do not synthesize a boot hold.
    pub(crate) fn begin_supply_boot(&mut self, ctx: &mut Ctx<'_>) {
        self.latch_time = None;
        self.latch_level = None;
        self.boot_release_at = None;
        self.button_pressed = false;
        self.handle_pressed = false;
        self.held_buttons[1] = false;
        self.pulse_until = [None; 2];
        self.light_edges.clear();
        self.apply_inputs(ctx);
        self.arm(ctx);
    }

    /// Desktop pulses still held on the simulated clock: power, then handle.
    pub fn button_pulses(&self) -> [bool; 2] {
        self.pulse_until.map(|until| until.is_some())
    }

    /// Last observed latch GPIO level. A low latch with Power held does not imply supply loss.
    pub const fn power_latched(&self) -> Option<bool> {
        self.latch_level
    }

    /// Last observed instantaneous headlight GPIO level; `None` means released or unwired.
    pub fn headlight_pin_high(&self) -> Option<bool> {
        self.light_edges.back().and_then(|&(_, level)| level)
    }

    /// High-time fraction over up to 40 ms of observed GPIO edges, in simulated time.
    ///
    /// A released interval or no elapsed observation returns `None`. The bounded edge history
    /// can shorten the window for very fast inputs. This measures the simulated pin, not light flux.
    pub fn headlight_duty(&self, now: u64, hclk: u64) -> Option<f64> {
        let &(first, _) = self.light_edges.front()?;
        let start = now.saturating_sub((hclk / 25).max(1)).max(first);
        let elapsed = now.checked_sub(start).filter(|&ticks| ticks > 0)?;
        let mut high = 0_u64;
        for (i, &(at, level)) in self.light_edges.iter().enumerate() {
            let end = self.light_edges.get(i + 1).map_or(now, |&(at, _)| at).min(now);
            let ticks = end.saturating_sub(at.max(start));
            if ticks > 0 && level? {
                high += ticks;
            }
        }
        Some(high as f64 / elapsed as f64)
    }

    fn light_edge(&mut self, now: u64, level: Option<bool>, hclk: u64) {
        if self.light_edges.back().is_some_and(|&(_, last)| last == level) {
            return;
        }
        let cutoff = now.saturating_sub((hclk / 25).max(1));
        while self.light_edges.len() > 1 && self.light_edges.get(1).is_some_and(|&(at, _)| at <= cutoff) {
            self.light_edges.pop_front();
        }
        if self.light_edges.len() == LIGHT_EDGES {
            self.light_edges.pop_front();
        }
        self.light_edges.push_back((now, level));
    }
}

impl crate::machine::Machine {
    /// Sets a live physical input at the current cycle, without advancing the CPU.
    /// Release remains available after halt. The interactive caller must enforce a wall lease.
    pub fn set_button(&mut self, button: &str, pressed: bool) -> Result<(), String> {
        if pressed && self.halted().is_some() {
            return Err("The simulator halted; restart before pressing a button".into());
        }
        let board = self
            .sys
            .slots
            .iter()
            .position(|slot| slot.name == crate::machine::BOARD)
            .and_then(|index| euc_core::DeviceId::try_from(index).ok())
            .ok_or("The machine has no board inputs")?;
        let result = self
            .sys
            .call(board, |device, ctx| {
                (device as &mut dyn std::any::Any)
                    .downcast_mut::<BoardLogic>()
                    .ok_or_else(|| "The board does not support physical buttons".to_owned())?
                    .set_button(button, pressed, ctx)
            })
            .ok_or("The board inputs are busy")?;
        self.sys.drain();
        self.sync_supply();
        result
    }
    /// Pulses a wired physical button without running firmware or writing firmware RAM.
    ///
    /// Call only between machine runs. `BoardLogic` owns the GPIO levels and the timed release.
    pub fn pulse_button(&mut self, button: &str, duration_ms: u32) -> Result<(), String> {
        if self.halted().is_some() {
            return Err("The simulator halted; restart before pressing a button".into());
        }
        let board = self
            .sys
            .slots
            .iter()
            .position(|slot| slot.name == crate::machine::BOARD)
            .and_then(|index| euc_core::DeviceId::try_from(index).ok())
            .ok_or("The machine has no board inputs")?;
        let result = self
            .sys
            .call(board, |device, ctx| {
                (device as &mut dyn std::any::Any)
                    .downcast_mut::<BoardLogic>()
                    .ok_or_else(|| "The board does not support physical buttons".to_owned())?
                    .pulse_button(button, duration_ms, ctx)
            })
            .ok_or("The board inputs are busy")?;
        self.sys.drain();
        result
    }

    /// Releases desktop pulses and live holds without advancing the CPU, including while paused.
    pub fn release_button_pulses(&mut self) {
        let board = self
            .sys
            .slots
            .iter()
            .position(|slot| slot.name == crate::machine::BOARD)
            .and_then(|index| euc_core::DeviceId::try_from(index).ok());
        if let Some(board) = board {
            self.sys.call(board, |device, ctx| {
                if let Some(logic) = (device as &mut dyn std::any::Any).downcast_mut::<BoardLogic>() {
                    logic.release_button_pulses(ctx);
                }
            });
            self.sys.drain();
            self.sync_supply();
        }
    }
}

impl Peripheral for BoardLogic {
    fn reset(&mut self, kind: ResetKind, ctx: &mut Ctx<'_>) {
        self.held_buttons = [false; 2];
        if kind == ResetKind::Power {
            self.latch_time = None;
            self.latch_level = None;
            self.boot_release_at = None;
            self.pulse_until = [None; 2];
            self.button_pressed = true;
            self.handle_pressed = false;
            self.light_edges.clear();
            self.apply_inputs(ctx);
        } else {
            self.release_button_pulses(ctx);
        }
        self.arm(ctx);
    }

    fn on_event(&mut self, _tag: u32, ctx: &mut Ctx<'_>) {
        while let Some((_, action)) = self.actions.pop_due(ctx.now()) {
            match action {
                Action::Inject(bytes) => {
                    ctx.log(format!("BLE RX inject {:?}", String::from_utf8_lossy(&bytes)));
                    for byte in bytes {
                        ctx.emit(port::BLE_TX, Signal::Byte { value: u16::from(byte), at: ctx.now() });
                    }
                }
                Action::ReleaseButton => {
                    if self.boot_release_at.is_some_and(|at| at <= ctx.now()) {
                        ctx.log("power button released");
                        self.boot_release_at = None;
                        self.button_pressed = false;
                        self.apply_inputs(ctx);
                    }
                }
                Action::PowerCycle => ctx.request_reset(ResetKind::Power, "power cycle"),
            }
        }
        for index in 0..2 {
            if self.pulse_until[index].is_some_and(|at| at <= ctx.now()) {
                self.pulse_until[index] = None;
                if index == 0 {
                    self.button_pressed = false;
                } else {
                    self.handle_pressed = false;
                }
                ctx.log(if index == 0 { "power button pulse released" } else { "handle button pulse released" });
                self.apply_inputs(ctx);
            }
        }
        self.arm(ctx);
    }

    fn input(&mut self, port: u8, signal: Signal, ctx: &mut Ctx<'_>) {
        match (port, signal) {
            (port::BLE_RX, Signal::Byte { value, at }) => self.ble_tx.push((at, value as u8)),
            (port::LATCH, Signal::Level(high)) => {
                self.latch_level = Some(high);
                ctx.log(format!("power latch -> {}", u8::from(high)));
                if high && self.latch_time.is_none() {
                    self.latch_time = Some(ctx.now());
                    if self.button_pressed {
                        let release = ctx.now().saturating_add(self.release_after);
                        self.boot_release_at = Some(release);
                        self.actions.schedule(release, Action::ReleaseButton);
                        self.arm(ctx);
                    }
                }
            }
            (port::LATCH, Signal::Released) => self.latch_level = None,
            (port::HEADLIGHT, Signal::Level(high)) => self.light_edge(ctx.now(), Some(high), ctx.hclk()),
            (port::HEADLIGHT, Signal::Released) => self.light_edge(ctx.now(), None, ctx.hclk()),
            _ => {}
        }
    }

    fn port(&self, name: &str) -> Option<u8> {
        Some(match name {
            "ble_rx" => port::BLE_RX,
            "ble_tx" => port::BLE_TX,
            "latch" => port::LATCH,
            "button" => port::BUTTON,
            "handle" => port::HANDLE,
            "headlight" => port::HEADLIGHT,
            _ => return None,
        })
    }
}

/// The Cortex-M3 data watchpoint and trace unit: `CYCCNT` counts HCLK; other registers read
/// back what was written.
#[derive(Default)]
pub struct Dwt {
    regs: HashMap<u32, u32>,
}

impl Peripheral for Dwt {
    fn reset(&mut self, _kind: ResetKind, _ctx: &mut Ctx<'_>) {
        self.regs.clear();
    }

    fn read(&mut self, off: u32, _mask: u32, ctx: &mut Ctx<'_>) -> u32 {
        match off {
            0x004 => ctx.now() as u32,
            _ => self.regs.get(&off).copied().unwrap_or(0),
        }
    }

    fn write(&mut self, off: u32, value: u32, mask: u32, _ctx: &mut Ctx<'_>) {
        let reg = self.regs.entry(off).or_default();
        *reg = (*reg & !mask) | (value & mask);
    }

    fn reg_name(&self, off: u32) -> Option<&'static str> {
        match off {
            0x000 => Some("CTRL"),
            0x004 => Some("CYCCNT"),
            _ => None,
        }
    }
}

/// Registers the simulator's own kinds (`cortex-m3.dwt`).
pub fn register(registry: &mut Registry) {
    registry.register("cortex-m3.dwt", |_| Ok(Box::new(Dwt::default())));
}

#[cfg(test)]
mod tests {
    use euc_core::testing::TestHost;

    use super::*;

    fn live_ready() -> (TestHost, BoardLogic) {
        let mut host = TestHost::new(1_000);
        let mut board = BoardLogic::new(true, 1);
        board.reset(ResetKind::Power, &mut host.ctx());
        board.input(port::LATCH, Signal::Level(true), &mut host.ctx());
        host.now = 1;
        board.on_event(TIMELINE, &mut host.ctx());
        host.emitted.clear();
        (host, board)
    }

    #[test]
    fn supply_requires_button_or_latch_and_power_can_wake_but_handle_cannot() {
        let (mut host, mut board) = live_ready();
        assert!(board.supply_powered());
        board.set_button("power", true, &mut host.ctx()).unwrap();
        board.input(port::LATCH, Signal::Level(false), &mut host.ctx());
        assert!(board.supply_powered(), "the held physical Power input still supplies the rail");
        assert!(board.power_button_ready(), "a live hold can renew after latch drop");
        board.set_button("power", true, &mut host.ctx()).unwrap();
        board.set_button("power", false, &mut host.ctx()).unwrap();
        assert!(!board.supply_powered());
        assert!(!board.buttons_ready());
        assert!(board.power_button_ready());
        assert!(board.set_button("handle", true, &mut host.ctx()).is_err());
        board.set_button("power", true, &mut host.ctx()).unwrap();
        assert!(board.supply_powered());
        board.begin_supply_boot(&mut host.ctx());
        board.input(port::LATCH, Signal::Level(true), &mut host.ctx());
        assert!(board.held_buttons[0], "power-on cannot erase the user's hold");
        assert_eq!(board.boot_release_at, None, "only the synthetic initial hold auto-releases");
        board.set_button("power", false, &mut host.ctx()).unwrap();
        assert!(board.supply_powered(), "firmware now holds the latch");
    }

    #[test]
    fn live_hold_is_a_level_not_a_timed_pulse() {
        let (mut host, mut board) = live_ready();
        board.set_button("power", true, &mut host.ctx()).unwrap();
        assert!(host.emitted.contains(&(port::BUTTON, Signal::Level(false))));
        assert_eq!(board.button_pulses(), [false; 2]);
        assert!(host.scheduled.is_empty());
        host.emitted.clear();
        host.now = 500;
        board.set_button("power", true, &mut host.ctx()).unwrap();
        assert!(host.emitted.is_empty(), "renewal has no extra press edge");
        board.set_button("power", false, &mut host.ctx()).unwrap();
        assert!(host.emitted.contains(&(port::BUTTON, Signal::Level(true))));
        assert_eq!(host.now, 500, "release never advances simulated time");
    }

    #[test]
    fn live_and_timed_buttons_can_overlap_but_not_on_the_same_pin() {
        let (mut host, mut board) = live_ready();
        board.set_button("power", true, &mut host.ctx()).unwrap();
        assert!(board.pulse_button("power", 120, &mut host.ctx()).is_err());
        board.pulse_button("handle", 120, &mut host.ctx()).unwrap();
        assert!(board.set_button("handle", true, &mut host.ctx()).is_err());
        board.set_button("power", false, &mut host.ctx()).unwrap();
        assert_eq!(board.button_pulses(), [false, true]);
        host.now += 120;
        board.on_event(TIMELINE, &mut host.ctx());
        assert_eq!(board.button_pulses(), [false; 2]);
        assert!(host.emitted.contains(&(port::HANDLE, Signal::Level(true))));
    }

    #[test]
    fn live_release_all_and_reset_clear_holds_and_preserve_boot_recipe() {
        let (mut host, mut board) = live_ready();
        board.set_button("power", true, &mut host.ctx()).unwrap();
        board.set_button("handle", true, &mut host.ctx()).unwrap();
        board.release_button_pulses(&mut host.ctx());
        assert_eq!(board.held_buttons, [false; 2]);
        assert!(host.emitted.contains(&(port::BUTTON, Signal::Level(true))));
        assert!(host.emitted.contains(&(port::HANDLE, Signal::Level(true))));
        board.set_button("handle", true, &mut host.ctx()).unwrap();
        board.reset(ResetKind::Power, &mut host.ctx());
        assert_eq!(board.held_buttons, [false; 2]);
        assert!(board.button_pressed, "the normal boot hold remains intact");
        assert!(!board.handle_pressed);
        assert!(board.set_button("power", true, &mut host.ctx()).is_err());
        board.release_button_pulses(&mut host.ctx());
        assert!(board.button_pressed, "cleanup cannot cancel the boot hold");
    }

    #[test]
    fn live_system_reset_releases_holds_and_existing_timed_pulses() {
        let (mut host, mut board) = live_ready();
        board.set_button("power", true, &mut host.ctx()).unwrap();
        board.pulse_button("handle", 120, &mut host.ctx()).unwrap();
        board.reset(ResetKind::System, &mut host.ctx());
        assert_eq!(board.held_buttons, [false; 2]);
        assert_eq!(board.button_pulses(), [false; 2]);
        assert!(host.emitted.contains(&(port::BUTTON, Signal::Level(true))));
        assert!(host.emitted.contains(&(port::HANDLE, Signal::Level(true))));
    }

    #[test]
    fn live_release_is_allowed_after_firmware_drops_the_power_latch() {
        let (mut host, mut board) = live_ready();
        board.set_button("power", true, &mut host.ctx()).unwrap();
        board.input(port::LATCH, Signal::Level(false), &mut host.ctx());
        board.set_button("power", false, &mut host.ctx()).unwrap();
        assert!(!board.held_buttons[0]);
        assert_eq!(board.power_latched(), Some(false));
        assert!(board.set_button("handle", true, &mut host.ctx()).is_err());
    }

    #[test]
    fn power_on_holds_the_button() {
        let mut host = TestHost::new(72_000_000);
        let mut board = BoardLogic::new(true, 7_200_000);
        board.reset(ResetKind::Power, &mut host.ctx());
        assert_eq!(
            host.emitted,
            [(port::BUTTON, Signal::Level(false)), (port::HANDLE, Signal::Level(true))],
            "power held active low; handle released high"
        );
    }

    #[test]
    fn latch_schedules_the_button_release() {
        let mut host = TestHost::new(72_000_000);
        let mut board = BoardLogic::new(true, 7_200_000);
        board.reset(ResetKind::Power, &mut host.ctx());
        host.now = 1_000;
        board.input(port::LATCH, Signal::Level(true), &mut host.ctx());
        assert_eq!(host.scheduled[&TIMELINE], 7_201_000);
        host.emitted.clear();
        host.now = 7_201_000;
        board.on_event(TIMELINE, &mut host.ctx());
        assert!(host.emitted.contains(&(port::BUTTON, Signal::Level(true))), "released: high");
        assert!(!host.scheduled.contains_key(&TIMELINE));
    }

    #[test]
    fn injections_go_out_on_ble_tx_and_tx_is_captured() {
        let mut host = TestHost::new(72_000_000);
        let mut board = BoardLogic::new(true, 7_200_000);
        board.at(500, Action::Inject(b"VN".to_vec()));
        board.reset(ResetKind::Power, &mut host.ctx());
        host.emitted.clear();
        host.now = 503;
        board.on_event(TIMELINE, &mut host.ctx());
        assert_eq!(
            host.emitted,
            [
                (port::BLE_TX, Signal::Byte { value: 0x56, at: 503 }),
                (port::BLE_TX, Signal::Byte { value: 0x4E, at: 503 })
            ]
        );
        board.input(port::BLE_RX, Signal::Byte { value: 0x55, at: 900 }, &mut host.ctx());
        assert_eq!(board.ble_tx, [(900, 0x55)]);
    }
}
