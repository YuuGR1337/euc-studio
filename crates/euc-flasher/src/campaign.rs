//! The fault campaign: many simulated flashes, each with seeded random faults, each judged
//! against the simulated wheel's real state.
//!
//! A run ends in one of four verdicts:
//!
//! - **success**: the flasher reports `Flashed` and the wheel runs exactly that image;
//! - **recovered**: the flasher reports `Restored` and the wheel runs the stock image;
//! - **safe stop**: the flasher refused, aborted or stopped, and the wheel is recoverable: it runs
//!   a committed application, or its bootloader takes a clean stock restore afterwards;
//! - **unrecoverable**: anything else, including a false success, a changed bootloader or settings
//!   page, a forbidden command byte reaching the application, or a write the guard refused.
//!
//! The milestone asks for zero unrecoverable runs. The verdicts are only as good as the models:
//! the emulated bootloader follows this project's assumptions, not the vendor's code.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use euc_flash_sim::{BootState, Fault, FaultEffect, Phase};
use euc_image::Image;
use euc_profile::Sha256;
use serde::Serialize;

use crate::Target;
use crate::ledger::{FlashRecord, Ledger, MemoryLedger};
use crate::log::FlashLog;
use crate::session::{Outcome, Plan, Report, flash, restore};
use crate::sim::{
    AppFirmware, FaultPlan, OperatorScript, Rng, SIM_PEER, Sim, SimOperator, WheelScript, WheelState, images,
};
use crate::transport::Transport;

/// How a run ended, judged on the wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// The image runs.
    Success,
    /// The stock image was restored and runs.
    Recovered,
    /// Nothing flashed, or the flasher stopped, and the wheel is recoverable.
    SafeStop,
    /// The wheel is not in a state the flasher's rules allow.
    Unrecoverable,
}

impl Verdict {
    /// The kebab-case name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Recovered => "recovered",
            Self::SafeStop => "safe-stop",
            Self::Unrecoverable => "unrecoverable",
        }
    }
}

/// One simulated flash.
#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    /// Run number.
    pub index: usize,
    /// Its seed.
    pub seed: u64,
    /// What was flashed: `stock`, `ours`, `ours-first` (no stock flash yet), or `bad-*`.
    pub image: &'static str,
    /// Image length.
    pub image_len: usize,
    /// `stock` or `clean` application behaviour.
    pub firmware: &'static str,
    /// Planned fault kinds.
    pub faults: Vec<&'static str>,
    /// Faults that actually fired on the link.
    pub fired: Vec<String>,
    /// The flasher's outcome name.
    pub outcome: &'static str,
    /// The flasher's outcome, with its reason.
    pub detail: String,
    /// The verdict.
    pub verdict: Verdict,
    /// Why the run is unrecoverable, if it is.
    pub problem: Option<String>,
    /// The recoverability probe after a stop, if one ran.
    pub probe: Option<String>,
    /// Simulated seconds.
    pub seconds: f64,
}

/// The campaign's result and its distributions.
#[derive(Debug, Clone, Serialize)]
pub struct CampaignReport {
    /// Base seed.
    pub seed: u64,
    /// Every run.
    pub runs: Vec<RunSummary>,
    /// Runs per verdict.
    pub verdicts: BTreeMap<&'static str, usize>,
    /// Runs per flasher outcome.
    pub outcomes: BTreeMap<&'static str, usize>,
    /// Per fault kind: runs per verdict.
    pub by_fault: BTreeMap<&'static str, BTreeMap<&'static str, usize>>,
    /// Simulated hours in total.
    pub simulated_hours: f64,
}

impl CampaignReport {
    /// Runs judged unrecoverable.
    pub fn unrecoverable(&self) -> impl Iterator<Item = &RunSummary> {
        self.runs.iter().filter(|run| run.verdict == Verdict::Unrecoverable)
    }

