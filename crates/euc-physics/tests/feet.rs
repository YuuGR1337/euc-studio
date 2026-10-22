//! Independent foot contact (`model = "articulated_contact_v1"`): each articulated sole
//! stands on its own pedal through friction contact. Balanced by the plain PD law of
//! tests/physics.rs instead of firmware; no firmware image or hardware.

use euc_physics::{Foot, FootState, Intent, Leaving, Physics, RiderModel, Setup, TIMESTEP_S, Terrain, WheelBody};

/// The Begode Extreme's `[body]` (suspension included) from its profile; generic shell shapes.
fn extreme() -> WheelBody {
    let plant: toml::Value = toml::from_str(include_str!("../../../profiles/begode-extreme/plant.toml")).unwrap();
    plant["body"].clone().try_into().unwrap()
}

fn setup() -> Setup {
    let mut setup = Setup::new(extreme());
    assert_eq!(setup.rider.model, RiderModel::ArticulatedSagittalV1, "the welded articulated rider is the default");
    setup.rider.model = RiderModel::ArticulatedContactV1;
    setup
}

/// Motor torque that keeps the pedals level (nothing while the stand holds the wheel).
fn balance(physics: &Physics) -> f64 {
    if physics.held() {
        return 0.0;
    }
    let state = physics.state();
    (-2500.0 * state.pitch_rad + 250.0 * physics.imu().gyro_rad_s[1]).clamp(-300.0, 300.0)
}

fn feet(physics: &Physics) -> [FootState; 2] {
    physics.feet().expect("articulated feet")
}

/// Rides `seconds` under the balance law, calling `each` after every step.
fn ride(physics: &mut Physics, seconds: f64, mut each: impl FnMut(&Physics)) {
    for _ in 0..(seconds / TIMESTEP_S).round() as usize {
        let torque = balance(physics);
        physics.step(torque);
        each(physics);
    }
}

fn standing() -> Physics {
    let mut physics = Physics::new(setup()).unwrap();
    ride(&mut physics, 0.5, |_| {});
    physics.set_held(false);
    ride(&mut physics, 3.0, |_| {});
    physics
}

#[test]
fn both_feet_carry_the_rider_standing_still() {
    let mut physics = standing();
    let weight = physics.setup().rider.mass_kg * physics.gravity();
    let (mut worst_slip, mut worst_lift) = (0.0_f64, 0.0_f64);
    let mut least_load = f64::INFINITY;
    ride(&mut physics, 5.0, |p| {
        for foot in feet(p) {
            worst_slip = worst_slip.max(foot.slip_m);
            worst_lift = worst_lift.max(foot.lift_m.abs());
            least_load = least_load.min(foot.normal_n);
            assert!(foot.in_contact, "{foot:?}");
        }
    });
    let [left, right] = feet(&physics);
    let state = physics.state();
    println!(
        "standing: {left:?} {right:?}; slip {worst_slip:.6} m, lift {worst_lift:.6} m, least load {least_load:.1} N"
    );
    assert!(!state.fallen && !physics.rider_off() && state.speed_mps.abs() < 0.3, "{state:?}");
    // Each foot carries about half the rider; together the whole weight.
    let total = left.normal_n + right.normal_n;
    assert!((total - weight).abs() < 0.05 * weight, "feet carry {total} N of {weight} N");
    assert!((left.normal_n - right.normal_n).abs() < 0.1 * weight, "{left:?} {right:?}");
    assert!(left.friction_n < 0.05 * weight && right.friction_n < 0.05 * weight);
    assert!(worst_slip < 0.002 && worst_lift < 0.002, "standing feet stay put");
}

