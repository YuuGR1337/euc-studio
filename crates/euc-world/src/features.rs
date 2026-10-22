//! Features on the ground and props beside it.
//!
//! Potholes, manholes, cracks and rough and wet patches sit on global jittered grids: each grid
//! cell holds at most one candidate, a pure function of the seed and the cell. A candidate is
//! kept when its site is valid and no overlapping neighbour has a higher priority; neighbours
//! are judged by the same pure rules, so the result never depends on generation order.
//!
//! Speed bumps, crosswalks, curb cuts (crossings and driveways), tree pits, lamp posts and
//! street furniture are placed along each street edge at fixed arc-length slots.

// Geometry reads like its formulas: p, a, b, r, s (a random stream), h (a hash).
#![allow(clippy::many_single_char_names)]

use serde::{Deserialize, Serialize};

use crate::Densities;
use crate::det::{self, Rect, Stream, V2, hash};
use crate::fields;
use crate::roads::{self, Axis, Edge, EdgeId, RoadHit};

/// No feature or prop comes closer to the spawn point than this, metres.
pub const SPAWN_CLEAR_M: f64 = 10.0;
/// Pothole depth range, metres.
pub const POTHOLE_DEPTH_M: [f64; 2] = [0.02, 0.12];
/// Pothole width range (outer extent), metres.
pub const POTHOLE_WIDTH_M: [f64; 2] = [0.2, 1.5];

const POTHOLE_CELL_M: f64 = 4.0;
const MANHOLE_CELL_M: f64 = 16.0;
const CRACK_CELL_M: f64 = 3.0;
const ROUGH_CELL_M: f64 = 12.0;
const WET_CELL_M: f64 = 10.0;
/// smin can grow a blob by a quarter of its blend; discs stay within R, the blend is 0.3 R.
const BLOB_GROWTH: f64 = 1.0 + 0.3 / 4.0;

const SALT_POTHOLE: u64 = 0x301;
const SALT_MANHOLE: u64 = 0x302;
const SALT_CRACK: u64 = 0x303;
const SALT_ROUGH: u64 = 0x304;
const SALT_WET: u64 = 0x305;
const SALT_BUMP: u64 = 0x306;
const SALT_CROSSWALK: u64 = 0x307;
const SALT_DRIVEWAY: u64 = 0x308;
const SALT_TREE: u64 = 0x309;
const SALT_LAMP: u64 = 0x30a;
const SALT_FURNITURE: u64 = 0x30b;
const SALT_BOTTOM: u64 = 0x30c;

/// A smooth union of discs `[x, y, radius]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Blob {
    /// Discs, in union order.
    pub discs: Vec<[f64; 3]>,
    /// smin blend radius, metres.
    pub blend_m: f64,
}

impl Blob {
    /// Signed distance-like value: negative inside.
    pub fn distance(&self, p: V2) -> f64 {
        let mut g = roads::FAR_M;
        for (k, d) in self.discs.iter().enumerate() {
            let v = det::len(det::sub(p, [d[0], d[1]])) - d[2];
            g = if k == 0 { v } else { det::smin(g, v, self.blend_m) };
        }
        g
    }

    fn bounds(&self) -> Rect {
        let mut r = Rect::empty();
        for d in &self.discs {
            r.include([d[0] - d[2], d[1] - d[2]]);
            r.include([d[0] + d[2], d[1] + d[2]]);
        }
        r.grow(self.blend_m * 0.25)
    }

    /// A blob of 2 to 4 discs that stays within radius `r` of `centre` (plus the smin growth).
    fn random(s: &mut Stream, centre: V2, r: f64) -> Self {
        let mut discs = vec![[centre[0], centre[1], r * s.range(0.75, 1.0)]];
        for _ in 0..s.int(1, 3) {
            let offset = [s.signed() * 0.4 * r, s.signed() * 0.4 * r];
            let room = r - det::len(offset);
            discs.push([centre[0] + offset[0], centre[1] + offset[1], room * s.range(0.6, 1.0)]);
        }
        Self { discs, blend_m: 0.3 * r }
    }
}

/// A speed bump's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BumpShape {
    /// A short, sharp bump (0.4–0.6 m long).
    Bump,
    /// A long hump (3–4 m long).
    Hump,
}

