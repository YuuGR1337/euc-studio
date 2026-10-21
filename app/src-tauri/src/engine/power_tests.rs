//! Explicit local-stock integration checks for the controller supply domain.
//! Registered as a child of `host`, so production APIs need no test-only accessors.

use super::{Host, RideHost};
use euc_devices::{Buzzer, Tm1640};
use euc_profile::{ProfileSource, Sha256};
use euc_sim::scenario::Scenario;
use euc_sim::{Machine, MachineConfig, PlantState, Session, WheelFiles};

fn verify_stock_identity(files: &WheelFiles, image: &[u8]) -> Result<(), String> {
    let digest = Sha256::digest(image);
    match files.profile.find_pin(&digest) {
        Some(pin) if pin.label == files.stockmap.build => Ok(()),
        _ => Err(format!("Image {digest} does not match the profile pin for stock map {}", files.stockmap.build)),
    }
}

#[test]
fn stock_power_fixture_rejects_unpinned_images_before_using_stock_map() {
    let files = WheelFiles::load(&ProfileSource::Builtin, "begode-extreme").unwrap();
    assert!(verify_stock_identity(&files, b"not the pinned stock firmware").is_err());
}

fn stock_host(quantum: u32) -> RideHost {
    let path = std::env::var_os("EUC_TEST_STOCK_IMAGE")
        .expect("Set EUC_TEST_STOCK_IMAGE to the explicit local pinned stock image; no download occurs");
    let image = std::fs::read(path).expect("Read the explicit local stock image");
    let files = WheelFiles::load(&ProfileSource::Builtin, "begode-extreme").unwrap();
    verify_stock_identity(&files, &image).expect("Verify stock identity before applying its map");
    let board = files.board_with_settings("neutral").unwrap();
    let config = MachineConfig {
        board: &board,
        stockmap: Some(&files.stockmap),
        image: &image,
        load_addr: files.profile.board.app.base,
        hclk: files.hclk(),
        plant: PlantState::default(),
        injections: vec![],
        power_cycles: vec![],
        cpi: euc_sim::DEFAULT_CPI,
        exception_timing: euc_sim::ExceptionTiming::default(),
        max_quantum: quantum,
        marker_window_s: 4.0,
    };
    let live = files.live(Default::default(), Default::default()).unwrap();
    let machine =
        Machine::new_live(&config, &euc_sim::registry(), euc_sim::cpu::Backend::Own.factory().unwrap(), live).unwrap();
    let session = Session::new(machine, Some(&files.stockmap), 1000.0);
    let scenario = Scenario::from_toml(
        r#"
schema = 1
name = "power-supply-integration"
description = "Keep the live world on its stand while the real stock firmware powers off and on."
wheel = "begode-extreme"
seconds = 90.0
release_s = 89.0
speed_kmh = [[0, 0]]
"#,
    )
    .unwrap();
    RideHost::new(session, scenario).unwrap()
}

fn step(host: &mut RideHost, seconds: f64) {
    for _ in 0..(seconds * 1000.0).round() as usize {
        host.step(0.001);
        assert!(host.halted().is_none(), "{:?}", host.halted());
    }
}

