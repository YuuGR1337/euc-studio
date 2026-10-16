//! `euc fetch <wheel>`.

use euc_fwsource::{FetchOptions, FetchReport, FetchStatus, ListingResult, UreqClient};

use crate::ui::{self, BAD, DIM, GOOD, STRONG, WARN};
use crate::{Context, Verdict};

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Wheel profile id, e.g. begode-extreme.
    wheel: String,
    /// Download again even if the cache holds an intact copy.
    #[arg(long)]
    force: bool,
    /// Also fetch older builds pinned as archive.
    #[arg(long)]
    archive: bool,
    /// Cache server builds that match no pin, marked unverified.
    #[arg(long)]
    accept_unpinned: bool,
}

pub(crate) fn run(ctx: &Context, args: &Args) -> anyhow::Result<Verdict> {
    let wheel = ctx.wheel(&args.wheel)?;
    let cache = ctx.cache()?;
    let options = FetchOptions { force: args.force, archive: args.archive, accept_unpinned: args.accept_unpinned };
    let report = euc_fwsource::fetch(&wheel, &UreqClient::new(), &cache, options)?;
    if ctx.json {
        ui::json_with_profiles(&wheel.origin, &report)?;
    } else {
        print(&report, &wheel.name);
    }
    Ok(if report.ok() { Verdict::Ok } else { Verdict::Refused })
}

fn print(report: &FetchReport, name: &str) {
    ui::row(STRONG, "wheel", format!("{name} ({})", report.wheel));
    for listing in &report.listings {
        match &listing.result {
            ListingResult::Listed { builds } if builds.is_empty() => {
                ui::row(WARN, "listing", format!("{} lists no build", listing.type_code));
            }
            ListingResult::Listed { builds } => {
                for build in builds {
                    let about = ui::paint(DIM, format!("{} · {}", build.title, build.notes));
                    ui::row(DIM, "listing", format!("{} offers {}  {about}", listing.type_code, build.file_name()));
                }
            }
            ListingResult::Failed { error } => {
                ui::row(
                    WARN,
                    "listing",
                    format!("{} unavailable; pinned builds fetched by direct URL", listing.type_code),
                );
                ui::detail(ui::paint(DIM, error));
            }
        }
    }
    for build in &report.builds {
        let what = format!("{}  {}", ui::paint(STRONG, &build.label), build.file);
        match &build.status {
            FetchStatus::Verified { sha256, path } | FetchStatus::Cached { sha256, path } => {
                let word = if matches!(build.status, FetchStatus::Verified { .. }) { "verified" } else { "cached" };
                ui::row(GOOD, word, what);
                ui::detail(format!("sha256 {sha256} {}", ui::paint(GOOD, "matches the pin")));
                ui::detail(ui::paint(DIM, ui::path(path)));
            }
            FetchStatus::Unpinned { sha256, path } => {
                ui::row(WARN, "unpinned", what);
                ui::detail(format!("sha256 {sha256} matches no pin of this wheel"));
                match path {
                    Some(path) => ui::detail(format!("kept as unverified: {}", ui::path(path))),
                    None => ui::detail(ui::paint(
                        DIM,
                        "not cached; review it and pin it in the wheel profile, or pass --accept-unpinned",
                    )),
                }
            }
            FetchStatus::Mismatch { expected, actual } => {
                ui::row(BAD, "mismatch", what);
                ui::detail(format!("expected {expected}"));
                ui::detail(format!("received {actual}; nothing was cached"));
            }
            FetchStatus::Failed { error } => {
                ui::row(BAD, "failed", what);
                ui::detail(error);
            }
        }
    }
}
