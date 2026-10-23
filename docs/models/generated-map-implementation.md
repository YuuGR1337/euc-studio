# Generated map implementation

Status: implemented and integrated, 2026-10-16.
Update, 2026-10-23: the city world ([world.md](world.md)) replaced this 32 m map as Free Ride's ground. The studio no
longer offers the test pad; recordings made on it still play back on their saved map. The "Parent integration"
section below records the plan of its date.
Scope: native height-grid generation, collision geometry, view sampling, and recording integration.
Simulation geometry acceptance is recorded below. It does not establish measured terrain or rider behavior.

## Public API

The module uses the existing `serde` dependency.
It adds no dependencies.
The grid, specification, feature, feature kind, and sample types support `Serialize` and `Deserialize`.
They reject unknown fields.
`MapFeatureKind` accepts only `bump` and `pothole`.
`Ground` retains the existing `Terrain` type and provides an optional format adapter.
`Ground` does not add serialization to `Terrain`.

```rust
pub struct HeightMap {
    pub revision: u32,
    pub nx: u16,
    pub ny: u16,
    pub spacing_m: f64,
    pub origin_xy_m: [f64; 2],
    pub heights_mm: Vec<i16>,
}

pub struct GeneratedMapSpec {
    pub schema: u32,
    pub generator_revision: u32,
    pub seed: u32,
    pub grade_pct: [f64; 2],
    pub features: Vec<MapFeature>,
}

pub struct MapFeature {
    pub kind: MapFeatureKind,
    pub centre_xy_m: [f64; 2],
    pub width_xy_m: [f64; 2],
    pub size_m: f64,
}

pub enum MapFeatureKind { Bump, Pothole }

pub struct GroundSample {
    pub height_m: f64,
    pub normal_world: [f64; 3],
}

pub enum Ground { Profile(Terrain), Map(HeightMap) }

impl GeneratedMapSpec {
    pub const fn flat(seed: u32) -> Self;
    pub fn gentle_slope(seed: u32) -> Self;
    pub fn seeded_features(seed: u32, count: usize) -> Result<Self, String>;
    pub fn validate(&self) -> Result<(), String>;
    pub fn generate(&self, revision: u32) -> Result<HeightMap, String>;
    pub fn id(&self) -> Result<String, String>;
}

impl MapFeature {
    pub fn validate(&self) -> Result<(), String>;
}

impl HeightMap {
    pub fn validate(&self) -> Result<(), String>;
    pub fn sample(&self, x_m: f64, y_m: f64) -> Option<GroundSample>;
    pub fn bounds_xy_m(&self) -> Option<[[f64; 2]; 2]>;
    pub fn edge_clearance_m(
        &self, centre_xy_m: [f64; 2], half_xy_m: [f64; 2],
    ) -> Option<f64>;
    pub fn id(&self) -> Result<String, String>;
    pub fn asset(&self) -> Result<String, String>;
    pub fn ground(&self) -> Result<String, String>;
}

impl Ground {
    pub fn validate(&self) -> Result<(), String>;
    pub fn asset(&self) -> Result<Option<String>, String>;
    pub fn ground(&self) -> Result<String, String>;
}
```

`flat(seed)` produces zero grade and no features.
`gentle_slope(seed)` produces a 5% grade along world +x and no features.
`Default` selects `flat(0)`.
`seeded_features(seed, count)` returns an explicit feature list for display and editing.
The seed controls preset placement and feature choices.
`generate` adds no random features to an explicit specification.

`GeneratedMapSpec::validate` checks versions, base grade, and individual feature supports.
`generate` also checks the final combined surface.
Native Apply must use `generate(revision)` before it replaces the previous map.
The caller assigns the map revision at the Apply/reset boundary.

## Limits and data bounds

| Property | Implemented limit |
| --- | --- |
| Schema / generator revision | 1 / 1 |
| `MAP_MODEL_ID` | `generated_heightmap_v1` |
| Grid | Exactly 129 × 129 samples |
| Spacing | Exactly 0.25 m |
| Origin | Exactly [-16 m, -16 m] |
| Bounds | [-16 m, 16 m] on both axes |
| Stored heights | -12000 through 12000 mm |
| Combined base grade | At most 20% |
| Final triangle grade | At most 150% |
| Features | At most 32 |
| Feature size | More than zero through 0.30 m |
| Full support widths | 1 m through 32 m on each axis |
| Level spawn patch | Radius 1 m, height zero |
| Protected spawn zone | Radius 2 m |
| Map metadata budget | 128 KiB |

