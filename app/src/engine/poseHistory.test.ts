import { describe, expect, it } from "vitest";
import {
  interpolatePoseFrames,
  POSE_HISTORY_FRAMES,
  POSE_MAX_BODIES,
  POSE_SIM_GAP_S,
  POSE_WALL_GAP_MS,
  PoseHistory,
  validPoseFrame,
} from "./poseHistory.ts";
import { POSE_FLOATS } from "./telemetry.gen.ts";

function frame(x: number, yaw = 0, scale = 1, bodies = 1): Float32Array {
  const poses = new Float32Array(bodies * POSE_FLOATS);
  for (let body = 0; body < bodies; body++) {
    poses.set(
      [x + body * 10, x * 2, -x, Math.cos(yaw / 2) * scale, 0, 0, Math.sin(yaw / 2) * scale],
      body * POSE_FLOATS,
    );
  }
  return poses;
}

function filledHistory(): PoseHistory {
  const history = new PoseHistory();
  history.reset(1);
  for (let i = 0; i <= 60; i++) history.push(i / 60, frame(i / 60), 0, 100);
  return history;
}

function expectQuaternion(out: Float32Array, yaw: number, body = 0) {
  const at = body * POSE_FLOATS + 3;
  const w = out[at] as number;
  const x = out[at + 1] as number;
  const y = out[at + 2] as number;
  const z = out[at + 3] as number;
  expect(Math.hypot(w, x, y, z)).toBeCloseTo(1, 6);
  // q and -q describe the same rotation.
  expect(Math.abs(w * Math.cos(yaw / 2) + z * Math.sin(yaw / 2))).toBeCloseTo(1, 6);
  expect(x).toBeCloseTo(0, 6);
  expect(y).toBeCloseTo(0, 6);
}

describe("interpolatePoseFrames", () => {
  it.each([0, 0.25, 0.5, 0.75, 1])(
    "interpolates translations and normalized shortest slerp at %s",
    (alpha) => {
      const a = frame(2, 0, 2, 2);
      const b = frame(6, Math.PI / 2, -3, 2);
      const beforeA = a.slice();
      const beforeB = b.slice();
      const out = new Float32Array(2 * POSE_FLOATS + 3).fill(777);
      expect(interpolatePoseFrames(out, a, 0, b, 0, 2, alpha)).toBe(true);
      for (let body = 0; body < 2; body++) {
        expect(out[body * POSE_FLOATS]).toBeCloseTo(2 + 4 * alpha + 10 * body, 6);
        expect(out[body * POSE_FLOATS + 1]).toBeCloseTo(4 + 8 * alpha, 6);
        expect(out[body * POSE_FLOATS + 2]).toBeCloseTo(-2 - 4 * alpha, 6);
        expectQuaternion(out, (alpha * Math.PI) / 2, body);
      }
      expect(Array.from(out.subarray(2 * POSE_FLOATS))).toEqual([777, 777, 777]);
      expect(a).toEqual(beforeA);
      expect(b).toEqual(beforeB);
    },
  );

  it("crosses the ±180° boundary through 180°, not through zero", () => {
    const out = new Float32Array(POSE_FLOATS);
    expect(
      interpolatePoseFrames(
        out,
        frame(0, (170 * Math.PI) / 180),
        0,
        frame(1, (-170 * Math.PI) / 180),
        0,
        1,
        0.5,
      ),
    ).toBe(true);
    expectQuaternion(out, Math.PI);
    expect(Math.abs(out[3] as number)).toBeLessThan(1e-6);
  });

  it.each([0, 0.00001])("normalizes the near-identical/antipodal lerp branch at yaw %s", (yaw) => {
    const out = new Float32Array(POSE_FLOATS);
    expect(interpolatePoseFrames(out, frame(0, 0, 2), 0, frame(1, yaw, -4), 0, 1, 0.5)).toBe(true);
    expectQuaternion(out, yaw / 2);
  });

  it("accepts finite large quaternion components without overflowing normalization", () => {
    const out = new Float32Array(POSE_FLOATS);
    expect(interpolatePoseFrames(out, frame(0, 0.8, 1e38), 0, frame(1, 1.2, 1e38), 0, 1, 0.5)).toBe(
      true,
    );
    expectQuaternion(out, 1);
  });

  it.each([
    [0, NaN],
    [1, Infinity],
    [3, NaN],
    [6, -Infinity],
  ])("rejects nonfinite lane %s before writing any body", (lane, value) => {
    const a = frame(0, 0, 1, 2);
    const b = frame(1, 0, 1, 2);
    b[POSE_FLOATS + lane] = value;
    const out = new Float32Array(2 * POSE_FLOATS).fill(99);
    expect(interpolatePoseFrames(out, a, 0, b, 0, 2, 0.5)).toBe(false);
    expect(Array.from(out)).toEqual(Array(2 * POSE_FLOATS).fill(99));
  });

  it.each([0, 1e-20])("rejects degenerate quaternion norm %s", (scale) => {
    const out = new Float32Array(POSE_FLOATS).fill(99);
    expect(validPoseFrame(frame(1, 0, scale), 0, 1)).toBe(false);
    expect(interpolatePoseFrames(out, frame(0), 0, frame(1, 0, scale), 0, 1, 0.5)).toBe(false);
    expect(Array.from(out)).toEqual(Array(POSE_FLOATS).fill(99));
  });

  it.each([-1, 1.01, NaN, Infinity])("rejects extrapolation/invalid alpha %s", (alpha) => {
    const out = new Float32Array(POSE_FLOATS).fill(99);
    expect(interpolatePoseFrames(out, frame(0), 0, frame(1), 0, 1, alpha)).toBe(false);
    expect(out[0]).toBe(99);
  });

  it("rejects both source aliases, including distinct views over the same buffer", () => {
    const a = frame(0);
    const b = frame(1);
    for (const out of [a, b, new Float32Array(a.buffer)]) {
      expect(interpolatePoseFrames(out, a, 0, b, 0, 1, 0.5)).toBe(false);
    }
    expect(a).toEqual(frame(0));
    expect(b).toEqual(frame(1));
  });

  it("checks offsets, truncation, output capacity, and body-count bounds", () => {
    const a = frame(0, 0, 1, POSE_MAX_BODIES);
    const out = new Float32Array(a.length);
    for (const bodies of [0, -1, 1.5, POSE_MAX_BODIES + 1]) {
      expect(validPoseFrame(a, 0, bodies)).toBe(false);
      expect(interpolatePoseFrames(out, a, 0, a, 0, bodies, 0.5)).toBe(false);
    }
    for (const offset of [-1, 0.5, a.length, NaN, Infinity]) {
      expect(validPoseFrame(a, offset, 1)).toBe(false);
    }
    expect(interpolatePoseFrames(new Float32Array(6), a, 0, a, 0, 1, 0.5)).toBe(false);
    expect(validPoseFrame(a.subarray(0, a.length - 1), 0, POSE_MAX_BODIES)).toBe(false);
  });
});

