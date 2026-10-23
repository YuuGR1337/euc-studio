//! The lateral rider (`model = "articulated_lateral_v1"`): legs that also abduct, adduct and
//! twist at the hips and roll at the ankles, de Leva's segments, soft knees. Stage 1 keeps every
//! external aid; the legs ride like the sagittal rider's. Balanced by the plain PD law of
//! tests/physics.rs instead of firmware.

use euc_physics::{
    Intent, Physics, Rider, RiderModel, Setup, Shapes, Shell, TIMESTEP_S, Tire, Torus, Wheel, WheelBody,
};

/// The Begode Extreme without its suspension, with the scanned tire torus (as tests/articulated.rs).
fn setup(model: RiderModel) -> Setup {
    let mut setup = Setup::new(WheelBody {
        tire: Tire { radius_m: 0.2251, half_width_m: 0.0407, friction: 0.9, crr: 0.012, contact_time_s: 0.02 },
        wheel: Wheel { mass_kg: 16.0, spin_inertia_kgm2: 0.487 },
        shell: Shell {
            mass_kg: 26.0,
            com_above_axle_m: 0.10,
            inertia_kgm2: [0.85, 0.92, 0.46],
            pedal_below_axle_m: -0.012,
            imu_above_axle_m: 0.15,
        },
        suspension: None,
        shapes: Some(Shapes { tire: Some(Torus { major_m: 0.1716, minor_m: 0.058 }), ..Shapes::default() }),
    });
    setup.rider.model = model;
    setup
}

fn lateral() -> Physics {
    Physics::new(setup(RiderModel::ArticulatedLateralV1)).unwrap()
}

/// Motor torque that keeps the pedals level (nothing while the stand holds the wheel).
fn balance(physics: &Physics) -> f64 {
    let state = physics.state();
    if physics.held() {
        0.0
    } else {
        (-2500.0 * state.pitch_rad + 250.0 * physics.imu().gyro_rad_s[1]).clamp(-300.0, 300.0)
    }
}

/// One balanced step; the rider stays on, every joint within its cap, every ankle roll within
/// what its sole carries.
fn step(physics: &mut Physics) {
    let torque = balance(physics);
    physics.step(torque);
    assert!(!physics.state().fallen && !physics.rider_off(), "{:?}", physics.state());
    if let Some(soles) = physics.sole_loads() {
        for sole in soles {
            assert!(sole.roll_nm.abs() <= sole.roll_limit_nm + 1e-9, "{sole:?}");
        }
    }
}

fn ride(physics: &mut Physics, seconds: f64) {
    for _ in 0..(seconds / TIMESTEP_S).round() as usize {
        step(physics);
    }
}

fn joint(physics: &Physics, name: &str) -> f64 {
    physics.rider_joints().iter().find(|j| j.name == name).unwrap().angle_rad
}

/// The angle from vertical, along the heading, of the line from the ankles to the rider's
/// centre of mass (as tests/articulated.rs).
fn centre_of_mass_lean(physics: &Physics) -> f64 {
    let com = physics.rider_com_m();
    let (bodies, poses) = (physics.bodies(), physics.poses());
    let feet: Vec<[f64; 3]> = bodies
        .iter()
        .zip(&poses)
        .filter(|(body, _)| body.name.ends_with("_foot"))
        .map(|(_, pose)| pose.pos.map(f64::from))
        .collect();
    let ankles: [f64; 3] = std::array::from_fn(|i| f64::midpoint(feet[0][i], feet[1][i]));
    let offset: [f64; 3] = std::array::from_fn(|i| com[i] - ankles[i]);
    let heading = physics.heading_rad();
    (offset[0] * heading.cos() + offset[1] * heading.sin()).atan2(offset[2])
}

