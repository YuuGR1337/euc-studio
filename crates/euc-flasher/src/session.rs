//! The flashing state machine: offline gates, wheel gates, confirmation, bootloader entry,
//! transfer, boot check, and recovery.
//!
//! ```text
//! gates 1 2 3 R 7 ─ listen, V, N ─ gate 4 ─ 5 s still ─ gate 5 ─ type ─ gate 6 ─ 5 s still ─ gate 5
//!   └ any fail: Refused (nothing but V and N was ever written)
//! ! (300 ms) @ ─ [clean firmware: press the power button] ─ bootloader polls `C`?
//!   └ no: Aborted (the application still runs)
//! YMODEM transfer ─ delivered? ─ frames, V, N? ─ Flashed
//!   └ no: recovery: CAN, stock transfer ─ boots? ─ Restored, else (after the attempts) Stopped + SWD guide
//! ```

use std::time::Duration;

use euc_ble::Outbound;
use euc_image::{Identity, Image};
use euc_profile::{Sha256, WheelProfile};
use euc_proto_begode::Reply;
use euc_ymodem::{Awaiting, CAN, CRC_MODE, PacketId, Progress, SendStatus, Sender, SenderConfig};
use serde::Serialize;

use crate::gate::{self, GateCheck, GateId, Limits, Status, Target};
use crate::ledger::{FlashRecord, Ledger};
use crate::log::{FlashLog, Record, hex, micros};
use crate::observe::Observer;
use crate::transport::{LinkError, Operator, Transport};

/// Timeouts and attempt limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// How long the YMODEM sender waits for an answer before it sends again.
    pub answer: Duration,
    /// How long to wait for a `V` or `N` reply (the stock firmware answers at the third group
    /// time, 0.6 to 0.9 s later).
    pub reply: Duration,
    /// Gap between `!` and `@` (the firmware needs at least 202 ms).
    pub entry_gap: Duration,
    /// How long the bootloader may take to poll after `@`.
    pub bootloader_wait: Duration,
    /// How long a flashed image may take to send frames.
    pub boot_wait: Duration,
    /// One reconnect attempt.
    pub reconnect: Duration,
    /// Reconnect attempts before asking for a power cycle.
    pub reconnects: u8,
    /// Stock restore attempts before the flasher stops.
    pub restore_attempts: u8,
    /// Longest transfer.
    pub transfer_limit: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            answer: Duration::from_secs(3),
            reply: Duration::from_millis(2500),
            entry_gap: Duration::from_millis(300),
            bootloader_wait: Duration::from_secs(12),
            boot_wait: Duration::from_secs(15),
            reconnect: Duration::from_secs(10),
            reconnects: 3,
            restore_attempts: 2,
            transfer_limit: Duration::from_secs(600),
        }
    }
}

/// Everything a flash needs, decided before the link is touched.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The wheel profile (pins, app slot, name).
    pub wheel: WheelProfile,
    /// The image to flash.
    pub image: Image,
    /// The stock image for recovery.
    pub recovery: Image,
    /// Simulator or real wheel.
    pub target: Target,
    /// Gate 4 and 5 limits.
    pub limits: Limits,
    /// Timeouts.
    pub timing: Timing,
    /// Simulator only: after a successful flash, enter the bootloader again and flash the image a
    /// second time (the re-entry part of gate 3's dry-run).
    pub check_reentry: bool,
}

impl Plan {
    /// A plan with the default limits and timing.
    pub fn new(wheel: WheelProfile, image: Image, recovery: Image, target: Target) -> Self {
        Self {
            wheel,
            image,
            recovery,
            target,
            limits: Limits::default(),
            timing: Timing::default(),
            check_reentry: false,
        }
    }
}

/// What was last seen on the wheel when the flasher stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WheelSeen {
    /// Its bootloader polls: another transfer can start.
    Bootloader,
    /// An application sends frames.
    Application,
    /// Nothing: no link, or no bytes.
    Silent,
}

