//! Reading a recording: the header and the sparse streams at open, samples one keyframe at a
//! time.

use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::model::{Event, Outputs, Pose, Signals, Snapshot};
use crate::schema::Channel;

use super::{
    BleFrame, CHUNK_HEADER_BYTES, ChunkKind, END_MAGIC, FORMAT_VERSION, Header, INDEX_ENTRY_BYTES, MAGIC,
    MAX_CHUNK_BYTES, MAX_HEADER_BYTES, OutputsAt, POSE_FLOATS, PREAMBLE_BYTES, TRAILER_BYTES,
};

/// Allocation limits checked from chunk metadata before payload decompression.
/// Default readers can stream long sample tracks, but bound the sparse tracks held in memory.
#[derive(Debug, Clone, Copy)]
pub struct ReadLimits {
    /// Maximum input file size.
    pub max_file_bytes: u64,
    /// Maximum serialized JSON header size.
    pub max_header_bytes: u64,
    /// Maximum stored or decoded size of one known chunk.
    pub max_chunk_bytes: u64,
    /// Maximum number of indexed data chunks, including unknown kinds.
    pub max_chunks: u64,
    /// Maximum number of samples across all keyframes.
    pub max_samples: u64,
    /// Maximum scalar and pose values across all samples, including unknown channels.
    pub max_sample_values: u64,
    /// Maximum cumulative decoded bytes of events, frames, and outputs.
    pub max_sparse_bytes: u64,
    /// Maximum event items, including unknown event kinds.
    pub max_events: u64,
    /// Maximum BLE frame items.
    pub max_frames: u64,
    /// Maximum output change items.
    pub max_outputs: u64,
}

impl Default for ReadLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: u64::MAX,
            max_header_bytes: u64::from(MAX_HEADER_BYTES),
            max_chunk_bytes: u64::from(MAX_CHUNK_BYTES),
            max_chunks: 1_000_000,
            max_samples: u64::MAX,
            max_sample_values: u64::MAX,
            max_sparse_bytes: 256 << 20,
            max_events: 1_000_000,
            max_frames: 1_000_000,
            max_outputs: 1_000_000,
        }
    }
}

/// Why a recording cannot be read.
#[derive(Debug)]
pub enum ReadError {
    /// Reading the file failed.
    Io(io::Error),
    /// Not a recording (bad magic), or too short to hold its own header.
    NotARecording,
    /// A format version this build does not read.
    Version(u16),
    /// The header does not parse.
    Header(String),
    /// A configured resource limit was exceeded before allocation.
    Limit {
        /// The resource whose bound was exceeded.
        what: &'static str,
        /// The configured maximum.
        limit: u64,
    },
    /// A chunk the index lists is damaged.
    Corrupt {
        /// File offset of the chunk.
        offset: u64,
        /// What is wrong.
        why: String,
    },
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::NotARecording => f.write_str("not an .eucrec recording"),
            Self::Version(v) => write!(f, "recording format {v} is newer than this build reads ({FORMAT_VERSION})"),
            Self::Header(why) => write!(f, "recording header: {why}"),
            Self::Limit { what, limit } => write!(f, "recording exceeds {what} limit ({limit})"),
            Self::Corrupt { offset, why } => write!(f, "recording chunk at byte {offset}: {why}"),
        }
    }
}

impl std::error::Error for ReadError {}

impl From<io::Error> for ReadError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Samples in columns: times, one column per header channel, poses.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Samples {
    /// Simulated seconds of each sample.
    pub t: Vec<f64>,
    /// One column per header channel, in header order.
    pub columns: Vec<Vec<f32>>,
    /// Bodies per sample.
    pub bodies: usize,
    /// `t.len() * bodies * 7` floats: for each sample, each body's x y z qw qx qy qz.
    pub poses: Vec<f32>,
}

impl Samples {
    fn empty(channels: usize, bodies: usize) -> Self {
        Self { t: Vec::new(), columns: vec![Vec::new(); channels], bodies, poses: Vec::new() }
    }

    /// Number of samples.
    pub fn len(&self) -> usize {
        self.t.len()
    }

    /// No samples.
    pub fn is_empty(&self) -> bool {
        self.t.is_empty()
    }

