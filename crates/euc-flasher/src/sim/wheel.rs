//! The simulated wheel: application behaviour, the emulated bootloader, power, and the flash.
//!
//! Behaviour only, written from docs/protocol/begode.md and this project's plan. The vendor
//! bootloader is not public; its model here is an assumption (docs/spec/flasher.md, "Emulated
//! bootloader"): `!` `@` makes the application erase its request page and reset; with the request
//! page erased, or without a committed application, the bootloader polls with `C` once a second,
//! takes one file by YMODEM-1K with CRC-16, checks it with this project's image gate, writes it
//! through `euc-flash-sim`, and boots it.

use euc_flash_sim::{BootState, FlashSim, Options, Outcome as FlashOutcome};
use euc_image::Image;
use euc_profile::{Sha256, WheelProfile};
use euc_proto_begode::{Alerts, Bms, Cells, Live, LiveSettings, Modes, Pack, Power, Status, Telemetry};
use euc_ymodem::{CAN, ReceiveStatus, Receiver, ReceiverConfig};
use serde::Serialize;

use super::faults::{AppFirmware, FaultPlan, WheelScript};
use super::images::geometry;

/// Microseconds on the simulator's clock.
pub type Micros = u64;

const MS: Micros = 1_000;
const S: Micros = 1_000_000;
/// One frame group every 151 ticks of 2.016 ms.
const GROUP_PERIOD: Micros = 304 * MS;
/// `!` then `@` needs at least 100 ticks.
const SERVICE_GAP: Micros = 202 * MS;
/// From reset to the first frame group (the stock firmware's measured 2.00 s).
const FIRST_FRAME: Micros = 2 * S;
/// Ramp-down, page erase and the 200 ms delay before the reset after `!` `@`.
const ENTRY_RESET: Micros = 1_200 * MS;
/// Reset to bootloader decision.
const BOOT: Micros = 100 * MS;
/// The bootloader's poll period while it waits for a header.
const POLL: Micros = S;
/// The bootloader's receive timeout once data flows (longer than a 1 KiB block over BLE).
const RX_TIMEOUT: Micros = 2 * S;
/// After the end of file, how long the bootloader waits for the batch close before it commits.
const CLOSE_WAIT: Micros = 3 * S;
/// The clean firmware's window for the power-button press.
const PRESS_WINDOW: Micros = 10 * S;

/// What runs on the wheel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WheelState {
    /// No power.
    Off,
    /// Between reset and the bootloader's decision.
    Booting,
    /// An application with this committed image.
    App {
        /// The image.
        sha256: Sha256,
    },
    /// The bootloader.
    Bootloader,
}

/// What the wheel saw, for the campaign's checks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct WheelStats {
    /// Command bytes other than `V`, `N`, `!`, `@` that reached the application (must stay 0).
    pub unexpected_commands: u32,
    /// Service sequences the application acted on (`!@`; `cy` or `+-` must never appear).
    pub service_sequences: Vec<String>,
    /// Outcome of each bootloader commit.
    pub commits: Vec<String>,
    /// Bootloader sessions started.
    pub bootloader_sessions: u32,
    /// Power-ons.
    pub power_ons: u32,
}