    /// A plain-text table of the distributions.
    pub fn table(&self) -> String {
        use std::fmt::Write as _;
        let mut text = String::new();
        let _ = writeln!(
            text,
            "{} runs, seed {:#x}, {:.1} simulated hours",
            self.runs.len(),
            self.seed,
            self.simulated_hours
        );
        let _ = writeln!(text, "verdicts: {:?}", self.verdicts);
        let _ = writeln!(text, "outcomes: {:?}", self.outcomes);
        let _ = writeln!(
            text,
            "{:<24} {:>5} {:>8} {:>10} {:>9} {:>13}",
            "fault", "runs", "success", "recovered", "safe-stop", "unrecoverable"
        );
        for (fault, verdicts) in &self.by_fault {
            let count = |name| verdicts.get(name).copied().unwrap_or(0);
            let runs: usize = verdicts.values().sum();
            let _ = writeln!(
                text,
                "{fault:<24} {runs:>5} {:>8} {:>10} {:>9} {:>13}",
                count("success"),
                count("recovered"),
                count("safe-stop"),
                count("unrecoverable")
            );
        }
        text
    }
}

/// Runs `runs` simulated flashes from `seed`.
pub fn run(seed: u64, runs: usize) -> CampaignReport {
    let mut report = CampaignReport {
        seed,
        runs: Vec::with_capacity(runs),
        verdicts: BTreeMap::new(),
        outcomes: BTreeMap::new(),
        by_fault: BTreeMap::new(),
        simulated_hours: 0.0,
    };
    let mut seeds = Rng::new(seed);
    for index in 0..runs {
        let summary = run_one(index, seeds.next_u64());
        *report.verdicts.entry(summary.verdict.name()).or_default() += 1;
        *report.outcomes.entry(summary.outcome).or_default() += 1;
        let kinds = if summary.faults.is_empty() { vec!["(none)"] } else { summary.faults.clone() };
        for kind in kinds {
            *report.by_fault.entry(kind).or_default().entry(summary.verdict.name()).or_default() += 1;
        }
        report.simulated_hours += summary.seconds / 3600.0;
        report.runs.push(summary);
    }
    report
}

/// The image a run flashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Stock,
    Ours,
    OursFirst,
    BadUnknown,
    BadSize,
    BadBoard,
}

