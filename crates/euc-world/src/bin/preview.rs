//! `euc-world-preview`: draws a top view of a generated world into a PNG.
//!
//! ```text
//! cargo run -p euc-world --features preview --release --bin euc-world-preview -- \
//!     --seed 7 --area -64,-64,64,64 --px-per-m 4 --out world.png [--grid] [--print]
//! ```
//!
//! Shaded relief (light from the north-west, relief exaggerated 3×) over surface colours;
//! curb stones, stair steps, ramps and plaza platforms as their solids' footprints; buildings as
//! roofs; props as dots (lamp posts yellow, hydrants red, trees as canopies); the spawn as a
//! magenta ring. `--grid` draws the 16 m chunk borders; `--print` lists what the area holds.

use std::collections::BTreeMap;
use std::process::ExitCode;

use euc_world::{
    CHUNK_M, FeatureKind, Material, ParcelKind, PropKind, Rect, Solid, SolidKind, World, WorldSpec, Zone, keys_in,
};

struct Args {
    seed: u64,
    area: Rect,
    px_per_m: f64,
    out: String,
    grid: bool,
    print: bool,
}

fn parse() -> Result<Args, String> {
    let mut args = Args {
        seed: 0,
        area: Rect::new([-64.0, -64.0], [64.0, 64.0]),
        px_per_m: 4.0,
        out: String::new(),
        grid: false,
        print: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--seed" => args.seed = value()?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--area" => {
                let v: Vec<f64> = value()?
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<_, _>>()
                    .map_err(|e| format!("--area: {e}"))?;
                if v.len() != 4 || v[2] <= v[0] || v[3] <= v[1] {
                    return Err("--area takes x0,y0,x1,y1 with x1 > x0 and y1 > y0".into());
                }
                args.area = Rect::new([v[0], v[1]], [v[2], v[3]]);
            }
            "--px-per-m" => args.px_per_m = value()?.parse().map_err(|e| format!("--px-per-m: {e}"))?,
            "--out" => args.out = value()?,
            "--grid" => args.grid = true,
            "--print" => args.print = true,
            "-h" | "--help" => {
                return Err("usage: euc-world-preview --seed N [--area x0,y0,x1,y1] [--px-per-m K] --out file.png \
                            [--grid] [--print]"
                    .into());
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if args.out.is_empty() {
        return Err("--out file.png is required".into());
    }
    if !(args.px_per_m > 0.0 && args.px_per_m <= 200.0) {
        return Err("--px-per-m must be within (0, 200]".into());
    }
    Ok(args)
}

const fn colour(material: Material) -> [f64; 3] {
    match material {
        Material::Asphalt => [62.0, 64.0, 68.0],
        Material::WornAsphalt => [96.0, 93.0, 88.0],
        Material::Concrete => [178.0, 176.0, 168.0],
        Material::PavingStones => [176.0, 150.0, 124.0],
        Material::Cobblestones => [124.0, 110.0, 98.0],
        Material::Gravel => [180.0, 164.0, 128.0],
        Material::Grass => [88.0, 138.0, 68.0],
        Material::Dirt => [132.0, 100.0, 68.0],
        Material::Paint => [236.0, 234.0, 226.0],
        Material::Metal => [64.0, 80.0, 100.0],
    }
}

fn blend(base: [f64; 3], over: [f64; 3], alpha: f64) -> [f64; 3] {
    [0, 1, 2].map(|k| base[k] * (1.0 - alpha) + over[k] * alpha)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("euc-world-preview: {e}");
            ExitCode::FAILURE
        }
    }
}

