//! The streaming world as MuJoCo ground (`docs/models/world.md`, "Physics plan").
//!
//! A height-field window of 385 × 385 samples at 1/32 m (a 12 m square) sits in a mocap body and
//! follows the wheel by whole cells; a pool of mocap boxes carries every vertical face near the
//! wheel (curbs, steps, plaza walls, ramp sides, buildings, props). Both move only between steps,
//! on a fixed cadence and on state alone, so a run repeats exactly. A worker thread keeps the
//! world's chunk cache warm ahead of the wheel; the cache never changes a value, only how soon
//! it is ready.
//!
//! The physics frame is the world frame moved so that the start lies at the origin heading +x:
//! `world = start + R(heading) · physics`, `z_world = z_physics + origin_z`.

// Moving the window and the boxes writes MuJoCo's model between steps (height-field samples,
// box sizes and bounds, tire friction), which mujoco-rs only offers through `model_mut`.
#![allow(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::sync::{Arc, mpsc};

use euc_world::{Material, Rect, Solid, Surface, World, WorldSpec};
use mujoco_rs::prelude::*;

use crate::PhysicsError;

/// Samples along each side of the window.
pub const WINDOW_SAMPLES: usize = 385;
/// Window cell, metres (a power of two, so every sample position is exact).
pub const WINDOW_CELL_M: f64 = 1.0 / 32.0;
/// Half the window's side, metres.
pub const WINDOW_HALF_M: f64 = (WINDOW_SAMPLES - 1) as f64 / 2.0 * WINDOW_CELL_M;
/// The window's elevation range, metres (a power of two, so stored heights stay exact).
pub const WINDOW_RANGE_M: f64 = 8.0;
/// How far the wheel may drift from the window's centre before it recentres, metres.
pub const RECENTRE_M: f64 = 1.0;
/// Faces with a solid step up this far behind their solid's face in the height field, metres.
pub const FACE_INSET_M: f64 = 1.5 * WINDOW_CELL_M;
/// Boxes in the pool.
pub const POOL_SIZE: usize = 128;
/// Solids within this distance of the wheel go into the pool, metres.
pub const POOL_REACH_M: f64 = 6.0;
/// Physics steps between world ticks (recentre check, pool refresh, surface under the wheel).
pub const TICK_STEPS: u64 = 20;

/// Height quantum, metres: window heights are multiples of it, so `f32` data is exact.
const QUANTUM_M: f64 = 1.0 / 65536.0;
const WINDOW_BASE_M: f64 = 0.5;
const PARKED_Z_M: f64 = -1000.0;
const HALF_CELLS: i64 = (WINDOW_SAMPLES as i64 - 1) / 2;
/// Name of the window's mocap body, hfield and geom.
pub(crate) const WINDOW: &str = "world_window";
/// Prefix of the pool's mocap bodies.
pub(crate) const BOX: &str = "world_box_";
/// Contact settings of every world geom: today's floor, without friction of its own, so the
/// tire's friction (scaled per surface) and each body's own friction decide.
const GROUND_CONTACT: &str = r#"friction="0 0 0" condim="3" conaffinity="3""#;

/// Where a world run starts: the wheel's ground point and heading in world coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldStart {
    /// Ground point under the wheel, metres.
    pub xy_m: [f64; 2],
    /// Heading, radians, left positive from world +x.
    pub heading_rad: f64,
}

/// A streaming world as the ground of one run.
#[derive(Clone)]
pub struct WorldGround {
    spec: WorldSpec,
    start: WorldStart,
    world: Arc<World>,
    origin_z: f64,
    rotation: [f64; 2],
}

impl fmt::Debug for WorldGround {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorldGround").field("spec", &self.spec).field("start", &self.start).finish_non_exhaustive()
    }
}

impl PartialEq for WorldGround {
    fn eq(&self, other: &Self) -> bool {
        self.spec == other.spec && self.start == other.start
    }
}

impl WorldGround {
    /// The world of `spec`, starting at its spawn.
    pub fn at_spawn(spec: WorldSpec) -> Result<Self, String> {
        let world = World::new(spec)?;
        let spawn = world.spawn();
        let heading = spawn.heading[1].atan2(spawn.heading[0]);
        Self::with_world(
            Arc::new(world),
            WorldStart { xy_m: [spawn.position[0], spawn.position[1]], heading_rad: heading },
        )
    }

