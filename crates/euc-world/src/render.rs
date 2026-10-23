//! Chunk payloads for drawing: heights, materials, face boxes and props as compact binary, at
//! three levels of detail. The studio streams these to its view; nothing here feeds the physics.
//!
//! Layout (little-endian, every array 4-byte aligned): `"EUCW"`, version `u32`, chunk count
//! `u32`, then per chunk: `cx i32`, `cy i32`, `lod u32`, height samples per side `u32`, material
//! texels per side `u32`, heights `f32[n·n]` (world z, rows from min y, x fastest, borders shared
//! with the neighbours), materials `u8[m·m]` padded to 4 (material code, bit 7 = wet), solid count
//! `u32` and per solid `kind u32`, `id_lo u32`, `id_hi u32`, centre `f32[3]`, half `f32[3]`,
//! quaternion `f32[4]` (w, x, y, z), then prop count `u32` and per prop `kind u32`, `id_lo u32`,
//! `id_hi u32`, position `f32[3]`, facing `f32` (radians). Everything is in world coordinates.

use crate::{ChunkKey, PropKind, SolidKind, World};

/// Payload magic.
pub const MAGIC: &[u8; 4] = b"EUCW";
/// Payload version.
pub const VERSION: u32 = 1;

/// One level of detail.
#[derive(Debug, Clone, Copy)]
pub struct Lod {
    /// Height samples per chunk side.
    pub heights: usize,
    /// Material texels per chunk side.
    pub texels: usize,
    /// Faces with a solid move this far behind it, so a drawn box hides the mesh's slope, metres.
    pub inset: f64,
    /// Curbs, steps, plaza walls and ramps (buildings are always sent).
    pub small_solids: bool,
    /// Props.
    pub props: bool,
}

/// The levels: 0 near the wheel (12.5 cm heights, 6.25 cm materials), 1 the city around (50 cm),
/// 2 the distance (2 m).
pub const LODS: [Lod; 3] = [
    Lod { heights: 129, texels: 256, inset: 0.15, small_solids: true, props: true },
    Lod { heights: 33, texels: 32, inset: 0.0, small_solids: false, props: true },
    Lod { heights: 9, texels: 8, inset: 0.0, small_solids: false, props: false },
];

const fn solid_code(kind: SolidKind) -> u32 {
    match kind {
        SolidKind::Curb => 0,
        SolidKind::Step => 1,
        SolidKind::Platform => 2,
        SolidKind::Ramp => 3,
        SolidKind::Building => 4,
        SolidKind::Prop(_) => 5,
    }
}

const fn prop_code(kind: PropKind) -> u32 {
    match kind {
        PropKind::LampPost => 0,
        PropKind::Tree => 1,
        PropKind::Bench => 2,
        PropKind::Bin => 3,
        PropKind::Hydrant => 4,
        PropKind::Bollard => 5,
    }
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_f32(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&(v as f32).to_le_bytes());
}

fn put_id(out: &mut Vec<u8>, id: u64) {
    put_u32(out, id as u32);
    put_u32(out, (id >> 32) as u32);
}

/// Appends one chunk at level of detail `lod` (clamped to the coarsest) to `out`.
pub fn chunk_payload(world: &World, key: ChunkKey, lod: usize, out: &mut Vec<u8>) {
    let detail = &LODS[lod.min(LODS.len() - 1)];
    let chunk = world.chunk(key);
    let rect = chunk.rect();
    out.extend_from_slice(&key.cx.to_le_bytes());
    out.extend_from_slice(&key.cy.to_le_bytes());
    put_u32(out, lod.min(LODS.len() - 1) as u32);
    put_u32(out, detail.heights as u32);
    put_u32(out, detail.texels as u32);
    let step = crate::CHUNK_M / (detail.heights - 1) as f64;
    for j in 0..detail.heights {
        for i in 0..detail.heights {
            let p = [rect.min[0] + i as f64 * step, rect.min[1] + j as f64 * step];
            put_f32(out, chunk.ground_at(p, detail.inset));
        }
    }
    let texel = crate::CHUNK_M / detail.texels as f64;
    for j in 0..detail.texels {
        for i in 0..detail.texels {
            let p = [rect.min[0] + (i as f64 + 0.5) * texel, rect.min[1] + (j as f64 + 0.5) * texel];
            let surface = chunk.sample(p).surface;
            out.push(surface.material.code() | if surface.wet { 0x80 } else { 0 });
        }
    }
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    let solids: Vec<_> = chunk
        .solids()
        .iter()
        .filter(|s| match s.kind {
            SolidKind::Building => true,
            SolidKind::Prop(_) => false,
            _ => detail.small_solids,
        })
        .collect();
    put_u32(out, solids.len() as u32);
    for s in solids {
        put_u32(out, solid_code(s.kind));
        put_id(out, s.id);
        for v in s.centre.into_iter().chain(s.half).chain(s.quaternion()) {
            put_f32(out, v);
        }
    }
    let props = if detail.props { chunk.props() } else { &[] };
    put_u32(out, props.len() as u32);
    for p in props {
        put_u32(out, prop_code(p.kind));
        put_id(out, p.id);
        for v in p.position {
            put_f32(out, v);
        }
        put_f32(out, p.facing[1].atan2(p.facing[0]));
    }
}

/// The payload for `keys` at `lod`.
pub fn payload(world: &World, keys: &[[i32; 2]], lod: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(keys.len() * 140_000);
    out.extend_from_slice(MAGIC);
    put_u32(&mut out, VERSION);
    put_u32(&mut out, keys.len() as u32);
    for &[cx, cy] in keys {
        chunk_payload(world, ChunkKey { cx, cy }, lod, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorldSpec;

    /// Offsets of each chunk's heights, and the end of the payload.
    fn walk(bytes: &[u8]) -> (Vec<(usize, usize)>, usize) {
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let mut at = 12;
        let mut chunks = Vec::new();
        for _ in 0..u32_at(8) {
            let (n, m) = (u32_at(at + 12), u32_at(at + 16));
            chunks.push((at + 20, n));
            at += 20 + n * n * 4;
            at += (m * m).next_multiple_of(4);
            assert_eq!(at % 4, 0);
            at += 4 + u32_at(at) * 4 * 13;
            at += 4 + u32_at(at) * 4 * 7;
        }
        (chunks, at)
    }

    #[test]
    fn payload_layout_is_aligned_and_complete() {
        let world = World::new(WorldSpec::city(42)).unwrap();
        for (lod, detail) in LODS.iter().enumerate() {
            let bytes = payload(&world, &[[0, 0], [-1, 2]], lod);
            assert_eq!(&bytes[..4], MAGIC);
            let (chunks, end) = walk(&bytes);
            assert_eq!(end, bytes.len(), "lod {lod}");
            assert!(chunks.iter().all(|&(_, n)| n == detail.heights));
        }
    }

    #[test]
    fn neighbouring_chunks_share_their_border_heights() {
        let world = World::new(WorldSpec::city(42)).unwrap();
        let bytes = payload(&world, &[[0, 0], [1, 0]], 0);
        let (chunks, _) = walk(&bytes);
        let height = |(base, n): (usize, usize), i: usize, j: usize| {
            let at = base + (j * n + i) * 4;
            f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
        };
        let n = chunks[0].1;
        for j in 0..n {
            assert_eq!(height(chunks[0], n - 1, j).to_bits(), height(chunks[1], 0, j).to_bits(), "row {j}");
        }
    }
}