/// How a session ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Outcome {
    /// The image was delivered, booted and answered `V` and `N`.
    Flashed {
        /// The image.
        sha256: Sha256,
    },
    /// A gate refused; the wheel was not changed (only `V` and `N` may have been written).
    Refused {
        /// The gate.
        gate: GateId,
        /// Why.
        reason: String,
    },
    /// The flash stopped before the bootloader took over; the previous application still runs.
    Aborted {
        /// Why.
        reason: String,
    },
    /// The image failed after the bootloader took over; the stock image was restored and boots.
    Restored {
        /// What failed.
        cause: String,
        /// The stock image now on the wheel.
        sha256: Sha256,
    },
    /// The flasher gave up. The SWD guide applies if the wheel cannot be brought back to its
    /// bootloader (docs/spec/flasher.md, "Recovery").
    Stopped {
        /// What failed.
        cause: String,
        /// What the wheel showed last.
        wheel: WheelSeen,
    },
}

impl Outcome {
    /// Short name: `flashed`, `refused`, `aborted`, `restored`, `stopped`.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Flashed { .. } => "flashed",
            Self::Refused { .. } => "refused",
            Self::Aborted { .. } => "aborted",
            Self::Restored { .. } => "restored",
            Self::Stopped { .. } => "stopped",
        }
    }
}

/// Why a transfer ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Purpose {
    /// The image of the plan.
    Target,
    /// The stock image, after a failure.
    Restore,
    /// The image again, after `!` `@` re-entry (simulator dry-run).
    Reentry,
}

/// How a transfer ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TransferResult {
    /// The bootloader acknowledged the end of file.
    Delivered {
        /// It acknowledged the batch close too.
        closed: bool,
    },
    /// It did not.
    Failed {
        /// Why.
        reason: String,
    },
}

/// One YMODEM transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TransferReport {
    /// Why it ran.
    pub purpose: Purpose,
    /// The image sent.
    pub sha256: Sha256,
    /// How it ended.
    pub result: TransferResult,
    /// Sender counters.
    pub progress: Progress,
    /// Reconnects during the transfer.
    pub reconnects: u32,
    /// Bytes dropped as answers to an earlier packet.
    pub stale_bytes: u32,
}

/// Everything a session did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// How it ended.
    pub outcome: Outcome,
    /// Every gate, in order.
    pub gates: Vec<GateCheck>,
    /// Every transfer, in order.
    pub transfers: Vec<TransferReport>,
    /// Simulator re-entry check, when the plan asked for it.
    pub reentry: Option<bool>,
    /// Writes the guard refused (a flasher bug if ever nonzero).
    pub guard_refusals: u32,
    /// Time from start to end on the transport's clock.
    pub elapsed: Duration,
}

/// Flashes `plan.image` over `link`. See the module docs for the sequence.
pub fn flash(
    plan: &Plan,
    link: &mut dyn Transport,
    operator: &mut dyn Operator,
    ledger: &mut dyn Ledger,
    log: &mut FlashLog,
) -> Report {
    let mut session = Session::new(plan, link, operator, ledger, log);
    let outcome = session.run();
    session.finish(outcome)
}

/// Restores the stock image on a wheel that already sits in its bootloader (after a stopped
/// session). Runs the recovery-image gate, then waits for the bootloader's poll.
pub fn restore(
    plan: &Plan,
    link: &mut dyn Transport,
    operator: &mut dyn Operator,
    ledger: &mut dyn Ledger,
    log: &mut FlashLog,
) -> Report {
    let mut session = Session::new(plan, link, operator, ledger, log);
    let outcome = session.run_restore();
    session.finish(outcome)
}

/// A step failed; the variants say how far the wheel got.
enum Stop {
    /// Before the bootloader request: the application still runs.
    Abort(String),
    /// After it: recovery decides.
    Recover(String),
}

struct Session<'a> {
    plan: &'a Plan,
    link: &'a mut dyn Transport,
    operator: &'a mut dyn Operator,
    ledger: &'a mut dyn Ledger,
    log: &'a mut FlashLog,
    observer: Observer,
    gates: Vec<GateCheck>,
    transfers: Vec<TransferReport>,
    reentry: Option<bool>,
    refusals: u32,
    started: Duration,
    peer: String,
    image_sha: Sha256,
    recovery_sha: Sha256,
    /// The image is a pinned stock build (set by gate 1).
    image_stock: bool,
}

