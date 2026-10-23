# EUC Thrills (vendored)

Source files from [EUC Thrills](https://github.com/VibezZzCoder/EUC-thrills) by VibezZzCoder,
commit `a66dabd` (2026-10-18), for the rider figure of the 3D view
(`src/workspaces/ride/riderFigure`).

- Code: MIT License, Copyright (c) 2026 VibezZzCoder. The full text is in `LICENSE`; every file
  keeps the game's notice on its first line.
- No asset of the game is used. The look the figure wears, the street kit
  (`riderFigure/streetKit.ts`), is EUC Studio's own: its profiles, panels, extras and colours
  are original, built with the loft kit below. Earlier versions used the game's fictional Cool
  Rider look (CC BY 4.0, attributed then); its look file, painted sheet and colours were removed.

| File             | From `src/`             | Changes                                                                            |
| ---------------- | ----------------------- | ---------------------------------------------------------------------------------- |
| `blockoutKit.ts` | `render/blockoutKit.ts` | Two comments reworded (they named another rider)                                   |
| `riderLook.ts`   | `render/riderLook.ts`   | Only the look types (lines 99-710), unchanged; `id` is a string; comments shortened |
| `tuning.ts`      | `data/tuning.ts`        | Only the skeleton and rest arm values the rig reads, unchanged                     |

Comments that name the game's real riders removed: no copied file names or describes any of the
game's riders.

`src/workspaces/ride/riderFigure/rig.ts` adapts the game's `render/rider.ts` (the rider's
construction and its two-link IK) and keeps its notice; its header lists the changes.

The files here are kept as published: lint and formatting skip this folder; the type check
covers it.
