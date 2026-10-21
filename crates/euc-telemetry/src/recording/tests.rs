use std::io::Cursor;

use super::*;
use crate::model::{BodyRole, Event, EventKind, Pose, Signals, Snapshot};
use crate::schema::{CHANNELS, Channel};

const HZ: f64 = 100.0;

fn header(bodies: usize) -> Header {
    let mut h = Header::new("test 0", "begode-extreme", "GW-TEST");
    h.started_unix_ms = 1_792_503_907_000;
    h.firmware_sha256 = Some("ab".repeat(32));
    h.git_rev = Some("0123abc".into());
    h.seed = Some(7);
    h.scenario = Some("ride".into());
    h.settings = Some("neutral".into());
    h.sample_hz = HZ;
    h.bodies =
        (0..bodies).map(|i| BodyInfo { name: format!("b{i}"), role: BodyRole::Other, geoms: Vec::new() }).collect();
    h
}

fn snapshot(i: u32, bodies: usize) -> Snapshot {
    let t = f64::from(i) / HZ;
    let signals = Signals {
        speed_kmh: i as f32 * 0.1,
        battery_v: 130.0 - i as f32 * 0.01,
        pitch_deg: (t.sin() * 3.0) as f32,
        ..Signals::default()
    };
    let bodies = (0..bodies).map(|b| Pose { pos: [t as f32, b as f32, 0.28], quat: [1.0, 0.0, 0.0, 0.0] }).collect();
    Snapshot { t_s: t, signals, bodies }
}

/// `n` samples at 100 Hz with `bodies` bodies, an event and a frame every half second, an
/// output change every second.
fn recording(n: u32, bodies: usize) -> Vec<u8> {
    let mut w = Writer::new(Vec::new(), &header(bodies)).unwrap();
    for i in 0..n {
        let s = snapshot(i, bodies);
        if i % 50 == 0 {
            w.event(&Event { t_s: s.t_s, kind: EventKind::Scenario, text: format!("half second {i}") });
            w.frame(BleFrame {
                t_s: s.t_s,
                kind: "live".into(),
                summary: format!("{i} km/h"),
                fields: vec![("Speed".into(), format!("{i}"))],
                bytes: vec![0x55, 0xAA, i as u8],
                reply: i == 100,
            });
        }
        if i % 100 == 0 {
            w.outputs(OutputsAt { t_s: s.t_s, outputs: Outputs { buzzer_hz: i, ..Outputs::default() } });
        }
        w.push(&s).unwrap();
    }
    w.finish().unwrap()
}

fn open(bytes: &[u8]) -> Result<Reader<Cursor<&[u8]>>, ReadError> {
    Reader::new(Cursor::new(bytes))
}