/// What a feature is, with its geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum FeatureKind {
    /// An irregular hole with steep walls and a rough gravel bottom.
    Pothole {
        /// Outline.
        blob: Blob,
        /// Depth below the surrounding road, metres.
        depth_m: f64,
        /// Width of the wall, metres.
        rim_m: f64,
        /// Holds water.
        puddle: bool,
    },
    /// A round cast-iron cover, slightly sunk or raised.
    Manhole {
        /// Centre.
        centre: V2,
        /// Cover radius, metres.
        radius_m: f64,
        /// Height offset of the cover, metres (negative = sunk).
        offset_m: f64,
    },
    /// A narrow V-shaped crack along a polyline.
    Crack {
        /// Polyline.
        points: Vec<V2>,
        /// Width at the surface, metres.
        width_m: f64,
        /// Depth, metres.
        depth_m: f64,
    },
    /// Worn, uneven asphalt.
    RoughPatch {
        /// Outline.
        blob: Blob,
        /// Roughness amplitude, metres.
        amplitude_m: f64,
    },
    /// Standing water or a wet stain.
    WetPatch {
        /// Outline.
        blob: Blob,
    },
    /// A speed bump or hump across a street.
    SpeedBump {
        /// The street edge.
        edge: EdgeId,
        /// Arc length of its crest, metres.
        s_m: f64,
        /// Length along the street, metres.
        length_m: f64,
        /// Crest height, metres.
        height_m: f64,
        /// Bump or hump.
        shape: BumpShape,
    },
    /// A zebra crossing.
    Crosswalk {
        /// The street edge.
        edge: EdgeId,
        /// Arc length of its centre line, metres.
        s_m: f64,
        /// Half its depth along the street, metres.
        half_depth_m: f64,
    },
    /// A lowered curb with a ramp across the sidewalk: a crossing or a driveway.
    CurbCut {
        /// Middle of the lowered curb, on the curb face line.
        centre: V2,
        /// Unit vector along the curb.
        tangent: V2,
        /// Half the length of the fully lowered part, metres.
        half_length_m: f64,
        /// Length of each flared side, metres.
        flare_m: f64,
        /// Depth of the ramp into the sidewalk, metres.
        run_m: f64,
        /// A driveway (concrete apron) rather than a crossing.
        driveway: bool,
    },
    /// A square tree pit in the sidewalk, with a tree.
    TreePit {
        /// Centre.
        centre: V2,
        /// Unit vector along the curb.
        tangent: V2,
        /// Half the side, metres.
        half_m: f64,
        /// Depth, metres.
        depth_m: f64,
    },
}

/// A feature with its identity and bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    /// Stable identity.
    pub id: u64,
    /// Kind and geometry.
    pub kind: FeatureKind,
    /// Bounds of everything it changes.
    pub bounds: Rect,
}

impl Feature {
    /// Canonical evaluation order: kind, then identity.
    pub(crate) fn order_key(&self) -> (u8, u64) {
        let rank = match self.kind {
            FeatureKind::Pothole { .. } => 0,
            FeatureKind::Manhole { .. } => 1,
            FeatureKind::Crack { .. } => 2,
            FeatureKind::RoughPatch { .. } => 3,
            FeatureKind::WetPatch { .. } => 4,
            FeatureKind::SpeedBump { .. } => 5,
            FeatureKind::Crosswalk { .. } => 6,
            FeatureKind::CurbCut { .. } => 7,
            FeatureKind::TreePit { .. } => 8,
        };
        (rank, self.id)
    }

