//! `euc flash`: the gated flasher, against the simulated wheel only (docs/spec/flasher.md).
//!
//! `--sim` flashes the simulated wheel (emulated bootloader, virtual time); `--dry-run` runs the
//! offline gates a real flash needs and connects to nothing; `--campaign` runs the fault campaign.
//! This binary has no path to a real wheel: the BLE transport is a feature no default build
//! enables, and this command refuses anything else.

use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use euc_flasher::campaign::{self, Span};
use euc_flasher::sim::{AppFirmware, FaultPlan, OperatorScript, Rng, Sim, WheelScript, images};
use euc_flasher::{
    DryRun, FileLedger, FlashLog, GateCheck, GateId, Ledger, MemoryLedger, Outcome, Plan, Report, Status, Target,
    TransferResult,
};
use euc_image::Image;
use euc_profile::{Channel, FirmwareSource, Sha256, WheelProfile};
use serde::Serialize;

use crate::ui::{self, BAD, DIM, GOOD, STRONG, WARN};
use crate::{Context, Verdict};

/// Arguments of `euc flash`.
#[derive(Debug, clap::Args)]
#[allow(clippy::struct_excessive_bools)] // command-line switches
pub(crate) struct Args {
    /// Firmware image (.bin or .elf). With --synthetic it may be left out: the simulator's own
    /// stock image is flashed.
    image: Option<PathBuf>,
    /// Flash the simulated wheel: emulated bootloader, virtual time, nothing leaves this computer.
    #[arg(long, conflicts_with_all = ["dry_run", "campaign"])]
    sim: bool,
    /// Run the offline gates a real flash needs (1 image, 2 provenance, 3 dry-run, recovery image,
    /// 7 first flash = stock) and stop. Connects to nothing and writes nothing.
    #[arg(long, conflicts_with = "campaign")]
    dry_run: bool,
    /// Run RUNS simulated flashes with seeded random faults and print the distribution.
    #[arg(long, value_name = "RUNS")]
    campaign: Option<usize>,
    /// Wheel profile id [default: the only profile, when there is one]
    #[arg(long)]
    wheel: Option<String>,
    /// The stock image for recovery, also the simulated wheel's factory image [default: the
    /// wheel's current stock build from the firmware cache, see `euc fetch`]
    #[arg(long, value_name = "FILE")]
    recovery: Option<PathBuf>,
    /// With --sim: use the simulator's synthetic wheel profile and stock image (no vendor image).
    #[arg(long, requires = "sim")]
    synthetic: bool,
    /// With --sim: flash the stock image first, in the same session (gate 7 wants it for our builds).
    #[arg(long, requires = "sim")]
    stock_first: bool,
    /// With --sim: inject a fault, placed by --seed (repeatable). Kinds: see docs/spec/flasher.md.
    #[arg(long = "inject", value_name = "KIND", requires = "sim")]
    inject: Vec<String>,
    /// With --sim: the simulated application asks for the power button, like our clean firmware.
    #[arg(long, requires = "sim")]
    clean_firmware: bool,
    /// Seed for fault placement and for the campaign.
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Write the JSONL byte log of the session here (a new file).
    #[arg(long, value_name = "FILE")]
    log: Option<PathBuf>,
    /// With --dry-run: the wheel's BLE identifier, for gate 7's history.
    #[arg(long, value_name = "ID", requires = "dry_run")]
    peer: Option<String>,
    /// The flasher's ledger of dry-runs and flashes [default: the per-user data folder]
    #[arg(long, env = "EUC_FLASH_LEDGER", value_name = "FILE")]
    ledger: Option<PathBuf>,
}

pub(crate) fn run(ctx: &Context, args: &Args) -> anyhow::Result<Verdict> {
    if let Some(runs) = args.campaign {
        return campaign(ctx, runs, args.seed);
    }
    if args.sim {
        return sim(ctx, args);
    }
    if args.dry_run {
        return dry_run(ctx, args);
    }
    ui::row(BAD, "REFUSED", "this build cannot flash a real wheel (SAFETY.md); use --sim, --dry-run or --campaign");
    Ok(Verdict::Refused)
}

// ----- --sim ---------------------------------------------------------------------------------

#[derive(Serialize)]
struct SimOutput<'a> {
    mode: &'static str,
    wheel: &'a str,
    image: ImageOut<'a>,
    recovery: String,
    faults: &'a [String],
    fired: Vec<String>,
    stock_first: Option<Report>,
    report: &'a Report,
    dry_run: Option<DryRun>,
    log: Option<&'a Path>,
}

#[derive(Serialize)]
struct ImageOut<'a> {
    file: Option<&'a Path>,
    sha256: Sha256,
    bytes: usize,
}

