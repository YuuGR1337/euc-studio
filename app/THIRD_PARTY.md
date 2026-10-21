# Third-party software in EUC Studio (app)

EUC Studio itself is under the PolyForm Noncommercial License 1.0.0 (see the repository root).
This file lists the app's direct dependencies and their licenses. No dependency is GPL, LGPL or
AGPL.

Checked on 2026-10-16:

- `pnpm licenses list --prod` (the whole production JavaScript tree): MIT, ISC, Apache-2.0,
  BSD-3-Clause, 0BSD, OFL-1.1.
- `cargo metadata` for `src-tauri` (macOS and Linux targets): MIT and/or Apache-2.0 for most
  crates, plus Unicode-3.0, Zlib, BSD-3-Clause, Unlicense, CC0-1.0 and MPL-2.0 (five crates in
  Tauri's HTML tooling; MPL-2.0 is file-level copyleft and needs no change to our code).

## Fonts

| Font                                                 | License                   | How it ships                                                         |
| ---------------------------------------------------- | ------------------------- | -------------------------------------------------------------------- |
| JetBrains Mono (weights 400, 500, 600; Latin subset) | SIL Open Font License 1.1 | Bundled from `@fontsource/jetbrains-mono`                            |
| SF Pro Rounded                                       | Apple system font         | Not bundled. Used through `ui-rounded` only where the OS provides it |
| Segoe UI Variable, system UI faces                   | OS fonts                  | Not bundled. Fallbacks named in the font stack                       |

## Runtime dependencies (JavaScript)

| Package                    | Version | License                       |
| -------------------------- | ------- | ----------------------------- |
| react, react-dom           | 19.3.1  | MIT                           |
| @tauri-apps/api            | 2.12.4  | Apache-2.0 OR MIT             |
| @tauri-apps/plugin-dialog  | 2.8.3   | Apache-2.0 OR MIT             |
| motion                     | 14.3.0  | MIT                           |
| radix-ui                   | 1.8.1   | MIT                           |
| cmdk                       | 1.1.1   | MIT                           |
| react-resizable-panels     | 4.14.5  | MIT                           |
| three                      | 0.186.1 | MIT                           |
| @react-three/fiber         | 9.8.2   | MIT                           |
| @react-three/drei          | 10.7.9  | MIT                           |
| uplot                      | 1.6.32  | MIT                           |
| lucide-react               | 1.57.0  | ISC                           |
| zustand                    | 5.0.15  | MIT                           |
| @fontsource/jetbrains-mono | 5.3.0   | OFL-1.1 (font), MIT (package) |

## Development dependencies (JavaScript, not shipped)

| Package                                                   | Version                        | License           |
| --------------------------------------------------------- | ------------------------------ | ----------------- |
| vite                                                      | 8.3.4                          | MIT               |
| @vitejs/plugin-react                                      | 6.1.2                          | MIT               |
| tailwindcss, @tailwindcss/vite                            | 4.3.3                          | MIT               |
| typescript                                                | 6.0.3                          | Apache-2.0        |
| eslint, @eslint/js                                        | 10.12.0, 10.0.1                | MIT               |
| typescript-eslint                                         | 8.71.2                         | MIT               |
| eslint-plugin-react-hooks                                 | 7.1.1                          | MIT               |
| eslint-plugin-react-refresh                               | 0.5.7                          | MIT               |
| globals                                                   | 17.13.0                        | MIT               |
| prettier                                                  | 3.9.10                         | MIT               |
| vitest                                                    | 4.1.12                         | MIT               |
| @playwright/test                                          | 1.64.1                         | Apache-2.0        |
| tsx                                                       | 4.23.15                        | MIT               |
| @tauri-apps/cli                                           | 2.12.3                         | Apache-2.0 OR MIT |
| @types/react, @types/react-dom, @types/three, @types/node | 19.3.0, 19.3.0, 0.186.0, 20.19 | MIT               |

## Rust dependencies (`src-tauri`)

| Crate                     | License                               |
| ------------------------- | ------------------------------------- |
| tauri, tauri-build (2.x)  | Apache-2.0 OR MIT                     |
| tauri-plugin-dialog (2.x) | Apache-2.0 OR MIT (pulls in rfd, MIT) |
| directories (6.x)         | MIT OR Apache-2.0                     |
| serde, serde_json (1.x)   | MIT OR Apache-2.0                     |

## Assets

- Icons: Lucide (ISC) for the general set; the Ride, Compare, Flash and app glyphs are original.
- The 3D wheel is built from primitives in code; lighting uses in-scene light formers. No model,
  texture or HDR file from a third party ships with the app.
