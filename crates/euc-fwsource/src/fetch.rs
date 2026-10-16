//! `euc fetch` and `euc firmware import`: get stock images onto this machine, verified.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use euc_profile::{BegodeBuild, Channel, FirmwareSource, Sha256, WheelProfile};
use serde::Serialize;

use crate::FwError;
use crate::begode::{self, ServerBuild};
use crate::cache::{FirmwareCache, Provenance, Trust};
use crate::http::HttpClient;

/// What `fetch` should do beyond the default (download the current pinned builds once).
#[derive(Debug, Clone, Copy, Default)]
pub struct FetchOptions {
    /// Download again even when the cache already holds an intact copy.
    pub force: bool,
    /// Also fetch builds pinned as `channel = "archive"`.
    pub archive: bool,
    /// Cache server builds that match no pin, marked unverified.
    pub accept_unpinned: bool,
}

/// Everything one `fetch` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FetchReport {
    /// Wheel profile id.
    pub wheel: String,
    /// What the update API listed, per product code.
    pub listings: Vec<Listing>,
    /// Each build handled: the pinned targets first, then server builds no pin knows.
    pub builds: Vec<Fetched>,
}

impl FetchReport {
    /// True when every pinned target is in the cache and verified. An unpinned server build
    /// or an unreachable listing is reported but does not fail the fetch.
    pub fn ok(&self) -> bool {
        self.builds
            .iter()
            .all(|build| !matches!(build.status, FetchStatus::Mismatch { .. } | FetchStatus::Failed { .. }))
    }
}

/// The update API's answer for one product code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listing {
    /// Product code queried.
    pub type_code: String,
    /// The builds listed, or why the listing failed.
    #[serde(flatten)]
    pub result: ListingResult,
}

/// Outcome of one listing query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ListingResult {
    /// The server answered.
    Listed {
        /// Builds offered, newest first.
        builds: Vec<ServerBuild>,
    },
    /// The query failed; pinned builds are still fetched by direct URL.
    Failed {
        /// Why.
        error: String,
    },
}

/// One build `fetch` handled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fetched {
    /// File name on the server.
    pub file: String,
    /// Build name: the pin's label, or the server's title for unpinned builds.
    pub label: String,
    /// Download URL.
    pub url: String,
    /// What happened.
    #[serde(flatten)]
    pub status: FetchStatus,
}

/// What happened to one build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum FetchStatus {
    /// Downloaded now; matched its pin; stored.
    Verified {
        /// Digest (equal to the pin).
        sha256: Sha256,
        /// Cached file.
        path: PathBuf,
    },
    /// Already cached and intact; not downloaded again.
    Cached {
        /// Digest (equal to the pin).
        sha256: Sha256,
        /// Cached file.
        path: PathBuf,
    },
    /// The server offers bytes that match no pin of this wheel.
    Unpinned {
        /// Digest of what the server sent.
        sha256: Sha256,
        /// Where it was cached as unverified, when `accept_unpinned` was set.
        path: Option<PathBuf>,
    },
    /// The download does not match its pin. Nothing was stored.
    Mismatch {
        /// The pinned digest.
        expected: Sha256,
        /// What arrived.
        actual: Sha256,
    },
    /// The download failed.
    Failed {
        /// Why.
        error: String,
    },
}

/// Fetches the wheel's pinned Begode builds into `cache` and reports what the server offers.
///
/// Pinned builds are downloaded by direct URL, so they still arrive when the listing API is down
/// or has moved on. Every build the listing offers that no pin covers is downloaded once to learn
/// its digest; a digest that matches a pin under a new file name is still verified.
///
/// Returns `Err` only when the cache cannot be written; network trouble is part of the report.
pub fn fetch(
    wheel: &WheelProfile,
    http: &dyn HttpClient,
    cache: &FirmwareCache,
    options: FetchOptions,
) -> Result<FetchReport, FwError> {
    let sources: Vec<&BegodeBuild> = wheel
        .firmware
        .iter()
        .filter_map(|source| match source {
            FirmwareSource::BegodeApi(build) => Some(build),
            _ => None,
        })
        .collect();
    let type_codes: BTreeSet<&str> = sources
        .iter()
        .filter(|build| build.channel == Channel::Current)
        .filter_map(|build| build.type_code.as_deref())
        .collect();

    let listings: Vec<Listing> = type_codes
        .into_iter()
        .map(|type_code| Listing {
            type_code: type_code.to_owned(),
            result: match begode::list_builds(http, type_code) {
                Ok(builds) => ListingResult::Listed { builds },
                Err(error) => ListingResult::Failed { error: error.to_string() },
            },
        })
        .collect();

    let mut builds = Vec::new();
    for source in sources.iter().filter(|build| build.channel == Channel::Current || options.archive) {
        builds.push(fetch_pinned(wheel, source, http, cache, options)?);
    }
    for listing in &listings {
        let ListingResult::Listed { builds: offered } = &listing.result else { continue };
        for offer in offered {
            if !sources.iter().any(|source| source.file == offer.file_name()) {
                builds.push(fetch_offer(wheel, offer, http, cache, options)?);
            }
        }
    }
    Ok(FetchReport { wheel: wheel.id.clone(), listings, builds })
}