#[test]
fn a_lifted_foot_leaves_its_pedal_while_the_other_carries_the_rider() {
    let mut physics = standing();
    let weight = physics.setup().rider.mass_kg * physics.gravity();
    assert!(physics.set_foot_lift(Foot::Right, f64::NAN).is_err());
    assert!(physics.set_foot_lift(Foot::Right, 1.0).is_err());
    physics.set_foot_lift(Foot::Right, 0.3).unwrap();
    assert!(physics.set_foot_lift(Foot::Left, 0.3).is_err(), "one foot at a time");
    let (mut highest, mut left_least, mut left_sum) = (0.0_f64, f64::INFINITY, 0.0);
    let (mut off_s, mut stance_slip) = (0.0, 0.0_f64);
    // A quarter-second lift, as when a rider shifts a foot: the sagittal rider has no hip roll,
    // so it cannot hold one-foot stance for long (see the next test).
    ride(&mut physics, 0.25, |p| {
        let [left, right] = feet(p);
        highest = highest.max(right.lift_m);
        assert!(left.in_contact, "the left foot keeps its contact: {left:?}");
        if !right.in_contact {
            off_s += TIMESTEP_S;
            left_least = left_least.min(left.normal_n);
            left_sum += left.normal_n * TIMESTEP_S;
            stance_slip = stance_slip.max(left.slip_m);
        }
    });
    let left_mean = left_sum / off_s;
    println!(
        "lift: right up {highest:.4} m, off the pedal {off_s:.3} s, left mean {left_mean:.1} N, least {left_least:.1} N, left slip {stance_slip:.5} m"
    );
    assert!(highest > 0.01, "the right sole rises off its pedal: {highest} m");
    assert!(off_s > 0.1, "the right foot is off its pedal for a while: {off_s} s");
    assert!(left_mean > 0.85 * weight, "the left foot carries the rider meanwhile: {left_mean} N");
    assert!(left_least > 0.5 * weight, "and never lets the rider drop: {left_least} N");
    // 0.6-0.7 mm measured (2026-10-22), with the constant linkage ratio and with the rocker curve.
    assert!(stance_slip < 0.002, "and its sole sticks to the pedal while it carries the rider: {stance_slip} m");
    physics.set_foot_lift(Foot::Right, 0.0).unwrap();
    let (mut forward, mut sideways, mut widest) = (0.0_f64, 0.0_f64, 0.0_f64);
    ride(&mut physics, 2.0, |p| {
        for foot in feet(p) {
            forward = forward.max(foot.offset_m[0].abs());
            sideways = sideways.max(foot.offset_m[1].abs());
        }
        widest = widest.max(p.rider_com_m()[1].abs());
    });
    let [left, right] = feet(&physics);
    let state = physics.state();
    println!(
        "down again: {left:?} {right:?}, feet moved up to {forward:.4} m forward, {sideways:.4} m sideways; centre of mass {widest:.4} m sideways; {state:?}"
    );
    assert!(!physics.rider_off() && !state.fallen, "the rider stays on: {state:?}");
    assert!(left.in_contact && right.in_contact, "both feet back on: {left:?} {right:?}");
    assert!(right.lift_m.abs() < 0.005, "the right foot stands on its pedal again: {right:?}");
    // Setting the right foot down unloads the left one: it hops for up to about 1 s and lands a few
    // centimetres back on its deck (left 0.022 m with the constant linkage ratio, 0.044 m with the
    // rocker curve of plant.toml; 2026-10-22). How far follows the suspension, so the end position
    // is held to the decks below, not to where the feet stood.
    assert!(sideways < 0.03, "the soles stay on their decks sideways: {sideways} m");
    assert!(forward < 0.08, "and lengthwise: {forward} m");
    assert!(widest < 0.1, "the rider's centre of mass stays between the pedals: {widest} m");
}

#[test]
fn a_one_foot_stance_holds_with_the_roll_aid_and_without_it_the_rider_comes_off() {
    // The sagittal legs cannot roll, and one sole is a narrow base: the roll aid holds the
    // upper body upright as it holds the shell (standing in for hip and ankle roll).
    let mut physics = standing();
    physics.set_foot_lift(Foot::Right, 0.3).unwrap();
    ride(&mut physics, 1.0, |p| assert!(feet(p)[0].in_contact, "the left foot stands"));
    physics.set_foot_lift(Foot::Right, 0.0).unwrap();
    ride(&mut physics, 3.0, |_| {});
    let [left, right] = feet(&physics);
    println!("one-foot second: {left:?} {right:?}");
    assert!(!physics.rider_off() && !physics.state().fallen);
    assert!(left.in_contact && right.in_contact);
    assert!(left.offset_m[1].abs() < 0.02 && right.offset_m[1].abs() < 0.02, "sideways on the decks");
    assert!(left.offset_m[0].abs() < 0.08 && right.offset_m[0].abs() < 0.08, "lengthwise on the decks");

    // Without any roll aid the rider topples over the standing foot; once both feet have been
    // off their pedals for LOST_FOOTING_S the rider counts as off and every aid and motor stops.
    let mut model = setup();
    model.assist = euc_physics::Assist { roll_stiffness: 0.0, roll_damping: 0.0 };
    let mut physics = Physics::new(model).unwrap();
    ride(&mut physics, 0.5, |_| {});
    physics.set_foot_lift(Foot::Right, 0.3).unwrap();
    ride(&mut physics, 1.0, |_| {});
    physics.set_foot_lift(Foot::Right, 0.0).unwrap_or(());
    ride(&mut physics, 5.0, |_| {});
    let Some((at, how)) = physics.left_pedals() else { panic!("still on: {:?}", feet(&physics)) };
    println!("without aids: off at {at:.3} s ({how:?}), centre of mass {:?}", physics.rider_com_m());
    assert_eq!(how, Leaving::LostFooting);
    let state = physics.rider_state();
    assert!(!state.attached && state.roll_aid_torque_nm == 0.0 && state.yaw_aid_torque_nm == 0.0);
    assert!(physics.rider_joints().iter().all(|j| j.torque_nm == 0.0));
}

