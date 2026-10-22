# Recordings (`.eucrec`)

A recording holds one run of the simulator: every telemetry sample, the pose of every body, the
timeline of events, the BLE frames the firmware sent (decoded) and the board's outputs (display,
LEDs, buzzer). It is one file you can inspect from the command line, replay in the studio, or load in
Python. It never holds the firmware image: the header names it and gives its sha256, and nothing is
read from code flash. A recording does contain the firmware's UART/BLE output, including any text it
prints: the BLE frames are what the firmware sent, as a phone receives them (telemetry frames and text
such as the version banner and the boot log).

## Making one

```sh
# A scenario ride on our clean build (or --firmware stock, compat, a .bin or an .elf):
cargo run --release -p euc-cli -- sim run --scenario scenarios/ride-20kmh-stop.toml \
    --firmware clean --record ride.eucrec
```

`--record` needs `--scenario` (a run without one has no live plant to sample). `--sample-hz` sets the
samples per simulated second (100 by default). The studio captures each new run automatically when no unsaved capture exists.
Use the Recordings button to save or open a file (see [the studio](#in-the-studio)). In Python, `Session.save(path)` writes the
run so far ([python/README.md](../python/README.md)).

Before this format, `--record` wrote an `.eucr` file (a JSON header, then raw telemetry batches and the
events at the end). Nothing read that format, so it was replaced, not kept: `--record` now writes
`.eucrec`, and `euc rec info` refuses an `.eucr` file as "not an .eucrec recording".

## Looking inside

```sh
euc rec info ride.eucrec              # header, length, channels, bodies, frames, events
euc rec info ride.eucrec --events 100 # list more events
euc --json rec info ride.eucrec       # everything as JSON, all events
euc rec export ride.eucrec --csv out/ # CSV files, below
```

`rec export` writes one CSV per channel group, each with a `t_s` column and one column per channel
(empty where a channel is not modelled):

| File | Channels |
|---|---|
| `motion.csv` | `speed_kmh`, `wheel_rad_s`, `distance_m`, `target_speed_kmh` |
| `drive.csv` | `pwm_pct`, `modulation_pct`, `iq_a`, `torque_nm`, `phase_a_a`, `phase_b_a`, `phase_c_a` |
| `battery.csv` | `battery_v`, `battery_a`, `soc_pct` |
| `attitude.csv` | `pitch_deg`, `roll_deg`, `rider_lean_deg` |
| `thermal.csv` | `board_temp_c`, `motor_temp_c` |
| `suspension.csv` | `suspension_mm`, `shock_mm` |
| `firmware.csv` | `isr_load_pct` |
| `other.csv` | channels this build does not group (from a later build) |

and `channels.csv` (key, label, unit, group, id), `poses.csv` (`<body>_x ... <body>_qz` per sample),
`events.csv`, `frames.csv` (time, kind, reply, summary, bytes in hex) and `outputs.csv`.

## In the studio

Open **Recordings** from the side rail or command palette.

- **Capture** shows the current segment, sample count, and stop reason.
  The app captures into its private cache before the lossy display queue.
  **Stop and save** writes a new .eucrec file in Downloads.
  An existing file is never overwritten.
- The app keeps an unsaved segment when a run ends or restarts.
  Save that segment before starting another capture.
  **Start capture** records future samples only.
- **Playback** opens a selected .eucrec file.
  A valid file replaces the displayed data source and pauses the live simulator.
  Play, pause, step, seek, and speed operate on the saved samples.
  **Return to simulation** restores the paused live run.
- The reader restores body poses, events, phone frames, and board outputs.
  A saved Free Ride map supplies playback ground.
  Missing tracks stay unavailable.
  Buzzer frequency changes are stored; precise audio playback is not implemented.

Both scenario and Free Ride captures store the actual rider model and external aids.
These fields describe model configuration, not measured human behavior.

Desktop limits are 32 MiB per file, 600 seconds, 72,000 samples, and 2,000,000 stored signal and pose values.
Unknown channels also count toward the value limit.
Header, chunk, sparse-track, and item limits apply before decoding allocates their payloads.
Duplicate index offsets and inconsistent chunk counts are invalid.
A truncated file retains complete valid chunks and reports the missing tail.

The app rebuilds display, tail-light, and shock geometry from the built-in wheel profile.
Scenario terrain comes from the saved scenario text.
These visual layouts are not exact historical profile snapshots.
Scenario reconstruction stops at 10 km and 128 terrain features. Larger geometry remains unavailable.

## The format

The definition is `crates/euc-telemetry/src/recording/mod.rs`; this is a summary. All integers and
floats are little-endian.

```text
file    = preamble, header, chunk*, [trailer]
preamble: 8 bytes "EUCREC\r\n", u16 format version (1), u16 flags (0), u32 header length
header  : UTF-8 JSON
chunk   : 40-byte chunk header, payload
trailer : u64 offset of the index chunk, 8 bytes "EUCREND\n"
```

**Header** (JSON): `schema` (channel schema version), `producer` (`euc 0.1.0`, `EUC Studio 0.1.0`,
`eucsim 0.1.0`), `started_unix_ms`, `git_rev` (the engine's commit), `wheel` and `profile_sha256` (of
the profile files the engine reads), `firmware` and `firmware_sha256`, `cpu` and `cpi`, `seed` (of the
simulated sensor noise), `scenario` and `scenario_toml` (the file as it ran), `settings` (the preset
the wheel booted with; null for a blank journal), `sample_hz`, `channels` (id, key, label, unit, rate:
the sample columns in order) and `bodies` (name, role and shapes: the poses in order).

**Chunk header** (40 bytes): `u8 kind`, `u8 codec` (0 stored, 1 zstd), `u16` 0, `u32 count` (samples,
items or index entries), `u32 stored` (payload bytes in the file), `u32 raw` (after decompression),
`f64 t0` and `f64 t1` (simulated seconds of the first and last sample or item), `u32 crc32` of header
bytes 0..32 and then of the stored payload, `u32` 0.

**Chunk kinds**:

| Kind | Payload (after decompression) |
|---|---|
| 1 samples | `u32 n, u32 c, u32 b, u32 0`, then `f64[n]` times, `f32[c][n]` one column per header channel, `f32[n][b][7]` poses (x y z qw qx qy qz) |
| 2 events | JSON array of `{t_s, kind, text}` |
| 3 BLE frames | JSON array of `{t_s, kind, summary, fields, bytes, reply}` |
| 4 outputs | JSON array of `{t_s, outputs: {display_grid, display_on, display_brightness, leds, buzzer_hz}}` |
| 127 index | 32 bytes per chunk: `u8 kind, 3 bytes 0, u32 count, u64 offset, f64 t0, f64 t1` |

**Keyframes.** A samples chunk covers at most one simulated second and decodes on its own, so a reader
seeks by finding the last samples chunk that starts at or before the time it wants (the index has
every chunk's offset and time span) and decoding only that one. The events, frames and outputs of the
same second follow it. The writer flushes the file after every chunk.

**Truncated files.** A finished file ends with the index chunk and the trailer. A file whose writer
stopped (a crash, a killed run) has neither; a reader then walks the chunks from the start and keeps
every whole chunk whose crc checks, so everything up to the last whole second reads normally, and it
says that the file is truncated.

**Changes without a new version.** Readers skip chunk kinds they do not know, keep channels they do not
know as plain columns (a channel's id is stable: ids are append-only), read channels the file lacks as
NaN, skip events of a kind they do not know, and keep header fields they do not know. Only a change
that breaks those rules raises the format version; a reader refuses a version newer than its own.

## Reading one in Rust

```rust
use euc_telemetry::recording::Reader;

let mut reader = Reader::open(path)?;
println!("{} samples, {:?} truncated", reader.sample_count(), reader.truncated());
let at_12s = reader.sample_at(12.0)?;            // the last sample at or before 12 s
let window = reader.samples_in(10.0, 20.0)?;     // columns for 10 s < t <= 20 s
let events = reader.events();
```

`euc_telemetry::recording::Writer` writes one; `euc_sim::recording::Recorder` records a `Session`
(samples, its events, decoded BLE frames, and board output changes).
