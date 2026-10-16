// Checks every text/surface pair declared in src/design/tokens.ts against WCAG AA.
// Text pairs need 4.5:1, UI graphics (focus ring, ticks, traces) need 3:1. Exit 1 on any miss.

import { contrast } from "../src/design/color.ts";
import { MIN_RATIO, contrastPairs } from "../src/design/tokens.ts";

let failures = 0;
const rows = contrastPairs.map((p) => {
  const ratio = contrast(p.fg, p.bg);
  const min = MIN_RATIO[p.kind];
  const ok = ratio >= min;
  if (!ok) failures++;
  return { ok, ratio, min, p };
});

const width = Math.max(...rows.map((r) => `${r.p.fgName} on ${r.p.bgName}`.length));
for (const { ok, ratio, min, p } of rows) {
  const name = `${p.fgName} on ${p.bgName}`.padEnd(width);
  console.log(
    `${ok ? "pass" : "FAIL"}  ${name}  ${ratio.toFixed(2).padStart(5)}:1  (min ${min}:1, ${p.use})`,
  );
}

console.log(`\n${rows.length - failures}/${rows.length} pairs pass WCAG AA`);
if (failures > 0) process.exit(1);
