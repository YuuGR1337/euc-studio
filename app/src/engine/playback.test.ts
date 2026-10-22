import { describe, expect, it } from "vitest";
import { PlaybackEngine } from "./playback.ts";
import type { Recording } from "./recording.ts";
import { CHANNELS, CH, POSE_FLOATS } from "./telemetry.gen.ts";

function recording(bodies = 2, times: [number, number, number] = [10, 10.1, 10.3]): Recording {
  const poses = new Float32Array(times.length * bodies * POSE_FLOATS);
  for (let sample = 0; sample < times.length; sample++) {
    const x = [0, 4, 12][sample] as number;
    const angle = (([0, 170, -170][sample] as number) * Math.PI) / 180;
    const scale = sample + 1;
    for (let body = 0; body < bodies; body++) {
      poses.set(
        [x + body * 10, 2 * x, -x, scale * Math.cos(angle / 2), 0, 0, scale * Math.sin(angle / 2)],
        (sample * bodies + body) * POSE_FLOATS,
      );
    }
  }
  return {
    name: "pose-regression.eucr",
    header: {
      wheel: "test wheel",
      firmware: "test firmware",
      scenario: "test recording",
      sample_hz: 60,
      bodies: Array.from({ length: bodies }, (_, index) => ({
        name: `body-${index}`,
        role: index === 0 ? "wheel" : "shell",
        geoms: [],
      })),
      channels: [...CHANNELS],
    },
    start: times[0],
    end: times[2],
    droppedSamples: 0,
    times: Float64Array.from(times),
    signals: CHANNELS.map((_, channel) =>
      Float32Array.from(times, (_, sample) => channel * 100 + sample),
    ),
    poses,
    events: [{ t_s: 10.1, kind: "command", text: "Recorded event" }],
  };
}