fn fetch_pinned(
    wheel: &WheelProfile,
    build: &BegodeBuild,
    http: &dyn HttpClient,
    cache: &FirmwareCache,
    options: FetchOptions,
) -> Result<Fetched, FwError> {
    let url = begode::file_url(&build.file);
    let fetched = |status| Fetched { file: build.file.clone(), label: build.label.clone(), url: url.clone(), status };
    if !options.force
        && let Some(path) = cache.verified(&build.sha256)
    {
        return Ok(fetched(FetchStatus::Cached { sha256: build.sha256, path }));
    }
    let bytes = match http.get(&url) {
        Ok(bytes) => bytes,
        Err(error) => return Ok(fetched(FetchStatus::Failed { error: error.to_string() })),
    };
    let actual = Sha256::digest(&bytes);
    if actual != build.sha256 {
        return Ok(fetched(FetchStatus::Mismatch { expected: build.sha256, actual }));
    }
    let provenance = Provenance { wheel: wheel.id.clone(), label: build.label.clone(), origin: url.clone() };
    let path = cache.store(&bytes, Trust::Verified, &provenance)?;
    Ok(fetched(FetchStatus::Verified { sha256: actual, path }))
}

fn fetch_offer(
    wheel: &WheelProfile,
    offer: &ServerBuild,
    http: &dyn HttpClient,
    cache: &FirmwareCache,
    options: FetchOptions,
) -> Result<Fetched, FwError> {
    let url = offer.url();
    let fetched = |label: &str, status| Fetched {
        file: offer.file_name().to_owned(),
        label: label.to_owned(),
        url: url.clone(),
        status,
    };
    let bytes = match http.get(&url) {
        Ok(bytes) => bytes,
        Err(error) => return Ok(fetched(&offer.title, FetchStatus::Failed { error: error.to_string() })),
    };
    let sha256 = Sha256::digest(&bytes);
    if let Some(pin) = wheel.find_pin(&sha256) {
        let provenance = Provenance { wheel: wheel.id.clone(), label: pin.label.to_owned(), origin: url.clone() };
        let path = cache.store(&bytes, Trust::Verified, &provenance)?;
        return Ok(fetched(pin.label, FetchStatus::Verified { sha256, path }));
    }
    let path = if options.accept_unpinned {
        let provenance = Provenance { wheel: wheel.id.clone(), label: offer.title.clone(), origin: url.clone() };
        Some(cache.store(&bytes, Trust::Unverified, &provenance)?)
    } else {
        None
    };
    Ok(fetched(&offer.title, FetchStatus::Unpinned { sha256, path }))
}

/// Result of importing a local file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Imported {
    /// Digest of the file.
    pub sha256: Sha256,
    /// What happened.
    #[serde(flatten)]
    pub status: ImportStatus,
}

/// What happened to an imported file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ImportStatus {
    /// Matched a pin; stored as verified.
    Verified {
        /// The pin's label.
        label: String,
        /// Cached file.
        path: PathBuf,
    },
    /// Matched no pin; stored as unverified on request.
    Unverified {
        /// Cached file.
        path: PathBuf,
    },
    /// Matched no pin; not stored.
    Refused,
}

