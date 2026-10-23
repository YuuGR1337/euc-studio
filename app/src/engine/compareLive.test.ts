import { describe, expect, it, vi } from "vitest";
import type { CompareRequest } from "./compareTypes.ts";
import {
  BATCH_FLAG_POSES,
  BATCH_HEADER_BYTES,
  BATCH_MAGIC,
  BATCH_VERSION,
  CHANNELS,
  CH,
  POSE_FLOATS,
} from "./telemetry.gen.ts";

type Deliver = (value: unknown) => void;

const wire = vi.hoisted(() => ({
  runs: [] as { a: Deliver; b: Deliver; messages: Deliver }[],
}));

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    readonly onmessage: Deliver;

    constructor(onmessage: Deliver) {
      this.onmessage = onmessage;
    }
  },
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === "compare_info") return { firmwares: [], scenarios: [], tolerance_file: "" };
    if (command === "compare_start") {
      const channel = (key: string) => (args?.[key] as { onmessage: Deliver }).onmessage;
      wire.runs.push({
        a: channel("streamA"),
        b: channel("streamB"),
        messages: channel("messages"),
      });
    }
    return undefined;
  }),
}));

const { LiveCompare } = await import("./compareLive.ts");

const REQUEST: CompareRequest = {
  a: { kind: "stock" },
  b: { kind: "ours-clean" },
  scenario: "ride-20kmh-stop",
  fast: false,
};
const BODIES = [
  { name: "wheel", role: "wheel", geoms: [] },
  { name: "shell", role: "shell", geoms: [] },
];

/** The backend's first message of a comparison: both runs and their body lists. */
const reset = {
  type: "reset",
  scenario: "ride-20kmh-stop",
  duration_s: 40,
  stream_hz: 50,
  a: { spec: "stock", label: "Stock", sha256: "a", checks_skipped: [] },
  b: { spec: "ours-clean", label: "Ours-clean", sha256: "b", checks_skipped: [] },
  bodies_a: BODIES,
  bodies_b: BODIES,
  terrain: null,
  tolerance_file: "",
  tolerance_note: null,
};

/** A batch in the EUCT layout: speed `speeds[i]`, body `k` at x = speed + 10 k. */
function batch(times: number[], speeds: number[]): ArrayBuffer {
  const n = times.length;
  const count = BODIES.length;
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
  new Float64Array(buffer, BATCH_HEADER_BYTES, n).set(times);
  const values = new Float32Array(buffer, BATCH_HEADER_BYTES + n * 8, n * CHANNELS.length);
  values.fill(Number.NaN);
  values.set(speeds, CH.speed_kmh * n);
  const poses = new Float32Array(buffer, BATCH_HEADER_BYTES + n * (8 + 4 * CHANNELS.length));
  for (let sample = 0; sample < n; sample++) {
    for (let body = 0; body < count; body++) {
      const x = (speeds[sample] as number) + body * 10;
      poses.set([x, 0, 0.225, 1, 0, 0, 0], (sample * count + body) * POSE_FLOATS);
    }
  }
  return buffer;
}

function start(compare: InstanceType<typeof LiveCompare>) {
  compare.start(REQUEST);
  const run = wire.runs.at(-1);
  if (!run) throw new Error("compare_start was not called");
  return run;
}

const values = (ring: { toArray(): Float64Array }) => Array.from(ring.toArray());

describe("LiveCompare streams", () => {
  it("keeps the samples that came before the comparison's reset message", () => {
    const compare = new LiveCompare();
    const run = start(compare);
    // Tauri fetches the reset (both body lists, about 17 KB) but runs a one-sample batch (about
    // 380 bytes) at once: in real time the first batches come before the reset.
    run.a(batch([0.02], [1]));
    run.b(batch([0.02], [2]));
    run.a(batch([0.04], [3]));
    run.messages(reset);
    run.a(batch([0.06], [5]));
    run.b(batch([0.04], [4]));
    expect(values(compare.a.time)).toEqual([0.02, 0.04, 0.06]);
    expect(values(compare.a.channels.speed)).toEqual([1, 3, 5]);
    expect(values(compare.b.time)).toEqual([0.02, 0.04]);
    expect(values(compare.b.channels.speed)).toEqual([2, 4]);
    expect(compare.a.latest.t).toBe(0.06);
  });

  it("takes the city both runs ride from the reset, and drops a frame that is not one", () => {
    const compare = new LiveCompare();
    const run = start(compare);
    const world = {
      spec: { seed: 42, revision: 2, biome: "city", densities: {} },
      start_xy_m: [0, 0],
      start_heading_rad: 0,
      origin_z_m: 0.07,
    };
    run.messages({ ...reset, world });
    expect(compare.world).toEqual(world);
    run.messages({ ...reset, world: { ...world, start_xy_m: [Number.NaN, 0] } });
    expect(compare.world).toBeNull();
    run.messages(reset);
    expect(compare.world).toBeNull();
  });

  it("draws a run's first frame as soon as its reset comes", () => {
    const compare = new LiveCompare();
    const run = start(compare);
    run.a(batch([0.02], [1]));
    run.b(batch([0.02], [2]));
    run.messages(reset);
    expect(compare.a.latest.t).toBe(0.02);
    expect(Array.from(compare.a.latest.poses ?? [])).toEqual(
      [1, 0, 0.225, 1, 0, 0, 0, 11, 0, 0.225, 1, 0, 0, 0].map(Math.fround),
    );
    expect(compare.b.latest.poses?.[0]).toBe(2);
  });

  it("never mixes a new comparison's samples into the one before", () => {
    const compare = new LiveCompare();
    const first = start(compare);
    first.messages(reset);
    first.a(batch([0.02], [1]));
    const second = start(compare);
    second.a(batch([0.02], [9]));
    // A late batch of the first comparison.
    first.a(batch([0.04], [5]));
    // Until its reset, the view keeps the first comparison as it was.
    expect(values(compare.a.channels.speed)).toEqual([1]);
    second.messages(reset);
    expect(values(compare.a.channels.speed)).toEqual([9]);
  });

  it("drops the held samples of a stopped comparison", () => {
    const compare = new LiveCompare();
    const first = start(compare);
    first.a(batch([0.02], [1]));
    compare.stop();
    first.messages(reset);
    const second = start(compare);
    second.messages(reset);
    expect(values(compare.a.time)).toEqual([]);
    expect(compare.a.latest.poses).toBeNull();
  });
});
