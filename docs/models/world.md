# Procedural world (W1)

Status: W1a, 2026-10-22. Design, the `euc-world` generator (city biome), a PNG preview and a MuJoCo feasibility spike.
Not integrated yet: Free Ride still uses the finite 32 m map of [generated-map-implementation.md](generated-map-implementation.md) until W1b.

The owner asked for a more realistic map with roadside objects, generated progressively like Minecraft, with roads, potholes and stairs.
This document is the design for that world, what W1a built, what the spike measured, and the W1b integration plan.

## Goals

- An infinite world, generated on demand in chunks around the rider.
- One seed gives the same world on every machine, in any generation order.
- A resolution-independent description: physics samples it finely near the wheel, rendering coarsely far away.
- Real city ground: curbs up and down, curb cuts, crosswalks, speed bumps, manholes, cracks, potholes, rough and wet patches, stairs with a ramp, and surfaces with their own grip and rolling resistance.
- Recordings store the seed and generator revision, never meshes.

## World model

### Chunks

A chunk is 16 m × 16 m. Chunk `(cx, cy)` covers `[16 cx, 16 cx + 16] × [16 cy, 16 cy + 16]`.

Why 16 m:

- The physics window (12 m, below) overlaps at most 2 × 2 chunks, so a recentre touches at most four chunks.
- Every local feature fits inside one chunk plus a neighbour: potholes (≤ 1.5 m), crosswalks (3 m), tree pits, curb cuts (≤ 6 m with flares).
- Generation stays small and parallel: 0.94 ms per chunk with the tile cache (measured, below).
- A chunk is a power of two in metres, so the physics lattice (1/32 m) divides it exactly (512 cells).

`Chunk::generate(spec, key)` depends only on the `WorldSpec` (seed, generator revision, biome, densities) and the key.
Two chunks give bit-identical heights and surfaces on their shared border (tested).

### Determinism rules

- Every random decision hashes integer coordinates with the seed and a per-layer salt (`det::hash`, a `SplitMix64` finalizer chain). There is no random state carried between items.
- A feature is placed by its own grid cell or street edge. Conflicts between neighbours are thinned by priority: a candidate is dropped when an overlapping neighbour candidate has a higher priority. Neighbours are judged by the same pure rule, so the result does not depend on which chunk asks first.
- Height contributions are summed in a canonical order (feature kind, then identity), so floating-point sums are identical in every chunk.
- Only IEEE-754 `+ − × ÷`, `sqrt`, `floor`, `abs`, `min`, `max` and integer operations appear in generation and sampling. Rust neither fuses them into FMAs nor reorders them, and they are correctly rounded on every shipped target. `sin`, `cos`, `exp`, `powf`, `hypot` and `atan2` call the platform's libm, which differs in the last bit between macOS, Linux and Windows, so `euc-world` never calls them. Curves are Catmull-Rom splines, profiles are polynomials, noise is value noise with a quintic fade.
- A golden test hashes the chunks around the spawn for a fixed seed. Any change to generated output fails it; the fix is to bump `GENERATOR_REVISION` and record the new value.

### Layers