#[derive(Debug)]
struct App {
    sha256: Sha256,
    next_group: Micros,
    group: u32,
    pause: u8,
    slot: Option<u8>,
    last: Option<(u8, Micros)>,
    bang: Option<Micros>,
    /// Clean firmware: waiting for the power-button press until this time.
    press_until: Option<Micros>,
    /// Service replies, sent at the next group time.
    text: Vec<u8>,
    reset_at: Option<Micros>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitEnd {
    Boot,
    PowerLoss,
    Stay,
}

#[derive(Debug)]
struct Boot {
    receiver: Receiver,
    next_poll: Micros,
    last_rx: Micros,
    complete_at: Option<Micros>,
    restart_at: Option<Micros>,
    commit: Option<(Micros, CommitEnd)>,
}

#[derive(Debug)]
enum Mode {
    Off,
    Booting { until: Micros },
    App(Box<App>),
    Bootloader(Box<Boot>),
}

/// The wheel: controller, BLE module UART side, flash and power.
#[derive(Debug)]
pub struct SimWheel {
    profile: WheelProfile,
    flash: FlashSim,
    /// The application erased its request page (`!` `@`): the bootloader stays.
    request: bool,
    mode: Mode,
    script: WheelScript,
    plan: FaultPlan,
    commits: usize,
    slow_bootloader: Option<Micros>,
    model: String,
    out: Vec<u8>,
    stats: WheelStats,
}

impl SimWheel {
    /// A powered wheel running `factory` (installed through the flash model without faults).
    pub fn new(profile: WheelProfile, factory: &Image, script: WheelScript, plan: &FaultPlan) -> Result<Self, String> {
        let mut flash = FlashSim::new(profile.clone(), geometry()).map_err(|e| e.to_string())?;
        let report = flash.update(factory, Options::default(), &[]);
        if report.outcome != FlashOutcome::Booted {
            return Err(format!("the factory image does not install: {:?}", report.outcome));
        }
        let model = crate::gate::model_name(&profile);
        let mut wheel = Self {
            profile,
            flash,
            request: false,
            mode: Mode::Off,
            script,
            plan: plan.clone(),
            commits: 0,
            slow_bootloader: plan.slow_bootloader.map(|d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX)),
            model,
            out: Vec::new(),
            stats: WheelStats::default(),
        };
        // Already running when the session starts: the first group is due soon.
        if let BootState::Application { sha256, .. } = wheel.flash.state().clone() {
            wheel.mode = Mode::App(Box::new(App::new(sha256, 100 * MS)));
        }
        Ok(wheel)
    }

    /// What runs now.
    pub fn state(&self) -> WheelState {
        match &self.mode {
            Mode::Off => WheelState::Off,
            Mode::Booting { .. } => WheelState::Booting,
            Mode::App(app) => WheelState::App { sha256: app.sha256 },
            Mode::Bootloader(_) => WheelState::Bootloader,
        }
    }

    /// The flash model.
    pub const fn flash(&self) -> &FlashSim {
        &self.flash
    }

    /// The profile the wheel runs with.
    pub const fn profile(&self) -> &WheelProfile {
        &self.profile
    }

    /// Counters.
    pub const fn stats(&self) -> &WheelStats {
        &self.stats
    }

    /// True while powered.
    pub const fn powered(&self) -> bool {
        !matches!(self.mode, Mode::Off)
    }

    /// Replaces the fault plan (the campaign clears it before probing recovery).
    pub fn set_faults(&mut self, plan: &FaultPlan) {
        self.plan = plan.clone();
        self.slow_bootloader = None;
    }

    /// Cuts the power.
    pub fn power_off(&mut self) {
        self.mode = Mode::Off;
        self.out.clear();
    }

    /// Switches on at `now`.
    pub fn power_on(&mut self, now: Micros) {
        if !self.powered() {
            self.stats.power_ons += 1;
            self.mode = Mode::Booting { until: now + BOOT };
        }
    }

    /// UART output since the last call.
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    /// The next time the wheel does something on its own.
    pub fn next_wake(&self) -> Option<Micros> {
        match &self.mode {
            Mode::Off => None,
            Mode::Booting { until } => Some(*until),
            Mode::App(app) => [Some(app.next_group), app.reset_at, app.press_until].into_iter().flatten().min(),
            Mode::Bootloader(boot) => {
                if let Some((at, _)) = boot.commit {
                    return Some(at);
                }
                if let Some(at) = boot.restart_at {
                    return Some(at);
                }
                let mut next = match boot.receiver.status() {
                    ReceiveStatus::Idle => boot.next_poll,
                    _ => boot.last_rx + RX_TIMEOUT,
                };
                if let Some(at) = boot.complete_at {
                    next = next.min(at + CLOSE_WAIT);
                }
                Some(next)
            }
        }
    }