Validation rejects nonfinite values, unsupported versions, invalid dimensions, and supports outside the map.
Generation rejects excess combined heights before rounding or integer conversion.
It sums the base grade and all features before it rounds each sample once.
Rounding uses the nearest millimetre, with half values away from zero.
Generation never clips output.

The generator checks both stored triangles in each cell.
It compares squared integer height differences to avoid rounding errors at exactly 150% grade.
Grade errors identify the cell, triangle, base grade, and possible feature contributors.
Feature indices start at zero.

Bumps use separate cosine weights along x and y.
Potholes have flat bottoms and a 0.25 m cosine rim on each axis.
Support widths describe the full support, including the rim.
The generator permits overlapping supports when the final height and grade checks pass.

The base grade uses a cosine transition between radii 1 m and 2 m.
All vertices of cells that touch the level patch remain at zero.
This conservative rule keeps triangle interpolation level throughout the patch.
Feature validation also reserves one cell beyond analytic support around the protected 2 m zone.
This rule prevents a feature sample from affecting that zone through interpolation.

Array deserialization stops above 16641 heights or 32 features.
Call `validate` after deserialization and before use.
Public fields permit mutation, so callers must treat accepted maps as immutable.
`sample` checks layout and nearby heights without scanning the complete map.

Compact JSON for the grid and specification fits within 128 KiB.
A compile-time bound reserves seven bytes per height, 256 bytes per feature, and 512 bytes for headers.
The resulting bound is 125191 bytes.
The parent must enforce the actual byte limit on raw input and complete recording metadata.
Whitespace, wrappers, and unrelated recording fields require their own byte checks.

## Surface and MJCF contract

Sample index is `iy * nx + ix`.
Columns increase along world +x.
Rows increase along world +y.
Stored heights use millimetres above the local map datum.

Each cell has corners `00`, `10`, `01`, and `11`.
The diagonal joins `00` to `11`.
For local cell fractions `(u, v)`, the first triangle applies when `v <= u`.
Its vertices are `(00, 10, 11)`.
The second triangle has vertices `(00, 11, 01)`.

```text
v <= u: h = h00 + u*(h10-h00) + v*(h11-h10)
v >  u: h = h00 + u*(h11-h01) + v*(h01-h00)

first gradient  = [(h10-h00)/spacing, (h11-h10)/spacing]
second gradient = [(h11-h01)/spacing, (h01-h00)/spacing]
normal_world    = normalize([-gradient.x, -gradient.y, 1])
```

`sample` uses triangle interpolation and returns the upward triangle normal.
It returns `None` for coordinates outside the bounds or for nonfinite coordinates.
The outer edges remain valid sample positions.
The first triangle supplies the normal on the diagonal.
Interior grid lines select the cell in the positive direction.
Maximum edges select the last cell.
A surface crease has no unique normal at its shared edge.

