//! One 16 m × 16 m chunk: the primitives that reach it, binned, and the sampler over them.
//!
//! A chunk lists every street, feature and parcel that changes anything inside it, including
//! those that start in a neighbour. Its sampler is exact for any point inside its rectangle
//! (borders included), and two chunks give bit-identical results on their shared border.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::det::{self, Digest, Rect, V2, hash};
use crate::features::{self, CTX_REACH_M, Ctx, Feature, FeatureKind, GRID_REACH_M, Prop};
use crate::fields::{self, CUT_LIP_M, SIDEWALK_SLOPE};
use crate::parcels::{self, Kept, Parcel, ParcelKind, TileKey};
use crate::roads::{self, Edge, FIELD_REACH_M, RoadHit, StreetClass};
use crate::solid::{Solid, SolidKind};
use crate::surface::{Material, Surface};
use crate::{Densities, WorldSpec};

/// Chunk side, metres.
pub const CHUNK_M: f64 = 16.0;
/// A face lower than this gets no solid; the height field carries it, metres.
pub const FACE_MIN_M: f64 = 0.04;
/// Depth of a curb stone from its face into the sidewalk, metres.
pub const CURB_DEPTH_M: f64 = 0.15;

const BINS: usize = 4;
const BIN_M: f64 = CHUNK_M / BINS as f64;
/// Marching-squares cells per side for curb contours (0.25 m).
const CONTOUR_CELLS: usize = 64;
/// What a chunk's own placement reads beyond it: grid features and their neighbours, and every
/// slot along a street edge that reaches it (an edge is at most about 90 m long), metres.
const CTX_MARGIN_M: f64 = 110.0;
const _: () = assert!(CTX_MARGIN_M >= GRID_REACH_M + 2.0 * CHUNK_M);
/// Coarse contour cells per side: a coarse cell whose corners all lie farther than its half
/// diagonal from the curb line holds none of it (the road field is 1-Lipschitz).
const COARSE_CELLS: usize = 8;
const SALT_CURB: u64 = 0x501;
const SALT_LOT: [u64; 2] = [0x502, 0x503];

/// A chunk's integer coordinates: it covers `[16 cx, 16 cx + 16] × [16 cy, 16 cy + 16]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChunkKey {
    /// Column.
    pub cx: i32,
    /// Row.
    pub cy: i32,
}

impl ChunkKey {
    /// The chunk containing a point.
    pub fn of(p: V2) -> Self {
        Self { cx: (p[0] / CHUNK_M).floor() as i32, cy: (p[1] / CHUNK_M).floor() as i32 }
    }

    /// The chunk's rectangle.
    pub fn rect(self) -> Rect {
        let (x, y) = (f64::from(self.cx) * CHUNK_M, f64::from(self.cy) * CHUNK_M);
        Rect::new([x, y], [x + CHUNK_M, y + CHUNK_M])
    }

    fn packed(self) -> i64 {
        (i64::from(self.cx) << 32) | i64::from(self.cy as u32)
    }
}

/// What kind of ground a point is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Zone {
    /// The carriageway, gutter included.
    Road,
    /// Between the curb face and the sidewalk's outer edge.
    Sidewalk,
    /// Open ground in a block: lawn, dirt, gravel.
    Lot,
    /// A building's footprint (the building's solid stands on it).
    Building,
    /// A park.
    Park,
    /// A parking lot.
    Parking,
    /// A raised plaza with its stairs and ramp.
    Plaza,
}

/// Height, zone and surface at a point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Ground height, metres.
    pub height_m: f64,
    /// Zone.
    pub zone: Zone,
    /// Material and contact parameters.
    pub surface: Surface,
}

/// A contour crossing: on the grid edge from vertex (ix, iy) toward +x (0) or +y (1).
type Key = (usize, usize, u8);

#[derive(Debug, Clone, Default)]
struct Bin {
    edges: Vec<u16>,
    features: Vec<u16>,
    parcels: Vec<u16>,
}