/// Imports a firmware file the user already has, checking it against the wheel's pins.
pub fn import(
    wheel: &WheelProfile,
    cache: &FirmwareCache,
    bytes: &[u8],
    origin: &Path,
    accept_unpinned: bool,
) -> Result<Imported, FwError> {
    let sha256 = Sha256::digest(bytes);
    let provenance = |label: &str| Provenance {
        wheel: wheel.id.clone(),
        label: label.to_owned(),
        origin: origin.display().to_string(),
    };
    let status = if let Some(pin) = wheel.find_pin(&sha256) {
        let path = cache.store(bytes, Trust::Verified, &provenance(pin.label))?;
        ImportStatus::Verified { label: pin.label.to_owned(), path }
    } else if accept_unpinned {
        let label = origin.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        ImportStatus::Unverified { path: cache.store(bytes, Trust::Unverified, &provenance(&label))? }
    } else {
        ImportStatus::Refused
    };
    Ok(Imported { sha256, status })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeHttp;

    const CURRENT: &[u8] = b"synthetic current build";
    const ARCHIVE: &[u8] = b"synthetic archive build";
    const NEWER: &[u8] = b"synthetic newer build";

    fn wheel() -> WheelProfile {
        WheelProfile::parse(&format!(
            r#"
schema = 1
id = "demo"
name = "Demo"
brand = "Demo"

[board]
id = "demo"
mcu = "STM32F103RC-class"
core = "Cortex-M3"
hclk_hz = 72_000_000
app = {{ base = 0x0800_3000, max_size = 49_152, vectors = 76, info_offset = 0x140 }}
ram = {{ base = 0x2000_0000, size = 0xC000 }}

[[firmware]]
kind = "begode-api"
type_code = "TEST01"
file = "pwm_2000.bin"
sha256 = "{}"
label = "FW2"

[[firmware]]
kind = "begode-api"
channel = "archive"
file = "pwm_1000.bin"
sha256 = "{}"
label = "FW1"
"#,
            Sha256::digest(CURRENT),
            Sha256::digest(ARCHIVE)
        ))
        .unwrap()
    }

    fn listing(files: &[&str]) -> Vec<u8> {
        let records: Vec<String> = files
            .iter()
            .map(|f| format!(r#"{{"apkWrap":"temp/{f}","briefIntroduction":"DEMO_{f}","perfectBug":"notes"}}"#))
            .collect();
        format!(r#"{{"code":0,"data":{{"records":[{}]}}}}"#, records.join(",")).into_bytes()
    }

    fn server(files: &[&str]) -> FakeHttp {
        FakeHttp::default()
            .with(&begode::listing_url("TEST01"), &listing(files))
            .with(&begode::file_url("pwm_2000.bin"), CURRENT)
            .with(&begode::file_url("pwm_1000.bin"), ARCHIVE)
            .with(&begode::file_url("pwm_3000.bin"), NEWER)
    }

    fn statuses(report: &FetchReport) -> Vec<(&str, &'static str)> {
        report
            .builds
            .iter()
            .map(|b| {
                let status = match &b.status {
                    FetchStatus::Verified { .. } => "verified",
                    FetchStatus::Cached { .. } => "cached",
                    FetchStatus::Unpinned { path: Some(_), .. } => "unpinned-cached",
                    FetchStatus::Unpinned { path: None, .. } => "unpinned",
                    FetchStatus::Mismatch { .. } => "mismatch",
                    FetchStatus::Failed { .. } => "failed",
                };
                (b.file.as_str(), status)
            })
            .collect()
    }

    #[test]
    fn fetches_the_current_pin_then_uses_the_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = FirmwareCache::new(tmp.path());
        let http = server(&["pwm_2000.bin"]);

        let first = fetch(&wheel(), &http, &cache, FetchOptions::default()).unwrap();
        assert!(first.ok());
        assert_eq!(statuses(&first), [("pwm_2000.bin", "verified")]);
        assert!(cache.verified(&Sha256::digest(CURRENT)).is_some());

        let again = fetch(&wheel(), &http, &cache, FetchOptions::default()).unwrap();
        assert_eq!(statuses(&again), [("pwm_2000.bin", "cached")]);
        let downloads = http.requests().iter().filter(|url| **url == begode::file_url("pwm_2000.bin")).count();
        assert_eq!(downloads, 1, "the second run must not download again");

        let forced = fetch(&wheel(), &http, &cache, FetchOptions { force: true, ..FetchOptions::default() }).unwrap();
        assert_eq!(statuses(&forced), [("pwm_2000.bin", "verified")]);
    }

    #[test]
    fn archive_builds_on_request() {
        let tmp = tempfile::tempdir().unwrap();
        let options = FetchOptions { archive: true, ..FetchOptions::default() };
        let report = fetch(&wheel(), &server(&["pwm_2000.bin"]), &FirmwareCache::new(tmp.path()), options).unwrap();
        assert_eq!(statuses(&report), [("pwm_2000.bin", "verified"), ("pwm_1000.bin", "verified")]);
    }

    #[test]
    fn unpinned_server_build_is_reported_not_trusted() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = FirmwareCache::new(tmp.path());
        let http = server(&["pwm_3000.bin"]);

        let report = fetch(&wheel(), &http, &cache, FetchOptions::default()).unwrap();
        assert!(report.ok(), "the pinned build still arrives by direct URL");
        assert_eq!(statuses(&report), [("pwm_2000.bin", "verified"), ("pwm_3000.bin", "unpinned")]);
        assert_eq!(report.builds[1].label, "DEMO_pwm_3000.bin");
        assert_eq!(cache.list().unwrap().len(), 1, "unpinned bytes are not cached by default");

        let options = FetchOptions { accept_unpinned: true, ..FetchOptions::default() };
        let report = fetch(&wheel(), &http, &cache, options).unwrap();
        assert_eq!(statuses(&report)[1], ("pwm_3000.bin", "unpinned-cached"));
        let listed = cache.list().unwrap();
        assert_eq!(listed.iter().filter(|i| i.trust == Trust::Unverified).count(), 1);
    }

    #[test]
    fn renamed_pinned_bytes_are_still_verified() {
        let tmp = tempfile::tempdir().unwrap();
        let http = server(&["pwm_4000.bin"]).with(&begode::file_url("pwm_4000.bin"), ARCHIVE);
        let report = fetch(&wheel(), &http, &FirmwareCache::new(tmp.path()), FetchOptions::default()).unwrap();
        assert_eq!(statuses(&report)[1], ("pwm_4000.bin", "verified"));
        assert_eq!(report.builds[1].label, "FW1");
    }

    #[test]
    fn mismatch_is_an_error_and_stores_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = FirmwareCache::new(tmp.path());
        let http = server(&["pwm_2000.bin"]).with(&begode::file_url("pwm_2000.bin"), b"tampered");
        let report = fetch(&wheel(), &http, &cache, FetchOptions::default()).unwrap();
        assert!(!report.ok());
        assert_eq!(
            report.builds[0].status,
            FetchStatus::Mismatch { expected: Sha256::digest(CURRENT), actual: Sha256::digest(b"tampered") }
        );
        assert_eq!(cache.list().unwrap(), []);
    }

    #[test]
    fn listing_failure_still_fetches_pins() {
        let tmp = tempfile::tempdir().unwrap();
        let http = FakeHttp::default().with(&begode::file_url("pwm_2000.bin"), CURRENT);
        let report = fetch(&wheel(), &http, &FirmwareCache::new(tmp.path()), FetchOptions::default()).unwrap();
        assert!(report.ok());
        assert!(matches!(report.listings[0].result, ListingResult::Failed { .. }));
        assert_eq!(statuses(&report), [("pwm_2000.bin", "verified")]);
    }

    #[test]
    fn download_failure_fails_the_fetch() {
        let tmp = tempfile::tempdir().unwrap();
        let http = FakeHttp::default().with(&begode::listing_url("TEST01"), &listing(&[]));
        let report = fetch(&wheel(), &http, &FirmwareCache::new(tmp.path()), FetchOptions::default()).unwrap();
        assert!(!report.ok());
        assert_eq!(statuses(&report), [("pwm_2000.bin", "failed")]);
    }

    #[test]
    fn report_json_is_flat() {
        let tmp = tempfile::tempdir().unwrap();
        let report =
            fetch(&wheel(), &server(&["pwm_2000.bin"]), &FirmwareCache::new(tmp.path()), FetchOptions::default())
                .unwrap();
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["listings"][0]["status"], "listed");
        assert_eq!(json["builds"][0]["status"], "verified");
        assert_eq!(json["builds"][0]["sha256"], Sha256::digest(CURRENT).to_string());
    }

    #[test]
    fn import_checks_pins() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = FirmwareCache::new(tmp.path());
        let origin = Path::new("/downloads/fw.bin");

        let imported = import(&wheel(), &cache, ARCHIVE, origin, false).unwrap();
        assert!(matches!(&imported.status, ImportStatus::Verified { label, .. } if label == "FW1"));
        assert_eq!(cache.list().unwrap()[0].provenance.as_ref().unwrap().origin, "/downloads/fw.bin");

        assert_eq!(import(&wheel(), &cache, b"mystery", origin, false).unwrap().status, ImportStatus::Refused);
        let kept = import(&wheel(), &cache, b"mystery", origin, true).unwrap();
        assert!(matches!(kept.status, ImportStatus::Unverified { .. }));
    }
}
