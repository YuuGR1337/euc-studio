//! Parcels in the blocks between streets: buildings, parks, parking lots and raised plazas with
//! stairs and a ramp beside them.
//!
//! Parcels sit on a global jittered grid of 20 m cells. Each candidate faces its nearest street,
//! fronts on the sidewalk's outer edge, shrinks to fit a tight block, and is kept when its whole
//! footprint lies beyond the sidewalks and no overlapping neighbour has a higher priority (plazas
//! first). Decisions are resolved per 80 m tile, a pure function of the seed and the tile, so a
//! world can cache them for every chunk the tile reaches.

// Geometry reads like its formulas: u, v (the parcel frame), p, a, b, z (a plaza), s (a stream).
#![allow(clippy::many_single_char_names)]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::Densities;
use crate::det::{self, Rect, Stream, V2, hash};
use crate::features::{CTX_REACH_M, Ctx, Feature, Prop, PropKind, SPAWN_CLEAR_M, curb_cut};
use crate::fields;
use crate::roads;
use crate::solid::{Solid, SolidKind};
use crate::surface::Material;

/// Plaza step count range.
pub const PLAZA_STEPS: [i64; 2] = [3, 12];
/// Stair riser range, metres.
pub const RISER_M: [f64; 2] = [0.15, 0.18];
/// Stair tread range, metres.
pub const TREAD_M: [f64; 2] = [0.28, 0.32];
/// Steepest plaza ramp (rise over run).
pub const RAMP_MAX_SLOPE: f64 = 0.125;

const CELL_M: f64 = 20.0;
// An anchor lies at most 20 m behind its front edge; a footprint's centre lies at most 9.7 m
// from the front edge's middle (a plaza's offset) and its corners at most 14.2 m from its
// centre (the largest park).
/// Farthest a parcel reaches from its anchor: 20 + 9.7 + 14.2, rounded up, metres.
const REACH_M: f64 = 45.0;
/// Farthest apart two anchors can be with overlapping footprints: 2 × (20 + 9.7) + 2 × 14.2 + 1,
/// rounded up, metres.
const CONFLICT_M: f64 = 90.0;
/// Free space kept between neighbouring parcels, metres.
const CLEARANCE_M: f64 = 1.0;
/// Footprint scales tried in turn until one fits the block.
const FIT_SCALES: [f64; 3] = [1.0, 0.75, 0.55];
const SALT_PARCEL: u64 = 0x401;
const SALT_PARK_TREE: u64 = 0x402;
const STAIRS_U0_M: f64 = 1.0;
const RAMP_GAP_M: f64 = 0.6;
const RAMP_WIDTH_M: f64 = 1.5;
const RAMP_MAX_LENGTH_M: f64 = 18.0;
const SOLID_BASE_DEPTH_M: f64 = 0.3;

/// Neighbour cells checked on each side for conflicts.
const CONFLICT_CELLS: i64 = 5;
const _: () = assert!(CONFLICT_CELLS as f64 * CELL_M >= CONFLICT_M);
/// Anchor cells per tile side.
const TILE_CELLS: i64 = 4;
const TILE_M: f64 = CELL_M * TILE_CELLS as f64;
/// What a tile's placement reads beyond its anchor area: conflicting neighbours (rounded out by
/// a cell) and their footprints, metres.
const TILE_CTX_MARGIN_M: f64 = (CONFLICT_CELLS + 1) as f64 * CELL_M + REACH_M + 4.0;

/// One 80 m tile of parcel anchors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TileKey {
    a: i64,
    b: i64,
}

/// A kept parcel with the curb cut and trees it brings.
#[derive(Debug, Clone)]
pub(crate) struct Kept {
    pub parcel: Parcel,
    pub extras: Vec<Feature>,
    pub props: Vec<Prop>,
}

/// Every tile whose parcels can reach `rect`.
pub(crate) fn tiles_for(rect: Rect) -> impl Iterator<Item = TileKey> {
    let grown = rect.grow(REACH_M + CELL_M);
    let lo = |v: f64| (v / TILE_M).floor() as i64;
    let (a0, a1, b0, b1) = (lo(grown.min[0]), lo(grown.max[0]), lo(grown.min[1]), lo(grown.max[1]));
    (b0..=b1).flat_map(move |b| (a0..=a1).map(move |a| TileKey { a, b }))
}