#[test]
fn a_pothole_impact_keeps_both_feet_on_their_pedals() {
    let mut model = setup();
    model.terrain = Terrain {
        length_m: 60.0,
        features: vec![euc_physics::Feature::Pothole { at_m: 20.0, depth_m: 0.02, length_m: 0.5 }],
        ..Terrain::default()
    };
    let mut physics = Physics::new(model).unwrap();
    ride(&mut physics, 0.5, |_| {});
    physics.set_held(false);
    physics.rider().set_intent(Intent::Speed(12.0 / 3.6));
    let (mut worst_slip, mut worst_lift, mut off_s) = (0.0_f64, 0.0_f64, 0.0);
    let mut peak = 0.0_f64;
    let mut crossed = false;
    ride(&mut physics, 14.0, |p| {
        let x = p.position_m()[0];
        if (19.0..23.0).contains(&x) {
            crossed = true;
            for foot in feet(p) {
                worst_slip = worst_slip.max(foot.slip_m);
                worst_lift = worst_lift.max(foot.lift_m);
                peak = peak.max(foot.normal_n);
                if !foot.in_contact {
                    off_s += TIMESTEP_S;
                }
            }
        }
    });
    let state = physics.state();
    let [left, right] = feet(&physics);
    println!(
        "pothole: slip {worst_slip:.4} m, lift {worst_lift:.4} m, off {off_s:.3} s, peak load {peak:.0} N, {state:?}, {left:?} {right:?}"
    );
    assert!(crossed && physics.position_m()[0] > 24.0, "rides through the pothole: {state:?}");
    assert!(!physics.rider_off() && !state.fallen, "{state:?}");
    assert!(left.in_contact && right.in_contact, "both feet on after the impact");
    // Plausible slip: the soles stay on their decks (the pedal is 29 cm long) and settle back.
    assert!(worst_slip < 0.02, "slip {worst_slip} m");
    assert!(worst_lift < 0.02, "lift {worst_lift} m");
}

