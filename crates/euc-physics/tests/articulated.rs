//! Physical articulation (the default rider model). No firmware image or hardware is used.
use euc_physics::{
    Intent, Physics, Rider, RiderModel, Setup, Shapes, Shell, TIMESTEP_S, Terrain, Tire, Torus, Wheel, WheelBody,
};

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
fn run(physics: &mut Physics, seconds: f64) {
    for _ in 0..(seconds / TIMESTEP_S).round() as usize {
        physics.step(0.0);
    }
}
#[test]
fn articulated_model_is_the_default_and_the_rigid_baseline_stays_selectable() {
    let rider: Rider = toml::from_str("").unwrap();
    assert_eq!(rider.model, RiderModel::ArticulatedSagittalV1);
    assert_eq!(rider.model.id(), euc_physics::RIDER_MODEL);
    assert_eq!(Rider::default().model, RiderModel::ArticulatedSagittalV1);
    let rigid: Rider = toml::from_str(r#"model = "rigid_ankle_v1""#).unwrap();
    assert_eq!(rigid.model, RiderModel::RigidAnkleV1);
    // The rigid baseline keeps its three exported bodies.
    let physics = Physics::new(setup(rigid.model)).unwrap();
    let names: Vec<_> = physics.bodies().into_iter().map(|b| b.name).collect();
    assert_eq!(names, ["wheel", "shell", "rider"]);
    assert_eq!(physics.poses().len(), 3);
    assert_eq!(physics.rider_joints(), []);
    assert_eq!(physics.rider_state().model, "rigid_ankle_v1");
    assert!((physics.total_mass() - 122.0).abs() < 1e-10);
}
#[test]
fn articulated_mass_pose_order_and_six_joints_are_real() {
    let physics = Physics::new(setup(RiderModel::ArticulatedSagittalV1)).unwrap();
    let bodies = physics.bodies();
    assert_eq!(bodies.len(), 9);
    assert_eq!(bodies.len(), physics.poses().len());
    assert!(bodies.iter().all(|body| !body.name.is_empty() && !body.geoms.is_empty()));
    assert!((physics.total_mass() - 122.0).abs() < 1e-10);
    assert_eq!(physics.rider_state().model, "articulated_sagittal_v1");
    let joints = physics.rider_joints();
    assert_eq!(joints.len(), 6);
    assert!(joints.iter().all(|j| j.angle_rad.is_finite() && j.rate_rad_s.is_finite()));
}
#[test]
fn crouch_moves_physical_knees_and_lowers_the_pelvis_without_pose_writes() {
    let mut physics = Physics::new(setup(RiderModel::ArticulatedSagittalV1)).unwrap();
    let height = physics.rider_position_m()[2];
    assert!(physics.set_crouch(f64::NAN).is_err());
    assert!(physics.set_crouch(2.0).is_err());
    physics.set_crouch(0.6).unwrap();
    run(&mut physics, 1.5);
    let joints = physics.rider_joints();
    for joint in joints.iter().filter(|j| j.name.ends_with("knee")) {
        assert!(joint.angle_rad > 0.15 && joint.angle_rad < 1.0, "{joint:?}");
    }
    assert!(physics.rider_position_m()[2] < height - 0.01, "start {height}, now {:?}", physics.rider_position_m());
    assert!(joints.iter().all(|j| j.torque_nm.abs() <= 200.0));
    assert!(physics.poses().iter().all(|p| p.pos.iter().chain(p.quat.iter()).all(|v| v.is_finite())));
}
/// Free Ride's crouch: a full crouch asked for at once, while riding and at a stop, with the
/// manual lean at rest. The knees bend at the crouch rate and the hips keep the centre of mass
/// over the feet, so the shell barely pitches and the wheel does not ride off; standing up again
/// restores the height.
#[test]
fn a_full_crouch_lowers_the_centre_of_mass_over_the_feet_and_the_wheel_stays_put() {
    // On the stand: the centre of mass stays over the ankles at any depth.
    for crouch in [0.6, 1.2] {
        let mut physics = Physics::new(setup(RiderModel::ArticulatedSagittalV1)).unwrap();
        physics.set_crouch(crouch).unwrap();
        balanced_for(&mut physics, 6000);
        let lean = centre_of_mass_lean(&physics).to_degrees();
        println!(
            "on the stand, crouch {crouch} rad: centre of mass lean {lean:.2}°, height {:.3} m",
            physics.rider_com_m()[2]
        );
        assert!(lean.abs() < 1.5, "crouch {crouch}: the centre of mass leans {lean}° from the ankles");
    }
    for speed in [4.0, 0.0] {
        let mut physics = Physics::new(setup(RiderModel::ArticulatedSagittalV1)).unwrap();
        balanced_for(&mut physics, 1000);
        physics.set_held(false);
        physics.rider().set_manual(true);
        physics.rider().set_intent(Intent::Speed(speed));
        balanced_for(&mut physics, 16_000);
        // From here on the stick is at rest: the lean the rider holds, not a speed.
        let held = physics.rider().lean_command();
        physics.rider().set_intent(Intent::Lean(held));
        let (height, before) = (physics.rider_com_m()[2], physics.state().speed_mps);
        physics.set_crouch(1.2).unwrap();
        let mut pitch = 0.0_f64;
        for _ in 0..4000 {
            balanced_step(&mut physics);
            pitch = pitch.max(physics.state().pitch_rad.abs());
        }
        let (crouched, during) = (physics.rider_com_m()[2], physics.state().speed_mps);
        physics.set_crouch(0.0).unwrap();
        for _ in 0..4000 {
            balanced_step(&mut physics);
            pitch = pitch.max(physics.state().pitch_rad.abs());
        }
        let (standing, after) = (physics.rider_com_m()[2], physics.state().speed_mps);
        println!(
            "crouch at {speed} m/s: centre of mass {height:.3} -> {crouched:.3} -> {standing:.3} m; \
             speed {before:.2} / {during:.2} / {after:.2} m/s; largest pitch {:.2}°",
            pitch.to_degrees()
        );
        assert!(crouched < height - 0.1, "a full crouch lowers the centre of mass: {height} -> {crouched}");
        assert!((standing - height).abs() < 0.01, "standing up restores it: {height} -> {standing}");
        assert!(pitch < 3_f64.to_radians(), "the shell barely pitches: {}°", pitch.to_degrees());
        assert!((during - before).abs() < 0.5 && (after - before).abs() < 0.5, "{before} / {during} / {after} m/s");
    }
}

#[test]
fn release_carries_both_feet_to_ground_and_disables_every_joint_motor() {
    let mut physics = Physics::new(setup(RiderModel::ArticulatedSagittalV1)).unwrap();
    let start = physics.rider_position_m();
    physics.step_off();
    assert_eq!(physics.rider_position_m(), start, "release must not teleport");
    assert!(physics.set_crouch(0.2).is_err());
    run(&mut physics, 2.0);
    assert!(physics.rider_off());
    assert!(physics.rider_joints().iter().all(|j| j.torque_nm == 0.0));
    let bodies = physics.bodies();
    let poses = physics.poses();
    for (body, pose) in bodies.iter().zip(&poses).filter(|(b, _)| b.name.ends_with("foot")) {
        assert!(pose.pos[2] > -0.03 && pose.pos[2] < 0.25, "{} {:?}", body.name, pose.pos);
    }
    let state = physics.rider_state();
    assert_eq!(state.yaw_aid_torque_nm, 0.0);
    assert_eq!(state.roll_aid_torque_nm, 0.0);
    assert!((physics.total_mass() - 122.0).abs() < 1e-10);
}
#[test]
fn articulation_parameters_reject_invalid_joint_limits_before_building() {
    let mut model = setup(RiderModel::ArticulatedSagittalV1);
    model.rider.articulation.joint_max_nm = f64::INFINITY;
    assert!(Physics::new(model).is_err());
}

#[test]
fn rigid_body_metadata_keeps_the_actual_48_segment_tire_geometry() {
    let physics = Physics::new(setup(RiderModel::RigidAnkleV1)).unwrap();
    let bodies = physics.bodies();
    assert_eq!(bodies[0].geoms.len(), 48, "do not drop collision shapes to satisfy a reader cap");
    assert!(bodies.iter().map(|body| body.geoms.len()).sum::<usize>() <= 256);
    for body in bodies {
        for geom in body.geoms {
            assert!(geom.size.iter().chain(geom.pos.iter()).chain(geom.quat.iter()).all(|v| v.is_finite()));
        }
    }
}

#[test]
fn articulated_rider_accepts_lean_input_on_a_balanced_moving_wheel() {
    let mut physics = Physics::new(setup(RiderModel::ArticulatedSagittalV1)).unwrap();
    physics.set_held(false);
    physics.rider().set_intent(Intent::Lean(0.04));
    for _ in 0..6000 {
        let state = physics.state();
        let torque = (-2500.0 * state.pitch_rad + 250.0 * physics.imu().gyro_rad_s[1]).clamp(-300.0, 300.0);
        physics.step(torque);
    }
    let state = physics.state();
    assert!(!physics.rider_off() && !state.fallen, "{state:?}");
    assert!(state.speed_mps > 0.1, "lean must drive real dynamics: {state:?}");
    assert!((physics.rider_state().commanded_lean_rad - 0.04).abs() < 1e-9);
}

#[test]
fn model_selection_preserves_explicit_articulation_and_delay_settings() {
    let rider: Rider = toml::from_str(
        r#"
model = "articulated_sagittal_v1"
[articulation]
joint_kp = 750.0
joint_kd = 45.0
joint_max_nm = 120.0
[control]
command_delay_s = 0.05
feedback_delay_s = 0.02
"#,
    )
    .unwrap();
    assert_eq!(rider.model, RiderModel::ArticulatedSagittalV1);
    assert_eq!(rider.articulation.joint_kp, 750.0);
    assert_eq!(rider.articulation.joint_kd, 45.0);
    assert_eq!(rider.articulation.joint_max_nm, 120.0);
    assert_eq!(rider.control.command_delay_s, 0.05);
    assert_eq!(rider.control.feedback_delay_s, 0.02);
    let mut model = setup(rider.model);
    model.rider = rider;
    assert!(Physics::new(model).is_ok());
    assert!(toml::from_str::<Rider>("[articulation]\nunknown_gain = 1.0").is_err());
}

#[test]
fn zero_ankle_cap_remains_a_valid_disabled_actuator_configuration() {
    let mut model = setup(RiderModel::ArticulatedSagittalV1);
    model.rider.control.ankle_max_nm = 0.0;
    let physics = Physics::new(model).unwrap();
    assert!(physics.rider_joints().iter().filter(|j| j.name.ends_with("ankle")).all(|j| j.torque_nm == 0.0));
}

/// Same test-only motor law as tests/physics.rs. It replaces firmware, not rider mechanics.
fn balanced_step(physics: &mut Physics) -> f64 {
    let state = physics.state();
    let torque = if physics.held() {
        0.0
    } else {
        (-2500.0 * state.pitch_rad + 250.0 * physics.imu().gyro_rad_s[1]).clamp(-300.0, 300.0)
    };
    physics.step(torque);
    assert_attached_physics(physics);
    torque
}

fn assert_attached_physics(physics: &Physics) {
    let state = physics.state();
    assert!(!physics.rider_off() && !state.fallen, "{state:?}");
    assert!(physics.poses().iter().all(|pose| pose.to_array().iter().all(|v| v.is_finite())));
    let rider = physics.setup().rider;
    for joint in physics.rider_joints() {
        let cap = if joint.name.ends_with("ankle") {
            rider.control.ankle_max_nm / 2.0
        } else {
            rider.articulation.joint_max_nm
        };
        assert!(joint.angle_rad.is_finite() && joint.rate_rad_s.is_finite() && joint.torque_nm.is_finite());
        assert!(joint.torque_nm.abs() <= cap, "{joint:?}, cap={cap}");
    }
}

fn balanced_for(physics: &mut Physics, ticks: usize) {
    for _ in 0..ticks {
        balanced_step(physics);
    }
}

fn moving_articulated() -> Physics {
    let mut physics = Physics::new(setup(RiderModel::ArticulatedSagittalV1)).unwrap();
    balanced_for(&mut physics, 1000);
    physics.set_held(false);
    physics.rider().set_intent(Intent::Lean(0.04));
    balanced_for(&mut physics, 6000);
    assert!(physics.state().speed_mps > 0.1, "the test requires physical motion before changing input");
    physics
}

#[test]
fn neutral_removes_lean_request_without_resetting_speed_or_pose() {
    let mut neutral = moving_articulated();
    let mut continuing = moving_articulated();
    let before = neutral.state();
    let wheel = neutral.wheel();
    let poses = neutral.poses();
    neutral.rider().set_intent(Intent::Lean(0.0));
    assert_eq!(neutral.state(), before, "an input request must not change physical state");
    assert_eq!(neutral.wheel(), wheel);
    assert_eq!(neutral.poses(), poses);
    balanced_for(&mut neutral, 4000);
    balanced_for(&mut continuing, 4000);
    let end = neutral.state();
    let control = continuing.state();
    println!("neutral: before={before:?}, after={end:?}, continued lean={control:?}");
    assert_eq!(neutral.rider_state().desired_lean_rad, 0.0);
    assert_eq!(neutral.rider_state().commanded_lean_rad, 0.0);
    assert!(end.rider_lean_rad.abs() < before.rider_lean_rad.abs(), "the body must return toward vertical");
    assert!(end.speed_mps > 0.0 && end.distance_m > before.distance_m, "neutral is not an instantaneous stop");
    assert!(end.speed_mps < control.speed_mps, "neutral must reduce acceleration versus the matched lean control");
}

#[test]
fn controlled_braking_reduces_forward_speed_with_negative_axle_work() {
    let mut physics = Physics::new(setup(RiderModel::ArticulatedSagittalV1)).unwrap();
    balanced_for(&mut physics, 1000);
    physics.set_held(false);
    physics.rider().set_intent(Intent::Speed(2.0));
    balanced_for(&mut physics, 16000);
    let cruising = physics.state();
    assert!(cruising.speed_mps > 1.0, "a braking test must first carry real forward momentum: {cruising:?}");
    physics.rider().set_intent(Intent::Speed(0.0));
    let mut work = 0.0;
    let mut minimum_lean = 0.0_f64;
    let mut minimum_torque = 0.0_f64;
    for _ in 0..20000 {
        let rate = physics.wheel().1;
        let torque = balanced_step(&mut physics);
        work += torque * rate * TIMESTEP_S;
        minimum_torque = minimum_torque.min(torque);
        minimum_lean = minimum_lean.min(physics.rider_state().commanded_lean_rad);
    }
    let stopped = physics.state();
    println!(
        "brake: cruise={cruising:?}, stopped={stopped:?}, work={work:.6} J, minimum lean={minimum_lean:.6} rad, minimum torque={minimum_torque:.6} Nm"
    );
    // Same 10 s and 0.3 m/s stop criterion as the existing rigid speed/brake regression.
    assert!(stopped.speed_mps.abs() < 0.3, "the unchanged speed controller must stop: {stopped:?}");
    assert!(stopped.speed_mps.abs() < cruising.speed_mps);
    assert!(minimum_lean < 0.0 && minimum_torque < 0.0 && work < 0.0, "braking must remove energy through the axle");
    assert!(stopped.distance_m > cruising.distance_m, "the stop must use a physical stopping distance");
}

#[test]
fn articulated_rider_crosses_mild_uphill_and_downhill_grades() {
    for grade in [2.0, -2.0] {
        let mut model = setup(RiderModel::ArticulatedSagittalV1);
        model.terrain =
            Terrain { slope_pct: grade, slope_start_m: 2.0, slope_ramp_m: 2.0, length_m: 30.0, ..Terrain::default() };
        let mut physics = Physics::new(model).unwrap();
        balanced_for(&mut physics, 1000);
        let start_z = physics.poses()[0].pos[2];
        physics.set_held(false);
        physics.rider().set_intent(Intent::Speed(1.5));
        balanced_for(&mut physics, 16000);
        let state = physics.state();
        let x = physics.position_m()[0];
        let height = physics.setup().terrain.height(x);
        let rise = f64::from(physics.poses()[0].pos[2] - start_z);
        println!("grade {grade}%: x={x:.6} m, ground={height:.6} m, wheel rise={rise:.6} m, state={state:?}");
        assert!(x > 4.0 && x < 29.0, "cross the entire grade ramp and remain on modeled ground");
        assert!(state.speed_mps > 0.0 && height * grade > 0.0);
        // Ten millimetres covers contact compliance and torus facets. A flat response cannot
        // pass after crossing the ramp; this is numerical contact acceptance, not calibration.
        assert!((rise - height).abs() < 0.01, "the wheel must physically follow the ground height");
    }
}

#[test]
fn articulated_rider_crosses_a_small_bump_and_returns_to_ground_level() {
    let mut model = setup(RiderModel::ArticulatedSagittalV1);
    model.terrain = Terrain {
        length_m: 30.0,
        features: vec![euc_physics::Feature::Bump { at_m: 2.0, height_m: 0.01, length_m: 1.0 }],
        ..Terrain::default()
    };
    let mut physics = Physics::new(model).unwrap();
    balanced_for(&mut physics, 1000);
    let start_z = f64::from(physics.poses()[0].pos[2]);
    physics.set_held(false);
    physics.rider().set_intent(Intent::Speed(1.0));
    let mut crest_rise = f64::NEG_INFINITY;
    for _ in 0..16000 {
        balanced_step(&mut physics);
        if (2.4..=2.6).contains(&physics.position_m()[0]) {
            crest_rise = crest_rise.max(f64::from(physics.poses()[0].pos[2]) - start_z);
        }
    }
    let x = physics.position_m()[0];
    let final_rise = f64::from(physics.poses()[0].pos[2]) - start_z;
    println!(
        "small bump: crest rise={crest_rise:.6} m, final rise={final_rise:.6} m, x={x:.6} m, state={:?}",
        physics.state()
    );
    assert!(x > 4.0 && x < 29.0, "must cross the whole bump and retain ground beyond it");
    assert!(crest_rise > 0.005, "a physical wheel must rise over the 10 mm bump");
    assert!(final_rise.abs() < 0.005, "the wheel must return to the original flat-ground height");
    assert!(physics.state().speed_mps > 0.0);
}

fn airborne_rider(physics: &Physics) {
    use euc_telemetry::{BodyRole, GeomKind};
    for (body, pose) in physics.bodies().iter().zip(physics.poses()) {
        if body.role != BodyRole::Rider {
            continue;
        }
        for geom in &body.geoms {
            let [a, b, c] = geom.size.map(f64::from);
            let radius = match geom.kind {
                GeomKind::Capsule => a + b,
                GeomKind::Sphere => a,
                GeomKind::Box => (a * a + b * b + c * c).sqrt(),
                _ => panic!("unexpected human shape"),
            };
            let offset = geom.pos.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>().sqrt();
            assert!(f64::from(pose.pos[2]) > radius + offset, "{} must have no ground contact", body.name);
        }
    }
}

#[test]
fn moving_release_keeps_visible_state_and_airborne_horizontal_momentum() {
    let mut physics = moving_articulated();
    let mass = physics.total_mass();
    let previous_com = physics.rider_com_m();
    balanced_step(&mut physics);
    let before_com = physics.rider_com_m();
    let before_velocity = (before_com[0] - previous_com[0]) / TIMESTEP_S;
    assert!(before_velocity > 0.1, "release must begin with moving rider mass");
    let before = (physics.time_s(), physics.poses(), physics.wheel(), physics.rider_position_m(), before_com);
    let joints = physics.rider_joints();
    physics.step_off();
    assert_eq!(
        (physics.time_s(), physics.poses(), physics.wheel(), physics.rider_position_m(), physics.rider_com_m()),
        before
    );
    for (old, new) in joints.iter().zip(physics.rider_joints()) {
        assert_eq!((old.angle_rad, old.rate_rad_s), (new.angle_rad, new.rate_rad_s));
        assert_eq!(new.torque_nm, 0.0);
    }
    // Kinematic exports are one physics step old. Discard the first post-release export so
    // the following COM difference covers a genuinely released integration interval.
    physics.step(0.0);
    let released_com = physics.rider_com_m();
    physics.step(0.0);
    let first_com = physics.rider_com_m();
    let released_velocity = (first_com[0] - released_com[0]) / TIMESTEP_S;
    assert!(
        (released_velocity - before_velocity).abs() < 0.01,
        "no release impulse within finite-difference resolution"
    );
    for _ in 0..100 {
        physics.step(0.0);
        airborne_rider(&physics);
        let state = physics.rider_state();
        assert_eq!((state.ankle_torque_nm, state.yaw_aid_torque_nm, state.roll_aid_torque_nm), (0.0, 0.0, 0.0));
        assert!(physics.rider_joints().iter().all(|joint| joint.torque_nm == 0.0));
    }
    let final_velocity = (physics.rider_com_m()[0] - first_com[0]) / (100.0 * TIMESTEP_S);
    println!(
        "moving release: COM vx before={before_velocity:.9}, first released={released_velocity:.9}, airborne mean={final_velocity:.9} m/s"
    );
    // Gravity has no horizontal component. The entire rider subtree is airborne; joint
    // damping is internal and cannot change its total horizontal linear momentum.
    assert!((final_velocity - released_velocity).abs() < 1e-5, "airborne rider COM must retain horizontal velocity");
    assert_eq!(physics.total_mass(), mass);
    assert!(physics.rider_off());
}

/// The lean of the whole rider: the angle from vertical of the line from the ankles (the mean of
/// the two foot body origins) to the rider subtree's centre of mass, along the shell heading. For
/// the rigid rider this is its hinge angle; it is what the acceleration answers to.
fn centre_of_mass_lean(physics: &Physics) -> f64 {
    let com = physics.rider_com_m();
    let (bodies, poses) = (physics.bodies(), physics.poses());
    let feet: Vec<[f64; 3]> = bodies
        .iter()
        .zip(&poses)
        .filter(|(body, _)| body.name.ends_with("_foot"))
        .map(|(_, pose)| pose.pos.map(f64::from))
        .collect();
    assert_eq!(feet.len(), 2);
    let ankles: [f64; 3] = std::array::from_fn(|i| f64::midpoint(feet[0][i], feet[1][i]));
    let offset: [f64; 3] = std::array::from_fn(|i| com[i] - ankles[i]);
    let heading = physics.heading_rad();
    (offset[0] * heading.cos() + offset[1] * heading.sin()).atan2(offset[2])
}

#[test]
fn the_body_leans_from_the_ankles_so_the_centre_of_mass_follows_the_upper_body() {
    let mut physics = Physics::new(setup(RiderModel::ArticulatedSagittalV1)).unwrap();
    balanced_for(&mut physics, 1000);
    physics.set_held(false);
    physics.rider().set_intent(Intent::Speed(3.3));
    balanced_for(&mut physics, 24000);
    let (mut worst, mut upper, mut whole) = (0.0_f64, 0.0, 0.0);
    let (start, start_speed) = (physics.time_s(), physics.state().speed_mps);
    for _ in 0..4000 {
        balanced_step(&mut physics);
        let (trunk, com) = (physics.rider_state().measured_lean_rad, centre_of_mass_lean(&physics));
        worst = worst.max((trunk - com).abs());
        upper += trunk / 4000.0;
        whole += com / 4000.0;
    }
    let accel = (physics.state().speed_mps - start_speed) / (physics.time_s() - start);
    println!(
        "steady 3.3 m/s: upper body {:.3}°, centre of mass {:.3}°, largest gap {:.3}°, acceleration {accel:.4} m/s²",
        upper.to_degrees(),
        whole.to_degrees(),
        worst.to_degrees()
    );
    // A straight body: the two leans agree in steady riding. Before the ankle fix the trunk
    // pitched forward while the legs angled back, and the two parted by degrees.
    assert!(worst < 0.5_f64.to_radians(), "the legs bend against the lean: gap {}°", worst.to_degrees());
    assert!(whole > 0.0, "the rider leans into the drag at speed");
}
