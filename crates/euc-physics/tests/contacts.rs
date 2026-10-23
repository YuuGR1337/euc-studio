//! The contact digest for sound (`Physics::take_contact_digest`): a pothole's edge hits the tyre,
//! steady riding does not; a wheel that falls over scrapes and its rider thuds, then lies still.
//! Balanced by the plain PD law of tests/physics.rs instead of firmware.

use euc_physics::{Assist, ContactDigest, Feature, Intent, Physics, Setup, TIMESTEP_S, Terrain, WheelBody};

/// The Begode Extreme's `[body]` from its profile.
fn extreme() -> WheelBody {
    let plant: toml::Value = toml::from_str(include_str!("../../../profiles/begode-extreme/plant.toml")).unwrap();
    plant["body"].clone().try_into().unwrap()
}

/// Motor torque that keeps the pedals level, none once the wheel lies on its side.
fn balance(physics: &Physics) -> f64 {
    let state = physics.state();
    if physics.held() || state.roll_rad.abs() > 0.5 {
        return 0.0;
    }
    (-2500.0 * state.pitch_rad + 250.0 * physics.imu().gyro_rad_s[1]).clamp(-300.0, 300.0)
}

/// Rides `seconds` in sixtieths of a second, like the runner's samples: each sample's digest.
fn samples(physics: &mut Physics, seconds: f64) -> Vec<(f64, ContactDigest)> {
    let steps = (1.0 / 60.0 / TIMESTEP_S).round() as usize;
    (0..(seconds * 60.0).round() as usize)
        .map(|_| {
            for _ in 0..steps {
                let torque = balance(physics);
                physics.step(torque);
            }
            (physics.position_m()[0], physics.take_contact_digest())
        })
        .collect()
}

#[test]
fn a_pothole_edge_hits_the_tyre_and_steady_riding_does_not() {
    let mut setup = Setup::new(extreme());
    setup.terrain = Terrain {
        length_m: 60.0,
        features: vec![Feature::Pothole { at_m: 20.0, depth_m: 0.03, length_m: 0.5 }],
        ..Terrain::default()
    };
    let mut physics = Physics::new(setup).unwrap();
    samples(&mut physics, 0.5);
    physics.set_held(false);
    physics.rider().set_intent(Intent::Speed(12.0 / 3.6));
    let ride = samples(&mut physics, 10.0);
    let most = |from: f64, to: f64| {
        ride.iter().filter(|(x, _)| (from..to).contains(x)).map(|(_, d)| d.tyre_impact_ns).fold(0.0, f64::max)
    };
    let (steady, hit) = (most(8.0, 18.0), most(19.0, 22.0));
    let rate =
        ride.iter().filter(|(x, _)| (19.0..22.0).contains(x)).map(|(_, d)| d.tyre_load_rate_n_s).fold(0.0, f64::max);
    println!(
        "tyre: steady samples up to {steady:.3} N·s, at the pothole {hit:.3} N·s, load rising up to {rate:.0} N/s"
    );
    assert!(ride.iter().any(|(x, _)| *x > 22.0), "rides past the pothole");
    assert!(hit > 5.0 * steady.max(0.01), "the pothole's edge is a hit: {hit} N·s against {steady}");
    assert!(ride.iter().all(|(_, d)| d.shell_contact_n == 0.0 && d.rider_impact_ns == 0.0), "only the tyre touches");
}

#[test]
fn a_wheel_that_falls_over_scrapes_its_rider_thuds_then_lies_still() {
    let mut setup = Setup::new(extreme());
    setup.assist = Assist { roll_stiffness: 0.0, roll_damping: 0.0 };
    let mut physics = Physics::new(setup).unwrap();
    samples(&mut physics, 0.5);
    physics.set_held(false);
    physics.rider().set_intent(Intent::Speed(3.0));
    // Asked to turn with nothing holding the roll, the wheel tips over (the flat plane alone keeps
    // it upright to 1e-14 rad).
    physics.rider().set_turn_rate(1.0);
    let fall = samples(&mut physics, 6.0);
    let shell_force = fall.iter().map(|(_, d)| d.shell_contact_n).fold(0.0, f64::max);
    let shell_impact: f64 = fall.iter().map(|(_, d)| d.shell_impact_ns).sum();
    let rider_impact: f64 = fall.iter().map(|(_, d)| d.rider_impact_ns).sum();
    let parts: Vec<_> = fall.iter().filter_map(|(_, d)| d.rider_part).collect();
    let slip = fall.iter().map(|(_, d)| d.shell_slip_mps).fold(0.0, f64::max);
    let still = samples(&mut physics, 1.0);
    let resting = still.iter().map(|(_, d)| d.rider_impact_ns + d.shell_impact_ns).fold(0.0, f64::max);
    println!(
        "fall: shell up to {shell_force:.0} N slipping up to {slip:.2} m/s, shell impulse {shell_impact:.2} N·s, rider \
         impulse {rider_impact:.2} N·s on {parts:?}; then lying, at most {resting:.4} N·s a sample; {:?}",
        physics.state()
    );
    assert!(physics.state().roll_rad.abs() > 1.0, "the wheel lies on its side");
    assert!(shell_force > 50.0 && shell_impact > 0.5, "the shell hits the road");
    assert!(rider_impact > 0.5 && !parts.is_empty(), "the rider thuds");
    assert!(resting < 0.05, "lying still makes no new thud: {resting} N·s");
}
