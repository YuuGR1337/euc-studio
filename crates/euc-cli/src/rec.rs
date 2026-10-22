//! `euc rec info` and `euc rec export`: what an `.eucrec` recording holds, and its contents as
//! CSV (docs/recordings.md).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use euc_telemetry::recording::{Header, Reader, Samples, channel_group};
use euc_telemetry::{Event, EventKind, Pose};
use serde::Serialize;

use crate::ui::{self, DIM, GOOD, STRONG, WARN};
use crate::{Context, Verdict};

#[derive(Debug, clap::Subcommand)]
pub(crate) enum Command {
    /// Show a recording's header, length, channels and events.
    Info {
        /// The recording (.eucrec).
        file: PathBuf,
        /// Events to list (all with --json).
        #[arg(long, default_value_t = 30)]
        events: usize,
    },
    /// Write a recording's samples, poses, events and frames as CSV files.
    Export {
        /// The recording (.eucrec).
        file: PathBuf,
        /// Folder for the CSV files: one per channel group, plus poses, events, frames, outputs
        /// and the channel list (created if missing).
        #[arg(long, value_name = "DIR")]
        csv: PathBuf,
    },
}

pub(crate) fn run(ctx: &Context, command: Command) -> anyhow::Result<Verdict> {
    match command {
        Command::Info { file, events } => info(ctx, &file, events),
        Command::Export { file, csv } => export(ctx, &file, &csv),
    }
}

fn open(file: &Path) -> anyhow::Result<Reader<std::io::BufReader<fs::File>>> {
    Reader::open(file).with_context(|| format!("cannot read {}", ui::path(file)))
}

/// What `rec info --json` prints.
#[derive(Serialize)]
struct Info<'a> {
    file: String,
    header: &'a Header,
    started_utc: String,
    start_s: f64,
    end_s: f64,
    samples: u64,
    keyframes: usize,
    frames: usize,
    frames_by_kind: BTreeMap<&'a str, usize>,
    outputs: usize,
    events: &'a [Event],
    /// Why the file ends early, if it does.
    truncated: Option<&'a str>,
    /// Chunks and items of kinds this build does not know.
    skipped: usize,
}