/// One chunk's primitives and sampler.
#[derive(Debug, Clone)]
pub struct Chunk {
    key: ChunkKey,
    rect: Rect,
    seed: u64,
    edges: Vec<Edge>,
    features: Vec<Feature>,
    parcels: Vec<Parcel>,
    props: Vec<Prop>,
    solids: Vec<Solid>,
    bins: Vec<Bin>,
}

impl Chunk {
    /// Generates a chunk. Depends only on the specification and the key.
    pub fn generate(spec: &WorldSpec, key: ChunkKey) -> Self {
        Self::generate_with(spec, key, &|tile| std::sync::Arc::new(parcels::tile(spec.seed, &spec.densities, tile)))
    }

    /// Generates a chunk with resolved parcel tiles from `tiles` (a cache of [`parcels::tile`]).
    pub(crate) fn generate_with(
        spec: &WorldSpec,
        key: ChunkKey,
        tiles: &dyn Fn(TileKey) -> std::sync::Arc<Vec<Kept>>,
    ) -> Self {
        let rect = key.rect();
        let seed = spec.seed;
        let ctx_rect = rect.grow(CTX_MARGIN_M);
        let ctx_edges = roads::edges_near(seed, ctx_rect, CTX_REACH_M);
        let densities: &Densities = &spec.densities;
        let ctx = Ctx::new(seed, densities, &ctx_edges, ctx_rect);

        let mut features = Vec::new();
        let mut props = Vec::new();
        features::grid_features(&ctx, rect, &mut features);
        for edge in &ctx_edges {
            if rect.gap(&edge.bounds) - edge.half_width_m <= 8.0 {
                let start = features.len();
                features::street_items(&ctx, edge, &mut features, &mut props);
                let kept: Vec<Feature> = features.drain(start..).filter(|f| f.bounds.overlaps(&rect)).collect();
                features.extend(kept);
            }
        }
        let mut parcels = Vec::new();
        for tile in parcels::tiles_for(rect) {
            parcels::reaching(&tiles(tile), rect, &mut features, &mut props, &mut parcels);
        }
        features.sort_by_key(Feature::order_key);
        features.dedup_by_key(|f| f.id);
        let edges: Vec<Edge> =
            ctx_edges.iter().filter(|e| rect.gap(&e.bounds) - e.half_width_m <= FIELD_REACH_M).cloned().collect();

        let mut chunk =
            Self { key, rect, seed, edges, features, parcels, props: Vec::new(), solids: Vec::new(), bins: Vec::new() };
        chunk.bin();
        // Props: the chunk owns those standing in it; solids also come from neighbours' props.
        props.sort_by_key(|p| p.id);
        props.dedup_by_key(|p| p.id);
        let mut solids = chunk.curb_solids();
        for parcel in &chunk.parcels {
            solids.extend(parcel.solids().into_iter().filter(|s| s.bounds().overlaps(&rect)));
        }
        let outer = rect.grow(1.0);
        for mut prop in props {
            let xy = [prop.position[0], prop.position[1]];
            if !outer.contains(xy) {
                continue;
            }
            prop.position[2] = chunk.height_at(xy);
            let solid = prop_solid(&prop);
            if solid.bounds().overlaps(&rect) {
                solids.push(solid);
            }
            let owned = xy[0] >= rect.min[0] && xy[0] < rect.max[0] && xy[1] >= rect.min[1] && xy[1] < rect.max[1];
            if owned {
                chunk.props.push(prop);
            }
        }
        solids.sort_by_key(|s| s.id);
        solids.dedup_by_key(|s| s.id);
        chunk.solids = solids;
        chunk
    }