1. **Terrain relief.** Two octaves of value noise (256 m and 80 m wavelengths, 3.5 m and 0.6 m amplitudes). Per-axis slope is bounded by 5.1 % and 2.8 %; the measured maximum over 5 seeds × 1.6 km² is under 12 % (test `hills_stay_gentle`). The relief is level within 12 m of the spawn and blends back in by 72 m.
2. **Road network.** Intersections sit on a jittered 64 m lattice. Every fourth row and column is an arterial (14 m, four lanes, double centre line, always present). Each 4 × 4 superblock keeps all of its local edges along one axis and 60 % along the other, so every node reaches an arterial along its row or column: the network is connected (test `the_road_network_is_connected`). Local streets are 7–10 m wide, with midpoint bulges of up to 6 m, so streets curve; missing edges leave dead ends and bends. Centrelines are Catmull-Rom splines through nodes and bulged midpoints, flattened to 32 segments per edge, and they join with matching tangents. Street paving is per edge: arterials asphalt; local streets 73 % asphalt, 12 % concrete slabs, 15 % cobblestones.
3. **Road field.** `f(p)` is the signed distance to the curb face line, negative on the carriageway: a hard minimum over the row edges and over the column edges, then a polynomial smooth minimum of the two (blend 4 m). The smooth minimum rounds block corners; it is exactly the plain minimum away from intersections. Every zone and profile reads `f`.
4. **Street profile.** Carriageway: a 2 % crown that rises over 3.5 m from the gutter, so wide streets have a level middle. Gutter pan: 0.3 m of concrete. Curb face: 10–16 cm (a slow noise field along the street). Sidewalk: 2.5–4 m wide, rising 1.5 % away from the curb, concrete with expansion joints every 1.5 m or pavers. Beyond it, the lot zone (grass, dirt, gravel) at the sidewalk's outer height.
5. **Street items** (per edge, at fixed arc-length slots): crosswalks at intersection ends (zebra paint, with a curb cut on both sides), speed bumps or humps mid-block on local streets (6–9 cm), driveways (curb cuts with flares, 3–4.5 m), tree pits with trees, lamp posts, benches, bins, hydrants and bollards.
6. **Grid features** (global jittered grids, one candidate per cell): potholes (4 m cells; 2–12 cm deep, 0.2–1.5 m wide, irregular outlines from 2–4 blended discs, steep walls 4 cm wide, a rough gravel bottom, 30 % hold water), manholes (16 m; 0.6–0.76 m covers, 1–3 cm sunk or 0.8–2 cm raised, 2 cm rim), cracks (3 m; 1–2 cm wide V grooves, 0.5–1.5 cm deep, 3–6 segment random walks), rough patches (12 m; worn asphalt with 3–8 mm roughness) and wet patches (10 m).
7. **Parcels** (20 m jittered grid, resolved per 80 m tile): buildings (setback 0.5–2.5 m, 6–40 m tall), parks (grass, a gravel path, trees), parking lots (painted bays and a driveway) and raised plazas. A parcel faces its nearest street, fronts on the sidewalk's outer edge, shrinks to fit a tight block (scales 1, 0.75, 0.55), and must lie wholly beyond the sidewalks. Exact rectangle overlap (separating axes) with 1 m clearance decides conflicts; plazas win, then buildings, parking, parks.
8. **Plazas: stairs between levels.** A level platform 3–12 risers above the ground at the foot of its stairs (risers 15–18 cm, treads 28–32 cm). The stairs (2–4 m wide) climb from the front. A ramp (1.5 m wide, 8.3–12 %, at most 12.5 %, at most 18 m long) runs along the platform's front edge beside the stairs, from the ground up to the platform. The step count rises until the platform clears the ground everywhere, so every wall is an up face.
9. **Textures in the height:** cobblestone setts (14 cm domes, about 8 mm peak to peak), paver joints (2 mm), concrete joints (6–8 mm grooves).

### Sampler

All on `World` (thread-safe; it caches chunks and parcel tiles) and on `Chunk`:

| Call | Returns | Used by |
| --- | --- | --- |
| `height_at(x, y)` | ground height, faces where they are | rendering, preview |
| `ground_at(x, y, inset)` | height with every face that has a solid moved `inset` behind its solid's face | physics height field |
| `fill_heights(origin, cell, nx, ny, inset, out)` | a lattice of `ground_at`, chunk by chunk | physics window |
| `sample(x, y)` / `surface_at(x, y)` | height, `Zone`, `Surface` (material, wet, friction, rolling resistance) | physics grip, rendering |
| `solids_in(rect)` | oriented boxes (curbs, steps, platforms, ramps, buildings, props), each once | physics box pool, rendering faces |
| `props_in(rect)` | props with ground z | rendering |
| `terrain_at(x, y)` | bare relief | distant scenery |
| `node(i, j)`, `edge(id)` | the road graph | routes, autopilot |
| `spawn()` | origin, heading +x | Free Ride start |