describe("PlaybackEngine visual pose interpolation", () => {
  it("replays only recorded output and phone tracks at the playhead, including backward seeks", () => {
    const source = recording();
    source.outputs = [
      {
        t_s: 10.1,
        outputs: {
          display_grid: [1],
          display_on: true,
          display_brightness: 7,
          leds: [[1, 2, 3]],
          buzzer_hz: 1000,
        },
      },
      {
        t_s: 10.3,
        outputs: {
          display_grid: [2],
          display_on: true,
          display_brightness: 7,
          leds: [],
          buzzer_hz: 0,
        },
      },
    ];
    source.frames = [
      { id: 1, t: 10.1, kind: "text", summary: "NAME:FIXTURE", fields: [], hex: "4e" },
    ];
    const playback = new PlaybackEngine(source);
    expect(playback.outputs).toBeNull();
    expect(playback.frames).toEqual([]);
    playback.seek(10.2);
    expect(playback.outputs?.buzzer_hz).toBe(1000);
    expect(playback.frames.map((frame) => frame.summary)).toEqual(["NAME:FIXTURE"]);
    playback.seek(10);
    expect(playback.outputs).toBeNull();
    expect(playback.frames).toEqual([]);
    playback.setPlaying(true);
    playback.advance(0.3);
    expect(playback.outputs?.display_grid).toEqual([2]);
    expect(playback.outputs?.buzzer_hz).toBe(0);
  });
  it.each(["rigid_ankle_v1", "articulated_sagittal_v1", "articulated_contact_v1"] as const)(
    "describes the saved %s model in free-ride playback",
    (rider_model) => {
      const source = recording();
      source.header.scenario = null;
      source.header.context = {
        schema: 1,
        run_mode: "free_ride",
        rider_model,
        model_aids: {
          roll_stiffness: 0,
          roll_damping: 0,
          steer_kp: 0,
          steer_kd: 0,
          steer_max_nm: 0,
        },
        height_map: {
          revision: 1,
          nx: 129,
          ny: 129,
          spacing_m: 0.25,
          origin_xy_m: [-16, -16],
          heights_mm: new Array<number>(129 * 129).fill(0),
        },
      };
      const entry = new PlaybackEngine(source).scenarios()[0];
      expect(entry?.label).toBe("Recorded free ride");
      expect(entry?.detail).toBe(
        {
          rigid_ankle_v1: "Rigid baseline with recorded external aids",
          articulated_sagittal_v1: "Articulated with recorded external aids",
          articulated_contact_v1:
            "Articulated, foot contact (experimental) with recorded external aids",
        }[rider_model],
      );
    },
  );

  it("brackets actual timestamps and all bodies without altering raw telemetry or the recording", () => {
    const source = recording();
    const original = source.poses.slice();
    const engine = new PlaybackEngine(source);
    engine.seek(10.2);
    const raw = engine.latest.poses?.slice();
    const signals = engine.channels.speed.toArray();
    const epoch = engine.poseEpoch;
    const out = new Float32Array(2 * POSE_FLOATS + 2).fill(99);
    expect(engine.samplePoses(out, 0)).toBeCloseTo(10.2, 10);
    for (let body = 0; body < 2; body++) {
      const at = body * POSE_FLOATS;
      expect(out[at]).toBeCloseTo(8 + 10 * body, 5);
      expect(out[at + 1]).toBeCloseTo(16, 5);
      expect(out[at + 2]).toBeCloseTo(-8, 5);
      expect(Math.abs(out[at + 3] as number)).toBeLessThan(1e-6);
      expect(Math.abs(out[at + 6] as number)).toBeCloseTo(1, 6);
      expect(Math.hypot(...out.subarray(at + 3, at + 7))).toBeCloseTo(1, 6);
    }
    expect(engine.samplePoses(out, 1e9)).toBeCloseTo(10.2, 10); // No wall-time latency or drift.
    expect(out[2 * POSE_FLOATS]).toBe(99);
    expect(engine.latest.t).toBe(10.1);
    expect(engine.latest.speed).toBe(source.signals[CH.speed_kmh]?.[1]);
    expect(engine.latest.poses).toEqual(raw);
    expect(engine.channels.speed.toArray()).toEqual(signals);
    expect(source.poses).toEqual(original);
    expect(engine.poseEpoch).toBe(epoch);
  });

  it.each([0.25, 1, 8])("advances at RTF %s with no extra presentation delay", (rtf) => {
    const engine = new PlaybackEngine(recording());
    engine.setTargetRtf(rtf);
    engine.setPlaying(true);
    const epoch = engine.poseEpoch;
    engine.advance(0.01);
    const expected = 10 + 0.01 * rtf;
    const out = new Float32Array(2 * POSE_FLOATS);
    expect(engine.position).toBeCloseTo(expected, 10);
    expect(engine.samplePoses(out, 10)).toBeCloseTo(expected, 10);
    expect(out[0]).toBeCloseTo((4 * (expected - 10)) / 0.1, 5);
    expect(engine.poseEpoch).toBe(epoch);
  });

  it.each([0.25, 1, 8].flatMap((rtf) => [30, 60, 120, 144, 240].map((hz) => [rtf, hz])))(
    "tracks the same recorded curve at RTF %s and %s render Hz",
    (rtf, hz) => {
      const engine = new PlaybackEngine(recording(2, [10, 12, 16]));
      engine.setTargetRtf(rtf);
      engine.setPlaying(true);
      const epoch = engine.poseEpoch;
      const out = new Float32Array(2 * POSE_FLOATS);
      let previous = 10;
      for (let render = 0; render <= hz; render++) {
        if (render > 0) engine.advance(1 / hz);
        const t = engine.samplePoses(out, (render * 1000) / hz);
        const expected = Math.min(16, 10 + (render / hz) * rtf);
        expect(t).toBeCloseTo(expected, 9);
        expect(t).toBeGreaterThanOrEqual(previous);
        expect(t).toBeLessThanOrEqual(16);
        expect(out[0]).toBeCloseTo(2 * (expected - 10), 5);
        expect(out[POSE_FLOATS]).toBeCloseTo(10 + 2 * (expected - 10), 5);
        expect(Math.hypot(...out.subarray(3, 7))).toBeCloseTo(1, 6);
        previous = t;
      }
      expect(engine.poseEpoch).toBe(epoch);
    },
  );

  it("clamps both ends and never extrapolates, including after a long pause", () => {
    const engine = new PlaybackEngine(recording());
    const out = new Float32Array(2 * POSE_FLOATS);
    engine.seek(-100);
    expect(engine.samplePoses(out, 0)).toBe(10);
    expect(out[0]).toBe(0);
    engine.seek(1000);
    expect(engine.samplePoses(out, 0)).toBe(10.3);
    expect(out[0]).toBe(12);
    const end = out.slice();
    expect(engine.samplePoses(out, 1e9)).toBe(10.3);
    expect(out).toEqual(end);
    expect(engine.advance(100)).toBe(0);
    expect(engine.position).toBe(10.3);
  });

  it("bumps epoch for explicit seeks, restart, and paused step, but ignores invalid seeks", () => {
    const engine = new PlaybackEngine(recording());
    const start = engine.poseEpoch;
    engine.seek(10.2);
    expect(engine.poseEpoch).toBe(start + 1);
    engine.seek(10.2);
    expect(engine.poseEpoch).toBe(start + 2);
    engine.seek(NaN);
    engine.seek(Infinity);
    expect(engine.poseEpoch).toBe(start + 2);
    expect(engine.position).toBe(10.2);
    engine.step();
    expect(engine.poseEpoch).toBe(start + 3);
    const out = new Float32Array(2 * POSE_FLOATS);
    expect(engine.samplePoses(out, 0)).toBe(10.3);
    expect(out[0]).toBe(12);
    engine.restart();
    expect(engine.poseEpoch).toBe(start + 4);
    expect(engine.samplePoses(out, 0)).toBe(10);
    expect(out[0]).toBe(0);
  });

  it("keeps a fractional playhead still while paused and restarts play at the recording end", () => {
    const engine = new PlaybackEngine(recording());
    const out = new Float32Array(2 * POSE_FLOATS);
    engine.seek(10.2);
    expect(engine.advance(1)).toBe(0);
    expect(engine.samplePoses(out, 100)).toBeCloseTo(10.2);
    expect(engine.samplePoses(out, 1000)).toBeCloseTo(10.2);
    engine.seek(10.3);
    const epoch = engine.poseEpoch;
    engine.setPlaying(true);
    expect(engine.poseEpoch).toBe(epoch + 1);
    expect(engine.position).toBe(10);
    expect(engine.playing).toBe(true);
  });

  it("rejects aliases to recorded or raw latest poses without modifying either", () => {
    const source = recording();
    const engine = new PlaybackEngine(source);
    const raw = engine.latest.poses;
    if (!raw) throw new Error("Expected a raw frame");
    const originalRecording = source.poses.slice();
    const originalRaw = raw.slice();
    for (const out of [
      source.poses,
      source.poses.subarray(POSE_FLOATS),
      raw,
      new Float32Array(raw.buffer),
    ]) {
      expect(engine.samplePoses(out, 0)).toBeNaN();
    }
    expect(source.poses).toEqual(originalRecording);
    expect(raw).toEqual(originalRaw);
  });

  it("rejects an invalid quaternion in any body before writing output", () => {
    const source = recording();
    source.poses[2 * 2 * POSE_FLOATS + POSE_FLOATS + 3] = NaN;
    const engine = new PlaybackEngine(source);
    engine.seek(10.2);
    const out = new Float32Array(2 * POSE_FLOATS).fill(99);
    expect(engine.samplePoses(out, 0)).toBeNaN();
    expect(Array.from(out)).toEqual(Array(2 * POSE_FLOATS).fill(99));
    expect(engine.samplePoses(new Float32Array(6), 0)).toBeNaN();
  });

  it("supports all 128 bodies and rejects absent or excess bodies", () => {
    const engine = new PlaybackEngine(recording(128));
    engine.seek(10.2);
    const out = new Float32Array(129 * POSE_FLOATS).fill(99);
    expect(engine.samplePoses(out, 0)).toBeCloseTo(10.2);
    expect(out[127 * POSE_FLOATS]).toBe(1278);
    expect(out[128 * POSE_FLOATS]).toBe(99);
    expect(new PlaybackEngine(recording(129)).samplePoses(out, 0)).toBeNaN();
    expect(new PlaybackEngine(recording(0)).samplePoses(out, 0)).toBeNaN();
  });
});