#[test]
fn contact_feet_steer_lean_into_a_turn_and_follow_manual_lean() {
    let mut physics = standing();
    physics.rider().set_intent(Intent::Speed(15.0 / 3.6));
    ride(&mut physics, 20.0, |_| {});
    let before = physics.state();
    assert!(before.speed_mps > 3.8 && before.roll_rad.abs() < 0.01, "rides straight: {before:?}");
    // Turn left at 0.4 rad/s: about a 10 m circle, the wheel leant about 9.5° into it.
    physics.rider().set_turn_rate(0.4);
    ride(&mut physics, 4.0, |_| {});
    let start = physics.heading_rad();
    let (mut roll, mut sideways, mut least) = (0.0_f64, 0.0_f64, f64::INFINITY);
    let steps = (4.0 / TIMESTEP_S).round() as usize;
    ride(&mut physics, 4.0, |p| {
        roll += p.state().roll_rad / steps as f64;
        for foot in feet(p) {
            sideways = sideways.max(foot.offset_m[1].abs());
            least = least.min(foot.normal_n);
        }
        assert!(!p.rider_off() && !p.state().fallen, "{:?}", p.state());
    });
    let yaw_rate = euc_physics::wrap_angle(physics.heading_rad() - start) / 4.0;
    println!(
        "contact turn: yaw {yaw_rate:.3} rad/s, roll {:.1}°, feet sideways up to {sideways:.4} m, least load {least:.0} N",
        roll.to_degrees()
    );
    assert!((yaw_rate - 0.4).abs() < 0.05, "turns at the rate asked: {yaw_rate}");
    assert!(roll < -0.1, "leans left into the turn: {roll}");
    assert!(sideways < 0.02 && least > 150.0, "both feet stay planted through the turn");
    physics.rider().set_turn_rate(0.0);
    ride(&mut physics, 6.0, |_| {});
    assert!(physics.state().roll_rad.abs() < 0.01, "upright again: {:?}", physics.state());
    // Manual lean (keyboard or gamepad): neutral lean slows the wheel without a fall.
    let speed = physics.state().speed_mps;
    physics.rider().set_intent(Intent::Lean(-0.05));
    ride(&mut physics, 3.0, |_| {});
    let state = physics.state();
    assert!(state.speed_mps < speed - 0.5 && !state.fallen && !physics.rider_off(), "{state:?}");
    assert!(feet(&physics).iter().all(|f| f.in_contact));
}

#[test]
fn letting_go_releases_both_feet_from_the_pedals() {
    let mut physics = standing();
    physics.rider().set_intent(Intent::Speed(15.0 / 3.6));
    ride(&mut physics, 8.0, |_| {});
    assert!(physics.state().speed_mps > 3.5, "{:?}", physics.state());
    physics.set_drive_cut(true);
    for _ in 0..(3.0 / TIMESTEP_S) as usize {
        physics.step(0.0);
    }
    let Some((_, how)) = physics.left_pedals() else { panic!("still on the pedals: {:?}", physics.state()) };
    assert_eq!(how, Leaving::LetGoAfterCut);
    let [left, right] = feet(&physics);
    println!("let go: {left:?} {right:?}");
    assert!(!left.in_contact && !right.in_contact, "the pedals no longer carry the feet");
    assert!(physics.rider_joints().iter().all(|j| j.torque_nm == 0.0));
    assert!(physics.set_foot_lift(Foot::Left, 0.2).is_err());
}

#[test]
fn stepping_off_lands_the_rider_beside_the_wheel() {
    let mut physics = standing();
    physics.step_off();
    for _ in 0..(2.0 / TIMESTEP_S) as usize {
        physics.step(0.0);
    }
    assert!(physics.rider_off());
    let bodies = physics.bodies();
    for (body, pose) in bodies.iter().zip(physics.poses()).filter(|(b, _)| b.name.ends_with("foot")) {
        assert!(pose.pos[2] > -0.03 && pose.pos[2] < 0.25, "{} on the ground: {:?}", body.name, pose.pos);
    }
    assert!(feet(&physics).iter().all(|f| !f.in_contact && f.normal_n < 1e-9));
}

#[test]
fn the_default_articulated_rider_keeps_welded_feet_that_never_slip() {
    let mut model = setup();
    model.rider.model = RiderModel::ArticulatedSagittalV1;
    let mut physics = Physics::new(model).unwrap();
    assert!(physics.set_foot_lift(Foot::Left, 0.2).is_err(), "welded feet cannot lift");
    ride(&mut physics, 0.5, |_| {});
    physics.set_held(false);
    physics.rider().set_intent(Intent::Speed(10.0 / 3.6));
    ride(&mut physics, 6.0, |p| {
        for foot in feet(p) {
            assert!(foot.in_contact && foot.normal_n.is_nan() && foot.slip_m < 0.003, "{foot:?}");
        }
    });
    assert!(!physics.state().fallen);
    let parsed: euc_physics::Rider = toml::from_str("model = \"articulated_contact_v1\"").unwrap();
    assert_eq!(parsed.model, RiderModel::ArticulatedContactV1);
    assert_eq!(parsed.model.id(), "articulated_contact_v1");
    assert!(toml::from_str::<euc_physics::Rider>("[articulation]\nfeet = \"contact\"").is_err(), "no feet switch");
    let mut bad = setup();
    bad.rider.articulation.sole_contact_time_s = 0.0005;
    assert!(Physics::new(bad).is_err(), "contact time below two steps");
}
