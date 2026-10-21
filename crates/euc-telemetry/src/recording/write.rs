//! Writing a recording as the run goes.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use crate::model::{Event, Pose, Snapshot};
use crate::schema::Channel;

use super::{
    BleFrame, CHUNK_HEADER_BYTES, ChunkKind, END_MAGIC, FORMAT_VERSION, Header, INDEX_ENTRY_BYTES, KEYFRAME_S, MAGIC,
    OutputsAt, POSE_FLOATS,
};

/// zstd level: fast, and still about four times smaller than raw floats.
const LEVEL: i32 = 3;

/// One chunk as the index lists it.
struct IndexEntry {
    kind: ChunkKind,
    count: u32,
    offset: u64,
    t0: f64,
    t1: f64,
}

/// Writes a recording as a run goes. Samples gather into chunks of [`KEYFRAME_S`] simulated
/// seconds; events, BLE frames and outputs go out next to the samples chunk of their second;
/// [`Writer::finish`] adds the index. Every chunk is flushed when it is written, so a run that
/// stops early still leaves a readable file.
pub struct Writer<W: Write> {
    out: W,
    /// Bytes written so far.
    pos: u64,
    channels: usize,
    bodies: usize,
    /// The channel of this build each header column holds, for [`Writer::push`].
    known: Vec<Option<Channel>>,
    t: Vec<f64>,
    columns: Vec<Vec<f32>>,
    poses: Vec<f32>,
    events: Vec<Event>,
    frames: Vec<BleFrame>,
    outputs: Vec<OutputsAt>,
    last_t: f64,
    samples: u64,
    index: Vec<IndexEntry>,
}

impl Writer<BufWriter<File>> {
    /// Creates `path` and writes the header.
    pub fn create(path: &Path, header: &Header) -> io::Result<Self> {
        Self::new(BufWriter::new(File::create(path)?), header)
    }
}

impl<W: Write> Writer<W> {
    /// Writes the preamble and `header` to `out`.
    pub fn new(mut out: W, header: &Header) -> io::Result<Self> {
        let json = serde_json::to_vec(header)?;
        let len = u32::try_from(json.len()).map_err(|_| invalid("header over 4 GiB"))?;
        out.write_all(&MAGIC)?;
        out.write_all(&FORMAT_VERSION.to_le_bytes())?;
        out.write_all(&0u16.to_le_bytes())?;
        out.write_all(&len.to_le_bytes())?;
        out.write_all(&json)?;
        out.flush()?;
        let channels = header.channels.len();
        Ok(Self {
            out,
            pos: (super::PREAMBLE_BYTES + json.len()) as u64,
            channels,
            bodies: header.bodies.len(),
            known: header.channels.iter().map(|c| Channel::from_id(c.id)).collect(),
            t: Vec::new(),
            columns: vec![Vec::new(); channels],
            poses: Vec::new(),
            events: Vec::new(),
            frames: Vec::new(),
            outputs: Vec::new(),
            last_t: f64::NEG_INFINITY,
            samples: 0,
            index: Vec::new(),
        })
    }

    /// Samples written so far.
    pub const fn samples(&self) -> u64 {
        self.samples
    }

    /// Bytes written to the output so far (the preamble, the header and every chunk written).
    /// Samples, events, frames and outputs still waiting for their chunk are not counted.
    pub const fn bytes_written(&self) -> u64 {
        self.pos
    }

    /// Samples waiting for their chunk (at most one keyframe's worth).
    pub fn pending_samples(&self) -> usize {
        self.t.len()
    }

    /// Adds a sample: each header channel from `snapshot.signals` (NaN for one this build does
    /// not know), and its poses (missing ones read as [`Pose::IDENTITY`], extra ones are dropped).
    pub fn push(&mut self, snapshot: &Snapshot) -> io::Result<()> {
        self.begin_sample(snapshot.t_s)?;
        for (column, channel) in self.columns.iter_mut().zip(&self.known) {
            column.push(channel.map_or(f32::NAN, |c| snapshot.signals.get(c)));
        }
        self.push_poses(&snapshot.bodies);
        Ok(())
    }

    /// Adds a sample from raw values, one per header channel, in header order.
    pub fn push_values(&mut self, t_s: f64, values: &[f32], poses: &[Pose]) -> io::Result<()> {
        if values.len() != self.channels {
            return Err(invalid("one value per header channel"));
        }
        self.begin_sample(t_s)?;
        for (column, &value) in self.columns.iter_mut().zip(values) {
            column.push(value);
        }
        self.push_poses(poses);
        Ok(())
    }

    /// Adds an event.
    pub fn event(&mut self, event: &Event) {
        self.events.push(event.clone());
    }

    /// Adds a decoded BLE frame.
    pub fn frame(&mut self, frame: BleFrame) {
        self.frames.push(frame);
    }

    /// Records the board's outputs from `at.t_s` on (call it when they change).
    pub fn outputs(&mut self, at: OutputsAt) {
        self.outputs.push(at);
    }

    /// Writes what is waiting (ending the current keyframe early) and flushes the output.
    pub fn flush(&mut self) -> io::Result<()> {
        self.write_pending()?;
        self.out.flush()
    }

