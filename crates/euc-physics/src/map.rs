//! Finite generated terrain for free ride. Scenario [`crate::Terrain`] remains a separate format.
//!
//! The stored millimetre grid is authoritative for collision, rendering, and recording.
//! Rows increase along world +y; columns increase along world +x. Every cell uses the
//! `(0, 0)` to `(1, 1)` diagonal used by MuJoCo 3.9.0. Inline MJCF elevation rows must
//! be reversed because MuJoCo's XML reader reverses them on import.

use std::fmt::{self, Write};
use std::marker::PhantomData;

use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

/// Recording and transport identifier for this finite surface contract.
pub const MAP_MODEL_ID: &str = "generated_heightmap_v1";
/// Supported generated specification schema.
pub const MAP_SCHEMA: u32 = 1;
/// Generator algorithm revision. Change it when generation semantics change.
pub const GENERATOR_REVISION: u32 = 1;
/// Samples along each horizontal axis.
pub const MAP_SAMPLES: u16 = 129;
/// Distance between neighbouring samples, metres.
pub const MAP_SPACING_M: f64 = 0.25;
/// World x/y of the first sample, metres.
pub const MAP_ORIGIN_XY_M: [f64; 2] = [-16.0, -16.0];
/// Maximum absolute stored height, millimetres.
pub const MAX_HEIGHT_MM: i16 = 12_000;
/// Maximum number of explicit features.
pub const MAX_MAP_FEATURES: usize = 32;
/// Maximum compact JSON size for a map and its specification, bytes.
pub const MAX_MAP_METADATA_BYTES: usize = 128 * 1024;
/// Maximum magnitude of the base grade, percent.
pub const MAX_BASE_GRADE_PCT: f64 = 20.0;
/// Maximum magnitude of either stored triangle's gradient, percent.
pub const MAX_LOCAL_GRADE_PCT: f64 = 150.0;
/// Maximum feature height or depth, metres.
pub const MAX_FEATURE_SIZE_M: f64 = 0.30;
/// Minimum feature support width on each axis, metres.
pub const MIN_FEATURE_WIDTH_M: f64 = 1.0;
/// Radius of the level spawn patch, metres.
pub const SPAWN_LEVEL_RADIUS_M: f64 = 1.0;
/// Radius reserved for the spawn patch and base-grade transition, metres.
pub const SPAWN_ZONE_RADIUS_M: f64 = 2.0;
/// Edge warning distance for the host and view, metres.
pub const MAP_EDGE_WARNING_M: f64 = 2.0;
/// Inner boundary distance at which the host must pause, metres.
pub const MAP_EDGE_PAUSE_M: f64 = 1.0;

const SAMPLE_COUNT: usize = MAP_SAMPLES as usize * MAP_SAMPLES as usize;
const MAP_MAX_M: f64 = 16.0;
const HFIELD_NAME: &str = "generated_map";
const HFIELD_BASE_M: f64 = 0.5;
const CONSTANT_HEIGHT_RANGE_M: f64 = 1e-6;

/// One immutable map revision. Call [`Self::validate`] after deserialization or mutation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeightMap {
    /// Run-local map revision, assigned by the integration owner at Apply/reset.
    pub revision: u32,
    /// Column count, initially exactly 129.
    pub nx: u16,
    /// Row count, initially exactly 129.
    pub ny: u16,
    /// Grid spacing along both horizontal axes, metres.
    pub spacing_m: f64,
    /// World position of sample `(0, 0)`, metres.
    pub origin_xy_m: [f64; 2],
    /// Signed heights above the map datum, indexed by `iy * nx + ix`.
    #[serde(deserialize_with = "deserialize_heights")]
    pub heights_mm: Vec<i16>,
}

/// Height and upward unit normal on one triangle of the stored collision surface.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundSample {
    /// Surface height in world coordinates, metres.
    pub height_m: f64,
    /// Upward unit normal in world x/y/z coordinates.
    pub normal_world: [f64; 3],
}