impl<'a> Session<'a> {
    fn new(
        plan: &'a Plan,
        link: &'a mut dyn Transport,
        operator: &'a mut dyn Operator,
        ledger: &'a mut dyn Ledger,
        log: &'a mut FlashLog,
    ) -> Self {
        let started = link.now();
        let peer = link.peer();
        let image_sha = Sha256::digest(plan.image.bytes());
        let recovery_sha = Sha256::digest(plan.recovery.bytes());
        log.record(&Record::Start {
            version: env!("CARGO_PKG_VERSION"),
            link: link.kind(),
            peer: &peer,
            wheel: &plan.wheel.id,
            image_sha256: image_sha.to_string(),
            recovery_sha256: recovery_sha.to_string(),
        });
        Self {
            plan,
            link,
            operator,
            ledger,
            log,
            observer: Observer::default(),
            gates: Vec::new(),
            transfers: Vec::new(),
            reentry: None,
            refusals: 0,
            started,
            peer,
            image_sha,
            recovery_sha,
            image_stock: false,
        }
    }

    fn finish(self, outcome: Outcome) -> Report {
        let now = self.link.now();
        self.log.record(&Record::Outcome { t_us: micros(now), outcome: &outcome });
        Report {
            outcome,
            gates: self.gates,
            transfers: self.transfers,
            reentry: self.reentry,
            guard_refusals: self.refusals,
            elapsed: now.saturating_sub(self.started),
        }
    }

    // ----- the sequence --------------------------------------------------------------------

    fn run(&mut self) -> Outcome {
        if let Some(refused) = self.offline_gates() {
            return refused;
        }
        if let Err(outcome) = self.wheel_gates() {
            return outcome;
        }
        if let Err(stop) = self.enter_bootloader() {
            return match stop {
                Stop::Abort(reason) => Outcome::Aborted { reason },
                Stop::Recover(cause) => self.recover(cause),
            };
        }
        match self.transfer_and_boot(Purpose::Target) {
            Ok(()) => {
                let stock = self.image_stock;
                self.ledger.record_flash(FlashRecord { peer: self.peer.clone(), sha256: self.image_sha, stock });
                if self.plan.check_reentry && self.plan.target == Target::Simulator {
                    match self.reenter() {
                        Ok(()) => self.reentry = Some(true),
                        // The entry did not take: the flashed image still runs.
                        Err(Stop::Abort(why)) => {
                            self.step("reentry-failed", &why);
                            self.reentry = Some(false);
                        }
                        // The second transfer failed: the flashed image may be gone.
                        Err(Stop::Recover(cause)) => {
                            self.reentry = Some(false);
                            return self.recover(format!("re-entry check: {cause}"));
                        }
                    }
                }
                Outcome::Flashed { sha256: self.image_sha }
            }
            Err(cause) => self.recover(cause),
        }
    }

    fn run_restore(&mut self) -> Outcome {
        let (check, _) = gate::recovery_image(&self.plan.recovery, &self.plan.wheel, self.plan.target);
        if !self.gate(check) {
            return Outcome::Refused { gate: GateId::RecoveryImage, reason: self.last_gate_message() };
        }
        self.recover("restore requested".to_owned())
    }

    /// Gates 1, 2, 3, the recovery image and 7: no link traffic.
    fn offline_gates(&mut self) -> Option<Outcome> {
        let plan = self.plan;
        let (check, report) = gate::image(&plan.image, &plan.wheel);
        self.image_stock = matches!(report.identity, Identity::Stock { .. });
        let checks = [
            check,
            gate::provenance(&report, plan.target),
            gate::dry_run(&self.image_sha, &plan.wheel.id, plan.target, &*self.ledger),
            gate::recovery_image(&plan.recovery, &plan.wheel, plan.target).0,
            gate::first_flash_stock(&report, &self.peer, &*self.ledger),
        ];
        for check in checks {
            let id = check.gate;
            if !self.gate(check) {
                return Some(Outcome::Refused { gate: id, reason: self.last_gate_message() });
            }
        }
        None
    }

