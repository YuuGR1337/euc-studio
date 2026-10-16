//! `euc image validate`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use euc_image::{Image, Outcome, Report};
use euc_profile::WheelProfile;
use serde::Serialize;

use crate::ui::{self, BAD, DIM, GOOD, STRONG};
use crate::{Context, Verdict};

#[derive(Debug, clap::Subcommand)]
pub(crate) enum Command {
    /// Run the image gate: size, stack pointer, vectors, info block, provenance.
    Validate {
        /// Firmware image (.bin or .elf).
        file: PathBuf,
        /// Wheel profile id [default: the only profile, when there is one]
        #[arg(long)]
        wheel: Option<String>,
    },
}

pub(crate) fn run(ctx: &Context, command: Command) -> anyhow::Result<Verdict> {
    let Command::Validate { file, wheel } = command;
    let wheel = pick_wheel(ctx, wheel.as_deref())?;
    let bytes = fs::read(&file).with_context(|| format!("cannot read {}", file.display()))?;
    let image = Image::from_bytes(&bytes).with_context(|| format!("cannot load {}", file.display()))?;
    let report = euc_image::validate(&image, &wheel);
    if ctx.json {
        ui::json(&Output { file: &file, report: &report })?;
    } else {
        print(&file, &wheel, &report);
    }
    Ok(if report.passed { Verdict::Ok } else { Verdict::Refused })
}

#[derive(Serialize)]
struct Output<'a> {
    file: &'a Path,
    #[serde(flatten)]
    report: &'a Report,
}

fn pick_wheel(ctx: &Context, id: Option<&str>) -> anyhow::Result<WheelProfile> {
    if let Some(id) = id {
        return ctx.wheel(id);
    }
    let profiles = ctx.profiles()?;
    match profiles.ids()?.as_slice() {
        [only] => Ok(profiles.load(only)?),
        ids => bail!("several wheel profiles ({}); choose one with --wheel", ids.join(", ")),
    }
}

fn print(file: &Path, wheel: &WheelProfile, report: &Report) {
    let size = ui::thousands(report.size as u64);
    ui::row(STRONG, "image", file.display());
    ui::row(STRONG, "wheel", format!("{} ({})", wheel.name, wheel.id));
    ui::row(STRONG, "format", format!("{}, {size} bytes at 0x{:08X}", report.format, report.load_addr));
    ui::row(STRONG, "sha256", report.sha256);
    let gate_width = report.checks.iter().map(|check| check.gate.to_string().len()).max().unwrap_or(0);
    for check in &report.checks {
        let (style, word) = match check.outcome {
            Outcome::Pass(_) => (GOOD, "pass"),
            Outcome::Skip(_) => (DIM, "skip"),
            Outcome::Fail(_) => (BAD, "fail"),
        };
        ui::row(style, &format!("  {word}"), format!("{:gate_width$}  {}", check.gate.to_string(), check.outcome));
    }
    if report.passed {
        ui::row(GOOD, "PASS", &report.identity);
    } else {
        let count = report.problems().count();
        ui::row(
            BAD,
            "FAIL",
            format!("{count} problem{}; this image must not be flashed", if count == 1 { "" } else { "s" }),
        );
    }
}
