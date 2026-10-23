# EUC Thrills (vendored)

Source files from [EUC Thrills](https://github.com/VibezZzCoder/EUC-thrills) by VibezZzCoder,
commit `a66dabd` (2026-10-18), for the rider figure of the 3D view
(`src/workspaces/ride/riderFigure`).

- Code: MIT License, Copyright (c) 2026 VibezZzCoder. The full text is in `LICENSE`; every file
  keeps the game's notice on its first line.
- The rider's look (its procedural geometry, colours and painted tailoring sheet) is an original
  asset of the game under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/):

  Assets from EUC Thrills by VibezZzCoder — CC BY 4.0 — https://github.com/VibezZzCoder/EUC-thrills

  Changes: the figure is posed on EUC Studio's physics bodies, its legs stretched along the bone
  to the physics' leg lengths and its upper body scaled to the physics' head height.

Only the game's fictional rider, Cool Rider, is used. Several of the game's other riders are real
people whose names and likenesses its licences do not cover; no other rider's look is copied.

| File                | From `src/`                | Changes                                               |
| ------------------- | -------------------------- | ----------------------------------------------------- |
| `blockoutKit.ts`    | `render/blockoutKit.ts`    | Two comments reworded (they named another rider)      |
| `coolRiderLook.ts`  | `render/coolRiderLook.ts`  | Imports `./tuning.ts`                                 |
| `coolRiderAtlas.ts` | `render/coolRiderAtlas.ts` | Imports `./tuning.ts`; `@ts-nocheck` (see the file)   |
| `inkKit.ts`         | `render/inkKit.ts`         | Only the blocks the atlas uses, each unchanged        |
| `riderLook.ts`      | `render/riderLook.ts`      | Only the look types (lines 99-710), unchanged; `id` is a string; comments shortened |
| `tuning.ts`         | `data/tuning.ts`           | Only the values the look and the rig read, unchanged  |

Comments that name the game's real riders removed: no copied file names or describes any of the
game's other riders.

`src/workspaces/ride/riderFigure/rig.ts` adapts the game's `render/rider.ts` (the rider's
construction and its two-link IK) and keeps its notice; its header lists the changes.

The files here are kept as published: lint and formatting skip this folder; the type check
covers it. `riderFigure/figure.test.ts` pins the tailoring sheet's bytes to the game's.
