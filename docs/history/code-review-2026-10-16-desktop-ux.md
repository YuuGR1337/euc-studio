# Desktop change review

> **Historical review (2026-10-16).** Written during the agent-built desktop work; it describes the state of its
> date, not the current one. See [project status](../project-status.md) and [the docs index](../README.md).

## Review steps

| Step | State | Evidence |
| --- | --- | --- |
| 0. Change impact | Complete | No `.code-graph/graph.db` exists. Source review replaces graph analysis. |
| 0.3. Change risks | Complete | IPC contracts, recording format, runtime controls, firmware startup, and package configuration change. No dependency version upgrade occurs. |
| 0.5. Plan check | In progress | Compare the completion plan and `app/DESIGN.md` with source. |
| 0.7. Review areas | Complete | Split native controls, recording context, shared shell, and workspace UI among separate reviewers. |
| 1. Collect changes | Complete | The active checkout contains tracked edits and new modules. `git status` identifies both groups. |
| 2. Review files | In progress | Read source and inspect rendered layouts at desktop sizes. |
| 3. Independent review | In progress | A separate reviewer checks control behavior and design rules. |
| 4. Final report | Pending | Record confirmed defects, corrections, and remaining limits. |
| 5. Update documents | In progress | `app/DESIGN.md` now defines workspace layout and revised audio behavior. |

## Confirmed corrections

- The Recordings bar reduced the viewport height. The utility rail now opens the shared modal sheet.
- The run bar wrapped its labels at 1280 pixels. Compact controls now keep number readouts on one line.
- Vite rejected the bundled number font from shared dependencies. It now permits the font package directory.
- The button duration selector and collapsible section conflicted with the requested interaction. Measured holds replace them.
- The audio path muted valid beeps when measured speed dropped. It no longer uses that mute rule.
- A stock register trace found the RTC prescaler reload defect. Its delayed first interrupt left startup volume at zero. The correction and waveform comparison remain active.

## Review limits

Browser captures show the demo layout. They do not prove native button delivery or audible output.
Subsequent integration checks include 236 frontend tests, 72 camera/pose tests, native button tests,
battery/plant boundary regressions, recording round trips, and ten browser end-to-end tests.
Native startup output and the power supply model remain under investigation. Final source edits
require another integration pass. No check establishes physical calibration or riding safety.