fn same_bits(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

#[test]
fn round_trip() {
    let bytes = recording(350, 2);
    let mut r = open(&bytes).unwrap();
    assert_eq!(r.header(), &header(2));
    assert_eq!(r.truncated(), None);
    assert_eq!(r.sample_count(), 350);
    assert_eq!((r.start_s(), r.end_s()), (0.0, 3.49));
    assert_eq!(r.keyframe_times(), [0.0, 1.0, 2.0, 3.0], "a keyframe every simulated second");

    let all = r.read_all().unwrap();
    assert_eq!(all.len(), 350);
    for i in [0, 99, 100, 349] {
        let want = snapshot(i as u32, 2);
        let got = r.snapshot(&all, i);
        assert_eq!(got.t_s, want.t_s);
        assert_eq!(got.bodies, want.bodies);
        assert_eq!(got.signals.speed_kmh.to_bits(), want.signals.speed_kmh.to_bits());
        assert_eq!(got.signals.pitch_deg.to_bits(), want.signals.pitch_deg.to_bits());
        assert!(got.signals.pwm_pct.is_nan(), "unmodelled stays NaN");
    }
    let speed = r.header().column("speed_kmh").unwrap();
    let want: Vec<f32> = (0..350).map(|i| i as f32 * 0.1).collect();
    assert!(same_bits(&all.columns[speed], &want));

    assert_eq!(r.events().len(), 7);
    assert_eq!(r.events()[2].text, "half second 100");
    assert_eq!(r.frames().len(), 7);
    assert_eq!(r.frames()[2].bytes, [0x55, 0xAA, 100]);
    assert!(r.frames()[2].reply && !r.frames()[1].reply);
    assert_eq!(r.outputs().len(), 4);
    assert_eq!(r.outputs_at(2.5).map(|o| o.buzzer_hz), Some(200));
    assert_eq!(r.outputs_at(-1.0), None);
    assert_eq!(r.skipped(), 0);
}

#[test]
fn seeks_by_decoding_one_keyframe() {
    let bytes = recording(500, 1);
    let mut r = open(&bytes).unwrap();
    assert_eq!(r.decoded(), 0, "opening decodes no samples");
    let s = r.sample_at(2.345).unwrap().unwrap();
    assert_eq!((s.t_s, r.decoded()), (2.34, 1), "the last sample at or before t, from one keyframe");
    assert_eq!(s.signals.speed_kmh, snapshot(234, 1).signals.speed_kmh);
    assert_eq!(r.sample_at(2.0).unwrap().unwrap().t_s, 2.0, "a sample exactly at t");
    assert_eq!(r.sample_at(1.999).unwrap().unwrap().t_s, 1.99, "the previous keyframe's last sample");
    assert_eq!(r.sample_at(-0.5).unwrap(), None, "before the first sample");
    assert_eq!(r.sample_at(99.0).unwrap().unwrap().t_s, 4.99, "after the end: the last sample");
    assert_eq!(r.keyframe_at(3.7), Some(3));

    let window = r.samples_in(1.5, 2.5).unwrap();
    assert_eq!(window.len(), 100);
    assert_eq!((window.t[0], window.t[99]), (1.51, 2.5));
    assert_eq!(window.pose(99, 0).unwrap().pos[0], 2.5);
}

#[test]
fn a_cut_file_reads_up_to_its_last_whole_chunk() {
    let full = recording(250, 1);
    let total = open(&full).unwrap().sample_count();
    let data_at = PREAMBLE_BYTES + serde_json::to_vec(&header(1)).unwrap().len();
    let mut last = 0;
    let mut cuts: Vec<usize> = (0..full.len()).step_by(13).collect();
    cuts.extend(data_at.saturating_sub(4)..data_at + 64);
    cuts.extend(full.len() - 64..full.len());
    cuts.sort_unstable();
    cuts.dedup();
    for cut in cuts {
        let bytes = &full[..cut];
        let Ok(mut r) = open(bytes) else {
            assert!(cut < data_at, "a cut after the header still opens ({cut} of {})", full.len());
            continue;
        };
        assert!(r.truncated().is_some(), "cut at {cut}");
        let n = r.sample_count();
        assert!(n >= last && n <= total, "samples never go backwards as the cut moves on");
        last = n;
        let samples = r.read_all().unwrap();
        assert_eq!(samples.len() as u64, n);
        let expected: Vec<f64> = (0..n).map(|i| i as f64 / HZ).collect();
        assert_eq!(samples.t, expected, "the samples that survive are the first ones");
        assert!(r.events().len() <= 5);
    }
    assert_eq!(last, total, "the cut just before the trailer keeps everything");
    let r = open(&full).unwrap();
    assert_eq!((r.truncated(), r.sample_count()), (None, total));
}

#[test]
fn a_writer_that_never_finishes_leaves_a_readable_file() {
    let mut bytes = Vec::new();
    let mut w = Writer::new(&mut bytes, &header(0)).unwrap();
    for i in 0..230 {
        w.push(&snapshot(i, 0)).unwrap();
    }
    w.event(&Event { t_s: 2.2, kind: EventKind::Cutoff, text: "cut".into() });
    w.flush().unwrap();
    drop(w);
    let r = open(&bytes).unwrap();
    assert!(r.truncated().unwrap().contains("did not finish"));
    assert_eq!(r.sample_count(), 230, "flush writes the keyframe in progress");
    assert_eq!(r.events().len(), 1);
}

#[test]
fn a_damaged_samples_chunk_is_an_error_when_read() {
    let mut bytes = recording(300, 0);
    let second = open(&bytes).unwrap().keyframe_offset(1) as usize;
    bytes[second + CHUNK_HEADER_BYTES + 3] ^= 0xFF;
    let mut r = open(&bytes).unwrap();
    assert!(r.sample_at(0.5).unwrap().is_some(), "other keyframes still read");
    let error = r.sample_at(1.5).unwrap_err();
    assert!(matches!(error, ReadError::Corrupt { .. }), "{error}");
}

#[test]
fn channels_chunks_and_fields_from_a_later_build_are_kept_or_skipped() {
    let mut h = header(0);
    h.channels.retain(|c| c.key != "pwm_pct");
    h.channels.push(ChannelInfo {
        id: 999,
        key: "future_gizmo".into(),
        label: "Gizmo".into(),
        unit: "zz".into(),
        rate: "medium".into(),
    });
    h.extra.insert("future_field".into(), serde_json::json!({ "anything": [1, 2] }));
    let gizmo = h.channels.len() - 1;
    let mut w = Writer::new(Vec::new(), &h).unwrap();
    for i in 0..120 {
        let mut values = vec![f32::NAN; h.channels.len()];
        values[h.column("speed_kmh").unwrap()] = 5.0;
        values[gizmo] = i as f32;
        w.push_values(f64::from(i) / HZ, &values, &[]).unwrap();
    }
    // A chunk kind and an event kind this build has never heard of.
    w.chunk(ChunkKind::Unknown(42), 1, 0.5, 0.5, b"from the future").unwrap();
    let events = br#"[{"t_s":0.5,"kind":"ghost","text":"?"},{"t_s":0.6,"kind":"alarm","text":"speed"}]"#;
    w.chunk(ChunkKind::Events, 2, 0.5, 0.6, events).unwrap();
    let bytes = w.finish().unwrap();

    let mut r = open(&bytes).unwrap();
    assert_eq!(r.header().extra["future_field"]["anything"][1], 2, "unknown header fields are kept");
    assert_eq!(r.skipped(), 2, "the unknown chunk and the unknown event");
    assert_eq!(r.events().len(), 1);
    assert_eq!(r.events()[0].kind, EventKind::Alarm);
    assert_eq!(r.channel_of(gizmo), None);
    let all = r.read_all().unwrap();
    assert_eq!(all.columns[gizmo][119], 119.0, "an unknown channel reads as a plain column");
    let s = r.snapshot(&all, 10);
    assert_eq!(s.signals.speed_kmh, 5.0);
    assert!(s.signals.pwm_pct.is_nan(), "a channel the file lacks reads as NaN");
}

#[test]
fn refuses_other_files_and_newer_formats() {
    assert!(matches!(open(b""), Err(ReadError::NotARecording)));
    assert!(matches!(open(b"EUCT\x01\0\0\0 not a recording"), Err(ReadError::NotARecording)));
    let mut bytes = recording(10, 0);
    bytes[8] = 2;
    assert!(matches!(open(&bytes), Err(ReadError::Version(2))));
    let mut bytes = recording(10, 0);
    bytes[16] = b'!';
    assert!(matches!(open(&bytes), Err(ReadError::Header(_))));
}

#[test]
fn samples_must_come_in_time_order() {
    let mut w = Writer::new(Vec::new(), &header(0)).unwrap();
    w.push(&snapshot(5, 0)).unwrap();
    assert!(w.push(&snapshot(4, 0)).is_err());
    assert!(w.push_values(1.0, &[0.0], &[]).is_err(), "one value per channel");
}

#[test]
fn every_channel_of_this_build_has_a_group() {
    for spec in CHANNELS {
        assert_ne!(channel_group(spec.key), "other", "{}", spec.key);
    }
    assert_eq!(channel_group("future_gizmo"), "other");
    assert_eq!(ChannelInfo::all().len(), CHANNELS.len());
    assert_eq!(ChannelInfo::all()[Channel::SpeedKmh as usize].unit, "km/h");
}

#[test]
fn utc_formats_civil_dates() {
    assert_eq!(utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(utc(1_792_503_907_000), "2026-10-20T13:45:07Z");
    assert_eq!(utc(951_868_799_000), "2000-02-29T23:59:59Z");
    assert_eq!(utc(946_598_400_999), "1999-12-31T00:00:00Z");
}

fn stored_chunk(kind: ChunkKind, count: u32, payload: &[u8]) -> Vec<u8> {
    let mut head = [0u8; CHUNK_HEADER_BYTES];
    head[0] = kind.code();
    head[4..8].copy_from_slice(&count.to_le_bytes());
    head[8..12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    head[12..16].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    let crc = read::chunk_crc(&head, payload);
    head[32..36].copy_from_slice(&crc.to_le_bytes());
    [head.as_slice(), payload].concat()
}

fn indexed_chunks(h: &Header, chunks: &[(ChunkKind, u32, Vec<u8>)], refs: &[usize]) -> Vec<u8> {
    let json = serde_json::to_vec(h).unwrap();
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&json);
    let mut entries = Vec::new();
    for (kind, count, payload) in chunks {
        let mut entry = [0u8; INDEX_ENTRY_BYTES];
        entry[0] = kind.code();
        entry[4..8].copy_from_slice(&count.to_le_bytes());
        entry[8..16].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
        entries.push(entry);
        bytes.extend(stored_chunk(*kind, *count, payload));
    }
    let index_at = bytes.len() as u64;
    let index: Vec<u8> = refs.iter().flat_map(|&i| entries[i]).collect();
    bytes.extend(stored_chunk(ChunkKind::Index, refs.len() as u32, &index));
    bytes.extend_from_slice(&index_at.to_le_bytes());
    bytes.extend_from_slice(&END_MAGIC);
    bytes
}

fn one_event() -> Vec<u8> {
    br#"[{"t_s":0.0,"kind":"scenario","text":"one"}]"#.to_vec()
}

fn replace_chunk_count(bytes: &mut [u8], offset: usize, count: u32) {
    bytes[offset + 4..offset + 8].copy_from_slice(&count.to_le_bytes());
    let stored = u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap()) as usize;
    let head: [u8; CHUNK_HEADER_BYTES] = bytes[offset..offset + CHUNK_HEADER_BYTES].try_into().unwrap();
    let crc = read::chunk_crc(&head, &bytes[offset + CHUNK_HEADER_BYTES..offset + CHUNK_HEADER_BYTES + stored]);
    bytes[offset + 32..offset + 36].copy_from_slice(&crc.to_le_bytes());
}

#[test]
fn duplicate_index_offsets_cannot_replay_sparse_items() {
    let bytes = indexed_chunks(&header(0), &[(ChunkKind::Events, 1, one_event())], &vec![0; 2000]);
    assert!(matches!(open(&bytes), Err(ReadError::Corrupt { .. })), "duplicate offsets must fail");
}

#[test]
fn index_count_must_match_its_payload() {
    let mut bytes = indexed_chunks(
        &header(0),
        &[(ChunkKind::Events, 1, one_event()), (ChunkKind::Events, 1, one_event())],
        &[0, 1],
    );
    let index_at = u64::from_le_bytes(bytes[bytes.len() - 16..bytes.len() - 8].try_into().unwrap()) as usize;
    replace_chunk_count(&mut bytes, index_at, 1);
    assert!(matches!(open(&bytes), Err(ReadError::Corrupt { .. })), "index count must describe all entries");
}

#[test]
fn index_count_must_match_the_referenced_chunk() {
    let mut bytes = indexed_chunks(&header(0), &[(ChunkKind::Events, 1, one_event())], &[0]);
    let data_at = PREAMBLE_BYTES + serde_json::to_vec(&header(0)).unwrap().len();
    replace_chunk_count(&mut bytes, data_at, 2);
    assert!(matches!(open(&bytes), Err(ReadError::Corrupt { .. })), "the index cannot lie about a chunk");
}

#[test]
fn sparse_count_must_match_the_json_array_even_for_unknown_items() {
    for payload in [one_event(), b"[null,null]".to_vec()] {
        let bytes = indexed_chunks(&header(0), &[(ChunkKind::Events, 0, payload)], &[0]);
        assert!(matches!(open(&bytes), Err(ReadError::Corrupt { .. })), "zero items cannot carry an array item");
    }
}

#[test]
fn sample_count_must_match_chunk_header_before_arrays_are_built() {
    let mut h = header(0);
    h.channels.clear();
    let payload: Vec<u8> = [1u32, 0, 0, 0].into_iter().flat_map(u32::to_le_bytes).chain(0f64.to_le_bytes()).collect();
    let bytes = indexed_chunks(&h, &[(ChunkKind::Samples, 2, payload)], &[0]);
    let result = open(&bytes).and_then(|mut reader| reader.read_all());
    assert!(matches!(result, Err(ReadError::Corrupt { .. })), "the sample count cannot disagree with the payload");
}

struct GuardedRead<'a> {
    cursor: Cursor<&'a [u8]>,
    forbidden: std::ops::Range<u64>,
    touched: std::rc::Rc<std::cell::Cell<bool>>,
}