/// A strict feature kind. Size is positive; this kind supplies its sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapFeatureKind {
    /// A separable raised-cosine bump.
    Bump,
    /// A flat-bottom depression with a one-cell cosine transition rim.
    Pothole,
}

/// An axis-aligned local feature with full support widths, metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapFeature {
    /// Bump or pothole.
    pub kind: MapFeatureKind,
    /// World x/y of the support centre, metres.
    pub centre_xy_m: [f64; 2],
    /// Full support width along world x/y, metres.
    pub width_xy_m: [f64; 2],
    /// Positive bump height or pothole depth, metres.
    pub size_m: f64,
}

/// Explicit, versioned input to the native generator. No random noise is implicit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedMapSpec {
    /// Specification schema, currently 1.
    pub schema: u32,
    /// Generator revision, currently 1.
    pub generator_revision: u32,
    /// Seed used by generated preset placement; explicit features stay unchanged.
    pub seed: u32,
    /// Base grade along world x/y, percent.
    pub grade_pct: [f64; 2],
    /// Features in deterministic summation order.
    #[serde(deserialize_with = "deserialize_features")]
    pub features: Vec<MapFeature>,
}

/// Optional format adapter. `Setup` retains `Terrain` and a separate optional map.
#[derive(Debug, Clone, PartialEq)]
pub enum Ground {
    /// Existing scenario profile, including its infinite-flat behavior.
    Profile(crate::Terrain),
    /// Finite free ride map, including when flat.
    Map(HeightMap),
}

impl Ground {
    /// Validate the selected format.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Profile(terrain) => terrain.validate(),
            Self::Map(map) => map.validate(),
        }
    }

    /// MJCF asset block, absent only for an existing flat scenario profile.
    pub fn asset(&self) -> Result<Option<String>, String> {
        self.validate()?;
        match self {
            Self::Profile(terrain) => Ok(terrain.asset()),
            Self::Map(map) => map.asset().map(Some),
        }
    }

    /// MJCF ground geom for the selected format.
    pub fn ground(&self) -> Result<String, String> {
        self.validate()?;
        match self {
            Self::Profile(terrain) => Ok(terrain.ground()),
            Self::Map(map) => map.ground(),
        }
    }
}

impl Default for GeneratedMapSpec {
    fn default() -> Self {
        Self::flat(0)
    }
}

impl GeneratedMapSpec {
    /// A finite flat preset with an explicit seed and no hidden features.
    pub const fn flat(seed: u32) -> Self {
        Self {
            schema: MAP_SCHEMA,
            generator_revision: GENERATOR_REVISION,
            seed,
            grade_pct: [0.0, 0.0],
            features: Vec::new(),
        }
    }

    /// A gentle +5% world-x grade, with the protected spawn patch.
    pub fn gentle_slope(seed: u32) -> Self {
        Self { grade_pct: [5.0, 0.0], ..Self::flat(seed) }
    }

    /// A flat preset with deterministic, separated bumps and potholes.
    ///
    /// `SplitMix64` selects centres on a 4 m lattice outside the spawn zone. Features
    /// have 2 m support widths and 50–100 mm sizes. The returned explicit list is
    /// suitable for display and editing; generation itself adds no hidden features.
    pub fn seeded_features(seed: u32, count: usize) -> Result<Self, String> {
        if count > MAX_MAP_FEATURES {
            return Err(format!("map preset: feature count must be at most {MAX_MAP_FEATURES}"));
        }
        let mut centres = Vec::with_capacity(48);
        for y in -3..=3 {
            for x in -3..=3 {
                if x != 0 || y != 0 {
                    centres.push([f64::from(x) * 4.0, f64::from(y) * 4.0]);
                }
            }
        }
        let mut state = u64::from(seed);
        let mut spec = Self::flat(seed);
        for _ in 0..count {
            let index = (splitmix64(&mut state) % centres.len() as u64) as usize;
            let centre_xy_m = centres.swap_remove(index);
            let choice = splitmix64(&mut state);
            spec.features.push(MapFeature {
                kind: if choice & 1 == 0 { MapFeatureKind::Bump } else { MapFeatureKind::Pothole },
                centre_xy_m,
                width_xy_m: [2.0, 2.0],
                size_m: (50 + (choice >> 1) % 51) as f64 / 1000.0,
            });
        }
        spec.validate()?;
        Ok(spec)
    }

