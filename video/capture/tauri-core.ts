// The showcase capture's transport: stands in for @tauri-apps/api/core in the studio's engine
// modules (capture.mjs says which) while the capture replays real runs. It answers only from
// what the showcase tool exported (app/src-tauri/src/engine/showcase.rs): a recording's data,
// world chunk payloads, and a comparison's setup and stream. Nothing here simulates; a request
// it cannot answer is counted in `misses` and fails.

type Deliver = (value: unknown) => void;

interface WorldStore {
  /** Chunk blocks as `euc_world::render` lays them out, without the payload header. */
  bytes: Uint8Array;
  /** `"lod/cx/cy"` to `[offset, length]` in `bytes`. */
  chunks: Record<string, [number, number]>;
  frame: { spec: unknown; start_xy_m: [number, number]; origin_z_m: number };
}

/** A world specification as a key: seed, revision, biome and every density (sorted). */
function specKey(spec: unknown): string {
  const sorted = (value: unknown): unknown =>
    value && typeof value === "object" && !Array.isArray(value)
      ? Object.fromEntries(
          Object.entries(value as Record<string, unknown>)
            .sort(([a], [b]) => a.localeCompare(b))
            .map(([k, v]) => [k, sorted(v)]),
        )
      : value;
  return JSON.stringify(sorted(spec));
}

const showcase = {
  /** What `recordings_open` answers: the RecordingData of the file being opened. */
  recording: null as unknown,
  worlds: [] as WorldStore[],
  /** What `compare_info` answers (staged before the app loads: the Compare client asks at once). */
  compareInfo: (window.showcaseInit?.compareInfo ?? null) as unknown,
  /** The channels of the comparison the workspace started. */
  compare: null as { a: Deliver; b: Deliver; messages: Deliver } | null,
  misses: [] as string[],
};
window.showcase = showcase;
declare global {
  interface Window {
    showcase: typeof showcase;
    showcaseInit?: { compareInfo: unknown };
  }
}

export class Channel<T> {
  readonly onmessage: (value: T) => void;

  constructor(onmessage: (value: T) => void) {
    this.onmessage = onmessage;
  }
}

function worldChunks(spec: unknown, keys: [number, number][], lod: number): ArrayBuffer {
  const blocks: Uint8Array[] = [];
  const wanted = specKey(spec);
  for (const [cx, cy] of keys) {
    const key = `${lod}/${cx}/${cy}`;
    const store = showcase.worlds.find((w) => specKey(w.frame.spec) === wanted && w.chunks[key]);
    if (!store) {
      showcase.misses.push(`world ${key}`);
      throw new Error(`No exported chunk ${key}`);
    }
    const [offset, length] = store.chunks[key];
    blocks.push(store.bytes.subarray(offset, offset + length));
  }
  const size = 12 + blocks.reduce((sum, b) => sum + b.length, 0);
  const out = new Uint8Array(size);
  const view = new DataView(out.buffer);
  out.set([0x45, 0x55, 0x43, 0x57], 0); // "EUCW"
  view.setUint32(4, 1, true);
  view.setUint32(8, keys.length, true);
  let at = 12;
  for (const block of blocks) {
    out.set(block, at);
    at += block.length;
  }
  return out.buffer;
}

export async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  switch (command) {
    case "recordings_open":
      if (!showcase.recording) throw new Error("No recording staged");
      return showcase.recording as T;
    case "world_chunks":
      return worldChunks(args.spec, args.keys as [number, number][], args.lod as number) as T;
    case "world_origin_z": {
      const wanted = specKey(args.spec);
      const start = args.startXyM as [number, number];
      const store = showcase.worlds.find(
        (w) =>
          specKey(w.frame.spec) === wanted &&
          w.frame.start_xy_m[0] === start[0] &&
          w.frame.start_xy_m[1] === start[1],
      );
      if (!store) {
        showcase.misses.push(`origin ${start.join(",")}`);
        throw new Error("No exported world start");
      }
      return store.frame.origin_z_m as T;
    }
    case "compare_info":
      return showcase.compareInfo as T;
    case "compare_start": {
      const channel = (key: string) => (args[key] as Channel<unknown>).onmessage;
      showcase.compare = {
        a: channel("streamA"),
        b: channel("streamB"),
        messages: channel("messages"),
      };
      return undefined as T;
    }
    case "compare_stop":
      return undefined as T;
    default:
      showcase.misses.push(command);
      throw new Error(`The showcase transport does not answer ${command}`);
  }
}