    /// Runs what is due at `now`.
    pub fn wake(&mut self, now: Micros) {
        match &mut self.mode {
            Mode::Off => {}
            Mode::Booting { until } => {
                if *until <= now {
                    self.boot(now);
                }
            }
            Mode::App(_) => self.app_wake(now),
            Mode::Bootloader(_) => self.boot_wake(now),
        }
    }

    /// Bytes from the BLE module's UART at `now`.
    pub fn receive(&mut self, now: Micros, bytes: &[u8]) {
        match &mut self.mode {
            Mode::App(_) => {
                for &byte in bytes {
                    self.app_byte(now, byte);
                }
            }
            Mode::Bootloader(boot) => {
                if boot.commit.is_some() || boot.restart_at.is_some() {
                    return;
                }
                let reply = boot.receiver.receive(bytes);
                boot.last_rx = now;
                self.out.extend(reply);
                self.after_receive(now);
            }
            Mode::Off | Mode::Booting { .. } => {}
        }
    }

    /// A short press of the power button.
    pub fn press_button(&mut self, now: Micros) {
        let Mode::App(app) = &mut self.mode else { return };
        if app.press_until.is_some_and(|until| now <= until) {
            app.press_until = None;
            app.text.extend_from_slice(b"SERVICE:OK\r\n");
            app.reset_at = Some(now + ENTRY_RESET);
            self.stats.service_sequences.push("!@".into());
        }
    }

    // ----- reset and boot ------------------------------------------------------------------

    fn boot(&mut self, now: Micros) {
        // A reset re-checks the committed application exactly as a power cycle does.
        let state = self.flash.power_cycle();
        match state {
            BootState::Application { sha256, .. } if !self.request => {
                self.mode = Mode::App(Box::new(App::new(sha256, now + FIRST_FRAME - BOOT)));
            }
            _ => {
                self.stats.bootloader_sessions += 1;
                let delay = self.slow_bootloader.take().unwrap_or(BOOT);
                self.mode = Mode::Bootloader(Box::new(Boot::new(now + delay)));
            }
        }
    }

    // ----- the application -----------------------------------------------------------------

    fn speed(&self, now: Micros) -> i16 {
        let moving = self.script.moving.is_some_and(|(from, to)| {
            let (from, to) = (micros(from), micros(to));
            (from..to).contains(&now)
        });
        if moving { 150 } else { 0 }
    }

    fn app_byte(&mut self, now: Micros, byte: u8) {
        let still = self.speed(now) == 0;
        let firmware = self.script.firmware;
        let ignores = self.script.ignores_entry;
        let Mode::App(app) = &mut self.mode else { return };
        if COMMANDS.contains(&byte) && !b"VN!@".contains(&byte) {
            self.stats.unexpected_commands += 1;
        }
        // Every byte pauses telemetry for two group times and takes the command slot.
        app.pause = 2;
        app.slot = Some(byte);
        let previous = app.last.replace((byte, now));
        let after = |first: u8, gap: Micros| previous.is_some_and(|(b, at)| b == first && now - at >= gap);
        if (byte == b'y' && after(b'c', 101 * MS)) || (byte == b'-' && after(b'+', 101 * MS)) {
            let pair = if byte == b'y' { "cy" } else { "+-" };
            self.stats.service_sequences.push(pair.into());
        }
        match byte {
            b'!' => app.bang = still.then_some(now),
            b'@' => {
                let armed = app.bang.take().is_some_and(|at| now - at >= SERVICE_GAP) && after(b'!', SERVICE_GAP);
                if armed && still && !ignores {
                    match firmware {
                        AppFirmware::Stock => {
                            app.reset_at = Some(now + ENTRY_RESET);
                            self.stats.service_sequences.push("!@".into());
                        }
                        AppFirmware::Clean => {
                            app.press_until = Some(now + PRESS_WINDOW);
                            app.text.extend_from_slice(b"SERVICE:CONFIRM\r\n");
                        }
                    }
                }
            }
            _ => app.bang = None,
        }
    }