    /// The world of `spec`, starting at `start`.
    pub fn new(spec: WorldSpec, start: WorldStart) -> Result<Self, String> {
        Self::with_world(Arc::new(World::new(spec)?), start)
    }

    /// A run on an existing (cached) world.
    pub fn with_world(world: Arc<World>, start: WorldStart) -> Result<Self, String> {
        if !start.xy_m.iter().all(|v| v.is_finite()) || !start.heading_rad.is_finite() {
            return Err("world start must be finite".into());
        }
        let rotation =
            if start.heading_rad == 0.0 { [1.0, 0.0] } else { [start.heading_rad.cos(), start.heading_rad.sin()] };
        let origin_z = world.height_at(start.xy_m[0], start.xy_m[1]);
        Ok(Self { spec: *world.spec(), start, world, origin_z, rotation })
    }

    /// The world's specification.
    pub const fn spec(&self) -> &WorldSpec {
        &self.spec
    }

    /// The start.
    pub const fn start(&self) -> WorldStart {
        self.start
    }

    /// The world (shared, with its caches).
    pub fn world(&self) -> &Arc<World> {
        &self.world
    }

    /// World height of the physics frame's z = 0, metres.
    pub const fn origin_z(&self) -> f64 {
        self.origin_z
    }

    /// A physics-frame point in world coordinates.
    pub fn to_world(&self, p: [f64; 2]) -> [f64; 2] {
        let [c, s] = self.rotation;
        [self.start.xy_m[0] + c * p[0] - s * p[1], self.start.xy_m[1] + s * p[0] + c * p[1]]
    }

    /// A world point in the physics frame.
    pub fn to_physics(&self, p: [f64; 2]) -> [f64; 2] {
        let [c, s] = self.rotation;
        let d = [p[0] - self.start.xy_m[0], p[1] - self.start.xy_m[1]];
        [c * d[0] + s * d[1], -s * d[0] + c * d[1]]
    }

    /// Drawn ground height at a physics-frame point, in the physics frame, metres.
    pub fn height_m(&self, p: [f64; 2]) -> f64 {
        let w = self.to_world(p);
        self.world.height_at(w[0], w[1]) - self.origin_z
    }

    /// The surface at a physics-frame point.
    pub fn surface(&self, p: [f64; 2]) -> Surface {
        let w = self.to_world(p);
        self.world.surface_at(w[0], w[1])
    }

    /// MJCF `<asset>` line: the window's height field, filled at run time.
    pub(crate) fn asset() -> String {
        format!(
            r#"  <asset><hfield name="{WINDOW}" nrow="{WINDOW_SAMPLES}" ncol="{WINDOW_SAMPLES}" size="{WINDOW_HALF_M} {WINDOW_HALF_M} {WINDOW_RANGE_M} {WINDOW_BASE_M}"/></asset>"#
        )
    }

    /// MJCF world-body lines: the window's mocap body and the parked box pool.
    pub(crate) fn bodies() -> String {
        let mut xml = format!(
            r#"    <body name="{WINDOW}" mocap="true"><geom name="floor" type="hfield" hfield="{WINDOW}" {GROUND_CONTACT}/></body>"#
        );
        for k in 0..POOL_SIZE {
            // Writing to a String cannot fail.
            let _ = write!(
                xml,
                "\n    <body name=\"{BOX}{k}\" mocap=\"true\" pos=\"0 0 {PARKED_Z_M}\"><geom name=\"{BOX}{k}\" type=\"box\" size=\"0.1 0.1 0.1\" {GROUND_CONTACT}/></body>"
            );
        }
        xml
    }
}

/// One pool slot.
#[derive(Debug, Clone, Copy)]
struct Slot {
    mocap: usize,
    geom: usize,
    solid: Option<u64>,
}

/// The window, the pool and the tire friction of one run, inside [`crate::Physics`].
pub(crate) struct Window {
    ground: WorldGround,
    hfield_adr: usize,
    mocap: usize,
    centre: [i64; 2],
    z0: f64,
    heights: Vec<f64>,
    slots: Vec<Slot>,
    tires: Vec<usize>,
    /// The tire's own contact friction on asphalt: sliding, spin, rolling (length).
    tire_friction: [f64; 3],
    steps: u64,
    surface: Surface,
    prefetch: Option<mpsc::Sender<[f64; 4]>>,
    /// Recentres so far, and the cost of the last one on the step thread, seconds.
    pub recentres: u64,
    pub last_recentre_s: f64,
}