/// A raised plaza: stairs up the front, a ramp along the front beside them, a level platform.
/// Coordinates are in the parcel frame: `u` along the front from its left end, `v` away from
/// the street from the front edge.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Plaza {
    /// Number of risers from the base to the platform.
    pub steps: i64,
    /// Riser height, metres.
    pub riser_m: f64,
    /// Tread depth, metres.
    pub tread_m: f64,
    /// Stairs from `u` to `u`, metres.
    pub stairs_u_m: [f64; 2],
    /// `v` of the first riser: the apron in front of the stairs keeps the platform edge at least
    /// the ramp's width (plus 0.3 m) from the front, metres.
    pub stairs_v_m: f64,
    /// Ramp from `u` to the parcel's far end, metres.
    pub ramp_u0_m: f64,
    /// Ramp width (toward the street from the platform edge), metres.
    pub ramp_width_m: f64,
    /// `v` of the platform's front edge, metres.
    pub platform_v_m: f64,
    /// Ground height at the foot of the stairs, metres.
    pub base_m: f64,
    /// Ground height at the foot of the ramp, metres.
    pub ramp_base_m: f64,
    /// Platform height, metres.
    pub top_m: f64,
}

/// What a parcel holds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ParcelKind {
    /// A building: a solid block.
    Building {
        /// Height above the lowest ground under it, metres.
        height_m: f64,
    },
    /// Grass with a gravel path and trees.
    Park,
    /// An asphalt lot with painted bays and a driveway.
    Parking,
    /// A raised plaza.
    Plaza(Plaza),
}

/// A parcel with its frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Parcel {
    /// Stable identity.
    pub id: u64,
    /// What it holds.
    pub kind: ParcelKind,
    /// Front-left corner (`u = 0`, `v = 0`).
    pub origin: V2,
    /// Unit vector along the front.
    pub u: V2,
    /// Unit vector away from the street; `u × v` points up.
    pub v: V2,
    /// Length along `u`, metres.
    pub width_m: f64,
    /// Length along `v`, metres.
    pub depth_m: f64,
    /// Lowest ground under the footprint, metres (solids reach below it).
    pub ground_min_m: f64,
    /// Horizontal bounds.
    pub bounds: Rect,
}

impl Parcel {
    /// `(u, v)` coordinates of a point.
    pub fn local(&self, p: V2) -> V2 {
        let d = det::sub(p, self.origin);
        [det::dot(d, self.u), det::dot(d, self.v)]
    }

    /// Whether a point lies on the footprint.
    pub fn contains(&self, p: V2) -> bool {
        let [pu, pv] = self.local(p);
        (0.0..=self.width_m).contains(&pu) && (0.0..=self.depth_m).contains(&pv)
    }

    fn at(&self, pu: f64, pv: f64) -> V2 {
        det::add(self.origin, det::add(det::scale(self.u, pu), det::scale(self.v, pv)))
    }

    /// Ground height on the footprint given the plain lot height `ground` there. `inset`
    /// shrinks every raised region for physics sampling, so each face lies inside its solid.
    pub(crate) fn height(&self, p: V2, ground: f64, inset: f64) -> f64 {
        let [pu, pv] = self.local(p);
        match self.kind {
            ParcelKind::Plaza(z) => {
                let (w, d) = (self.width_m, self.depth_m);
                let from = z.stairs_v_m + inset;
                if pu >= z.stairs_u_m[0] + inset && pu <= z.stairs_u_m[1] - inset && pv >= from && pv <= d - inset {
                    let k = ((pv - from) / z.tread_m).floor() as i64 + 1;
                    return ground.max(z.base_m + k.min(z.steps) as f64 * z.riser_m);
                }
                if pv >= z.platform_v_m + inset && pu >= inset && pu <= w - inset && pv <= d - inset {
                    return z.top_m;
                }
                if pu >= z.ramp_u0_m && pv >= z.platform_v_m - z.ramp_width_m + inset && pv < z.platform_v_m + inset {
                    let t = ((pu - z.ramp_u0_m) / (w - z.ramp_u0_m)).min(1.0);
                    return ground.max(det::lerp(z.ramp_base_m, z.top_m, t));
                }
                ground
            }
            ParcelKind::Park => {
                let edge = pu.min(self.width_m - pu).min(pv).min(self.depth_m - pv);
                ground + 0.03 * det::value_noise(self.id, 1, p, 5.0) * det::smoothstep(0.0, 2.0, edge)
            }
            ParcelKind::Building { .. } | ParcelKind::Parking => ground,
        }
    }

