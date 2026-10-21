# Project Completion Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans or superpowers:subagent-driven-development for assigned tasks. Keep isolated worktrees and file ownership.

**Goal:** Complete the missing desktop workflows and expose physical model limits without claiming unsupported hardware accuracy.

**Architecture:** Keep the Rust simulator as the source of run data. Route desktop requests through the simulation thread. Use existing recording and comparison formats. Keep hardware transport disabled.

**Tech Stack:** Rust, Tauri, React, TypeScript, MuJoCo, Python/PyO3.

**Spec:** User approval of the source audit in this chat; docs/roadmap.md; app/DESIGN.md; SAFETY.md.

**Coverage:** [Model and workflow matrix](../../models/coverage-matrix.md). Feature delivery does not establish physical validity.

## Global Constraints

- No hardware connection or flashing.
- No vendor bytes, private user files, or personal measurements in new public artifacts.
- Keep existing safety thresholds and measured profile values.
- Distinguish estimates, measurements, unsupported features, and implementation status.
- The user explicitly authorized automated tests and simulator checks on 2026-10-16.
- Parent owns shared wiring, commits, PRs, and integration.
- A feature enters the completed list only after implementation and review.

## Review Focus

- Run restart must clear old firmware, probe, graph, and recording state.
- Scenario results cover only the declared scenario duration.
- Missing firmware and symbols must produce clear errors, never synthetic substitutes.
- Exhausted battery/model limits must not produce a successful physical claim.
- Local app bundles must carry their runtime libraries and exclude stock images.

## Implementation status

Checked boxes below identify source implementation and review. They do not claim runtime or hardware validation.
Automated verification now runs with user authorization. The request ledger records current results and open checks.
See [requests and phase status](../../project-status.md). An implementation checkbox does not close its runtime acceptance criteria.

## Task 1: Tune workspace (Goodall)

**Files:** app/src/workspaces/tune/**; app/src/engine/tune.ts; engine/tune.rs; narrow host/runner probe plumbing.

- [x] Read named variables on the simulation thread.
- [x] Show live source, values, filtering, and bounded variable scopes.
- [x] Report unavailable probes and clear history on restart.
- [x] Implement sampled triggers and per-handler CPU metrics. Keep memory editing, gain editing, presets, and runtime quirk changes explicit as incomplete.

## Task 2: Compare workspace (Kant)

**Files:** app/src/workspaces/compare/**; app/src/engine/compare.ts; engine/comparison.rs.

- [x] Run real A/B scenarios off the UI thread with cancellation and resource bounds.
- [x] Show image identity, each verdict, skipped checks, unpaired variables, tolerances, and trace differences.
- [x] Label sample divergence correctly. Do not claim causal instruction analysis.
- [x] Implement sampled 3D ghost and split playback. Keep instruction-level causal analysis explicit as incomplete.

## Task 3: Recordings (Franklin)

**Files:** engine/recordings.rs; frontend recording/playback modules and recording controls.

- [x] Save complete bounded run captures using EUCR.
- [x] Validate and open EUCR recordings with resource limits.
- [x] Implement playback, seek, and playback speed.
- [x] Disable live firmware commands during playback.

## Task 4: Python bindings (Ampere)

**Files:** crates/euc-python/**; python/**; docs/python.md; parent workspace wiring.

- [x] Supply an in-process simulation API through PyO3.
- [x] Accept explicit local images and profiles without hardware or hidden downloads.
- [x] Return telemetry and scenario outcomes with clear Python errors and run bounds.
- [x] Document installation and API limits.

## Task 5: Battery validity (Locke)

**Files:** crates/euc-plant/src/battery.rs; narrow plant.rs changes; docs/models/battery-validity.md.

- [x] End unbounded discharge at zero charge, or expose model exhaustion and stop clearly.
- [x] Do not invent the real BMS disconnect threshold or unsupported resistance curves.
- [x] Document calibration inputs and omitted physical effects.
- [x] Parent surfaces validity failures through the simulator and desktop.

## Task 6: Local Mac app (Carson)

**Files:** app bundle configuration/build script; app/scripts/macos-package.mjs; docs/local-macos-install.md.

- [x] Bundle MuJoCo with portable runtime linkage and the own CPU backend.
- [x] Provide a safe explicit Applications install command.
- [x] Distinguish local ad-hoc signing from a notarized public release.
- [x] Keep stock images in the external firmware cache.

## Task 7: Desktop run correctness (parent)

**Files:** app/src-tauri/src/engine/{firmware,runner,host,scenarios,mod}.rs; app/src/engine/{types,live}.ts; app/src-tauri/src/lib.rs.

- [x] Enable stock, compat, and clean with matching ELF probes and image validation.
- [x] Stop at scenario completion and show a distinct completed state.
- [x] Embed all shipped scenarios in packaged builds.
- [x] Measure ISR time from simulator cycles, not synthetic values.
- [x] Wire Tune, Compare, and recording commands and pages.

## Task 8: Review and status (parent)

- [ ] Map every completion claim to implementation, recorded checks, measurements, estimates, and unknowns.
- [ ] Integrate each patch with explicit scope and dependency review.
- [ ] Update README, app instructions, and roadmap status from actual implementation.
- [ ] Preserve unfinished Tune/Compare promises in the completion ledger.
- [ ] Build and run static checks on the integrated tree.
- [ ] Record outstanding verification and measurement dependencies.
- [ ] Package the integrated local Mac app after shared code settles.

## Later Milestones

### Active requests that were not complete

- Human model: implement and verify jointed legs and hips. Independent foot contact and calibrated response follow.
- Free ride: finish native runtime checks of generated terrain and keyboard, slider, and gamepad controls.
- Native audio: initialize output before firmware boot, then verify startup and physical-button tones.
- Live buttons: apply down/up GPIO states immediately, with expiry and reset guards.
- Display: use four horizontal headlight lenses and complete visual review.
- Camera: verify shared pose interpolation and chase tracking during turns, hills, pause, seek, and restart.
- Stress scenarios: accelerate to 70 km/h and apply maximum configured rider braking at three charge levels.
- Local Mac app: build, inspect, and install the final integrated artifact.

These tasks remain active until their stated acceptance checks pass. Documentation alone does not close an implementation request.

M6 wire protocol, real recovery gates, and fault-run acceptance remain separate from the vendor-neutral simulation foundation.
M7 public signing, notarization, docs site, browser firmware execution, and a working second wheel remain incomplete.
M8 requires a separate authorized bench procedure. This plan does not authorize bench flashing or riding.