    /// Pose of `body` in sample `i`.
    pub fn pose(&self, i: usize, body: usize) -> Option<Pose> {
        if body >= self.bodies {
            return None;
        }
        let at = (i * self.bodies + body) * POSE_FLOATS;
        let p = self.poses.get(at..at + POSE_FLOATS)?;
        Some(Pose { pos: [p[0], p[1], p[2]], quat: [p[3], p[4], p[5], p[6]] })
    }

    /// Appends samples `range` of `other`.
    fn extend_from(&mut self, other: &Self, range: std::ops::Range<usize>) {
        self.t.extend_from_slice(&other.t[range.clone()]);
        for (mine, theirs) in self.columns.iter_mut().zip(&other.columns) {
            mine.extend_from_slice(&theirs[range.clone()]);
        }
        let size = self.bodies * POSE_FLOATS;
        self.poses.extend_from_slice(&other.poses[range.start * size..range.end * size]);
    }
}

/// A chunk the reader knows about.
#[derive(Debug, Clone, Copy)]
struct ChunkRef {
    kind: ChunkKind,
    count: u32,
    offset: u64,
    t0: f64,
    t1: f64,
    raw: u32,
    stored: u32,
}

#[derive(Default, Clone, Copy)]
struct Totals {
    samples: u64,
    values: u64,
    sparse_bytes: u64,
    events: u64,
    frames: u64,
    outputs: u64,
}

/// Reads a recording. Opening reads the header, the index (or, in a file whose writer stopped,
/// every whole chunk) and the events, frames and outputs; samples are decoded a keyframe at a
/// time, with the last one cached.
pub struct Reader<R> {
    src: R,
    header: Header,
    chunks: Vec<ChunkRef>,
    /// Indices into `chunks` of the samples chunks, in time order.
    keyframes: Vec<usize>,
    events: Vec<Event>,
    frames: Vec<BleFrame>,
    outputs: Vec<OutputsAt>,
    known: Vec<Option<Channel>>,
    truncated: Option<String>,
    skipped: usize,
    cache: Option<(usize, Samples)>,
    decoded: usize,
    limits: ReadLimits,
    totals: Totals,
}

impl Reader<BufReader<File>> {
    /// Opens the recording at `path`.
    pub fn open(path: &Path) -> Result<Self, ReadError> {
        Self::new(BufReader::new(File::open(path)?))
    }

    /// Opens a recording with explicit allocation limits.
    pub fn open_with_limits(path: &Path, limits: ReadLimits) -> Result<Self, ReadError> {
        Self::new_with_limits(BufReader::new(File::open(path)?), limits)
    }
}

impl<R: Read + Seek> Reader<R> {
    /// Reads a recording with explicit allocation limits.
    pub fn new_with_limits(mut src: R, limits: ReadLimits) -> Result<Self, ReadError> {
        let len = src.seek(SeekFrom::End(0))?;
        bounded(len, limits.max_file_bytes, "file bytes")?;
        src.seek(SeekFrom::Start(0))?;
        let mut preamble = [0u8; PREAMBLE_BYTES];
        read_up_to(&mut src, &mut preamble).map_err(ReadError::Io)?;
        if preamble[..8] != MAGIC || len < PREAMBLE_BYTES as u64 {
            return Err(ReadError::NotARecording);
        }
        let version = u16::from_le_bytes([preamble[8], preamble[9]]);
        if version != FORMAT_VERSION {
            return Err(ReadError::Version(version));
        }
        let h = u32::from_le_bytes([preamble[12], preamble[13], preamble[14], preamble[15]]);
        let data_at = PREAMBLE_BYTES as u64 + u64::from(h);
        if h > MAX_HEADER_BYTES || data_at > len {
            return Err(ReadError::Header("cut short or damaged".into()));
        }
        bounded(u64::from(h), limits.max_header_bytes, "header bytes")?;
        let mut json = vec![0u8; h as usize];
        src.read_exact(&mut json)?;
        let header: Header = serde_json::from_slice(&json).map_err(|e| ReadError::Header(e.to_string()))?;

        let mut reader = Self {
            src,
            known: header.channels.iter().map(|c| Channel::from_id(c.id)).collect(),
            header,
            chunks: Vec::new(),
            keyframes: Vec::new(),
            events: Vec::new(),
            frames: Vec::new(),
            outputs: Vec::new(),
            truncated: None,
            skipped: 0,
            cache: None,
            decoded: 0,
            limits,
            totals: Totals::default(),
        };
        match reader.read_index(data_at, len)? {
            Some(chunks) => {
                for chunk in chunks {
                    reader.add_chunk(chunk)?;
                }
            }
            None => reader.scan(data_at, len)?,
        }
        reader.load_sparse()?;
        reader.keyframes = (0..reader.chunks.len()).filter(|&i| reader.chunks[i].kind == ChunkKind::Samples).collect();
        let chunks = &reader.chunks;
        reader.keyframes.sort_by(|&a, &b| chunks[a].t0.total_cmp(&chunks[b].t0));
        Ok(reader)
    }