#[allow(clippy::too_many_lines)]
fn run() -> Result<(), String> {
    let args = parse()?;
    let world = World::new(WorldSpec::city(args.seed))?;
    let area = args.area;
    let k = args.px_per_m;
    let width = ((area.max[0] - area.min[0]) * k).round() as usize;
    let height = ((area.max[1] - area.min[1]) * k).round() as usize;
    if width == 0 || height == 0 || width > 12_000 || height > 12_000 {
        return Err(format!("image would be {width} × {height} px; keep each side within 1..=12000"));
    }
    let started = std::time::Instant::now();
    // Pixel centres; row 0 is the top (largest y). One extra ring for the relief gradient.
    let (gw, gh) = (width + 2, height + 2);
    let at = |i: usize, j: usize| [area.min[0] + (i as f64 - 0.5) / k, area.max[1] - (j as f64 - 0.5) / k];
    let mut samples = vec![None; gw * gh];
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    std::thread::scope(|scope| {
        for (t, rows) in samples.chunks_mut(gw * gh.div_ceil(threads)).enumerate() {
            let world = &world;
            scope.spawn(move || {
                let first = t * gh.div_ceil(threads);
                for (r, row) in rows.chunks_mut(gw).enumerate() {
                    for (i, cell) in row.iter_mut().enumerate() {
                        let p = at(i, first + r);
                        *cell = Some(world.sample(p[0], p[1]));
                    }
                }
            });
        }
    });
    let sample = |i: usize, j: usize| samples[j * gw + i].expect("every pixel was sampled");
    let light = {
        let l: [f64; 3] = [-1.0, 1.0, 1.4];
        let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        l.map(|v| v / n)
    };
    let mut rgb = vec![[0.0f64; 3]; width * height];
    for j in 0..height {
        for i in 0..width {
            let s = sample(i + 1, j + 1);
            let slope_x = (sample(i + 2, j + 1).height_m - sample(i, j + 1).height_m) * k / 2.0 * 3.0;
            let slope_y = (sample(i + 1, j).height_m - sample(i + 1, j + 2).height_m) * k / 2.0 * 3.0;
            let n = (slope_x * slope_x + slope_y * slope_y + 1.0).sqrt();
            let lit = (-slope_x * light[0] - slope_y * light[1] + light[2]) / n;
            let shade = (0.3 + 0.8 * lit).clamp(0.25, 1.2);
            let mut c = colour(s.surface.material);
            if s.zone == Zone::Building {
                c = [96.0, 100.0, 116.0];
            } else if s.zone == Zone::Road && s.surface.material == Material::Concrete {
                // Concrete streets darker than concrete sidewalks, so the curb line reads.
                c = [146.0, 144.0, 138.0];
            }
            if s.surface.wet {
                c = blend(c, [52.0, 84.0, 132.0], 0.45);
            }
            // Contours every 0.5 m, faint.
            let band = |h: f64| (h / 0.5).floor() as i64;
            if band(s.height_m) != band(sample(i + 2, j + 1).height_m)
                || band(s.height_m) != band(sample(i + 1, j + 2).height_m)
            {
                c = blend(c, [0.0, 0.0, 0.0], 0.12);
            }
            rgb[j * width + i] = c.map(|v| v * shade);
        }
    }
    let to_px = |x: f64, y: f64| ((x - area.min[0]) * k, (area.max[1] - y) * k);
    let solids = world.solids_in(area);
    for solid in &solids {
        let (fill, alpha, outline) = match solid.kind {
            SolidKind::Curb => ([226.0, 222.0, 208.0], 0.85, None),
            SolidKind::Step => ([214.0, 206.0, 190.0], 0.0, Some([250.0, 246.0, 236.0])),
            SolidKind::Ramp => ([200.0, 170.0, 120.0], 0.35, Some([120.0, 90.0, 50.0])),
            SolidKind::Platform => ([0.0; 3], 0.0, Some([90.0, 70.0, 50.0])),
            SolidKind::Building => ([0.0; 3], 0.0, Some([40.0, 42.0, 52.0])),
            SolidKind::Prop(PropKind::Bench) => ([110.0, 72.0, 40.0], 1.0, None),
            SolidKind::Prop(PropKind::Bin) => ([70.0, 74.0, 70.0], 1.0, None),
            SolidKind::Prop(_) => continue,
        };
        paint_solid(&mut rgb, width, height, solid, &to_px, k, fill, alpha, outline);
    }
    for prop in world.props_in(area.grow(3.0)) {
        let (x, y) = to_px(prop.position[0], prop.position[1]);
        match prop.kind {
            PropKind::Tree => {
                disc(&mut rgb, width, height, x, y, 2.0 * k, [36.0, 86.0, 40.0], 0.55);
                disc(&mut rgb, width, height, x, y, (0.18 * k).max(1.5), [90.0, 60.0, 30.0], 1.0);
            }
            PropKind::LampPost => disc(&mut rgb, width, height, x, y, (0.2 * k).max(2.0), [250.0, 206.0, 50.0], 1.0),
            PropKind::Hydrant => disc(&mut rgb, width, height, x, y, (0.2 * k).max(2.0), [210.0, 40.0, 36.0], 1.0),
            PropKind::Bollard => disc(&mut rgb, width, height, x, y, (0.12 * k).max(1.5), [40.0, 40.0, 44.0], 1.0),
            PropKind::Bench | PropKind::Bin => {}
        }
    }
    if args.grid {
        let mut x = (area.min[0] / CHUNK_M).ceil() * CHUNK_M;
        while x <= area.max[0] {
            let (px, _) = to_px(x, 0.0);
            for j in 0..height {
                let i = px as usize;
                if i < width {
                    rgb[j * width + i] = blend(rgb[j * width + i], [255.0, 255.0, 255.0], 0.25);
                }
            }
            x += CHUNK_M;
        }
        let mut y = (area.min[1] / CHUNK_M).ceil() * CHUNK_M;
        while y <= area.max[1] {
            let (_, py) = to_px(0.0, y);
            let j = py as usize;
            if j < height {
                for i in 0..width {
                    rgb[j * width + i] = blend(rgb[j * width + i], [255.0, 255.0, 255.0], 0.25);
                }
            }
            y += CHUNK_M;
        }
    }
    let spawn = world.spawn();
    let (sx, sy) = to_px(spawn.position[0], spawn.position[1]);
    ring(&mut rgb, width, height, sx, sy, (0.8 * k).max(5.0), [230.0, 40.0, 200.0]);
    write_png(&args.out, width, height, &rgb)?;
    eprintln!(
        "wrote {} ({width} × {height} px, {:.0} m × {:.0} m, seed {}) in {:.1} s",
        args.out,
        area.max[0] - area.min[0],
        area.max[1] - area.min[1],
        args.seed,
        started.elapsed().as_secs_f64()
    );
    if args.print {
        print_summary(&world, area, &solids);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments, clippy::many_single_char_names)] // k is the scale, b the bounds, x and y a pixel's world point