describe("PoseHistory", () => {
  it("leaves output unchanged before data, after reset, and for invalid output/clock", () => {
    const history = new PoseHistory();
    const out = new Float32Array(POSE_FLOATS).fill(99);
    expect(history.sample(out, 100, 1)).toBeNaN();
    history.reset(1);
    history.push(1, frame(5), 0, 100);
    expect(history.sample(out.subarray(0, 6), 100, 1)).toBeNaN();
    expect(history.sample(out, NaN, 1)).toBeNaN();
    const epoch = history.epoch;
    history.reset();
    expect(history.epoch).toBe(epoch + 1);
    expect(history.sample(out, 100, 1)).toBeNaN();
    expect(Array.from(out)).toEqual(Array(POSE_FLOATS).fill(99));
  });

  it("copies every sample from a shared batch buffer at the actual timestamp", () => {
    const history = new PoseHistory();
    history.reset(1);
    const packet = new Float32Array([...frame(0), ...frame(20), ...frame(0)]);
    for (const [index, time] of [1, 1.04, 1.1].entries()) {
      expect(history.push(time, packet, index * POSE_FLOATS, 100)).toBe(true);
    }
    packet.fill(900); // The sidecar owns copies, not views over the transport buffer.
    const out = new Float32Array(POSE_FLOATS);
    expect(history.sample(out, 100, 2)).toBeCloseTo(1);
    expect(out[0]).toBe(0);
    expect(history.sample(out, 105, 2)).toBeCloseTo(1.01);
    expect(out[0]).toBeCloseTo(5, 5);
    expect(history.sample(out, 125, 2)).toBeCloseTo(1.05);
    expect(out[0]).toBeCloseTo(50 / 3, 5);
    expect(history.sample(out, 150, 2)).toBeCloseTo(1.1);
    expect(out[0]).toBe(0);
  });

  it.each([0.25, 1, 8])(
    "drains a paused buffer with the last positive RTF %s, then holds",
    (rtf) => {
      const history = filledHistory();
      const out = new Float32Array(POSE_FLOATS);
      const epoch = history.epoch;
      expect(history.sample(out, 100, rtf)).toBeCloseTo(1 - 0.05 * rtf, 10);
      expect(history.sample(out, 125, 0)).toBeCloseTo(1 - 0.025 * rtf, 10);
      expect(history.sample(out, 150, 0)).toBe(1);
      for (const now of [200, 300, 400]) {
        expect(history.sample(out, now, 0)).toBe(1);
        expect(out[0]).toBe(1);
      }
      expect(history.epoch).toBe(epoch);
    },
  );

  it("presents a paused step exactly on its first sample, without extrapolation", () => {
    const history = filledHistory();
    const out = new Float32Array(POSE_FLOATS);
    history.sample(out, 100, 1);
    const epoch = history.epoch;
    const next = 1 + 1 / 60;
    history.push(next, frame(7, 0.6, 3), 0, 110);
    history.finishStep();
    expect(history.sample(out, 110, 0)).toBe(next);
    expect(out[0]).toBe(7);
    expectQuaternion(out, 0.6);
    expect(history.sample(out, 150, 0)).toBe(next);
    expect(history.epoch).toBe(epoch);
  });

  it("never reverses visual time when measured RTF changes or becomes unavailable", () => {
    const history = filledHistory();
    const out = new Float32Array(POSE_FLOATS);
    const first = history.sample(out, 100, 0.25);
    expect(first).toBeCloseTo(0.9875);
    expect(history.sample(out, 110, 8)).toBe(first);
    expect(history.sample(out, 125, NaN)).toBe(first);
    expect(history.sample(out, 150, 0)).toBe(1);
    expect(out[0]).toBe(1);
  });

  it.each([0.1, 0.05])("resets on duplicate/backward simulation time %s", (time) => {
    const history = new PoseHistory();
    history.reset(1);
    history.push(0.1, frame(0), 0, 100);
    const epoch = history.epoch;
    history.push(time, frame(20), 0, 101);
    const out = new Float32Array(POSE_FLOATS);
    expect(history.epoch).toBe(epoch + 1);
    expect(history.sample(out, 101, 1)).toBe(time);
    expect(out[0]).toBe(20);
  });

  it.each([
    [POSE_SIM_GAP_S + 1e-6, 101],
    [0.05, 100 + POSE_WALL_GAP_MS + 1],
    [0.05, 99],
  ])("resets across an input gap/clock reversal (%s s, %s ms)", (time, now) => {
    const history = new PoseHistory();
    history.reset(1);
    history.push(0, frame(0), 0, 100);
    const epoch = history.epoch;
    history.push(time, frame(20), 0, now);
    const out = new Float32Array(POSE_FLOATS);
    expect(history.epoch).toBe(epoch + 1);
    expect(history.sample(out, now, 1)).toBe(time);
    expect(out[0]).toBe(20);
  });

  it("keeps exact gap boundaries continuous", () => {
    const history = new PoseHistory();
    history.reset(1);
    history.push(0, frame(0), 0, 100);
    const epoch = history.epoch;
    history.push(POSE_SIM_GAP_S, frame(1), 0, 100 + POSE_WALL_GAP_MS);
    expect(history.epoch).toBe(epoch);
    expect(history.sample(new Float32Array(POSE_FLOATS), 350, 1)).toBeCloseTo(0.2);
  });

  it.each([99, 351])("snaps after a renderer clock reversal/freeze at %s ms", (now) => {
    const history = filledHistory();
    const out = new Float32Array(POSE_FLOATS);
    expect(history.sample(out, 100, 1)).toBeCloseTo(0.95);
    const epoch = history.epoch;
    expect(history.sample(out, now, 1)).toBe(1);
    expect(out[0]).toBe(1);
    expect(history.epoch).toBe(epoch + 1);
  });

  it("retains only 64 frames after repeated ring wrap, including all 128 bodies", () => {
    const history = new PoseHistory();
    history.reset(POSE_MAX_BODIES);
    const source = frame(0, 0, 2, POSE_MAX_BODIES);
    const frames = POSE_HISTORY_FRAMES * 4;
    for (let i = 0; i < frames; i++) {
      source[0] = i;
      source[(POSE_MAX_BODIES - 1) * POSE_FLOATS] = 1000 + i;
      history.push(i * 0.01, source, 0, 100);
    }
    const out = new Float32Array(source.length + 1).fill(999);
    // A requested delay larger than storage clamps to the oldest retained frame.
    const oldest = frames - POSE_HISTORY_FRAMES;
    expect(history.sample(out, 100, 100)).toBeCloseTo(oldest * 0.01);
    expect(out[0]).toBe(oldest);
    expect(out[(POSE_MAX_BODIES - 1) * POSE_FLOATS]).toBe(1000 + oldest);
    expectQuaternion(out, 0, POSE_MAX_BODIES - 1);
    expect(out.at(-1)).toBe(999);
    expect(POSE_HISTORY_FRAMES).toBe(64);
    expect(POSE_MAX_BODIES).toBe(128);
  });

  it("bumps the epoch if ring wrap overwrites the presented bracket", () => {
    const history = new PoseHistory();
    history.reset(1);
    const out = new Float32Array(POSE_FLOATS);
    history.push(0, frame(0), 0, 0);
    history.sample(out, 0, 1);
    const epoch = history.epoch;
    for (let i = 1; i <= POSE_HISTORY_FRAMES + 1; i++) history.push(i / 1000, frame(i), 0, 1);
    expect(history.sample(out, 2, 1)).toBe((POSE_HISTORY_FRAMES + 1) / 1000);
    expect(out[0]).toBe(POSE_HISTORY_FRAMES + 1);
    expect(history.epoch).toBe(epoch + 1);
  });

  it("invalidates old frames on a body change and rejects more than 128 bodies", () => {
    const history = filledHistory();
    const out = new Float32Array(129 * POSE_FLOATS).fill(99);
    const epoch = history.epoch;
    history.reset(2);
    expect(history.epoch).toBe(epoch + 1);
    expect(history.sample(out, 100, 1)).toBeNaN();
    expect(history.push(1, frame(2, 0, 1, 2), 0, 100)).toBe(true);
    expect(history.sample(out, 100, 1)).toBe(1);
    expect(out[POSE_FLOATS]).toBe(12);
    history.reset(129);
    expect(history.push(1, frame(3, 0, 1, 129), 0, 100)).toBe(false);
    out.fill(99);
    expect(history.sample(out, 100, 1)).toBeNaN();
    expect(out[0]).toBe(99);
  });

  it("clears old history on an invalid pose and recovers on the next valid frame", () => {
    const history = filledHistory();
    const invalid = frame(1);
    invalid[3] = NaN;
    const epoch = history.epoch;
    expect(history.push(1.1, invalid, 0, 110)).toBe(false);
    expect(history.epoch).toBe(epoch + 1);
    const out = new Float32Array(POSE_FLOATS).fill(99);
    expect(history.sample(out, 110, 1)).toBeNaN();
    expect(out[0]).toBe(99);
    expect(history.push(1.2, frame(8), 0, 120)).toBe(true);
    expect(history.sample(out, 120, 1)).toBe(1.2);
    expect(out[0]).toBe(8);
  });
});