    /// Reads a recording with the default streaming limits.
    pub fn new(src: R) -> Result<Self, ReadError> {
        Self::new_with_limits(src, ReadLimits::default())
    }

    /// The header.
    pub const fn header(&self) -> &Header {
        &self.header
    }

    /// Why the file ends early (the writer stopped before finishing it), if it does. What it
    /// holds up to there reads normally.
    pub fn truncated(&self) -> Option<&str> {
        self.truncated.as_deref()
    }

    /// Chunks and items of kinds this build does not know, skipped.
    pub const fn skipped(&self) -> usize {
        self.skipped
    }

    /// Simulated seconds of the first sample (0 without samples).
    pub fn start_s(&self) -> f64 {
        self.keyframes.first().map_or(0.0, |&i| self.chunks[i].t0)
    }

    /// Simulated seconds of the last sample (0 without samples).
    pub fn end_s(&self) -> f64 {
        self.keyframes.last().map_or(0.0, |&i| self.chunks[i].t1)
    }

    /// Number of samples.
    pub fn sample_count(&self) -> u64 {
        self.keyframes.iter().map(|&i| u64::from(self.chunks[i].count)).sum()
    }

    /// Start time of each keyframe (samples chunk), in order.
    pub fn keyframe_times(&self) -> Vec<f64> {
        self.keyframes.iter().map(|&i| self.chunks[i].t0).collect()
    }

    /// File offset of keyframe `k`'s chunk.
    #[cfg(test)]
    pub(super) fn keyframe_offset(&self, k: usize) -> u64 {
        self.chunks[self.keyframes[k]].offset
    }

    /// Keyframes decoded so far (cache misses), for tests and diagnostics.
    pub const fn decoded(&self) -> usize {
        self.decoded
    }

    /// Every event, in file order.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Every decoded BLE frame, in file order.
    pub fn frames(&self) -> &[BleFrame] {
        &self.frames
    }

    /// Every board output change, in file order.
    pub fn outputs(&self) -> &[OutputsAt] {
        &self.outputs
    }

    /// The board's outputs at `t_s`: the last change at or before it.
    pub fn outputs_at(&self, t_s: f64) -> Option<&Outputs> {
        self.outputs.iter().rev().find(|o| o.t_s <= t_s).map(|o| &o.outputs)
    }

    /// The channel of this build that header column `i` holds, if this build knows it.
    pub fn channel_of(&self, column: usize) -> Option<Channel> {
        self.known.get(column).copied().flatten()
    }

    /// Sample `i` of `samples` as a snapshot: the channels this build knows (the rest of
    /// [`Signals`] stays NaN) and the poses.
    pub fn snapshot(&self, samples: &Samples, i: usize) -> Snapshot {
        let mut signals = Signals::default();
        for (column, channel) in samples.columns.iter().zip(&self.known) {
            if let (Some(channel), Some(&v)) = (channel, column.get(i)) {
                signals.set(*channel, v);
            }
        }
        let bodies = (0..samples.bodies).filter_map(|b| samples.pose(i, b)).collect();
        Snapshot { t_s: samples.t.get(i).copied().unwrap_or(f64::NAN), signals, bodies }
    }

    /// Keyframe `k` (the `k`-th samples chunk), decoded.
    pub fn keyframe(&mut self, k: usize) -> Result<&Samples, ReadError> {
        self.load_keyframe(k)?;
        self.cache
            .as_ref()
            .map(|(_, samples)| samples)
            .ok_or_else(|| ReadError::Corrupt { offset: 0, why: format!("keyframe {k} did not load") })
    }

