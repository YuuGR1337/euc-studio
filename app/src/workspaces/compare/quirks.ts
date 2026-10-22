// Which firmware keeps which stock behaviour of firmware/config/quirks.def (parsed in
// src/engine/quirks.ts).

import type { CompareFirmwareId } from "../../engine/compareTypes.ts";
import type { Quirk } from "../../engine/quirks.ts";

export { type Quirk, parseQuirks } from "../../engine/quirks.ts";

/**
 * Whether a firmware has the stock behaviour of `quirk`: the stock image has every one (that is
 * what the list records), our builds as their profile says, "Ours (custom quirks)" as its mask
 * `custom` says; for a file we cannot tell (null).
 */
export function hasQuirk(
  firmware: CompareFirmwareId,
  quirk: Quirk,
  custom: readonly string[] = [],
): boolean | null {
  switch (firmware) {
    case "stock":
      return true;
    case "ours-compat":
      return quirk.compat;
    case "ours-clean":
      return quirk.clean;
    case "ours-custom":
      return custom.includes(quirk.id);
    case "file":
      return null;
  }
}