#[test]
fn the_lateral_rider_is_opt_in_with_fourteen_real_joints_on_the_same_bodies() {
    let parsed: Rider = toml::from_str(r#"model = "articulated_lateral_v1""#).unwrap();
    assert_eq!(parsed.model, RiderModel::ArticulatedLateralV1);
    assert_eq!(parsed.model.id(), "articulated_lateral_v1");
    assert!(parsed.model.is_articulated() && parsed.model.has_lateral_legs() && !parsed.model.has_contact_feet());
    assert_eq!(Rider::default().model, RiderModel::ArticulatedSagittalV1, "the sagittal rider stays the default");
    let physics = lateral();
    let names: Vec<_> = physics.bodies().into_iter().map(|b| b.name).collect();
    assert_eq!(
        names,
        [
            "wheel",
            "shell",
            "rider",
            "left_thigh",
            "left_shank",
            "left_foot",
            "right_thigh",
            "right_shank",
            "right_foot"
        ]
    );
    assert!((physics.total_mass() - 122.0).abs() < 1e-9);
    assert_eq!(physics.rider_state().model, "articulated_lateral_v1");
    let joints: Vec<_> = physics.rider_joints().into_iter().map(|j| j.name).collect();
    assert_eq!(
        joints,
        [
            "left_hip",
            "left_hip_adduction",
            "left_hip_rotation",
            "left_knee",
            "left_knee_rotation",
            "left_ankle",
            "left_ankle_roll",
            "right_hip",
            "right_hip_adduction",
            "right_hip_rotation",
            "right_knee",
            "right_knee_rotation",
            "right_ankle",
            "right_ankle_roll"
        ]
    );
    let bad: Result<Rider, _> = toml::from_str("[articulation]\nhip_rotation_max_nm = -1.0");
    let mut invalid = setup(RiderModel::ArticulatedLateralV1);
    invalid.rider = bad.unwrap();
    assert!(Physics::new(invalid).is_err(), "strength caps must be positive");
    let mut invalid = setup(RiderModel::ArticulatedLateralV1);
    invalid.rider.articulation.soft_knee_rad = 1.5;
    assert!(Physics::new(invalid).is_err(), "soft knees stay within the deepest crouch");
    let mut invalid = setup(RiderModel::ArticulatedLateralV1);
    invalid.rider.articulation.foot_offset_m = 0.1;
    assert!(Physics::new(invalid).is_err(), "the foot stays on its pedal");
}

/// At rest the rider stands on soft knees (30°, the owner's report), the hips at their anatomical
/// width above feet on the pedals: the legs lean in, the feet lie flat, and the centre of mass sits
/// lower than the straight-legged sagittal rider's and still over the ankles. Without pads (this
/// wheel has no measured shapes) the knees do not swing out.
#[test]
fn soft_knees_lower_the_centre_of_mass_over_the_ankles() {
    let mut physics = lateral();
    ride(&mut physics, 1.0);
    let mut straight = Physics::new(setup(RiderModel::ArticulatedSagittalV1)).unwrap();
    ride(&mut straight, 1.0);
    let knees = [joint(&physics, "left_knee"), joint(&physics, "right_knee")];
    let drop = straight.rider_com_m()[2] - physics.rider_com_m()[2];
    let (pelvis, thigh) = (physics.rider_position_m(), {
        let index = physics.bodies().iter().position(|b| b.name == "left_thigh").unwrap();
        physics.poses()[index].pos.map(f64::from)
    });
    println!(
        "rest: knees {:.1}° {:.1}°, centre of mass {drop:.4} m lower than straight legs, lean {:.2}°, hip at {:.4} m",
        knees[0].to_degrees(),
        knees[1].to_degrees(),
        centre_of_mass_lean(&physics).to_degrees(),
        thigh[1] - pelvis[1]
    );
    for knee in knees {
        assert!((knee.to_degrees() - 30.0).abs() < 3.0, "soft knees: {}°", knee.to_degrees());
    }
    assert!((0.015..0.05).contains(&drop), "centre of mass drop {drop} m");
    assert!(centre_of_mass_lean(&physics).abs() < 1_f64.to_radians());
    assert!((thigh[1] - pelvis[1] - 0.0898).abs() < 0.003, "hip half width {}", thigh[1] - pelvis[1]);
    // The legs lean in: hips abducted, ankles inverted, and nothing twisted.
    for side in ["left", "right"] {
        let (adduction, roll) =
            (joint(&physics, &format!("{side}_hip_adduction")), joint(&physics, &format!("{side}_ankle_roll")));
        assert!(adduction < -0.05 && roll > 0.05, "{side}: adduction {adduction}, ankle roll {roll}");
        assert!(joint(&physics, &format!("{side}_hip_rotation")).abs() < 0.02);
        assert!(joint(&physics, &format!("{side}_knee_rotation")).abs() < 0.02);
    }
}

/// Welded soles carry the rider's weight, on the stand and riding; no foot pulls its pedal, and
/// each ankle rolls its pedal no harder than its sole could (checked on every step).
#[test]
fn welded_soles_carry_the_rider_and_the_ankles_roll_within_the_soles() {
    let mut physics = lateral();
    ride(&mut physics, 1.0);
    let weight = physics.setup().rider.mass_kg * physics.gravity();
    let soles = physics.sole_loads().unwrap();
    let standing = soles[0].normal_n + soles[1].normal_n;
    assert!((standing - weight).abs() < 0.02 * weight, "{standing} N of {weight}");
    physics.set_held(false);
    physics.rider().set_intent(Intent::Speed(15.0 / 3.6));
    let (mut sum, mut n, mut least) = (0.0, 0, f64::INFINITY);
    for tick in 0..(12.0 / TIMESTEP_S) as usize {
        step(&mut physics);
        assert!(!physics.sole_pulls(), "{:?}", physics.sole_loads());
        if tick as f64 * TIMESTEP_S > 8.0 {
            let soles = physics.sole_loads().unwrap();
            sum += soles[0].normal_n + soles[1].normal_n;
            n += 1;
            least = least.min(soles[0].normal_n.min(soles[1].normal_n));
        }
    }
    let riding = sum / f64::from(n);
    println!(
        "soles: {standing:.1} N standing, {riding:.1} N riding at 15 km/h (least {least:.1} N per sole) of {weight:.1} N"
    );
    assert!((riding - weight).abs() < 0.05 * weight, "{riding} N of {weight}");
}

/// The same speed control and braking as the sagittal rider: the legs change the posture, not
/// the riding.
#[test]
fn it_rides_and_brakes_like_the_sagittal_rider() {
    let mut results = Vec::new();
    for model in [RiderModel::ArticulatedSagittalV1, RiderModel::ArticulatedLateralV1] {
        let mut physics = Physics::new(setup(model)).unwrap();
        ride(&mut physics, 0.5);
        physics.set_held(false);
        physics.rider().set_intent(Intent::Speed(15.0 / 3.6));
        ride(&mut physics, 10.0);
        let cruise = physics.state().speed_mps;
        physics.rider().set_intent(Intent::Speed(0.0));
        let start = physics.state().distance_m;
        ride(&mut physics, 10.0);
        let (stopped, distance) = (physics.state().speed_mps, physics.state().distance_m - start);
        println!("{model:?}: {cruise:.3} m/s after 10 s, stopped to {stopped:.3} m/s in {distance:.2} m");
        assert!(stopped.abs() < 0.3, "{model:?} stops: {stopped} m/s");
        results.push((cruise, distance));
    }
    let [(cruise, distance), (lateral_cruise, lateral_distance)] = results[..] else { unreachable!() };
    assert!((lateral_cruise - cruise).abs() < 0.2, "{lateral_cruise} vs {cruise} m/s");
    assert!((lateral_distance - distance).abs() < 0.1 * distance, "{lateral_distance} vs {distance} m");
}

/// A crouch bends on from the soft knees and keeps the centre of mass over the ankles, on the
/// stand and while riding; standing up again comes back to the soft knees.
#[test]
fn a_crouch_bends_on_from_the_soft_knees_over_the_ankles() {
    for crouch in [0.6, 1.2] {
        let mut physics = lateral();
        physics.set_crouch(crouch).unwrap();
        ride(&mut physics, 3.0);
        let lean = centre_of_mass_lean(&physics).to_degrees();
        println!(
            "on the stand, crouch {crouch} rad: knee {:.3} rad, centre of mass lean {lean:.2}°",
            joint(&physics, "left_knee")
        );
        assert!(lean.abs() < 1.0, "crouch {crouch}: {lean}°");
    }
    let mut physics = lateral();
    ride(&mut physics, 0.5);
    physics.set_held(false);
    physics.rider().set_manual(true);
    physics.rider().set_intent(Intent::Speed(4.0));
    ride(&mut physics, 8.0);
    let held = physics.rider().lean_command();
    physics.rider().set_intent(Intent::Lean(held));
    let (height, before) = (physics.rider_com_m()[2], physics.state().speed_mps);
    let mut pitch = 0.0_f64;
    for request in [1.2, 0.0] {
        physics.set_crouch(request).unwrap();
        for _ in 0..4000 {
            step(&mut physics);
            pitch = pitch.max(physics.state().pitch_rad.abs());
        }
    }
    let (standing, after) = (physics.rider_com_m()[2], physics.state().speed_mps);
    println!(
        "crouch at {before:.2} m/s: back to {after:.2} m/s, height {height:.3} -> {standing:.3} m, pitch {:.2}°",
        pitch.to_degrees()
    );
    assert!((standing - height).abs() < 0.01 && (after - before).abs() < 0.5 && pitch < 3_f64.to_radians());
    assert!((joint(&physics, "left_knee").to_degrees() - 30.0).abs() < 3.0);
}

/// A straight run at 30 km/h stays upright and steady: the extra joints add no wobble.
#[test]
fn a_straight_run_at_30_kmh_stays_steady() {
    let mut physics = lateral();
    ride(&mut physics, 0.5);
    physics.set_held(false);
    physics.rider().set_intent(Intent::Speed(30.0 / 3.6));
    ride(&mut physics, 12.0);
    let mut roll = 0.0_f64;
    for _ in 0..(10.0 / TIMESTEP_S) as usize {
        step(&mut physics);
        roll = roll.max(physics.state().roll_rad.abs());
    }
    println!("30 km/h: {:.2} m/s, largest roll {:.3}°", physics.state().speed_mps, roll.to_degrees());
    assert!(physics.state().speed_mps > 7.8, "{:?}", physics.state());
    assert!(roll < 1_f64.to_radians(), "{}°", roll.to_degrees());
}

/// Stepping off releases both welds, stops all fourteen motors and every aid, and the feet come
/// down beside the wheel.
#[test]
fn stepping_off_releases_the_feet_and_stops_every_motor_and_aid() {
    let mut physics = lateral();
    physics.step_off();
    for _ in 0..(2.0 / TIMESTEP_S) as usize {
        physics.step(0.0);
    }
    assert!(physics.rider_off());
    assert!(physics.rider_joints().iter().all(|j| j.torque_nm == 0.0));
    let state = physics.rider_state();
    assert_eq!((state.yaw_aid_torque_nm, state.roll_aid_torque_nm), (0.0, 0.0));
    assert!(physics.sole_loads().unwrap().iter().all(|sole| sole.normal_n == 0.0));
    for (body, pose) in physics.bodies().iter().zip(physics.poses()).filter(|(b, _)| b.name.ends_with("foot")) {
        assert!(pose.pos[2] > -0.03 && pose.pos[2] < 0.25, "{} {:?}", body.name, pose.pos);
    }
}
