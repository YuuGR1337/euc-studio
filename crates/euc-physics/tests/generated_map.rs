//! Collision acceptance for the exact height grid sent to the Free Ride view.
use euc_physics::{GeneratedMapSpec, HeightMap, MapFeature, MapFeatureKind};
use mujoco_rs::prelude::*;

fn world(map: &HeightMap, bodies: &str) -> MjData<Box<MjModel>> {
    let xml =
        format!("<mujoco>{}<worldbody>{}{bodies}</worldbody></mujoco>", map.asset().unwrap(), map.ground().unwrap());
    let mut data = MjData::new(Box::new(MjModel::from_xml_string(&xml).unwrap()));
    data.forward();
    data
}

fn assert_surface(data: &mut MjData<Box<MjModel>>, map: &HeightMap, x: f64, y: f64) {
    let expected = map.sample(x, y).unwrap();
    let mut normal = [0.0; 3];
    let (hit, distance) = data.ray(&[x, y, 20.0], &[0.0, 0.0, -1.0], None, true, None, Some(&mut normal));
    assert!(hit.is_some(), "missing ground at ({x}, {y})");
    let height = 20.0 - distance;
    assert!((height - expected.height_m).abs() < 0.001, "({x}, {y}): collision {height}, grid {expected:?}");
    for (actual, expected) in normal.into_iter().zip(expected.normal_world) {
        assert!((actual - expected).abs() < 0.001, "({x}, {y}): collision normal {normal:?}");
    }
}

#[test]
fn generated_collision_matches_both_triangles_in_every_grid_cell() {
    let mut spec = GeneratedMapSpec::flat(71);
    spec.grade_pct = [8.0, -3.0];
    spec.features = vec![
        MapFeature { kind: MapFeatureKind::Bump, centre_xy_m: [5.0, -4.0], width_xy_m: [3.0, 2.0], size_m: 0.12 },
        MapFeature { kind: MapFeatureKind::Pothole, centre_xy_m: [-6.0, 5.0], width_xy_m: [2.0, 3.0], size_m: 0.08 },
    ];
    let map = spec.generate(1).unwrap();
    let mut data = world(&map, "");
    // Non-diagonal interior points distinguish the two planes. Asymmetric x/y
    // grades and features detect row reversal, transposition, and axis errors.
    for iy in 0..usize::from(map.ny - 1) {
        for ix in 0..usize::from(map.nx - 1) {
            for [u, v] in [[0.72, 0.19], [0.21, 0.76]] {
                let x = map.origin_xy_m[0] + (ix as f64 + u) * map.spacing_m;
                let y = map.origin_xy_m[1] + (iy as f64 + v) * map.spacing_m;
                assert_surface(&mut data, &map, x, y);
            }
        }
    }
}

#[test]
fn flat_surface_has_no_ground_outside_its_bounds() {
    let map = GeneratedMapSpec::flat(0).generate(1).unwrap();
    let mut data = world(&map, "");
    for [x, y] in [[0.13, 0.17], [-15.99, -15.99], [15.99, 15.99]] {
        assert_surface(&mut data, &map, x, y);
    }
    for [x, y] in [[-16.01, 0.0], [16.01, 0.0], [0.0, -16.01], [0.0, 16.01]] {
        assert!(map.sample(x, y).is_none());
        let (hit, _) = data.ray(&[x, y, 20.0], &[0.0, 0.0, -1.0], None, true, None, None);
        assert!(hit.is_none(), "unexpected ground outside map at ({x}, {y})");
    }
}

#[test]
fn downhill_surface_remains_below_zero_without_a_hidden_floor_plane() {
    let mut spec = GeneratedMapSpec::flat(0);
    spec.grade_pct = [-5.0, -2.0];
    let map = spec.generate(1).unwrap();
    let mut data = world(&map, "");
    for [x, y] in [[3.13, 2.17], [10.13, 5.17], [15.99, 15.99]] {
        assert!(map.sample(x, y).unwrap().height_m < -0.1);
        assert_surface(&mut data, &map, x, y);
    }
}

#[test]
fn a_body_settles_on_the_generated_pothole_bottom() {
    let mut spec = GeneratedMapSpec::flat(0);
    spec.features.push(MapFeature {
        kind: MapFeatureKind::Pothole,
        centre_xy_m: [5.0, 4.0],
        width_xy_m: [2.0, 2.0],
        size_m: 0.08,
    });
    let map = spec.generate(1).unwrap();
    let mut data = world(
        &map,
        "<body name=\"probe\" pos=\"5 4 0.6\"><freejoint/><geom type=\"box\" size=\"0.1 0.1 0.1\" mass=\"1\"/></body>",
    );
    let body = data.model().name_to_id(MjtObj::mjOBJ_BODY, "probe").unwrap();
    for _ in 0..3000 {
        data.step();
    }
    let expected = map.sample(5.0, 4.0).unwrap().height_m + 0.1;
    assert!((data.xpos()[body][2] - expected).abs() < 0.005, "probe {:?}, expected {expected}", data.xpos()[body]);
}