    /// Material on the footprint.
    pub(crate) fn material(&self, p: V2) -> Material {
        let [pu, pv] = self.local(p);
        match self.kind {
            ParcelKind::Building { .. } => Material::Concrete,
            ParcelKind::Park => {
                if (pv - self.depth_m / 2.0).abs() < 0.8 {
                    Material::Gravel
                } else {
                    Material::Grass
                }
            }
            ParcelKind::Parking => {
                let bay = pu - 2.5 * (pu / 2.5).floor();
                let in_rows = (1.0..=6.0).contains(&pv) || (self.depth_m - 6.0..=self.depth_m - 1.0).contains(&pv);
                if in_rows && !(0.05..=2.45).contains(&bay) { Material::Paint } else { Material::Asphalt }
            }
            ParcelKind::Plaza(z) => {
                let stairs =
                    pu >= z.stairs_u_m[0] && pu <= z.stairs_u_m[1] && pv >= z.stairs_v_m && pv < z.platform_v_m;
                let ramp = pu >= z.ramp_u0_m && pv >= z.platform_v_m - z.ramp_width_m && pv < z.platform_v_m;
                if stairs || ramp { Material::Concrete } else { Material::PavingStones }
            }
        }
    }

    /// Solid boxes: building block, or plaza steps, platform and ramp.
    pub fn solids(&self) -> Vec<Solid> {
        let base = self.ground_min_m - SOLID_BASE_DEPTH_M;
        let (o, u, v) = (self.origin, self.u, self.v);
        let id = |k: u64| det::mix(self.id ^ k.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        match self.kind {
            ParcelKind::Building { height_m } => {
                let top = self.ground_min_m + height_m;
                vec![Solid::upright(
                    id(1),
                    SolidKind::Building,
                    o,
                    u,
                    v,
                    [0.0, 0.0],
                    [self.width_m, self.depth_m],
                    [base, top],
                )]
            }
            ParcelKind::Plaza(z) => {
                let mut solids = Vec::new();
                for k in 1..z.steps {
                    let (v0, v1) = (z.stairs_v_m + (k - 1) as f64 * z.tread_m, z.stairs_v_m + k as f64 * z.tread_m);
                    let top = z.base_m + k as f64 * z.riser_m;
                    solids.push(Solid::upright(
                        id(10 + k as u64),
                        SolidKind::Step,
                        o,
                        u,
                        v,
                        [z.stairs_u_m[0], v0],
                        [z.stairs_u_m[1], v1],
                        [base, top],
                    ));
                }
                solids.push(Solid::upright(
                    id(2),
                    SolidKind::Platform,
                    o,
                    u,
                    v,
                    [0.0, z.platform_v_m],
                    [self.width_m, self.depth_m],
                    [base, z.top_m],
                ));
                let edge = z.platform_v_m - z.ramp_width_m;
                let a = self.at(z.ramp_u0_m, edge);
                let b = self.at(self.width_m, edge);
                solids.push(Solid::slab(
                    id(3),
                    SolidKind::Ramp,
                    [a[0], a[1], z.ramp_base_m],
                    [b[0], b[1], z.top_m],
                    z.ramp_width_m,
                    1.0,
                    z.top_m - base + 0.2,
                ));
                solids
            }
            ParcelKind::Park | ParcelKind::Parking => Vec::new(),
        }
    }
}

struct Candidate {
    parcel: Parcel,
    priority: u64,
    extras: Vec<Feature>,
    props: Vec<Prop>,
}

/// Whether two footprints come closer than `clearance` (separating-axis test).
fn overlaps(a: &Parcel, b: &Parcel, clearance: f64) -> bool {
    let corners = |p: &Parcel| {
        [[0.0, 0.0], [p.width_m, 0.0], [0.0, p.depth_m], [p.width_m, p.depth_m]].map(|[pu, pv]| p.at(pu, pv))
    };
    let (ca, cb) = (corners(a), corners(b));
    for axis in [a.u, a.v, b.u, b.v] {
        let span = |c: &[V2; 4]| {
            c.iter()
                .map(|&p| det::dot(p, axis))
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)))
        };
        let ((a0, a1), (b0, b1)) = (span(&ca), span(&cb));
        if a1 + clearance <= b0 || b1 + clearance <= a0 {
            return false;
        }
    }
    true
}