    fn app_wake(&mut self, now: Micros) {
        let speed = self.speed(now);
        let (battery_v, board_c, alerts) = (self.script.battery_v, self.script.board_c, self.script.alerts);
        let Mode::App(app) = &mut self.mode else { return };
        if app.reset_at.is_some_and(|at| at <= now) {
            // The application erases its request page and resets.
            self.request = true;
            self.mode = Mode::Booting { until: now + BOOT };
            return;
        }
        if app.press_until.is_some_and(|until| until <= now) {
            app.press_until = None;
            app.text.extend_from_slice(b"SERVICE:DENIED\r\n");
        }
        if app.next_group <= now {
            app.next_group += GROUP_PERIOD;
            self.out.append(&mut app.text);
            if app.pause > 0 {
                app.pause -= 1;
            } else {
                match app.slot.take() {
                    Some(b'V') => self.out.extend_from_slice(b"GW1825020"),
                    Some(b'N') => self.out.extend_from_slice(format!("NAME:{}\r\n", self.model).as_bytes()),
                    _ => {
                        let group = app.group;
                        app.group += 1;
                        self.out.extend(frame_group(group, speed, battery_v, board_c, alerts));
                    }
                }
            }
        }
    }

    // ----- the bootloader ------------------------------------------------------------------

    fn after_receive(&mut self, now: Micros) {
        let Mode::Bootloader(boot) = &mut self.mode else { return };
        match boot.receiver.status() {
            ReceiveStatus::Done { .. } => self.start_commit(now),
            ReceiveStatus::Failed(_) => boot.restart_at = Some(now + S),
            ReceiveStatus::Idle | ReceiveStatus::Receiving => {
                if boot.receiver.file().is_some() && boot.complete_at.is_none() {
                    boot.complete_at = Some(now);
                }
            }
        }
    }

    fn boot_wake(&mut self, now: Micros) {
        let Mode::Bootloader(boot) = &mut self.mode else { return };
        if let Some((at, end)) = boot.commit {
            if at <= now {
                self.finish_commit(now, end);
            }
            return;
        }
        if boot.restart_at.is_some_and(|at| at <= now) {
            **boot = Boot::new(now);
            return;
        }
        if boot.complete_at.is_some_and(|at| at + CLOSE_WAIT <= now) {
            self.start_commit(now);
            return;
        }
        match boot.receiver.status() {
            ReceiveStatus::Idle if boot.next_poll <= now => {
                let poll = boot.receiver.timeout();
                self.out.extend(poll);
                boot.next_poll = now + POLL;
            }
            ReceiveStatus::Receiving if boot.last_rx + RX_TIMEOUT <= now => {
                let reply = boot.receiver.timeout();
                self.out.extend(reply);
                boot.last_rx = now;
                self.after_receive(now);
            }
            _ => {}
        }
    }

    fn start_commit(&mut self, now: Micros) {
        let Mode::Bootloader(boot) = &mut self.mode else { return };
        let Some(file) = boot.receiver.file() else {
            boot.restart_at = Some(now + S);
            return;
        };
        let faults = self.plan.flash_faults(self.commits);
        self.commits += 1;
        let report = self.flash.update(&Image::raw(file.data), Options { chunk_size: 1024, max_attempts: 3 }, &faults);
        // Page erase about 20 ms, 1 KiB program about 25 ms (STM32F1 class), plus the checks.
        let duration = 50 * MS + report.attempts.erase as u64 * 20 * MS + report.attempts.write as u64 * 25 * MS;
        let end = match report.outcome {
            FlashOutcome::Booted => CommitEnd::Boot,
            FlashOutcome::Interrupted { .. } => CommitEnd::PowerLoss,
            _ => CommitEnd::Stay,
        };
        self.stats.commits.push(outcome_name(&report.outcome).into());
        boot.commit = Some((now + duration, end));
    }