    /// Height added to the ground at `p`, metres, or `None` outside its support.
    /// `edges`, `hit` describe the streets at `p` (speed bumps follow their edge).
    pub(crate) fn height(&self, p: V2, edges: &[Edge], hit: &RoadHit) -> Option<f64> {
        match &self.kind {
            FeatureKind::Pothole { blob, depth_m, rim_m, .. } => {
                let g = blob.distance(p);
                (g < 0.0).then(|| {
                    let bottom = 0.5 + 0.5 * det::value_noise(self.id, SALT_BOTTOM, p, 0.08);
                    -depth_m * det::smoothstep(0.0, *rim_m, -g) * (1.0 - 0.15 * bottom)
                })
            }
            FeatureKind::Manhole { centre, radius_m, offset_m } => {
                let d = det::len(det::sub(p, *centre));
                (d < *radius_m).then(|| offset_m * det::smoothstep(*radius_m, radius_m - 0.02, d))
            }
            FeatureKind::Crack { points, width_m, depth_m } => {
                let half = width_m / 2.0;
                let d = polyline_distance(points, p);
                (d < half).then(|| -depth_m * (1.0 - d / half))
            }
            FeatureKind::RoughPatch { blob, amplitude_m } => {
                let g = blob.distance(p);
                (g < 0.0).then(|| amplitude_m * det::value_noise(self.id, 1, p, 0.15) * det::smoothstep(0.0, 0.3, -g))
            }
            FeatureKind::SpeedBump { edge, s_m, length_m, height_m, .. } => {
                let near = hit.near?;
                if hit.f >= 0.0 || edges[near.edge].id != *edge || (near.s - s_m).abs() >= length_m / 2.0 {
                    return None;
                }
                let u = 2.0 * (near.s - s_m) / length_m;
                let w = 1.0 - u * u;
                Some(height_m * w * w * det::smoothstep(0.15, 0.5, -hit.f))
            }
            FeatureKind::TreePit { centre, tangent, half_m, depth_m } => {
                let d = det::sub(p, *centre);
                let a = det::dot(d, *tangent).abs().max(det::cross(*tangent, d).abs());
                (a < *half_m).then(|| -depth_m * det::smoothstep(*half_m, half_m - 0.02, a))
            }
            FeatureKind::WetPatch { .. } | FeatureKind::Crosswalk { .. } | FeatureKind::CurbCut { .. } => None,
        }
    }

    /// How far a curb cut lowers the curb at `p` with road field `f` (0 = not at all, 1 = fully).
    pub(crate) fn cut(&self, p: V2, f: f64) -> f64 {
        let FeatureKind::CurbCut { centre, tangent, half_length_m, flare_m, run_m, .. } = self.kind else {
            return 0.0;
        };
        let d = det::sub(p, centre);
        let along = det::dot(d, tangent).abs();
        let across = det::cross(tangent, d).abs();
        let lengthwise = det::smoothstep(half_length_m + flare_m, half_length_m, along);
        let depthwise = 1.0 - det::smoothstep(0.0, run_m, f.max(0.0));
        let lateral = det::smoothstep(run_m + 1.0, run_m + 0.5, across);
        lengthwise * depthwise * lateral
    }
}

/// Distance from `p` to a polyline.
pub(crate) fn polyline_distance(points: &[V2], p: V2) -> f64 {
    points.windows(2).map(|w| det::segment_distance(p, w[0], w[1]).0).fold(f64::INFINITY, f64::min)
}

/// A kind of street furniture or roadside object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropKind {
    /// A street light pole.
    LampPost,
    /// A street or park tree (trunk solid, canopy drawn only).
    Tree,
    /// A park bench.
    Bench,
    /// A litter bin.
    Bin,
    /// A fire hydrant.
    Hydrant,
    /// A short post.
    Bollard,
}

impl PropKind {
    /// Collision footprint half sizes along its facing and across it, and its height, metres.
    pub const fn size(self) -> [f64; 3] {
        match self {
            Self::LampPost => [0.08, 0.08, 7.0],
            Self::Tree => [0.15, 0.15, 3.0],
            Self::Bench => [0.9, 0.3, 0.45],
            Self::Bin => [0.25, 0.25, 0.9],
            Self::Hydrant => [0.15, 0.15, 0.7],
            Self::Bollard => [0.1, 0.1, 0.9],
        }
    }
}

/// A prop standing on the ground.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Prop {
    /// Stable identity.
    pub id: u64,
    /// What it is.
    pub kind: PropKind,
    /// Base position; z is the ground height there, metres.
    pub position: [f64; 3],
    /// Unit vector along the prop's long side (along the curb for street furniture).
    pub facing: V2,
}

/// Placement decisions read the road field up to this distance from a street, metres.
pub(crate) const CTX_REACH_M: f64 = 24.0;
const CTX_BIN_M: f64 = 32.0;

