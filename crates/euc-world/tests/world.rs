//! World acceptance: determinism, chunk independence, border continuity, solids against the
//! sampler, densities, the spawn, feature size limits and a golden digest.

use std::collections::{BTreeSet, VecDeque};

use euc_world::{
    Axis, CHUNK_M, CURB_HEIGHT_M, Chunk, ChunkKey, Densities, EdgeId, FeatureKind, Material, PLAZA_STEPS,
    POTHOLE_DEPTH_M, POTHOLE_WIDTH_M, ParcelKind, RAMP_MAX_SLOPE, RISER_M, Rect, SPAWN_CLEAR_M, SPAWN_LEVEL_RADIUS_M,
    SolidKind, TREAD_M, World, WorldSpec, Zone, keys_in,
};

/// Physics face inset used by these tests: 1.5 cells of a 1/32 m height field.
const INSET_M: f64 = 1.5 / 32.0;

fn world(seed: u64) -> World {
    World::new(WorldSpec::city(seed)).unwrap()
}

fn digests(world: &World, rect: Rect) -> Vec<(ChunkKey, u64)> {
    keys_in(rect).map(|k| (k, world.chunk(k).digest())).collect()
}

#[test]
fn the_same_seed_gives_identical_chunks_and_another_seed_does_not() {
    let area = Rect::new([-40.0, -40.0], [40.0, 40.0]);
    let a = digests(&world(11), area);
    let b = digests(&world(11), area);
    assert_eq!(a, b);
    let c = digests(&world(12), area);
    assert!(a.iter().zip(&c).filter(|(x, y)| x.1 != y.1).count() > a.len() / 2);
}

#[test]
fn chunks_do_not_depend_on_generation_order() {
    let spec = WorldSpec::city(3);
    let keys: Vec<ChunkKey> = keys_in(Rect::new([-32.0, -32.0], [31.0, 31.0])).collect();
    let forward: Vec<u64> = keys.iter().map(|&k| Chunk::generate(&spec, k).digest()).collect();
    // A scrambled order through a cached world, with unrelated chunks generated in between.
    let w = World::new(spec).unwrap();
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by_key(|&i| (i * 7919) % keys.len());
    for &i in &order {
        w.chunk(ChunkKey { cx: keys[i].cx + 40, cy: keys[i].cy - 13 });
        assert_eq!(w.chunk(keys[i]).digest(), forward[i], "chunk {:?}", keys[i]);
    }
}