/// One run, fully determined by `seed`.
///
/// # Panics
///
/// If a synthetic factory image does not install (a bug in the simulator).
pub fn run_one(index: usize, seed: u64) -> RunSummary {
    let mut rng = Rng::new(seed);
    let stock_len = rng.range(4_096, 49_152) as usize;
    let ours_len = rng.range(4_096, 49_152) as usize;
    let stock_bytes = images::synthetic_app(stock_len, rng.next_u64());
    let wheel = images::sim_profile(&[&stock_bytes]);
    let stock = Image::raw(stock_bytes);
    let kind = match rng.below(100) {
        0..=54 => Kind::Stock,
        55..=84 => Kind::Ours,
        85..=89 => Kind::OursFirst,
        90..=93 => Kind::BadUnknown,
        94..=96 => Kind::BadSize,
        _ => Kind::BadBoard,
    };
    let image = match kind {
        Kind::Stock => stock.clone(),
        Kind::Ours | Kind::OursFirst => images::our_image(&wheel, ours_len, rng.next_u64(), "clean"),
        Kind::BadUnknown => Image::raw(images::synthetic_app(ours_len, rng.next_u64())),
        Kind::BadSize => Image::raw(images::synthetic_app(49_152 + 1 + rng.below(4_000) as usize, rng.next_u64())),
        Kind::BadBoard => {
            let mut other = wheel.clone();
            other.board.id = "other-board".into();
            images::our_image(&other, ours_len, rng.next_u64(), "clean")
        }
    };
    let firmware = if rng.chance(250) { AppFirmware::Clean } else { AppFirmware::Stock };
    let mut script = WheelScript { firmware, ..WheelScript::default() };
    let mut operator = OperatorScript::default();
    let mut faults = FaultPlan::none();
    let span = Span::of(image.bytes().len());
    let mut kinds = Vec::new();
    let count = [0, 1, 1, 2, 2, 3][rng.below(6) as usize];
    for _ in 0..count {
        let kind = FAULT_KINDS[rng.below(FAULT_KINDS.len() as u64) as usize];
        inject(kind, &mut rng, &mut faults, &mut script, &mut operator, span);
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }

    let mut sim = Sim::new(wheel.clone(), &stock, script, operator, faults, seed).expect("the factory image installs");
    let protected = protected_bytes(&sim);
    let mut ledger = MemoryLedger::default();
    if kind == Kind::Ours {
        ledger.record_flash(FlashRecord { peer: SIM_PEER.into(), sha256: Sha256::digest(stock.bytes()), stock: true });
    }
    let mut plan = Plan::new(wheel, image.clone(), stock.clone(), Target::Simulator);
    // Some runs also check re-entry, as `euc flash --sim` does: a second entry and transfer.
    plan.check_reentry = rng.chance(300);
    let report = flash(&plan, &mut sim.transport, &mut sim.operator, &mut ledger, &mut FlashLog::discard());
    let mut summary = RunSummary {
        index,
        seed,
        image: match kind {
            Kind::Stock => "stock",
            Kind::Ours => "ours",
            Kind::OursFirst => "ours-first",
            Kind::BadUnknown => "bad-unknown",
            Kind::BadSize => "bad-size",
            Kind::BadBoard => "bad-board",
        },
        image_len: image.bytes().len(),
        firmware: if firmware == AppFirmware::Clean { "clean" } else { "stock" },
        faults: kinds,
        fired: sim.world.borrow().events.clone(),
        outcome: report.outcome.name(),
        detail: format!("{:?}", report.outcome),
        verdict: Verdict::Unrecoverable,
        problem: None,
        probe: None,
        seconds: report.elapsed.as_secs_f64(),
    };
    let (verdict, problem) = judge(&mut sim, &plan, &report, &image, &stock, &protected, &mut summary.probe);
    summary.verdict = verdict;
    summary.problem = problem;
    summary.seconds = sim.transport.now().as_secs_f64();
    summary
}

/// Every fault kind the campaign injects, in the order the generator draws them.
pub const FAULT_KINDS: [&str; 22] = [
    "drop-write",
    "corrupt-write",
    "drop-notification",
    "corrupt-notification",
    "stall",
    "link-loss",
    "link-loss-early",
    "power-loss-transfer",
    "power-loss-erase",
    "power-loss-write",
    "flash-transient",
    "flash-corrupt",
    "wrong-image-on-wire",
    "slow-bootloader",
    "wheel-moving",
    "low-battery",
    "hot-board",
    "alarm",
    "entry-ignored",
    "wrong-confirmation",
    "restore-fails",
    "operator-absent",
];

/// Where a fault can land in a flash of an image of `image_len` bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// First and last write index of the transfer (commands come before it).
    pub writes: (u64, u64),
    /// 1 KiB blocks (and flash write chunks) of the image.
    pub blocks: u64,
}

impl Span {
    /// The span of a flash of `image_len` bytes.
    pub fn of(image_len: usize) -> Self {
        let blocks = image_len.div_ceil(1024) as u64;
        // Commands (V, N, retries, ! and @) come first; then about 52 writes per 1 KiB block.
        Self { writes: (8, 8 + blocks * 52), blocks }
    }
}

