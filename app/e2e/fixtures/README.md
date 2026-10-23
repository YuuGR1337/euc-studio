# e2e fixtures

The browser suites with their own `e2e/*.config.ts` run the real frontend against controlled fixtures. No fixture
runs firmware, the simulator or native audio. Run a suite from `app/`, for example
`pnpm exec playwright test -c e2e/live-poses.config.ts`.

| Fixture                                               | Suites                                 | What it is                                                                                                                        |
| ----------------------------------------------------- | -------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `live-wire.ts`                                        | live-poses, camera-modes, rider-figure | Stands in for the Tauri transport of the live and compare engine clients; the spec delivers messages and batches in Tauri's order |
| `rider-poses.json`                                    | rider-figure                           | Rider body lists and pose frames taken from engine recordings of scenario runs; no script regenerates it                          |
| `free-ride-controls.ts`                               | free-ride-controls                     | An engine that answers as the play-mode Free Ride engine does                                                                     |
| `tune-probes.ts`                                      | tune-capture                           | Controlled Tune catalog and snapshot replies                                                                                      |
| `world-chunks.ts`, `world-payload.ts`                 | world-view                             | Synthetic city chunks; with `WORLD_PAYLOAD_BIN` set to euc-world's payload example, real generated chunks instead                 |
| `wheel-output-engine.ts`, `wheel-output-layouts.json` | wheel-outputs                          | Display and light layouts from the profile, with a controlled engine (below)                                                      |

## Display layout fixture

`wheel-output-layouts.json` contains public geometry from the Begode Extreme profile.
The native `DisplayLayout::parse` and `LightsLayout::parse` adapters export the geometry.
The fixture contains no cached firmware, stock bytes, or private data.

Refresh the geometry from the repository root:

```sh
node app/e2e/fixtures/generate-wheel-output-layouts.mjs
```

The export uses Cargo offline mode and a temporary build directory.
The script removes its temporary directory after the export.

Run the isolated browser checks from `app/`:

```sh
pnpm exec playwright test -c e2e/wheel-outputs.config.ts
```

The test server replaces engine imports only in `WheelOutputs`, `WheelButtons`, and `useBuzzerSound`.
The rest of the app uses the browser demo.
The card uses a controlled `EngineClient` with an all-segment display pattern and fixed light outputs.
These values are test state. They are not a firmware boot capture.
The fixture never runs GPIO commands or audio output.

Screenshots and measured bounds go to `app/test-results/wheel-outputs/screens/`.
The screenshot label states the fixture limits.
The browser checks cover layout and frontend input guards only.
They do not verify simulator GPIO, firmware gestures, or native audio.
The powered-off case sets the output fields directly.
It checks the dark display, front Off state, rear LED count, and absent blue indicator.
It does not verify power-domain physics.
The release-error case rejects three automatic release requests.
It checks that new presses stay blocked until Retry release receives an acknowledgement.
These are controlled transport replies. They are not native GPIO results.