fn sim(ctx: &Context, args: &Args) -> anyhow::Result<Verdict> {
    let (wheel, stock, recovery_label) = if args.synthetic {
        let stock = images::synthetic_app(24_000, args.seed);
        (images::sim_profile(&[&stock]), Image::raw(stock), "the simulator's synthetic stock image".to_owned())
    } else {
        let wheel = pick_wheel(ctx, args.wheel.as_deref())?;
        let (stock, label) = recovery(ctx, &wheel, args.recovery.as_deref())?;
        (wheel, stock, label)
    };
    let image = match &args.image {
        Some(path) => load_image(path)?,
        None if args.synthetic => stock.clone(),
        None => bail!("name the image to flash, or use --synthetic"),
    };

    let mut faults = FaultPlan::none();
    let mut script = WheelScript::default();
    let mut operator = OperatorScript::default();
    if args.clean_firmware {
        script.firmware = AppFirmware::Clean;
    }
    let mut rng = Rng::new(args.seed);
    let span = Span::of(image.bytes().len());
    for kind in &args.inject {
        if !campaign::inject(kind, &mut rng, &mut faults, &mut script, &mut operator, span) {
            bail!("unknown fault \"{}\"; one of: {}", ui::safe(kind), campaign::FAULT_KINDS.join(", "));
        }
    }
    let mut sim = Sim::new(wheel.clone(), &stock, script, operator, faults, args.seed).map_err(anyhow::Error::msg)?;
    let mut log = match &args.log {
        Some(path) => {
            let file = File::create_new(path).with_context(|| format!("cannot create {}", ui::path(path)))?;
            FlashLog::to_writer(Box::new(file))
        }
        None => FlashLog::discard(),
    };
    // The simulated wheel's history lives with the simulated wheel.
    let mut history = MemoryLedger::default();
    let stock_first = args.stock_first.then(|| {
        let plan = Plan::new(wheel.clone(), stock.clone(), stock.clone(), Target::Simulator);
        euc_flasher::flash(&plan, &mut sim.transport, &mut sim.operator, &mut history, &mut log)
    });
    let mut plan = Plan::new(wheel.clone(), image.clone(), stock.clone(), Target::Simulator);
    plan.check_reentry = true;
    let report = euc_flasher::flash(&plan, &mut sim.transport, &mut sim.operator, &mut history, &mut log);

    let flashed = matches!(report.outcome, Outcome::Flashed { .. });
    let dry_run = (flashed && !args.synthetic).then(|| DryRun {
        sha256: Sha256::digest(image.bytes()),
        wheel: wheel.id.clone(),
        flashed: true,
        reentry: report.reentry == Some(true),
        ride: None,
    });
    if let Some(run) = &dry_run {
        let mut ledger = open_ledger(args.ledger.as_deref())?;
        ledger.record_dry_run(run.clone());
        if let Some(error) = &ledger.write_error {
            ui::warn(format!("the dry-run was not recorded: {}", ui::safe(error)));
        }
    }
    let output = SimOutput {
        mode: "sim",
        wheel: &wheel.id,
        image: ImageOut {
            file: args.image.as_deref(),
            sha256: Sha256::digest(image.bytes()),
            bytes: image.bytes().len(),
        },
        recovery: recovery_label,
        faults: &args.inject,
        fired: sim.world.borrow().events.clone(),
        stock_first,
        report: &report,
        dry_run,
        log: args.log.as_deref(),
    };
    if ctx.json {
        ui::json_with_profiles(&wheel.origin, &output)?;
    } else {
        print_sim(&wheel, &image, &output, args.synthetic);
    }
    Ok(if flashed { Verdict::Ok } else { Verdict::Refused })
}

fn print_sim(wheel: &WheelProfile, image: &Image, output: &SimOutput<'_>, synthetic: bool) {
    ui::row(STRONG, "wheel", format!("{} ({}), simulated", wheel.name, wheel.id));
    ui::row(STRONG, "image", image_line(output.image.file, image));
    ui::row(STRONG, "recovery", &output.recovery);
    if !output.faults.is_empty() {
        ui::row(STRONG, "faults", output.faults.join(", "));
    }
    if let Some(first) = &output.stock_first {
        ui::row(DIM, "first", format!("stock flash: {}", outcome_text(&first.outcome)));
    }
    print_report(output.report);
    if matches!(output.report.outcome, Outcome::Refused { gate: GateId::FirstFlashStock, .. }) {
        ui::detail("add --stock-first to flash the stock image first in the same simulated session");
    }
    for event in &output.fired {
        ui::detail(ui::paint(DIM, format!("fault: {event}")));
    }
    match (&output.dry_run, synthetic) {
        (Some(run), _) => ui::row(
            DIM,
            "dry-run",
            format!(
                "recorded (flash yes, re-entry {}); the 30 s ride is not implemented, so gate 3 still refuses real flashes",
                if run.reentry { "yes" } else { "no" }
            ),
        ),
        (None, true) => ui::row(DIM, "dry-run", "not recorded: synthetic images"),
        (None, false) => ui::row(DIM, "dry-run", "not recorded: the image was not flashed"),
    }
    if let Some(path) = output.log {
        ui::row(DIM, "log", ui::path(path));
    }
}

