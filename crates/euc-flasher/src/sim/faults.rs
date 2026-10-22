//! Injected faults and the scripts of the simulated wheel and its rider-less operator.

use std::time::Duration;

use euc_flash_sim::Fault as FlashFault;

/// `SplitMix64`: small, seedable, reproducible.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator from `seed`.
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (`n` > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }

    /// Uniform in `low..high`.
    pub fn range(&mut self, low: u64, high: u64) -> u64 {
        low + self.below(high.saturating_sub(low))
    }

    /// True with probability `per_mille` / 1000.
    pub fn chance(&mut self, per_mille: u64) -> bool {
        self.below(1000) < per_mille
    }
}

/// Faults on the link and in the wheel. Indices count BLE writes (phone to wheel) and
/// notifications (wheel to phone) from the start of the run, zero-based.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FaultPlan {
    /// Writes that never arrive.
    pub drop_writes: Vec<u64>,
    /// Writes with one byte changed.
    pub corrupt_writes: Vec<u64>,
    /// Notifications that never arrive.
    pub drop_notifications: Vec<u64>,
    /// Notifications with one byte changed.
    pub corrupt_notifications: Vec<u64>,
    /// Notifications held back this much longer (and everything behind them: BLE keeps order).
    pub stalls: Vec<(u64, Duration)>,
    /// At this write the link drops for this long.
    pub link_losses: Vec<(u64, Duration)>,
    /// At this write the wheel loses power (it stays off until the operator switches it on).
    pub power_losses: Vec<u64>,
    /// Flash faults for the bootloader's n-th commit (zero-based): `Interrupt` is a power loss
    /// mid-erase or mid-write, `Transient` an operation retried, `CorruptWrite` a bad cell.
    pub flash: Vec<(usize, Vec<FlashFault>)>,
    /// The n-th YMODEM data block (one-based, counted over the run) arrives with other bytes and
    /// a matching CRC: a wrong image that YMODEM cannot see.
    pub wrong_block: Option<u64>,
    /// The bootloader starts polling this much later than usual.
    pub slow_bootloader: Option<Duration>,
}

impl FaultPlan {
    /// No faults.
    pub fn none() -> Self {
        Self::default()
    }

    /// True when nothing is injected.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The flash faults for commit `index`.
    pub fn flash_faults(&self, index: usize) -> Vec<FlashFault> {
        self.flash.iter().filter(|(at, _)| *at == index).flat_map(|(_, faults)| faults.iter().copied()).collect()
    }
}

/// The application firmware the simulated wheel runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppFirmware {
    /// Stock behaviour: `!` `@` at standstill enters the bootloader at once.
    Stock,
    /// Our clean profile: `!` `@` waits for a power-button press within 10 s.
    Clean,
}

/// How the simulated wheel behaves around the flash.
#[derive(Debug, Clone, PartialEq)]
pub struct WheelScript {
    /// The application's service-sequence behaviour.
    pub firmware: AppFirmware,
    /// The wheel rolls (speed 1.5 m/s) during this window from the start.
    pub moving: Option<(Duration, Duration)>,
    /// Battery voltage.
    pub battery_v: f64,
    /// Board temperature.
    pub board_c: f64,
    /// Alert flags in frame 04.
    pub alerts: u8,
    /// The application ignores `!` `@` (as if the wheel were never still enough).
    pub ignores_entry: bool,
}

impl Default for WheelScript {
    fn default() -> Self {
        Self {
            firmware: AppFirmware::Stock,
            moving: None,
            battery_v: 129.7,
            board_c: 25.3,
            alerts: 0,
            ignores_entry: false,
        }
    }
}

/// How the simulated person at the wheel answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorScript {
    /// What they type at gate 6; `None` types the expected text.
    pub types: Option<String>,
    /// They press the power button when asked.
    pub presses_button: bool,
    /// How long they take to press it.
    pub press_delay: Duration,
    /// They switch the wheel off and on when asked.
    pub power_cycles: bool,
    /// How long that takes.
    pub power_cycle_delay: Duration,
}

impl Default for OperatorScript {
    fn default() -> Self {
        Self {
            types: None,
            presses_button: true,
            press_delay: Duration::from_secs(2),
            power_cycles: true,
            power_cycle_delay: Duration::from_secs(5),
        }
    }
}