The pinned MuJoCo 3.9.0 source confirms the diagonal.
The [ray implementation](https://github.com/google-deepmind/mujoco/blob/3.9.0/src/engine/engine_ray.c#L619) uses these two triangles.
The [collision implementation](https://github.com/google-deepmind/mujoco/blob/3.9.0/src/engine/engine_collision_convex.c#L1144) constructs prisms with the same split.
The original conclusion came from source inspection. The acceptance tests below now verify
the same diagonal through MuJoCo and Three.js runtime rays.

`asset` returns a complete `<asset>` block with heightfield name `generated_map`.
It normalizes heights to [0, 1] and uses the actual height range as the vertical scale.
Constant grids use a positive 0.000001 m range and zero elevations.
`ground` returns the finite `floor` heightfield geom at world z equal to the grid minimum.
The heightfield base extends 0.5 m below that minimum.

The [XML reader](https://github.com/google-deepmind/mujoco/blob/3.9.0/src/xml/xml_native_reader.cc#L3304) reverses inline elevation rows.
`asset` therefore emits rows from maximum y to minimum y.
The [heightfield compiler](https://github.com/google-deepmind/mujoco/blob/3.9.0/src/user/user_objects.cc#L4429) normalizes the imported values.
The emitted nonconstant endpoints remain zero and one.
No plane extends beyond the map.

The view must use the stored heights, row order, and triangle split.
The view must apply `(x, y, z) → (x, z, -y)` only at the render boundary.
Playback must use the saved grid.
It must not regenerate a surface from its seed.

## IDs

`GeneratedMapSpec::id()` returns `generated-spec-v1-` plus a 16-digit FNV-1a hexadecimal value.
Its input includes schema, generator revision, seed, grade, and the ordered feature list.
`HeightMap::id()` returns `heightmap-v1-` plus the same digest format.
Its input includes dimensions, spacing, origin, and every quantized height.
It excludes the run-local map revision.
Identical surfaces therefore retain their surface ID across resets.

Both IDs use fixed domain prefixes and little-endian integer bytes.
Floating values use IEEE-754 bits, with negative zero normalized to positive zero.
Feature kind bytes are zero for bump and one for pothole.
These IDs provide cache and recording labels.
They are not cryptographic integrity checks.

## Parent integration

The current integration keeps `Setup.terrain: Terrain` for compatibility.
It adds `Setup.height_map: Option<HeightMap>` with a default value of `None`.
This contract replaces the proposed `Setup.terrain: Ground` change in section 7.
`Ground` remains available as an optional format adapter.

Required exports in `lib.rs` are:

```rust
pub mod map;
pub use map::{GeneratedMapSpec, Ground, GroundSample, HeightMap, MapFeature, MapFeatureKind};
```

These exports and the optional `Setup.height_map` field exist in the shared files at handoff.
The parent owns those shared changes.

The parent owns the following integration work:

1. Validate `height_map` in `Setup::validate` when it contains a map.
2. Select `map.asset()` and `map.ground()` in `mjcf.rs` when `height_map` contains a map.
3. Propagate generation errors before MuJoCo model allocation.
4. Use the existing `Terrain` asset and ground methods when `height_map` is `None`.
5. Add `WheelFiles::live_on_map` with the authoritative map as input.
6. Retain `WheelFiles::live_on` for existing scenario construction.
7. Use `map.sample(0.0, 0.0)` for map spawn height.
8. Add that height to shell and rider spawn positions in MJCF.
9. Send the map once per revision through the map transport path.
10. Save the grid, specification, IDs, and revision in recording context.

`asset()` and `ground()` return `Result`, unlike the existing `Terrain` methods.
The parent must handle those errors at model construction.
The level patch currently makes the map spawn height zero.

`edge_clearance_m` accepts a world x/y centre and nonnegative axis-aligned half extents.
It returns the minimum distance from the complete footprint to the map edge.
Negative values mean the footprint crosses the map edge.
Invalid values or arithmetic overflow return `None`.

The parent must check wheel and detached rider extents.
Use `MAP_EDGE_WARNING_M = 2.0` for the visible warning band.
Pause at the first sample with clearance at most `MAP_EDGE_PAUSE_M = 1.0`.
The helper does not change body positions or pause the host itself.

## Acceptance evidence and limits

`cargo test -p euc-physics --test generated_map` passes four tests:

- A MuJoCo downward ray checks both triangles in all 16,384 cells of an asymmetric map.
  The map has two-axis grade, a bump, and a pothole. Heights and normals match `HeightMap::sample`
  within 0.001 m and 0.001 per normal component.
- A flat map has no collision surface beyond any of its four edges.
- A descending map stays below zero; no hidden plane covers its negative heights.
- A freely falling box settles on the generated pothole bottom within 5 mm.

`pnpm exec vitest run src/workspaces/ride/mapGeometry.test.ts` passes two view tests:

- Three.js rays intersect the actual generated mesh at independently calculated heights on both
  triangles of a non-planar cell. Upward normals, the physics-to-view rotation, negative heights,
  and the finite edge agree with the sampler.
- Every metre-grid endpoint lies on the authoritative surface.

The renderer uses `mapGeometry.ts` for this tested geometry and disposes it when the revision changes.
These tests establish geometry agreement for the specified cases. Sustained native riding, input
lifecycle checks, and measured physical calibration remain separate acceptance requirements.
The map has one height per horizontal point.
It cannot represent overhangs, stacked roads, vertical walls, or loose ground.
The 0.25 m spacing cannot represent narrow holes or sharp curb detail.