    /// Gates 4, 5, 6 and 5 again, on the live wheel. Only `V` and `N` are written.
    fn wheel_gates(&mut self) -> Result<(), Outcome> {
        let plan = self.plan;
        let refuse = |gate: GateId, reason: String| Outcome::Refused { gate, reason };
        let since = self.link.now();
        self.step("listen", "telemetry before asking for the identity");
        self.listen(Duration::from_millis(1500)).map_err(|e| Outcome::Aborted { reason: e.to_string() })?;
        for byte in *b"VN" {
            self.ask(byte, 2).map_err(|e| Outcome::Aborted { reason: e.to_string() })?;
        }
        let check = gate::identity(&self.observer, since, &gate::model_name(&plan.wheel), &plan.limits);
        if !self.gate(check) {
            return Err(refuse(GateId::Identity, self.last_gate_message()));
        }
        self.still_window().map_err(|reason| refuse(GateId::WheelState, reason))?;

        let expected = gate::confirmation_text(&plan.wheel);
        let typed = self.operator.confirm(&expected);
        let now = micros(self.link.now());
        self.log.record(&Record::Operator { t_us: now, prompt: &expected, answer: Some(typed.clone()) });
        if !self.gate(gate::confirmation(&typed, &expected)) {
            return Err(refuse(GateId::Confirmation, self.last_gate_message()));
        }
        // The person took their time: the wheel must still be standing still right now.
        self.still_window().map_err(|reason| refuse(GateId::WheelState, reason))
    }

    /// Watches the wheel for the still time without writing; gate 5 judges the window.
    fn still_window(&mut self) -> Result<(), String> {
        let limits = self.plan.limits;
        let from = self.link.now();
        self.step("still", "watching the wheel without writing");
        if let Err(error) = self.listen(limits.still + Duration::from_millis(100)) {
            let check = GateCheck {
                gate: GateId::WheelState,
                number: GateId::WheelState.number(),
                status: Status::Fail,
                message: format!("{error} while watching the wheel"),
            };
            self.gate(check);
            return Err(self.last_gate_message());
        }
        let to = self.link.now();
        let check = gate::wheel_state(&self.observer, from, to, &limits);
        if self.gate(check) { Ok(()) } else { Err(self.last_gate_message()) }
    }

    /// `!`, the gap, `@`; with our clean firmware, the power-button press; then the bootloader's
    /// first poll.
    fn enter_bootloader(&mut self) -> Result<(), Stop> {
        let timing = self.plan.timing;
        self.link.arm_entry();
        self.step("entry", "sending '!' then '@'");
        self.command(b'!').map_err(|e| Stop::Abort(format!("'!' not sent: {e}")))?;
        self.listen(timing.entry_gap).map_err(|e| Stop::Abort(format!("{e} after '!'")))?;
        let at = self.link.now();
        // Once '@' may have left, the wheel may be on its way to the bootloader: recover, not abort.
        match self.command(b'@') {
            Ok(()) => {}
            Err(LinkError::Lost) => return Err(Stop::Recover("the link dropped while sending '@'".into())),
            Err(error) => return Err(Stop::Abort(format!("'@' not sent: {error}"))),
        }
        // Now the bootloader polls, or our clean firmware first asks for the power button. Its
        // SERVICE:CONFIRM can come late (a slow link), so watch for it the whole time.
        let mut asked: Option<Duration> = None;
        let mut deadline = at + timing.bootloader_wait;
        loop {
            let now = self.link.now();
            if self.observer.bootloader_listening(now) {
                return Ok(());
            }
            if asked.is_none() && service_reply(&self.observer, at, &["SERVICE:CONFIRM"]).is_some() {
                self.operator.say("Press the wheel's power button briefly now.");
                self.log.record(&Record::Operator {
                    t_us: micros(now),
                    prompt: "press the power button",
                    answer: None,
                });
                self.operator.press_power_button();
                asked = Some(now);
                deadline = self.link.now() + timing.bootloader_wait;
            }
            if service_reply(&self.observer, at, &["SERVICE:DENIED"]).is_some() {
                return Err(Stop::Abort("the wheel denied the bootloader request".into()));
            }
            if now >= deadline {
                return Err(if self.observer.app_running(now) {
                    Stop::Abort("the wheel ignored the bootloader request".into())
                } else {
                    Stop::Recover("no bootloader poll after '@'".into())
                });
            }
            let wait = deadline.saturating_sub(now).min(Duration::from_millis(500));
            if self.receive(wait).is_err() && !self.regain_link(true) {
                return Err(Stop::Recover("the link did not come back after '@'".into()));
            }
        }
    }