/// The lowest ground under a footprint, if it lies wholly beyond the sidewalks and away from
/// the spawn.
fn fits(ctx: &Ctx, origin: V2, u: V2, v: V2, width: f64, depth: f64) -> Option<f64> {
    let inside = |[pu, pv]: V2| {
        let p = det::add(origin, det::add(det::scale(u, pu), det::scale(v, pv)));
        (ctx.field(p).f >= fields::sidewalk_width(ctx.seed, p) + 0.2 && det::len(p) >= SPAWN_CLEAR_M + 15.0)
            .then_some(p)
    };
    // The far corners fail most often: try them before the whole grid.
    if inside([0.0, depth]).is_none() || inside([width, depth]).is_none() {
        return None;
    }
    let mut ground_min = f64::INFINITY;
    for sample in footprint_samples(width, depth) {
        ground_min = ground_min.min(fields::lot_height(ctx.seed, inside(sample)?));
    }
    Some(ground_min)
}

/// Samples of a footprint: a grid at most 2 m apart, edges included.
fn footprint_samples(width: f64, depth: f64) -> Vec<V2> {
    let nu = (width / 2.0).ceil().max(1.0) as usize;
    let nv = (depth / 2.0).ceil().max(1.0) as usize;
    let mut out = Vec::with_capacity((nu + 1) * (nv + 1));
    for a in 0..=nu {
        for b in 0..=nv {
            out.push([width * a as f64 / nu as f64, depth * b as f64 / nv as f64]);
        }
    }
    out
}

fn make(ctx: &Ctx, a: i64, b: i64) -> Option<Candidate> {
    let h = hash(ctx.seed, SALT_PARCEL, a, b);
    let mut s = Stream::new(h);
    let anchor = [(a as f64 + s.range(0.1, 0.9)) * CELL_M, (b as f64 + s.range(0.1, 0.9)) * CELL_M];
    let hit = ctx.field(anchor);
    let near = hit.near?;
    let sidewalk = fields::sidewalk_width(ctx.seed, anchor);
    let depth_into_block = hit.row.min(hit.column);
    if !(sidewalk + 3.0..=sidewalk + 20.0).contains(&depth_into_block) || hit.f < sidewalk + 2.0 {
        return None;
    }
    let v = det::norm_or(det::sub(anchor, near.point), [0.0, 1.0]);
    let u = [v[1], -v[0]];
    let weights = [0.55, 0.15, 0.12, ctx.densities.plazas];
    let total: f64 = weights.iter().sum();
    let mut pick = s.unit() * total;
    let mut class = 0;
    while class < 3 && pick >= weights[class] {
        pick -= weights[class];
        class += 1;
    }
    let setback = if class == 0 { s.range(0.5, 2.5) } else { 0.3 };
    // The front edge's middle: back from the anchor to the sidewalk's outer edge plus setback.
    let front = det::sub(anchor, det::scale(v, hit.f - (sidewalk + setback)));
    let (kind, width, depth, origin, ground_min, extras) = if class == 3 {
        let (kind, width, depth, origin) = plaza(ctx, &mut s, front, u, v)?;
        (kind, width, depth, origin, fits(ctx, origin, u, v, width, depth)?, Vec::new())
    } else {
        let (kind, w, d, min_w) = match class {
            0 => (ParcelKind::Building { height_m: s.range(6.0, 40.0) }, s.range(8.0, 18.0), s.range(8.0, 16.0), 6.0),
            1 => (ParcelKind::Park, s.range(12.0, 22.0), s.range(10.0, 18.0), 8.0),
            _ => (ParcelKind::Parking, s.range(12.0, 20.0), s.range(12.0, 18.0), 10.0),
        };
        // Shrink to fit a tight block, keeping the front centred.
        let (width, depth, origin, ground_min) = FIT_SCALES.iter().find_map(|&k| {
            let (w, d) = ((w * k).max(min_w), (d * k).max(min_w));
            let origin = det::sub(front, det::scale(u, w / 2.0));
            fits(ctx, origin, u, v, w, d).map(|g| (w, d, origin, g))
        })?;
        let extras = if class == 2 {
            // The driveway: a curb cut where the parcel's middle meets the curb.
            let curb = det::sub(front, det::scale(v, sidewalk + setback));
            vec![curb_cut(h ^ 0x5a5a, curb, u, 2.0, 0.75, 1.0, true)]
        } else {
            Vec::new()
        };
        (kind, width, depth, origin, ground_min, extras)
    };
    let mut bounds = Rect::empty();
    for [pu, pv] in [[0.0, 0.0], [width, 0.0], [0.0, depth], [width, depth]] {
        bounds.include(det::add(origin, det::add(det::scale(u, pu), det::scale(v, pv))));
    }
    let parcel = Parcel { id: h, kind, origin, u, v, width_m: width, depth_m: depth, ground_min_m: ground_min, bounds };
    let mut props = Vec::new();
    if matches!(kind, ParcelKind::Park) {
        park_trees(ctx, &parcel, &mut props);
    }
    // Plazas win conflicts, then buildings, parking, parks.
    let rank: u64 = match class {
        3 => 3,
        0 => 2,
        2 => 1,
        _ => 0,
    };
    Some(Candidate { parcel, priority: rank << 60 | h >> 4, extras, props })
}