// ----- --dry-run -----------------------------------------------------------------------------

#[derive(Serialize)]
struct DryRunOutput<'a> {
    mode: &'static str,
    wheel: &'a str,
    image: ImageOut<'a>,
    gates: &'a [GateCheck],
    allowed: bool,
    note: &'static str,
}

const NO_REAL_FLASH: &str = "this build has no transport to a real wheel; nothing was connected or written";

fn dry_run(ctx: &Context, args: &Args) -> anyhow::Result<Verdict> {
    let wheel = pick_wheel(ctx, args.wheel.as_deref())?;
    let path = args.image.as_deref().context("name the image to check")?;
    let image = load_image(path)?;
    let (stock, _) = recovery(ctx, &wheel, args.recovery.as_deref())?;
    let ledger = open_ledger(args.ledger.as_deref())?;
    let peer = args.peer.clone().unwrap_or_default();
    let sha256 = Sha256::digest(image.bytes());
    let (image_check, report) = euc_flasher::image(&image, &wheel);
    let gates = vec![
        image_check,
        euc_flasher::provenance(&report, Target::Wheel),
        euc_flasher::dry_run(&sha256, &wheel.id, Target::Wheel, &ledger),
        euc_flasher::recovery_image(&stock, &wheel, Target::Wheel).0,
        euc_flasher::first_flash_stock(&report, &peer, &ledger),
    ];
    let allowed = gates.iter().all(GateCheck::passed);
    if ctx.json {
        let output = DryRunOutput {
            mode: "dry-run",
            wheel: &wheel.id,
            image: ImageOut { file: Some(path), sha256, bytes: image.bytes().len() },
            gates: &gates,
            allowed,
            note: NO_REAL_FLASH,
        };
        ui::json_with_profiles(&wheel.origin, &output)?;
    } else {
        ui::row(STRONG, "wheel", format!("{} ({}), from {}", wheel.name, wheel.id, wheel.origin));
        ui::row(STRONG, "image", image_line(Some(path), &image));
        print_gates(&gates);
        if allowed {
            ui::row(GOOD, "PASS", "the offline gates pass; gates 4, 5 and 6 need the wheel");
        } else {
            let first = gates.iter().find(|g| !g.passed()).map_or(String::new(), gate_name);
            ui::row(BAD, "REFUSED", format!("a real flash would stop at {first}"));
        }
        ui::row(DIM, "note", NO_REAL_FLASH);
    }
    Ok(if allowed { Verdict::Ok } else { Verdict::Refused })
}

// ----- --campaign ----------------------------------------------------------------------------

fn campaign(ctx: &Context, runs: usize, seed: u64) -> anyhow::Result<Verdict> {
    let report = campaign::run(seed, runs);
    let bad = report.unrecoverable().count();
    if ctx.json {
        ui::json(&report)?;
    } else {
        for line in report.table().lines() {
            ui::detail(line);
        }
        for run in report.unrecoverable() {
            ui::row(
                BAD,
                "BAD",
                format!("run {} seed {:#x}: {}", run.index, run.seed, run.problem.as_deref().unwrap_or("")),
            );
        }
        if bad == 0 {
            ui::row(GOOD, "PASS", format!("{runs} runs: every one ended in success or a recoverable state"));
        } else {
            ui::row(BAD, "FAIL", format!("{bad} of {runs} runs ended unrecoverable"));
        }
    }
    Ok(if bad == 0 { Verdict::Ok } else { Verdict::Refused })
}

// ----- helpers -------------------------------------------------------------------------------

fn pick_wheel(ctx: &Context, id: Option<&str>) -> anyhow::Result<WheelProfile> {
    if let Some(id) = id {
        return ctx.wheel(id);
    }
    let profiles = ctx.profiles();
    match profiles.ids()?.as_slice() {
        [only] => Ok(profiles.load(only)?),
        ids => bail!("several wheel profiles ({}); choose one with --wheel", ids.join(", ")),
    }
}

/// Largest file `euc flash` reads.
const MAX_FILE: u64 = 4 * 1024 * 1024;

