//! A seeded, infinite, chunked world for free ride: city streets with curbs, sidewalks, curb
//! cuts, crosswalks, speed bumps, manholes, cracks, potholes, rough and wet patches, parks,
//! parking lots, buildings, and raised plazas with stairs and ramps.
//!
//! The world is generated on demand in 16 m chunks, like Minecraft. [`Chunk::generate`] depends
//! only on the [`WorldSpec`] (seed, revision, biome, densities) and the chunk's key: every
//! decision hashes integer coordinates, so any generation order gives identical chunks, and two
//! chunks agree bit for bit on their shared border.
//!
//! The description is resolution-independent: chunks hold analytic primitives (street
//! centrelines, feature shapes, parcels, props) and the sampler evaluates them at any point:
//! - [`World::height_at`] and [`World::sample`] (height, [`Zone`], [`Surface`]) for drawing;
//! - [`World::ground_at`] and [`World::fill_heights`] for a physics height field, with every
//!   face that has a solid moved just behind it;
//! - [`World::solids_in`]: oriented boxes for vertical faces (curbs, steps, plaza walls, ramp
//!   sides, buildings, props), for a physics pool of box geoms.
//!
//! Generation uses only correctly rounded IEEE-754 arithmetic (see `det`), so macOS, Linux and
//! Windows produce the same bits. `docs/models/world.md` describes the design.
//!
//! ```
//! use euc_world::{World, WorldSpec};
//!
//! let world = World::new(WorldSpec::city(7)).unwrap();
//! let spawn = world.spawn();
//! assert_eq!(world.height_at(spawn.position[0], spawn.position[1]), spawn.position[2]);
//! let curbs = world.solids_in(euc_world::Rect::new([-6.0, -6.0], [6.0, 6.0]));
//! assert!(curbs.iter().all(|s| s.bounds().distance([0.0, 0.0]) > 4.0));
//! ```

mod chunk;
mod det;
mod features;
mod fields;
mod parcels;
mod roads;
mod solid;
mod surface;

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

pub use chunk::{CHUNK_M, CURB_DEPTH_M, Chunk, ChunkKey, FACE_MIN_M, Sample, Zone};
pub use det::{Rect, V2};
pub use features::{
    Blob, BumpShape, Feature, FeatureKind, POTHOLE_DEPTH_M, POTHOLE_WIDTH_M, Prop, PropKind, SPAWN_CLEAR_M,
};
pub use fields::{CURB_HEIGHT_M, SIDEWALK_WIDTH_M};
pub use parcels::{PLAZA_STEPS, Parcel, ParcelKind, Plaza, RAMP_MAX_SLOPE, RISER_M, TREAD_M};
pub use roads::{Axis, Edge, EdgeId, PITCH_M, StreetClass};
pub use solid::{Solid, SolidKind};
pub use surface::{Material, Surface};

/// Generator revision. Change it whenever any generated output changes; recordings store it.
pub const GENERATOR_REVISION: u32 = 1;
/// Radius around the spawn that is level to ±1 cm, metres.
pub const SPAWN_LEVEL_RADIUS_M: f64 = 2.0;
/// Chunks kept in a [`World`]'s cache before it starts over.
const CACHE_CHUNKS: usize = 1024;

/// The terrain style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Biome {
    /// City blocks on gentle hills.
    City,
}

/// How often things occur. Area densities count per 1000 m² of the zone they occur in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Densities {
    /// Potholes per 1000 m² of road.
    pub potholes: f64,
    /// Manholes per 1000 m² of road.
    pub manholes: f64,
    /// Cracks per 1000 m² of road and sidewalk.
    pub cracks: f64,
    /// Rough patches per 1000 m² of road.
    pub rough_patches: f64,
    /// Wet patches per 1000 m² of road and sidewalk.
    pub wet_patches: f64,
    /// Fraction of local street edges with a speed bump or hump.
    pub speed_bumps: f64,
    /// Fraction of street ends at intersections with a crosswalk.
    pub crosswalks: f64,
    /// Driveways per 100 m of street side.
    pub driveways: f64,
    /// Weight of plazas among parcels (buildings 0.55, parks 0.15, parking 0.12).
    pub plazas: f64,
    /// Street furniture and trees, as a fraction of the slots that could hold them.
    pub props: f64,
}

