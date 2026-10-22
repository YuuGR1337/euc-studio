//! Single sessions against the simulated wheel, one behaviour each.

use std::time::Duration;

use euc_flash_sim::{Fault, FaultEffect, Phase};
use euc_flasher::sim::{AppFirmware, FaultPlan, OperatorScript, SIM_PEER, Sim, WheelScript, WheelState, images};
use euc_flasher::{
    FlashLog, FlashRecord, GateId, Ledger, MemoryLedger, Outcome, Plan, Status, Target, TransferResult, WheelSeen,
    flash, restore,
};
use euc_image::Image;
use euc_profile::{Sha256, WheelProfile};

struct Bench {
    wheel: WheelProfile,
    stock: Image,
    ours: Image,
}

fn bench() -> Bench {
    let stock = images::synthetic_app(12_000, 11);
    let wheel = images::sim_profile(&[&stock]);
    let ours = images::our_image(&wheel, 20_000, 12, "clean");
    Bench { wheel, stock: Image::raw(stock), ours }
}

fn sim(bench: &Bench, script: WheelScript, operator: OperatorScript, faults: FaultPlan) -> Sim {
    Sim::new(bench.wheel.clone(), &bench.stock, script, operator, faults, 99).unwrap()
}

fn run(bench: &Bench, sim: &mut Sim, image: &Image, ledger: &mut MemoryLedger) -> (euc_flasher::Report, FlashLog) {
    let mut log = FlashLog::in_memory();
    let plan = Plan::new(bench.wheel.clone(), image.clone(), bench.stock.clone(), Target::Simulator);
    let report = flash(&plan, &mut sim.transport, &mut sim.operator, ledger, &mut log);
    (report, log)
}

fn state(sim: &Sim) -> WheelState {
    sim.world.borrow().wheel.state()
}

fn assert_clean_traffic(sim: &Sim, report: &euc_flasher::Report) {
    let world = sim.world.borrow();
    let stats = world.wheel.stats();
    assert_eq!(stats.unexpected_commands, 0, "the application saw command bytes it must not");
    assert!(stats.service_sequences.iter().all(|s| s == "!@"), "{:?}", stats.service_sequences);
    assert_eq!(report.guard_refusals, 0, "the flasher tried a write the guard refused");
}