    /// Validate versions, dimensions, base grade, and feature supports before grid allocation.
    /// Final combined heights and triangle grades are checked by [`Self::generate`].
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MAP_SCHEMA || self.generator_revision != GENERATOR_REVISION {
            return Err(format!("map: supported schema is {MAP_SCHEMA}, generator_revision is {GENERATOR_REVISION}"));
        }
        if !self.grade_pct.iter().all(|v| v.is_finite())
            || self.grade_pct[0].hypot(self.grade_pct[1]) > MAX_BASE_GRADE_PCT
        {
            return Err(format!("map: combined base grade must be finite and at most {MAX_BASE_GRADE_PCT}%"));
        }
        if self.features.len() > MAX_MAP_FEATURES {
            return Err(format!("map: feature count must be at most {MAX_MAP_FEATURES}"));
        }
        for (index, feature) in self.features.iter().enumerate() {
            feature.validate().map_err(|error| format!("map feature {index}: {error}"))?;
        }
        Ok(())
    }

    /// Generate and validate one authoritative grid. Quantize once, after summation.
    pub fn generate(&self, revision: u32) -> Result<HeightMap, String> {
        self.validate()?;
        let mut heights_mm = Vec::with_capacity(SAMPLE_COUNT);
        for iy in 0..MAP_SAMPLES {
            let y = MAP_ORIGIN_XY_M[1] + f64::from(iy) * MAP_SPACING_M;
            for ix in 0..MAP_SAMPLES {
                let x = MAP_ORIGIN_XY_M[0] + f64::from(ix) * MAP_SPACING_M;
                let radius = x.hypot(y);
                let blend = if protected_spawn_vertex(x, y) {
                    0.0
                } else if radius >= SPAWN_ZONE_RADIUS_M {
                    1.0
                } else {
                    let t = (radius - SPAWN_LEVEL_RADIUS_M) / (SPAWN_ZONE_RADIUS_M - SPAWN_LEVEL_RADIUS_M);
                    (1.0 - (std::f64::consts::PI * t).cos()) * 0.5
                };
                let mut height = (self.grade_pct[0] * x + self.grade_pct[1] * y) / 100.0 * blend;
                for feature in &self.features {
                    height += feature.height(x, y);
                }
                let mm = height * 1000.0;
                // Check before rounding and casting: never clip a height or saturate an integer.
                if !mm.is_finite() || mm.abs() > f64::from(MAX_HEIGHT_MM) {
                    return Err(format!("map: combined height at sample ({ix}, {iy}) exceeds ±{MAX_HEIGHT_MM} mm"));
                }
                heights_mm.push(mm.round() as i16);
            }
        }
        let map = HeightMap {
            revision,
            nx: MAP_SAMPLES,
            ny: MAP_SAMPLES,
            spacing_m: MAP_SPACING_M,
            origin_xy_m: MAP_ORIGIN_XY_M,
            heights_mm,
        };
        map.validate().map_err(|error| {
            if let Some((ix, iy, _)) = map.excessive_grade() {
                let minimum = [ix, iy].map(|index| -16.0 + index as f64 * MAP_SPACING_M);
                let features: Vec<usize> = self
                    .features
                    .iter()
                    .enumerate()
                    .filter_map(|(index, feature)| {
                        (0..2)
                            .all(|axis| {
                                let half = feature.width_xy_m[axis] * 0.5 + MAP_SPACING_M;
                                feature.centre_xy_m[axis] + half >= minimum[axis]
                                    && feature.centre_xy_m[axis] - half <= minimum[axis] + MAP_SPACING_M
                            })
                            .then_some(index)
                    })
                    .collect();
                format!("{error}; base grade {:?}%; contributing feature indices {features:?}", self.grade_pct)
            } else {
                error
            }
        })?;
        Ok(map)
    }

    /// Stable FNV-1a specification identifier, including seed and feature order.
    /// This is a cache/recording label, not a cryptographic digest.
    pub fn id(&self) -> Result<String, String> {
        self.validate()?;
        let mut hash = Identity::new(b"euc-generated-spec-v1\0");
        hash.bytes(&self.schema.to_le_bytes());
        hash.bytes(&self.generator_revision.to_le_bytes());
        hash.bytes(&self.seed.to_le_bytes());
        for value in self.grade_pct {
            hash.float(value);
        }
        hash.bytes(&(self.features.len() as u32).to_le_bytes());
        for feature in &self.features {
            hash.bytes(&[match feature.kind {
                MapFeatureKind::Bump => 0,
                MapFeatureKind::Pothole => 1,
            }]);
            for value in feature.centre_xy_m.into_iter().chain(feature.width_xy_m).chain([feature.size_m]) {
                hash.float(value);
            }
        }
        Ok(format!("generated-spec-v1-{:016x}", hash.0))
    }
}