    fn finish_commit(&mut self, now: Micros, end: CommitEnd) {
        match end {
            CommitEnd::Boot => {
                self.request = false;
                self.mode = Mode::Booting { until: now + BOOT };
            }
            CommitEnd::PowerLoss => self.power_off(),
            CommitEnd::Stay => {
                // The image was refused: say so and wait for another transfer.
                self.out.extend_from_slice(&[CAN, CAN]);
                if let Mode::Bootloader(boot) = &mut self.mode {
                    **boot = Boot::new(now + S);
                }
            }
        }
    }
}

impl App {
    fn new(sha256: Sha256, first_group: Micros) -> Self {
        Self {
            sha256,
            next_group: first_group,
            group: 0,
            pause: 0,
            slot: None,
            last: None,
            bang: None,
            press_until: None,
            text: Vec::new(),
            reset_at: None,
        }
    }
}

impl Boot {
    fn new(first_poll: Micros) -> Self {
        let config = ReceiverConfig { max_size: 49_152, require_size: true, max_errors: 10 };
        Self {
            receiver: Receiver::new(config),
            next_poll: first_poll,
            last_rx: first_poll,
            complete_at: None,
            restart_at: None,
            commit: None,
        }
    }
}

/// Every byte the stock command parser acts on (docs/protocol/begode.md, "Commands").
const COMMANDS: &[u8] = b"bQEThfsouiI<=>gmex\"VNkpW+-cy!@";

fn micros(duration: std::time::Duration) -> Micros {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn outcome_name(outcome: &FlashOutcome) -> &'static str {
    match outcome {
        FlashOutcome::Booted => "booted",
        FlashOutcome::Rejected { .. } => "rejected",
        FlashOutcome::Interrupted { .. } => "interrupted",
        FlashOutcome::RetryLimit { .. } => "retry-limit",
        FlashOutcome::VerificationFailed { .. } => "verification-failed",
    }
}

/// One telemetry group: frames 00, 01(n), 02/03(n), 04, 07.
fn frame_group(group: u32, speed: i16, battery_v: f64, board_c: f64, alerts: u8) -> Vec<u8> {
    let n = (group % 4) as u8;
    let pack = if (group / 4).is_multiple_of(2) { Pack::A } else { Pack::B };
    let frames = [
        Telemetry::Live(Live {
            voltage: (battery_v * 50.0).round().clamp(0.0, 6720.0) as u16,
            speed,
            range: 117,
            trip: 6,
            phase_current: 90,
            temperature: ((board_c - 21.0) * 334.0 - 5280.0).round() as i16,
            settings: LiveSettings(0x1529),
            beeper_volume: 9,
            sub: 0x18,
        }),
        Telemetry::Bms(Bms {
            pwm_alarm: 80,
            speed_copy: 0,
            voltage: (battery_v * 10.0).round() as u16,
            current: 0,
            temp_max: 23,
            temp_min: 22,
            word42: 651,
            status: 0x7D01,
            block: n,
        }),
        Telemetry::Cells(Cells { pack, page: n, millivolts: [4_070; 8] }),
        Telemetry::Status(Status {
            odometer: 1_468_670,
            modes: Modes(0x4800),
            auto_off: 7_000,
            tiltback: 69,
            byte12: 0,
            wm: 1,
            alerts: Alerts(alerts),
            light: 0,
            tag: 0x0012,
            sub: 0x18,
        }),
        Telemetry::Power(Power {
            battery_current: 0,
            settings: 0x55,
            motor_temperature: 0,
            pwm: 0,
            tail: [0; 8],
            sub: 0x18,
        }),
    ];
    frames.iter().flat_map(|frame| frame.encode().as_bytes().to_vec()).collect()
}