describe("deterministic live packet/render clock", () => {
  it.each([0.25, 0.5, 1, 4, 8].flatMap((rtf) => [30, 60, 120, 144, 240].map((hz) => [rtf, hz])))(
    "keeps bounded, correctly scaled poses at RTF %s and %s render Hz",
    (rtf, hz) => {
      const history = new PoseHistory();
      history.reset(1);
      history.push(0, frame(0), 0, 0);
      const epoch = history.epoch;
      const out = new Float32Array(POSE_FLOATS);
      let packet = 1;
      let nextSample = 1;
      let latest = 0;
      let previous = 0;
      for (let render = 0; render <= hz; render++) {
        const now = (render * 1000) / hz;
        while ((packet * 1000) / 60 <= now + 1e-8) {
          const arrival = (packet * 1000) / 60;
          while (nextSample / 60 <= (arrival / 1000) * rtf + 1e-10) {
            latest = nextSample / 60;
            history.push(latest, frame(latest, latest), 0, arrival);
            nextSample++;
          }
          packet++;
        }
        const t = history.sample(out, now, rtf);
        expect(Number.isFinite(t)).toBe(true);
        expect(t).toBeGreaterThanOrEqual(previous);
        expect(t).toBeLessThanOrEqual(latest);
        expect(out[0]).toBeCloseTo(t, 5);
        expect(out[1]).toBeCloseTo(2 * t, 5);
        expectQuaternion(out, t);
        previous = t;
      }
      expect(previous).toBeCloseTo(0.95 * rtf, 9);
      expect(history.epoch).toBe(epoch);
    },
  );
});
