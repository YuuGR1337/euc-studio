//! The README showcase video's runs (`video/`): real runs of the real engine and stock firmware,
//! kept the way the studio keeps them, for the studio's own views to replay in a headless browser.
//!
//! For each run of the shot list (`video/shots.json`; its path in `EUC_SHOWCASE`) it writes to
//! the folder in `EUC_SHOWCASE_OUT`:
//! - `<id>.eucrec`: the studio's capture of the run, fed from the runner's own sample loop;
//! - `<id>.json`: what the studio's playback opens (`recordings::read_recording`);
//! - `<id>.wav`: the run's sound through the native scheduler, offline, 48 kHz mono from the first
//!   sample: the firmware's buzzer. The ride bed is off as in the studio (`RIDE_SOUND`); with it
//!   on, a free ride's bed is its own (`Host::ride_sound`) and a scenario's (it has none in the
//!   studio) the same mapping (`free_ride::ride_sound`) with the world's surface under the wheel.
//!
//! A run on a world also writes `<id>.world.bin` and `<id>.world.json`: every chunk payload the
//! high-quality view asks for along the path (`euc_world::render`, one block per chunk, without
//! the payload header) and where each block is, so the browser can answer `world_chunks`.
//!
//! A comparison writes `<id>.compare.json`: the workspace's setup answer (`compare_info`) and
//! everything its channels carry (`compare::record_stream`), each batch with its last sample's
//! time, for the browser to replay at that pace.
//!
//! Test-only and ignored; `video/scripts/make.sh` runs it.

use std::fs;
use std::path::{Path, PathBuf};

use euc_profile::ProfileSource;
use euc_telemetry::{Event, Snapshot};
use serde::Deserialize;
use serde_json::json;

use super::compare::{self, CompareSpec, Sent};
use super::free_ride::{self, FreeRideSpec, FreeRideState, InputSource, ManualInput, WorldChoice, WorldPose};
use super::host::Host;
use super::native_audio::{self, Ride, Tone};
use super::recordings::{self, RecordingState, RunInfo};
use super::runner::Built;