impl std::io::Read for GuardedRead<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let start = self.cursor.position();
        if start < self.forbidden.end && start + out.len() as u64 > self.forbidden.start {
            self.touched.set(true);
            return Err(std::io::Error::other("the reader reached a forbidden payload"));
        }
        std::io::Read::read(&mut self.cursor, out)
    }
}

impl std::io::Seek for GuardedRead<'_> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.cursor, pos)
    }
}

fn rejects_before_payload(bytes: &[u8], forbidden: std::ops::Range<u64>, limits: ReadLimits) {
    let touched = std::rc::Rc::new(std::cell::Cell::new(false));
    let src = GuardedRead { cursor: Cursor::new(bytes), forbidden, touched: touched.clone() };
    let result = Reader::new_with_limits(src, limits);
    assert!(matches!(result, Err(ReadError::Limit { .. })), "the metadata must trigger an allocation limit");
    assert!(!touched.get(), "the payload must remain unread");
}

fn first_payload(bytes: &[u8]) -> std::ops::Range<u64> {
    let data_at = PREAMBLE_BYTES + u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let stored = u32::from_le_bytes(bytes[data_at + 8..data_at + 12].try_into().unwrap()) as usize;
    let start = data_at + CHUNK_HEADER_BYTES;
    start as u64..(start + stored) as u64
}