Solids are oriented boxes (`centre`, `half`, unit `axes` rows, right-handed).
A curb box is 15 cm deep from the face into the sidewalk, its top follows the curb top, and it reaches 15 cm below the road.
Curb boxes are traced from the zero contour of `f` by marching squares on a 0.25 m grid, simplified to 1 cm, cut into pieces of at most 2 m (shorter in curb-cut flares), and clipped to the chunk.
Every face of at least 4 cm gets a box; lower faces (manhole rims, tree pits, curb-cut lips, pothole walls) stay in the height field.

### Surfaces

Generic estimates for a pneumatic 3-inch tire from the usual published ranges. None is measured on a wheel; they set the order of the surfaces, not a validated grip model.

| Material | μ dry | μ wet | rolling resistance |
| --- | --- | --- | --- |
| asphalt | 0.90 | 0.60 | 0.010 |
| worn asphalt | 0.80 | 0.50 | 0.012 |
| concrete | 0.85 | 0.60 | 0.009 |
| paving stones | 0.75 | 0.50 | 0.012 |
| cobblestones | 0.70 | 0.40 | 0.018 |
| gravel | 0.55 | 0.50 | 0.035 |
| grass | 0.45 | 0.30 | 0.060 |
| dirt | 0.60 | 0.40 | 0.030 |
| paint | 0.65 | 0.35 | 0.010 |
| metal (manholes) | 0.55 | 0.25 | 0.009 |

Wet ground rolls 10 % harder.

### Spawn

The spawn is the origin, heading +x, 1.5 m right of the centreline of a straight arterial (row 0 is straight from x = −64 to 64 m).
Guarantees (test `the_spawn_is_level_clear_and_on_a_straight_road`, 12 seeds): asphalt road; level within ±1 cm over a 2 m radius; no feature within 10 m (crosswalks and curb cuts measured by their painted band and lowered curb); no prop within 10 m; no solid within 4.5 m (the curb is 5.5 m to the right); a smooth lane 15 m ahead.

### Densities

`Densities` sets every rate: potholes 1.5, manholes 0.6, cracks 12, rough patches 0.8 and wet patches 1.0 per 1000 m² of their zone; 35 % of local edges with a bump; 70 % of intersection ends with a crosswalk; 1.5 driveways per 100 m of street side; plaza weight 0.15 among parcels; props 0.8 of their slots.
Realised pothole density lands at 50–110 % of nominal (sites near the gutter and the spawn are refused) and doubles when the parameter doubles (test `densities_follow_their_parameters`).

## Physics plan (W1b)

### Height-field window

- One MuJoCo `hfield` of 385 × 385 samples at 1/32 m (3.125 cm): a 12 m square, elevation range 4 m, base 0.5 m.
- Its geom sits in a **mocap body**. Recentring writes `mocap_pos` and the hfield data. A geom in the world body does not move when `geom_pos` changes at run time in MuJoCo 3.9 (spike A1, B): the wheel falls through.
- The window recentres when the wheel is more than 1 m from its centre, by whole lattice cells, so the wheel always stays at least 5 m from its edge.
- Exactness: heights are quantized to 2⁻¹⁶ m, the window base sits on a whole metre and the range is a power of two, so `pos.z + f32(data) × range` reproduces every stored height exactly. A recentre therefore changes nothing under the wheel: ray heights at 2000 fixed points moved by at most 1.8 × 10⁻¹⁵ m over four recentres (spike E).
- The window samples `ground_at(…, inset = 1.5 cells)`: each curb, step and plaza wall steps up just behind its box's face, inside the box, so the height field never shows a slope in front of a box.
- If the local relief ever exceeds the 4 m range, the base moves by whole metres and the window refills.

