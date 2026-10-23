/// <reference lib="webworker" />
// Paints skies off the main thread: a 1024 × 512 sky with clouds takes a few hundred milliseconds.
import { paintSky, type SkyOptions } from "./sky.ts";

export interface SkyRequest extends SkyOptions {
  id: number;
  width: number;
  height: number;
  seed: number;
}
export interface SkyReply {
  id: number;
  width: number;
  height: number;
  data: Uint8Array;
}

const scope = self as unknown as DedicatedWorkerGlobalScope;

scope.addEventListener("message", (event: MessageEvent<SkyRequest>) => {
  const { id, width, height, seed, clouds, ground } = event.data;
  const data = paintSky(width, height, seed, { clouds, ground });
  const reply: SkyReply = { id, width, height, data };
  scope.postMessage(reply, [data.buffer]);
});
