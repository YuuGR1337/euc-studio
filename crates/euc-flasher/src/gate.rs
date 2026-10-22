//! Gates 1-7 and the recovery-image gate. Every gate returns a [`GateCheck`] for the log.

use std::time::Duration;

use euc_image::{Gate as ImageGate, Identity, Image, Report, validate};
use euc_profile::{Origin, Sha256, WheelProfile};
use euc_proto_begode::{Reply, Telemetry};
use serde::Serialize;

use crate::ledger::Ledger;
use crate::observe::Observer;

/// Where an image is going.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    /// The simulated wheel. Unsigned sealed development images are allowed; gate 3 is this run.
    Simulator,
    /// A real wheel. Stock pins must come from the built-in profiles; our images need a verified
    /// signature (not implemented yet, so they are refused); gate 3 needs a complete dry-run.
    Wheel,
}

/// The gates, numbered as in the plan and SAFETY.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateId {
    /// 1: size, vectors, load address, info block.
    Image,
    /// 2: pinned stock build or our sealed (and, for a real wheel, signed) image.
    Provenance,
    /// 3: a simulator dry-run of the same sha256.
    DryRun,
    /// 4: Begode frames, `V` and `N` answers, a 32S pack voltage.
    Identity,
    /// 5: standing still for 5 s, no alarms, battery and temperature limits, a stable link.
    WheelState,
    /// 6: the typed confirmation.
    Confirmation,
    /// 7: the first flash of a wheel is a stock image.
    FirstFlashStock,
    /// Not numbered: the stock image for recovery is at hand and valid.
    RecoveryImage,
}

impl GateId {
    /// The plan's number, if the gate has one.
    pub const fn number(self) -> Option<u8> {
        match self {
            Self::Image => Some(1),
            Self::Provenance => Some(2),
            Self::DryRun => Some(3),
            Self::Identity => Some(4),
            Self::WheelState => Some(5),
            Self::Confirmation => Some(6),
            Self::FirstFlashStock => Some(7),
            Self::RecoveryImage => None,
        }
    }
}

/// Pass, fail, or not applicable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Passed.
    Pass,
    /// Failed: the flash stops here.
    Fail,
    /// Not applicable to this target.
    Skip,
}

/// One gate result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateCheck {
    /// Which gate.
    pub gate: GateId,
    /// Its number in the plan.
    pub number: Option<u8>,
    /// Result.
    pub status: Status,
    /// What was found.
    pub message: String,
}

impl GateCheck {
    fn new(gate: GateId, status: Status, message: impl Into<String>) -> Self {
        Self { gate, number: gate.number(), status, message: message.into() }
    }

    /// True unless failed.
    pub fn passed(&self) -> bool {
        self.status != Status::Fail
    }
}

/// Wheel limits for gates 4 and 5. The defaults are the Begode Extreme's (32S pack).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Cells in series.
    pub series_cells: u32,
    /// Lowest plausible cell voltage for the identity check.
    pub min_cell_v: f64,
    /// Highest cell voltage; the firmware clamps the frame value at 32 × 4.2 V.
    pub max_cell_v: f64,
    /// Lowest battery voltage to flash at.
    pub min_battery_v: f64,
    /// Board temperature must stay below this.
    pub max_board_c: f64,
    /// How long the wheel must stand still.
    pub still: Duration,
    /// Longest gap between live frames on a stable link.
    pub max_frame_gap: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            series_cells: 32,
            min_cell_v: 3.0,
            max_cell_v: 4.2,
            min_battery_v: 118.0,
            max_board_c: 60.0,
            still: Duration::from_secs(5),
            max_frame_gap: Duration::from_millis(1500),
        }
    }
}

/// The text gate 6 asks for: `FLASH` and the model name the wheel reports (`FLASH EXTREME`).
pub fn confirmation_text(wheel: &WheelProfile) -> String {
    format!("FLASH {}", model_name(wheel))
}

/// The name a Begode wheel answers to `N` with: the profile name without the brand, in capitals.
pub fn model_name(wheel: &WheelProfile) -> String {
    let name = wheel.name.strip_prefix(&wheel.brand).unwrap_or(&wheel.name);
    name.trim().to_uppercase()
}

/// Gate 1: the image's structure (every image-gate step except provenance).
pub fn image(image: &Image, wheel: &WheelProfile) -> (GateCheck, Report) {
    let report = validate(image, wheel);
    let failed: Vec<String> = report
        .checks
        .iter()
        .filter(|check| check.gate != ImageGate::Provenance && check.outcome.is_fail())
        .map(|check| format!("{}: {}", check.gate, check.outcome))
        .collect();
    let check = if failed.is_empty() {
        let info = match report.checks.iter().find(|check| check.gate == ImageGate::InfoBlock) {
            Some(check) if matches!(check.outcome, euc_image::Outcome::Pass(_)) => "info block valid",
            _ => "no info block",
        };
        GateCheck::new(
            GateId::Image,
            Status::Pass,
            format!("{} bytes at 0x{:08X}, vectors inside the image, {info}", report.size, report.load_addr),
        )
    } else {
        GateCheck::new(GateId::Image, Status::Fail, failed.join("; "))
    };
    (check, report)
}