impl MapFeature {
    /// Validate a feature's complete support, including its interpolation footprint.
    pub fn validate(&self) -> Result<(), String> {
        if !self.centre_xy_m.iter().chain(&self.width_xy_m).chain([&self.size_m]).all(|v| v.is_finite()) {
            return Err("centre, widths, and size must be finite".into());
        }
        if self.size_m <= 0.0 || self.size_m > MAX_FEATURE_SIZE_M {
            return Err(format!("size_m must be more than 0 and at most {MAX_FEATURE_SIZE_M}"));
        }
        if self.width_xy_m.iter().any(|&width| !(MIN_FEATURE_WIDTH_M..=32.0).contains(&width)) {
            return Err(format!("each support width must be within {MIN_FEATURE_WIDTH_M}..=32 m"));
        }
        let half = self.width_xy_m.map(|width| width * 0.5);
        for (axis, &half_width) in half.iter().enumerate() {
            if self.centre_xy_m[axis] - half_width < MAP_ORIGIN_XY_M[axis]
                || self.centre_xy_m[axis] + half_width > MAP_MAX_M
            {
                return Err("the complete feature support must stay inside the map".into());
            }
        }
        // A triangle can carry a nonzero sample up to one cell beyond analytic support.
        // Reserve that footprint too, so interpolation cannot alter the spawn transition.
        let nearest = [0, 1].map(|axis| (self.centre_xy_m[axis].abs() - half[axis] - MAP_SPACING_M).max(0.0));
        if nearest[0].hypot(nearest[1]) <= SPAWN_ZONE_RADIUS_M {
            return Err("feature support and its one-cell footprint must stay outside the 2 m spawn zone".into());
        }
        Ok(())
    }

    fn height(&self, x: f64, y: f64) -> f64 {
        let distance = [(x - self.centre_xy_m[0]).abs(), (y - self.centre_xy_m[1]).abs()];
        let half = self.width_xy_m.map(|width| width * 0.5);
        if distance[0] >= half[0] || distance[1] >= half[1] {
            return 0.0;
        }
        let weight = [0, 1].map(|axis| match self.kind {
            MapFeatureKind::Bump => f64::midpoint(1.0, (std::f64::consts::PI * distance[axis] / half[axis]).cos()),
            MapFeatureKind::Pothole => {
                let inward = half[axis] - distance[axis];
                if inward >= MAP_SPACING_M {
                    1.0
                } else {
                    (1.0 - (std::f64::consts::PI * inward / MAP_SPACING_M).cos()) * 0.5
                }
            }
        });
        let sign = match self.kind {
            MapFeatureKind::Bump => 1.0,
            MapFeatureKind::Pothole => -1.0,
        };
        sign * self.size_m * weight[0] * weight[1]
    }
}