fn info(ctx: &Context, file: &Path, max_events: usize) -> anyhow::Result<Verdict> {
    let reader = open(file)?;
    let h = reader.header();
    let mut frames_by_kind = BTreeMap::new();
    for frame in reader.frames() {
        *frames_by_kind.entry(frame.kind.as_str()).or_insert(0) += 1;
    }
    let info = Info {
        file: ui::path(file),
        header: h,
        started_utc: h.started_utc(),
        start_s: reader.start_s(),
        end_s: reader.end_s(),
        samples: reader.sample_count(),
        keyframes: reader.keyframe_times().len(),
        frames: reader.frames().len(),
        frames_by_kind,
        outputs: reader.outputs().len(),
        events: reader.events(),
        truncated: reader.truncated(),
        skipped: reader.skipped(),
    };
    if ctx.json {
        ui::json(&info)?;
        return Ok(Verdict::Ok);
    }
    let short = |sha: &Option<String>| sha.as_deref().map_or_else(String::new, |s| ui::safe(&s[..s.len().min(12)]));
    ui::row(STRONG, "file", &info.file);
    let git = h.git_rev.as_deref().map_or_else(String::new, |r| format!(", git {}", ui::safe(&r[..r.len().min(12)])));
    ui::row(STRONG, "recorded", format!("{} by {}{git}", info.started_utc, ui::safe(&h.producer)));
    ui::row(STRONG, "wheel", format!("{}  {}", ui::safe(&h.wheel), ui::paint(DIM, short(&h.profile_sha256))));
    ui::row(STRONG, "firmware", format!("{}  {}", ui::safe(&h.firmware), ui::paint(DIM, short(&h.firmware_sha256))));
    let mut engine = Vec::new();
    if let Some(cpu) = &h.cpu {
        engine.push(format!("cpu {}", ui::safe(cpu)));
    }
    if let Some(cpi) = h.cpi {
        engine.push(format!("CPI {cpi}"));
    }
    if let Some(seed) = h.seed {
        engine.push(format!("noise seed {seed}"));
    }
    if !engine.is_empty() {
        ui::row(STRONG, "engine", engine.join(", "));
    }
    let scenario = h.scenario.as_deref().map_or_else(|| "none".to_owned(), ui::safe);
    let settings = h.settings.as_deref().map_or_else(|| "blank".to_owned(), ui::safe);
    ui::row(STRONG, "scenario", format!("{scenario}, settings {settings}"));
    ui::row(
        STRONG,
        "length",
        format!(
            "{:.2} s ({:.2} to {:.2} s), {} samples at {} Hz, {} keyframes",
            info.end_s - info.start_s,
            info.start_s,
            info.end_s,
            ui::thousands(info.samples),
            h.sample_hz,
            info.keyframes
        ),
    );
    let keys: Vec<String> = h.channels.iter().map(|c| ui::safe(&c.key)).collect();
    ui::row(STRONG, "channels", format!("{}: {}", keys.len(), keys.join(" ")));
    let bodies: Vec<String> = h.bodies.iter().map(|b| ui::safe(&b.name)).collect();
    ui::row(STRONG, "bodies", format!("{}: {}", bodies.len(), bodies.join(" ")));
    let kinds: Vec<String> = info.frames_by_kind.iter().map(|(k, n)| format!("{} {n}", ui::safe(k))).collect();
    ui::row(STRONG, "frames", format!("{} BLE frames ({})", info.frames, kinds.join(", ")));
    ui::row(STRONG, "outputs", format!("{} board output changes", info.outputs));
    ui::row(STRONG, "events", info.events.len());
    for event in info.events.iter().take(max_events) {
        let style = if matches!(event.kind, EventKind::Cutoff | EventKind::Fault) { WARN } else { DIM };
        ui::detail(ui::paint(
            style,
            format!("{:8.2} s  {:<9} {}", event.t_s, kind_name(event.kind), ui::safe(&event.text)),
        ));
    }
    if info.events.len() > max_events {
        ui::detail(ui::paint(DIM, format!("... {} more (--events N, or --json)", info.events.len() - max_events)));
    }
    if info.skipped > 0 {
        ui::row(DIM, "skipped", format!("{} chunks or items of kinds this build does not know", info.skipped));
    }
    match info.truncated {
        Some(why) => ui::row(WARN, "truncated", format!("{why}; everything before it reads normally")),
        None => ui::row(GOOD, "OK", "complete"),
    }
    Ok(Verdict::Ok)
}

/// `alarm`, `cutoff`, ... as the JSON spells it.
fn kind_name(kind: EventKind) -> String {
    serde_json::to_value(kind).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

/// A CSV field, quoted when it needs to be.
fn field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) { format!("\"{}\"", text.replace('"', "\"\"")) } else { text.to_owned() }
}

/// A value, empty when not modelled; exponent notation for very small and large ones.
fn value(v: f32) -> String {
    if v.is_nan() { String::new() } else { format!("{v:?}") }
}

/// The export, written to `dir`: what each file holds.
#[derive(Serialize)]
struct Written {
    file: String,
    rows: usize,
}

