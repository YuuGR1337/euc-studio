# Showcase video

The README's showcase: `docs/assets/video/showcase.mp4` (with sound), `showcase-loop.webp` (the
silent loop the README plays inline) and `poster.png`. Everything in them is the studio itself,
filmed headless, playing real runs of the real engine and the stock firmware. Only the titles are
drawn (Remotion).

## What is real

- **The runs.** `engine/showcase.rs` (an ignored test in the app crate) runs the hosts the studio
  runs: `build_free_host` for Free Ride, `build_host` for a scenario, the native comparison for
  Compare. It feeds the studio's recorder from the runner's own sample loop, sends Free Ride its
  input every 50 ms as the Free Ride view does (drive mode, `driveAsk`), and writes each run's
  `.eucrec`, the data the studio's playback opens, and the world chunks the view asks for.
- **The pictures.** `capture/capture.mjs` opens the studio in headless WebKit, the desktop app's
  web engine, with no window on screen. Ride shots open the recordings in the studio's own
  Playback; the Compare shot replays the comparison's real stream into the Compare workspace.
  `capture/tauri-core.ts` stands in for Tauri and answers only from what the tool exported. The
  clock is Playwright's fake clock, stepped 32 ms per frame, so the runs play at 1x at 31.25 fps.
- **The beeps.** The native scheduler renders each run's buzzer offline from the same samples
  the live runner would submit, at the simulator's level. The ride sound (motor, tyre, wind) is
  left out, as the studio ships it off ("Ride sound (experimental)"); `RIDE_SOUND` in the tool
  puts it back. The Compare shot plays run A's beeps, recorded on its own (the studio plays no
  sound in Compare).

Not from the studio: the titles, and the music under the beeps, a Mixkit track
(`public/audio/LICENSES.md`, cut by `scripts/make-music.sh`). The scenes cut on its beat
(`BPM` in `src/timeline.ts`). The Compare shot hides the status bar, which shows the browser
demo engine.

## Make it

From the repository root, with the cached stock firmware, the MuJoCo library, our compat build
(`firmware/build/target-compat`), `pnpm install` in `app/` and `npm install` in `video/`:

```sh
sh video/scripts/make.sh            # record, capture, footage, render
sh video/scripts/make.sh capture    # start at a later step: capture, footage or render
node video/scripts/stills.mjs       # stills over the cut and a contact sheet (out/stills/)
```

The MP4 is levelled to -17 LUFS with a true peak under -1.5 dBTP (`scripts/finish.mjs`); the
beeps sit about 10 LU over the music.

`shots.json` lists the runs (a Free Ride script, scenarios, a comparison) and what to film of
them. Everything the steps write stays in `video/out/` and `video/public/` (ignored by git) until
`render` writes the three files in `docs/assets/video/`.

Render again whenever the physics, the rider or the studio's look changes: the runs, the pictures
and the sound all come from the code of the day. Check the stills and the timings in
`shots.json` first (a run that rides differently may need its window moved).

## Limits

The video shows the simulator, not a wheel. Its runs carry the model's limits (docs/models):
some physical values are estimates, and the top speed waits for the plant's calibration. The
simulator does not establish real riding safety.
