// Writes src/design/tokens.css from src/design/tokens.ts.
//   pnpm tokens         regenerate
//   pnpm tokens:check   exit 1 when the committed CSS differs from the generated CSS

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { renderTokensCss } from "./lib/tokens-css.ts";

const target = fileURLToPath(new URL("../src/design/tokens.css", import.meta.url));
const css = renderTokensCss();

if (process.argv.includes("--check")) {
  let current = "";
  try {
    current = readFileSync(target, "utf8");
  } catch {
    // Missing file counts as out of date.
  }
  if (current !== css) {
    console.error("tokens.css is out of date. Run `pnpm tokens` and commit the result.");
    process.exit(1);
  }
  console.log("tokens.css is in sync with tokens.ts");
} else {
  writeFileSync(target, css);
  console.log(`wrote ${target}`);
}
