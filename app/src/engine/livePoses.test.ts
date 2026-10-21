import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BATCH_FLAG_POSES,
  BATCH_HEADER_BYTES,
  BATCH_MAGIC,
  BATCH_VERSION,
  CHANNELS,
  CH,
  POSE_FLOATS,
} from "./telemetry.gen.ts";

const wire = vi.hoisted(() => ({
  stream: undefined as ((buffer: ArrayBuffer) => void) | undefined,
  messages: undefined as ((message: unknown) => void) | undefined,
  calls: [] as { command: string; args?: Record<string, unknown> }[],
}));

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    readonly onmessage: (value: unknown) => void;

    constructor(onmessage: (value: unknown) => void) {
      this.onmessage = onmessage;
    }
  },
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    wire.calls.push({ command, args });
    if (command === "engine_connect") {
      wire.stream = (args?.stream as { onmessage: (buffer: ArrayBuffer) => void }).onmessage;
      wire.messages = (args?.messages as { onmessage: (message: unknown) => void }).onmessage;
      return {
        backend: "test",
        scenarios: [],
        commands: [],
        firmwares: [{ id: "stock", label: "Test", detail: "" }],
        perf_seconds: null,
      };
    }
    return undefined;
  }),
}));

const { LiveEngine } = await import("./live.ts");
let wall = 100;

function bodies(count = 2) {
  wire.messages?.({
    type: "bodies",
    bodies: Array.from({ length: count }, (_, i) => ({
      name: `body-${i}`,
      role: i === 0 ? "wheel" : "shell",
      geoms: [],
    })),
  });
}

function status(rtf = 2, playing = true, epoch = 1) {
  wire.messages?.({
    type: "status",
    run_mode: "scenario",
    run_epoch: epoch,
    state: playing ? "running" : "paused",
    message: "",
    label: "Test",
    build: "Test",
    backend: "test",
    scenario: "test",
    firmware: "stock",
    t_s: 0,
    rtf,
    target_rtf: rtf || 2,
    playing,
  });
}

async function connected(count = 2) {
  const engine = new LiveEngine();
  await Promise.resolve();
  expect(wire.stream).toBeDefined();
  bodies(count);
  status();
  return engine;
}

/** A real EUCT-layout packet, decoded through LiveEngine's normal channel callback. */
function batch(times: number[], xs = times, count = 2, seq = 0, dropped = 0): ArrayBuffer {
  const n = times.length;
  const buffer = new ArrayBuffer(
    BATCH_HEADER_BYTES + n * (8 + 4 * CHANNELS.length + 4 * count * POSE_FLOATS),
  );
  const header = new DataView(buffer);
  header.setUint32(0, BATCH_MAGIC, true);
  header.setUint16(4, BATCH_VERSION, true);
  header.setUint16(6, BATCH_FLAG_POSES, true);
  header.setUint32(8, n, true);
  header.setUint32(12, CHANNELS.length, true);
  header.setUint32(16, count, true);
  header.setUint32(20, dropped, true);
  header.setUint32(24, seq, true);
  new Float64Array(buffer, BATCH_HEADER_BYTES, n).set(times);
  const values = new Float32Array(buffer, BATCH_HEADER_BYTES + n * 8, n * CHANNELS.length);
  values.fill(NaN);
  values.set(xs, CH.speed_kmh * n);
  const poses = new Float32Array(buffer, BATCH_HEADER_BYTES + n * (8 + 4 * CHANNELS.length));
  for (let sample = 0; sample < n; sample++) {
    for (let body = 0; body < count; body++) {
      const x = (xs[sample] as number) + body * 10;
      poses.set([x, 2 * x, -x, 2, 0, 0, 0], (sample * count + body) * POSE_FLOATS);
    }
  }
  return buffer;
}