/// Placement context: the seed, densities and every street within [`CTX_REACH_M`] of a region,
/// binned. Its field is exact wherever a decision reads it, so every chunk that places the same
/// item decides the same way.
pub(crate) struct Ctx<'a> {
    pub seed: u64,
    pub densities: &'a Densities,
    rect: Rect,
    nx: usize,
    ny: usize,
    edges: &'a [Edge],
    bins: Vec<Vec<u16>>,
}

impl<'a> Ctx<'a> {
    /// `edges` must hold every street within [`CTX_REACH_M`] of `rect`, in [`EdgeId`] order.
    pub fn new(seed: u64, densities: &'a Densities, edges: &'a [Edge], rect: Rect) -> Self {
        let nx = ((rect.max[0] - rect.min[0]) / CTX_BIN_M).ceil().max(1.0) as usize;
        let ny = ((rect.max[1] - rect.min[1]) / CTX_BIN_M).ceil().max(1.0) as usize;
        let mut bins = Vec::with_capacity(nx * ny);
        for by in 0..ny {
            for bx in 0..nx {
                let x = rect.min[0] + bx as f64 * CTX_BIN_M;
                let y = rect.min[1] + by as f64 * CTX_BIN_M;
                let bin = Rect::new([x, y], [x + CTX_BIN_M, y + CTX_BIN_M]);
                bins.push(
                    (0..edges.len())
                        .filter(|&k| bin.gap(&edges[k].bounds) - edges[k].half_width_m <= CTX_REACH_M)
                        .map(|k| k as u16)
                        .collect(),
                );
            }
        }
        Self { seed, densities, rect, nx, ny, edges, bins }
    }

    /// The road field at `p`, which must lie inside the context's rectangle.
    pub fn field(&self, p: V2) -> RoadHit {
        debug_assert!(self.rect.contains(p), "placement query {p:?} outside its context");
        let bx = (((p[0] - self.rect.min[0]) / CTX_BIN_M).floor().max(0.0) as usize).min(self.nx - 1);
        let by = (((p[1] - self.rect.min[1]) / CTX_BIN_M).floor().max(0.0) as usize).min(self.ny - 1);
        roads::field(self.edges, &self.bins[by * self.nx + bx], p)
    }
}

struct Candidate {
    feature: Feature,
    anchor: V2,
    radius: f64,
    priority: u64,
}

struct GridClass {
    cell: f64,
    /// Largest distance from the anchor to anything the feature changes, metres.
    reach: f64,
    /// Clearance between kept neighbours, metres; `None` lets them overlap.
    clearance: Option<f64>,
    make: fn(&Ctx, i64, i64) -> Option<Candidate>,
}

fn anchor(s: &mut Stream, cell: f64, a: i64, b: i64) -> V2 {
    [(a as f64 + s.unit()) * cell, (b as f64 + s.unit()) * cell]
}

fn clear_of_spawn(p: V2, radius: f64) -> bool {
    det::len(p) > SPAWN_CLEAR_M + radius
}

fn pothole(ctx: &Ctx, a: i64, b: i64) -> Option<Candidate> {
    let h = hash(ctx.seed, SALT_POTHOLE, a, b);
    let mut s = Stream::new(h);
    if !s.chance(ctx.densities.potholes * POTHOLE_CELL_M * POTHOLE_CELL_M / 1000.0) {
        return None;
    }
    let p = anchor(&mut s, POTHOLE_CELL_M, a, b);
    // Width skews small: u^1.5 over the range. Extent ≤ 2 R × BLOB_GROWTH.
    let u = s.unit();
    let r = det::lerp(POTHOLE_WIDTH_M[0] / 1.5, POTHOLE_WIDTH_M[1] / (2.0 * BLOB_GROWTH), u * u.sqrt());
    let depth_m = s.range(POTHOLE_DEPTH_M[0], POTHOLE_DEPTH_M[1]);
    let blob = Blob::random(&mut s, p, r);
    let puddle = s.chance(0.3);
    if !clear_of_spawn(p, r) || ctx.field(p).f > -(r * BLOB_GROWTH + 0.25) {
        return None;
    }
    let bounds = blob.bounds();
    let rim_m = (0.3 * r).min(0.04);
    let kind = FeatureKind::Pothole { blob, depth_m, rim_m, puddle };
    Some(Candidate { feature: Feature { id: h, kind, bounds }, anchor: p, radius: r * BLOB_GROWTH, priority: h })
}

