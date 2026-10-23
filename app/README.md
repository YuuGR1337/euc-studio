# EUC Studio (app)

The desktop studio: Tauri 2, React 19, Vite, TypeScript, Tailwind 4, Motion, react-three-fiber
and uPlot. In the desktop window the studio runs the real engine: the stock Begode Extreme
firmware (or our compat and clean builds, or the SIL library) on an emulated STM32F1, its electrical
plant and a MuJoCo body with a rider, riding the repository's scenarios or the generated city, and
streamed to the UI from a simulation thread. In a plain browser and in tests it runs a deterministic
synthetic demo engine instead. The status bar always names the engine.

![Ride workspace with the real engine: the stock firmware carries the rider at 20 km/h](docs/screenshots/ride-live.png)

The real engine in `pnpm tauri dev` (an earlier build, before the city and the current rider
figure): the stock firmware at 20.4 km/h in `ride-20kmh-stop`, its display replica showing `020`,
the light strip from the WS2812 buffer. The browser build's demo engine looks like
[this](docs/screenshots/ride.png).

## Run

Needs Node 20.19+ (22 in CI), pnpm 10 and, for the desktop shell, stable Rust plus the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
pnpm install
pnpm dev            # web build at http://localhost:1420 (no Tauri), demo engine
pnpm tauri dev      # desktop window, real engine
pnpm tauri build    # bundles for this OS
```

The real engine needs two things once per machine, from the repository root:

```sh
python3 tools/fetch-mujoco/fetch.py          # MuJoCo, the physics library the engine links
cargo run -p euc-cli -- fetch begode-extreme # the stock image, from Begode's update server
```

If the image is not in the firmware cache, the app fetches it on start. Our own CPU core runs the
firmware in every build. `EUC_UNICORN=1 pnpm tauri dev` also builds in the `unicorn` feature (the
Unicorn emulator, GPL-2.0, a development oracle), and `EUC_CPU=unicorn` then selects it. That
feature is for development only: a bundle build refuses to compile with it, and CI checks that.
Add `?engine=demo` to the URL to force the demo engine in the desktop window.

Ours-compat and Ours-clean in the run bar run the builds in `firmware/build/target-compat/` and
`target-clean/`, and Ours-custom runs the SIL library in `firmware/build/host-sil/`: build them first
([firmware](../firmware/README.md#build), [SIL](../docs/sil.md)).

`EUC_STUDIO_PERF=20 pnpm tauri dev` measures frame times for 20 s after a warm-up, then again
every 10 s, and prints them to the terminal (`PERF ...`). The window must stay in front: a covered
window gets few frames, and its lines show that in the frame count.
`EUC_STUDIO_CONTROL=<file>` (dev builds) runs each line written to that file as a command:
`restart [scenario]`, `play`, `pause`, `rtf <x>`, `send <command id> [value]`, `eval <js>`.

In the window:

- Workspaces: Ride (⌘1), Tune (⌘2), Compare (⌘3) and a Flash preview (⌘4). The rail's
  Recordings button (or ⌘K "Recordings…") saves and opens `.eucrec` files.
- The run bar lists the scenarios: a development build reads `scenarios/*.toml` when the web view
  connects, a release build has the 20 shipped ones built in. Scenarios in `scenarios/world/` open
  with the run bar's "Open scenario file" button.
- The wheel is the scanned Begode Extreme from `public/models/begode-extreme/` when the files are
  there (level of detail 1, or 2 without a GPU), else a stylized model.
- The Display card shows the replica of the wheel's display board and its tail light. The bell
  plays the firmware's buzzer through the speakers (off by default; ⌘K "Toggle buzzer sound").
- Ride by hand: W/S or ↑/↓ lean forward and back, A/D or ←/→ turn, or use a gamepad's left
  stick; the first push takes the rider from the scenario. Space steps off, Esc (or M) gives the
  rider back. The Manual card shows what the hands ask.
- Free ride (the button above the 3D view) starts a play-mode ride in the generated city: the
  keys and a gamepad drive at once, R respawns, and the sound comes from the simulation. Keys,
  modes and the native API: [the Free Ride README](src/workspaces/freeRide/README.md).
- Compare (⌘3) rides one scenario with two firmwares (stock, our compat and clean builds from
  `firmware/build/target-*/`, or a `.bin`/`.elf` you pick) and streams both: run B is a ghost over
  run A, the scopes draw A solid and B dashed, and the summary is `euc sim compare`'s, judged by
  `scenarios/compare/compat-vs-stock.toml`. The Quirks tab lists which stock behaviours each
  firmware keeps. A side set to Ours-custom runs the SIL library (`firmware/build/host-sil/`),
  and its quirks switch one at a time for the next run; built images stay read only. Tune's
  Quirks tab restarts the ride with Ours-custom and the chosen quirks ([SIL](../docs/sil.md)).
  While Compare is shown the run bar steps aside, and a comparison pauses the ride until it is
  over and Compare is left.

## Check

```sh
pnpm typecheck && pnpm lint && pnpm format:check
pnpm tokens:check   # src/design/tokens.css matches tokens.ts
pnpm contrast       # every declared text/surface pair passes WCAG AA
pnpm test           # Vitest: engine clients, controls, cameras, tokens, digits, keys, palette
pnpm e2e            # Playwright: builds, serves, screenshots every workspace and the gallery
cargo test --manifest-path src-tauri/Cargo.toml                    # native host: runner, recordings, Free Ride
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture  # with the cached stock image
```

`pnpm e2e` runs the default suite in Chromium. The suites with their own `e2e/*.config.ts`
(camera-modes, free-ride-controls, live-poses, rider-figure, tune-capture, wheel-outputs,
world-view) run in Chromium and WebKit: `pnpm exec playwright install chromium webkit` once, then
`pnpm exec playwright test -c e2e/free-ride-controls.config.ts`. The world-view suite uses the real
city generator when `WORLD_PAYLOAD_BIN` names euc-world's payload example
(`cargo build -p euc-world --release --example payload`), else a synthetic payload.

Screenshots land in `test-results/screens/`. `DOCS_SCREENSHOTS=1 pnpm e2e` also refreshes the
curated ones in `docs/screenshots/`.

## Layout

| Path                      | What                                                                                                                               |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `src/design/tokens.ts`    | The only hand-written design token file                                                                                            |
| `src/design/tokens.css`   | Generated by `pnpm tokens` (do not edit)                                                                                           |
| `src/ui/`                 | Component library; every component in every state at `#/gallery`                                                                   |
| `src/shell/`              | Run bar, rail, docks, status bar, commands and hotkeys                                                                             |
| `src/workspaces/`         | Ride (3D viewport, telemetry, events, riding by hand), Free Ride, Tune, Compare (A/B with a ghost), Recordings; Flash is a preview |
| `src/engine/`             | `EngineClient` interface, live engine client, batch decoder, engine selection                                                      |
| `src/sim/`                | Demo engine (browser and tests)                                                                                                    |
| `src-tauri/`              | Desktop shell (its own cargo workspace)                                                                                            |
| `src-tauri/src/engine/`   | Simulation thread, host over the emulated board, phone decoder, commands, A/B compare runs, Free Ride, sound                       |
| `src/vendor/euc-thrills/` | Code copied from EUC Thrills (MIT) for the rider figure, see [NOTICE](../NOTICE)                                                   |
| `e2e/`                    | Playwright suites and their fixtures                                                                                               |

Design system, motion rules and the decisions log: [DESIGN.md](DESIGN.md). Licenses:
[THIRD_PARTY.md](THIRD_PARTY.md).