/// Lays out a plaza fronting at `front`: its kind, width, depth and origin. Raises the step
/// count until the platform clears the ground everywhere; gives up past the largest count.
fn plaza(ctx: &Ctx, s: &mut Stream, front: V2, u: V2, v: V2) -> Option<(ParcelKind, f64, f64, V2)> {
    let first = s.int(PLAZA_STEPS[0], PLAZA_STEPS[1]);
    let riser = s.range(RISER_M[0], RISER_M[1]);
    let tread = s.range(TREAD_M[0], TREAD_M[1]);
    let stairs_width = s.range(2.0, 4.0);
    let platform_depth = s.range(4.0, 7.0);
    let slope = s.range(0.083, 0.12);
    let ramp_u0 = STAIRS_U0_M + stairs_width + RAMP_GAP_M;
    let origin = det::sub(front, det::scale(u, ramp_u0 + 2.0));
    let at = |pu: f64, pv: f64| det::add(origin, det::add(det::scale(u, pu), det::scale(v, pv)));
    for steps in first..=PLAZA_STEPS[1] {
        // An apron in front of short stairs keeps the ramp strip inside the parcel.
        let apron = (RAMP_WIDTH_M + 0.3 - (steps - 1) as f64 * tread).max(0.0);
        let base = fields::lot_height(ctx.seed, at(STAIRS_U0_M + stairs_width / 2.0, apron));
        let platform_v = apron + (steps - 1) as f64 * tread;
        let top = base + steps as f64 * riser;
        let ramp_base = fields::lot_height(ctx.seed, at(ramp_u0, platform_v - RAMP_WIDTH_M / 2.0));
        if top - ramp_base < 0.3 {
            continue;
        }
        let ramp_length = (top - ramp_base) / slope.max((top - ramp_base) / RAMP_MAX_LENGTH_M);
        if (top - ramp_base) / ramp_length > RAMP_MAX_SLOPE {
            continue;
        }
        let width = ramp_u0 + ramp_length;
        let depth = platform_v + platform_depth;
        let mut fits = true;
        for [pu, pv] in footprint_samples(width, depth) {
            let ground = fields::lot_height(ctx.seed, at(pu, pv));
            let on_stairs = pu >= STAIRS_U0_M && pu <= STAIRS_U0_M + stairs_width && pv >= apron && pv < platform_v;
            let limit = if pv >= platform_v {
                top - riser
            } else if on_stairs {
                base + riser - 0.02
            } else {
                f64::INFINITY
            };
            if ground > limit {
                fits = false;
                break;
            }
        }
        if !fits {
            continue;
        }
        let z = Plaza {
            steps,
            riser_m: riser,
            tread_m: tread,
            stairs_u_m: [STAIRS_U0_M, STAIRS_U0_M + stairs_width],
            stairs_v_m: apron,
            ramp_u0_m: ramp_u0,
            ramp_width_m: RAMP_WIDTH_M,
            platform_v_m: platform_v,
            base_m: base,
            ramp_base_m: ramp_base,
            top_m: top,
        };
        return Some((ParcelKind::Plaza(z), width, depth, origin));
    }
    None
}