fn manhole(ctx: &Ctx, a: i64, b: i64) -> Option<Candidate> {
    let h = hash(ctx.seed, SALT_MANHOLE, a, b);
    let mut s = Stream::new(h);
    if !s.chance(ctx.densities.manholes * MANHOLE_CELL_M * MANHOLE_CELL_M / 1000.0) {
        return None;
    }
    let centre = anchor(&mut s, MANHOLE_CELL_M, a, b);
    let radius_m = s.range(0.30, 0.38);
    let offset_m = if s.chance(0.7) { -s.range(0.01, 0.03) } else { s.range(0.008, 0.02) };
    if !clear_of_spawn(centre, radius_m) || ctx.field(centre).f > -(radius_m + 0.6) {
        return None;
    }
    let bounds = Rect::new(centre, centre).grow(radius_m + 0.1);
    let kind = FeatureKind::Manhole { centre, radius_m, offset_m };
    Some(Candidate { feature: Feature { id: h, kind, bounds }, anchor: centre, radius: radius_m + 0.1, priority: h })
}

fn crack(ctx: &Ctx, a: i64, b: i64) -> Option<Candidate> {
    let h = hash(ctx.seed, SALT_CRACK, a, b);
    let mut s = Stream::new(h);
    if !s.chance(ctx.densities.cracks * CRACK_CELL_M * CRACK_CELL_M / 1000.0) {
        return None;
    }
    let start = anchor(&mut s, CRACK_CELL_M, a, b);
    let mut dir = det::norm_or([s.signed(), s.signed()], [1.0, 0.0]);
    let mut points = vec![start];
    let mut bounds = Rect::new(start, start);
    for _ in 0..s.int(3, 6) {
        dir = det::norm_or(det::add(dir, det::scale(det::perp(dir), s.signed() * 0.5)), dir);
        let next = det::add(*points.last().unwrap_or(&start), det::scale(dir, s.range(0.3, 0.8)));
        bounds.include(next);
        points.push(next);
    }
    let width_m = s.range(0.008, 0.02);
    let depth_m = s.range(0.005, 0.015);
    let f = ctx.field(start).f;
    let sidewalk = fields::sidewalk_width(ctx.seed, start);
    let paved = f < -0.3 || (f > 0.3 && f < sidewalk - 0.3);
    if !paved || !clear_of_spawn(start, 5.0) {
        return None;
    }
    let kind = FeatureKind::Crack { points, width_m, depth_m };
    Some(Candidate {
        feature: Feature { id: h, kind, bounds: bounds.grow(width_m) },
        anchor: start,
        radius: 5.0,
        priority: h,
    })
}

fn rough(ctx: &Ctx, a: i64, b: i64) -> Option<Candidate> {
    let h = hash(ctx.seed, SALT_ROUGH, a, b);
    let mut s = Stream::new(h);
    if !s.chance(ctx.densities.rough_patches * ROUGH_CELL_M * ROUGH_CELL_M / 1000.0) {
        return None;
    }
    let p = anchor(&mut s, ROUGH_CELL_M, a, b);
    let r = s.range(1.0, 3.0);
    let amplitude_m = s.range(0.003, 0.008);
    let blob = Blob::random(&mut s, p, r);
    if !clear_of_spawn(p, r * BLOB_GROWTH) || ctx.field(p).f > -(r * 0.5) {
        return None;
    }
    let bounds = blob.bounds();
    let kind = FeatureKind::RoughPatch { blob, amplitude_m };
    Some(Candidate { feature: Feature { id: h, kind, bounds }, anchor: p, radius: r * BLOB_GROWTH, priority: h })
}

fn wet(ctx: &Ctx, a: i64, b: i64) -> Option<Candidate> {
    let h = hash(ctx.seed, SALT_WET, a, b);
    let mut s = Stream::new(h);
    if !s.chance(ctx.densities.wet_patches * WET_CELL_M * WET_CELL_M / 1000.0) {
        return None;
    }
    let p = anchor(&mut s, WET_CELL_M, a, b);
    let r = s.range(0.5, 2.5);
    let blob = Blob::random(&mut s, p, r);
    if !clear_of_spawn(p, r * BLOB_GROWTH) || ctx.field(p).f > fields::sidewalk_width(ctx.seed, p) - 0.5 {
        return None;
    }
    let bounds = blob.bounds();
    Some(Candidate {
        feature: Feature { id: h, kind: FeatureKind::WetPatch { blob }, bounds },
        anchor: p,
        radius: r * BLOB_GROWTH,
        priority: h,
    })
}