    /// Writes what is waiting, the index and the trailer, and hands back the output.
    pub fn finish(mut self) -> io::Result<W> {
        self.write_pending()?;
        let mut raw = Vec::with_capacity(self.index.len() * INDEX_ENTRY_BYTES);
        for e in &self.index {
            raw.push(e.kind.code());
            raw.extend_from_slice(&[0; 3]);
            raw.extend_from_slice(&e.count.to_le_bytes());
            raw.extend_from_slice(&e.offset.to_le_bytes());
            raw.extend_from_slice(&e.t0.to_le_bytes());
            raw.extend_from_slice(&e.t1.to_le_bytes());
        }
        let index_at = self.pos;
        let count = u32::try_from(self.index.len()).map_err(|_| invalid("too many chunks"))?;
        let (t0, t1) = (self.index.first().map_or(0.0, |e| e.t0), self.index.last().map_or(0.0, |e| e.t1));
        self.chunk(ChunkKind::Index, count, t0, t1, &raw)?;
        self.out.write_all(&index_at.to_le_bytes())?;
        self.out.write_all(&END_MAGIC)?;
        self.out.flush()?;
        Ok(self.out)
    }

    fn begin_sample(&mut self, t_s: f64) -> io::Result<()> {
        if !t_s.is_finite() || t_s < self.last_t {
            return Err(invalid("samples must come in time order"));
        }
        if self.t.first().is_some_and(|&t0| t_s >= t0 + KEYFRAME_S) {
            self.write_pending()?;
            self.out.flush()?;
        }
        self.last_t = t_s;
        self.t.push(t_s);
        self.samples += 1;
        Ok(())
    }

    fn push_poses(&mut self, poses: &[Pose]) {
        for i in 0..self.bodies {
            self.poses.extend_from_slice(&poses.get(i).copied().unwrap_or(Pose::IDENTITY).to_array());
        }
    }

    /// The samples chunk, then the events, frames and outputs that arrived with it.
    fn write_pending(&mut self) -> io::Result<()> {
        if let (Some(&t0), Some(&t1)) = (self.t.first(), self.t.last()) {
            let n = self.t.len();
            let mut raw = Vec::with_capacity(16 + 8 * n + 4 * n * (self.channels + self.bodies * POSE_FLOATS));
            for word in [n, self.channels, self.bodies, 0] {
                raw.extend_from_slice(&u32::try_from(word).map_err(|_| invalid("chunk too large"))?.to_le_bytes());
            }
            for t in &self.t {
                raw.extend_from_slice(&t.to_le_bytes());
            }
            for column in &self.columns {
                for v in column {
                    raw.extend_from_slice(&v.to_le_bytes());
                }
            }
            for v in &self.poses {
                raw.extend_from_slice(&v.to_le_bytes());
            }
            self.chunk(ChunkKind::Samples, n as u32, t0, t1, &raw)?;
            self.t.clear();
            self.columns.iter_mut().for_each(Vec::clear);
            self.poses.clear();
        }
        let events = std::mem::take(&mut self.events);
        self.sparse(ChunkKind::Events, &events, |e| e.t_s)?;
        let frames = std::mem::take(&mut self.frames);
        self.sparse(ChunkKind::Frames, &frames, |f| f.t_s)?;
        let outputs = std::mem::take(&mut self.outputs);
        self.sparse(ChunkKind::Outputs, &outputs, |o| o.t_s)
    }

    /// A chunk of items as a JSON array.
    fn sparse<T: serde::Serialize>(&mut self, kind: ChunkKind, items: &[T], t: impl Fn(&T) -> f64) -> io::Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let t0 = items.iter().map(&t).fold(f64::INFINITY, f64::min);
        let t1 = items.iter().map(&t).fold(f64::NEG_INFINITY, f64::max);
        let raw = serde_json::to_vec(items)?;
        let count = u32::try_from(items.len()).map_err(|_| invalid("chunk too large"))?;
        self.chunk(kind, count, t0, t1, &raw)
    }

    /// Writes one chunk: zstd when that is smaller, stored otherwise.
    pub(super) fn chunk(&mut self, kind: ChunkKind, count: u32, t0: f64, t1: f64, raw: &[u8]) -> io::Result<()> {
        let packed = zstd::bulk::compress(raw, LEVEL)?;
        let (codec, stored) = if packed.len() < raw.len() { (1u8, packed.as_slice()) } else { (0, raw) };
        let too_large = || invalid("chunk too large");
        let mut head = [0u8; CHUNK_HEADER_BYTES];
        head[0] = kind.code();
        head[1] = codec;
        head[4..8].copy_from_slice(&count.to_le_bytes());
        head[8..12].copy_from_slice(&u32::try_from(stored.len()).map_err(|_| too_large())?.to_le_bytes());
        head[12..16].copy_from_slice(&u32::try_from(raw.len()).map_err(|_| too_large())?.to_le_bytes());
        head[16..24].copy_from_slice(&t0.to_le_bytes());
        head[24..32].copy_from_slice(&t1.to_le_bytes());
        let crc = super::read::chunk_crc(&head, stored);
        head[32..36].copy_from_slice(&crc.to_le_bytes());
        self.out.write_all(&head)?;
        self.out.write_all(stored)?;
        if kind != ChunkKind::Index {
            self.index.push(IndexEntry { kind, count, offset: self.pos, t0, t1 });
        }
        self.pos += (CHUNK_HEADER_BYTES + stored.len()) as u64;
        Ok(())
    }
}

fn invalid(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, why.to_owned())
}