    fn bin(&mut self) {
        let mut bins = vec![Bin::default(); BINS * BINS];
        for (k, bin) in bins.iter_mut().enumerate() {
            let x = self.rect.min[0] + (k % BINS) as f64 * BIN_M;
            let y = self.rect.min[1] + (k / BINS) as f64 * BIN_M;
            let r = Rect::new([x, y], [x + BIN_M, y + BIN_M]).grow(0.01);
            bin.edges = (0..self.edges.len())
                .filter(|&e| r.gap(&self.edges[e].bounds) - self.edges[e].half_width_m <= FIELD_REACH_M)
                .map(|e| e as u16)
                .collect();
            bin.features =
                (0..self.features.len()).filter(|&f| self.features[f].bounds.overlaps(&r)).map(|f| f as u16).collect();
            bin.parcels =
                (0..self.parcels.len()).filter(|&f| self.parcels[f].bounds.overlaps(&r)).map(|f| f as u16).collect();
        }
        self.bins = bins;
    }

    fn bin_of(&self, p: V2) -> &Bin {
        let index = |v: f64, min: f64| (((v - min) / BIN_M).floor().max(0.0) as usize).min(BINS - 1);
        &self.bins[index(p[1], self.rect.min[1]) * BINS + index(p[0], self.rect.min[0])]
    }

    /// The chunk's key.
    pub const fn key(&self) -> ChunkKey {
        self.key
    }

    /// The chunk's rectangle.
    pub const fn rect(&self) -> Rect {
        self.rect
    }

    /// Street edges whose influence reaches the chunk.
    pub fn streets(&self) -> &[Edge] {
        &self.edges
    }

    /// Features that change anything in the chunk, in evaluation order.
    pub fn features(&self) -> &[Feature] {
        &self.features
    }

    /// Parcels that overlap the chunk.
    pub fn parcels(&self) -> &[Parcel] {
        &self.parcels
    }

    /// Props standing in the chunk (each prop belongs to exactly one chunk).
    pub fn props(&self) -> &[Prop] {
        &self.props
    }

    /// Solids that overlap the chunk, by identity (shared ones appear in every chunk they touch).
    pub fn solids(&self) -> &[Solid] {
        &self.solids
    }

    /// The road field at `p`.
    fn field(&self, p: V2) -> RoadHit {
        roads::field(&self.edges, &self.bin_of(p).edges, p)
    }

    /// Ground height at `p`, metres: the drawn surface, faces where they are.
    pub fn height_at(&self, p: V2) -> f64 {
        self.evaluate(p, 0.0, false).height_m
    }

    /// Ground height for physics sampling: each face that has a solid moves `inset` metres
    /// behind the solid's face, so the height field's slope stays inside the solid.
    pub fn ground_at(&self, p: V2, inset: f64) -> f64 {
        self.evaluate(p, inset, false).height_m
    }

    /// Height, zone and surface at `p`.
    pub fn sample(&self, p: V2) -> Sample {
        self.evaluate(p, 0.0, true)
    }

    /// The curb face height at a point on the curb line (curb cuts applied), metres.
    fn face_at(&self, p: V2) -> f64 {
        let bin = self.bin_of(p);
        let cut = bin.features.iter().map(|&k| self.features[usize::from(k)].cut(p, 0.0)).fold(0.0, f64::max);
        let c = fields::curb_height(self.seed, p);
        c - (c - CUT_LIP_M) * cut
    }