const GRID_CLASSES: [GridClass; 5] = [
    GridClass { cell: POTHOLE_CELL_M, reach: POTHOLE_WIDTH_M[1] / 2.0, clearance: Some(0.4), make: pothole },
    GridClass { cell: MANHOLE_CELL_M, reach: 0.5, clearance: Some(3.0), make: manhole },
    GridClass { cell: CRACK_CELL_M, reach: 5.0, clearance: None, make: crack },
    GridClass { cell: ROUGH_CELL_M, reach: 3.0 * BLOB_GROWTH, clearance: None, make: rough },
    GridClass { cell: WET_CELL_M, reach: 2.5 * BLOB_GROWTH, clearance: None, make: wet },
];

/// The most a grid feature reaches from its cell, metres: how far beyond a region to look.
pub(crate) const GRID_REACH_M: f64 = 5.0 + 2.0 * MANHOLE_CELL_M;

/// Grid features whose bounds overlap `rect`.
pub(crate) fn grid_features(ctx: &Ctx, rect: Rect, out: &mut Vec<Feature>) {
    for class in &GRID_CLASSES {
        let grown = rect.grow(class.reach);
        let lo = |v: f64| (v / class.cell).floor() as i64;
        for a in lo(grown.min[0])..=lo(grown.max[0]) {
            for b in lo(grown.min[1])..=lo(grown.max[1]) {
                let Some(c) = (class.make)(ctx, a, b) else { continue };
                if !c.feature.bounds.overlaps(&rect) {
                    continue;
                }
                if let Some(clearance) = class.clearance {
                    let k = ((2.0 * class.reach + clearance) / class.cell).ceil() as i64;
                    let beaten = (-k..=k).any(|da| {
                        (-k..=k).any(|db| {
                            (da, db) != (0, 0)
                                && (class.make)(ctx, a + da, b + db).is_some_and(|n| {
                                    n.priority > c.priority
                                        && det::len(det::sub(n.anchor, c.anchor)) < n.radius + c.radius + clearance
                                })
                        })
                    });
                    if beaten {
                        continue;
                    }
                }
                out.push(c.feature);
            }
        }
    }
}

/// Point and unit tangent at arc length `s` along an edge.
pub(crate) fn at(edge: &Edge, s: f64) -> (V2, V2) {
    let last = edge.points.len() - 1;
    let s = s.clamp(0.0, edge.s[last]);
    let k = edge.s.partition_point(|&x| x <= s).saturating_sub(1).min(last - 1);
    let (a, b) = (edge.points[k], edge.points[k + 1]);
    let length = edge.s[k + 1] - edge.s[k];
    let t = if length > 0.0 { (s - edge.s[k]) / length } else { 0.0 };
    (det::add(a, det::scale(det::sub(b, a), t)), det::norm_or(det::sub(b, a), [1.0, 0.0]))
}

/// Bounds of an edge's centreline between two arc lengths, grown by `margin`.
fn span_bounds(edge: &Edge, s0: f64, s1: f64, margin: f64) -> Rect {
    let mut r = Rect::empty();
    r.include(at(edge, s0).0);
    r.include(at(edge, s1).0);
    for (k, &p) in edge.points.iter().enumerate() {
        if edge.s[k] > s0 && edge.s[k] < s1 {
            r.include(p);
        }
    }
    r.grow(margin)
}

fn edge_key(id: EdgeId) -> (i64, i64) {
    (id.i * 2 + id.axis as i64, id.j)
}

/// Half width of the street crossing `edge` at its start (`end == false`) or end node.
fn crossing_half_width(seed: u64, id: EdgeId, end: bool) -> f64 {
    let (i, j) = if end { id.end() } else { id.start() };
    let crossing = match id.axis {
        Axis::Row => EdgeId { axis: Axis::Column, i, j },
        Axis::Column => EdgeId { axis: Axis::Row, i, j },
    };
    roads::half_width(seed, crossing)
}