fn paint_solid(
    rgb: &mut [[f64; 3]],
    width: usize,
    height: usize,
    solid: &Solid,
    to_px: &dyn Fn(f64, f64) -> (f64, f64),
    k: f64,
    fill: [f64; 3],
    alpha: f64,
    outline: Option<[f64; 3]>,
) {
    let b = solid.bounds();
    let (x0, y0) = to_px(b.min[0], b.max[1]);
    let (x1, y1) = to_px(b.max[0], b.min[1]);
    let edge = 1.0 / k;
    for j in (y0.floor().max(0.0) as usize)..(y1.ceil().max(0.0) as usize).min(height) {
        for i in (x0.floor().max(0.0) as usize)..(x1.ceil().max(0.0) as usize).min(width) {
            let x = b.min[0] + (i as f64 + 0.5 - x0) / k;
            let y = b.max[1] - (j as f64 + 0.5 - y0) / k;
            if solid.top_at([x, y], 0.0).is_none() {
                continue;
            }
            let inner = solid.top_at([x, y], -edge).is_some();
            let px = &mut rgb[j * width + i];
            match (inner, outline) {
                (false, Some(o)) => *px = o,
                _ => *px = blend(*px, fill, alpha),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn disc(rgb: &mut [[f64; 3]], width: usize, height: usize, x: f64, y: f64, r: f64, c: [f64; 3], alpha: f64) {
    for j in ((y - r).floor().max(0.0) as usize)..((y + r).ceil().max(0.0) as usize).min(height) {
        for i in ((x - r).floor().max(0.0) as usize)..((x + r).ceil().max(0.0) as usize).min(width) {
            let (dx, dy) = (i as f64 + 0.5 - x, j as f64 + 0.5 - y);
            if dx * dx + dy * dy <= r * r {
                rgb[j * width + i] = blend(rgb[j * width + i], c, alpha);
            }
        }
    }
}

fn ring(rgb: &mut [[f64; 3]], width: usize, height: usize, x: f64, y: f64, r: f64, c: [f64; 3]) {
    for j in ((y - r - 2.0).floor().max(0.0) as usize)..((y + r + 2.0).ceil().max(0.0) as usize).min(height) {
        for i in ((x - r - 2.0).floor().max(0.0) as usize)..((x + r + 2.0).ceil().max(0.0) as usize).min(width) {
            let (dx, dy) = (i as f64 + 0.5 - x, j as f64 + 0.5 - y);
            if ((dx * dx + dy * dy).sqrt() - r).abs() < 1.5 {
                rgb[j * width + i] = c;
            }
        }
    }
}

fn print_summary(world: &World, area: Rect, solids: &[Solid]) {
    let mut features: BTreeMap<&str, usize> = BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut parcels: BTreeMap<&str, usize> = BTreeMap::new();
    let mut plazas = Vec::new();
    for key in keys_in(area) {
        let chunk = world.chunk(key);
        for f in chunk.features() {
            if seen.insert(f.id) && f.bounds.overlaps(&area) {
                let name = match &f.kind {
                    FeatureKind::Pothole { .. } => "pothole",
                    FeatureKind::Manhole { .. } => "manhole",
                    FeatureKind::Crack { .. } => "crack",
                    FeatureKind::RoughPatch { .. } => "rough patch",
                    FeatureKind::WetPatch { .. } => "wet patch",
                    FeatureKind::SpeedBump { .. } => "speed bump",
                    FeatureKind::Crosswalk { .. } => "crosswalk",
                    FeatureKind::CurbCut { driveway: true, .. } => "driveway",
                    FeatureKind::CurbCut { .. } => "crossing curb cut",
                    FeatureKind::TreePit { .. } => "tree pit",
                };
                *features.entry(name).or_default() += 1;
            }
        }
        for p in chunk.parcels() {
            if seen.insert(p.id) && p.bounds.overlaps(&area) {
                let name = match p.kind {
                    ParcelKind::Building { .. } => "building",
                    ParcelKind::Park => "park",
                    ParcelKind::Parking => "parking",
                    ParcelKind::Plaza(z) => {
                        let c = [
                            p.origin[0] + p.u[0] * p.width_m / 2.0 + p.v[0] * p.depth_m / 2.0,
                            p.origin[1] + p.u[1] * p.width_m / 2.0 + p.v[1] * p.depth_m / 2.0,
                        ];
                        plazas.push((c, z, p.width_m, p.depth_m));
                        "plaza"
                    }
                };
                *parcels.entry(name).or_default() += 1;
            }
        }
    }
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for s in solids {
        *kinds.entry(format!("{:?}", s.kind)).or_default() += 1;
    }
    println!("features: {features:?}");
    println!("parcels: {parcels:?}");
    println!("solids: {kinds:?}");
    for (c, z, w, d) in plazas {
        println!(
            "plaza at ({:.1}, {:.1}): {w:.1} × {d:.1} m, {} steps of {:.3} m rise, {:.3} m tread, ramp {:.1} m at {:.1} %",
            c[0],
            c[1],
            z.steps,
            z.riser_m,
            z.tread_m,
            w - z.ramp_u0_m,
            (z.top_m - z.ramp_base_m) / (w - z.ramp_u0_m) * 100.0
        );
    }
}

fn write_png(path: &str, width: usize, height: usize, rgb: &[[f64; 3]]) -> Result<(), String> {
    let mut raw = Vec::with_capacity(height * (1 + width * 3));
    for j in 0..height {
        raw.push(0);
        for px in &rgb[j * width..(j + 1) * width] {
            raw.extend(px.map(|v| v.round().clamp(0.0, 255.0) as u8));
        }
    }
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        png.extend((data.len() as u32).to_be_bytes());
        let mut crc = crc32fast::Hasher::new();
        crc.update(kind);
        crc.update(data);
        png.extend(kind);
        png.extend(data);
        png.extend(crc.finalize().to_be_bytes());
    };
    let mut header = Vec::with_capacity(13);
    header.extend((width as u32).to_be_bytes());
    header.extend((height as u32).to_be_bytes());
    header.extend([8, 2, 0, 0, 0]);
    chunk(b"IHDR", &header);
    chunk(b"IDAT", &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6));
    chunk(b"IEND", &[]);
    std::fs::write(path, png).map_err(|e| format!("{path}: {e}"))
}
