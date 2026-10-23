//! The lateral rider's legs on the Begode Extreme's measured side pads (`articulated_lateral_v1`,
//! stage 2): the pads come from the profile's `[pad_covers]` contact faces. Balanced by the plain
//! PD law of the physics tests instead of firmware.

use euc_physics::{Intent, LegPad, Physics, RiderModel, Setup, TIMESTEP_S};
use euc_profile::ProfileSource;
use euc_sim::WheelFiles;

fn physics(model: RiderModel) -> Physics {
    let files = WheelFiles::load(&ProfileSource::Builtin, "begode-extreme").unwrap();
    let mut setup = Setup::new(files.body);
    setup.rider.model = model;
    Physics::new(setup).unwrap()
}

fn balance(physics: &Physics) -> f64 {
    let state = physics.state();
    if physics.held() {
        0.0
    } else {
        (-2500.0 * state.pitch_rad + 250.0 * physics.imu().gyro_rad_s[1]).clamp(-300.0, 300.0)
    }
}

/// Rides `seconds` and returns the largest leg-on-shell force on either leg, newtons.
fn ride(physics: &mut Physics, seconds: f64) -> f64 {
    let mut peak = 0.0_f64;
    for _ in 0..(seconds / TIMESTEP_S).round() as usize {
        let torque = balance(physics);
        physics.step(torque);
        assert!(!physics.state().fallen && !physics.rider_off(), "{:?}", physics.state());
        if let Some(legs) = physics.leg_pads() {
            peak = legs.iter().map(|leg| leg.force_n).fold(peak, f64::max);
        }
    }
    peak
}

fn legs(physics: &mut Physics) -> [LegPad; 2] {
    physics.leg_pads().expect("the Extreme has measured pads")
}

/// At rest each leg's calf rests on its pad's upper arm with about the preload, the knees swung
/// out a little; nothing presses in.
#[test]
fn at_rest_the_legs_rest_on_the_pads_with_the_preload() {
    let mut physics = physics(RiderModel::ArticulatedLateralV1);
    ride(&mut physics, 1.0);
    let preload = physics.setup().rider.articulation.pad_preload_n;
    for leg in legs(&mut physics) {
        println!("rest: {leg:?}");
        assert!((leg.force_n - preload).abs() < 10.0, "{leg:?}");
        assert!(leg.gap_m > -0.005, "{leg:?}");
        assert!((0.0..0.45).contains(&leg.swing_rad), "{leg:?}");
    }
    // The other riders have no leg contact.
    assert!(self::physics(RiderModel::ArticulatedSagittalV1).leg_pads().is_none());
}

/// Riding straight the pads do not hold the rider back: the speed builds up as the sagittal
/// rider's, and the legs stay near the preload.
#[test]
fn riding_straight_the_pads_do_not_hold_the_rider_back() {
    let mut speeds = Vec::new();
    for model in [RiderModel::ArticulatedSagittalV1, RiderModel::ArticulatedLateralV1] {
        let mut physics = physics(model);
        ride(&mut physics, 0.5);
        physics.set_held(false);
        physics.rider().set_intent(Intent::Speed(15.0 / 3.6));
        let peak = ride(&mut physics, 10.0);
        speeds.push(physics.state().speed_mps);
        println!("{model:?}: {:.3} m/s after 10 s, largest leg load {peak:.1} N", physics.state().speed_mps);
        if model == RiderModel::ArticulatedLateralV1 {
            assert!(peak < 100.0, "{peak} N");
        }
    }
    assert!((speeds[0] - speeds[1]).abs() < 0.2, "{speeds:?}");
}

/// In a 0.4 rad/s turn at 15 km/h (the external aids still steer) the legs stay on their pads:
/// a short load as the wheel rolls in, then about the preload.
#[test]
fn in_a_moderate_turn_the_legs_stay_on_the_pads() {
    let mut physics = physics(RiderModel::ArticulatedLateralV1);
    ride(&mut physics, 0.5);
    physics.set_held(false);
    physics.rider().set_intent(Intent::Speed(15.0 / 3.6));
    ride(&mut physics, 10.0);
    let before = physics.rider_state();
    physics.rider().set_turn_rate(0.4);
    let entry = ride(&mut physics, 2.0);
    let (mut mean, n) = ([0.0; 2], (2.0 / TIMESTEP_S) as usize);
    for _ in 0..n {
        let torque = balance(&physics);
        physics.step(torque);
        assert!(!physics.state().fallen && !physics.rider_off());
        for (sum, leg) in mean.iter_mut().zip(legs(&mut physics)) {
            *sum += leg.force_n / n as f64;
        }
    }
    let after = physics.rider_state();
    println!(
        "turn: entry peak {entry:.0} N, steady {:.1} / {:.1} N, roll {:.1}°; aid work in 4 s: roll {:.1} J, yaw {:.1} J",
        mean[0],
        mean[1],
        physics.state().roll_rad.to_degrees(),
        after.roll_aid_work_j - before.roll_aid_work_j,
        after.yaw_aid_work_j - before.yaw_aid_work_j
    );
    assert!(entry < 600.0, "{entry} N");
    assert!(mean.iter().all(|force| (5.0..100.0).contains(force)), "{mean:?}");
    assert!(physics.state().roll_rad < -0.1);
}

/// Stepping off switches the legs' pairs off: the legs pass the pads.
#[test]
fn stepping_off_releases_the_legs_from_the_pads() {
    let mut physics = physics(RiderModel::ArticulatedLateralV1);
    physics.step_off();
    for _ in 0..(1.0 / TIMESTEP_S) as usize {
        physics.step(0.0);
    }
    assert!(legs(&mut physics).iter().all(|leg| leg.force_n == 0.0));
}
