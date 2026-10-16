//! Talks to Begode's real update server. Ignored by default; run with
//! `cargo test -p euc-fwsource --test live -- --ignored`.

use euc_fwsource::{FetchOptions, FetchStatus, FirmwareCache, UreqClient, begode, fetch};
use euc_profile::{Channel, FirmwareSource, ProfileSource};

#[test]
#[ignore = "needs network access to one-api.begode.com"]
fn current_begode_extreme_build_matches_its_pin() {
    let wheel = ProfileSource::Builtin.load("begode-extreme").unwrap();
    let http = UreqClient::new();

    let current = wheel
        .firmware
        .iter()
        .find_map(|source| match source {
            FirmwareSource::BegodeApi(build) if build.channel == Channel::Current => Some(build),
            _ => None,
        })
        .unwrap();
    let listed = begode::list_builds(&http, current.type_code.as_deref().unwrap()).unwrap();
    assert!(!listed.is_empty(), "the API lists no build for {:?}", current.type_code);

    let tmp = tempfile::tempdir().unwrap();
    let report = fetch(&wheel, &http, &FirmwareCache::new(tmp.path()), FetchOptions::default()).unwrap();
    assert!(report.ok(), "{report:#?}");
    assert!(matches!(&report.builds[0].status, FetchStatus::Verified { sha256, .. } if *sha256 == current.sha256));
}