    /// Calls `f` with keyframe `k` (decoded once, then cached) and the reader.
    fn with_keyframe<T>(&mut self, k: usize, f: impl FnOnce(&Self, &Samples) -> T) -> Result<T, ReadError> {
        self.load_keyframe(k)?;
        let (at, samples) = self.cache.take().expect("loaded");
        let out = f(self, &samples);
        self.cache = Some((at, samples));
        Ok(out)
    }

    fn load_keyframe(&mut self, k: usize) -> Result<(), ReadError> {
        if self.cache.as_ref().is_some_and(|(at, _)| *at == k) {
            return Ok(());
        }
        let chunk = *self
            .keyframes
            .get(k)
            .and_then(|&i| self.chunks.get(i))
            .ok_or_else(|| ReadError::Corrupt { offset: 0, why: format!("there is no keyframe {k}") })?;
        let raw = self.payload(&chunk)?;
        let samples =
            self.decode_samples(&raw, &chunk).map_err(|why| ReadError::Corrupt { offset: chunk.offset, why })?;
        self.decoded += 1;
        self.cache = Some((k, samples));
        Ok(())
    }

    /// Which keyframe holds time `t_s`: the last one that starts at or before it (the first one
    /// for earlier times); `None` without samples.
    pub fn keyframe_at(&self, t_s: f64) -> Option<usize> {
        if self.keyframes.is_empty() {
            return None;
        }
        let after = self.keyframes.partition_point(|&i| self.chunks[i].t0 <= t_s);
        Some(after.saturating_sub(1))
    }

    /// The last sample at or before `t_s` (seeking decodes one keyframe); `None` before the first
    /// sample.
    pub fn sample_at(&mut self, t_s: f64) -> Result<Option<Snapshot>, ReadError> {
        let Some(k) = self.keyframe_at(t_s) else { return Ok(None) };
        self.with_keyframe(k, |reader, samples| {
            let i = samples.t.partition_point(|&t| t <= t_s);
            (i > 0).then(|| reader.snapshot(samples, i - 1))
        })
    }

    /// Every sample with `after < t <= until`, decoding only the keyframes that hold them.
    pub fn samples_in(&mut self, after: f64, until: f64) -> Result<Samples, ReadError> {
        let mut out = Samples::empty(self.header.channels.len(), self.header.bodies.len());
        let Some(first) = self.keyframe_at(after) else { return Ok(out) };
        for k in first..self.keyframes.len() {
            let chunk = self.chunks[self.keyframes[k]];
            if chunk.t0 > until {
                break;
            }
            if chunk.t1 <= after {
                continue;
            }
            self.with_keyframe(k, |_, samples| {
                let from = samples.t.partition_point(|&t| t <= after);
                let to = samples.t.partition_point(|&t| t <= until);
                if from < to {
                    out.extend_from(samples, from..to);
                }
            })?;
        }
        Ok(out)
    }

    /// Every sample.
    pub fn read_all(&mut self) -> Result<Samples, ReadError> {
        self.samples_in(f64::NEG_INFINITY, f64::INFINITY)
    }