### Box pool

- 128 mocap bodies with one box geom each, parked 1000 m below when unused.
- Every refresh, `solids_in` a 16 m square around the wheel; a solid keeps its slot while it stays in range (assignment by solid identity), new ones take free slots.
- Moving a box is `mocap_pos` and `mocap_quat`. Changing its size also needs `geom_size`, `geom_rbound` and `geom_aabb` written: with stale bounds MuJoCo misses contacts (spike B).
- Pool boxes use the floor's contact settings (`conaffinity="3"`), so wheel, shell and rider all touch them.

### Ground friction

The window geom gets `priority="1"`, so its friction wins over the tire's.
Before each step, the friction and rolling resistance under the wheel's contact point come from `surface_at`. MuJoCo applies a run-time change of `geom_friction` from the next step (spike F).
Rider limbs on the ground share that value; it is a single surface per step.

### Scheduling

Recentres and pool refreshes run between steps, at a fixed cadence (every 50 steps) and on state alone, so a replay repeats them exactly.
New strips and chunks are prefetched on a worker thread; the step blocks if they are not ready, so timing never changes results.

### Spike results

A throwaway program in `.cache/world-spike` (git-ignored) linked mujoco-rs 5.0.0 against MuJoCo 3.9.0, as `euc-physics` does. Measured on this Mac (Apple silicon), release build.

