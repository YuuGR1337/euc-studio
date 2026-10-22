//! Recordings (`.eucrec`): a run's samples, body poses, events, BLE frames and board outputs in
//! one seekable file. `docs/recordings.md` describes the format for people; this is the
//! definition.
//!
//! ```text
//! offset  size   field
//!      0  8      magic "EUCREC\r\n"
//!      8  u16    format version (1)
//!     10  u16    flags (0)
//!     12  u32    h: header length
//!     16  h      header: UTF-8 JSON (`Header`)
//!         ...    chunks, back to back
//!         16     trailer of a finished file: u64 offset of the index chunk, then "EUCREND\n"
//! ```
//!
//! Every chunk starts with a 40-byte header:
//!
//! ```text
//!      0  u8     kind: 1 samples, 2 events, 3 BLE frames, 4 board outputs, 127 index
//!      1  u8     codec: 0 stored, 1 zstd
//!      2  u16    reserved (0)
//!      4  u32    count: samples, items or index entries
//!      8  u32    stored: payload bytes in the file
//!     12  u32    raw: payload bytes after decompression
//!     16  f64    t0: simulated seconds of the first sample or item
//!     24  f64    t1: of the last one
//!     32  u32    crc32 (IEEE) of header bytes 0..32, then of the stored payload
//!     36  u32    reserved (0)
//!     40  stored payload
//! ```
//!
//! Raw payloads:
//!
//! - **samples** (a keyframe): `u32 n, u32 c, u32 b, u32 0`, then `f64[n]` times, `f32[c][n]`
//!   one column per header channel, `f32[n][b][7]` poses (x y z qw qx qy qz per body). A samples
//!   chunk covers about one simulated second ([`KEYFRAME_S`]) and decodes on its own, so a reader
//!   seeks by decoding one chunk.
//! - **events**, **BLE frames**, **board outputs**: JSON arrays of [`Event`], [`BleFrame`] and
//!   [`OutputsAt`], written next to the samples chunk of the same second.
//! - **index**: 32 bytes per chunk: `u8 kind, 3 bytes 0, u32 count, u64 offset, f64 t0, f64 t1`.
//!
//! All integers and floats are little-endian. A reader skips chunk kinds it does not know, keeps
//! channels it does not know as plain columns and ignores header fields it does not know, so
//! additions need no new format version. A file without a valid trailer (the writer stopped) is
//! read up to its last whole chunk.
//!
//! A recording holds what the simulator measured and the firmware sent, never firmware code: the
//! header names the image and its sha256, and no chunk kind carries flash contents.

mod read;
mod write;

use serde::{Deserialize, Serialize};

use crate::model::{BodyInfo, Outputs};
use crate::schema::{CHANNELS, RateClass};

pub use read::{ReadError, ReadLimits, Reader, Samples};
pub use write::Writer;

/// First eight bytes of a recording.
pub const MAGIC: [u8; 8] = *b"EUCREC\r\n";
/// Last eight bytes of a finished recording.
pub const END_MAGIC: [u8; 8] = *b"EUCREND\n";
/// Format version this module writes and reads.
pub const FORMAT_VERSION: u16 = 1;
/// Version of the channel schema the header's channel list follows (ids are append-only).
pub const SCHEMA_VERSION: u32 = 1;
/// Simulated seconds a samples chunk covers at most: the keyframe interval.
pub const KEYFRAME_S: f64 = 1.0;
/// File extension, without the dot.
pub const EXTENSION: &str = "eucrec";

const PREAMBLE_BYTES: usize = 16;
const CHUNK_HEADER_BYTES: usize = 40;
const TRAILER_BYTES: usize = 16;
const INDEX_ENTRY_BYTES: usize = 32;
/// The most a chunk may claim (a guard against corrupt lengths).
const MAX_CHUNK_BYTES: u32 = 256 << 20;
/// The most a header may claim.
const MAX_HEADER_BYTES: u32 = 16 << 20;
/// Floats per pose.
const POSE_FLOATS: usize = 7;

/// What a chunk holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkKind {
    /// Samples and poses: a keyframe.
    Samples,
    /// Events.
    Events,
    /// Decoded BLE frames.
    Frames,
    /// Board output changes.
    Outputs,
    /// The chunk index of a finished file.
    Index,
    /// A kind this reader does not know (skipped).
    Unknown(u8),
}

impl ChunkKind {
    const fn code(self) -> u8 {
        match self {
            Self::Samples => 1,
            Self::Events => 2,
            Self::Frames => 3,
            Self::Outputs => 4,
            Self::Index => 127,
            Self::Unknown(code) => code,
        }
    }

    const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Samples,
            2 => Self::Events,
            3 => Self::Frames,
            4 => Self::Outputs,
            127 => Self::Index,
            other => Self::Unknown(other),
        }
    }
}

/// A channel as the header lists it: the [`ChannelSpec`](crate::ChannelSpec) of the writer,
/// owned, so a reader keeps channels its own build does not know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelInfo {
    /// Stable id ([`crate::Channel`] discriminant).
    pub id: u16,
    /// `snake_case` key.
    pub key: String,
    /// Label for people.
    #[serde(default)]
    pub label: String,
    /// Unit symbol.
    #[serde(default)]
    pub unit: String,
    /// `fast` or `slow` (see [`RateClass`]); text, so a later class still reads.
    #[serde(default)]
    pub rate: String,
}

impl ChannelInfo {
    /// Every channel this build streams, in id order.
    pub fn all() -> Vec<Self> {
        CHANNELS
            .iter()
            .map(|s| Self {
                id: s.id,
                key: s.key.to_owned(),
                label: s.label.to_owned(),
                unit: s.unit.to_owned(),
                rate: match s.rate {
                    RateClass::Fast => "fast",
                    RateClass::Slow => "slow",
                }
                .to_owned(),
            })
            .collect()
    }
}