    fn evaluate(&self, p: V2, inset: f64, with_surface: bool) -> Sample {
        let seed = self.seed;
        let bin = self.bin_of(p);
        let hit = self.field(p);
        let f = hit.f;
        let terrain = fields::terrain(seed, p);
        let sidewalk = fields::sidewalk_width(seed, p);
        let mut height = if f < inset {
            terrain + fields::crown(f)
        } else {
            let cut = bin.features.iter().map(|&k| self.features[usize::from(k)].cut(p, f)).fold(0.0, f64::max);
            let c = fields::curb_height(seed, p);
            terrain + c - (c - CUT_LIP_M) * cut + SIDEWALK_SLOPE * f.min(sidewalk)
        };
        let mut zone = if f < 0.0 {
            Zone::Road
        } else if f < sidewalk {
            Zone::Sidewalk
        } else {
            Zone::Lot
        };
        let parcel = if zone == Zone::Lot {
            bin.parcels.iter().map(|&k| &self.parcels[usize::from(k)]).find(|parcel| parcel.contains(p))
        } else {
            None
        };
        if let Some(parcel) = parcel {
            height = parcel.height(p, height, inset);
            zone = match parcel.kind {
                ParcelKind::Building { .. } => Zone::Building,
                ParcelKind::Park => Zone::Park,
                ParcelKind::Parking => Zone::Parking,
                ParcelKind::Plaza(_) => Zone::Plaza,
            };
        }
        for &k in &bin.features {
            if let Some(dh) = self.features[usize::from(k)].height(p, &self.edges, &hit) {
                height += dh;
            }
        }
        let near_edge = hit.near.map(|n| &self.edges[n.edge]);
        let paving = paving_edge(&self.edges, &hit);
        let material =
            |e: Option<&Edge>, road: bool| e.map_or(Material::Asphalt, |e| if road { e.material } else { e.sidewalk });
        height += match zone {
            Zone::Road if -f >= 0.3 => match material(paving, true) {
                Material::Cobblestones => cobbles(p),
                Material::Concrete if paving.map(|e| e.id) == near_edge.map(|e| e.id) => hit
                    .near
                    .map_or(0.0, |n| joint(n.s, 4.5, 0.012, 0.008) + if n.across.abs() < 0.006 { -0.008 } else { 0.0 }),
                _ => 0.0,
            },
            Zone::Sidewalk if f >= CURB_DEPTH_M => match material(near_edge, false) {
                Material::Concrete => hit.near.map_or(0.0, |n| joint(n.s, 1.5, 0.01, 0.006)),
                Material::PavingStones => pavers(p),
                _ => 0.0,
            },
            Zone::Plaza if parcel.is_some_and(|parcel| parcel.material(p) == Material::PavingStones) => pavers(p),
            _ => 0.0,
        };
        let surface =
            if with_surface { self.surface(p, &hit, zone, parcel) } else { Surface::of(Material::Asphalt, false) };
        Sample { height_m: height, zone, surface }
    }

    /// The surface at `p`: features first (manhole, pothole, tree pit, paint), then the zone's own.
    fn surface(&self, p: V2, hit: &RoadHit, zone: Zone, parcel: Option<&Parcel>) -> Surface {
        let (bin, f) = (self.bin_of(p), hit.f);
        let near_edge = hit.near.map(|n| &self.edges[n.edge]);
        let paving = paving_edge(&self.edges, hit);
        let material =
            |e: Option<&Edge>, road: bool| e.map_or(Material::Asphalt, |e| if road { e.material } else { e.sidewalk });
        let mut wet = false;
        let mut best: Option<(u8, Material)> = None;
        let mut offer = |rank: u8, m: Material| {
            if best.is_none_or(|(r, _)| rank < r) {
                best = Some((rank, m));
            }
        };
        for &k in &bin.features {
            let feature = &self.features[usize::from(k)];
            match &feature.kind {
                FeatureKind::Manhole { centre, radius_m, .. } => {
                    let d = det::len(det::sub(p, *centre));
                    if d < *radius_m {
                        offer(0, Material::Metal);
                    } else if d < radius_m + 0.08 {
                        offer(1, Material::Concrete);
                    }
                }
                FeatureKind::Pothole { blob, puddle, .. } => {
                    if blob.distance(p) < 0.0 {
                        offer(2, Material::Gravel);
                        wet |= *puddle;
                    }
                }
                FeatureKind::TreePit { .. } => {
                    if feature.height(p, &self.edges, hit).is_some() {
                        offer(3, Material::Dirt);
                    }
                }
                FeatureKind::Crosswalk { edge, s_m, half_depth_m } => {
                    if let Some(n) = hit.near
                        && zone == Zone::Road
                        && -f > 0.3
                        && self.edges[n.edge].id == *edge
                        && (n.s - s_m).abs() <= *half_depth_m
                        && fraction(n.across, 1.0) < 0.5
                    {
                        offer(4, Material::Paint);
                    }
                }
                FeatureKind::SpeedBump { .. } => {
                    if feature.height(p, &self.edges, hit).is_some()
                        && hit.near.is_some_and(|n| fraction(n.across, 1.2) < 0.4)
                    {
                        offer(5, Material::Paint);
                    }
                }
                FeatureKind::RoughPatch { blob, .. } => {
                    if zone == Zone::Road && blob.distance(p) < 0.0 {
                        offer(7, Material::WornAsphalt);
                    }
                }
                FeatureKind::CurbCut { driveway: true, .. } => {
                    if zone == Zone::Sidewalk && feature.cut(p, f) > 0.5 {
                        offer(8, Material::Concrete);
                    }
                }
                FeatureKind::WetPatch { blob } => wet |= blob.distance(p) < 0.0,
                FeatureKind::CurbCut { .. } | FeatureKind::Crack { .. } => {}
            }
        }
        if zone == Zone::Road && near_edge.is_some_and(|e| marking(e, hit)) {
            offer(6, Material::Paint);
        }
        let base = match zone {
            Zone::Road if -f < 0.3 => Material::Concrete,
            Zone::Road => material(paving, true),
            Zone::Sidewalk if f < CURB_DEPTH_M => Material::Concrete,
            Zone::Sidewalk => material(near_edge, false),
            Zone::Lot => lot_material(self.seed, p),
            _ => parcel.map_or(Material::Grass, |parcel| parcel.material(p)),
        };
        offer(9, base);
        Surface::of(best.map_or(base, |(_, m)| m), wet)
    }

