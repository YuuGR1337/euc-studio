// Quirk masks of our firmware as the host SIL library ("Ours (custom quirks)"): which stock
// behaviours of firmware/config/quirks.def a run keeps. A mask is the list of quirk IDs that are on,
// in the file's order (the library's bit order); the engine takes the names, never bit numbers.

import { readStored, writeStored } from "../lib/storage.ts";
import { QUIRKS, type Quirk } from "./quirks.ts";
import type { FirmwareId } from "./types.ts";

export type Profile = "compat" | "clean";

/** The quirks a profile keeps: compat has every stock behaviour, clean only those it keeps on purpose. */
export function profileQuirks(profile: Profile, list: readonly Quirk[] = QUIRKS): string[] {
  return list.filter((q) => (profile === "compat" ? q.compat : q.clean)).map((q) => q.id);
}

/** Known IDs only, each once, in quirks.def order. */
export function normalizeQuirks(ids: readonly string[], list: readonly Quirk[] = QUIRKS): string[] {
  const on = new Set(ids);
  return list.filter((q) => on.has(q.id)).map((q) => q.id);
}

/** `ids` with quirk `id` switched on or off. */
export function setQuirk(
  ids: readonly string[],
  id: string,
  on: boolean,
  list: readonly Quirk[] = QUIRKS,
): string[] {
  const rest = ids.filter((q) => q !== id);
  return normalizeQuirks(on ? [...rest, id] : rest, list);
}

export function sameQuirks(a: readonly string[], b: readonly string[]): boolean {
  const x = normalizeQuirks(a);
  const y = normalizeQuirks(b);
  return x.length === y.length && x.every((id, i) => id === y[i]);
}

/**
 * A mask as the engine and `euc sim run --quirks` write it, from the profile it is closest to:
 * "compat", "clean", "clean+ASIN_ACCEL-NO_WATCHDOG", "none+KEIL_FTZ".
 */
export function describeQuirks(ids: readonly string[], list: readonly Quirk[] = QUIRKS): string {
  const on = new Set(normalizeQuirks(ids, list));
  const bases: [string, Set<string>][] = [
    ["compat", new Set(profileQuirks("compat", list))],
    ["clean", new Set(profileQuirks("clean", list))],
    ["none", new Set()],
    ["all", new Set(list.map((q) => q.id))],
  ];
  const distance = (base: Set<string>) =>
    list.filter((q) => base.has(q.id) !== on.has(q.id)).length;
  let best = bases[0] as [string, Set<string>];
  for (const candidate of bases) if (distance(candidate[1]) < distance(best[1])) best = candidate;
  const [name, base] = best;
  return (
    name +
    list
      .filter((q) => base.has(q.id) !== on.has(q.id))
      .map((q) => (on.has(q.id) ? `+${q.id}` : `-${q.id}`))
      .join("")
  );
}

/** The quirks "Ours (custom quirks)" last ran with (compat until someone changes them). */
export function storedCustomQuirks(): string[] {
  const stored = readStored<unknown>("quirks.custom", null);
  return Array.isArray(stored)
    ? normalizeQuirks(stored.filter((q): q is string => typeof q === "string"))
    : profileQuirks("compat");
}

export function storeCustomQuirks(ids: readonly string[]): void {
  writeStored("quirks.custom", normalizeQuirks(ids));
}

/** The engine's firmware choice for a mask: `ours-custom:ID,ID`. */
export function customFirmware(ids: readonly string[]): string {
  return `ours-custom:${normalizeQuirks(ids).join(",")}`;
}

/** The firmware ID and, for `ours-custom`, its quirks, from the engine's choice text. */
export function splitFirmware(choice: string): { id: string; quirks: string[] | null } {
  const [id, list] = choice.split(/:(.*)/s, 2) as [string, string | undefined];
  if (id !== "ours-custom") return { id: choice, quirks: null };
  return { id, quirks: (list ?? "").split(",").filter((q) => q.length > 0) };
}

/** The quirks a firmware keeps: stock has every one, our images their profile's, ours-custom its mask. */
export function quirksOf(firmware: FirmwareId, custom: readonly string[] | null): string[] {
  switch (firmware) {
    case "stock":
      return QUIRKS.map((q) => q.id);
    case "ours-compat":
      return profileQuirks("compat");
    case "ours-clean":
      return profileQuirks("clean");
    case "ours-custom":
      return normalizeQuirks(custom ?? []);
  }
}
