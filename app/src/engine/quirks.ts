// The quirk list of our firmware (firmware/config/quirks.def): every place where the stock
// firmware behaves in a way we would not choose, and which of our profiles keeps it. The studio
// reads the file itself (bundled at build time), so the list never drifts from the firmware's.

import quirksDef from "../../../firmware/config/quirks.def?raw";

export interface Quirk {
  id: string;
  /** The heading it is listed under in quirks.def ("Motor drive and protection"). */
  group: string;
  /** The compat profile has the stock behaviour. */
  compat: boolean;
  /** The clean profile keeps it (false: clean fixes it). */
  clean: boolean;
  /** Where the behaviour is in the stock image. */
  stockRef: string;
  /** One line describing the stock behaviour. */
  summary: string;
}

const STRING = String.raw`"((?:[^"\\]|\\.)*)"`;
const ENTRY = String.raw`EUC_QUIRK_DEF\(\s*(\w+)\s*,\s*([01])\s*,\s*([01])\s*,\s*${STRING}\s*,\s*${STRING}\s*\)`;
const GROUP = String.raw`^/\* ([^*\n]+?) \*/$`;

const unescape = (s: string) => s.replace(/\\(.)/g, "$1");

/** Parses quirks.def: its entries, in bit order, each under the heading above it. */
export function parseQuirks(text: string): Quirk[] {
  const quirks: Quirk[] = [];
  let group = "";
  for (const m of text.matchAll(new RegExp(`${GROUP}|${ENTRY}`, "gm"))) {
    if (m[1] !== undefined) {
      group = m[1];
      continue;
    }
    const [, , id, compat, clean, stockRef, summary] = m;
    quirks.push({
      id: id as string,
      group,
      compat: compat === "1",
      clean: clean === "1",
      stockRef: unescape(stockRef as string),
      summary: unescape(summary as string),
    });
  }
  return quirks;
}

/** quirks.def as the app bundles it, in bit order (the SIL library's mask bits). */
export const QUIRKS: Quirk[] = parseQuirks(quirksDef);