fn load_image(path: &Path) -> anyhow::Result<Image> {
    let file = File::open(path).with_context(|| format!("cannot read {}", ui::path(path)))?;
    if !file.metadata()?.is_file() {
        bail!("{} is not a regular file", ui::path(path));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE {
        bail!("{} is larger than any firmware image", ui::path(path));
    }
    Image::from_bytes(&bytes).with_context(|| format!("cannot load {}", ui::path(path)))
}

/// The stock image for recovery: `--recovery`, else the current pinned build from the cache.
fn recovery(ctx: &Context, wheel: &WheelProfile, file: Option<&Path>) -> anyhow::Result<(Image, String)> {
    if let Some(path) = file {
        return Ok((load_image(path)?, ui::path(path)));
    }
    let current = wheel
        .firmware
        .iter()
        .find_map(|source| match source {
            FirmwareSource::BegodeApi(build) if build.channel == Channel::Current => Some(build),
            _ => None,
        })
        .with_context(|| format!("{} pins no current stock build; pass --recovery FILE", wheel.name))?;
    let cache = ctx.cache()?;
    let path = cache.verified(&current.sha256).with_context(|| {
        format!("the stock build {} is not cached; run `euc fetch {}` or pass --recovery FILE", current.label, wheel.id)
    })?;
    let bytes = fs::read(&path).with_context(|| format!("cannot read {}", ui::path(&path)))?;
    Ok((Image::raw(bytes), format!("stock build {} (cached)", current.label)))
}

fn open_ledger(path: Option<&Path>) -> anyhow::Result<FileLedger> {
    let path = match path {
        Some(path) => path.to_owned(),
        None => directories::ProjectDirs::from("", "", "euc-studio")
            .context("this system reports no per-user data folder; set EUC_FLASH_LEDGER")?
            .data_dir()
            .join("flasher-ledger.json"),
    };
    FileLedger::open(&path).with_context(|| format!("cannot read the ledger {}", ui::path(&path)))
}

fn image_line(path: Option<&Path>, image: &Image) -> String {
    let name = path.map_or_else(|| "synthetic stock image".to_owned(), ui::path);
    format!("{name}, {} bytes, sha256 {}", ui::thousands(image.bytes().len() as u64), Sha256::digest(image.bytes()))
}

fn gate_name(check: &GateCheck) -> String {
    match check.number {
        Some(number) => format!("gate {number} ({:?})", check.gate),
        None => format!("the {:?} gate", check.gate),
    }
}

fn print_gates(gates: &[GateCheck]) {
    for check in gates {
        let (style, word) = match check.status {
            Status::Pass => (GOOD, "pass"),
            Status::Skip => (DIM, "skip"),
            Status::Fail => (BAD, "fail"),
        };
        let number = check.number.map_or_else(|| "R".to_owned(), |n| n.to_string());
        ui::row(
            style,
            &format!("  {word}"),
            format!("{number} {:<16} {}", format!("{:?}", check.gate), ui::safe(&check.message)),
        );
    }
}

fn outcome_text(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Flashed { sha256 } => format!("flashed {}", sha256.short()),
        Outcome::Refused { gate, reason } => format!("refused at {gate:?}: {}", ui::safe(reason)),
        Outcome::Aborted { reason } => format!("aborted, the application still runs: {}", ui::safe(reason)),
        Outcome::Restored { cause, sha256 } => format!("restored stock {} after: {}", sha256.short(), ui::safe(cause)),
        Outcome::Stopped { cause, wheel } => format!("stopped ({wheel:?} seen): {}", ui::safe(cause)),
    }
}

fn print_report(report: &Report) {
    print_gates(&report.gates);
    for transfer in &report.transfers {
        let result = match &transfer.result {
            TransferResult::Delivered { closed: true } => "delivered".to_owned(),
            TransferResult::Delivered { closed: false } => "delivered (batch close unanswered)".to_owned(),
            TransferResult::Failed { reason } => format!("failed: {}", ui::safe(reason)),
        };
        let progress = transfer.progress;
        ui::row(
            STRONG,
            "transfer",
            format!(
                "{:?}: {result}; {} of {} blocks, {} retransmissions, {} reconnects",
                transfer.purpose,
                progress.acked_blocks,
                progress.total_blocks,
                progress.retransmissions,
                transfer.reconnects
            ),
        );
    }
    if let Some(reentry) = report.reentry {
        ui::row(
            if reentry { GOOD } else { WARN },
            "re-entry",
            if reentry { "'!' '@' reached the bootloader again" } else { "failed" },
        );
    }
    let seconds = report.elapsed.as_secs_f64();
    let (style, word) = match report.outcome {
        Outcome::Flashed { .. } => (GOOD, "FLASHED"),
        Outcome::Restored { .. } => (WARN, "RESTORED"),
        Outcome::Refused { .. } | Outcome::Aborted { .. } => (WARN, "STOPPED"),
        Outcome::Stopped { .. } => (BAD, "STOPPED"),
    };
    ui::row(style, word, format!("{} ({seconds:.1} s simulated)", outcome_text(&report.outcome)));
    if matches!(report.outcome, Outcome::Stopped { .. }) {
        ui::detail("see docs/spec/flasher.md, \"Recovery\", for the stock restore and the SWD guide");
    }
}