impl Window {
    /// Resolves the model's world bodies and fills the window and pool around the origin.
    pub(crate) fn new(
        ground: WorldGround,
        data: &mut MjData<Box<MjModel>>,
        tire_friction: [f64; 3],
    ) -> Result<Self, PhysicsError> {
        let model = data.model();
        let missing = |what: &str| PhysicsError::Model(format!("no {what} in the world model"));
        let hfield = model.name_to_id(MjtObj::mjOBJ_HFIELD, WINDOW).ok_or_else(|| missing("window hfield"))?;
        let body = model.name_to_id(MjtObj::mjOBJ_BODY, WINDOW).ok_or_else(|| missing("window body"))?;
        let mocap = usize::try_from(model.body_mocapid()[body]).map_err(|_| missing("window mocap"))?;
        let mut slots = Vec::with_capacity(POOL_SIZE);
        for k in 0..POOL_SIZE {
            let name = format!("{BOX}{k}");
            let body = model.name_to_id(MjtObj::mjOBJ_BODY, &name).ok_or_else(|| missing(&name))?;
            let geom = model.name_to_id(MjtObj::mjOBJ_GEOM, &name).ok_or_else(|| missing(&name))?;
            let mocap = usize::try_from(model.body_mocapid()[body]).map_err(|_| missing(&name))?;
            slots.push(Slot { mocap, geom, solid: None });
        }
        let tires: Vec<usize> = (0..model.geom_friction().len())
            .filter(|&g| model.id_to_name(MjtObj::mjOBJ_GEOM, g).is_some_and(|n| n == "tire" || n.starts_with("tire_")))
            .collect();
        if tires.is_empty() {
            return Err(missing("tire geom"));
        }
        let (sender, receiver) = mpsc::channel::<[f64; 4]>();
        let world = Arc::clone(ground.world());
        let prefetch = std::thread::Builder::new()
            .name("world-prefetch".into())
            .spawn(move || prefetch(&world, &receiver))
            .ok()
            .map(|_| sender);
        let mut window = Self {
            hfield_adr: model.hfield_adr()[hfield] as usize,
            mocap,
            centre: [0, 0],
            z0: 0.0,
            heights: Vec::new(),
            slots,
            tires,
            tire_friction,
            steps: 0,
            surface: Surface::of(Material::Asphalt, false),
            prefetch,
            recentres: 0,
            last_recentre_s: 0.0,
            ground,
        };
        window.heights = window.sample(&Self::lattice([0, 0], |_, _| true));
        window.upload(data);
        window.refresh(data, [0.0, 0.0]);
        Ok(window)
    }

    /// The ground this window shows.
    pub(crate) const fn ground(&self) -> &WorldGround {
        &self.ground
    }

    /// The surface under the wheel as of the last tick.
    pub(crate) const fn surface(&self) -> Surface {
        self.surface
    }

    /// Runs before a step: every [`TICK_STEPS`] steps, recentres if the wheel drifted, refreshes
    /// the pool and the tire friction. `position` and `velocity` are the shell's, physics frame.
    pub(crate) fn tick(&mut self, data: &mut MjData<Box<MjModel>>, position: [f64; 2], velocity: [f64; 2]) {
        self.steps += 1;
        if !self.steps.is_multiple_of(TICK_STEPS) {
            return;
        }
        let centre = [self.centre[0] as f64 * WINDOW_CELL_M, self.centre[1] as f64 * WINDOW_CELL_M];
        if (position[0] - centre[0]).abs() > RECENTRE_M || (position[1] - centre[1]).abs() > RECENTRE_M {
            let started = std::time::Instant::now();
            self.recentre([(position[0] / WINDOW_CELL_M).round() as i64, (position[1] / WINDOW_CELL_M).round() as i64]);
            self.upload(data);
            self.recentres += 1;
            self.last_recentre_s = started.elapsed().as_secs_f64();
        }
        self.refresh(data, position);
        if let Some(sender) = &self.prefetch {
            let w = self.ground.to_world(position);
            let v = self.ground.to_world([position[0] + velocity[0], position[1] + velocity[1]]);
            // A gone worker only means a colder cache.
            let _ = sender.send([w[0], w[1], v[0] - w[0], v[1] - w[1]]);
        }
    }