#[test]
fn limits_apply_before_the_header_is_allocated() {
    let bytes = indexed_chunks(&header(0), &[], &[]);
    rejects_before_payload(&bytes, 16..17, ReadLimits { max_header_bytes: 1, ..ReadLimits::default() });
    rejects_before_payload(&bytes, 0..1, ReadLimits { max_file_bytes: 1, ..ReadLimits::default() });
}

#[test]
fn sparse_totals_are_bounded_before_any_sparse_payload_is_read() {
    let bytes = indexed_chunks(
        &header(0),
        &[(ChunkKind::Events, 1, one_event()), (ChunkKind::Events, 1, one_event())],
        &[0, 1],
    );
    for limits in [
        ReadLimits { max_events: 1, ..ReadLimits::default() },
        ReadLimits { max_sparse_bytes: one_event().len() as u64, ..ReadLimits::default() },
        ReadLimits { max_chunks: 1, ..ReadLimits::default() },
    ] {
        rejects_before_payload(&bytes, first_payload(&bytes), limits);
    }
}

#[test]
fn sample_budgets_count_unknown_columns_before_decoding() {
    let mut h = header(0);
    h.channels.truncate(1);
    h.channels.push(ChannelInfo {
        id: 999,
        key: "future".into(),
        label: String::new(),
        unit: String::new(),
        rate: String::new(),
    });
    let payload: Vec<u8> = [2u32, 2, 0, 0].into_iter().flat_map(u32::to_le_bytes).chain([0u8; 32]).collect();
    let bytes = indexed_chunks(&h, &[(ChunkKind::Samples, 2, payload)], &[0]);
    for limits in [
        ReadLimits { max_samples: 1, ..ReadLimits::default() },
        ReadLimits { max_sample_values: 3, ..ReadLimits::default() },
    ] {
        rejects_before_payload(&bytes, first_payload(&bytes), limits);
    }
}

