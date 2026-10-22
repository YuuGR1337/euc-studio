//! Smooth scalar fields: terrain relief, curb height and sidewalk width, and the height profiles
//! across a street. All are pure functions of the seed and the position.

use crate::det::{self, V2, value_noise};

/// Radius around the spawn inside which the terrain is level, metres.
pub const SPAWN_FLAT_RADIUS_M: f64 = 12.0;
/// Radius at which the spawn levelling has faded out, metres.
pub const SPAWN_BLEND_RADIUS_M: f64 = 72.0;
/// Range of curb face heights, metres.
pub const CURB_HEIGHT_M: [f64; 2] = [0.10, 0.16];
/// Range of sidewalk widths from the curb face, metres.
pub const SIDEWALK_WIDTH_M: [f64; 2] = [2.5, 4.0];
/// Cross slope of a sidewalk, rising away from the street.
pub const SIDEWALK_SLOPE: f64 = 0.015;
/// Crown cross slope of a carriageway, rising from the gutter.
pub const CROWN_SLOPE: f64 = 0.02;
/// Distance from the gutter over which the crown rises; the middle stays level, metres.
pub const CROWN_RISE_M: f64 = 3.5;
/// Height left at a curb cut's lip, metres.
pub const CUT_LIP_M: f64 = 0.012;

// Two octaves of value noise. Per-axis slope bound 3.75 A / L: 5.1 % and 2.8 %.
const HILL_WAVELENGTH_M: [f64; 2] = [256.0, 80.0];
const HILL_AMPLITUDE_M: [f64; 2] = [3.5, 0.6];
const CURB_WAVELENGTH_M: f64 = 48.0;
const SIDEWALK_WAVELENGTH_M: f64 = 64.0;

const SALT_HILL: [u64; 2] = [0x201, 0x202];
const SALT_CURB: u64 = 0x203;
const SALT_SIDEWALK: u64 = 0x204;

fn relief(seed: u64, p: V2) -> f64 {
    HILL_AMPLITUDE_M[0] * value_noise(seed, SALT_HILL[0], p, HILL_WAVELENGTH_M[0])
        + HILL_AMPLITUDE_M[1] * value_noise(seed, SALT_HILL[1], p, HILL_WAVELENGTH_M[1])
}

/// Terrain height before streets and features, metres: gentle hills, level around the spawn.
pub fn terrain(seed: u64, p: V2) -> f64 {
    let base = relief(seed, p);
    let r = det::len(p);
    if r >= SPAWN_BLEND_RADIUS_M {
        return base;
    }
    let origin = relief(seed, [0.0, 0.0]);
    origin + (base - origin) * det::smoothstep(SPAWN_FLAT_RADIUS_M, SPAWN_BLEND_RADIUS_M, r)
}

/// Curb face height before curb cuts, metres.
pub fn curb_height(seed: u64, p: V2) -> f64 {
    let t = 0.5 + 0.5 * value_noise(seed, SALT_CURB, p, CURB_WAVELENGTH_M);
    det::lerp(CURB_HEIGHT_M[0], CURB_HEIGHT_M[1], t)
}

/// Sidewalk width from the curb face, metres.
pub fn sidewalk_width(seed: u64, p: V2) -> f64 {
    let t = 0.5 + 0.5 * value_noise(seed, SALT_SIDEWALK, p, SIDEWALK_WAVELENGTH_M);
    det::lerp(SIDEWALK_WIDTH_M[0], SIDEWALK_WIDTH_M[1], t)
}

/// Height of the carriageway above the terrain at road field value `f` (negative), metres.
pub fn crown(f: f64) -> f64 {
    CROWN_SLOPE * (-f).min(CROWN_RISE_M)
}

/// Height of the ground beyond the sidewalk (no parcel, no feature), metres.
pub fn lot_height(seed: u64, p: V2) -> f64 {
    terrain(seed, p) + curb_height(seed, p) + SIDEWALK_SLOPE * sidewalk_width(seed, p)
}
