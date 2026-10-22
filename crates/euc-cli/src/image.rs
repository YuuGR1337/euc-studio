//! Dry image validation and unsigned development sealing.

use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context as _, bail};
use euc_image::{Image, Outcome, Report, SealError};
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
    /// Seal an unsigned development BIN image with its length and CRC.
    Seal {
        /// Input firmware image (raw .bin only).
        input: PathBuf,
        /// New output file. Existing files are refused.
        #[arg(long)]
        out: PathBuf,
        /// Wheel profile id [default: the only profile, when there is one]
        #[arg(long)]
        wheel: Option<String>,
    },
}

pub(crate) fn run(ctx: &Context, command: Command) -> anyhow::Result<Verdict> {
    match command {
        Command::Validate { file, wheel } => validate(ctx, &file, wheel.as_deref()),
        Command::Seal { input, out, wheel } => seal(ctx, &input, &out, wheel.as_deref()),
    }
}

fn validate(ctx: &Context, file: &Path, wheel: Option<&str>) -> anyhow::Result<Verdict> {
    let wheel = pick_wheel(ctx, wheel)?;
    let bytes = fs::read(file).with_context(|| format!("cannot read {}", file.display()))?;
    let image = Image::from_bytes(&bytes).with_context(|| format!("cannot load {}", file.display()))?;
    let report = euc_image::validate(&image, &wheel);
    if ctx.json {
        ui::json_with_profiles(&wheel.origin, &Output { file, report: &report })?;
    } else {
        print(file, &wheel, &report);
    }
    Ok(if report.passed { Verdict::Ok } else { Verdict::Refused })
}

fn seal(ctx: &Context, input: &Path, out: &Path, wheel: Option<&str>) -> anyhow::Result<Verdict> {
    let wheel = pick_wheel(ctx, wheel)?;
    let bytes =
        read_seal_input(input, wheel.board.app.max_size).with_context(|| format!("cannot read {}", input.display()))?;
    // Sealing accepts raw bytes only. The pure API rejects ELF magic without flattening it.
    let image = Image::raw(bytes);
    let sealed = match euc_image::seal(&image, &wheel) {
        Ok(sealed) => sealed,
        Err(error) => {
            if ctx.json {
                #[derive(Serialize)]
                struct Refusal<'a> {
                    input: &'a Path,
                    out: &'a Path,
                    error: &'a SealError,
                }
                ui::json_with_profiles(&wheel.origin, &Refusal { input, out, error: &error })?;
            } else {
                if let SealError::InvalidInput { report } | SealError::InvalidOutput { report } = &error {
                    print(input, &wheel, report);
                }
                ui::row(BAD, "REFUSED", &error);
            }
            return Ok(Verdict::Refused);
        }
    };
    write_output(out, sealed.bytes()).with_context(|| format!("cannot create {}", out.display()))?;
    let report = euc_image::validate(&sealed, &wheel);
    if ctx.json {
        #[derive(Serialize)]
        struct Sealed<'a> {
            input: &'a Path,
            #[serde(flatten)]
            output: Output<'a>,
        }
        ui::json_with_profiles(&wheel.origin, &Sealed { input, output: Output { file: out, report: &report } })?;
    } else {
        print(out, &wheel, &report);
        ui::row(GOOD, "SEALED", "Unsigned development image. CRC does not prove authenticity.");
    }
    Ok(Verdict::Ok)
}

/// Read only a regular file, bounded by the selected wheel's application slot.
fn read_seal_input(input: &Path, max_size: u32) -> anyhow::Result<Vec<u8>> {
    // Check before open so ordinary FIFOs and devices cannot block the command.
    if !fs::metadata(input)?.is_file() {
        bail!("sealing requires a regular input file");
    }
    let file = fs::File::open(input)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        bail!("sealing requires a regular input file");
    }
    let limit = u64::from(max_size);
    if metadata.len() > limit {
        bail!("input exceeds the {max_size} byte application slot");
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("input exceeds the {max_size} byte application slot");
    }
    Ok(bytes)
}

/// Publish complete bytes through an exclusive hard link in the destination directory.
/// Unlike rename, `hard_link` never replaces an existing file or symlink. This also refuses
/// input aliases (the original path, symlinks and hard links) because they already exist.
fn write_output(out: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    out.file_name().context("the output path must name a new file")?;
    let parent = out.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or(Path::new("."));
    for _ in 0..128 {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".euc-seal-{}-{sequence}.tmp", std::process::id()));
        let file = fs::OpenOptions::new().write(true).create_new(true).open(&path);
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };
        let pending = PendingOutput { path };
        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        written?;
        fs::hard_link(&pending.path, out)?;
        return Ok(());
    }
    bail!("cannot create a temporary output file")
}

/// Remove the temporary link on success and on errors before publication.
struct PendingOutput {
    path: PathBuf,
}

impl Drop for PendingOutput {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
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
    let profiles = ctx.profiles();
    match profiles.ids()?.as_slice() {
        [only] => Ok(profiles.load(only)?),
        ids => bail!("several wheel profiles ({}); choose one with --wheel", ids.join(", ")),
    }
}

fn print(file: &Path, wheel: &WheelProfile, report: &Report) {
    let size = ui::thousands(report.size as u64);
    ui::row(STRONG, "image", ui::path(file));
    ui::row(STRONG, "wheel", format!("{} ({}), from {}", wheel.name, wheel.id, wheel.origin));
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