impl HeightMap {
    /// Check the first-release geometry, height bounds, level spawn cells, and both triangle grades.
    pub fn validate(&self) -> Result<(), String> {
        if !self.has_valid_layout() {
            return Err(
                "map: needs 129 × 129 samples, 0.25 m spacing, origin [-16, -16], and exactly 16641 heights".into()
            );
        }
        let nx = usize::from(self.nx);
        for (index, &mm) in self.heights_mm.iter().enumerate() {
            if !(-MAX_HEIGHT_MM..=MAX_HEIGHT_MM).contains(&mm) {
                return Err(format!("map: height at sample {index} exceeds ±{MAX_HEIGHT_MM} mm"));
            }
            let x = self.origin_xy_m[0] + (index % nx) as f64 * self.spacing_m;
            let y = self.origin_xy_m[1] + (index / nx) as f64 * self.spacing_m;
            if protected_spawn_vertex(x, y) && mm != 0 {
                return Err(format!("map: sample ({}, {}) changes the level spawn patch", index % nx, index / nx));
            }
        }
        if let Some((ix, iy, triangle)) = self.excessive_grade() {
            return Err(format!("map: cell ({ix}, {iy}), triangle {triangle} exceeds {MAX_LOCAL_GRADE_PCT}% grade"));
        }
        Ok(())
    }

    /// Inclusive `[minimum_xy, maximum_xy]` world bounds, or `None` for a malformed layout.
    pub fn bounds_xy_m(&self) -> Option<[[f64; 2]; 2]> {
        self.has_valid_layout().then_some([self.origin_xy_m, [MAP_MAX_M, MAP_MAX_M]])
    }

    /// Sample the stored triangles. Outside and nonfinite coordinates return `None`.
    ///
    /// For cell fractions `(u, v)`, use `(00, 10, 11)` when `v <= u`, otherwise
    /// `(00, 11, 01)`. Normals are triangle normals, never averaged vertex normals.
    /// On the diagonal the first triangle supplies the normal. On an interior grid
    /// line the cell in the positive direction wins; the maximum edges use the last cell.
    /// Validate a map once before use; this hot path only checks layout and touched heights.
    pub fn sample(&self, x_m: f64, y_m: f64) -> Option<GroundSample> {
        let [minimum, maximum] = self.bounds_xy_m()?;
        if !x_m.is_finite()
            || !y_m.is_finite()
            || x_m < minimum[0]
            || x_m > maximum[0]
            || y_m < minimum[1]
            || y_m > maximum[1]
        {
            return None;
        }
        let fx = (x_m - minimum[0]) / self.spacing_m;
        let fy = (y_m - minimum[1]) / self.spacing_m;
        let ix = (fx.floor() as usize).min(usize::from(self.nx) - 2);
        let iy = (fy.floor() as usize).min(usize::from(self.ny) - 2);
        let u = fx - ix as f64;
        let v = fy - iy as f64;
        let heights = self.cell_heights(ix, iy);
        if heights.iter().any(|height| height.abs() > f64::from(MAX_HEIGHT_MM) / 1000.0) {
            return None;
        }
        let [h00, h10, h01, h11] = heights;
        let (height_m, gradient) = if v <= u {
            (h00 + u * (h10 - h00) + v * (h11 - h10), [(h10 - h00) / self.spacing_m, (h11 - h10) / self.spacing_m])
        } else {
            (h00 + u * (h11 - h01) + v * (h01 - h00), [(h11 - h01) / self.spacing_m, (h01 - h00) / self.spacing_m])
        };
        let norm = gradient[0].hypot(gradient[1]).hypot(1.0);
        Some(GroundSample { height_m, normal_world: [-gradient[0] / norm, -gradient[1] / norm, 1.0 / norm] })
    }