/// Adds one fault of `kind` with seeded random placement. False for an unknown kind.
#[allow(clippy::too_many_lines)] // one arm per fault kind, all in one place
pub fn inject(
    kind: &str,
    rng: &mut Rng,
    faults: &mut FaultPlan,
    script: &mut WheelScript,
    operator: &mut OperatorScript,
    span: Span,
) -> bool {
    let ((first, last), blocks) = (span.writes, span.blocks);
    let write = |rng: &mut Rng| rng.range(first, last.max(first + 1));
    let notification = |rng: &mut Rng| rng.below(700);
    let secs = |rng: &mut Rng, low: u64, high: u64| Duration::from_millis(rng.range(low * 1000, high * 1000));
    let chunks = (blocks as usize).max(1);
    let commit = rng.below(2) as usize;
    match kind {
        "drop-write" => {
            faults.drop_writes.push(write(rng));
        }
        "corrupt-write" => {
            faults.corrupt_writes.push(write(rng));
        }
        "drop-notification" => {
            let n = notification(rng);
            faults.drop_notifications.extend([n, n + 1 + rng.below(40)]);
        }
        "corrupt-notification" => {
            faults.corrupt_notifications.push(notification(rng));
        }
        "stall" => {
            let stall = secs(rng, 2, 9);
            faults.stalls.push((notification(rng), stall));
        }
        "link-loss" => {
            let down = secs(rng, 1, 25);
            faults.link_losses.push((write(rng), down));
        }
        "link-loss-early" => {
            let down = secs(rng, 1, 4);
            faults.link_losses.push((rng.below(first), down));
        }
        "power-loss-transfer" => {
            faults.power_losses.push(write(rng));
        }
        "power-loss-erase" => {
            let op = rng.below(24) as usize;
            faults.flash.push((
                commit,
                vec![Fault { phase: Phase::Erase, operation: op, attempt: None, effect: FaultEffect::Interrupt }],
            ));
        }
        "power-loss-write" => {
            let op = rng.below(chunks as u64) as usize;
            faults.flash.push((
                commit,
                vec![Fault { phase: Phase::Write, operation: op, attempt: None, effect: FaultEffect::Interrupt }],
            ));
        }
        "flash-transient" => {
            let op = rng.below(chunks as u64) as usize;
            let effect = FaultEffect::Transient;
            faults.flash.push((commit, vec![Fault { phase: Phase::Write, operation: op, attempt: Some(1), effect }]));
        }
        "flash-corrupt" => {
            let op = rng.below(chunks as u64) as usize;
            let effect = FaultEffect::CorruptWrite;
            faults.flash.push((commit, vec![Fault { phase: Phase::Write, operation: op, attempt: None, effect }]));
        }
        "wrong-image-on-wire" => {
            faults.wrong_block = Some(rng.range(1, blocks + 1));
        }
        "slow-bootloader" => {
            faults.slow_bootloader = Some(secs(rng, 3, 10));
        }
        "wheel-moving" => {
            let start = secs(rng, 0, 20);
            script.moving = Some((start, start + secs(rng, 1, 4)));
        }
        "low-battery" => {
            script.battery_v = 105.0 + rng.below(12) as f64;
        }
        "hot-board" => {
            script.board_c = 60.0 + rng.below(25) as f64;
        }
        "alarm" => {
            script.alerts = 1 << rng.below(8);
        }
        "entry-ignored" => {
            script.ignores_entry = true;
        }
        "wrong-confirmation" => {
            operator.types = Some("FLASH".into());
        }
        "restore-fails" => {
            // Every commit fails: the flasher must stop, and the bootloader must still recover.
            let corrupt = Fault { phase: Phase::Write, operation: 0, attempt: None, effect: FaultEffect::CorruptWrite };
            faults.flash.extend((0..4).map(|n| (n, vec![corrupt])));
            operator.power_cycles = rng.chance(500);
        }
        "operator-absent" => {
            operator.presses_button = false;
            operator.power_cycles = false;
        }
        _ => return false,
    }
    true
}

/// The bootloader region and the settings pages, as stored.
fn protected_bytes(sim: &Sim) -> Vec<u8> {
    let world = sim.world.borrow();
    let flash = world.wheel.flash();
    let regions = flash.regions();
    let mut bytes = flash
        .read(regions.bootloader.start, (regions.bootloader.end - regions.bootloader.start) as usize)
        .unwrap_or_default()
        .to_vec();
    for page in &regions.settings {
        bytes.extend_from_slice(flash.read(page.start, (page.end - page.start) as usize).unwrap_or_default());
    }
    bytes
}

