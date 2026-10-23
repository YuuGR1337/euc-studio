//! Turning: the rider leans the wheel over and the cambered tire carves the curve. Balanced by
//! the plain PD law of tests/physics.rs instead of firmware; both rider models.

use euc_physics::{
    Intent, Physics, RiderModel, Setup, Shapes, Shell, TIMESTEP_S, Tire, Torus, Wheel, WheelBody, wrap_angle,
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
        shapes: Some(Shapes { tire: Some(Torus { major_m: 0.1716, minor_m: 0.058 }), solids: Vec::new() }),
    });
    setup.rider.model = model;
    setup
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

fn ride(physics: &mut Physics, seconds: f64) {
    for _ in 0..(seconds / TIMESTEP_S).round() as usize {
        let torque = balance(physics);
        physics.step(torque);
    }
}

/// By hand (Free Ride, Ride's manual mode) a stopped rider twists the wheel round on the spot:
/// the line turns at the requested rate and the twist aid, within its 60 N·m cap, follows it.
#[test]
fn by_hand_the_rider_pivots_the_wheel_at_a_stop() {
    for model in [RiderModel::RigidAnkleV1, RiderModel::ArticulatedSagittalV1, RiderModel::ArticulatedLateralV1] {
        let mut physics = Physics::new(setup(model)).unwrap();
        ride(&mut physics, 0.5);
        physics.set_held(false);
        ride(&mut physics, 0.5);
        let heading = physics.heading_rad();
        physics.rider().set_manual(true);
        physics.rider().set_turn_rate(1.5);
        let (mut fastest, mut last) = (0.0_f64, heading);
        for _ in 0..(2.0 / TIMESTEP_S) as usize {
            let torque = balance(&physics);
            physics.step(torque);
            let now = physics.heading_rad();
            fastest = fastest.max((wrap_angle(now - last) / TIMESTEP_S).abs());
            last = now;
        }
        let turned = wrap_angle(physics.heading_rad() - heading);
        physics.rider().set_turn_rate(0.0);
        ride(&mut physics, 2.0);
        let settled = wrap_angle(physics.heading_rad() - heading);
        let state = physics.state();
        println!(
            "{model:?} pivot: {turned:.3} rad in 2 s (asked 3.0), fastest {fastest:.2} rad/s, settled at {settled:.3} rad, speed {:.3} m/s, roll {:.2}°",
            state.speed_mps,
            state.roll_rad.to_degrees()
        );
        assert!(!state.fallen && !physics.rider_off(), "{model:?}: {state:?}");
        assert!(turned > 2.5, "{model:?} turns on the spot: {turned} rad");
        assert!((settled - physics.rider().line()).abs() < 0.05, "{model:?} keeps the heading reached: {settled}");
        assert!(state.speed_mps.abs() < 0.1, "{model:?} stays put: {} m/s", state.speed_mps);
    }
}

/// By hand at 5 m/s, a full turn request is capped by the lean it needs: the rider leans the
/// wheel to the manual limit (35°, about 0.7 g) and carves a circle the tire holds.
#[test]
fn by_hand_a_full_turn_at_speed_leans_to_the_manual_limit() {
    for model in [RiderModel::RigidAnkleV1, RiderModel::ArticulatedSagittalV1, RiderModel::ArticulatedLateralV1] {
        let mut physics = Physics::new(setup(model)).unwrap();
        ride(&mut physics, 0.5);
        physics.set_held(false);
        physics.rider().set_manual(true);
        physics.rider().set_intent(Intent::Speed(5.0));
        ride(&mut physics, 8.0);
        let speed = physics.state().speed_mps;
        let gravity = physics.gravity();
        let cap = physics.rider().manual_turn_cap(speed, gravity);
        physics.rider().set_turn_rate(10.0);
        ride(&mut physics, 2.0);
        let (mut yaw, mut roll, mut v, n) = (0.0, 0.0, 0.0, (2.0 / TIMESTEP_S) as usize);
        for _ in 0..n {
            let torque = balance(&physics);
            let before = physics.heading_rad();
            physics.step(torque);
            let state = physics.state();
            yaw += wrap_angle(physics.heading_rad() - before) / TIMESTEP_S / n as f64;
            roll += state.roll_rad / n as f64;
            v += state.speed_mps / n as f64;
        }
        let state = physics.state();
        println!(
            "{model:?} full turn: speed {v:.2} m/s, yaw {yaw:.3} rad/s (cap {cap:.3} at {speed:.2}), roll {:.1}°, radius {:.2} m",
            roll.to_degrees(),
            v / yaw
        );
        assert!(!state.fallen && !physics.rider_off(), "{model:?}: {state:?}");
        // At the cap the turn's lateral acceleration v·rate is the manual lean's g·tan(35°).
        let lateral = gravity * physics.setup().rider.control.manual_turn_lean_max_deg.to_radians().tan();
        assert!((yaw * v - lateral).abs() < 0.1 * lateral, "{model:?}: {} m/s² of {lateral}", yaw * v);
        assert!((30.0..36.0).contains(&-roll.to_degrees()), "{model:?} leans into the turn: {}°", roll.to_degrees());
    }
}