    /// Signed minimum distance to the edge for an axis-aligned body footprint.
    /// Negative means outside; nonfinite coordinates or invalid half extents return `None`.
    /// The host compares this with [`MAP_EDGE_WARNING_M`] and [`MAP_EDGE_PAUSE_M`].
    pub fn edge_clearance_m(&self, centre_xy_m: [f64; 2], half_xy_m: [f64; 2]) -> Option<f64> {
        let [minimum, maximum] = self.bounds_xy_m()?;
        if !centre_xy_m.iter().chain(&half_xy_m).all(|v| v.is_finite()) || half_xy_m.iter().any(|&v| v < 0.0) {
            return None;
        }
        let mut clearance = f64::INFINITY;
        for axis in 0..2 {
            clearance = clearance.min(centre_xy_m[axis] - half_xy_m[axis] - minimum[axis]);
            clearance = clearance.min(maximum[axis] - centre_xy_m[axis] - half_xy_m[axis]);
        }
        clearance.is_finite().then_some(clearance)
    }

    /// Stable FNV-1a surface identifier. Run-local revision is deliberately excluded.
    pub fn id(&self) -> Result<String, String> {
        self.validate()?;
        let mut hash = Identity::new(b"euc-heightmap-v1\0");
        hash.bytes(&self.nx.to_le_bytes());
        hash.bytes(&self.ny.to_le_bytes());
        hash.float(self.spacing_m);
        for value in self.origin_xy_m {
            hash.float(value);
        }
        for height in &self.heights_mm {
            hash.bytes(&height.to_le_bytes());
        }
        Ok(format!("heightmap-v1-{:016x}", hash.0))
    }

    /// Validated MJCF `<asset>` block. Inline rows run from maximum y to minimum y.
    pub fn asset(&self) -> Result<String, String> {
        self.validate()?;
        let (low, range) = self.height_range();
        let mut xml = format!(
            "  <asset><hfield name=\"{HFIELD_NAME}\" nrow=\"{}\" ncol=\"{}\" size=\"16 16 {range} {HFIELD_BASE_M}\" elevation=\"",
            self.ny, self.nx
        );
        for iy in (0..usize::from(self.ny)).rev() {
            for ix in 0..usize::from(self.nx) {
                if ix != 0 || iy != usize::from(self.ny) - 1 {
                    xml.push(' ');
                }
                let height = f64::from(self.heights_mm[iy * usize::from(self.nx) + ix]) / 1000.0;
                // Endpoints are exactly 0 and 1; MuJoCo's import normalization is idempotent.
                write!(&mut xml, "{}", (height - low) / range).map_err(|error| error.to_string())?;
            }
        }
        xml.push_str("\"/></asset>");
        Ok(xml)
    }

    /// Validated finite MJCF ground geom. There is no extra plane beyond its edge.
    pub fn ground(&self) -> Result<String, String> {
        self.validate()?;
        let (low, _) = self.height_range();
        Ok(format!(
            "    <geom name=\"floor\" type=\"hfield\" hfield=\"{HFIELD_NAME}\" pos=\"0 0 {low}\" friction=\"0.5 0 0\" condim=\"3\" conaffinity=\"3\"/>"
        ))
    }

    fn has_valid_layout(&self) -> bool {
        // Exact binary values define revision 1; tolerances would change MJCF's world footprint.
        self.nx == MAP_SAMPLES
            && self.ny == MAP_SAMPLES
            && self.spacing_m.to_bits() == MAP_SPACING_M.to_bits()
            && self
                .origin_xy_m
                .iter()
                .zip(MAP_ORIGIN_XY_M)
                .all(|(&value, expected)| value.to_bits() == expected.to_bits())
            && self.heights_mm.len() == SAMPLE_COUNT
    }

    fn cell_heights(&self, ix: usize, iy: usize) -> [f64; 4] {
        let nx = usize::from(self.nx);
        let index = iy * nx + ix;
        [index, index + 1, index + nx, index + nx + 1].map(|i| f64::from(self.heights_mm[i]) / 1000.0)
    }