    /// The index of a finished file: `None` when the trailer or its checksum is incomplete.
    fn read_index(&mut self, data_at: u64, len: u64) -> Result<Option<Vec<ChunkRef>>, ReadError> {
        if len < data_at + TRAILER_BYTES as u64 {
            return Ok(None);
        }
        let mut trailer = [0u8; TRAILER_BYTES];
        self.src.seek(SeekFrom::Start(len - TRAILER_BYTES as u64))?;
        self.src.read_exact(&mut trailer)?;
        if trailer[8..] != END_MAGIC {
            return Ok(None);
        }
        let at = u64::from_le_bytes(trailer[..8].try_into().expect("8 bytes"));
        let index_end = len - TRAILER_BYTES as u64;
        let Some((head, encoded)) = self.read_head(at, index_end)? else { return Ok(None) };
        if head.kind != ChunkKind::Index || at < data_at || at + head_size(&head) != index_end {
            return Ok(None);
        }
        bounded(u64::from(head.count), self.limits.max_chunks, "chunks")?;
        if u64::from(head.raw) != u64::from(head.count) * INDEX_ENTRY_BYTES as u64 {
            return Err(corrupt(at, "index count differs from its payload size"));
        }
        let Some(raw) = self.read_payload(at, &head, &encoded)? else { return Ok(None) };
        let mut chunks = Vec::new();
        for e in raw.as_chunks::<INDEX_ENTRY_BYTES>().0 {
            let word = |at: usize| u32::from_le_bytes([e[at], e[at + 1], e[at + 2], e[at + 3]]);
            let wide = |at: usize| u64::from_le_bytes(e[at..at + 8].try_into().expect("8 bytes"));
            chunks.push(ChunkRef {
                kind: ChunkKind::from_code(e[0]),
                count: word(4),
                offset: wide(8),
                t0: f64::from_bits(wide(16)),
                t1: f64::from_bits(wide(24)),
                raw: 0,
                stored: 0,
            });
        }
        // Entries may be reordered, but each physical chunk must occur exactly once. A gap,
        // duplicate or overlap cannot silently replay data or hide an unbudgeted chunk.
        chunks.sort_unstable_by_key(|chunk| chunk.offset);
        let mut next = data_at;
        for chunk in &mut chunks {
            if chunk.offset != next {
                return Err(corrupt(chunk.offset, "index offsets contain a gap, duplicate, or overlap"));
            }
            let Some((head, _)) = self.read_head(chunk.offset, at)? else {
                return Err(corrupt(chunk.offset, "indexed chunk is cut short"));
            };
            if !chunk.matches(&head) || head.kind == ChunkKind::Index {
                return Err(corrupt(chunk.offset, "index metadata differs from its chunk header"));
            }
            self.check_shape(chunk.offset, &head)?;
            chunk.raw = head.raw;
            chunk.stored = head.stored;
            next += head_size(&head);
        }
        if next != at {
            return Err(corrupt(at, "index omits physical chunks"));
        }
        Ok(Some(chunks))
    }

    /// Walks whole chunks without decompression. Sparse decoding starts only after all
    /// surviving chunk metadata passes the cumulative budgets.
    fn scan(&mut self, data_at: u64, len: u64) -> Result<(), ReadError> {
        let mut at = data_at;
        loop {
            if at == len {
                self.truncated = Some("the writer did not finish the file (no index)".into());
                return Ok(());
            }
            let Some((head, encoded)) = self.read_head(at, len)? else {
                self.truncated = Some(format!("the file is cut short or damaged at byte {at}"));
                return Ok(());
            };
            if head.kind != ChunkKind::Index {
                self.check_shape(at, &head)?;
                self.check_budget(&ChunkRef::from(head).with_offset(at))?;
            }
            if !self.check_crc(at, &head, &encoded)? {
                self.truncated = Some(format!("the file is cut short or damaged at byte {at}"));
                return Ok(());
            }
            if head.kind == ChunkKind::Index {
                self.truncated = Some("the file ends inside its trailer".into());
                return Ok(());
            }
            self.add_chunk(ChunkRef::from(head).with_offset(at))?;
            at += head_size(&head);
        }
    }

    /// Reads just the fixed header, checking sizes before any payload allocation.
    fn read_head(&mut self, at: u64, end: u64) -> Result<Option<(ChunkHead, [u8; CHUNK_HEADER_BYTES])>, ReadError> {
        if at > end || end - at < CHUNK_HEADER_BYTES as u64 {
            return Ok(None);
        }
        self.src.seek(SeekFrom::Start(at))?;
        let mut encoded = [0u8; CHUNK_HEADER_BYTES];
        self.src.read_exact(&mut encoded)?;
        let head = ChunkHead::parse(&encoded);
        if head.stored > MAX_CHUNK_BYTES || head.raw > MAX_CHUNK_BYTES || head_size(&head) > end - at {
            return Ok(None);
        }
        if !matches!(head.kind, ChunkKind::Unknown(_)) {
            bounded(u64::from(head.stored.max(head.raw)), self.limits.max_chunk_bytes, "chunk bytes")?;
        }
        Ok(Some((head, encoded)))
    }

