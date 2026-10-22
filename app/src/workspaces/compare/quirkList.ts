// quirks.def as the app bundles it (Vite reads the file at build time).

import quirksDef from "../../../../firmware/config/quirks.def?raw";
import { type Quirk, parseQuirks } from "./quirks.ts";

export const QUIRKS: Quirk[] = parseQuirks(quirksDef);