    /// Lattice points (physics frame) of the window centred on cell `centre`, row by row, where
    /// `keep(row, column)` holds.
    fn lattice(centre: [i64; 2], keep: impl Fn(i64, i64) -> bool) -> Vec<(usize, [f64; 2])> {
        let n = WINDOW_SAMPLES as i64;
        let mut points = Vec::new();
        for r in 0..n {
            for c in 0..n {
                if keep(r, c) {
                    let x = (centre[0] - HALF_CELLS + c) as f64 * WINDOW_CELL_M;
                    let y = (centre[1] - HALF_CELLS + r) as f64 * WINDOW_CELL_M;
                    points.push(((r * n + c) as usize, [x, y]));
                }
            }
        }
        points
    }

    /// Quantized physics-frame heights at lattice points, in their order.
    fn sample(&self, points: &[(usize, [f64; 2])]) -> Vec<f64> {
        let world: Vec<[f64; 2]> = points.iter().map(|&(_, p)| self.ground.to_world(p)).collect();
        let mut out = vec![0.0; world.len()];
        self.ground.world().fill_points(&world, FACE_INSET_M, &mut out);
        out.iter().map(|&h| ((h - self.ground.origin_z()) / QUANTUM_M).round() * QUANTUM_M).collect()
    }

    /// Moves the window to cell `to`: keeps the overlap, samples only the new strips.
    fn recentre(&mut self, to: [i64; 2]) {
        let n = WINDOW_SAMPLES as i64;
        let [dx, dy] = [to[0] - self.centre[0], to[1] - self.centre[1]];
        let inside = |r: i64, c: i64| (0..n).contains(&(r + dy)) && (0..n).contains(&(c + dx));
        let fresh = Self::lattice(to, |r, c| !inside(r, c));
        let values = self.sample(&fresh);
        let mut heights = vec![0.0; self.heights.len()];
        for r in 0..n {
            for c in 0..n {
                if inside(r, c) {
                    heights[(r * n + c) as usize] = self.heights[((r + dy) * n + c + dx) as usize];
                }
            }
        }
        for (&(index, _), value) in fresh.iter().zip(values) {
            heights[index] = value;
        }
        self.heights = heights;
        self.centre = to;
    }

    /// Writes the heights and the window's position into the model and data.
    fn upload(&mut self, data: &mut MjData<Box<MjModel>>) {
        let low = self.heights.iter().copied().fold(f64::INFINITY, f64::min);
        self.z0 = low.floor();
        let pos = [self.centre[0] as f64 * WINDOW_CELL_M, self.centre[1] as f64 * WINDOW_CELL_M, self.z0];
        // SAFETY: only height-field samples change; sizes, counts and the signature stay.
        let model = unsafe { data.model_mut() };
        let field = &mut model.hfield_data_mut()[self.hfield_adr..self.hfield_adr + self.heights.len()];
        for (value, &h) in field.iter_mut().zip(&self.heights) {
            // Exact: h − z0 is a multiple of 2⁻¹⁶ below 8 m, and the range is a power of two.
            *value = ((h - self.z0).clamp(0.0, WINDOW_RANGE_M) / WINDOW_RANGE_M) as f32;
        }
        data.mocap_pos_mut()[self.mocap] = pos;
    }

    /// Fills the pool with the solids near `position` and sets the tire friction for the surface
    /// under it.
    fn refresh(&mut self, data: &mut MjData<Box<MjModel>>, position: [f64; 2]) {
        let reach = POOL_REACH_M;
        let mut rect = Rect::empty();
        for corner in [[-reach, -reach], [reach, -reach], [-reach, reach], [reach, reach]] {
            rect.include(self.ground.to_world([position[0] + corner[0], position[1] + corner[1]]));
        }
        let anchor = self.ground.to_world(position);
        let mut near: Vec<(f64, Solid)> = self
            .ground
            .world()
            .solids_in(rect)
            .into_iter()
            .map(|s| (s.bounds().distance(anchor), s))
            .filter(|(d, _)| *d <= reach)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
        near.truncate(POOL_SIZE);
        let wanted: BTreeMap<u64, Solid> = near.into_iter().map(|(_, s)| (s.id, s)).collect();
        // Keep the slots of solids still in reach; park the rest.
        for slot in &mut self.slots {
            if slot.solid.is_some_and(|id| !wanted.contains_key(&id)) {
                slot.solid = None;
                data.mocap_pos_mut()[slot.mocap] = [0.0, 0.0, PARKED_Z_M];
            }
        }
        let placed: Vec<u64> = self.slots.iter().filter_map(|s| s.solid).collect();
        let free: Vec<usize> = (0..self.slots.len()).filter(|&k| self.slots[k].solid.is_none()).collect();
        let mut free = free.into_iter();
        for (id, solid) in wanted.iter().filter(|(id, _)| !placed.contains(id)) {
            let Some(k) = free.next() else { break };
            self.slots[k].solid = Some(*id);
            self.place(data, k, solid);
        }
        let surface = self.ground.surface(position);
        if surface != self.surface || self.steps == 0 {
            self.surface = surface;
            let asphalt = Surface::of(Material::Asphalt, false);
            let grip = surface.friction / asphalt.friction;
            let roll = surface.rolling_resistance / asphalt.rolling_resistance;
            let [mu, spin, rolling] = self.tire_friction;
            // SAFETY: friction coefficients only.
            let model = unsafe { data.model_mut() };
            for &g in &self.tires {
                model.geom_friction_mut()[g] = [mu * grip, spin, rolling * roll];
            }
        }
    }