/// Run only against an explicitly supplied local image, never a fetch or physical wheel.
#[test]
#[ignore = "needs EUC_TEST_STOCK_IMAGE and runs real stock off/wake at three quanta"]
fn stock_power_off_wake_preserves_live_world_and_host_outputs() {
    for quantum in [4000, 3000, 2000] {
        let mut host = stock_host(quantum);
        step(&mut host, 4.0);
        assert!(host.buttons_ready());
        assert!(host.power_button_ready());
        assert_eq!(host.outputs().unwrap().controller_powered, Some(true));
        assert!(host.outputs().unwrap().display_on);

        host.set_button("power", true).unwrap();
        let start = host.time_s();
        while host.outputs().unwrap().power_latched != Some(false) && host.time_s() - start < 3.0 {
            step(&mut host, 0.001);
        }
        assert_eq!(host.outputs().unwrap().power_latched, Some(false), "stock must actually drop PB14");
        let latch_at = host.time_s();
        assert_eq!(host.outputs().unwrap().controller_powered, Some(true), "the held Power contact sustains the rail");
        assert!(host.power_button_ready(), "a held input must remain renewable after latch drop");
        assert!(!host.buttons_ready());
        assert!(host.set_button("handle", true).is_err());
        host.set_button("power", true).unwrap();
        step(&mut host, 0.1);
        assert!(host.session.machine().powered());

        host.set_button("power", false).unwrap();
        let off_at = host.time_s();
        let off = host.outputs().unwrap();
        assert_eq!(off.controller_powered, Some(false));
        assert!(!off.display_on);
        assert_eq!(off.display_brightness, 0);
        assert_eq!(off.buzzer_hz, 0);
        // With no controller supply, PB12 releases; only effective light output is zero.
        assert_eq!(off.headlight_pin_high, None);
        assert_eq!(off.headlight_duty, Some(0.0));
        assert!(off.leds.iter().all(|rgb| *rgb == [0, 0, 0]));
        let machine = host.session.machine();
        assert!(!machine.component::<Tm1640>("display").unwrap().display_on());
        assert!(machine.component::<Buzzer>("buzzer").unwrap().tone().is_none());
        let retired = machine.cpu().retired();
        let isr = machine.isr_cycles();
        let physics_time = machine.plant().live_part().unwrap().physics().time_s();
        let buzzer_changes = machine.component::<Buzzer>("buzzer").unwrap().changes();
        step(&mut host, 0.5);
        let machine = host.session.machine();
        assert_eq!(machine.cpu().retired(), retired, "no powered-off CPU instructions");
        assert_eq!(machine.isr_cycles(), isr, "no powered-off handlers");
        assert!(host.time_s() >= off_at + 0.49);
        assert!(machine.plant().live_part().unwrap().physics().time_s() >= physics_time + 0.49);
        assert_eq!(machine.component::<Buzzer>("buzzer").unwrap().changes(), buzzer_changes);
        assert_eq!(host.firmware_value("vq_active"), None, "stale firmware RAM is unavailable while off");
        assert_eq!(host.session.cutoff_flags(), vec!["controller_supply_off"]);
        assert!(host.set_button("handle", true).is_err());
        assert!(!host.buttons_ready());
        assert!(host.power_button_ready());

        let world = host.session.machine().plant().live_part().unwrap().physics().state();
        let clock = host.time_s();
        host.set_button("power", true).unwrap();
        assert_eq!(host.outputs().unwrap().controller_powered, Some(true));
        let wake_world = host.session.machine().plant().live_part().unwrap().physics().state();
        assert_eq!(wake_world.time_s, world.time_s);
        assert_eq!(wake_world.speed_mps, world.speed_mps);
        assert_eq!(wake_world.pitch_rad, world.pitch_rad);
        assert_eq!(wake_world.distance_m, world.distance_m);
        assert_eq!(host.time_s(), clock, "contact down does not move the world or clock");
        assert!(host.power_button_ready(), "the held wake input can renew during boot");
        assert!(!host.buttons_ready());
        let wake_at = host.time_s();
        while host.outputs().unwrap().power_latched != Some(true) && host.time_s() - wake_at < 3.0 {
            step(&mut host, 0.001);
        }
        assert_eq!(host.outputs().unwrap().power_latched, Some(true));
        step(&mut host, 0.15);
        let gpio = host.session.machine().component::<euc_stm32f1::Gpio>("GPIOB").unwrap();
        assert_eq!(gpio.idr() & (1 << 15), 0, "no synthetic release of the user's physical wake hold");
        host.set_button("power", false).unwrap();
        assert!(host.session.machine().powered(), "the new firmware latch now holds the rail");
        step(&mut host, 3.0);
        assert_eq!(host.outputs().unwrap().controller_powered, Some(true));
        assert!(host.outputs().unwrap().display_on, "firmware initializes its display again");
        assert!(host.session.machine().component::<Tm1640>("display").unwrap().frames() > 0);
        assert!(host.session.machine().cpu().retired() > 0);
        assert!(host.session.machine().plant().live_part().unwrap().physics().time_s() > physics_time);
        assert!(host.buttons_ready());
        println!(
            "quantum {quantum}: latch low {latch_at:.6} s; rail off {off_at:.6} s; wake {wake_at:.6} s; world preserved"
        );
    }
}

fn gpio_halls(machine: &Machine) -> u8 {
    let idr = machine.component::<euc_stm32f1::Gpio>("GPIOB").unwrap().idr();
    (((idr >> 7) & 1) | (((idr >> 6) & 1) << 1) | (((idr >> 8) & 1) << 2)) as u8
}

// Include the complete accepted physical and electrical snapshot. A zero-time contact
// change may reset controller registers, never pose, velocity, rotor, current, SOC, or heat.
fn physical_state(host: &RideHost) -> impl std::fmt::Debug + PartialEq + use<> {
    let live = host.session.machine().plant().live_part().unwrap();
    (
        host.time_s(),
        live.physics().state(),
        live.physics().poses(),
        live.physics().wheel(),
        live.electrical().snapshot(),
    )
}