    /// Transfers an image, waits for it to boot and checks `V` and `N`.
    fn transfer_and_boot(&mut self, purpose: Purpose) -> Result<(), String> {
        let (image, sha) = match purpose {
            Purpose::Restore => (self.plan.recovery.bytes().to_vec(), self.recovery_sha),
            Purpose::Target | Purpose::Reentry => (self.plan.image.bytes().to_vec(), self.image_sha),
        };
        let report = self.transfer(&image, sha, purpose);
        let result = report.result.clone();
        self.transfers.push(report);
        if let TransferResult::Failed { reason } = result {
            return Err(format!("transfer failed: {reason}"));
        }
        self.await_boot()
    }

    /// After a failure past the bootloader request: stock restore attempts, then stop.
    fn recover(&mut self, cause: String) -> Outcome {
        let timing = self.plan.timing;
        self.step("recover", &cause);
        let mut last_seen = WheelSeen::Silent;
        for attempt in 1..=timing.restore_attempts {
            self.step("restore", &format!("attempt {attempt}"));
            last_seen = self.await_bootloader(timing.bootloader_wait);
            match last_seen {
                WheelSeen::Bootloader => {}
                // No YMODEM went out: the bootloader request never took, the old application runs.
                WheelSeen::Application if self.transfers.is_empty() => {
                    return Outcome::Aborted { reason: format!("{cause}; the application still runs") };
                }
                // Never write to an application here: it is not ours to judge which image runs.
                WheelSeen::Application => {
                    return Outcome::Stopped {
                        cause: format!("{cause}; an application boots instead of the bootloader"),
                        wheel: last_seen,
                    };
                }
                WheelSeen::Silent => continue,
            }
            // End any half-finished session in the receiver, then let it poll again.
            let _ = self.transmit(&[CAN, CAN]);
            let _ = self.listen(Duration::from_millis(1500));
            match self.transfer_and_boot(Purpose::Restore) {
                Ok(()) => {
                    self.ledger.record_flash(FlashRecord {
                        peer: self.peer.clone(),
                        sha256: self.recovery_sha,
                        stock: true,
                    });
                    return Outcome::Restored { cause, sha256: self.recovery_sha };
                }
                Err(why) => self.step("restore-failed", &why),
            }
        }
        if last_seen != WheelSeen::Application {
            last_seen = self.seen_now();
        }
        self.step("swd-guide", "stock restore failed; see docs/spec/flasher.md, Recovery");
        self.operator.say(
            "The flasher stopped. If the wheel's bootloader still polls, run the stock restore again; \
             otherwise follow the SWD recovery guide in docs/spec/flasher.md.",
        );
        Outcome::Stopped { cause, wheel: last_seen }
    }

    /// Simulator dry-run: `!` `@` again from the new image, then the same image once more.
    fn reenter(&mut self) -> Result<(), Stop> {
        self.step("reentry", "checking that the flashed image enters the bootloader again");
        self.listen(Duration::from_secs(2)).map_err(|e| Stop::Abort(format!("{e} before the re-entry")))?;
        self.enter_bootloader()?;
        self.transfer_and_boot(Purpose::Reentry).map_err(Stop::Recover)
    }

    // ----- waiting -------------------------------------------------------------------------

    /// Waits until the bootloader polls, an application is seen past `wait`, or nothing comes.
    fn await_bootloader(&mut self, wait: Duration) -> WheelSeen {
        let deadline = self.link.now() + wait;
        loop {
            let now = self.link.now();
            if self.observer.bootloader_listening(now) {
                return WheelSeen::Bootloader;
            }
            if now >= deadline {
                return if self.observer.app_running(now) { WheelSeen::Application } else { WheelSeen::Silent };
            }
            match self.receive(deadline.saturating_sub(now).min(Duration::from_millis(500))) {
                Ok(_) => {}
                Err(_) => {
                    if !self.regain_link(true) {
                        return WheelSeen::Silent;
                    }
                }
            }
        }
    }