    /// Checks stored bytes in fixed memory, including unknown kinds and codecs during recovery.
    fn check_crc(&mut self, at: u64, head: &ChunkHead, encoded: &[u8; CHUNK_HEADER_BYTES]) -> Result<bool, ReadError> {
        self.src.seek(SeekFrom::Start(at + CHUNK_HEADER_BYTES as u64))?;
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&encoded[..32]);
        let mut buffer = [0u8; 8192];
        let mut remaining = head.stored as usize;
        while remaining != 0 {
            let count = remaining.min(buffer.len());
            self.src.read_exact(&mut buffer[..count])?;
            hasher.update(&buffer[..count]);
            remaining -= count;
        }
        Ok(hasher.finalize() == head.crc)
    }

    fn read_payload(
        &mut self,
        at: u64,
        head: &ChunkHead,
        encoded: &[u8; CHUNK_HEADER_BYTES],
    ) -> Result<Option<Vec<u8>>, ReadError> {
        self.src.seek(SeekFrom::Start(at + CHUNK_HEADER_BYTES as u64))?;
        let mut stored = vec![0u8; head.stored as usize];
        self.src.read_exact(&mut stored)?;
        if chunk_crc(encoded, &stored) != head.crc {
            return Ok(None);
        }
        let raw = match head.codec {
            0 if head.raw == head.stored => stored,
            1 => match zstd::bulk::decompress(&stored, head.raw as usize) {
                Ok(raw) if raw.len() == head.raw as usize => raw,
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };
        Ok(Some(raw))
    }

    /// Checks header shape without trusting counts for allocations.
    fn check_shape(&self, offset: u64, head: &ChunkHead) -> Result<(), ReadError> {
        if head.kind == ChunkKind::Samples && (!head.t0.is_finite() || !head.t1.is_finite() || head.t0 > head.t1) {
            return Err(corrupt(offset, "sample chunk bounds must be finite and ordered"));
        }
        let expected = match head.kind {
            ChunkKind::Samples => Some(
                sample_bytes(u64::from(head.count), &self.header)
                    .ok_or_else(|| corrupt(offset, "sample dimensions overflow"))?,
            ),
            ChunkKind::Index => Some(u64::from(head.count) * INDEX_ENTRY_BYTES as u64),
            _ => None,
        };
        if expected.is_some_and(|bytes| bytes != u64::from(head.raw)) {
            return Err(corrupt(offset, "chunk count and dimensions differ from decoded payload size"));
        }
        if matches!(head.kind, ChunkKind::Events | ChunkKind::Frames | ChunkKind::Outputs) {
            let smallest_array = if head.count == 0 { 2 } else { 2 * u64::from(head.count) + 1 };
            if u64::from(head.raw) < smallest_array {
                return Err(corrupt(offset, "sparse count cannot fit in the declared payload"));
            }
        }
        Ok(())
    }

    fn check_budget(&self, chunk: &ChunkRef) -> Result<Totals, ReadError> {
        bounded(self.chunks.len() as u64 + 1, self.limits.max_chunks, "chunks")?;
        let mut totals = self.totals;
        let count = u64::from(chunk.count);
        match chunk.kind {
            ChunkKind::Samples => {
                totals.samples = add_bounded(totals.samples, count, self.limits.max_samples, "samples")?;
                let width = sample_width(&self.header).ok_or_else(|| corrupt(chunk.offset, "sample width overflow"))?;
                let values =
                    count.checked_mul(width).ok_or_else(|| corrupt(chunk.offset, "sample value count overflow"))?;
                totals.values = add_bounded(totals.values, values, self.limits.max_sample_values, "sample values")?;
            }
            ChunkKind::Events | ChunkKind::Frames | ChunkKind::Outputs => {
                totals.sparse_bytes = add_bounded(
                    totals.sparse_bytes,
                    u64::from(chunk.raw),
                    self.limits.max_sparse_bytes,
                    "sparse bytes",
                )?;
                match chunk.kind {
                    ChunkKind::Events => {
                        totals.events = add_bounded(totals.events, count, self.limits.max_events, "events")?;
                    }
                    ChunkKind::Frames => {
                        totals.frames = add_bounded(totals.frames, count, self.limits.max_frames, "frames")?;
                    }
                    _ => totals.outputs = add_bounded(totals.outputs, count, self.limits.max_outputs, "outputs")?,
                }
            }
            _ => {}
        }
        Ok(totals)
    }

    fn add_chunk(&mut self, chunk: ChunkRef) -> Result<(), ReadError> {
        self.totals = self.check_budget(&chunk)?;
        self.chunks.push(chunk);
        Ok(())
    }

    /// Reads a budgeted chunk, verifying its actual header against the validated index.
    fn payload(&mut self, chunk: &ChunkRef) -> Result<Vec<u8>, ReadError> {
        let end = self.src.seek(SeekFrom::End(0))?;
        let Some((head, encoded)) = self.read_head(chunk.offset, end)? else {
            return Err(corrupt(chunk.offset, "cut short or damaged"));
        };
        if !chunk.matches(&head) || chunk.raw != head.raw || chunk.stored != head.stored {
            return Err(corrupt(chunk.offset, "not the chunk the index lists"));
        }
        self.read_payload(chunk.offset, &head, &encoded)?.ok_or_else(|| corrupt(chunk.offset, "cut short or damaged"))
    }

    fn decode_samples(&self, raw: &[u8], chunk: &ChunkRef) -> Result<Samples, String> {
        let word = |at: usize| -> Result<usize, String> {
            raw.get(at..at + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
                .ok_or_else(|| "samples chunk too short".to_owned())
        };
        let (n, c, b) = (word(0)?, word(4)?, word(8)?);
        if n != chunk.count as usize {
            return Err("sample payload count differs from its chunk header".into());
        }
        let (channels, bodies) = (self.header.channels.len(), self.header.bodies.len());
        if (c, b) != (channels, bodies) {
            return Err(format!("{c} channels and {b} bodies, but the header has {channels} and {bodies}"));
        }
        let expected = sample_bytes(n as u64, &self.header).ok_or("sample dimensions overflow")?;
        if raw.len() as u64 != expected {
            return Err(format!("samples chunk is {} bytes, expected {expected}", raw.len()));
        }
        let floats = |from: usize, count: usize| -> Vec<f32> {
            raw[from..from + 4 * count].as_chunks::<4>().0.iter().map(|q| f32::from_le_bytes(*q)).collect()
        };
        let times = raw[16..16 + 8 * n].as_chunks::<8>().0;
        let mut previous = f64::NEG_INFINITY;
        for (i, encoded) in times.iter().enumerate() {
            let time = f64::from_le_bytes(*encoded);
            if !time.is_finite()
                || time < previous
                || (i == 0 && time.to_bits() != chunk.t0.to_bits())
                || (i + 1 == n && time.to_bits() != chunk.t1.to_bits())
            {
                return Err("sample times differ from the chunk bounds or time order".into());
            }
            previous = time;
        }
        let t = times.iter().map(|q| f64::from_le_bytes(*q)).collect();
        let columns_at = 16 + 8 * n;
        let columns = (0..c).map(|i| floats(columns_at + 4 * n * i, n)).collect();
        let poses = floats(columns_at + 4 * n * c, n * b * POSE_FLOATS);
        Ok(Samples { t, columns, bodies: b, poses })
    }

    /// Reads every events, frames and outputs chunk. Items of a kind this build cannot parse are
    /// skipped and counted.
    fn load_sparse(&mut self) -> Result<(), ReadError> {
        let chunks = self.chunks.clone();
        for chunk in &chunks {
            match chunk.kind {
                ChunkKind::Samples => {}
                ChunkKind::Events => {
                    let items = self.items(chunk)?;
                    self.events.extend(items);
                }
                ChunkKind::Frames => {
                    let items = self.items(chunk)?;
                    self.frames.extend(items);
                }
                ChunkKind::Outputs => {
                    let items = self.items(chunk)?;
                    self.outputs.extend(items);
                }
                ChunkKind::Index | ChunkKind::Unknown(_) => self.skipped += 1,
            }
        }
        Ok(())
    }

    /// The items of a JSON chunk; an item that does not parse (a kind from a later build) is
    /// skipped.
    fn items<T: serde::de::DeserializeOwned>(&mut self, chunk: &ChunkRef) -> Result<Vec<T>, ReadError> {
        use serde::de::Deserializer as _;
        let raw = self.payload(chunk)?;
        let mut decoder = serde_json::Deserializer::from_slice(&raw);
        let visitor = ItemsVisitor::<T> { count: chunk.count, marker: std::marker::PhantomData };
        let (items, skipped) = decoder
            .deserialize_seq(visitor)
            .map_err(|e| ReadError::Corrupt { offset: chunk.offset, why: e.to_string() })?;
        decoder.end().map_err(|e| ReadError::Corrupt { offset: chunk.offset, why: e.to_string() })?;
        self.skipped += skipped;
        Ok(items)
    }
}

/// A chunk header, parsed.
#[derive(Debug, Clone, Copy)]
struct ChunkHead {
    kind: ChunkKind,
    codec: u8,
    count: u32,
    stored: u32,
    raw: u32,
    t0: f64,
    t1: f64,
    crc: u32,
}

impl ChunkHead {
    fn parse(head: &[u8; CHUNK_HEADER_BYTES]) -> Self {
        let word = |at: usize| u32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
        let float = |at: usize| f64::from_le_bytes(head[at..at + 8].try_into().expect("8 bytes"));
        Self {
            kind: ChunkKind::from_code(head[0]),
            codec: head[1],
            count: word(4),
            stored: word(8),
            raw: word(12),
            t0: float(16),
            t1: float(24),
            crc: word(32),
        }
    }
}

impl From<ChunkHead> for ChunkRef {
    fn from(h: ChunkHead) -> Self {
        Self { kind: h.kind, count: h.count, offset: 0, t0: h.t0, t1: h.t1, raw: h.raw, stored: h.stored }
    }
}

impl ChunkRef {
    fn matches(&self, head: &ChunkHead) -> bool {
        self.kind == head.kind
            && self.count == head.count
            && self.t0.to_bits() == head.t0.to_bits()
            && self.t1.to_bits() == head.t1.to_bits()
    }
    const fn with_offset(self, offset: u64) -> Self {
        Self { offset, ..self }
    }
}

fn corrupt(offset: u64, why: &str) -> ReadError {
    ReadError::Corrupt { offset, why: why.to_owned() }
}

fn bounded(value: u64, limit: u64, what: &'static str) -> Result<(), ReadError> {
    if value > limit { Err(ReadError::Limit { what, limit }) } else { Ok(()) }
}

fn add_bounded(value: u64, more: u64, limit: u64, what: &'static str) -> Result<u64, ReadError> {
    let total = value.checked_add(more).ok_or(ReadError::Limit { what, limit })?;
    bounded(total, limit, what)?;
    Ok(total)
}

fn sample_width(header: &Header) -> Option<u64> {
    (header.bodies.len() as u64).checked_mul(POSE_FLOATS as u64)?.checked_add(header.channels.len() as u64)
}

fn sample_bytes(count: u64, header: &Header) -> Option<u64> {
    sample_width(header)?.checked_mul(4)?.checked_add(8)?.checked_mul(count)?.checked_add(16)
}

struct ItemsVisitor<T> {
    count: u32,
    marker: std::marker::PhantomData<T>,
}

impl<'de, T: serde::de::DeserializeOwned> serde::de::Visitor<'de> for ItemsVisitor<T> {
    type Value = (Vec<T>, usize);

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "an array with {} items", self.count)
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        use serde::de::Error as _;
        let mut items = Vec::new();
        let mut skipped = 0;
        for _ in 0..self.count {
            // Borrow each item's encoded text. A generic Value tree can multiply a tiny
            // nested numeric array's memory before the typed item rejects it.
            let value = seq
                .next_element::<&'de serde_json::value::RawValue>()?
                .ok_or_else(|| A::Error::custom("sparse array has fewer items than the chunk count"))?;
            match serde_json::from_str(value.get()) {
                Ok(item) => items.push(item),
                Err(_) => skipped += 1,
            }
        }
        if seq.next_element::<serde::de::IgnoredAny>()?.is_some() {
            return Err(A::Error::custom("sparse array has more items than the chunk count"));
        }
        Ok((items, skipped))
    }
}

/// Bytes a chunk takes in the file.
fn head_size(head: &ChunkHead) -> u64 {
    CHUNK_HEADER_BYTES as u64 + u64::from(head.stored)
}

/// The crc32 a chunk header carries: of its first 32 bytes, then of the stored payload.
pub(super) fn chunk_crc(head: &[u8; CHUNK_HEADER_BYTES], stored: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&head[..32]);
    hasher.update(stored);
    hasher.finalize()
}

/// Fills `buf` as far as the source goes.
fn read_up_to(src: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match src.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}