fn judge(
    sim: &mut Sim,
    plan: &Plan,
    report: &Report,
    image: &Image,
    stock: &Image,
    protected: &[u8],
    probe: &mut Option<String>,
) -> (Verdict, Option<String>) {
    let unrecoverable = |why: String| (Verdict::Unrecoverable, Some(why));
    {
        let world = sim.world.borrow();
        let stats = world.wheel.stats();
        if protected_bytes(sim) != protected {
            return unrecoverable("the bootloader region or a settings page changed".into());
        }
        if stats.unexpected_commands > 0 {
            return unrecoverable(format!("{} command bytes reached the application", stats.unexpected_commands));
        }
        if let Some(sequence) = stats.service_sequences.iter().find(|s| *s != "!@") {
            return unrecoverable(format!("the application ran the {sequence} service sequence"));
        }
        if report.guard_refusals > 0 {
            return unrecoverable(format!("the guard refused {} writes", report.guard_refusals));
        }
        let committed = match world.wheel.flash().state() {
            BootState::Application { sha256, .. } => Some(*sha256),
            BootState::RecoverableBootloader => None,
        };
        if let WheelState::App { sha256 } = world.wheel.state()
            && committed != Some(sha256)
        {
            return unrecoverable("an uncommitted image runs".into());
        }
    }
    let state = sim.world.borrow().wheel.state();
    match &report.outcome {
        Outcome::Flashed { sha256 } => {
            if *sha256 == Sha256::digest(image.bytes()) && state == (WheelState::App { sha256: *sha256 }) {
                (Verdict::Success, None)
            } else {
                unrecoverable(format!("false success: the wheel is {state:?}"))
            }
        }
        Outcome::Restored { sha256, .. } => {
            if *sha256 == Sha256::digest(stock.bytes()) && state == (WheelState::App { sha256: *sha256 }) {
                (Verdict::Recovered, None)
            } else {
                unrecoverable(format!("false restore: the wheel is {state:?}"))
            }
        }
        Outcome::Refused { .. } | Outcome::Aborted { .. } => {
            if state == (WheelState::App { sha256: Sha256::digest(stock.bytes()) }) {
                (Verdict::SafeStop, None)
            } else {
                unrecoverable(format!("the wheel changed without a flash: {state:?}"))
            }
        }
        Outcome::Stopped { .. } => match recover_after_stop(sim, plan, probe) {
            Ok(()) => (Verdict::SafeStop, None),
            Err(why) => unrecoverable(why),
        },
    }
}

/// After a stop: the person switches the wheel on if needed; an application must run a
/// committed image, a bootloader must take a clean stock restore.
fn recover_after_stop(sim: &mut Sim, plan: &Plan, probe: &mut Option<String>) -> Result<(), String> {
    {
        let mut world = sim.world.borrow_mut();
        world.set_faults(FaultPlan::none());
        if !world.wheel.powered() {
            world.power_on();
        }
        world.advance(Duration::from_secs(3));
    }
    let state = sim.world.borrow().wheel.state();
    match state {
        WheelState::App { .. } => {
            *probe = Some("an application runs a committed image".into());
            Ok(())
        }
        WheelState::Bootloader => {
            let mut operator = SimOperator::new(Rc::clone(&sim.world), OperatorScript::default());
            let mut restore_plan = plan.clone();
            restore_plan.image = plan.recovery.clone();
            let report = restore(
                &restore_plan,
                &mut sim.transport,
                &mut operator,
                &mut MemoryLedger::default(),
                &mut FlashLog::discard(),
            );
            *probe = Some(format!("clean restore: {}", report.outcome.name()));
            let stock = Sha256::digest(plan.recovery.bytes());
            match (&report.outcome, sim.world.borrow().wheel.state()) {
                (Outcome::Restored { .. }, WheelState::App { sha256 }) if sha256 == stock => Ok(()),
                (outcome, state) => Err(format!("the bootloader did not take a clean restore: {outcome:?}, {state:?}")),
            }
        }
        other => Err(format!("after a power cycle the wheel is {other:?}")),
    }
}