fn current_magnitude(host: &RideHost) -> f64 {
    host.session
        .machine()
        .plant()
        .live_part()
        .unwrap()
        .electrical()
        .snapshot()
        .phase_current
        .iter()
        .map(|i| i * i)
        .sum::<f64>()
        .sqrt()
}

#[test]
#[ignore = "needs EUC_TEST_STOCK_IMAGE and real moving off/wake checks at three quanta"]
fn stock_power_wake_restores_current_halls_before_any_instruction() {
    for quantum in [4000, 3000, 2000] {
        let mut host = stock_host(quantum);
        step(&mut host, 4.0);
        host.session.set_held(false);
        // Target half the firmware shutdown gate (0.5 m/s), not a commanded motor speed.
        // Only the rider request changes; no pose, velocity, current, or motor torque is set.
        for _ in 0..3000 {
            host.session.set_intent(euc_physics::Intent::Speed(0.25));
            host.session.step();
        }
        let moving_speed = host.session.machine().plant().live_part().unwrap().physics().state().speed_mps;
        assert!(moving_speed.abs() > 0.005, "the world must move before off: {moving_speed}");
        host.set_button("power", true).unwrap();
        let start = host.time_s();
        while host.outputs().unwrap().power_latched != Some(false) && host.time_s() - start < 3.0 {
            host.session.set_intent(euc_physics::Intent::Speed(0.25));
            host.session.step();
        }
        assert_eq!(host.outputs().unwrap().power_latched, Some(false));
        let before_off = physical_state(&host);
        host.set_button("power", false).unwrap();
        assert_eq!(physical_state(&host), before_off, "zero-time off must preserve the full moving world");
        let initial_gpio = gpio_halls(host.session.machine());
        let retired = host.session.machine().cpu().retired();
        let isr = host.session.machine().isr_cycles();
        let initial_current = current_magnitude(&host);
        let mut max_current = initial_current;
        let angle_start = host.session.machine().plant().live_part().unwrap().physics().wheel().0;
        let physics_start = host.session.machine().plant().live_part().unwrap().physics().time_s();
        let mut previous_halls = host.session.machine().plant().halls();
        let mut transitions = 0;
        for _ in 0..4000 {
            host.session.set_intent(euc_physics::Intent::Lean(15_f64.to_radians()));
            host.session.step();
            max_current = max_current.max(current_magnitude(&host));
            let timer = host.session.machine().component::<euc_stm32f1::Timer>("TIM8").unwrap();
            for channel in 1..=3 {
                assert_eq!(timer.output(channel), euc_core::Signal::Released, "no retained PWM drive");
            }
            let halls = host.session.machine().plant().halls();
            if halls != previous_halls {
                transitions += 1;
                previous_halls = halls;
            }
            if transitions >= 4
                && halls != initial_gpio
                && host.session.machine().plant().live_part().unwrap().physics().time_s() > physics_start + 0.05
            {
                break;
            }
        }
        let machine = host.session.machine();
        assert!(!machine.powered());
        assert_eq!(machine.cpu().retired(), retired);
        assert_eq!(machine.isr_cycles(), isr);
        assert!(transitions >= 4, "the unpowered rotor must cross several Hall sectors: {transitions}");
        assert!(machine.plant().live_part().unwrap().physics().time_s() > physics_start + 0.05);
        // Normal firmware shutdown disables PWM before saving settings and dropping the latch.
        // The separate plant supply-loss test covers an energized inductor at rail removal.
        assert_eq!(initial_current, 0.0, "stock already drains the current before latch release");
        assert_eq!(max_current, 0.0, "no PWM drive reappears during the unpowered low-speed motion");
        let expected = machine.plant().halls();
        assert_ne!(expected, initial_gpio);
        assert_eq!(gpio_halls(machine), initial_gpio, "unpowered GPIO does not receive Hall edges");
        let angle_end = machine.plant().live_part().unwrap().physics().wheel().0;
        assert_ne!(angle_start, angle_end);
        let before_wake = physical_state(&host);
        host.set_button("power", true).unwrap();
        assert_eq!(
            physical_state(&host),
            before_wake,
            "zero-time wake must preserve pose, velocity, rotor, current, SOC, and heat"
        );
        let machine = host.session.machine();
        assert!(machine.powered());
        assert_eq!(
            gpio_halls(machine),
            machine.plant().halls(),
            "fresh Hall truth precedes the first boot instruction"
        );
        assert_eq!(gpio_halls(machine), expected);
        println!(
            "quantum {quantum}: moving speed {moving_speed:.6} m/s; {transitions} Hall transitions while off; rotor {angle_start:.6} -> {angle_end:.6} rad; off-current maximum {max_current:.9} A; GPIO {initial_gpio:03b} -> {expected:03b}; complete zero-time state equal"
        );
    }
}