| Check | Result |
| --- | --- |
| A1: change hfield data under a resting ball | mocap window: ball rose 5.0 cm, as the data did. World-body window: `geom_pos` change ignored. |
| A2: ball rolling 10 m, recentred 10 m window vs one static hfield on the same lattice | mocap: 9 recentres, max deviation 3.2 × 10⁻⁸ m after 4 s. World-body window: fell through at step 800. |
| E: ray heights at fixed points across 4 recentres | max change 1.8 × 10⁻¹⁵ m; 7998 of 8000 bit-identical |
| B: boxes moved between steps | static world geom via `geom_pos`: not seen. Mocap box via `mocap_pos`: contact correct. Mocap box resized: wrong until `geom_rbound` and `geom_aabb` are written, then correct. |
| F: ground friction changed at run time (priority 1) | μ 0.9 → 9.6 m/s² (start transient), 0.2 → 1.80 (expected 1.96), 0.5 → 4.76 (expected 4.91) |
| C: step cost, 20-inch wheel of 48 capsules on a carriage, 5 m/s | plane 2.8 µs; hfield 0.25 m 14.5 µs; 3.125 cm 21–25 µs (window 10, 12 or 16 m: no difference); 2 cm (today's scenario strip) 34.5 µs; 1.5625 cm 37–43 µs |
| C: recentre cost, stand-in terrain | 3.125 cm, 12 m: 0.26–0.41 ms; 1.5625 cm, 10 m: 1.1 ms |
| D: 12 cm curb at 2.5 m/s, straight on | reference box, hfield only, hfield + box: same outcome (min vx −0.75 m/s, peak vertical acceleration 117–125 m/s²) |
| D: the same curb at 30° | reference box: lateral push 11 m/s², drift 2.3 cm. Hfield only: 0 m/s², no drift (wrong). Hfield + box: 12 m/s², 2.3 cm (matches). |
| G: euc-world sampler | 164–229 ns per sample; full 385² window 24–34 ms warm, 72–80 ms with chunk generation; a 1 m strip 1.7–2.1 ms |
| G: chunk generation | 0.94 ms per chunk through `World` (256 m × 256 m); 29 ms for a lone `Chunk::generate` (no tile cache) |
| G: the rig on the generated spawn street, 6 m/s, 7 s | 36 recentres at 4.0 ms each (strip sampling and chunk generation on demand); 29.8 µs per step including recentres; axle stayed within 1.0 mm of ground + radius |

Conclusions: the mocap window and the mocap box pool work. Use 1/32 m cells: 1/64 m doubles the contact cost for no visible gain once boxes carry the faces. Boxes are needed for oblique faces. Prefetch strips off the step thread: on demand they cost 4 ms per recentre.

### Budgets

- Physics: at most 30 µs per step on average for ground contact and world upkeep together (2 kHz, about 6 % of real time).
- On the step thread, a recentre is a data copy: at most 0.5 ms. Strips and chunks come from the worker.
- Chunk generation: at most 2 ms amortized (0.94 ms measured), off the step thread.

## Rendering plan

- The engine sends chunk payloads, not meshes from the UI thread: per chunk and level of detail, a height grid (f32, relative to a chunk base), a material grid (u8 codes), and lists of props, solids and paint.
- Meshes are built in a Web Worker into `BufferGeometry` (positions, normals, material attribute) and handed over as transferable buffers.
- Rings around the wheel: ring 0 (2 chunks, 0.125 m grid plus the solids' vertical faces as quads), ring 1 (to 160 m, 0.5 m grid), ring 2 (to 600 m, 2 m grid from `terrain_at` and a street mask), then fog. Skirts hide cracks between rings.
- Props and buildings are `InstancedMesh` per kind: lamp posts, trees (trunk plus canopy), benches, bins, hydrants, bollards; buildings as instanced boxes with a facade shader.
- Materials: physically based standard materials per `Material` with small tiling textures generated at start-up, within the [studio design system](../../app/DESIGN.md) (black background, dark UI). Wet patches darken and gain gloss.
- A floating origin: the scene origin follows the wheel in 256 m steps, so f32 positions keep millimetre precision far from the spawn.
- Reduced-quality mode (CPU rasterizer) drops to ring 0 and ring 1 at half resolution, without textures.

## Recordings and replay

- The recording context stores the `WorldSpec` (seed, revision, biome, densities), the route if any, and the digests of the chunks the run visited. It never stores meshes or height grids.
- Replay regenerates the world. A revision other than the running generator's is refused with a clear message; a chunk digest mismatch is reported as world drift.
- Recordings stay small: the context grows by a few hundred bytes plus 8 bytes per visited chunk.

## Scenario format

```toml
[world]
seed = 12345
biome = "city"
revision = 1               # must match the generator; recorded runs keep theirs
route = [[0.0, 0.0], [120.0, 0.0], [120.0, 64.0]]   # optional waypoints for the autopilot

[world.densities]          # optional; defaults as above
potholes = 3.0
plazas = 0.3
```

A scenario has either `[terrain]` (today's 1-D strip) or `[world]`, never both.

## Cross-platform determinism

- Generation is bit-identical on macOS, Linux and Windows by construction (the arithmetic rules above). The golden digest checks it on every platform that runs the tests.
- f32 hfield values are exact (quantized heights, power-of-two range), so the physics receives the same ground everywhere.
- MuJoCo itself is not guaranteed bit-identical across CPUs and compilers. That is today's situation too: a replay is bit-identical on the same build and platform; across platforms the world is identical and the physics can drift at round-off level.

## Hooks (designed, not built)

- **Biomes:** `Biome` selects the layer set. Park (paths, grass, benches, gentle hills), trail (dirt single track, roots, rocks, steep grades), mountain road (switchbacks, guard rails, 10–15 % grades), industrial (wide concrete, rail crossings, loading docks). Biomes blend by a low-frequency field across region borders.
- **Weather:** a `Weather` input (rain intensity, wind vector, temperature). Rain makes every surface wet (the wet columns above) and grows puddles in potholes and low spots; wind is an external force on rider and wheel; temperature feeds battery and tire models.
- **Obstacles and traffic:** static obstacles reuse the solid pool (parked cars, bins, construction barriers). Moving traffic is kinematic mocap bodies on lane centrelines from the road graph, with simple yielding at intersections.
- **Autopilot rider:** follows a route over `edge()` centrelines with a look-ahead point, picks a speed from curvature and surface, and sees the ground through a reaction time (perception delay plus the rider's existing command delay).
- **Campaign runs:** ride N km over M seeds per firmware build; count incidents (falls, cutoffs, alarms, tilt-backs) per km; keep a recording for each incident; compare firmware builds by incidents per km with the same seeds.
- **Real-world import:** OpenStreetMap ways become road edges with real widths and classes; an elevation model (SRTM or a city DEM) replaces the relief. The owner's ride logs (GPS and speed) become routes and validation targets. The sampler interface stays the same.

## Preview

```text
cargo run -p euc-world --features preview --release --bin euc-world-preview -- \
    --seed 42 --area -128,-128,128,128 --px-per-m 4 --out world.png [--grid] [--print]
```

Shaded relief (light from the north-west, relief exaggerated 3×) over surface colours: asphalt dark grey, concrete light grey, pavers tan, cobblestones brown-grey, grass green, dirt brown, gravel beige, paint white, metal blue-grey, wet patches blue.
Curb stones, stair steps (white outlines), ramps and plaza platforms are drawn from their solids, buildings as slate roofs, props as dots (lamp posts yellow, hydrants red, bollards black, benches and bins as their boxes, trees as canopies). The spawn is a magenta ring. `--grid` draws chunk borders; `--print` lists counts and every plaza with its steps and ramp.
The `preview` feature adds two small dependencies (`miniz_oxide` for deflate, `crc32fast` for PNG chunks); the library has only `serde`.

## Tests

`cargo test -p euc-world` (about 21 s in a debug build, 4 s in release):

- the same seed gives identical chunks, another seed does not;
- chunks do not depend on generation order (standalone generation vs a cached world in scrambled order);
- neighbouring chunks agree bit for bit on their border (drawn and physics heights, zones, surfaces);
- `fill_heights` matches `ground_at`;
- every jump of more than 4.5 cm over 1 cm, drawn or physics, lies under a solid (scan lines over 140 m × 140 m, two seeds);
- curb boxes agree with the sampler: face on the curb line, top on the sidewalk edge, physics step behind the face;
- the spawn guarantees, over 12 seeds;
- feature sizes within their limits (potholes, manholes, bumps, plaza steps, risers, treads, ramp slopes, curbs);
- densities follow their parameters; the network is connected; hills stay under 12 %;
- a golden digest of the chunks around the spawn.

## Known limits

- The road field takes the cross slope from the terrain: on the steepest hills a street can tilt sideways up to the terrain's grade.
- Cracks (1–2 cm wide) are narrower than a physics cell; the height field catches them as scattered one-cell dips.
- Plazas face their anchor's nearest street; on a strongly curved street the plaza sits at an angle to the curb.
- Street paving changes at intersections along a line where the nearer street changes; an arterial wins inside the crossing itself.
- Only the city biome exists.

## W1b order of work

1. `euc-physics`: a `Ground::World(WorldSpec)` variant; MJCF with the mocap window (385², 1/32 m, range 4 m, priority 1) and the 128-box mocap pool; a between-steps world tick (recentre, pool refresh, friction under the wheel) at a fixed cadence; acceptance tests on curbs (straight and oblique), potholes, stairs, ramps and the spawn.
2. A prefetch worker for strips and chunks, with a blocking hand-off so timing never changes results; then check the 30 µs per step budget.
3. `euc-sim` recordings: store the `WorldSpec`, revision, route and visited chunk digests; replay regenerates and refuses a revision mismatch.
4. Scenario `[world]` table and `euc sim run` support; an `euc world preview` wrapper around the preview binary.
5. Studio: chunk payloads over the engine channel, the worker mesh builder, LOD rings, instanced props and buildings, materials, floating origin, and the reduced-quality path.
6. Free Ride: seed and density controls, a "new world" action, a minimap from the preview colours; remove the 32 m edge pause.
7. CI: the golden digest on Linux and Windows runners, so cross-platform identity is checked, not only designed.