    /// After a delivered transfer: frames within the boot time, then `V` and `N`.
    fn await_boot(&mut self) -> Result<(), String> {
        let timing = self.plan.timing;
        let start = self.link.now();
        self.step("boot", "waiting for the image to send frames");
        let deadline = start + timing.boot_wait;
        loop {
            let now = self.link.now();
            let groups = self.observer.frames_since(start).count();
            if groups >= 5 {
                break;
            }
            if now >= deadline {
                return Err(match self.seen_now() {
                    WheelSeen::Bootloader => "the bootloader did not boot the image".to_owned(),
                    _ => "no frames after the transfer".to_owned(),
                });
            }
            if self.receive(deadline.saturating_sub(now).min(Duration::from_millis(500))).is_err()
                && !self.regain_link(true)
            {
                return Err("the link did not come back after the transfer".into());
            }
        }
        let since = self.link.now();
        for byte in *b"VN" {
            // A wheel that just booted gets more patience than one that is about to be flashed.
            let answered = self.ask(byte, 4).map_err(|e| format!("{e} during the identity check"))?;
            if !answered {
                return Err(format!("the booted image did not answer '{}'", byte as char));
            }
        }
        let expected = gate::model_name(&self.plan.wheel);
        match self.observer.reply_since(since, |r| if let Reply::Name(n) = r { Some(n.clone()) } else { None }) {
            Some(name) if name == expected => {
                self.step("booted", &format!("frames and NAME:{name}"));
                Ok(())
            }
            other => Err(format!("the booted image answers N with {other:?}")),
        }
    }

    /// What the wheel shows right now, after a short look.
    fn seen_now(&mut self) -> WheelSeen {
        if self.listen(Duration::from_millis(2500)).is_err() {
            return WheelSeen::Silent;
        }
        let now = self.link.now();
        if self.observer.app_running(now) {
            WheelSeen::Application
        } else if self.observer.bootloader_listening(now) {
            WheelSeen::Bootloader
        } else {
            WheelSeen::Silent
        }
    }

    // ----- the transfer --------------------------------------------------------------------

