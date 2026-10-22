/// <reference lib="webworker" />
// Decodes world chunk payloads and builds their mesh arrays off the main thread.
import { buildChunk, decodePayload, type ChunkMeshArrays } from "./worldMesh.ts";

export interface WorkerRequest {
  id: number;
  buffer: ArrayBuffer;
}
export type WorkerReply = { id: number; chunks: ChunkMeshArrays[] } | { id: number; error: string };

const scope = self as unknown as DedicatedWorkerGlobalScope;

scope.addEventListener("message", (event: MessageEvent<WorkerRequest>) => {
  const { id, buffer } = event.data;
  try {
    const chunks = decodePayload(buffer).map(buildChunk);
    // Hand the large arrays over; the shared index buffers are copied.
    const transfer = chunks.flatMap((c) => [
      c.heights.buffer,
      c.positions.buffer,
      c.normals.buffer,
      c.uvs.buffer,
      c.texture.buffer,
      c.boxPositions.buffer,
      c.boxNormals.buffer,
      c.boxColors.buffer,
      c.boxIndices.buffer,
    ]);
    scope.postMessage({ id, chunks } satisfies WorkerReply, transfer);
  } catch (error) {
    scope.postMessage({ id, error: String(error) } satisfies WorkerReply);
  }
});