#[test]
fn neighbouring_chunks_agree_bit_for_bit_on_their_border() {
    for seed in [1, 5] {
        let w = world(seed);
        for cy in -3..3 {
            for cx in -3..3 {
                let a = w.chunk(ChunkKey { cx, cy });
                let right = w.chunk(ChunkKey { cx: cx + 1, cy });
                let up = w.chunk(ChunkKey { cx, cy: cy + 1 });
                let r = a.rect();
                for k in 0..=320 {
                    let t = f64::from(k) * CHUNK_M / 320.0;
                    for (b, p) in [(&right, [r.max[0], r.min[1] + t]), (&up, [r.min[0] + t, r.max[1]])] {
                        for inset in [0.0, INSET_M] {
                            assert_eq!(
                                a.ground_at(p, inset).to_bits(),
                                b.ground_at(p, inset).to_bits(),
                                "seed {seed}, {p:?}, inset {inset}"
                            );
                        }
                        assert_eq!(a.sample(p), b.sample(p), "seed {seed}, {p:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn fill_heights_matches_ground_at() {
    let w = world(9);
    let (nx, ny, cell) = (97, 61, 1.0 / 32.0);
    let origin = [-3.0, 5.5];
    let mut out = vec![0.0; nx * ny];
    w.fill_heights(origin, cell, nx, ny, INSET_M, &mut out);
    for iy in (0..ny).step_by(7) {
        for ix in (0..nx).step_by(5) {
            let (x, y) = (origin[0] + ix as f64 * cell, origin[1] + iy as f64 * cell);
            assert_eq!(out[iy * nx + ix].to_bits(), w.ground_at(x, y, INSET_M).to_bits());
        }
    }
}

/// Every jump steeper than 4.5 cm over 1 cm lies under a solid: a curb, a step, a plaza wall or a
/// ramp side. Scanned along lines in x and y over a large area, drawn and physics heights both.
#[test]
#[allow(clippy::many_single_char_names)] // w world, c line, t along it, p point, h and g heights
fn every_vertical_face_has_a_solid() {
    for seed in [2, 8] {
        let w = world(seed);
        let area = Rect::new([-70.0, -70.0], [70.0, 70.0]);
        let solids = w.solids_in(area);
        let mut faces = 0;
        for line in 0..28 {
            let c = -69.0 + f64::from(line) * 4.93;
            for axis in 0..2 {
                let mut previous: Option<([f64; 2], f64, f64)> = None;
                let mut chunk = w.chunk(ChunkKey::of([-70.0, -70.0]));
                for k in 0..=14_000 {
                    let t = -70.0 + f64::from(k) * 0.01;
                    let p = if axis == 0 { [t, c] } else { [c, t] };
                    if chunk.key() != ChunkKey::of(p) {
                        chunk = w.chunk(ChunkKey::of(p));
                    }
                    let (h, g) = (chunk.height_at(p), chunk.ground_at(p, INSET_M));
                    if let Some((q, hq, gq)) = previous {
                        for (jump, label) in [((h - hq).abs(), "drawn"), ((g - gq).abs(), "physics")] {
                            if jump > 0.045 {
                                faces += 1;
                                let mid = [f64::midpoint(p[0], q[0]), f64::midpoint(p[1], q[1])];
                                assert!(
                                    solids.iter().any(|s| s.top_at(mid, 0.02).is_some()),
                                    "seed {seed}: {label} face of {jump:.3} m at {mid:?} has no solid"
                                );
                            }
                        }
                    }
                    previous = Some((p, h, g));
                }
            }
        }
        assert!(faces > 100, "seed {seed}: only {faces} faces crossed; the scan proves little");
    }
}

/// A curb box's face sits on the drawn curb line and its top on the sidewalk's edge. In the
/// physics heights the step lies behind the face, inside the box: road level in front of the
/// face and within the inset, sidewalk level beyond it.
#[test]
fn curb_solids_agree_with_the_sampler() {
    for seed in [4, 6] {
        let w = world(seed);
        let curbs: Vec<_> = w
            .solids_in(Rect::new([-80.0, -80.0], [80.0, 80.0]))
            .into_iter()
            .filter(|s| s.kind == SolidKind::Curb)
            .collect();
        assert!(curbs.len() > 200, "seed {seed}: {} curb boxes", curbs.len());
        for s in &curbs {
            // A point on the box's top plane: `along` its length, `out` metres in front of the face
            // on local y = `face_y` (negative `out` goes into the box).
            let point = |along: f64, face_y: f64, out: f64| {
                let l = [along, face_y + face_y.signum() * out, s.half[2]];
                [0, 1].map(|k| s.centre[k] + (0..3).map(|a| s.axes[a][k] * l[a]).sum::<f64>())
            };
            // The face is the long side with the lower ground in front of it.
            let face_y = [-s.half[1], s.half[1]]
                .into_iter()
                .min_by(|&a, &b| {
                    let h = |y: f64| {
                        let p = point(0.0, y, 0.02);
                        w.height_at(p[0], p[1])
                    };
                    h(a).total_cmp(&h(b))
                })
                .unwrap();
            for along in [-0.4, 0.0, 0.4].map(|f| f * s.half[0]) {
                let edge = point(along, face_y, 0.0);
                let top = s.top_at(edge, 0.001).unwrap();
                let at = |out: f64| {
                    let p = point(along, face_y, out);
                    (w.height_at(p[0], p[1]), w.ground_at(p[0], p[1], INSET_M))
                };
                // Faces are simplified to within 1 cm, so test 2 cm to either side.
                let (road, road_physics) = at(0.02);
                let (walk, _) = at(-0.02);
                let (_, inset_physics) = at(-INSET_M / 2.0);
                let (_, behind_physics) = at(-(INSET_M + 0.03));
                let label = format!("seed {seed}, curb at {edge:?}");
                let face = top - road;
                // Pieces in a curb cut's flare may end on a face lower than FACE_MIN_M, never on none.
                assert!((walk - top).abs() < 0.015, "{label}: sidewalk {walk:.4} vs top {top:.4}");
                assert!(face >= 0.005, "{label}: no face ({face:.4})");
                assert!(face <= CURB_HEIGHT_M[1] + 0.03, "{label}: face {face:.3} too tall");
                assert!(top - road_physics >= face - 0.005, "{label}: physics step in front of the face");
                assert!(top - inset_physics >= face - 0.012, "{label}: physics step not behind the face");
                assert!((behind_physics - top).abs() < 0.015, "{label}: physics sidewalk {behind_physics:.4}");
            }
        }
    }
}

/// Every curb, step, platform and ramp box carries the drawn ground on its top: inside its
/// footprint the ground is its top (a box never stands out of the ground it stands for).
#[test]
fn solid_tops_match_the_drawn_ground() {
    let mut checked = [0usize; 4];
    for seed in [42, 7] {
        let w = world(seed);
        for s in w.solids_in(Rect::new([-128.0, -128.0], [128.0, 128.0])) {
            let slot = match s.kind {
                SolidKind::Curb => 0,
                SolidKind::Step => 1,
                SolidKind::Platform => 2,
                SolidKind::Ramp => 3,
                _ => continue,
            };
            checked[slot] += 1;
            for (fu, fv) in [(0.0, 0.0), (-0.8, -0.6), (0.8, 0.6), (-0.8, 0.6), (0.8, -0.6)] {
                // Points on the top face, 5 cm in from its edges.
                let lu = fu * (s.half[0] - 0.05).max(0.0);
                let lv = fv * (s.half[1] - 0.05).max(0.0);
                let p = [0, 1].map(|k| s.centre[k] + s.axes[0][k] * lu + s.axes[1][k] * lv);
                let (Some(top), h) = (s.top_at(p, 0.0), w.height_at(p[0], p[1])) else { continue };
                assert!((h - top).abs() < 0.02, "seed {seed}: {:?} top {top:.3} over ground {h:.3} at {p:?}", s.kind);
            }
        }
    }
    assert!(checked.iter().all(|&n| n > 0), "solids checked (curb, step, platform, ramp): {checked:?}");
}

#[test]
fn the_spawn_is_level_clear_and_on_a_straight_road() {
    for seed in 0..12 {
        let w = world(seed);
        let spawn = w.spawn();
        assert_eq!(spawn.position[..2], [0.0, 0.0]);
        let s = w.sample(0.0, 0.0);
        assert_eq!((s.zone, s.surface.material), (Zone::Road, Material::Asphalt), "seed {seed}");
        for k in 0..=40 {
            for j in 0..=40 {
                let p = [-SPAWN_LEVEL_RADIUS_M + 0.1 * f64::from(k), -SPAWN_LEVEL_RADIUS_M + 0.1 * f64::from(j)];
                if p[0] * p[0] + p[1] * p[1] <= SPAWN_LEVEL_RADIUS_M * SPAWN_LEVEL_RADIUS_M {
                    let dh = (w.height_at(p[0], p[1]) - spawn.position[2]).abs();
                    assert!(dh <= 0.01, "seed {seed}: {dh:.4} m off level at {p:?}");
                }
            }
        }
        let area = Rect::new([-30.0, -30.0], [30.0, 30.0]);
        for key in keys_in(area) {
            let chunk = w.chunk(key);
            for f in chunk.features() {
                let clearance = match &f.kind {
                    // A crosswalk's bounds cover the whole carriageway; measure its painted band.
                    FeatureKind::Crosswalk { edge, s_m, half_depth_m } => {
                        let edge = w.edge(*edge).unwrap();
                        let k = edge.s.partition_point(|&s| s <= *s_m).clamp(1, edge.s.len() - 1);
                        edge.points[k][0].hypot(edge.points[k][1]) - half_depth_m - (edge.s[k] - edge.s[k - 1])
                    }
                    // A curb cut lowers the curb along its length and flares; its ramp runs away
                    // from the street, so away from the spawn.
                    FeatureKind::CurbCut { centre, half_length_m, flare_m, .. } => {
                        centre[0].hypot(centre[1]) - half_length_m - flare_m
                    }
                    _ => f.bounds.distance([0.0, 0.0]),
                };
                assert!(clearance > SPAWN_CLEAR_M - 0.5, "seed {seed}: {:?} near the spawn", f.kind);
            }
            for p in chunk.props() {
                assert!(p.position[0].hypot(p.position[1]) > SPAWN_CLEAR_M, "seed {seed}: {:?} near the spawn", p.kind);
            }
        }
        for solid in w.solids_in(area) {
            assert!(solid.bounds().distance([0.0, 0.0]) > 4.5, "seed {seed}: {:?} within 4.5 m", solid.kind);
        }
        // A clear, smooth lane ahead to the first crosswalk.
        let mut previous = spawn.position[2];
        for k in 1..=150 {
            let x = f64::from(k) * 0.1;
            let h = w.height_at(x, 0.0);
            assert!((h - previous).abs() < 0.003, "seed {seed}: lane ahead not smooth at x {x}");
            previous = h;
        }
        assert!(w.solids_in(Rect::new([0.0, -1.0], [15.0, 1.0])).is_empty(), "seed {seed}: obstacle ahead");
    }
}

#[test]
fn feature_sizes_stay_within_their_limits() {
    let mut seen = BTreeSet::new();
    let (mut potholes, mut plazas, mut faces) = (0, 0, 0);
    for seed in [1, 2, 3, 4] {
        let w = world(seed);
        let area = Rect::new([-128.0, -128.0], [128.0, 128.0]);
        for key in keys_in(area) {
            let chunk = w.chunk(key);
            for f in chunk.features() {
                if !seen.insert((seed, f.id)) {
                    continue;
                }
                match &f.kind {
                    FeatureKind::Pothole { blob, depth_m, .. } => {
                        potholes += 1;
                        assert!((POTHOLE_DEPTH_M[0]..=POTHOLE_DEPTH_M[1]).contains(depth_m));
                        let width = (f.bounds.max[0] - f.bounds.min[0]).max(f.bounds.max[1] - f.bounds.min[1]);
                        assert!(width <= POTHOLE_WIDTH_M[1] + 1e-9, "pothole {width:.3} m wide");
                        assert!(blob.discs[0][2] * 2.0 >= POTHOLE_WIDTH_M[0] - 1e-9, "pothole too narrow: {blob:?}");
                    }
                    FeatureKind::Manhole { offset_m, .. } => assert!((-0.03..=0.02).contains(offset_m)),
                    FeatureKind::SpeedBump { height_m, .. } => assert!((0.06..=0.09).contains(height_m)),
                    _ => {}
                }
            }
            for p in chunk.parcels() {
                if let ParcelKind::Plaza(z) = p.kind
                    && seen.insert((seed, p.id))
                {
                    plazas += 1;
                    assert!((PLAZA_STEPS[0]..=PLAZA_STEPS[1]).contains(&z.steps));
                    assert!((RISER_M[0]..=RISER_M[1]).contains(&z.riser_m));
                    assert!((TREAD_M[0]..=TREAD_M[1]).contains(&z.tread_m));
                    let slope = (z.top_m - z.ramp_base_m) / (p.width_m - z.ramp_u0_m);
                    assert!(slope > 0.0 && slope <= RAMP_MAX_SLOPE + 1e-9, "ramp at {:.1} %", slope * 100.0);
                }
            }
            for s in chunk.solids() {
                if s.kind == SolidKind::Curb {
                    faces += 1;
                    assert!(s.half[2] * 2.0 <= CURB_HEIGHT_M[1] + 0.15 + 1e-9);
                }
            }
        }
    }
    assert!(potholes > 20 && plazas > 3 && faces > 1000, "{potholes} potholes, {plazas} plazas, {faces} curbs");
}

/// Realised densities follow the parameters: per 1000 m² of road for potholes and manholes.
#[test]
fn densities_follow_their_parameters() {
    let area = Rect::new([-128.0, -128.0], [128.0, 128.0]);
    let count = |spec: WorldSpec| {
        let w = World::new(spec).unwrap();
        let mut seen = BTreeSet::new();
        let (mut potholes, mut manholes) = (0.0, 0.0);
        for key in keys_in(area) {
            for f in w.chunk(key).features() {
                let centre =
                    [f64::midpoint(f.bounds.min[0], f.bounds.max[0]), f64::midpoint(f.bounds.min[1], f.bounds.max[1])];
                if area.contains(centre) && seen.insert(f.id) {
                    match f.kind {
                        FeatureKind::Pothole { .. } => potholes += 1.0,
                        FeatureKind::Manhole { .. } => manholes += 1.0,
                        _ => {}
                    }
                }
            }
        }
        let mut road = 0.0;
        for j in 0..256 {
            for i in 0..256 {
                let p = [area.min[0] + f64::from(i) + 0.5, area.min[1] + f64::from(j) + 0.5];
                if w.sample(p[0], p[1]).zone == Zone::Road {
                    road += 1.0;
                }
            }
        }
        (potholes / road * 1000.0, manholes / road * 1000.0)
    };
    let mut total = (0.0, 0.0);
    let mut single = 0.0;
    for seed in [21, 22, 23] {
        let (p, m) = count(WorldSpec::city(seed));
        total = (total.0 + p / 3.0, total.1 + m / 3.0);
        if seed == 21 {
            single = p;
        }
    }
    let d = Densities::default();
    // Sites near the gutter, the spawn and thinned neighbours are refused: 50–110 % of nominal.
    assert!((0.5 * d.potholes..=1.1 * d.potholes).contains(&total.0), "potholes {:.2} per 1000 m²", total.0);
    assert!((0.4 * d.manholes..=1.3 * d.manholes).contains(&total.1), "manholes {:.2} per 1000 m²", total.1);
    let mut none = WorldSpec::city(21);
    none.densities.potholes = 0.0;
    assert_eq!(count(none).0, 0.0);
    let mut double = WorldSpec::city(21);
    double.densities.potholes = 2.0 * d.potholes;
    let doubled = count(double).0;
    assert!(doubled > 1.5 * single && doubled < 2.3 * single, "{single:.2} → {doubled:.2}");
}

#[test]
fn the_road_network_is_connected() {
    let w = world(17);
    let range = -8..=8;
    let mut reached = BTreeSet::from([(0i64, 0i64)]);
    let mut queue = VecDeque::from([(0i64, 0i64)]);
    while let Some((i, j)) = queue.pop_front() {
        let links = [
            (EdgeId { axis: Axis::Row, i, j }, (i + 1, j)),
            (EdgeId { axis: Axis::Row, i: i - 1, j }, (i - 1, j)),
            (EdgeId { axis: Axis::Column, i, j }, (i, j + 1)),
            (EdgeId { axis: Axis::Column, i, j: j - 1 }, (i, j - 1)),
        ];
        for (edge, next) in links {
            if range.contains(&next.0) && range.contains(&next.1) && w.edge(edge).is_some() && reached.insert(next) {
                queue.push_back(next);
            }
        }
    }
    assert_eq!(reached.len(), 17 * 17, "unreachable nodes in a 17 × 17 lattice");
}

#[test]
fn hills_stay_gentle() {
    for seed in [1, 2, 3, 4, 5] {
        let w = world(seed);
        let mut steepest: f64 = 0.0;
        for j in -100..100 {
            for i in -100..100 {
                let (x, y) = (f64::from(i) * 8.0, f64::from(j) * 8.0);
                let h = w.terrain_at(x, y);
                let gx = (w.terrain_at(x + 1.0, y) - h).abs();
                let gy = (w.terrain_at(x, y + 1.0) - h).abs();
                steepest = steepest.max(gx.hypot(gy));
            }
        }
        assert!(steepest <= 0.12, "seed {seed}: terrain grade {:.1} %", steepest * 100.0);
    }
}

/// Generator regressions: a fixed seed's chunks around the spawn hash to a recorded value.
/// A level city for tests that need one: no hills, no road defects.
fn level(seed: u64) -> WorldSpec {
    let densities = Densities {
        hills: 0.0,
        potholes: 0.0,
        manholes: 0.0,
        cracks: 0.0,
        rough_patches: 0.0,
        wet_patches: 0.0,
        ..Densities::default()
    };
    WorldSpec { densities, ..WorldSpec::city(seed) }
}

#[test]
fn hills_zero_levels_the_terrain_and_keeps_the_streets() {
    let flat = World::new(level(42)).unwrap();
    let hilly = world(42);
    for x in (-300..=300).step_by(25) {
        for y in (-300..=300).step_by(25) {
            assert_eq!(flat.terrain_at(f64::from(x), f64::from(y)), 0.0, "({x}, {y})");
        }
    }
    // The road network does not depend on the relief: the same streets, the same curb lines.
    for (i, j) in [(0, 0), (3, -2), (-4, 5)] {
        for axis in [Axis::Row, Axis::Column] {
            let id = EdgeId { axis, i, j };
            assert_eq!(flat.edge(id).map(|e| e.points), hilly.edge(id).map(|e| e.points), "{id:?}");
        }
    }
    // Along the spawn arterial the lane is level to millimetres (its crown is 7 cm above the gutter).
    let start = flat.height_at(0.0, 0.0);
    for x in (0..=450).step_by(5) {
        let h = flat.height_at(f64::from(x), 0.0);
        assert!((h - start).abs() < 0.005, "x {x}: {h} against {start}");
    }
}

#[test]
fn an_incline_grades_the_whole_world_along_its_direction() {
    let incline =
        euc_world::Incline { origin: [0.0, 0.0], direction: [1.0, 0.0], slope_pct: 20.0, start_m: 40.0, ramp_m: 10.0 };
    let graded = World::new(WorldSpec { incline: Some(incline), ..level(42) }).unwrap();
    let flat = World::new(level(42)).unwrap();
    for (x, y) in [(-50.0, 0.0), (0.0, 20.0), (39.9, -60.0)] {
        assert_eq!(graded.height_at(x, y), flat.height_at(x, y), "behind the start ({x}, {y})");
    }
    // Past the ramp the lane climbs 20 cm per metre, the same at any offset across the grade.
    for y in [0.0, -64.0, 128.0] {
        let rise = graded.terrain_at(110.0, y) - graded.terrain_at(100.0, y);
        assert!((rise - 2.0).abs() < 1e-9, "y {y}: {rise}");
    }
    let lane = graded.height_at(150.0, 0.0) - graded.height_at(100.0, 0.0);
    assert!((lane - 10.0).abs() < 0.01, "{lane}");
    // A curb follows the grade: its top stays on the sidewalk's edge.
    let solids = graded.solids_in(Rect::new([140.0, -10.0], [160.0, 10.0]));
    let curb = solids.iter().find(|s| s.kind == SolidKind::Curb).expect("a curb along the arterial");
    let c = curb.centre;
    assert!((curb.top() - graded.height_at(c[0], c[1])).abs() < 0.3, "{curb:?}");
    // Buildings stand in the slope: no footprint corner sees the box's bottom above the ground.
    let mut buildings = 0;
    for s in graded.solids_in(Rect::new([60.0, -150.0], [300.0, 150.0])) {
        if s.kind != SolidKind::Building {
            continue;
        }
        buildings += 1;
        let bottom = s.centre[2] - s.half[2];
        for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let x = s.centre[0] + s.axes[0][0] * s.half[0] * a + s.axes[1][0] * s.half[1] * b;
            let y = s.centre[1] + s.axes[0][1] * s.half[0] * a + s.axes[1][1] * s.half[1] * b;
            let ground = graded.height_at(x, y);
            assert!(bottom <= ground + 0.02, "building {} floats {:.3} m at ({x:.1}, {y:.1})", s.id, bottom - ground);
        }
    }
    assert!(buildings > 10, "{buildings}");
    for bad in [
        euc_world::Incline { slope_pct: 45.0, ..incline },
        euc_world::Incline { direction: [1.0, 1.0], ..incline },
        euc_world::Incline { start_m: -1.0, ..incline },
    ] {
        assert!(World::new(WorldSpec { incline: Some(bad), ..level(42) }).is_err(), "{bad:?}");
    }
}

/// The arithmetic is IEEE-754 only, so this value holds on every platform.
#[test]
fn golden_digest() {
    let w = world(20_261_022);
    let mut all = 0xcbf2_9ce4_8422_2325u64;
    for (_, d) in digests(&w, Rect::new([-80.0, -80.0], [79.0, 79.0])) {
        all = (all ^ d).wrapping_mul(0x0000_0100_0000_01b3);
    }
    assert_eq!(all, GOLDEN, "generator output changed: bump GENERATOR_REVISION and update GOLDEN ({all:#018x})");
}

const GOLDEN: u64 = 0x534d_b713_2964_6e17;
