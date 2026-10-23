//! The streaming world for the studio: one shared `euc_world::World` per specification (physics
//! and drawing use the same chunk cache), and chunk payloads for the view as compact binary (the
//! layout is in `euc_world::render`).

use std::sync::{Arc, Mutex, OnceLock};

use euc_world::{World, WorldSpec};
use serde::Serialize;

/// Where a run is in its world: the world, and the frame its physics uses. The view draws the
/// world in the physics frame: physics = R(−heading) · (world − start), z = world z − origin.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorldFrame {
    pub spec: WorldSpec,
    /// Ground point under the wheel at the start, world coordinates, metres.
    pub start_xy_m: [f64; 2],
    /// Heading at the start, radians, left positive from world +x.
    pub start_heading_rad: f64,
    /// World height of the physics frame's z = 0, metres.
    pub origin_z_m: f64,
}

impl WorldFrame {
    pub(crate) fn of(ground: &euc_physics::WorldGround) -> Self {
        let start = ground.start();
        Self {
            spec: *ground.spec(),
            start_xy_m: start.xy_m,
            start_heading_rad: start.heading_rad,
            origin_z_m: ground.origin_z(),
        }
    }
}

/// Worlds kept alive between runs and views.
const KEPT_WORLDS: usize = 4;
/// Most chunks one request may ask for.
const MAX_KEYS: usize = 64;

fn worlds() -> &'static Mutex<Vec<Arc<World>>> {
    static WORLDS: OnceLock<Mutex<Vec<Arc<World>>>> = OnceLock::new();
    WORLDS.get_or_init(|| Mutex::new(Vec::new()))
}

/// The shared world of `spec` (created on first use; the four most recent stay cached).
pub(crate) fn shared(spec: WorldSpec) -> Result<Arc<World>, String> {
    let mut kept = worlds().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(index) = kept.iter().position(|w| *w.spec() == spec) {
        let world = kept.remove(index);
        kept.push(Arc::clone(&world));
        return Ok(world);
    }
    let world = Arc::new(World::new(spec)?);
    if kept.len() >= KEPT_WORLDS {
        kept.remove(0);
    }
    kept.push(Arc::clone(&world));
    Ok(world)
}

/// Chunk payloads for the view (see `euc_world::render`). Runs off the main thread.
#[tauri::command]
pub async fn world_chunks(spec: WorldSpec, keys: Vec<[i32; 2]>, lod: u8) -> Result<tauri::ipc::Response, String> {
    if keys.len() > MAX_KEYS {
        return Err(format!("ask for at most {MAX_KEYS} chunks at a time"));
    }
    let world = shared(spec)?;
    Ok(tauri::ipc::Response::new(euc_world::render::payload(&world, &keys, usize::from(lod))))
}

/// The world height under a start point: the physics frame's z = 0, metres.
#[tauri::command]
pub fn world_origin_z(spec: WorldSpec, start_xy_m: [f64; 2]) -> Result<f64, String> {
    if !start_xy_m.iter().all(|v| v.is_finite()) {
        return Err("start must be finite".into());
    }
    Ok(shared(spec)?.height_at(start_xy_m[0], start_xy_m[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_worlds_are_reused_per_spec() {
        let a = shared(WorldSpec::city(42)).unwrap();
        let b = shared(WorldSpec::city(42)).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!Arc::ptr_eq(&a, &shared(WorldSpec::city(43)).unwrap()));
    }

    #[test]
    fn origin_height_refuses_a_point_that_is_not_finite() {
        assert!(world_origin_z(WorldSpec::city(42), [f64::NAN, 0.0]).is_err());
        assert!(world_origin_z(WorldSpec::city(42), [0.0, 0.0]).unwrap().is_finite());
    }
}
