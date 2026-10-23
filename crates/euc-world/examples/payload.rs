//! Writes a chunk payload (`euc_world::render`) of a city world with default densities to stdout,
//! for the studio's browser checks with real chunks:
//!
//! ```text
//! cargo run -p euc-world --release --example payload -- SEED LOD CX,CY [CX,CY ...] > chunks.bin
//! ```

use std::io::Write;
use std::process::ExitCode;

use euc_world::{World, WorldSpec};

fn parse() -> Result<(u64, usize, Vec<[i32; 2]>), String> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: payload SEED LOD CX,CY [CX,CY ...]";
    let seed = args.next().ok_or(usage)?.parse().map_err(|e| format!("seed: {e}"))?;
    let lod = args.next().ok_or(usage)?.parse().map_err(|e| format!("lod: {e}"))?;
    let keys = args
        .map(|key| {
            let (cx, cy) = key.split_once(',').ok_or_else(|| format!("key {key}: want CX,CY"))?;
            Ok([cx.parse().map_err(|e| format!("cx: {e}"))?, cy.parse().map_err(|e| format!("cy: {e}"))?])
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok((seed, lod, keys))
}

fn main() -> ExitCode {
    let result = parse().and_then(|(seed, lod, keys)| {
        let world = World::new(WorldSpec::city(seed))?;
        let bytes = euc_world::render::payload(&world, &keys, lod);
        std::io::stdout().lock().write_all(&bytes).map_err(|e| e.to_string())
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("payload: {error}");
            ExitCode::FAILURE
        }
    }
}