    /// Boxes along every curb face of at least [`FACE_MIN_M`], traced from the road field's zero
    /// contour on a 0.25 m grid and clipped to the chunk.
    fn curb_solids(&self) -> Vec<Solid> {
        let mut solids = Vec::new();
        for chain in self.contour_chains() {
            let points = simplify(&chain, 0.01);
            for pair in points.windows(2) {
                let pieces = (det::len(det::sub(pair[1], pair[0])) / 2.0).ceil().max(1.0) as usize;
                for k in 0..pieces {
                    let a = det::add(pair[0], det::scale(det::sub(pair[1], pair[0]), k as f64 / pieces as f64));
                    let b = det::add(pair[0], det::scale(det::sub(pair[1], pair[0]), (k + 1) as f64 / pieces as f64));
                    self.curb_piece(a, b, &mut solids);
                }
            }
        }
        solids
    }

    fn contour_vertex(&self, ix: usize, iy: usize) -> V2 {
        let step = CHUNK_M / CONTOUR_CELLS as f64;
        [self.rect.min[0] + ix as f64 * step, self.rect.min[1] + iy as f64 * step]
    }

    /// The road field on the contour grid. Coarse cells that cannot hold a zero take the value
    /// of a corner (same sign), which is all marching squares reads there.
    fn contour_values(&self) -> Vec<f64> {
        let n = CONTOUR_CELLS;
        let step = CHUNK_M / n as f64;
        let vertex = |ix: usize, iy: usize| self.contour_vertex(ix, iy);
        let per = n / COARSE_CELLS;
        let coarse: Vec<f64> = (0..=COARSE_CELLS)
            .flat_map(|cy| (0..=COARSE_CELLS).map(move |cx| (cx, cy)))
            .map(|(cx, cy)| self.field(vertex(cx * per, cy * per)).f)
            .collect();
        let reach = step * per as f64 * std::f64::consts::FRAC_1_SQRT_2 + 0.01;
        let mut values = vec![f64::NAN; (n + 1) * (n + 1)];
        for cy in 0..COARSE_CELLS {
            for cx in 0..COARSE_CELLS {
                let corners = [(cx, cy), (cx + 1, cy), (cx, cy + 1), (cx + 1, cy + 1)]
                    .map(|(x, y)| coarse[y * (COARSE_CELLS + 1) + x]);
                let quiet = corners.iter().all(|&v| v > reach) || corners.iter().all(|&v| v < -reach);
                for iy in cy * per..=(cy + 1) * per {
                    for ix in cx * per..=(cx + 1) * per {
                        let slot = &mut values[iy * (n + 1) + ix];
                        if quiet {
                            if slot.is_nan() {
                                *slot = corners[0];
                            }
                        } else {
                            *slot = self.field(vertex(ix, iy)).f;
                        }
                    }
                }
            }
        }
        values
    }