describe("LiveEngine pose sidecar", () => {
  beforeEach(() => {
    wire.calls.length = 0;
    wire.stream = undefined;
    wire.messages = undefined;
    wall = 100;
    vi.spyOn(performance, "now").mockImplementation(() => wall);
  });
  afterEach(() => vi.restoreAllMocks());

  it("ingests all nonlinear samples and all bodies at their sim timestamps, preserving raw values", async () => {
    const engine = await connected();
    const packet = batch([1, 1.04, 1.1], [0, 20, 0]);
    const bytes = packet.slice(0);
    wire.stream?.(packet);
    const raw = engine.latest.poses?.slice();
    const out = new Float32Array(2 * POSE_FLOATS);
    for (const [now, t, x] of [
      [100, 1, 0],
      [105, 1.01, 5],
      [125, 1.05, 50 / 3],
      [150, 1.1, 0],
    ]) {
      expect(engine.samplePoses(out, now as number)).toBeCloseTo(t as number, 10);
      expect(out[0]).toBeCloseTo(x as number, 5);
      expect(out[POSE_FLOATS]).toBeCloseTo((x as number) + 10, 5);
      expect(out[3]).toBe(1);
    }
    expect(engine.latest.t).toBe(1.1);
    expect(engine.latest.poses).toEqual(raw);
    expect(engine.latest.poses?.[3]).toBe(2);
    expect(engine.latest.speed).toBe(0);
    expect(Array.from(engine.time.toArray())).toEqual([1, 1.04, 1.1]);
    expect(Array.from(engine.channels.speed.toArray())).toEqual([0, 20, 0]);
    expect(packet).toEqual(bytes);
  });

  it.each([
    [1, 3],
    [2, 0],
  ])("resets on dropped samples or missing sequence (seq %s, dropped %s)", async (seq, dropped) => {
    const engine = await connected();
    wire.stream?.(batch([1], [0]));
    const out = new Float32Array(2 * POSE_FLOATS);
    engine.samplePoses(out, 100);
    const epoch = engine.poseEpoch;
    wall = 101;
    wire.stream?.(batch([1.01], [99], 2, seq, dropped));
    expect(engine.poseEpoch).toBe(epoch + 1);
    expect(engine.samplePoses(out, 101)).toBe(1.01);
    expect(out[0]).toBe(99);
  });

  it.each(["bodies", "reset", "epoch"] as const)(
    "invalidates the visual history on %s",
    async (kind) => {
      const engine = await connected();
      wire.stream?.(batch([1], [0]));
      const epoch = engine.poseEpoch;
      if (kind === "bodies") bodies();
      else if (kind === "reset") wire.messages?.({ type: "reset", scenario: "next" });
      else status(2, true, 2);
      const out = new Float32Array(2 * POSE_FLOATS).fill(99);
      expect(engine.poseEpoch).toBeGreaterThan(epoch);
      expect(engine.samplePoses(out, 100)).toBeNaN();
      expect(out[0]).toBe(99);
    },
  );

  it("drains on native pause and presents an explicit paused step immediately", async () => {
    const engine = await connected();
    wire.stream?.(batch([1, 1.05, 1.1], [0, 5, 10]));
    const out = new Float32Array(2 * POSE_FLOATS);
    expect(engine.samplePoses(out, 100)).toBe(1);
    const epoch = engine.poseEpoch;
    status(0, false);
    expect(engine.samplePoses(out, 125)).toBeCloseTo(1.05);
    expect(engine.samplePoses(out, 150)).toBe(1.1);
    engine.step();
    expect(wire.calls.at(-1)?.command).toBe("engine_step");
    wall = 160;
    const stepped = 1.1 + 1 / 60;
    wire.stream?.(batch([stepped], [20], 2, 1));
    expect(engine.samplePoses(out, 160)).toBe(stepped);
    expect(out[0]).toBe(20);
    expect(engine.samplePoses(out, 200)).toBe(stepped);
    expect(engine.poseEpoch).toBe(epoch);
  });

  it("rejects raw-output aliases without changing raw poses", async () => {
    const engine = await connected();
    wire.stream?.(batch([1], [4]));
    const raw = engine.latest.poses;
    if (!raw) throw new Error("Expected a raw frame");
    const before = raw.slice();
    expect(engine.samplePoses(raw, 100)).toBeNaN();
    expect(engine.samplePoses(new Float32Array(raw.buffer), 100)).toBeNaN();
    expect(raw).toEqual(before);
  });

  it("samples 128 bodies, rejects 129, and rejects a body-layout mismatch", async () => {
    const engine = await connected(128);
    wire.stream?.(batch([1], [0], 128));
    const out = new Float32Array(129 * POSE_FLOATS).fill(99);
    expect(engine.samplePoses(out, 100)).toBe(1);
    expect(out[127 * POSE_FLOATS]).toBe(1270);
    expect(out[128 * POSE_FLOATS]).toBe(99);
    bodies(129);
    wire.stream?.(batch([1.01], [1], 129, 1));
    expect(engine.samplePoses(out, 100)).toBeNaN();
    bodies(2);
    wire.stream?.(batch([1.02], [2], 1, 2));
    expect(engine.samplePoses(out, 100)).toBeNaN();
  });
});
