//! The streaming world as ground: the window follows the wheel, the contacts see the world's
//! heights, and curb boxes stop a wheel that would otherwise roll through a curb. Balanced by the
//! plain PD law of tests/physics.rs instead of firmware.

use std::f64::consts::FRAC_PI_2;

use euc_physics::world::{RECENTRE_M, WINDOW_HALF_M};
use euc_physics::{
    Intent, Physics, RiderModel, Setup, Shapes, Shell, TIMESTEP_S, Tire, Torus, Wheel, WheelBody, WorldGround,
    WorldStart,
};
use euc_world::WorldSpec;

/// The Begode Extreme without its suspension, with the scanned tire torus (as tests/turning.rs).
fn setup(model: RiderModel, ground: WorldGround) -> Setup {
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
    setup.world = Some(ground);
    setup
}

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

#[test]
fn the_window_follows_the_wheel_down_the_spawn_street() {
    for model in [RiderModel::RigidAnkleV1, RiderModel::ArticulatedSagittalV1, RiderModel::ArticulatedLateralV1] {
        let ground = WorldGround::at_spawn(WorldSpec::city(7)).unwrap();
        let mut physics = Physics::new(setup(model, ground.clone())).unwrap();
        // The window and the box pool are not part of the moving mass (the rider's drag sense
        // divides by it): wheel 16 kg, shell 26 kg and rider 80 kg, as without a world.
        let mut flat = setup(model, ground.clone());
        flat.world = None;
        assert!((physics.total_mass() - 122.0).abs() < 1e-9, "{model:?}: {} kg", physics.total_mass());
        assert_eq!(physics.total_mass(), Physics::new(flat).unwrap().total_mass(), "{model:?}");
        ride(&mut physics, 0.5);
        physics.set_held(false);
        physics.rider().set_intent(Intent::Speed(5.0));
        for _ in 0..(11.0 / TIMESTEP_S).round() as usize {
            let torque = balance(&physics);
            physics.step(torque);
            let state = physics.state();
            assert!(!state.fallen && !physics.rider_off(), "{model:?}: fell on the spawn street: {state:?}");
        }
        let p = physics.position_m();
        assert!(p[0] > 25.0, "{model:?}: only reached x = {:.1} m", p[0]);
        let (recentres, _, _) = physics.world_upkeep().unwrap();
        assert!(recentres as f64 >= p[0] / (2.0 * RECENTRE_M), "{model:?}: {recentres} recentres for {:.1} m", p[0]);
        // The contacts see the world: ray heights match the world's drawn heights near the wheel
        // (away from the rider), to the window's resolution on the crowned road.
        for dx in [-3.0, -1.5, 1.5, 3.0] {
            for dy in [-2.0, 2.0] {
                let xy = [p[0] + dx, p[1] + dy];
                let ray = physics.ground_ray_m(xy).expect("ground under the window");
                let world = ground.height_m(xy);
                assert!((ray - world).abs() < 0.003, "{model:?}: ray {ray:.4} vs world {world:.4} at {xy:?}");
            }
        }
        // Beyond the window there is no ground.
        assert!(physics.ground_ray_m([p[0] + WINDOW_HALF_M + 2.0, p[1]]).is_none());
    }
}

/// Heading straight at the curb 5.5 m to the right of the spawn: the curb box must either be
/// climbed or stop the wheel; the wheel never ends up inside the sidewalk.
#[test]
fn a_curb_box_is_climbed_or_stops_the_wheel_never_passed_through() {
    let start = WorldStart { xy_m: [0.0, 0.0], heading_rad: -FRAC_PI_2 };
    let ground = WorldGround::new(WorldSpec::city(7), start).unwrap();
    let mut physics = Physics::new(setup(RiderModel::RigidAnkleV1, ground.clone())).unwrap();
    ride(&mut physics, 0.5);
    physics.set_held(false);
    physics.rider().set_intent(Intent::Speed(2.0));
    let mut pooled = 0;
    for _ in 0..(5.0 / TIMESTEP_S).round() as usize {
        let torque = balance(&physics);
        physics.step(torque);
        pooled = pooled.max(physics.world_upkeep().unwrap().2);
        let p = physics.position_m();
        let under = ground.height_m([p[0], p[1]]);
        // The shell origin rides about a tire radius above the ground under it; inside the curb
        // it would sit 10 cm or more too low.
        assert!(p[2] - under > 0.12, "wheel sank into the ground: shell {:.3} over ground {under:.3} at {p:?}", p[2]);
    }
    assert!(pooled > 0, "no solids reached the pool near the curb");
    let p = physics.position_m();
    assert!(p[0] > 4.0, "the wheel did not reach the curb: {p:?}");
}