    /// The curb line (the road field's zero contour) inside the chunk, as polylines.
    fn contour_chains(&self) -> Vec<Vec<V2>> {
        let n = CONTOUR_CELLS;
        let values = self.contour_values();
        let value = |ix: usize, iy: usize| values[iy * (n + 1) + ix];
        let crossing = |(ix, iy, dir): Key| -> V2 {
            let (jx, jy) = if dir == 0 { (ix + 1, iy) } else { (ix, iy + 1) };
            let (fa, fb) = (value(ix, iy), value(jx, jy));
            let t = fa / (fa - fb);
            let (a, b) = (self.contour_vertex(ix, iy), self.contour_vertex(jx, jy));
            [det::lerp(a[0], b[0], t), det::lerp(a[1], b[1], t)]
        };
        let mut segments: Vec<(Key, Key)> = Vec::new();
        for iy in 0..n {
            for ix in 0..n {
                let f = [value(ix, iy), value(ix + 1, iy), value(ix + 1, iy + 1), value(ix, iy + 1)];
                let code = f.iter().enumerate().fold(0u8, |c, (k, &v)| c | (u8::from(v < 0.0) << k));
                let (bottom, right, top, left) = ((ix, iy, 0), (ix + 1, iy, 1), (ix, iy + 1, 0), (ix, iy, 1));
                let centre_inside = (f[0] + f[1] + f[2] + f[3]) / 4.0 < 0.0;
                match code {
                    1 | 14 => segments.push((left, bottom)),
                    2 | 13 => segments.push((bottom, right)),
                    3 | 12 => segments.push((left, right)),
                    4 | 11 => segments.push((right, top)),
                    6 | 9 => segments.push((bottom, top)),
                    7 | 8 => segments.push((left, top)),
                    5 | 10 => {
                        // Saddle: the centre decides which pair of opposite corners connects.
                        if (code == 5) == centre_inside {
                            segments.push((bottom, right));
                            segments.push((top, left));
                        } else {
                            segments.push((left, bottom));
                            segments.push((right, top));
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut adjacency: BTreeMap<Key, Vec<usize>> = BTreeMap::new();
        for (k, &(a, b)) in segments.iter().enumerate() {
            adjacency.entry(a).or_default().push(k);
            adjacency.entry(b).or_default().push(k);
        }
        let mut used = vec![false; segments.len()];
        let mut chains: Vec<Vec<V2>> = Vec::new();
        let walk = |start: Key, used: &mut Vec<bool>| -> Vec<V2> {
            let mut chain = vec![crossing(start)];
            let mut key = start;
            while let Some(&k) = adjacency[&key].iter().find(|&&k| !used[k]) {
                used[k] = true;
                key = if segments[k].0 == key { segments[k].1 } else { segments[k].0 };
                chain.push(crossing(key));
            }
            chain
        };
        let ends: Vec<Key> = adjacency.iter().filter(|(_, s)| s.len() == 1).map(|(&k, _)| k).collect();
        for key in ends {
            if !used[adjacency[&key][0]] {
                chains.push(walk(key, &mut used));
            }
        }
        for k in 0..segments.len() {
            if !used[k] {
                chains.push(walk(segments[k].0, &mut used));
            }
        }
        chains
    }

    fn curb_piece(&self, a: V2, b: V2, out: &mut Vec<Solid>) {
        let length = det::len(det::sub(b, a));
        if length < 1e-4 {
            return;
        }
        let mid = det::scale(det::add(a, b), 0.5);
        let (fa, fb, fm) = (self.face_at(a), self.face_at(b), self.face_at(mid));
        let low = fa.min(fb).min(fm);
        let high = fa.max(fb).max(fm);
        if high < FACE_MIN_M {
            return;
        }
        // Follow a face that changes height (a curb cut's flare) in short pieces; a piece that
        // ends below the threshold still gets its box, so no face of FACE_MIN_M goes uncovered.
        let curved = (fm - f64::midpoint(fa, fb)).abs() > 0.005;
        if (curved || low < FACE_MIN_M) && length > 0.3 {
            self.curb_piece(a, mid, out);
            self.curb_piece(mid, b, out);
            return;
        }
        let dir = det::norm_or(det::sub(b, a), [1.0, 0.0]);
        let left = det::perp(dir);
        let side = if self.field(det::add(mid, det::scale(left, 0.05))).f
            > self.field(det::sub(mid, det::scale(left, 0.05))).f
        {
            1.0
        } else {
            -1.0
        };
        // Overlap neighbouring pieces by 1 cm so an angle between them leaves no slot.
        let (a2, b2) = (det::sub(a, det::scale(dir, 0.01)), det::add(b, det::scale(dir, 0.01)));
        let za = fields::terrain(self.seed, a) + fa;
        let zb = fields::terrain(self.seed, b) + fb;
        let id = hash(self.seed, SALT_CURB, self.key.packed(), out.len() as i64);
        out.push(Solid::slab(
            id,
            SolidKind::Curb,
            [a2[0], a2[1], za],
            [b2[0], b2[1], zb],
            CURB_DEPTH_M,
            side,
            high + 0.15,
        ));
    }

    /// Stable digest of everything the chunk produces, including heights and materials on a
    /// 0.5 m grid. Equal digests on two machines mean identical worlds there.
    pub fn digest(&self) -> u64 {
        let mut d = Digest::new(b"euc-world-chunk-v1\0");
        d.int(self.key.packed());
        for e in &self.edges {
            d.int(e.id.axis as i64);
            d.int(e.id.i);
            d.int(e.id.j);
        }
        for f in &self.features {
            d.bytes(&f.id.to_le_bytes());
            for v in [f.bounds.min[0], f.bounds.min[1], f.bounds.max[0], f.bounds.max[1]] {
                d.float(v);
            }
        }
        for p in &self.parcels {
            d.bytes(&p.id.to_le_bytes());
            for v in [p.origin[0], p.origin[1], p.u[0], p.u[1], p.width_m, p.depth_m, p.ground_min_m] {
                d.float(v);
            }
        }
        for p in &self.props {
            d.bytes(&p.id.to_le_bytes());
            d.int(p.kind as i64);
            for v in p.position {
                d.float(v);
            }
        }
        for s in &self.solids {
            d.bytes(&s.id.to_le_bytes());
            for v in s.centre.iter().chain(&s.half).chain(s.axes.iter().flatten()) {
                d.float(*v);
            }
        }
        for iy in 0..=32 {
            for ix in 0..=32 {
                let p = [self.rect.min[0] + f64::from(ix) * 0.5, self.rect.min[1] + f64::from(iy) * 0.5];
                let s = self.sample(p);
                d.float(s.height_m);
                d.bytes(&[s.surface.material.code(), s.zone as u8, u8::from(s.surface.wet)]);
            }
        }
        d.0
    }
}

/// Collision box of a prop.
fn prop_solid(prop: &Prop) -> Solid {
    let [hu, hv, height] = prop.kind.size();
    let v = det::perp(prop.facing);
    Solid::upright(
        prop.id,
        SolidKind::Prop(prop.kind),
        [prop.position[0], prop.position[1]],
        prop.facing,
        v,
        [-hu, -hv],
        [hu, hv],
        [prop.position[2] - 0.2, prop.position[2] + height],
    )
}

/// `x mod period` in `[0, period)`.
fn fraction(x: f64, period: f64) -> f64 {
    x - period * (x / period).floor()
}

/// A groove `width` wide and `depth` deep every `spacing` metres of arc length.
fn joint(s: f64, spacing: f64, width: f64, depth: f64) -> f64 {
    if fraction(s, spacing) < width { -depth } else { 0.0 }
}

/// Cobblestone setts, 14 cm, in courses offset by half a stone: domes with sunken joints.
fn cobbles(p: V2) -> f64 {
    const SIZE: f64 = 0.14;
    let row = (p[1] / SIZE).floor();
    let shift = if (row as i64) & 1 == 0 { 0.0 } else { 0.5 };
    let gx = p[0] / SIZE + shift;
    let u = gx - gx.floor() - 0.5;
    let v = p[1] / SIZE - row - 0.5;
    0.004 * (1.0 - 4.0 * (u * u + v * v)) - 0.002
}

/// Concrete pavers, 20 × 10 cm in running bond, with 2 mm joints.
fn pavers(p: V2) -> f64 {
    let row = (p[1] / 0.1).floor();
    let shift = if (row as i64) & 1 == 0 { 0.0 } else { 0.5 };
    let u = fraction(p[0] / 0.2 + shift, 1.0);
    let v = p[1] / 0.1 - row;
    let edge = (u * 0.2).min((1.0 - u) * 0.2).min(v * 0.1).min((1.0 - v) * 0.1);
    if edge < 0.004 { -0.002 } else { 0.0 }
}

/// The street whose paving covers a point: the nearer one, except where two carriageways
/// overlap (an intersection), where an arterial wins over a local street, asphalt over other
/// pavings, then the lower edge identity.
fn paving_edge<'a>(edges: &'a [Edge], hit: &RoadHit) -> Option<&'a Edge> {
    let near = &edges[hit.near?.edge];
    let Some(other) = hit.near_other.filter(|_| hit.other_group() < 0.0) else { return Some(near) };
    let other = &edges[other.edge];
    let rank = |e: &Edge| (e.class != StreetClass::Arterial, e.material != Material::Asphalt, e.id);
    Some(if rank(other) < rank(near) { other } else { near })
}

/// Road markings: arterial double centre line, dashed lane lines and edge lines; local dashed
/// centre line. None on cobblestones or inside intersections.
fn marking(edge: &Edge, hit: &RoadHit) -> bool {
    let Some(near) = hit.near else { return false };
    if hit.f >= 0.0 || hit.other_group() < 1.0 || edge.material == Material::Cobblestones {
        return false;
    }
    let d = near.across.abs();
    match edge.class {
        StreetClass::Arterial => {
            (0.05..=0.15).contains(&d)
                || ((d - 3.5).abs() <= 0.06 && fraction(near.s, 12.0) < 4.0)
                || (0.35..=0.45).contains(&-hit.f)
        }
        StreetClass::Local => d <= 0.06 && fraction(near.s, 9.0) < 3.0,
    }
}

fn lot_material(seed: u64, p: V2) -> Material {
    if det::value_noise(seed, SALT_LOT[0], p, 7.0) > 0.55 {
        Material::Dirt
    } else if det::value_noise(seed, SALT_LOT[1], p, 5.0) > 0.7 {
        Material::Gravel
    } else {
        Material::Grass
    }
}

/// Douglas–Peucker simplification with tolerance `tol`, keeping both ends.
fn simplify(points: &[V2], tol: f64) -> Vec<V2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let mut worst = (0.0, a);
        for k in a + 1..b {
            let d = det::segment_distance(points[k], points[a], points[b]).0;
            if d > worst.0 {
                worst = (d, k);
            }
        }
        if worst.0 > tol {
            keep[worst.1] = true;
            stack.push((a, worst.1));
            stack.push((worst.1, b));
        }
    }
    points.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}