/// What a recording is of. Written once, as JSON, at the start of the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// Channel schema version ([`SCHEMA_VERSION`]).
    pub schema: u32,
    /// What wrote the file (`euc 0.1.0`, `EUC Studio 0.1.0`, `eucsim 0.1.0`).
    pub producer: String,
    /// Wall-clock start of the recording, milliseconds since the Unix epoch (UTC).
    pub started_unix_ms: u64,
    /// The engine's git commit, when the build knew it.
    #[serde(default)]
    pub git_rev: Option<String>,
    /// Wheel profile id.
    pub wheel: String,
    /// sha256 (hex) of the wheel profile's files, as the engine read them.
    #[serde(default)]
    pub profile_sha256: Option<String>,
    /// The firmware, for people: the stock build label or a file name.
    pub firmware: String,
    /// sha256 (hex) of the firmware image that ran. The image itself is never stored.
    #[serde(default)]
    pub firmware_sha256: Option<String>,
    /// CPU backend (`own`, `unicorn`).
    #[serde(default)]
    pub cpu: Option<String>,
    /// HCLK cycles per instruction the CPU model used, with the fixed timing.
    #[serde(default)]
    pub cpi: Option<f64>,
    /// Instruction timing (`fixed`, `flash`, `flash-wN`). Absent in recordings made before the
    /// flash model, which all used the fixed timing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<String>,
    /// Seed of the simulated sensor noise.
    #[serde(default)]
    pub seed: Option<u64>,
    /// Scenario name, if a scenario ran.
    #[serde(default)]
    pub scenario: Option<String>,
    /// The scenario file as it ran, so the run can be repeated.
    #[serde(default)]
    pub scenario_toml: Option<String>,
    /// Settings preset the wheel booted with; none for a blank settings journal.
    #[serde(default)]
    pub settings: Option<String>,
    /// Nominal samples per simulated second.
    pub sample_hz: f64,
    /// The sample columns, in order.
    pub channels: Vec<ChannelInfo>,
    /// The bodies, in pose order.
    #[serde(default)]
    pub bodies: Vec<BodyInfo>,
    /// The run's context (schema 1): `run_mode` (`scenario`, `free_ride`), `rider_model`,
    /// `model_aids` and, for a free ride, the `height_map` the run was on. Kept as JSON here:
    /// `euc_sim::recording::RecordingContext` defines and validates it, and a reader that does
    /// not know it still plays the samples. None on a run that recorded no context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
    /// Fields this build does not know, kept as they were.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Header {
    /// A header for `wheel` running `firmware`, with every channel of this build, no bodies and
    /// the start time now; fill in the rest.
    pub fn new(producer: impl Into<String>, wheel: impl Into<String>, firmware: impl Into<String>) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            producer: producer.into(),
            started_unix_ms: now_unix_ms(),
            git_rev: None,
            wheel: wheel.into(),
            profile_sha256: None,
            firmware: firmware.into(),
            firmware_sha256: None,
            cpu: None,
            cpi: None,
            timing: None,
            seed: None,
            scenario: None,
            scenario_toml: None,
            settings: None,
            sample_hz: 0.0,
            channels: ChannelInfo::all(),
            bodies: Vec::new(),
            context: None,
            extra: serde_json::Map::new(),
        }
    }

    /// The start time as `YYYY-MM-DDThh:mm:ssZ`.
    pub fn started_utc(&self) -> String {
        utc(self.started_unix_ms)
    }

    /// Column index of channel `key`.
    pub fn column(&self, key: &str) -> Option<usize> {
        self.channels.iter().position(|c| c.key == key)
    }
}

/// A decoded BLE segment the firmware sent: a telemetry frame or a text reply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BleFrame {
    /// Simulated seconds of its last byte.
    pub t_s: f64,
    /// `live`, `bms`, `cells`, `status`, `power`, `other` or `text`.
    pub kind: String,
    /// One line for people (`31.4 km/h, 128.20 V, 2.10 A`).
    pub summary: String,
    /// Decoded fields, in wire order.
    #[serde(default)]
    pub fields: Vec<(String, String)>,
    /// The segment as it went over the air.
    pub bytes: Vec<u8>,
    /// A reply to a phone command (`V`, `N`).
    #[serde(default)]
    pub reply: bool,
}

/// The board's outputs from `t_s` on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputsAt {
    /// Simulated seconds.
    pub t_s: f64,
    /// Display, LEDs and buzzer.
    pub outputs: Outputs,
}

/// Which CSV file (`euc rec export`) a channel goes to: related signals together, `other` for a
/// channel this build does not group.
pub fn channel_group(key: &str) -> &'static str {
    match key {
        "speed_kmh" | "wheel_rad_s" | "distance_m" | "target_speed_kmh" => "motion",
        "pitch_deg" | "roll_deg" | "rider_lean_deg" => "attitude",
        "pwm_pct" | "modulation_pct" | "iq_a" | "torque_nm" | "phase_a_a" | "phase_b_a" | "phase_c_a" => "drive",
        "battery_v" | "battery_a" | "soc_pct" => "battery",
        "board_temp_c" | "motor_temp_c" => "thermal",
        "suspension_mm" | "shock_mm" => "suspension",
        "isr_load_pct" => "firmware",
        _ => "other",
    }
}

/// Milliseconds since the Unix epoch now (0 if the clock is before it).
pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// `YYYY-MM-DDThh:mm:ssZ` for milliseconds since the Unix epoch (proleptic Gregorian, UTC).
pub fn utc(unix_ms: u64) -> String {
    let secs = unix_ms / 1000;
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Days to a civil date (Howard Hinnant's algorithm), from 1970-01-01.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests;
