# Clean cutoff edge cases: decisions and evidence

Current decision date: 2026-10-21. Working-tree base: `aa129c4`.

The clean profile now uses the approved `ERROR_CUTOFF_22MS` change.
An error angle alone disables balance on the 52nd consecutive call above 40 degrees.
Stock and compat retain the 11-call error limit.
The clean profile also retains the 11-call limit when `tipped` is true.
Pitch, overload, roll angles, hard gates, and stale-command limits retain their existing values.

The clean headroom warning now includes a second trigger.
Positive driving Vd passes through a 0.3-second low-pass filter.
A filtered value above 35% starts PWM tiltback, even when its setting is off.
The existing 75% voltage-vector trigger remains active.
This change does not fix the current-loop oscillation reported with doubled estimated inductance.

The approved source also initializes the beeper volume from settings before the first beep.
Host unit tests cover the error-call limits, retained roll delay, and filtered Vd behavior.
Scenario evidence must use the current firmware, physics, profiles, and all three instruction quanta.
A tolerance pass before a failure does not prove that two complete rides agree.
The [full-effort report](full-effort-acceptance.md) records the separate acceleration and braking results.

The historical brief and passive trace below preserve the earlier decision process.
Their instructions to keep the error window unchanged are superseded by the approved clean-only change above.
Their addresses, hashes, mechanical model, and outcomes identify earlier builds only.
They do not describe the current image or establish a real-wheel operating limit.

## Current validation, 2026-10-21

The final CLI retains yaw-rate damping at speed in the rigid rider.
An earlier rebuild exposed a steering instability before firmware cutoff.
That instability also reproduced without firmware.
The final evidence excludes those rejected runs.
The public scenarios retain their default `rigid_ankle_v1` rider.