    /// Moves pool slot `k` onto `solid`.
    fn place(&self, data: &mut MjData<Box<MjModel>>, k: usize, solid: &Solid) {
        let slot = self.slots[k];
        let [c, s] = self.ground.rotation;
        let rotate = |v: [f64; 3]| [c * v[0] + s * v[1], -s * v[0] + c * v[1], v[2]];
        let axes = solid.axes.map(rotate);
        let centre = self.ground.to_physics([solid.centre[0], solid.centre[1]]);
        data.mocap_pos_mut()[slot.mocap] = [centre[0], centre[1], solid.centre[2] - self.ground.origin_z()];
        data.mocap_quat_mut()[slot.mocap] = Solid { axes, ..*solid }.quaternion();
        // SAFETY: a box's size and its bounds, kept consistent; the signature stays.
        let model = unsafe { data.model_mut() };
        model.geom_size_mut()[slot.geom] = solid.half;
        model.geom_rbound_mut()[slot.geom] = solid.half.iter().map(|h| h * h).sum::<f64>().sqrt();
        model.geom_aabb_mut()[slot.geom] = [0.0, 0.0, 0.0, solid.half[0], solid.half[1], solid.half[2]];
    }

    /// Solids in the pool now (physics frame), for checks and drawing.
    pub(crate) fn pooled(&self) -> usize {
        self.slots.iter().filter(|s| s.solid.is_some()).count()
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // Closing the channel ends the worker.
        self.prefetch.take();
    }
}

/// Keeps chunks generated ahead of the wheel: within 40 m of where it will be in 1.5 s.
fn prefetch(world: &World, hints: &mpsc::Receiver<[f64; 4]>) {
    while let Ok(mut hint) = hints.recv() {
        // Only the newest hint matters.
        while let Ok(newer) = hints.try_recv() {
            hint = newer;
        }
        let ahead = [hint[0] + hint[2] * 1.5, hint[1] + hint[3] * 1.5];
        let rect =
            Rect::new([ahead[0].min(hint[0]), ahead[1].min(hint[1])], [ahead[0].max(hint[0]), ahead[1].max(hint[1])])
                .grow(40.0);
        for key in euc_world::keys_in(rect) {
            world.chunk(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quaternion_of_identity_and_quarter_turn() {
        let solid = |axes| Solid { id: 0, kind: euc_world::SolidKind::Curb, centre: [0.0; 3], half: [1.0; 3], axes };
        let id = solid([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]).quaternion();
        assert!((id[0] - 1.0).abs() < 1e-12 && id[1..].iter().all(|v| v.abs() < 1e-12));
        // x → y: a quarter turn about z.
        let q = solid([[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]).quaternion();
        let h = std::f64::consts::FRAC_1_SQRT_2;
        assert!((q[0] - h).abs() < 1e-12 && (q[3] - h).abs() < 1e-12, "{q:?}");
    }

    #[test]
    fn frames_round_trip() {
        let g = WorldGround::new(WorldSpec::city(3), WorldStart { xy_m: [40.0, -12.0], heading_rad: 0.7 }).unwrap();
        let p = [3.25, -1.5];
        let back = g.to_physics(g.to_world(p));
        assert!((back[0] - p[0]).abs() < 1e-12 && (back[1] - p[1]).abs() < 1e-12);
        assert!(g.height_m([0.0, 0.0]).abs() < 1e-12);
    }
}