#[test]
fn chunk_limits_apply_before_compressed_payloads_are_read() {
    let mut w = Writer::new(Vec::new(), &header(0)).unwrap();
    w.event(&Event { t_s: 0.0, kind: EventKind::Scenario, text: "a".repeat(4096) });
    let bytes = w.finish().unwrap();
    let payload = first_payload(&bytes);
    assert_eq!(bytes[payload.start as usize - CHUNK_HEADER_BYTES + 1], 1, "fixture must use compression");
    rejects_before_payload(&bytes, payload, ReadLimits { max_chunk_bytes: 128, ..ReadLimits::default() });
}

#[test]
fn unknown_chunks_need_no_payload_allocation_or_known_codec() {
    let mut bytes = indexed_chunks(&header(0), &[(ChunkKind::Unknown(42), 9, vec![0u8; 256])], &[0]);
    let payload = first_payload(&bytes);
    let head_at = payload.start as usize - CHUNK_HEADER_BYTES;
    bytes[head_at + 1] = 99;
    replace_chunk_count(&mut bytes, head_at, 9);
    let touched = std::rc::Rc::new(std::cell::Cell::new(false));
    let src = GuardedRead { cursor: Cursor::new(bytes.as_slice()), forbidden: payload, touched: touched.clone() };
    let reader = Reader::new_with_limits(src, ReadLimits { max_chunk_bytes: 64, ..ReadLimits::default() }).unwrap();
    assert_eq!(reader.skipped(), 1);
    assert!(!touched.get(), "an indexed unknown payload stays unread");
}