impl Default for Densities {
    fn default() -> Self {
        Self {
            potholes: 1.5,
            manholes: 0.6,
            cracks: 12.0,
            rough_patches: 0.8,
            wet_patches: 1.0,
            speed_bumps: 0.35,
            crosswalks: 0.7,
            driveways: 1.5,
            plazas: 0.15,
            props: 0.8,
        }
    }
}

impl Densities {
    fn validate(&self) -> Result<(), String> {
        let fields = [
            ("potholes", self.potholes, 50.0),
            ("manholes", self.manholes, 3.0),
            ("cracks", self.cracks, 100.0),
            ("rough_patches", self.rough_patches, 6.0),
            ("wet_patches", self.wet_patches, 9.0),
            ("speed_bumps", self.speed_bumps, 1.0),
            ("crosswalks", self.crosswalks, 1.0),
            ("driveways", self.driveways, 5.0),
            ("plazas", self.plazas, 2.0),
            ("props", self.props, 1.0),
        ];
        for (name, value, max) in fields {
            if !value.is_finite() || !(0.0..=max).contains(&value) {
                return Err(format!("world density {name} must be within 0..={max}, got {value}"));
            }
        }
        Ok(())
    }
}

/// Everything a world depends on. Recordings store this, never meshes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldSpec {
    /// Seed.
    pub seed: u64,
    /// Generator revision; must equal [`GENERATOR_REVISION`].
    pub revision: u32,
    /// Biome.
    pub biome: Biome,
    /// Feature densities.
    #[serde(default)]
    pub densities: Densities,
}

impl WorldSpec {
    /// A city with default densities.
    pub fn city(seed: u64) -> Self {
        Self { seed, revision: GENERATOR_REVISION, biome: Biome::City, densities: Densities::default() }
    }

    /// Checks the revision and the densities.
    pub fn validate(&self) -> Result<(), String> {
        if self.revision != GENERATOR_REVISION {
            return Err(format!(
                "world revision {} is not this generator's revision {GENERATOR_REVISION}",
                self.revision
            ));
        }
        self.densities.validate()
    }
}

/// Where a run starts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Spawn {
    /// Contact point of the wheel on the ground, metres.
    pub position: [f64; 3],
    /// Unit heading (the spawn street runs along +x).
    pub heading: V2,
}

/// A world: its specification and caches of generated chunks and parcel tiles. Cheap to share
/// across threads; the caches never change what is generated, only how often.
#[derive(Debug)]
pub struct World {
    spec: WorldSpec,
    cache: Mutex<HashMap<ChunkKey, Arc<Chunk>>>,
    tiles: Mutex<HashMap<parcels::TileKey, Arc<Vec<parcels::Kept>>>>,
}

impl World {
    /// A world for `spec`.
    pub fn new(spec: WorldSpec) -> Result<Self, String> {
        spec.validate()?;
        Ok(Self { spec, cache: Mutex::new(HashMap::new()), tiles: Mutex::new(HashMap::new()) })
    }