/// The runner's step and sampling (`runner.rs`): 1/120 s steps, a sample every second step.
const STEP_S: f64 = 1.0 / 120.0;
const SNAPSHOT_EVERY: u32 = 2;
const EPOCH: u64 = 1;
const WAV_RATE: u32 = 48_000;
/// The Free Ride view sends its input every 50 ms (`FREE_RIDE_LIMITS.heartbeatMs`).
const HEARTBEAT_STEPS: u32 = 6;
/// The ride bed as the studio ships it: off ("Ride sound (experimental)"). The video carries the
/// firmware's beeps; the bed is still computed, for when the ride sound is good enough.
const RIDE_SOUND: bool = false;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Shots {
    runs: Vec<RunShot>,
    #[serde(default)]
    compares: Vec<CompareShot>,
    /// What the browser films of these runs (`video/capture/capture.mjs`); not read here.
    #[serde(default, rename = "captures")]
    _captures: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RunShot {
    /// Free Ride as the studio starts it (stock firmware, the neutral preset, a city world), driven
    /// by a script of drive-mode inputs.
    FreeRide {
        id: String,
        rider_model: String,
        seed: u64,
        #[serde(default)]
        start: Option<WorldPose>,
        /// Simulated seconds after the boot.
        seconds: f64,
        inputs: Vec<Input>,
    },
    /// A shipped scenario, as the Ride workspace runs it.
    Scenario { id: String, scenario: String, firmware: String },
}

/// From `at_s` (seconds after the boot) to the next input: the throttle as the studio's drive
/// controls give it (-1 full brake, 0 coast, 1 full throttle), the turn rate and the crouch.
#[derive(Deserialize, Clone, Copy, Default)]
#[serde(deny_unknown_fields)]
struct Input {
    at_s: f64,
    throttle: f64,
    #[serde(default)]
    turn_rad_s: f64,
    #[serde(default)]
    crouch: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompareShot {
    id: String,
    /// What the workspace's Run sends (`compare::CompareSpec`).
    spec: serde_json::Value,
}

/// One run's capture and sound, sample by sample, as the runner feeds them.
struct Capture {
    recordings: RecordingState,
    snapshot: Snapshot,
    events: Vec<Event>,
    ble: Vec<(f64, u8)>,
    sound: Vec<(f64, Vec<Tone>, Option<Ride>)>,
    steps: u32,
    /// The world the run rides, and the wheel's path on it (physics frame).
    world: Option<euc_physics::WorldGround>,
    path: Vec<[f64; 2]>,
}

impl Capture {
    /// Binds the run as `runner.rs` does: the built header at the runner's sample rate, and a
    /// free ride's context (its world) before the header's.
    fn bind(built: &Built, context: Option<serde_json::Value>, scratch: &Path) -> Self {
        let recordings = RecordingState::default();
        recordings.enable_autocapture(scratch.to_owned()).unwrap();
        let mut header = built.header.clone().expect("a recordable run");
        header.sample_hz = 1.0 / (STEP_S * f64::from(SNAPSHOT_EVERY));
        let context = context.or_else(|| header.context.clone());
        let world = context
            .as_ref()
            .map(|c| euc_sim::recording::RecordingContext::from_json(c).unwrap())
            .and_then(|c| c.world)
            .map(|w| w.ground().unwrap());
        recordings.bind_run(RunInfo { header, context }).unwrap();
        Self {
            recordings,
            snapshot: Snapshot::default(),
            events: Vec::new(),
            ble: Vec::new(),
            sound: Vec::new(),
            steps: 0,
            world,
            path: Vec::new(),
        }
    }

    /// One runner step, and a sample every `SNAPSHOT_EVERY` steps (or at the end).
    fn step(&mut self, host: &mut dyn Host, bed: &dyn Fn(&dyn Host, &Snapshot) -> Option<Ride>) {
        host.step(STEP_S);
        self.steps += 1;
        let end = host.completed() || host.halted().is_some();
        if self.steps.is_multiple_of(SNAPSHOT_EVERY) || end {
            host.snapshot(&mut self.snapshot);
            self.events.clear();
            host.drain_events(&mut self.events);
            self.ble.clear();
            host.drain_ble_tx(&mut self.ble);
            let mut tones = Vec::new();
            host.drain_tones(&mut tones);
            let ride = bed(&*host, &self.snapshot);
            self.recordings.ingest(&self.snapshot, &self.events, &self.ble, host.outputs().as_ref());
            self.sound.push((self.snapshot.t_s, tones, ride));
            if let Some(wheel) = self.snapshot.bodies.first() {
                self.path.push([f64::from(wheel.pos[0]), f64::from(wheel.pos[1])]);
            }
        }
    }

    /// Writes `<id>.eucrec`, `<id>.json`, `<id>.wav` and, on a world, its chunks to `out`.
    fn save(self, out: &Path, id: &str) {
        let path = out.join(format!("{id}.eucrec"));
        let _ = fs::remove_file(&path);
        recordings::save_capture(&self.recordings, &path).unwrap();
        let data = recordings::read_recording(&fs::read(&path).unwrap()).unwrap();
        fs::write(out.join(format!("{id}.json")), serde_json::to_vec(&data).unwrap()).unwrap();
        let pcm = native_audio::render_offline(&self.sound, RIDE_SOUND).unwrap();
        write_wav(&out.join(format!("{id}.wav")), &pcm);
        if let Some(ground) = &self.world {
            export_world(out, id, ground, &self.path);
        }
        let peak = pcm.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
        println!(
            "showcase {id}: {} samples, {:.2}-{:.2} s, sound {:.2} s, peak {peak:.3}",
            self.sound.len(),
            self.sound.first().map_or(0.0, |s| s.0),
            self.sound.last().map_or(0.0, |s| s.0),
            pcm.len() as f64 / f64::from(WAV_RATE)
        );
    }
}

/// The view's rings at high quality (`RINGS` in `worldStream.ts`): chunk centres within 30 m of
/// the wheel at level 0, within 104 m at level 1; with a chunk's margin.
const RINGS: [(usize, f64); 2] = [(0, 30.0 + euc_world::CHUNK_M), (1, 104.0 + euc_world::CHUNK_M)];

fn export_world(out: &Path, id: &str, ground: &euc_physics::WorldGround, path: &[[f64; 2]]) {
    let mut wanted = std::collections::BTreeSet::new();
    for p in path.iter().step_by(15) {
        let [x, y] = ground.to_world(*p);
        for (lod, radius) in RINGS {
            let reach = (radius / euc_world::CHUNK_M).ceil() as i32;
            let home = euc_world::ChunkKey::of([x, y]);
            for cy in home.cy - reach..=home.cy + reach {
                for cx in home.cx - reach..=home.cx + reach {
                    let centre =
                        [(f64::from(cx) + 0.5) * euc_world::CHUNK_M, (f64::from(cy) + 0.5) * euc_world::CHUNK_M];
                    if (centre[0] - x).hypot(centre[1] - y) <= radius {
                        wanted.insert((lod, cx, cy));
                    }
                }
            }
        }
    }
    let world = ground.world();
    let (mut blocks, mut index) = (Vec::new(), serde_json::Map::new());
    for &(lod, cx, cy) in &wanted {
        let payload = euc_world::render::payload(world, &[[cx, cy]], lod);
        let block = &payload[12..];
        index.insert(format!("{lod}/{cx}/{cy}"), json!([blocks.len(), block.len()]));
        blocks.extend_from_slice(block);
    }
    let start = ground.start();
    let frame = json!({
        "spec": ground.spec(),
        "start_xy_m": start.xy_m,
        "start_heading_rad": start.heading_rad,
        "origin_z_m": ground.origin_z(),
    });
    fs::write(out.join(format!("{id}.world.bin")), &blocks).unwrap();
    fs::write(
        out.join(format!("{id}.world.json")),
        serde_json::to_vec(&json!({ "frame": frame, "chunks": index })).unwrap(),
    )
    .unwrap();
    println!("showcase {id}: {} world chunks, {:.1} MB", wanted.len(), blocks.len() as f64 / 1e6);
}

/// 16-bit PCM, mono.
fn write_wav(path: &Path, pcm: &[f32]) {
    let data = u32::try_from(pcm.len() * 2).unwrap();
    let mut bytes = Vec::with_capacity(44 + pcm.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&WAV_RATE.to_le_bytes());
    bytes.extend_from_slice(&(WAV_RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for x in pcm {
        bytes.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    fs::write(path, bytes).unwrap();
}

fn free_ride(out: &Path, scratch: &Path, id: &str, shot: (&str, u64, Option<WorldPose>, f64, &[Input])) {
    let (rider_model, seed, start, seconds, inputs) = shot;
    let state = FreeRideState::default();
    let spec = FreeRideSpec {
        wheel: "begode-extreme".into(),
        settings: "neutral".into(),
        rider_model: rider_model.into(),
        map: None,
        world: Some(WorldChoice { seed, densities: Default::default() }),
        start,
    };
    let built = super::firmware::build_free_host(&spec, "stock", EPOCH, state.clone(), &mut |_| {}).unwrap();
    let mut capture = Capture::bind(&built, state.recording_context(EPOCH).unwrap(), scratch);
    let mut host = built.host;
    let bed = |host: &dyn Host, snapshot: &Snapshot| host.ride_sound(snapshot);
    while !state.status().ready {
        assert!(host.halted().is_none(), "{id}: halted while booting");
        capture.step(host.as_mut(), &bed);
    }
    let booted = host.time_s();
    let (mut seq, mut steps) = (0, 0);
    while host.time_s() - booted < seconds {
        let t = host.time_s() - booted;
        if steps % HEARTBEAT_STEPS == 0 {
            let input = inputs.iter().rev().find(|i| i.at_s <= t).copied().unwrap_or_default();
            let status = state.status();
            seq += 1;
            state
                .input(ManualInput {
                    run_epoch: EPOCH,
                    seq,
                    source: InputSource::Gamepad,
                    desired_lean_rad: 0.0,
                    heading_rate_rad_s: input.turn_rad_s,
                    armed: false,
                    crouch: input.crouch,
                    desired_accel_mps2: Some(drive_ask(
                        input.throttle,
                        status.accel_cap_mps2.unwrap(),
                        status.brake_cap_mps2.unwrap(),
                    )),
                })
                .unwrap();
        }
        steps += 1;
        capture.step(host.as_mut(), &bed);
        assert!(host.halted().is_none(), "{id}: halted at {t:.2} s");
        assert!(state.status().on_pedals, "{id}: the rider came off at {t:.2} s");
    }
    capture.save(out, id);
}

/// The studio's throttle to drive ask (`driveAsk` in `lib/rideControls.ts`): full brake at -1,
/// the coast between the caps at 0, full throttle at 1.
fn drive_ask(throttle: f64, accel_cap: f64, brake_cap: f64) -> f64 {
    let (lean, coast) = ((accel_cap + brake_cap) / 2.0, (accel_cap - brake_cap) / 2.0);
    (throttle.clamp(-1.0, 1.0) * lean + coast).clamp(-12.0, 12.0)
}

fn scenario(out: &Path, scratch: &Path, id: &str, name: &str, firmware: &str) {
    let built = super::firmware::build_host(name, firmware, &mut |_| {}).unwrap();
    // The world under the wheel, for the bed a free ride on this ground would play.
    let context = built.header.as_ref().and_then(|h| h.context.as_ref());
    let ground = context
        .map(|c| euc_sim::recording::RecordingContext::from_json(c).unwrap())
        .and_then(|c| c.world)
        .map(|w| w.ground().unwrap());
    let files = euc_sim::WheelFiles::load(&ProfileSource::Builtin, &built.wheel).unwrap();
    let pole_pairs = files.electrical.motor.pole_pairs;
    let mut capture = Capture::bind(&built, None, scratch);
    let mut host = built.host;
    let bed = |_: &dyn Host, snapshot: &Snapshot| {
        let wheel = snapshot.bodies.first()?;
        let surface = ground.as_ref().map(|g| g.surface([f64::from(wheel.pos[0]), f64::from(wheel.pos[1])]));
        free_ride::ride_sound(snapshot, pole_pairs, surface)
    };
    while !host.completed() && host.halted().is_none() {
        capture.step(host.as_mut(), &bed);
    }
    capture.save(out, id);
}

fn comparison(out: &Path, id: &str, request: &serde_json::Value) {
    let spec: CompareSpec = serde_json::from_value(request.clone()).unwrap();
    let sent = compare::record_stream(&spec);
    let mut items = Vec::new();
    let (mut a, mut b) = (0, 0);
    for item in &sent {
        items.push(match item {
            Sent::A(bytes) | Sent::B(bytes) => {
                let batch = euc_telemetry::batch::decode(bytes).unwrap();
                let side = if matches!(item, Sent::A(_)) {
                    a += batch.t.len();
                    "a"
                } else {
                    b += batch.t.len();
                    "b"
                };
                json!({ "side": side, "t_s": batch.t.last(), "bytes": bytes })
            }
            Sent::Message(message) => json!({ "message": message }),
        });
    }
    let file = json!({ "spec": request, "info": compare::compare_info(), "items": items });
    fs::write(out.join(format!("{id}.compare.json")), serde_json::to_vec(&file).unwrap()).unwrap();
    println!("showcase {id}: {} items, {a} samples on A, {b} on B", items.len());
}

fn paths() -> (Shots, PathBuf, PathBuf) {
    let list = std::env::var("EUC_SHOWCASE").expect("EUC_SHOWCASE: the shot list");
    let out = PathBuf::from(std::env::var("EUC_SHOWCASE_OUT").expect("EUC_SHOWCASE_OUT: the output folder"));
    let shots = serde_json::from_str(&fs::read_to_string(&list).unwrap()).unwrap();
    let scratch = out.join("scratch");
    fs::create_dir_all(&scratch).unwrap();
    (shots, out, scratch)
}

/// Every run and comparison in the shot list, or only the one named in `EUC_SHOWCASE_ONLY`.
#[test]
#[ignore = "README showcase: needs the cached stock firmware (and our compat build for a comparison)"]
fn showcase_runs() {
    let (shots, out, scratch) = paths();
    let only = std::env::var("EUC_SHOWCASE_ONLY").ok();
    let wanted = |id: &str| only.as_deref().is_none_or(|o| o == id);
    for shot in &shots.runs {
        match shot {
            RunShot::FreeRide { id, rider_model, seed, start, seconds, inputs } if wanted(id) => {
                free_ride(&out, &scratch.join(id), id, (rider_model, *seed, *start, *seconds, inputs));
            }
            RunShot::Scenario { id, scenario: name, firmware } if wanted(id) => {
                scenario(&out, &scratch.join(id), id, name, firmware);
            }
            _ => {}
        }
    }
    for shot in shots.compares.iter().filter(|c| wanted(&c.id)) {
        comparison(&out, &shot.id, &shot.spec);
    }
}
