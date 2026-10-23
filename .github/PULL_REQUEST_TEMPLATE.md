## What

<!-- What changed, in a few sentences. Link the issue: Closes #N or Refs #N. -->

## Why

<!-- The problem this solves, or the evidence that asked for it. -->

## How I verified it

<!-- The commands you ran and their results (see CONTRIBUTING.md, "Checks per area").
     For a change to the firmware, the plant, the physics, the rider, the world or the flasher: the scenarios you
     ran on stock, compat and clean, and the verdicts and numbers before and after. -->

## Checklist

- [ ] Every commit is signed off (`git commit -s`, the [DCO](https://developercertificate.org/)).
- [ ] The vendor guard passes (`python3 tools/vendor-guard/check.py --all`); no vendor firmware, decompiled code or
      analysis databases.
- [ ] The checks of every area I touched pass.
- [ ] A safety-relevant change comes with scenario evidence ([SAFETY.md](https://github.com/tarikbc/euc-studio/blob/main/SAFETY.md)).
- [ ] Docs that describe the changed behavior are updated.
- [ ] No GPS data, personal photos or other personal data.