/// Gate 2: where the image comes from.
pub fn provenance(report: &Report, target: Target) -> GateCheck {
    let gate = GateId::Provenance;
    match (&report.identity, target) {
        (Identity::Stock { label, pin_source, .. }, Target::Wheel) if *pin_source != Origin::Builtin => GateCheck::new(
            gate,
            Status::Fail,
            format!("stock build {label} is pinned only by {pin_source}; a real wheel needs a built-in pin"),
        ),
        (Identity::Stock { label, pin_source, .. }, _) => {
            GateCheck::new(gate, Status::Pass, format!("pinned stock build {label} (pin from {pin_source})"))
        }
        (Identity::Ours { info }, _) if !info.is_sealed() => {
            GateCheck::new(gate, Status::Fail, "our image is not sealed; run `euc image seal` first")
        }
        (Identity::Ours { .. }, Target::Wheel) => GateCheck::new(
            gate,
            Status::Fail,
            "our images need a verified Ed25519 signature for a real wheel; signing and verification are not implemented yet",
        ),
        (Identity::Ours { info }, Target::Simulator) => GateCheck::new(
            gate,
            Status::Pass,
            format!(
                "our sealed {} build {} ({}); unsigned, accepted for the simulator only",
                info.profile, info.version, info.git_sha
            ),
        ),
        (Identity::Unknown, _) => GateCheck::new(gate, Status::Fail, "unknown image: neither pinned stock nor ours"),
    }
}

/// Gate 3: a complete simulator dry-run of the same sha256 on this machine.
pub fn dry_run(sha256: &Sha256, wheel: &str, target: Target, ledger: &dyn Ledger) -> GateCheck {
    let gate = GateId::DryRun;
    if target == Target::Simulator {
        return GateCheck::new(gate, Status::Skip, "this run is the simulator dry-run");
    }
    match ledger.dry_run(sha256, wheel) {
        Some(run) if run.complete() => GateCheck::new(gate, Status::Pass, "dry-run complete: flash, re-entry, ride"),
        Some(run) if run.ride.is_none() => {
            GateCheck::new(gate, Status::Fail, "dry-run lacks the 30 s emulator ride, which is not implemented yet")
        }
        Some(_) => GateCheck::new(gate, Status::Fail, "the dry-run of this image did not pass"),
        None => GateCheck::new(gate, Status::Fail, "no dry-run of this image; run `euc flash --sim` first"),
    }
}

/// Gate 7: the first flash of a wheel is the stock image.
pub fn first_flash_stock(report: &Report, peer: &str, ledger: &dyn Ledger) -> GateCheck {
    let gate = GateId::FirstFlashStock;
    if matches!(report.identity, Identity::Stock { .. }) {
        return GateCheck::new(gate, Status::Pass, "the image is stock");
    }
    if ledger.history(peer).iter().any(|record| record.stock) {
        GateCheck::new(gate, Status::Pass, "a stock flash of this wheel succeeded before")
    } else {
        GateCheck::new(gate, Status::Fail, "this wheel has no successful stock flash yet; flash the stock image first")
    }
}

/// The recovery image must be a valid pinned stock build (built-in pin for a real wheel).
pub fn recovery_image(recovery: &Image, wheel: &WheelProfile, target: Target) -> (GateCheck, Report) {
    let gate = GateId::RecoveryImage;
    let report = validate(recovery, wheel);
    let check = match &report.identity {
        _ if !report.passed => GateCheck::new(gate, Status::Fail, "the recovery image fails the image gate"),
        Identity::Stock { pin_source, .. } if target == Target::Wheel && *pin_source != Origin::Builtin => {
            GateCheck::new(gate, Status::Fail, "the recovery image needs a built-in stock pin")
        }
        Identity::Stock { label, .. } => GateCheck::new(gate, Status::Pass, format!("stock build {label} ready")),
        _ => GateCheck::new(gate, Status::Fail, "the recovery image must be a pinned stock build"),
    };
    (check, report)
}