    fn tile(&self, key: parcels::TileKey) -> Arc<Vec<parcels::Kept>> {
        if let Some(tile) = self.tiles.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return Arc::clone(tile);
        }
        let tile = Arc::new(parcels::tile(self.spec.seed, &self.spec.densities, key));
        let mut tiles = self.tiles.lock().unwrap_or_else(PoisonError::into_inner);
        if tiles.len() >= CACHE_CHUNKS / 16 {
            tiles.clear();
        }
        Arc::clone(tiles.entry(key).or_insert(tile))
    }

    /// The specification.
    pub const fn spec(&self) -> &WorldSpec {
        &self.spec
    }

    /// The chunk at `key`, generated on first use.
    pub fn chunk(&self, key: ChunkKey) -> Arc<Chunk> {
        if let Some(chunk) = self.cache.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return Arc::clone(chunk);
        }
        // Generate outside the lock so other threads can sample and generate meanwhile.
        let chunk = Arc::new(Chunk::generate_with(&self.spec, key, &|tile| self.tile(tile)));
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() >= CACHE_CHUNKS {
            cache.clear();
        }
        Arc::clone(cache.entry(key).or_insert(chunk))
    }

    /// Ground height for drawing, metres.
    pub fn height_at(&self, x: f64, y: f64) -> f64 {
        self.chunk(ChunkKey::of([x, y])).height_at([x, y])
    }

    /// Ground height for physics sampling, with faces moved `inset` metres behind their solids.
    pub fn ground_at(&self, x: f64, y: f64, inset: f64) -> f64 {
        self.chunk(ChunkKey::of([x, y])).ground_at([x, y], inset)
    }

    /// Height, zone and surface.
    pub fn sample(&self, x: f64, y: f64) -> Sample {
        self.chunk(ChunkKey::of([x, y])).sample([x, y])
    }

    /// Surface (material, wet, friction, rolling resistance).
    pub fn surface_at(&self, x: f64, y: f64) -> Surface {
        self.sample(x, y).surface
    }

    /// Fills `out[iy * nx + ix]` with [`Self::ground_at`] at `origin + (ix, iy) · cell`.
    ///
    /// # Panics
    ///
    /// When `out` does not hold exactly `nx × ny` values.
    pub fn fill_heights(&self, origin: V2, cell: f64, nx: usize, ny: usize, inset: f64, out: &mut [f64]) {
        assert_eq!(out.len(), nx * ny, "fill_heights: output length must be nx × ny");
        let mut current: Option<Arc<Chunk>> = None;
        for iy in 0..ny {
            let y = origin[1] + iy as f64 * cell;
            for ix in 0..nx {
                let p = [origin[0] + ix as f64 * cell, y];
                let key = ChunkKey::of(p);
                let chunk = match &current {
                    Some(chunk) if chunk.key() == key => chunk,
                    _ => current.insert(self.chunk(key)),
                };
                out[iy * nx + ix] = chunk.ground_at(p, inset);
            }
        }
    }

    /// Every solid overlapping `rect`, each once, ordered by identity.
    pub fn solids_in(&self, rect: Rect) -> Vec<Solid> {
        let mut found = BTreeMap::new();
        for key in keys_in(rect) {
            for solid in self.chunk(key).solids() {
                if solid.bounds().overlaps(&rect) {
                    found.entry(solid.id).or_insert(*solid);
                }
            }
        }
        found.into_values().collect()
    }

    /// Every prop standing in `rect`.
    pub fn props_in(&self, rect: Rect) -> Vec<Prop> {
        keys_in(rect)
            .flat_map(|key| self.chunk(key).props().to_vec())
            .filter(|p| rect.contains([p.position[0], p.position[1]]))
            .collect()
    }

    /// Bare terrain relief (no streets, curbs or features), metres: for distant scenery.
    pub fn terrain_at(&self, x: f64, y: f64) -> f64 {
        fields::terrain(self.spec.seed, [x, y])
    }

    /// Position of road node `(i, j)` (an intersection, bend or dead end), metres.
    pub fn node(&self, i: i64, j: i64) -> V2 {
        roads::node(self.spec.seed, i, j)
    }

    /// The street edge `id` with its centreline, if it exists. Routes follow these.
    pub fn edge(&self, id: EdgeId) -> Option<Edge> {
        roads::exists(self.spec.seed, id).then(|| roads::build(self.spec.seed, id))
    }

    /// The spawn: the origin, on the right-hand lane of a straight arterial, heading +x. The
    /// ground is level within ±1 cm over [`SPAWN_LEVEL_RADIUS_M`]; no feature or prop lies within
    /// [`SPAWN_CLEAR_M`]; the nearest solid (the curb) is 5.5 m to the right.
    pub fn spawn(&self) -> Spawn {
        Spawn { position: [0.0, 0.0, self.height_at(0.0, 0.0)], heading: [1.0, 0.0] }
    }
}

/// Keys of every chunk overlapping `rect`, row by row.
pub fn keys_in(rect: Rect) -> impl Iterator<Item = ChunkKey> {
    let (a, b) = (ChunkKey::of(rect.min), ChunkKey::of(rect.max));
    (a.cy..=b.cy).flat_map(move |cy| (a.cx..=b.cx).map(move |cx| ChunkKey { cx, cy }))
}