    fn excessive_grade(&self) -> Option<(usize, usize, usize)> {
        let nx = usize::from(self.nx);
        let limit_mm = MAX_LOCAL_GRADE_PCT / 100.0 * self.spacing_m * 1000.0;
        for iy in 0..usize::from(self.ny) - 1 {
            for ix in 0..nx - 1 {
                let index = iy * nx + ix;
                let [h00, h10, h01, h11] =
                    [index, index + 1, index + nx, index + nx + 1].map(|i| i64::from(self.heights_mm[i]));
                for (triangle, [dx, dy]) in [[h10 - h00, h11 - h10], [h11 - h01, h01 - h00]].into_iter().enumerate() {
                    // Compare integer squared rises. Float subtraction can reject exactly 150%.
                    if (dx * dx + dy * dy) as f64 > limit_mm * limit_mm {
                        return Some((ix, iy, triangle));
                    }
                }
            }
        }
        None
    }

    fn height_range(&self) -> (f64, f64) {
        // Called only after validation, which guarantees a nonempty grid.
        let mut low = self.heights_mm[0];
        let mut high = low;
        for &height in &self.heights_mm {
            low = low.min(height);
            high = high.max(height);
        }
        let range = if high == low { CONSTANT_HEIGHT_RANGE_M } else { (f64::from(high) - f64::from(low)) / 1000.0 };
        (f64::from(low) / 1000.0, range)
    }
}

// Flatten every vertex of every cell touching the level disk. Merely flattening
// vertices inside the disk lets triangles interpolate nonzero heights into it.
fn protected_spawn_vertex(x: f64, y: f64) -> bool {
    (x.abs() - MAP_SPACING_M).max(0.0).hypot((y.abs() - MAP_SPACING_M).max(0.0)) <= SPAWN_LEVEL_RADIUS_M
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

struct Identity(u64);

impl Identity {
    fn new(domain: &[u8]) -> Self {
        let mut identity = Self(0xcbf2_9ce4_8422_2325);
        identity.bytes(domain);
        identity
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn float(&mut self, value: f64) {
        // Normalize negative zero without comparing floating point for equality.
        let bits = value.to_bits();
        self.bytes(&(if bits << 1 == 0 { 0 } else { bits }).to_le_bytes());
    }
}

fn deserialize_heights<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<i16>, D::Error> {
    deserialize_bounded_vec::<D, i16, SAMPLE_COUNT>(deserializer)
}

fn deserialize_features<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<MapFeature>, D::Error> {
    deserialize_bounded_vec::<D, MapFeature, MAX_MAP_FEATURES>(deserializer)
}

fn deserialize_bounded_vec<'de, D, T, const LIMIT: usize>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Bounded<T, const LIMIT: usize>(PhantomData<T>);

    impl<'de, T: Deserialize<'de>, const LIMIT: usize> Visitor<'de> for Bounded<T, LIMIT> {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "an array with at most {LIMIT} entries")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
            if sequence.size_hint().is_some_and(|size| size > LIMIT) {
                return Err(de::Error::custom(format!("map array exceeds {LIMIT} entries")));
            }
            let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(LIMIT));
            while let Some(value) = sequence.next_element()? {
                if values.len() == LIMIT {
                    return Err(de::Error::custom(format!("map array exceeds {LIMIT} entries")));
                }
                values.push(value);
            }
            Ok(values)
        }
    }

    deserializer.deserialize_seq(Bounded::<T, LIMIT>(PhantomData))
}

// Compact JSON for validated values uses at most seven bytes per height (sign,
// five digits, comma), 256 bytes per feature, and 512 bytes for both headers.
// There are no user strings. The host must separately bound raw/pretty JSON input.
const _: () = assert!(SAMPLE_COUNT * 7 + MAX_MAP_FEATURES * 256 + 512 <= MAX_MAP_METADATA_BYTES);