fn refused(report: &euc_flasher::Report) -> GateId {
    match &report.outcome {
        Outcome::Refused { gate, .. } => *gate,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn stock_flash_succeeds_and_logs_every_byte() {
    let bench = bench();
    let mut sim = sim(&bench, WheelScript::default(), OperatorScript::default(), FaultPlan::none());
    let mut ledger = MemoryLedger::default();
    let (report, log) = run(&bench, &mut sim, &bench.stock, &mut ledger);
    assert_eq!(report.outcome, Outcome::Flashed { sha256: Sha256::digest(bench.stock.bytes()) });
    assert_eq!(state(&sim), WheelState::App { sha256: Sha256::digest(bench.stock.bytes()) });
    assert_clean_traffic(&sim, &report);
    let numbers: Vec<_> = report.gates.iter().filter_map(|g| g.number).collect();
    assert_eq!(numbers, [1, 2, 3, 7, 4, 5, 6, 5], "every gate ran, in order");
    assert_eq!(report.gates.iter().filter(|g| g.status == Status::Skip).count(), 1, "only gate 3 is skipped");
    assert!(matches!(report.transfers[0].result, TransferResult::Delivered { closed: true }));
    assert_eq!(
        ledger.history(SIM_PEER),
        [FlashRecord { peer: SIM_PEER.into(), sha256: Sha256::digest(bench.stock.bytes()), stock: true }]
    );
    let lines = log.lines();
    assert!(lines[0].starts_with(r#"{"type":"start""#));
    assert!(lines.last().unwrap().starts_with(r#"{"type":"outcome""#));
    assert!(lines.iter().any(|l| l.contains(r#""kind":"command","hex":"21""#)), "'!' is logged");
    assert!(lines.iter().any(|l| l.contains(r#""kind":"command","hex":"40""#)), "'@' is logged");
    assert!(lines.iter().filter(|l| l.contains(r#""kind":"ymodem""#)).count() >= 12);
    assert!(lines.iter().all(|l| serde_json::from_str::<serde_json::Value>(l).is_ok()));
    // Twelve 1 KiB blocks over 20-byte writes: about 30 s of simulated time, all of it virtual.
    assert!(
        report.elapsed > Duration::from_secs(20) && report.elapsed < Duration::from_secs(120),
        "{:?}",
        report.elapsed
    );
}

#[test]
fn our_image_needs_a_stock_flash_first() {
    let bench = bench();
    let mut sim = sim(&bench, WheelScript::default(), OperatorScript::default(), FaultPlan::none());
    let mut ledger = MemoryLedger::default();
    let (report, log) = run(&bench, &mut sim, &bench.ours, &mut ledger);
    assert_eq!(refused(&report), GateId::FirstFlashStock);
    assert!(!log.lines().iter().any(|l| l.contains(r#""type":"tx""#)), "a refused offline gate writes nothing");

    let (report, _) = run(&bench, &mut sim, &bench.stock, &mut ledger);
    assert!(matches!(report.outcome, Outcome::Flashed { .. }));
    let (report, _) = run(&bench, &mut sim, &bench.ours, &mut ledger);
    assert_eq!(report.outcome, Outcome::Flashed { sha256: Sha256::digest(bench.ours.bytes()) });
    assert_eq!(state(&sim), WheelState::App { sha256: Sha256::digest(bench.ours.bytes()) });
    assert_clean_traffic(&sim, &report);
}

#[test]
fn clean_firmware_needs_the_power_button() {
    let bench = bench();
    let script = WheelScript { firmware: AppFirmware::Clean, ..WheelScript::default() };
    let mut sim1 = sim(&bench, script.clone(), OperatorScript::default(), FaultPlan::none());
    let (report, _) = run(&bench, &mut sim1, &bench.stock, &mut MemoryLedger::default());
    assert!(matches!(report.outcome, Outcome::Flashed { .. }), "{:?}", report.outcome);
    assert!(sim1.operator.said.iter().any(|s| s.contains("power button")));

    let lazy = OperatorScript { presses_button: false, ..OperatorScript::default() };
    let mut sim2 = sim(&bench, script, lazy, FaultPlan::none());
    let (report, _) = run(&bench, &mut sim2, &bench.stock, &mut MemoryLedger::default());
    assert!(matches!(report.outcome, Outcome::Aborted { .. }), "{:?}", report.outcome);
    assert!(matches!(state(&sim2), WheelState::App { .. }), "the application keeps running");
}

#[test]
fn wheel_state_gate_refuses_motion_alarms_low_battery_and_heat() {
    let bench = bench();
    let cases = [
        WheelScript { moving: Some((Duration::ZERO, Duration::from_secs(600))), ..WheelScript::default() },
        WheelScript { alerts: 0x40, ..WheelScript::default() },
        WheelScript { battery_v: 112.0, ..WheelScript::default() },
        WheelScript { board_c: 64.0, ..WheelScript::default() },
    ];
    for script in cases {
        let mut sim = sim(&bench, script.clone(), OperatorScript::default(), FaultPlan::none());
        let (report, _) = run(&bench, &mut sim, &bench.stock, &mut MemoryLedger::default());
        assert_eq!(refused(&report), GateId::WheelState, "{script:?}");
        assert!(matches!(state(&sim), WheelState::App { .. }));
        assert_clean_traffic(&sim, &report);
    }
    // Moving only while the person types: the second still window catches nothing, the first passes.
    let script =
        WheelScript { moving: Some((Duration::from_secs(9), Duration::from_secs(10))), ..WheelScript::default() };
    let mut sim = sim(&bench, script, OperatorScript::default(), FaultPlan::none());
    let (report, _) = run(&bench, &mut sim, &bench.stock, &mut MemoryLedger::default());
    assert_eq!(refused(&report), GateId::WheelState, "motion right before the entry is caught");
}

#[test]
fn identity_gate_refuses_a_pack_that_is_not_32s() {
    let bench = bench();
    let script = WheelScript { battery_v: 84.0, ..WheelScript::default() };
    let mut sim = sim(&bench, script, OperatorScript::default(), FaultPlan::none());
    let (report, _) = run(&bench, &mut sim, &bench.stock, &mut MemoryLedger::default());
    assert_eq!(refused(&report), GateId::Identity);
}

#[test]
fn a_wrong_confirmation_is_refused() {
    let bench = bench();
    let operator = OperatorScript { types: Some("flash extreme".into()), ..OperatorScript::default() };
    let mut sim = sim(&bench, WheelScript::default(), operator, FaultPlan::none());
    let (report, _) = run(&bench, &mut sim, &bench.stock, &mut MemoryLedger::default());
    assert_eq!(refused(&report), GateId::Confirmation);
}

#[test]
fn image_and_provenance_gates() {
    let bench = bench();
    let mut sim = sim(&bench, WheelScript::default(), OperatorScript::default(), FaultPlan::none());
    let unknown = Image::raw(images::synthetic_app(12_000, 77));
    assert_eq!(refused(&run(&bench, &mut sim, &unknown, &mut MemoryLedger::default()).0), GateId::Provenance);
    let big = Image::raw(images::synthetic_app(50_000, 78));
    assert_eq!(refused(&run(&bench, &mut sim, &big, &mut MemoryLedger::default()).0), GateId::Image);
    let mut unsealed = bench.ours.bytes().to_vec();
    unsealed[0x140 + 0x34..0x140 + 0x3C].fill(0);
    assert_eq!(
        refused(&run(&bench, &mut sim, &Image::raw(unsealed), &mut MemoryLedger::default()).0),
        GateId::Provenance
    );
}

#[test]
fn a_real_wheel_target_is_refused_today() {
    let bench = bench();
    let mut sim = sim(&bench, WheelScript::default(), OperatorScript::default(), FaultPlan::none());
    let mut ledger = MemoryLedger::default();
    for image in [&bench.stock, &bench.ours] {
        let plan = Plan::new(bench.wheel.clone(), image.clone(), bench.stock.clone(), Target::Wheel);
        let mut log = FlashLog::in_memory();
        let report = flash(&plan, &mut sim.transport, &mut sim.operator, &mut ledger, &mut log);
        // The simulator profile's pins are not built in, and our images have no signature.
        assert_eq!(refused(&report), GateId::Provenance, "{:?}", report.gates);
    }
}

#[test]
fn an_ignored_entry_aborts_with_the_application_running() {
    let bench = bench();
    let script = WheelScript { ignores_entry: true, ..WheelScript::default() };
    let mut sim = sim(&bench, script, OperatorScript::default(), FaultPlan::none());
    let (report, _) = run(&bench, &mut sim, &bench.stock, &mut MemoryLedger::default());
    assert!(matches!(report.outcome, Outcome::Aborted { .. }), "{:?}", report.outcome);
    assert!(report.transfers.is_empty(), "no YMODEM to an application");
    assert_clean_traffic(&sim, &report);
}

fn interrupt(phase: Phase, operation: usize) -> Fault {
    Fault { phase, operation, attempt: None, effect: FaultEffect::Interrupt }
}

#[test]
fn power_loss_mid_erase_restores_stock() {
    let bench = bench();
    let faults = FaultPlan { flash: vec![(0, vec![interrupt(Phase::Erase, 5)])], ..FaultPlan::none() };
    let mut sim = sim(&bench, WheelScript::default(), OperatorScript::default(), faults);
    let mut ledger = MemoryLedger::default();
    ledger.record_flash(FlashRecord {
        peer: SIM_PEER.into(),
        sha256: Sha256::digest(bench.stock.bytes()),
        stock: true,
    });
    let (report, _) = run(&bench, &mut sim, &bench.ours, &mut ledger);
    assert!(matches!(report.outcome, Outcome::Restored { .. }), "{:?}", report.outcome);
    assert_eq!(state(&sim), WheelState::App { sha256: Sha256::digest(bench.stock.bytes()) });
    assert_eq!(sim.world.borrow().wheel.stats().commits, ["interrupted", "booted"]);
    assert_clean_traffic(&sim, &report);
}

#[test]
fn power_loss_mid_write_and_mid_transfer_restore_stock() {
    let bench = bench();
    for faults in [
        FaultPlan { flash: vec![(0, vec![interrupt(Phase::Write, 4)])], ..FaultPlan::none() },
        FaultPlan { power_losses: vec![200], ..FaultPlan::none() },
    ] {
        let mut sim = sim(&bench, WheelScript::default(), OperatorScript::default(), faults.clone());
        let (report, _) = run(&bench, &mut sim, &bench.stock, &mut MemoryLedger::default());
        assert!(matches!(report.outcome, Outcome::Restored { .. }), "{faults:?}: {:?}", report.outcome);
        assert_eq!(state(&sim), WheelState::App { sha256: Sha256::digest(bench.stock.bytes()) });
        assert_clean_traffic(&sim, &report);
    }
}

#[test]
fn short_link_loss_and_lost_packets_are_survived() {
    let bench = bench();
    let faults = FaultPlan {
        link_losses: vec![(150, Duration::from_secs(4))],
        drop_writes: vec![60, 61, 300],
        corrupt_writes: vec![90, 400],
        drop_notifications: vec![210, 215],
        corrupt_notifications: vec![220],
        stalls: vec![(225, Duration::from_secs(5))],
        ..FaultPlan::none()
    };
    let mut sim = sim(&bench, WheelScript::default(), OperatorScript::default(), faults);
    let (report, _) = run(&bench, &mut sim, &bench.stock, &mut MemoryLedger::default());
    assert!(matches!(report.outcome, Outcome::Flashed { .. } | Outcome::Restored { .. }), "{:?}", report.outcome);
    assert!(matches!(state(&sim), WheelState::App { .. }));
    assert!(!sim.world.borrow().events.is_empty(), "the faults fired");
    assert_clean_traffic(&sim, &report);
}

#[test]
fn a_wrong_image_on_the_wire_is_refused_by_the_bootloader() {
    let bench = bench();
    let faults = FaultPlan { wrong_block: Some(3), ..FaultPlan::none() };
    let mut sim = sim(&bench, WheelScript::default(), OperatorScript::default(), faults);
    let (report, _) = run(&bench, &mut sim, &bench.stock, &mut MemoryLedger::default());
    assert!(matches!(report.outcome, Outcome::Restored { .. }), "{:?}", report.outcome);
    assert_eq!(sim.world.borrow().wheel.stats().commits, ["rejected", "booted"]);
}

#[test]
fn when_every_restore_fails_the_flasher_stops_and_a_later_restore_works() {
    let bench = bench();
    let corrupt = Fault { phase: Phase::Write, operation: 2, attempt: None, effect: FaultEffect::CorruptWrite };
    let faults = FaultPlan { flash: (0..3).map(|n| (n, vec![corrupt])).collect(), ..FaultPlan::none() };
    let mut sim = sim(&bench, WheelScript::default(), OperatorScript::default(), faults);
    let (report, log) = run(&bench, &mut sim, &bench.stock, &mut MemoryLedger::default());
    assert_eq!(report.outcome, Outcome::Stopped { cause: report_cause(&report), wheel: WheelSeen::Bootloader });
    assert!(log.lines().iter().any(|l| l.contains("swd-guide")));
    assert_eq!(state(&sim), WheelState::Bootloader, "the bootloader still listens: recoverable");

    sim.world.borrow_mut().set_faults(FaultPlan::none());
    let plan = Plan::new(bench.wheel.clone(), bench.stock.clone(), bench.stock.clone(), Target::Simulator);
    let report =
        restore(&plan, &mut sim.transport, &mut sim.operator, &mut MemoryLedger::default(), &mut FlashLog::discard());
    assert!(matches!(report.outcome, Outcome::Restored { .. }), "{:?}", report.outcome);
    assert!(matches!(state(&sim), WheelState::App { .. }));
}

fn report_cause(report: &euc_flasher::Report) -> String {
    match &report.outcome {
        Outcome::Stopped { cause, .. } => cause.clone(),
        other => panic!("expected a stop, got {other:?}"),
    }
}

#[test]
fn reentry_check_flashes_twice() {
    let bench = bench();
    let mut sim = sim(&bench, WheelScript::default(), OperatorScript::default(), FaultPlan::none());
    let mut plan = Plan::new(bench.wheel.clone(), bench.stock.clone(), bench.stock.clone(), Target::Simulator);
    plan.check_reentry = true;
    let report =
        flash(&plan, &mut sim.transport, &mut sim.operator, &mut MemoryLedger::default(), &mut FlashLog::discard());
    assert!(matches!(report.outcome, Outcome::Flashed { .. }));
    assert_eq!(report.reentry, Some(true));
    assert_eq!(report.transfers.len(), 2);
    assert_eq!(sim.world.borrow().wheel.stats().service_sequences, ["!@", "!@"]);
}