fn park_trees(ctx: &Ctx, parcel: &Parcel, props: &mut Vec<Prop>) {
    let nu = (parcel.width_m / 6.0).floor() as i64;
    let nv = (parcel.depth_m / 6.0).floor() as i64;
    for a in 0..nu {
        for b in 0..nv {
            let mut s = Stream::new(hash(ctx.seed, SALT_PARK_TREE, parcel.id as i64, a * 64 + b));
            if !s.chance(0.45) {
                continue;
            }
            let pu = (a as f64 + s.range(0.3, 0.7)) * 6.0;
            let pv = (b as f64 + s.range(0.3, 0.7)) * 6.0;
            if pu < 1.0 || pu > parcel.width_m - 1.0 || pv < 1.0 || pv > parcel.depth_m - 1.0 {
                continue;
            }
            if (pv - parcel.depth_m / 2.0).abs() < 1.5 {
                continue;
            }
            let p = parcel.at(pu, pv);
            props.push(Prop { id: s.bits(), kind: PropKind::Tree, position: [p[0], p[1], 0.0], facing: parcel.u });
        }
    }
}

/// The kept parcels anchored in one tile. A pure function of the seed, densities and tile.
pub(crate) fn tile(seed: u64, densities: &Densities, key: TileKey) -> Vec<Kept> {
    let anchors = Rect::new(
        [key.a as f64 * TILE_M, key.b as f64 * TILE_M],
        [(key.a + 1) as f64 * TILE_M, (key.b + 1) as f64 * TILE_M],
    );
    let ctx_rect = anchors.grow(TILE_CTX_MARGIN_M);
    let edges = roads::edges_near(seed, ctx_rect, CTX_REACH_M);
    let ctx = Ctx::new(seed, densities, &edges, ctx_rect);
    let k = CONFLICT_CELLS;
    let (a0, b0) = (key.a * TILE_CELLS, key.b * TILE_CELLS);
    let (a1, b1) = (a0 + TILE_CELLS - 1, b0 + TILE_CELLS - 1);
    // Every anchor of the tile, and every neighbour that can conflict with one.
    let mut memo: HashMap<(i64, i64), Option<Candidate>> = HashMap::new();
    for a in a0 - k..=a1 + k {
        for b in b0 - k..=b1 + k {
            memo.insert((a, b), make(&ctx, a, b));
        }
    }
    let mut out = Vec::new();
    for b in b0..=b1 {
        for a in a0..=a1 {
            let Some(Some(c)) = memo.get(&(a, b)) else { continue };
            let beaten = (-k..=k).any(|da| {
                (-k..=k).any(|db| {
                    (da, db) != (0, 0)
                        && memo.get(&(a + da, b + db)).is_some_and(|n| {
                            n.as_ref()
                                .is_some_and(|n| n.priority > c.priority && overlaps(&n.parcel, &c.parcel, CLEARANCE_M))
                        })
                })
            });
            if !beaten {
                out.push(Kept { parcel: c.parcel, extras: c.extras.clone(), props: c.props.clone() });
            }
        }
    }
    out
}

/// From kept parcels, those (and the curb cuts and trees they bring) that reach `rect`.
pub(crate) fn reaching(
    kept: &[Kept],
    rect: Rect,
    features: &mut Vec<Feature>,
    props: &mut Vec<Prop>,
    out: &mut Vec<Parcel>,
) {
    for c in kept {
        let touches = c.parcel.bounds.overlaps(&rect)
            || c.extras.iter().any(|e| e.bounds.overlaps(&rect))
            || c.props.iter().any(|p| rect.grow(1.0).contains([p.position[0], p.position[1]]));
        if touches {
            features.extend(c.extras.iter().filter(|e| e.bounds.overlaps(&rect)).cloned());
            props.extend(c.props.iter().copied());
            out.push(c.parcel);
        }
    }
}