    #[allow(clippy::too_many_lines)] // one loop: receive, drop stale answers, drive the sender, watch for restarts
    fn transfer(&mut self, image: &[u8], sha256: Sha256, purpose: Purpose) -> TransferReport {
        let timing = self.plan.timing;
        let config = SenderConfig { max_attempts: 10, start_timeouts: 5, short_tail: true };
        let mut report = TransferReport {
            purpose,
            sha256,
            result: TransferResult::Failed { reason: "not started".into() },
            progress: Progress::default(),
            reconnects: 0,
            stale_bytes: 0,
        };
        let mut sender = match Sender::new("app.bin", image, config) {
            Ok(sender) => sender,
            Err(error) => {
                report.result = TransferResult::Failed { reason: error.to_string() };
                return report;
            }
        };
        self.step("transfer", &format!("{purpose:?}: {} bytes", image.len()));
        let start = self.link.now();
        // When the latest transmission finished leaving: answers that arrived before it are stale.
        let mut sent_at = start;
        let mut timer = start;
        let mut polls = 0_u32;
        let mut closing = false;
        let failure = loop {
            match sender.status() {
                SendStatus::Running => {}
                SendStatus::Done { .. } => break None,
                SendStatus::Failed(error) => break Some(error.to_string()),
            }
            let now = self.link.now();
            if now.saturating_sub(start) > timing.transfer_limit {
                let cancel = sender.cancel();
                let _ = self.transmit(&cancel);
                break Some("the transfer took too long".into());
            }
            let wait = (timer + timing.answer).saturating_sub(now).max(Duration::from_millis(1));
            let Ok(inbound) = self.link.recv(wait) else {
                self.link_event("lost", "during the transfer");
                if !self.regain_link(true) {
                    break Some("the link did not come back".into());
                }
                report.reconnects += 1;
                timer = self.link.now();
                continue;
            };
            let Some(inbound) = inbound else {
                // The file is in and the close got its one try: stop here.
                if sender.delivered() && closing {
                    break None;
                }
                // Nothing for the answer time: the sender decides.
                if let Some(packet) = sender.timeout() {
                    match self.transmit(&packet) {
                        Ok(()) => sent_at = self.link.now(),
                        Err(error) => self.link_event("send", &error.to_string()),
                    }
                }
                closing |= matches!(sender.awaiting(), Awaiting::Answer(PacketId::Close));
                timer = self.link.now();
                continue;
            };
            self.rx(&inbound.at, &inbound.bytes);
            self.observer.push(inbound.at, &inbound.bytes);
            if self.observer.app_running(inbound.at) {
                break Some("application frames during the transfer".into());
            }
            let poll = !inbound.bytes.is_empty() && inbound.bytes.iter().all(|&b| b == CRC_MODE);
            for &byte in &inbound.bytes {
                if inbound.at < sent_at && byte != CAN {
                    report.stale_bytes += 1;
                    continue;
                }
                let before = sender.awaiting();
                if let Some(packet) = sender.receive(byte) {
                    match self.transmit(&packet) {
                        Ok(()) => sent_at = self.link.now(),
                        Err(error) => self.link_event("send", &error.to_string()),
                    }
                    timer = self.link.now();
                } else if sender.awaiting() != before {
                    timer = self.link.now();
                }
                closing |= matches!(sender.awaiting(), Awaiting::Answer(PacketId::Close));
            }
            // A receiver that polls while a data block or the EOT is out has started over.
            if poll && matches!(sender.awaiting(), Awaiting::Answer(PacketId::Data(_) | PacketId::Eot)) {
                polls += 1;
                if polls >= 3 {
                    let cancel = sender.cancel();
                    let _ = self.transmit(&cancel);
                    break Some("the bootloader started over".into());
                }
            } else if !poll {
                polls = 0;
            }
        };
        report.progress = sender.progress();
        report.result = match failure {
            None if sender.delivered() => {
                TransferResult::Delivered { closed: matches!(sender.status(), SendStatus::Done { closed: true }) }
            }
            None => TransferResult::Failed { reason: "the end of file was not acknowledged".into() },
            Some(reason) if sender.delivered() => {
                // The file is in; only the close failed.
                self.step("transfer", &format!("delivered; close failed: {reason}"));
                TransferResult::Delivered { closed: false }
            }
            Some(reason) => TransferResult::Failed { reason },
        };
        self.step("transfer-end", &format!("{:?}", report.result));
        report
    }

    // ----- link helpers --------------------------------------------------------------------

    /// Reconnects; after the attempts, asks for a power cycle once (when `power_cycle`).
    fn regain_link(&mut self, power_cycle: bool) -> bool {
        let timing = self.plan.timing;
        for _ in 0..timing.reconnects {
            if self.link.reconnect(timing.reconnect).is_ok() {
                self.link_event("reconnected", "");
                self.observer.forget();
                return true;
            }
            self.link_event("reconnect-failed", "");
        }
        if !power_cycle {
            return false;
        }
        let now = micros(self.link.now());
        self.log.record(&Record::Operator { t_us: now, prompt: "switch the wheel off and on", answer: None });
        if !self.operator.power_cycle_wheel() {
            return false;
        }
        for _ in 0..timing.reconnects {
            if self.link.reconnect(timing.reconnect).is_ok() {
                self.link_event("reconnected", "after a power cycle");
                self.observer.forget();
                return true;
            }
            self.link_event("reconnect-failed", "after a power cycle");
        }
        false
    }

