//! `euc firmware import` and `euc firmware list`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use euc_fwsource::{CachedImage, ImportStatus, Trust};
use serde::Serialize;

use crate::ui::{self, BAD, DIM, GOOD, STRONG, WARN};
use crate::{Context, Verdict};

#[derive(Debug, clap::Subcommand)]
pub(crate) enum Command {
    /// Copy a firmware file you already have into the cache, checked against the wheel's pins.
    Import {
        /// Wheel profile id, e.g. begode-extreme.
        wheel: String,
        /// The firmware file (.bin).
        file: PathBuf,
        /// Keep a file that matches no pin, marked unverified.
        #[arg(long)]
        accept_unpinned: bool,
    },
    /// List the cached firmware images.
    List,
}

pub(crate) fn run(ctx: &Context, command: Command) -> anyhow::Result<Verdict> {
    match command {
        Command::Import { wheel, file, accept_unpinned } => import(ctx, &wheel, &file, accept_unpinned),
        Command::List => list(ctx),
    }
}

fn import(ctx: &Context, wheel_id: &str, file: &Path, accept_unpinned: bool) -> anyhow::Result<Verdict> {
    let wheel = ctx.wheel(wheel_id)?;
    let cache = ctx.cache()?;
    let bytes = fs::read(file).with_context(|| format!("cannot read {}", file.display()))?;
    let imported = euc_fwsource::import(&wheel, &cache, &bytes, file, accept_unpinned)?;
    if ctx.json {
        ui::json(&imported)?;
    } else {
        match &imported.status {
            ImportStatus::Verified { label, path } => {
                ui::row(GOOD, "verified", format!("{}  pinned build of {}", ui::paint(STRONG, label), wheel.name));
                ui::detail(format!("sha256 {}", imported.sha256));
                ui::detail(ui::paint(DIM, path.display()));
            }
            ImportStatus::Unverified { path } => {
                ui::row(
                    WARN,
                    "kept",
                    format!("{} matches no pin of {}; cached as unverified", file.display(), wheel.name),
                );
                ui::detail(format!("sha256 {}", imported.sha256));
                ui::detail(ui::paint(DIM, path.display()));
            }
            ImportStatus::Refused => {
                ui::row(BAD, "refused", format!("{} is not a pinned build of {}", file.display(), wheel.name));
                ui::detail(format!("sha256 {}", imported.sha256));
                ui::detail(ui::paint(DIM, "pass --accept-unpinned to keep it as unverified"));
            }
        }
    }
    Ok(if imported.status == ImportStatus::Refused { Verdict::Refused } else { Verdict::Ok })
}

#[derive(Serialize)]
struct Listing<'a> {
    root: &'a Path,
    images: &'a [CachedImage],
}

fn list(ctx: &Context) -> anyhow::Result<Verdict> {
    let cache = ctx.cache()?;
    let images = cache.list()?;
    if ctx.json {
        ui::json(&Listing { root: cache.root(), images: &images })?;
        return Ok(Verdict::Ok);
    }
    if images.is_empty() {
        println!("no cached firmware in {}; run `euc fetch <wheel>`", cache.root().display());
        return Ok(Verdict::Ok);
    }
    let label_width = images.iter().map(|image| label(image).len()).max().unwrap_or(0);
    for image in &images {
        let (style, word) = match (image.intact, image.trust) {
            (false, _) => (BAD, "damaged"),
            (true, Trust::Verified) => (GOOD, "verified"),
            (true, Trust::Unverified) => (WARN, "unverified"),
        };
        let wheel = image.provenance.as_ref().map_or("?", |p| p.wheel.as_str());
        ui::row(
            style,
            word,
            format!(
                "{:label_width$}  {wheel}  {:>7} B  {}",
                label(image),
                ui::thousands(image.size),
                image.sha256.short()
            ),
        );
        ui::detail(ui::paint(DIM, image.path.display()));
    }
    Ok(Verdict::Ok)
}

fn label(image: &CachedImage) -> &str {
    image.provenance.as_ref().map_or("(no sidecar)", |p| p.label.as_str())
}
