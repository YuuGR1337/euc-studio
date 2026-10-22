//! The stock firmware's cadence against the owner's wheel, for each instruction timing: the
//! tail-light breathing period (video: 2.1 ± 0.2 s, one cycle, LED mode 1) and the BLE frame-group
//! period (capture `tests/data/begode-extreme/idle-2026-10-15.hex`: 309.6 ms). Both depend on how
//! long the firmware's code takes, so they validate the timing model; see
//! `docs/models/cpu-timing.md`. The wheel stands idle with the owner's settings (LED mode 1).
//! `EUC_CADENCE_SECONDS` changes the simulated time (default 30).
//!
//! ```text
//! cargo test -p euc-armv7m --release --test stock_cadence -- --nocapture
//! ```

use euc_profile::ProfileSource;
use euc_sim::cpu::{Backend, Timing};
use euc_sim::{Machine, MachineConfig, PlantState, Report, WheelFiles};

mod common;

/// The owner's settings preset: LED mode 1, current drive mode at 12 kHz.
const PRESET: &str = "tarik-2026-10-15";

/// Breathing and BLE cadence of one run.
struct Cadence {
    /// Seconds between successive rises of the tail light through its mid level.
    breaths: Vec<f64>,
    /// Milliseconds between successive BLE frame groups after boot.
    groups: Vec<f64>,
    /// Real-time factor of the run.
    speed: f64,
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn cadence(image: &[u8], timing: Timing, seconds: f64) -> Cadence {
    let files = WheelFiles::load(&ProfileSource::Builtin, common::WHEEL).unwrap();
    let board = files.board_with_settings(PRESET).unwrap();
    let hclk = files.hclk();
    let config = MachineConfig {
        board: &board,
        stockmap: Some(&files.stockmap),
        image,
        load_addr: files.profile.board.app.base,
        hclk,
        // As `euc sim run` idles the wheel: 130 V, 25 °C, hall code 5.
        plant: PlantState { battery_v: 130.0, board_temp_c: 25.0, hall: 5, ..PlantState::default() },
        injections: Vec::new(),
        power_cycles: Vec::new(),
        cpi: euc_sim::DEFAULT_CPI,
        exception_timing: timing.exception_timing(),
        max_quantum: 4000,
        marker_window_s: 4.0,
    };
    let mut machine = Machine::new(&config, &euc_sim::registry(), Backend::Own.factory_with(timing).unwrap()).unwrap();
    let step = hclk / 1000;
    let total = (seconds * hclk as f64) as u64;
    let mut levels = Vec::new();
    let started = std::time::Instant::now();
    while machine.cycles < total {
        assert_eq!(machine.run(step), None);
        let leds = euc_sim::recording::outputs(&machine).leds;
        let level: u32 = leds.iter().flatten().map(|&v| u32::from(v)).sum();
        levels.push((machine.cycles as f64 / hclk as f64, level));
    }
    let speed = seconds / started.elapsed().as_secs_f64();

    // Breathing: rising crossings of the mid level once the light has started to breathe.
    let settled: Vec<(f64, u32)> = levels.iter().copied().filter(|&(t, _)| t > 6.0).collect();
    let high = settled.iter().map(|&(_, l)| l).max().unwrap_or(0);
    let low = settled.iter().map(|&(_, l)| l).min().unwrap_or(0);
    let mid = low.midpoint(high);
    let rises: Vec<f64> = settled.windows(2).filter(|w| w[0].1 <= mid && w[1].1 > mid).map(|w| w[1].0).collect();
    let breaths = rises.windows(2).map(|w| w[1] - w[0]).collect();

    // BLE: a frame more than 100 ms after the previous one starts a group.
    let report = Report::new(&machine, &[]);
    let mut starts = Vec::new();
    let mut last = f64::NEG_INFINITY;
    for &t in &report.ble.frame_times_ms {
        if t - last > 100.0 {
            starts.push(t);
        }
        last = t;
    }
    let groups = starts.windows(2).skip(1).map(|w| w[1] - w[0]).collect();
    Cadence { breaths, groups, speed }
}

/// Both timings are measured and printed; the shipped one must stay inside the owner's measurements:
/// the video's 2.1 ± 0.2 s and 1 % around the captured 309.6 ms.
#[test]
fn breathing_and_ble_cadence_against_the_owners_wheel() {
    let Some((_, image)) = common::stock() else { return };
    let seconds = common::seconds("EUC_CADENCE_SECONDS", 30.0);
    for timing in [Timing::Fixed, Timing::Flash { wait_states: 2 }] {
        let c = cadence(&image, timing, seconds);
        let (min, max) = c.breaths.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &b| (lo.min(b), hi.max(b)));
        let (breath, group) = (mean(&c.breaths), mean(&c.groups));
        eprintln!(
            "{timing}: breathing {breath:.3} s mean over {} periods ({min:.3}-{max:.3} s); BLE group period {group:.3} ms \
             over {} groups; {:.2}x real time",
            c.breaths.len(),
            c.groups.len(),
            c.speed
        );
        assert!(c.breaths.len() >= 5, "{timing}: the tail light breathes");
        assert!(c.groups.len() >= 10, "{timing}: telemetry runs");
        if timing == euc_sim::cpu::DEFAULT_TIMING && seconds >= 20.0 {
            assert!((1.9..=2.3).contains(&breath), "{timing}: breathing {breath:.3} s, video 2.1 ± 0.2 s");
            assert!((306.5..=312.7).contains(&group), "{timing}: BLE groups every {group:.3} ms, capture 309.6 ms");
        }
    }
}