#[test]
fn recovery_checks_cumulative_budgets_before_decompression() {
    let mut bytes = indexed_chunks(
        &header(0),
        &[(ChunkKind::Events, 1, one_event()), (ChunkKind::Events, 1, one_event())],
        &[0, 1],
    );
    let index_at = u64::from_le_bytes(bytes[bytes.len() - 16..bytes.len() - 8].try_into().unwrap()) as usize;
    bytes.truncate(index_at);
    let payload = first_payload(&bytes);
    let at = payload.start as usize - CHUNK_HEADER_BYTES;
    // CRC is valid, but these bytes are not zstd. A decoder that runs before the metadata
    // pass would report corrupt compression instead of the cumulative item limit.
    bytes[at + 1] = 1;
    replace_chunk_count(&mut bytes, at, 1);
    let result = Reader::new_with_limits(Cursor::new(bytes), ReadLimits { max_events: 1, ..ReadLimits::default() });
    assert!(matches!(result, Err(ReadError::Limit { what: "events", .. })));
}

#[test]
fn an_unfinished_file_skips_unknown_codecs_without_decompression() {
    let mut bytes = indexed_chunks(&header(0), &[(ChunkKind::Unknown(42), 1, vec![7u8; 256])], &[0]);
    let index_at = u64::from_le_bytes(bytes[bytes.len() - 16..bytes.len() - 8].try_into().unwrap()) as usize;
    bytes.truncate(index_at);
    let at = first_payload(&bytes).start as usize - CHUNK_HEADER_BYTES;
    bytes[at + 1] = 99;
    replace_chunk_count(&mut bytes, at, 1);
    let r = Reader::new_with_limits(Cursor::new(bytes), ReadLimits { max_chunk_bytes: 64, ..ReadLimits::default() })
        .unwrap();
    assert_eq!(r.skipped(), 1);
    assert!(r.truncated().is_some());
}

#[test]
fn default_reader_can_stream_beyond_a_desktop_sample_limit() {
    let bytes = recording(500, 1);
    let limited =
        Reader::new_with_limits(Cursor::new(&bytes), ReadLimits { max_samples: 100, ..ReadLimits::default() });
    assert!(matches!(limited, Err(ReadError::Limit { what: "samples", .. })));
    let mut streaming = Reader::new(Cursor::new(&bytes)).unwrap();
    assert_eq!(streaming.sample_count(), 500);
    assert_eq!(streaming.sample_at(4.9).unwrap().unwrap().t_s, 4.9);
}

#[test]
fn nonfinite_chunk_bounds_cannot_hide_declared_samples() {
    let mut h = header(0);
    h.channels.clear();
    let payload: Vec<u8> = [1u32, 0, 0, 0].into_iter().flat_map(u32::to_le_bytes).chain(0f64.to_le_bytes()).collect();
    let mut bytes = indexed_chunks(&h, &[(ChunkKind::Samples, 1, payload)], &[0]);
    let data_at = PREAMBLE_BYTES + serde_json::to_vec(&h).unwrap().len();
    let index_at = u64::from_le_bytes(bytes[bytes.len() - 16..bytes.len() - 8].try_into().unwrap()) as usize;
    bytes[data_at + 24..data_at + 32].copy_from_slice(&f64::NEG_INFINITY.to_le_bytes());
    let index_end = index_at + CHUNK_HEADER_BYTES + 24;
    bytes[index_end..index_end + 8].copy_from_slice(&f64::NEG_INFINITY.to_le_bytes());
    replace_chunk_count(&mut bytes, data_at, 1);
    replace_chunk_count(&mut bytes, index_at, 1);
    let result = open(&bytes).and_then(|mut reader| reader.read_all());
    assert!(matches!(result, Err(ReadError::Corrupt { .. })), "invalid times must not hide a sample");
}
