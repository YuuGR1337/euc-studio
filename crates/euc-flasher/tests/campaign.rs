//! M6 exit check: hundreds of simulated flashes with injected faults all end in success or a
//! recoverable state. Print the distribution with `cargo test -p euc-flasher --test campaign --
//! --nocapture`.

use euc_flasher::campaign::{self, Verdict};

const SEED: u64 = 0x5EED_2026_1022;
const RUNS: usize = 320;

#[test]
fn every_faulty_flash_ends_in_success_or_a_recoverable_state() {
    let report = campaign::run(SEED, RUNS);
    println!("{}", report.table());
    let bad: Vec<_> = report.unrecoverable().collect();
    assert!(bad.is_empty(), "{} unrecoverable runs: {bad:#?}", bad.len());
    assert_eq!(report.runs.len(), RUNS);

    // The campaign exercised what it claims to: every fault kind ran, and most of them fired.
    let kinds = [
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
    for kind in kinds {
        assert!(report.by_fault.contains_key(kind), "no run injected {kind}");
    }
    let fired = report.runs.iter().filter(|run| !run.fired.is_empty()).count();
    assert!(fired >= RUNS / 4, "only {fired} runs had a link or power fault fire");
    for verdict in [Verdict::Success, Verdict::Recovered, Verdict::SafeStop] {
        assert!(report.runs.iter().any(|run| run.verdict == verdict), "no {verdict:?} run");
    }
    // Clean runs flash.
    let clean: Vec<_> =
        report.runs.iter().filter(|run| run.faults.is_empty() && run.image.starts_with("stock")).collect();
    assert!(!clean.is_empty() && clean.iter().all(|run| run.verdict == Verdict::Success), "{clean:#?}");
}

#[test]
fn runs_are_reproducible_from_their_seed() {
    for index in [0, 7, 21] {
        let a = campaign::run_one(index, SEED + index as u64);
        let b = campaign::run_one(index, SEED + index as u64);
        assert_eq!((a.verdict, &a.detail, &a.fired), (b.verdict, &b.detail, &b.fired));
    }
}