/// Arc length of the crosswalk at one end of an edge, measured from that end, metres.
fn crosswalk_offset(seed: u64, id: EdgeId, end: bool) -> f64 {
    crossing_half_width(seed, id, end) + roads::CORNER_BLEND_M + 2.5
}

/// Everything one street edge places: features and props. Deterministic per edge.
#[allow(clippy::too_many_lines)] // one walk along an edge: crossings, a bump, then each side's slots
pub(crate) fn street_items(ctx: &Ctx, edge: &Edge, features: &mut Vec<Feature>, props: &mut Vec<Prop>) {
    let id = edge.id;
    let (ka, kb) = edge_key(id);
    let length = edge.s[edge.s.len() - 1];
    let hw = edge.half_width_m;

    // Crosswalks at intersection ends, each with a curb cut on both sides.
    let mut crossings = Vec::new();
    for end in [false, true] {
        let (i, j) = if end { id.end() } else { id.start() };
        let offset = crosswalk_offset(ctx.seed, id, end);
        let mut s = Stream::new(hash(ctx.seed, SALT_CROSSWALK, ka, kb * 2 + i64::from(end)));
        if roads::degree(ctx.seed, i, j) < 3 || length < 2.0 * offset + 6.0 || !s.chance(ctx.densities.crosswalks) {
            continue;
        }
        let s_m = if end { length - offset } else { offset };
        let (centre, tangent) = at(edge, s_m);
        let base = s.bits();
        features.push(Feature {
            id: base,
            kind: FeatureKind::Crosswalk { edge: id, s_m, half_depth_m: 1.5 },
            bounds: span_bounds(edge, s_m - 1.5, s_m + 1.5, hw + 0.5),
        });
        for (k, side) in [1.0, -1.0].into_iter().enumerate() {
            let curb = det::add(centre, det::scale(det::perp(tangent), side * hw));
            features.push(curb_cut(base ^ (k as u64 + 1), curb, tangent, 1.6, 1.0, 1.2, false));
        }
        crossings.push(s_m);
    }
    let clear_ends = |s: f64| {
        s > crosswalk_offset(ctx.seed, id, false) + 4.0 && s < length - crosswalk_offset(ctx.seed, id, true) - 4.0
    };

    // A speed bump or hump on some local streets, near mid-block.
    let mut s = Stream::new(hash(ctx.seed, SALT_BUMP, ka, kb));
    if id.arterial() || length < 40.0 || !s.chance(ctx.densities.speed_bumps) {
        // none
    } else {
        let s_m = length / 2.0 + s.signed() * length / 8.0;
        let (shape, length_m, height_m) = if s.chance(0.5) {
            (BumpShape::Bump, s.range(0.4, 0.6), s.range(0.06, 0.09))
        } else {
            (BumpShape::Hump, s.range(3.0, 4.0), s.range(0.06, 0.08))
        };
        if clear_of_spawn(at(edge, s_m).0, hw + length_m) {
            features.push(Feature {
                id: s.bits(),
                kind: FeatureKind::SpeedBump { edge: id, s_m, length_m, height_m, shape },
                bounds: span_bounds(edge, s_m - length_m / 2.0, s_m + length_m / 2.0, hw),
            });
        }
    }

    for side in [1.0, -1.0] {
        let side_key = kb * 4 + if side > 0.0 { 1 } else { 2 };
        let point = |s: f64, offset: f64| {
            let (c, t) = at(edge, s);
            (det::add(c, det::scale(det::perp(t), side * offset)), t)
        };
        // Driveways every 18 m slot, away from intersections.
        let mut driveways = Vec::new();
        let mut slot = 0;
        while 9.0 + 18.0 * f64::from(slot) <= length - 9.0 {
            let s_m = 9.0 + 18.0 * f64::from(slot);
            let mut s = Stream::new(hash(ctx.seed, SALT_DRIVEWAY, ka, side_key * 1024 + i64::from(slot)));
            slot += 1;
            if !clear_ends(s_m) || !s.chance(ctx.densities.driveways * 18.0 / 100.0) {
                continue;
            }
            let width = s.range(3.0, 4.5);
            let (curb, tangent) = point(s_m, hw);
            if clear_of_spawn(curb, width) {
                features.push(curb_cut(s.bits(), curb, tangent, width / 2.0, 0.75, 1.0, true));
                driveways.push((s_m, width / 2.0 + 0.75));
            }
        }
        let free = |s_m: f64, room: f64| {
            clear_ends(s_m)
                && driveways.iter().all(|&(d, half)| (s_m - d).abs() > half + room)
                && crossings.iter().all(|&c| (s_m - c).abs() > 3.0 + room)
        };
        let sidewalk_spot = |s_m: f64, f_target: f64| -> Option<(V2, V2)> {
            let (p, t) = point(s_m, hw + f_target);
            let f = ctx.field(p).f;
            ((f - f_target).abs() < 0.2 && clear_of_spawn(p, 1.0)).then_some((p, t))
        };
        // Tree pits every 8 m.
        let mut slot = 0;
        while 4.0 + 8.0 * f64::from(slot) <= length - 4.0 {
            let s_m = 4.0 + 8.0 * f64::from(slot);
            let mut s = Stream::new(hash(ctx.seed, SALT_TREE, ka, side_key * 1024 + i64::from(slot)));
            slot += 1;
            if !free(s_m, 1.0) || !s.chance(0.5 * ctx.densities.props) {
                continue;
            }
            let Some((centre, tangent)) = sidewalk_spot(s_m, 1.0) else { continue };
            if fields::sidewalk_width(ctx.seed, centre) < 2.6 {
                continue;
            }
            let pit = s.bits();
            features.push(Feature {
                id: pit,
                kind: FeatureKind::TreePit { centre, tangent, half_m: 0.6, depth_m: 0.04 },
                bounds: Rect::new(centre, centre).grow(0.86),
            });
            props.push(Prop {
                id: pit ^ 1,
                kind: PropKind::Tree,
                position: [centre[0], centre[1], 0.0],
                facing: tangent,
            });
        }
        // Lamp posts every 24 m, staggered between the sides.
        let mut slot = 0;
        let first = if side > 0.0 { 14.0 } else { 26.0 };
        while first + 24.0 * f64::from(slot) <= length - 4.0 {
            let s_m = first + 24.0 * f64::from(slot);
            let mut s = Stream::new(hash(ctx.seed, SALT_LAMP, ka, side_key * 1024 + i64::from(slot)));
            slot += 1;
            if !free(s_m, 0.5) || !s.chance(ctx.densities.props.min(1.0)) {
                continue;
            }
            if let Some((p, t)) = sidewalk_spot(s_m, 0.45) {
                props.push(Prop { id: s.bits(), kind: PropKind::LampPost, position: [p[0], p[1], 0.0], facing: t });
            }
        }
        // Benches, bins, hydrants and bollards every 16 m.
        let mut slot = 0;
        while 8.0 + 16.0 * f64::from(slot) <= length - 4.0 {
            let s_m = 8.0 + 16.0 * f64::from(slot);
            let mut s = Stream::new(hash(ctx.seed, SALT_FURNITURE, ka, side_key * 1024 + i64::from(slot)));
            slot += 1;
            if !free(s_m, 1.0) || !s.chance(0.85 * ctx.densities.props.min(1.0)) {
                continue;
            }
            let pick = s.unit();
            let (kind, f_target) = if pick < 0.35 {
                let (c, _) = point(s_m, hw);
                (PropKind::Bench, fields::sidewalk_width(ctx.seed, c) - 0.7)
            } else if pick < 0.7 {
                (PropKind::Bin, 0.6)
            } else if pick < 0.88 {
                (PropKind::Hydrant, 0.5)
            } else {
                (PropKind::Bollard, 0.35)
            };
            if let Some((p, t)) = sidewalk_spot(s_m, f_target) {
                props.push(Prop { id: s.bits(), kind, position: [p[0], p[1], 0.0], facing: t });
            }
        }
    }
}

/// A curb cut feature with its bounds.
pub(crate) fn curb_cut(
    id: u64,
    centre: V2,
    tangent: V2,
    half_length_m: f64,
    flare_m: f64,
    run_m: f64,
    driveway: bool,
) -> Feature {
    Feature {
        id,
        kind: FeatureKind::CurbCut { centre, tangent, half_length_m, flare_m, run_m, driveway },
        bounds: Rect::new(centre, centre).grow(half_length_m + flare_m + run_m + 1.0),
    }
}
