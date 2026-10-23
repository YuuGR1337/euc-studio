// Controlled transport for the world view check. Only `world_chunks` and `world_origin_z`
// answer: chunks come from the test server (synthetic, or real ones from euc-world's payload
// example), and the origin height is 0. No physics or firmware runs.
const fixture = {
  requests: [] as { keys: [number, number][]; lod: number }[],
  /** Refuse every chunk request (the stream keeps what it drew and reports the error). */
  fail: new URLSearchParams(location.search).get("fail") === "1",
};
window.worldFixture = fixture;
declare global {
  interface Window {
    worldFixture: typeof fixture;
  }
}

export async function invoke<T>(command: string, args: Record<string, unknown>): Promise<T> {
  if (command === "world_origin_z") return 0 as T;
  if (command !== "world_chunks") throw new Error(`The fixture has no command ${command}.`);
  const { keys, lod } = args as { keys: [number, number][]; lod: number };
  fixture.requests.push({ keys, lod });
  if (fixture.fail) throw new Error("Fixture refused the chunks.");
  const response = await fetch(`/__world-chunks${location.search}`, {
    method: "POST",
    body: JSON.stringify(args),
  });
  if (!response.ok) throw new Error(await response.text());
  return (await response.arrayBuffer()) as T;
}