/// Gate 4: the wheel identifies as the profile's model, from frames and replies after `since`.
pub(crate) fn identity(observer: &Observer, since: Duration, expected: &str, limits: &Limits) -> GateCheck {
    let gate = GateId::Identity;
    let frames: Vec<&Telemetry> = observer.frames_since(since).map(|(_, frame)| frame).collect();
    let live: Vec<f64> = frames
        .iter()
        .filter_map(|frame| if let Telemetry::Live(l) = frame { Some(l.voltage_v()) } else { None })
        .collect();
    let has_status = frames.iter().any(|frame| matches!(frame, Telemetry::Status(_)));
    if live.is_empty() || !has_status {
        return GateCheck::new(gate, Status::Fail, format!("{} frames, no live and status frames", frames.len()));
    }
    let banner = observer.reply_since(since, |reply| match reply {
        Reply::Version(banner) => Some(banner.clone()),
        _ => None,
    });
    let Some(banner) = banner else { return GateCheck::new(gate, Status::Fail, "no answer to V") };
    let name = observer.reply_since(since, |reply| match reply {
        Reply::Name(name) => Some(name.clone()),
        _ => None,
    });
    let Some(name) = name else { return GateCheck::new(gate, Status::Fail, "no answer to N") };
    if name != expected {
        return GateCheck::new(gate, Status::Fail, format!("the wheel says it is {name}, not {expected}"));
    }
    let cells = f64::from(limits.series_cells);
    let (low, high) = (cells * limits.min_cell_v, cells * limits.max_cell_v);
    let voltage = live.iter().copied().fold(f64::NAN, f64::max);
    if !(low..=high + 0.01).contains(&voltage) {
        return GateCheck::new(
            gate,
            Status::Fail,
            format!("{voltage:.2} V is outside {low:.1}..{high:.1} V of a {}S pack", limits.series_cells),
        );
    }
    GateCheck::new(
        gate,
        Status::Pass,
        format!("{} frames, banner {banner}, NAME:{name}, {voltage:.2} V ({}S)", frames.len(), limits.series_cells),
    )
}

/// Gate 5: the wheel's state over `from..to`, which must span `limits.still`.
pub(crate) fn wheel_state(observer: &Observer, from: Duration, to: Duration, limits: &Limits) -> GateCheck {
    let gate = GateId::WheelState;
    let mut live_times = Vec::new();
    let (mut moving, mut alerts, mut statuses) = (0, 0_u8, 0);
    let (mut min_v, mut max_c) = (f64::INFINITY, f64::NEG_INFINITY);
    for (at, frame) in observer.frames_since(from).filter(|(at, _)| *at <= to) {
        match frame {
            Telemetry::Live(live) => {
                live_times.push(*at);
                moving += usize::from(live.speed != 0);
                min_v = min_v.min(live.voltage_v());
                max_c = max_c.max(live.temperature_c());
            }
            Telemetry::Status(status) => {
                statuses += 1;
                alerts |= status.alerts.0;
            }
            _ => {}
        }
    }
    let mut gaps = live_times.windows(2).map(|pair| pair[1].saturating_sub(pair[0])).collect::<Vec<_>>();
    if let (Some(first), Some(last)) = (live_times.first(), live_times.last()) {
        gaps.push(first.saturating_sub(from));
        gaps.push(to.saturating_sub(*last));
    }
    let max_gap = gaps.into_iter().max().unwrap_or(to.saturating_sub(from));
    let span = to.saturating_sub(from);
    let problem = if span < limits.still {
        Some(format!("watched {:.1} s; {:.1} s needed", span.as_secs_f64(), limits.still.as_secs_f64()))
    } else if live_times.len() < 4 || statuses == 0 {
        Some(format!("{} live and {statuses} status frames: the link is not stable", live_times.len()))
    } else if max_gap > limits.max_frame_gap {
        Some(format!("a {:.2} s gap between frames: the link is not stable", max_gap.as_secs_f64()))
    } else if moving > 0 {
        Some(format!("the wheel moved in {moving} frames; it must stand still"))
    } else if alerts != 0 {
        Some(format!("alert flags 0x{alerts:02X} are set"))
    } else if min_v < limits.min_battery_v {
        Some(format!("battery {min_v:.2} V is below {:.1} V", limits.min_battery_v))
    } else if max_c >= limits.max_board_c {
        Some(format!("board {max_c:.1} °C is not below {:.0} °C", limits.max_board_c))
    } else {
        None
    };
    match problem {
        Some(problem) => GateCheck::new(gate, Status::Fail, problem),
        None => GateCheck::new(
            gate,
            Status::Pass,
            format!(
                "still for {:.1} s, no alerts, {min_v:.2} V, board {max_c:.1} °C, {} live frames",
                span.as_secs_f64(),
                live_times.len()
            ),
        ),
    }
}

/// Gate 6: the typed confirmation must match exactly.
pub fn confirmation(typed: &str, expected: &str) -> GateCheck {
    if typed == expected {
        GateCheck::new(GateId::Confirmation, Status::Pass, format!("typed \"{expected}\""))
    } else {
        GateCheck::new(GateId::Confirmation, Status::Fail, format!("typed text does not match \"{expected}\""))
    }
}