The final input manifest and JSON reports are under `target/fw-scratch/resume-accepted/`.
The [full-effort report](full-effort-acceptance.md#identity-and-reproduction) records current image and CLI hashes.
No firmware source change followed from the resumed validation.

| Firmware check | Result |
|---|---|
| ASan/UBSan host build | 23/23 CTest suites pass |
| Host SIL build | 23/23 CTest suites pass |
| GCC host build | 23/23 CTest suites pass |
| Compat and clean target builds | Pass, including probe-layout checks |
| Format, host/target cppcheck/MISRA, generated quirks | Pass |
| Existing clang-tidy build | Pass; no rebuild required |
| Compat image size | 34,336 bytes |
| Clean image size | 36,760 bytes |

All 20 shipped stock/compat comparisons pass at quantum 4000 under their existing comparison rules.
An expanded set includes quanta 3000 and 2000 for full-effort, overlean, and pothole cases.
Of those 36 windowed comparisons, 35 pass.
The remaining SOC50 voltage failure appears below.
Three additional SOC50 diagnostics remove the scenario overrides from a local tolerance copy.
All three unwindowed diagnostics fail.
The repository tolerance values, windows, and `same_checks` rules remain unchanged.

The 27 full-effort cases produce six passes and 21 failures, without emulator halts.
Stock and compat pass the full-battery case at all three quanta.
Clean stays upright without cutoff there, but its 70 km/h gate times out.
At 50% SOC, every complete scenario fails.
Stock and compat still differ in which acceleration or braking gate fails.
The launch-only comparison does not resolve that difference.

### Scenario guards

The main matrix covers 20 shipped scenarios and a local 3 cm pothole case.
Every case runs with stock, compat, and clean at all three quanta.
All 189 runs complete without emulator halts.
The raw scenario verdicts are 133 passes and 56 failures.
These totals include cases that deliberately require stock behavior or challenge the motor's limit.
They do not constitute a firmware quality ranking.

| Scenario group | Stock passes | Compat passes | Clean passes |
|---|---:|---:|---:|
| Balance, normal ride-stop, coast, downhill, hard braking, low-battery hill | 18/18 | 18/18 | 18/18 |
| Potholes at 2, 3, and 5 cm | 9/9 | 9/9 | 9/9 |
| Rider step-off at 5 and 15 km/h | 6/6 | 6/6 | 6/6 |
| Total hall loss | 3/3 | 3/3 | 3/3 |
| Single hall-line loss | 3/3 | 3/3 | 0/3 |
| BLE service cut while standing | 0/3 | 0/3 | 0/3 |
| Low-battery tiltback | 3/3 | 3/3 | 0/3 |
| Overlean with PWM tiltback disabled | 3/3 | 3/3 | 0/3 |
| Full-battery overlean | 0/3 | 0/3 | 1/3 |
| 30% SOC overlean | 0/3 | 0/3 | 0/3 |
| Full-effort acceleration and braking, three SOC values | 3/9 | 3/9 | 0/9 |

All pothole cases stay upright without cutoff under the current approved firmware settings.
Both step-off cases retain their required cutoff behavior and timing checks.
Total hall loss also retains its required cutoff behavior.

The BLE service fixture requires both the stock service cutoff and an upright wheel afterward.
Stock and compat cut, then fail the upright check.
Clean rejects the unconfirmed service command and therefore fails the required-cutoff check.
The low-battery clean case fails the stock 8° pitch requirement because its bounded tiltback stays below that angle.
The PWM-disabled clean case stays upright without cutoff, so its stock cutoff-and-fall requirements fail.

The clean single-line hall case stays upright without cutoff but fails three pitch checks.
Before the fault, pitch standard deviation is 0.55–0.57° against a 0.5° limit.
Afterward, standard deviation reaches 4.18–4.21° and maximum pitch reaches 12.18–12.25°.
The latter measurements include the clean limp-mode response.
The pre-fault spread mismatch remains explicit; the thresholds remain unchanged.

Clean full-battery overlean falls at quanta 4000 and 3000 and holds at 2000.
The 30% ignore-tiltback overlean case falls at every quantum in every image.
These outcomes retain the current control/model limits instead of hiding them behind a selected quantum.

### Current inductance sensitivity

Local profile copies set `Ld = Lq` to 100, 200, and 400 µH.
They keep flux and all other profile values fixed.
The clean image and final rigid-rider model remain the same.
Each cell below lists physical outcomes at quanta 4000, 3000, and 2000, in that order.
`H` means the wheel stays upright without a cutoff.
`F` means the run records both a fall and firmware cutoff.
The symbol does not establish which event occurs first.

| Initial SOC | Rider policy | Lean cap | 100 µH | 200 µH | 400 µH | Warning lead at 400 µH |
|---|---|---:|---|---|---|---:|
| 100% | Ignores tiltback | 18° | H/H/H | F/F/H | F/F/F | 2.17–2.55 s |
| 100% | Default easing | 15° | H/H/H | H/H/H | H/H/H | 2.00–2.22 s |
| 100% | Twice default easing | 15° | H/H/H | H/H/H | H/H/H | 2.01–2.43 s |
| 30% | Ignores tiltback | 15° | H/H/H | F/F/F | F/F/F | 2.12–2.38 s |
| 30% | Default easing | 15° | H/H/H | H/H/H | H/H/H | 2.57–4.40 s |
| 30% | Twice default easing | 15° | H/H/H | H/H/H | H/H/H | 3.63–5.74 s |

These variants do not isolate the easing coefficient alone.
The full-battery ignore case also uses a larger lean cap.
The 30% ignore case keeps the default 15° cap.

Warning lead starts at the first sample with PWM tiltback above 0.01°.
It ends at the first sample with modulation above 98%.
These 100 Hz samples do not identify the exact circle-limit instruction.
They also do not establish a useful pedal-angle warning time.
The current 400 µH minimum lead is 2.00 s, not the earlier study's 2.45 s.
Earlier warning does not prevent the ignore-tiltback riders from falling.
The full-battery nominal-inductance result also remains sensitive to quantum size.

The separate PWM-disabled case stays upright without cutoff at every inductance and quantum.
Its stock-behavior assertions still fail on clean because they require a cutoff and nose-down fall.
The sensitivity batch contains 60 runs and no emulator halts.
It records 49 upright runs without cutoff and 11 runs with both a fall and cutoff.
Its raw scenario verdicts are 43 passes and 17 failures.

### Remaining investigation

The 50% SOC comparison also fails within its launch window at quantum 3000.
The sampled battery-voltage difference reaches 0.53842 V against the unchanged 0.5 V limit.
Quanta 4000 and 2000 pass that window only.
The [full-effort report](full-effort-acceptance.md#comparison-limits) records this limit separately from the later phase failures.

The standard scenario reports do not expose the first disabling detector.
Trace the matching ELF's detector counters and first disable store before changing compat behavior.
Keep the current scenario commands, verdicts, and comparison thresholds during that investigation.

The inductance study uses estimated motor values and 100 Hz voltage samples.
Warning onset does not establish a useful pedal-angle lead or an exact circle-limiter activation time.
It also does not demonstrate stable current regulation with doubled inductance.
Dedicated walking-speed over-angle and sustained-angle scenario guards still need explicit fixtures.
The passing unit tests do not replace those end-to-end guards.

## Historical planning brief, 2026-10-16

Original source revision: `32d4b75`.
The original brief defined the simulator investigation after PR6.
The later passive RTC investigation appears below.

## Evidence and scope

- **EARLIER OBSERVED** means a result reported in the task or existing project documents.
- **CODE** means behavior present in this revision.
- **INFERRED** means a possible cause or consequence that still needs a trace.
- **ESTIMATE** means an unmeasured model value.

The current baseline uses CPI 2.0 and exception costs of 14/14/8 HCLK cycles for entry/return/tail-chain.
The added memory delay remains an estimate.
Use the LTO clean image and the final suspension profile for comparisons.

| Case | Quantum 4000 | Quantum 3000 | Quantum 2000 | Evidence |
|---|---|---|---|---|
| Full battery, 18-degree overlean | Falls | Holds | Holds | EARLIER OBSERVED, final PR6 |
| 5 cm pothole | Holds | Falls | Falls | EARLIER OBSERVED, final PR6 |
| Shipped 2 cm pothole | Holds | Holds | Holds | EARLIER OBSERVED, final PR6 |
| Additional 3 cm pothole | Holds | Holds | Holds | EARLIER OBSERVED, final PR6 |

[CPU timing](cpu-timing.md#previous-scenario-validation) records these final results.
Its clean and compat image hash prefixes are `fe83cb1d` and `3fe25c10`.
These prefixes identify earlier evidence; this task did not hash or run those images.

[Scenario results](scenarios.md) contain earlier CPI 1.25 results without exception costs.
Their lean sweep does not establish current margins.
Their 5 cm description names a pitch cutoff, but the first detector remains unconfirmed.
[Cutoff notes](../../firmware/docs/cutoffs.md#open-questions) explicitly record that uncertainty.

Earlier 2x-inductance runs reported only 0.09 seconds of warning, or saturation before tiltback.
Those runs used an earlier suspension tree and historical timing.
They support an investigation priority, not a current 2x result.

Keep all existing cutoff angles, counter limits, overload limits, stale-command deadlines, and hard gates.
The older suggestion to extend the error window to 105 ms is outside this phase.
Do not use a selected quantum, reduced lean, or changed impact model to declare a fix.

## Code facts that constrain the cause

### Detector order and exact counts

[`running_tick()`](../../firmware/core/balance/balance.c) updates attitude, tiltbacks, handle offset, speed, and cutoffs in that order.
The law runs only while `motor_disabled` is false.
Otherwise, the command ramps down before the output limits apply.

[`cutoff.c`](../../firmware/core/balance/cutoff.c) uses these conditions:

| Detector | Present condition | First disabling call from a zero counter |
|---|---|---|
| Pitch | `abs(attitude.pitch_deg) > 40`, or `tipped` | 52nd consecutive call; about 105 ms |
| Error | `abs(error_deg) > 40`, or `tipped` | 11th consecutive call; about 22 ms |
| Overload | `abs(in.motor_voltage_q) > 31000` | 752nd consecutive call; about 1.516 s |
| Slow roll | `abs(roll) > 40 + 5 * roll_limit_step` | `tipped` on 21st consecutive call |

These durations assume normal RTC delivery near 496 Hz.
Measure actual delivery; counters count calls, not elapsed time.
Equality at 40 degrees resets the pitch or error counter when `tipped` is false.
The overload counter resets at equality with 31000.
Pitch sets `fallen`; error alone does not set it.
Both can set `motor_disabled` during the same call, so `fallen` alone cannot prove first cause.

The fast-roll hall branch cannot trigger after its own ±25 clamp because it requires more than 30 steps.
The positive-Vq fast-roll branch can trigger.
Preserve this behavior and record it separately from pitch and error.

### Balance error, rate terms, and output

The error equals pitch plus battery, speed, PWM, and trim offsets, minus restart and handle offsets.
Thus, error can cross 40 degrees before pitch crosses it.
The firmware pitch sign is positive nose down; body `pitch_deg` uses the opposite sign.

[`law.c`](../../firmware/core/balance/law.c) clamps only the proportional term in current mode.
It then adds `0.1 * Kd * pitch_rate_lpf_rad_per_s` and `160 * (error - error_prev)`.
The gyro term uses **radians per second**, not degrees per second.
The error-change term uses degrees per call, without a measured-time divisor.
Voltage compensation follows the sum.
The output then applies the 120-unit slew and soft-start limit near ±1050.
Soft disable reduces the command by two units per call; it does not immediately open the bridge.

### FOC voltage and timing

[`control.c`](../../firmware/apps/begode-extreme/control.c) reads `motor.v_qd` for the headroom vector.
[`motor.c`](../../firmware/core/motor/motor.c) stores that vector after circle limitation.
In contrast, `vq_active` retains the q regulator output before circle limitation, except during stop braking.
The overload detector therefore watches a different quantity from the headroom detector.
Frame 07 also does not report vector headroom.

[`tiltback.c`](../../firmware/core/balance/tiltback.c) starts clean headroom tiltback above `23625²`, or 75% on the 31500 scale.
Its forward offset grows initially by 0.0055 degrees per call and stops at 12 degrees.
The threshold crossing and a useful pedal angle are different events.
Report both times.

The FOC applies independent d/q PID limits, then a shared circle limit.
The circle limit acts above `32439²`; telemetry defines 100% at 31500.
Use the actual circle-limit branch for saturation events, not a Vq-only 100% check.
[`pid.c`](../../firmware/core/motor/pid.c) clamps integrals but receives no circle-limit feedback.
The regulator has no explicit d/q decoupling feed-forward.
These facts support a current-tracking hypothesis; they do not prove instability at 2x inductance.

## Ranked hypotheses and decisions

| Priority | Hypothesis, INFERRED | Supporting code or earlier evidence | Required discriminator |
|---|---|---|---|
| 1 | The 5 cm impact trips error before pitch during a recoverable dip. | Accurate clean attitude; 11-call error window; offsets precede detection. | First disable store and each detector's counter before/after the same RTC call. Reject if another cause occurs first. |
| 2 | A rate term drives command reversal before physical recovery completes. | Unclamped rate contributions precede a large allowed output slew. Earlier rigid-body traces showed reversal, but they do not prove the current case. | Separate proportional, gyro-rate, error-change, compensated command, and published command. Require reversal before the disabling condition, with matching torque change. |
| 3 | Full-battery overlean reaches a small torque margin before tiltback develops. | Final outcome changes with quantum; headroom offset grows gradually; rider ignores tiltback. | Pre-limit vector, current-reference error, applied torque, lean, and detector sequence. Distinguish physical loss of support from a detector that initiates the fall. |
| 4 | Interrupt timing changes a sampled input or command age near the boundary. | CPI 2.0 exposed an earlier stale-command fault. PR6 moved display transfers and added LTO. | RTC lateness, FOC count, command age, stale latch, and main-loop progress before trajectory divergence. Do not assume the display fault returned. |
| 5 | High inductance consumes d voltage and exposes regulator coupling or integral saturation. | Earlier 2x result; no explicit decoupling or circle feedback in the regulator. | d/q reference errors, PID integrals, pre/post-limit vector, electrical speed, and true dq currents. Distinguish voltage shortage from oscillation before saturation. |

For hypothesis 4, also check whether FOC pre-emption splits the RTC d/q reads across different FOC updates.
Those reads are separate statements; the source does not take a paired snapshot.
This is a sampling hypothesis, not a demonstrated race failure.
Trace actual read ordering before proposing a change.

## Exact probes and events needed

### First patch: observer support only

Start with an emulator observer for existing RAM writes and completed RTC/FOC calls.
Keep firmware instruction bytes and production control behavior unchanged.
Do not add diagnostic I/O to the firmware interrupts.
LTO can remove callable function boundaries; use the matching ELF and actual store sites.

Existing [`probes.toml`](../../profiles/begode-extreme/probes.toml) exposes pitch, roll, command, offsets, and aggregate cutoff flags.
It does not expose detector counters or the stale latch.
[`gen_probes.py`](../../firmware/tools/gen_probes.py) owns generated probe definitions and their layout identifier.
Use a separate local observer map tied to the frozen ELF for the initial investigation.
Do not bypass the layout check or assume offsets from another build.
A later normal probe expansion needs matching generated metadata and an authorized image build.

| Boundary | Exact state or values | Required event |
|---|---|---|
| Each completed balance call | `balance.attitude.pitch_deg`, `roll_deg`, `pitch_rate_lpf_rad_per_s`; `error_deg`, `error_prev_deg`; all six error offsets | RTC sequence, entry/exit cycles, input-read cycles |
| Each detector update | `pitch_over_ticks`, `error_over_ticks`, `overload_ticks`, `roll_over_ticks`; `tipped`, `fallen`, `motor_disabled` | Counter start/reset; threshold action; first false-to-true disable store with PC |
| Other disable paths | `numeric_fault`, `disable_request`, `balance_in.hard_inhibit`; `foc_hard_cut_request`, `motor.hard_cut`, `motor.hall.fault` | Cause before first disable; simultaneous causes kept separately |
| Command publication and each FOC | `balance.command`, `command_prev`, `command_limit`, `motor_command`; `motor.command`, `steps_since_command`, `stale_limit_steps`, `command_stale` | Publication cycle; stale-latch edge; command zero crossing; limiting stage |
| Supervisor | `rtc_ticks`, `foc_steps`, `main_loop_passes`, `supervisor_out.main_loop_stalled` | Missed/late RTC; stall edge; reset; per-context busy cycles |
| Each FOC regulation | `i_qd`, `id_gain`, `pid_d.integral`, `pid_q.integral`; q reference from `20 * motor.command`; d reference zero | PID output clipping; integral-bound contact; pre/post-circle d/q values |
| Bridge output | `vq_active`, `v_qd`, `duty`, `brake_latched`, `outputs_enabled`, `bridge_on`; TIM8 MOE and CCR values | First circle limit; stop brake; hard gate release; PWM update |
| Every 0.5 ms body step | True pitch/rate/roll, distance, speed, wheel speed, suspension travel, contact state, torque, rider lean and pedal release | Hole entry/far-edge contact; physical fall; rider release |
| Electrical plant at aligned cycles | Bus voltage, true d/q currents, applied voltage vector, torque, electrical angle/speed | Current-tracking loss and applied-voltage saturation |

`i_qd.d_q15` already includes a speed-dependent gain.
Record the raw Park result too when that value exists during FOC execution.
Do not compare the gained firmware d current directly with plant amperes.
Use the existing current scaling and electrical sign conventions in [electrical.md](electrical.md).

Derive law terms from the exact pre-law inputs and existing float operations first.
Label this reconstruction as inferred until its rounded sum matches the published command through every output limit.
If LTO removes intermediate values, request a separate instrumented build only after the frozen-image trace.
Such a build changes timing and cannot replace the frozen-image baseline.

Each event needs the cycle, sequence number, context, PC, value before/after, and timestamp precision.
Capture all detector actions in one call, even when an earlier action already sets the disabled flag.
Record the first cause and later consequences separately.
Store raw traces and synthetic recordings only under ignored `.claude/cutoff-analysis/`.
Publish only derived findings without vendor bytes or private source material.

### Timing limits of the current observer

[`Session::watch()`](../../crates/euc-sim/src/session.rs) checks aggregate cutoff flags after each sample period.
It reports the first listed active flag, not the detector's instruction time.
At the default 100 Hz, a sample covers about five RTC calls.
An 11-call error sequence needs a per-call trace.

`Session::step()` also updates rider tiltback input and the drive-cut signal at sample boundaries.
Therefore, `--sample-hz 1000` changes control coupling and scheduler boundaries; it is not a passive increase in trace detail.
Keep `--sample-hz 100` for the baseline and add observation inside the existing schedule.

[`Machine::run()`](../../crates/euc-sim/src/machine.rs) limits execution to the next scheduled event and the quantum cap.
[`System::now()`](../../crates/euc-sim/src/system.rs) uses the chunk start plus skipped cycles during a quantum.
Do not present an ordinary bus-write timestamp as an exact instruction timestamp.
Record the quantum interval and instruction order, or add a diagnostic retired-instruction timestamp.
State its timing model explicitly.
Compare each quantum result separately; smaller quantum does not establish hardware truth.

## Minimal commands for later authorized verification

These commands are a proposal only; this task did not execute them.
Run them from the project worktree root after authorization for scenario verification.
Use an existing CLI and the frozen clean `.bin` with its matching adjacent `.elf`.
Supply those artifacts first if the worktree lacks them.
Do not rebuild firmware to obtain the baseline.

Prepare one scratch scenario by changing only its name and depth:

```sh
mkdir -p .claude/cutoff-analysis
python3 - <<'PY'
from pathlib import Path
source = Path('scenarios/pothole-20kmh.toml').read_text()
source = source.replace('name = "pothole-20kmh"', 'name = "pothole-20kmh-5cm"', 1)
source = source.replace('depth_m = 0.02', 'depth_m = 0.05', 1)
Path('.claude/cutoff-analysis/pothole-20kmh-5cm.toml').write_text(source)
PY
```

The inherited description still names 2 cm; the scratch file's `depth_m` defines this 5 cm variant.
Retain its approach, obstacle position, release time, rider, and expectations.
Then run the six clean baseline cases:

```sh
cutoff_cli=target/release/euc
cutoff_fw=firmware/build/target-clean/euc-fw-begode-extreme.bin
for cutoff_q in 4000 3000 2000; do
  for cutoff_case in scenarios/overlean-top-speed.toml .claude/cutoff-analysis/pothole-20kmh-5cm.toml; do
    cutoff_name=${cutoff_case##*/}
    "$cutoff_cli" --json --profiles profiles sim run \
      --scenario "$cutoff_case" --firmware "$cutoff_fw" \
      --cpu own --cpi 2.0 --max-quantum "$cutoff_q" --sample-hz 100 \
      --record ".claude/cutoff-analysis/${cutoff_name%.toml}-q${cutoff_q}.eucr" \
      > ".claude/cutoff-analysis/${cutoff_name%.toml}-q${cutoff_q}.json"
    cutoff_status=$?
    printf '%s q%s exit=%s\n' "$cutoff_name" "$cutoff_q" "$cutoff_status"
  done
done
```

Exception overhead remains on because the commands omit `--no-exception-timing`.
The observer patch needs its own trace output; these commands alone cannot identify the detector.
Scenario verdict failures are expected in borderline cases.
Inspect JSON for halt/error states before accepting a physical outcome.
`record` checks always pass; neither those checks nor process exit alone establish cause.

Record full CLI/image/ELF hashes, source revision, and profile/scenario hashes in a local manifest.
Record the seed, timing settings, and observer mode in that manifest.
Keep report sampling, plant grid, body step, settings, and rider policy fixed across the six runs.
Repeat the unchanged run if it fails to reproduce the earlier physical outcome.
Resolve artifact or profile differences before comparing an observer or control change.

## Prioritized implementation and acceptance

1. **Observe the first cause.** Add the local emulator observer and collect the six frozen-image traces after authorization.
   Require identical physical outcomes and existing sample values with the observer on and off.
   Deliver the first divergence, detector sequence, maximum counter runs, and command/torque sequence for each quantum.
2. **Resolve timing faults first, if present.** Fix proven event ordering, incoherent inputs, or missed scheduling in a separate patch.
   Retain exception costs and all watchdog/stale deadlines.
   If command age stays normal before divergence, stop pursuing the earlier display fault as the cause.
3. **Select one clean control change from the trace.** Investigate bounded rate contribution before a new command slew rule.
   Require proof that the implicated term causes reversal or excess demand before disable.
   Define the smallest bound from observed terms and motor limits.
   Do not adopt the historical ±500 example without units and evidence.
   If the physical dip alone exhausts a detector window, leave the cutoff unchanged and report that unresolved model/control limit.
4. **Investigate 2x current tracking separately.** Start with equal `Ld = Lq` at 100/200/400 µH and fixed flux.
   Use local profile copies; keep nominal production values unchanged.
   First compare default and 2x traces on the full-battery scenario at all three quanta.
   Expand the rider-policy matrix only after the failure sequence is known.
   Consider decoupling or vector-aware integral handling only when current-error traces justify the change.
   Treat a filtered Vd trigger as an unproven alternative; check braking and impact peaks before selecting it.
5. **Verify the selected patch after separate authorization.** Use all three quanta for every affected guard below.
   Keep the physical outcome and detector cause beside the scenario verdict.

| Future verification group | Required result |
|---|---|
| 2/3/5 cm potholes | Reach the obstacle at the intended approach speed; no early limp; no new cut or fall |
| Full-battery overlean | Resolve all three outcomes, or retain the borderline finding with a measured cause |
| 30% overlean and no-PWM-tiltback | Retain the current clean hold behavior; separate intended stock-verdict failures |
| Hard braking, balance, normal ride | No new reverse-command instability, stale latch, stall, cut, or fall |
| Both rider step-off cases | Still cut; retain the existing three-second step-off limit |
| Sustained saturation with growing angle; walking-speed over-angle; slow/fast roll | Still cut through existing detector conditions; request missing dedicated scenarios separately |
| Hall loss, startup sensor fault, numeric fault, system cuts | Retain current hard gates and fault behavior |

The 30% scenario uses the default 15-degree lean; it is not the full-battery 18-degree case with only SOC changed.
Use equal lean and rider policy for a later battery comparison.
Do not extend the shipped step-off deadline or change its roll limits to obtain acceptance.

## Physical uncertainty and decision limits

`plant.toml` sets estimated `Ld = Lq = 200 µH` and flux `0.0205 Wb`.
Inductance changes cross-axis voltage demand; flux changes both back-EMF and torque per ampere.
Sweep them separately before any combined sweep.
The historical 0.5x/1x/2x inductance choices define sensitivity cases, not a measured confidence interval.
Choose flux bounds from separate evidence; this brief supplies no measured range.
Battery resistance, inverter dead time, and the averaged 5 µs electrical model also affect voltage margin.

The suspension spring and shock length have project evidence, but stroke, linkage, damping, and tire contact remain uncertain.
The rigid rider lacks knees, and the terrain uses a finite heightfield grid.
Those factors can change impact duration near the 22 ms error window.
Separate mechanics sensitivity from control changes; preserve the frozen mechanics baseline for each proposed fix.
Do not use historical 100 Hz pitch differences as exact gyro peak rates.

Earlier notes request about two seconds of tiltback lead before saturation across plausible motor values.
That is a proposed simulator target, not an established safety threshold or a demonstrated current result.
Report both first circle limitation and first sustained current-tracking loss, with the applied-voltage vector.
Report warning lead from both trigger onset and a stated useful pedal angle.
Leave the useful-angle and persistence criteria explicit until the parent selects them from evidence.

**Concrete next task:** implement the emulator-only detector/counter observer for the frozen clean ELF.
First explain why the 5 cm case disables at quantum 3000 and holds at 4000.
Deliver the first disabling store, preceding 60 ms of RTC/FOC state, and aligned body/torque events.
Keep all cutoff thresholds and windows unchanged.


## Historical RTC deep-pothole investigation

Date: 2026-10-16. Working-tree HEAD: `aa129c4cca8e052053c6520435be511d08307554`, with local changes.
The source snapshot includes the RTC divider reload correction.
HEAD alone does not reproduce this working tree.

### Frozen inputs and execution

The scratch directory holds the source snapshot, images, ELF files, profile files, manifest, observer, and raw results:

`/var/folders/ds/gykc6sn951j7wv1qyfj2f7v00000gn/T/euc-pothole-current-rtc-__w7zm3s/`

Only this document changes in the shared worktree.
No shared firmware build, probe generation, hardware operation, or commit occurs.
The scratch scenario changes only the pothole name and depth from 0.02 m to 0.05 m.
It retains the 50 m position, 0.5 m length, rider, settings, release time, and 23-second duration.
The inherited description still says 2 cm; `depth_m = 0.05` defines the actual obstacle.

All runs use the own CPU, CPI 2.0, exception costs 14/14/8, and 100 Hz session samples.
The observer retains every scheduler boundary and CPU instruction budget.
It reads body state after the existing 0.5 ms physics step.
It reads RAM after actual firmware stores.
The normal probe resolver checks each own ELF against layout `0xe061d25e`.
DWARF in each frozen ELF supplies the additional counter addresses.
ELF extraction reproduces each adjacent frozen image byte for byte.

| Artifact | SHA-256 |
|---|---|
| Stock image | `cbfb328cab1e0b8219790dd3ef710bb266569c70b6ac396ccd06c247da9fd27d` |
| Compat image | `1ac5331b643ad45d27973938ab729dbc9b537c09cd32818a0008c13a4e66af00` |
| Compat ELF | `440e89372902c1b09ea04481d65120c27523647fadc78580418d681072d52a4c` |
| Clean image | `1fc300aa5490507a79b8b7a0c49ebc04d1f62b0a0b04ea029d1507891a532fe1` |
| Clean ELF | `5e4386842640b9d1a589c47207bbfd78fadd26b381c9c2d09175fad770e8172b` |
| Shared probe file, frozen copy | `e95d96f629e6c8061105d6b8a3a23238d4f5f0f722c65592246da66e0a8fe05b` |
| RTC source, frozen copy | `4f9ab90172f678563c74405583e8696124776534161d877a92bcff1c8325be75` |

Nine baseline runs and nine passive observer runs complete.
Every passive run matches every baseline sample and final outcome byte for byte.
Three additional clean FOC observer runs also match every baseline sample.
`observer-validation.json` records these comparisons.

The first observer used extra DWT register reads.
Those reads called `System::drain()` and changed timing.
Its results remain under `rejected-dwt-observer/` and supply no findings below.
The accepted observer reads `System::now()` directly without event processing.
`evidence-notes.md` records its timestamp limits and unused diagnostic fields.

### Historical outcomes, OBSERVED

All nine runs reach the obstacle near 19.97–19.99 km/h.
Stock and compat remain upright without a cutoff at every quantum.
Clean disables and falls at every quantum.
Thus, the historical clean hold at quantum 4000 does not reproduce under these frozen current RTC inputs.

The pitch minima below use the 0.5 ms body trace from 19.3 to 19.95 seconds.
The cut times identify actual RAM stores, not the later session event.

| Firmware | Quantum | Minimum true pitch | First disabling detector | Estimated first store time | Outcome |
|---|---:|---:|---|---:|---|
| Stock | 4000 | -42.174° | None | None | Upright |
| Stock | 3000 | -40.674° | None | None | Upright |
| Stock | 2000 | -42.174° | None | None | Upright |
| Compat | 4000 | -41.537° | None | None | Upright |
| Compat | 3000 | -41.537° | None | None | Upright |
| Compat | 2000 | -41.537° | None | None | Upright |
| Clean | 4000 | -43.364° | Error | 19.875561 s | Falls at the 20.00 s session sample |
| Clean | 3000 | -41.502° | Error | 19.878739 s | Falls at the 20.00 s session sample |
| Clean | 2000 | -43.030° | Error | 19.876531 s | Falls at the 20.00 s session sample |

RAM timestamps contain the quantum start and instruction ordinal.
The reported estimate adds two HCLK cycles per preceding instruction, consistent with CPI 2.0.
The maximum quantum spans are 111.1, 83.3, and 55.6 µs for quanta 4000, 3000, and 2000.
These estimates do not establish exact hardware instruction timing.
Store identity and instruction order remain exact.
Body values have 0.5 ms resolution.

### First detector and preceding state, OBSERVED

All three clean runs execute the error branch's first disabling store at `0x080078D4`.
It writes byte 1 to `app.balance.motor_disabled`, address `0x200000D2`, after a previous value of zero.
The preceding store at `0x080078BE` changes `error_over_ticks` from 10 to 11.
The matched ELF maps both stores to the error detector in `cutoff.c`.
The pitch disabling store is a different site, `0x080078A2`.
It does not cause the first disable.

| Quantum | Firmware pitch | Error | Pitch counter | Error counter | Overload / roll counters | True pitch near store | Mean forward torque near store |
|---|---:|---:|---:|---:|---|---:|---:|
| 4000 | +40.901° | +42.200° | 7 | 11 | 0 / 0 | -43.244° | +170.70 N·m |
| 3000 | +39.786° | +41.101° | 0 | 11 | 0 / 0 | -41.500° | +162.20 N·m |
| 2000 | +40.500° | +41.793° | 6 | 11 | 0 / 0 | -42.977° | +164.45 N·m |

Firmware pitch uses the opposite sign from body pitch.
The error includes +1.2° pedal trim and +0.093–0.115° PWM tiltback at the first store.
Other error offsets are zero there.
Quantum 3000 therefore trips error while its estimated pitch never reaches 40° during the dip.

The first qualifying error calls occur at 19.854231, 19.857292, and 19.854448 seconds, respectively.
The first and eleventh calls span 21.331, 21.447, and 22.082 ms.
These spans cover ten intervals, not eleven full intervals.
The counter counts actual calls.

`tipped`, `fallen`, `numeric_fault`, `disable_request`, `hard_inhibit`, hall fault, and hard-cut flags are zero at the first store.
`command_stale` and `main_loop_stalled` are also zero.
The bridge remains enabled, and stop brake remains inactive.
Command age is 25, 23, and 25 FOC steps against an unchanged limit of 72.
The largest age at publication during the preceding 60 ms is 29, 27, and 28 steps.
RTC-counter update gaps in that window range from 1.906 to 2.402 ms across the three runs.
These traces provide no stale-command or main-loop-stall explanation for the first disable.

Compat's pitch and error counters remain zero through the dip.
Its overload counter reaches two calls without disabling.
The stock map lacks detector counters; this investigation does not claim stock counter values.
At 100 Hz, stock's maximum estimated nose-down pitch is 33.55–34.50°; compat reaches 31.60°.
True body pitch passes 40° in every run.
This difference agrees with the known stock-range gyro clipping; it does not establish a safe stock margin.

### Command, current, and physical sequence, OBSERVED

The clean 4000 trace shows this sequence:

| Estimated time | Actual state |
|---:|---|
| 19.768073 s | The law requests +655.63 command units; the output slew publishes +329.34. |
| 19.782305 s | Published command reaches +1051; the motor boundary clamps its command to +1050. |
| 19.805000 s | True pitch rate reaches about 720°/s during the nose-down motion. |
| 19.854231 s | Error counter becomes 1 at error +40.294°. |
| 19.865107 s | The actual circle-limit branch first executes within the 19.74–19.94 s observation window. |
| 19.875561 s | Error counter reaches 11 and disables balance. Published command still equals +1051. |
| 19.875576 s | The disabled branch publishes +1048 after the existing signed ramp-down. |
| 19.883000 s | True pitch reaches its minimum, -43.364°, then starts recovery. |
| 19.940000 s | True pitch recovers to about -34.8°, but published command remains approximately +984. |
| 19.980000 s | True pitch reaches about +11.8°, while published command remains approximately +944. |
| 19.980500 s | The existing rider rule releases the pedals after the session reports soft disable. |
| 20.000007 s | The session reports the physical fall. |

No reverse command initiates the first clean cut.
The last 60 ms before the cut use forward command and forward motor torque.
The historical rate-driven reverse-swing explanation therefore fails for this frozen case.
The rate-term reconstruction remains incomplete; the observed command stores alone establish the absence of pre-cut reversal.

The FOC observer records the circle test at `0x0800597A` and actual limiting branch at `0x0800597E`.
It reads the pre-limit q/d register operands directly.
It does not infer limitation from telemetry PWM or Vq alone.

| Quantum | First limit in impact window | Pre-limit q / d at last FOC before disable | Iq reference / measured at that FOC |
|---|---:|---|---|
| 4000 | 19.865107 s | 26267 / 28670 | 21000 / 14807 |
| 3000 | 19.858360 s | 30659 / 32767 | 21000 / 13662 |
| 2000 | 19.863587 s | 29090 / 31108 | 21000 / 14081 |

All current and voltage entries in this table use firmware Q15 units.
The d current in RAM includes the firmware's gain and does not equal raw plant amperes.
Error counting starts before the first observed circle limitation in all three runs.
Thus, circle limitation cannot explain the initial error-threshold crossing alone.
It can reduce subsequent recovery torque; that causal contribution still needs a controlled comparison.

The d integral continues growing while the vector limits.
For example, clean 4000 reads about 163.6 million at 19.865 s and 187.7 million at 19.875 s.
It reaches its existing bound after the first disable.
These values come from DWARF-resolved RAM fields, not inferred voltage percentages.

### Historical correction proposal and decision

**Confirmed cause:** valid clean attitude plus trim exceeds the existing error condition for eleven calls during the actual nose-down dip.
The error detector disables first, before pitch, roll, overload, stale-command, or numeric protection.
The disabled law then retains a large forward command through its slow signed ramp.
The rider release and positive recovery overshoot follow that first disable.

**Proposed first control experiment, not a proven fix:** add clean-only integral feedback from the existing circle-limited voltage vector.
Save each PI integral before its update.
If circle limitation reduces an axis voltage in the direction of that axis current error, reject that integral increment.
Keep every existing integral bound, output limit, gain, command slew, cutoff angle, detector count, and hard gate.
Keep compat unchanged.
This conditional integration needs no new tuning coefficient or protection threshold.

The measured discriminator is specific.
The change must improve measured Iq tracking during the pending error sequence and shorten that sequence below eleven calls.
It must preserve the intended 20 km/h approach at every quantum.
A changed generic scenario verdict alone does not establish this cause.
If the real dip still exhausts the unchanged detector, report the unresolved control/model limit.
Do not claim that integral feedback creates additional bus-voltage capacity.

The stop response requires a separate design decision.
`motor_disabled` stops law updates but does not immediately remove forward torque at riding speed.
A proposed stop controller can choose torque within a decreasing current envelope instead of retaining the pre-cut command sign.
That change must retain the detector latch and hard gates.
It does not remove the first cutoff or establish a passing 5 cm result.
Do not combine it with the first current-tracking experiment.

At the end of this historical study, no firmware correction was implemented or validated.
The study proposed keeping the error window unchanged while testing current-loop feedback.
The later approved clean-only error-window decision supersedes that proposal.
Its warning about a single passing quantum still applies.
The current validation above supplies the later evidence.