#[test]
fn scripted_turn_input_does_not_spin_a_stationary_wheel() {
    for model in [RiderModel::RigidAnkleV1, RiderModel::ArticulatedSagittalV1, RiderModel::ArticulatedLateralV1] {
        let mut physics = Physics::new(setup(model)).unwrap();
        ride(&mut physics, 0.5);
        physics.set_held(false);
        ride(&mut physics, 0.5);
        let heading = physics.heading_rad();
        physics.rider().set_turn_rate(0.5);
        ride(&mut physics, 3.0);
        let turned = wrap_angle(physics.heading_rad() - heading);
        assert!(turned.abs() < 0.01, "{model:?} must not spin at rest: {turned} rad");
        assert!(physics.state().speed_mps.abs() < 0.01);
        assert!(!physics.state().fallen && !physics.rider_off());
    }
}

#[test]
fn rigid_rider_keeps_a_straight_line_at_high_speed() {
    let mut physics = Physics::new(setup(RiderModel::RigidAnkleV1)).unwrap();
    ride(&mut physics, 0.5);
    physics.set_held(false);
    physics.rider().set_intent(Intent::Speed(20.0));
    for _ in 0..(30.0 / TIMESTEP_S).round() as usize {
        let torque = balance(&physics);
        physics.step(torque);
        assert!(!physics.state().fallen && !physics.rider_off(), "straight run: {:?}", physics.state());
        assert!(physics.state().roll_rad.abs() < 0.1, "straight roll: {:?}", physics.state());
    }
    assert!(physics.state().speed_mps > 18.0, "high-speed run: {:?}", physics.state());
}

#[test]
fn a_steady_turn_input_leans_the_wheel_into_a_circle_of_plausible_radius() {
    for model in [RiderModel::RigidAnkleV1, RiderModel::ArticulatedSagittalV1, RiderModel::ArticulatedLateralV1] {
        let mut physics = Physics::new(setup(model)).unwrap();
        ride(&mut physics, 0.5);
        physics.set_held(false);
        physics.rider().set_intent(Intent::Speed(15.0 / 3.6));
        // Prepare at the same measured speed. The original articulated rider needs longer to
        // accelerate than the rigid model; its gains and physical lean coordinate stay unchanged.
        for _ in 0..(20.0 / TIMESTEP_S).round() as usize {
            let torque = balance(&physics);
            physics.step(torque);
            if physics.state().speed_mps > 4.0 {
                break;
            }
        }
        let before = physics.state();
        assert!(before.speed_mps > 4.0 && before.roll_rad.abs() < 0.01, "{model:?} rides straight: {before:?}");

        // Turn left at 0.4 rad/s: a 10 m circle at 15 km/h, needing tan(lean) = v·Ω/g, about 9.5°.
        physics.rider().set_turn_rate(0.4);
        ride(&mut physics, 4.0);
        let start = (physics.heading_rad(), physics.position_m(), physics.state().distance_m);
        let (mut roll, mut yaw_aid) = (0.0_f64, 0.0_f64);
        let steps = (4.0 / TIMESTEP_S).round() as usize;
        for _ in 0..steps {
            let torque = balance(&physics);
            physics.step(torque);
            let state = physics.state();
            assert!(!state.fallen && !physics.rider_off(), "{model:?} stays up through the turn: {state:?}");
            roll += state.roll_rad / steps as f64;
            yaw_aid = yaw_aid.max(physics.rider_state().yaw_aid_torque_nm.abs());
        }
        let state = physics.state();
        let turned = wrap_angle(physics.heading_rad() - start.0);
        let yaw_rate = turned / 4.0;
        let radius = state.speed_mps / yaw_rate;
        let expected_lean = -(state.speed_mps * 0.4 / physics.gravity()).atan();
        let position = physics.position_m();
        println!(
            "{model:?}: v {:.2} m/s, yaw {yaw_rate:.3} rad/s, radius {radius:.1} m, roll {:.1}° (steady turn {:.1}°), \
             twist aid at most {yaw_aid:.1} N·m, moved ({:.1}, {:.1}) m",
            state.speed_mps,
            roll.to_degrees(),
            expected_lean.to_degrees(),
            position[0] - start.1[0],
            position[1] - start.1[1]
        );
        assert!((yaw_rate - 0.4).abs() < 0.05, "{model:?} turns at the rate asked: {yaw_rate} rad/s");
        assert!((7.0..14.0).contains(&radius), "{model:?} circles with a plausible radius: {radius} m");
        assert!(roll < -0.1 && (roll - expected_lean).abs() < 3f64.to_radians(), "{model:?} leans left: {roll}");
        // The curve comes from the lean, not from a twist: the aid is a few N·m against the
        // hundreds the roll spring and gravity exchange, and the wheel rolls along the arc.
        assert!(yaw_aid < 5.0, "{model:?} twist aid {yaw_aid} N·m");
        let arc = state.distance_m - start.2;
        let chord = (position[0] - start.1[0]).hypot(position[1] - start.1[1]);
        let expected_chord = 2.0 * radius.abs() * (turned / 2.0).sin();
        assert!(
            (chord - expected_chord).abs() < 0.15 * arc,
            "{model:?} rolls along the arc: chord {chord} m, arc {arc} m"
        );
        assert!(position[1] > start.1[1], "{model:?} moved left");

        // Back to a straight line: the rider keeps the heading reached and stands the wheel up.
        physics.rider().set_turn_rate(0.0);
        let line = physics.rider().line();
        ride(&mut physics, 6.0);
        let state = physics.state();
        assert!(state.roll_rad.abs() < 0.01, "{model:?} upright again: {}°", state.roll_rad.to_degrees());
        assert!(wrap_angle(physics.heading_rad() - line).abs() < 0.03, "{model:?} keeps the heading reached");
    }
}