    /// One receive into the observer.
    fn receive(&mut self, timeout: Duration) -> Result<bool, LinkError> {
        match self.link.recv(timeout) {
            Ok(Some(inbound)) => {
                self.rx(&inbound.at, &inbound.bytes);
                self.observer.push(inbound.at, &inbound.bytes);
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(error) => {
                self.link_event("lost", &error.to_string());
                Err(error)
            }
        }
    }

    /// Receives for `duration`.
    fn listen(&mut self, duration: Duration) -> Result<(), LinkError> {
        self.listen_until(duration, |_, _| false).map(|_| ())
    }

    /// Receives until `done` holds (true) or `duration` passed (false).
    fn listen_until(
        &mut self,
        duration: Duration,
        done: impl Fn(&Observer, Duration) -> bool,
    ) -> Result<bool, LinkError> {
        let deadline = self.link.now() + duration;
        loop {
            let now = self.link.now();
            if done(&self.observer, now) {
                return Ok(true);
            }
            if now >= deadline {
                return Ok(false);
            }
            self.receive(deadline.saturating_sub(now))?;
        }
    }

    /// Sends `V` or `N` and waits for its reply, `tries` times at most. True once it answered.
    fn ask(&mut self, byte: u8, tries: u8) -> Result<bool, LinkError> {
        for _ in 0..tries {
            let since = self.link.now();
            self.command(byte)?;
            let reply = self.plan.timing.reply;
            let answered = self.listen_until(reply, move |observer, _| {
                observer
                    .reply_since(since, |r| match (byte, r) {
                        (b'V', Reply::Version(_)) | (b'N', Reply::Name(_)) => Some(()),
                        _ => None,
                    })
                    .is_some()
            })?;
            // The stock firmware keeps one command slot: leave it alone a little longer.
            self.listen(Duration::from_millis(300))?;
            if answered {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn command(&mut self, byte: u8) -> Result<(), LinkError> {
        let now = self.link.now();
        let result = self.link.send(Outbound::Command(byte));
        self.tx(now, "command", &[byte], &result);
        result
    }

    /// Sends a YMODEM transmission only while the bootloader's receiver is known to listen
    /// (the guard's own rule): after a reconnect, nothing goes out until it speaks again.
    fn transmit(&mut self, packet: &[u8]) -> Result<(), LinkError> {
        if !self.observer.bootloader_listening(self.link.now()) {
            self.link_event("held", "no word from the bootloader since the link came up");
            return Err(LinkError::Other("held until the bootloader speaks".into()));
        }
        self.ymodem(packet)
    }

    fn ymodem(&mut self, packet: &[u8]) -> Result<(), LinkError> {
        let now = self.link.now();
        let result = self.link.send(Outbound::Ymodem(packet));
        self.tx(now, "ymodem", packet, &result);
        result
    }

    fn tx(&mut self, at: Duration, kind: &str, bytes: &[u8], result: &Result<(), LinkError>) {
        match result {
            Ok(()) => {
                if self.log.enabled() {
                    self.log.record(&Record::Tx { t_us: micros(at), kind, hex: hex(bytes) });
                }
            }
            Err(error) => {
                if matches!(error, LinkError::Refused(_)) {
                    self.refusals += 1;
                }
                let detail = format!("{kind} {}: {error}", hex(&bytes[..bytes.len().min(8)]));
                self.link_event("not-sent", &detail);
            }
        }
    }

    fn rx(&mut self, at: &Duration, bytes: &[u8]) {
        if self.log.enabled() {
            self.log.record(&Record::Rx { t_us: micros(*at), hex: hex(bytes) });
        }
    }

    fn link_event(&mut self, event: &str, detail: &str) {
        let now = micros(self.link.now());
        self.log.record(&Record::Link { t_us: now, event, detail: detail.to_owned() });
    }

    fn step(&mut self, step: &str, detail: &str) {
        let now = micros(self.link.now());
        self.log.record(&Record::Step { t_us: now, step, detail: detail.to_owned() });
    }

    /// Records a gate; true unless it failed.
    fn gate(&mut self, check: GateCheck) -> bool {
        let now = micros(self.link.now());
        self.log.record(&Record::Gate { t_us: now, check: &check });
        let passed = check.passed();
        self.gates.push(check);
        passed
    }

    fn last_gate_message(&self) -> String {
        self.gates.last().map(|check| check.message.clone()).unwrap_or_default()
    }
}

/// The first of the clean firmware's `SERVICE:*` replies in `wanted` after `since`.
fn service_reply(observer: &Observer, since: Duration, wanted: &[&str]) -> Option<String> {
    observer.reply_since(since, |reply| match reply {
        Reply::Text(text) if wanted.contains(&text.trim()) => Some(text.trim().to_owned()),
        _ => None,
    })
}

/// The wheel profile helper for callers that build plans: the expected `N` answer.
pub fn expected_name(wheel: &WheelProfile) -> String {
    gate::model_name(wheel)
}