fn export(ctx: &Context, file: &Path, dir: &Path) -> anyhow::Result<Verdict> {
    let mut reader = open(file)?;
    let samples = reader.read_all().with_context(|| format!("cannot read {}", ui::path(file)))?;
    fs::create_dir_all(dir).with_context(|| format!("cannot create {}", ui::path(dir)))?;
    let h = reader.header().clone();
    let mut written = Vec::new();
    let mut write = |name: &str, text: String, rows: usize| -> anyhow::Result<()> {
        let path = dir.join(name);
        fs::write(&path, text).with_context(|| format!("cannot write {}", ui::path(&path)))?;
        written.push(Written { file: ui::path(&path), rows });
        Ok(())
    };

    // One file per channel group, in the order the groups first appear.
    let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
    for (i, channel) in h.channels.iter().enumerate() {
        let group = channel_group(&channel.key);
        match groups.iter_mut().find(|(g, _)| *g == group) {
            Some((_, columns)) => columns.push(i),
            None => groups.push((group, vec![i])),
        }
    }
    for (group, columns) in &groups {
        write(&format!("{group}.csv"), group_csv(&h, &samples, columns), samples.len())?;
    }
    let mut channels = String::from("key,label,unit,group,id\n");
    for c in &h.channels {
        let _ = writeln!(
            channels,
            "{},{},{},{},{}",
            field(&c.key),
            field(&c.label),
            field(&c.unit),
            channel_group(&c.key),
            c.id
        );
    }
    write("channels.csv", channels, h.channels.len())?;
    if !h.bodies.is_empty() {
        write("poses.csv", poses_csv(&h, &samples), samples.len())?;
    }
    let mut events = String::from("t_s,kind,text\n");
    for e in reader.events() {
        let _ = writeln!(events, "{},{},{}", e.t_s, kind_name(e.kind), field(&e.text));
    }
    write("events.csv", events, reader.events().len())?;
    let mut frames = String::from("t_s,kind,reply,summary,hex\n");
    for f in reader.frames() {
        let _ = writeln!(
            frames,
            "{},{},{},{},{}",
            f.t_s,
            field(&f.kind),
            f.reply,
            field(&f.summary),
            euc_proto_begode::describe::hex(&f.bytes)
        );
    }
    write("frames.csv", frames, reader.frames().len())?;
    let mut outputs = String::from("t_s,display_on,display_brightness,buzzer_hz,display_grid,leds\n");
    for o in reader.outputs() {
        let leds: Vec<String> = o.outputs.leds.iter().map(|[r, g, b]| format!("{r:02X}{g:02X}{b:02X}")).collect();
        let _ = writeln!(
            outputs,
            "{},{},{},{},{},{}",
            o.t_s,
            o.outputs.display_on,
            o.outputs.display_brightness,
            o.outputs.buzzer_hz,
            euc_proto_begode::describe::hex(&o.outputs.display_grid),
            leds.join(" ")
        );
    }
    write("outputs.csv", outputs, reader.outputs().len())?;

    if ctx.json {
        ui::json(&written)?;
    } else {
        for w in &written {
            ui::row(GOOD, "wrote", format!("{}  {}", w.file, ui::paint(DIM, format!("{} rows", w.rows))));
        }
        if let Some(why) = reader.truncated() {
            ui::row(WARN, "truncated", format!("{why}; exported what it holds"));
        }
    }
    Ok(Verdict::Ok)
}

fn group_csv(h: &Header, samples: &Samples, columns: &[usize]) -> String {
    let mut out = String::from("t_s");
    for &c in columns {
        out.push(',');
        out.push_str(&field(&h.channels[c].key));
    }
    out.push('\n');
    for (i, t) in samples.t.iter().enumerate() {
        let _ = write!(out, "{t:?}");
        for &c in columns {
            out.push(',');
            out.push_str(&value(samples.columns[c][i]));
        }
        out.push('\n');
    }
    out
}

fn poses_csv(h: &Header, samples: &Samples) -> String {
    let mut out = String::from("t_s");
    for body in &h.bodies {
        for axis in ["x", "y", "z", "qw", "qx", "qy", "qz"] {
            let _ = write!(out, ",{}", field(&format!("{}_{axis}", body.name)));
        }
    }
    out.push('\n');
    for (i, t) in samples.t.iter().enumerate() {
        let _ = write!(out, "{t:?}");
        for b in 0..samples.bodies {
            for v in samples.pose(i, b).map_or([f32::NAN; 7], Pose::to_array) {
                out.push(',');
                out.push_str(&value(v));
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_fields_are_quoted_when_needed() {
        assert_eq!(field("plain"), "plain");
        assert_eq!(field("a, b"), "\"a, b\"");
        assert_eq!(field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(value(f32::NAN), "");
        assert_eq!(value(1.5), "1.5");
        assert_eq!(value(1.5e-21), "1.5e-21");
        assert_eq!(kind_name(EventKind::Tiltback), "tiltback");
    }
}
