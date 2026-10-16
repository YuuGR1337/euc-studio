# Contributing to EUC Studio

Thank you for helping make EUC firmware better and safer. Please read [SAFETY.md](SAFETY.md) first.

## Ground rules

- **License.** Contributions are accepted under the repository license (PolyForm Noncommercial 1.0.0). Sign off every
  commit with `git commit -s` to certify the [Developer Certificate of Origin](https://developercertificate.org/).
- **No vendor material.** Never commit stock firmware images, decompiled or disassembled vendor code, analysis
  databases, or long byte runs copied from a vendor image. Facts about hardware and behavior, written in your own words
  and code, are welcome. Install the guard once per clone: `git config core.hooksPath tools/hooks`.
- **No copied GPL code.** Protocol facts from other EUC apps are fine; their code is not.
- **Safety-relevant changes need evidence.** A change to balance, motor control, protections or the flasher needs a
  simulator scenario that shows its effect, and for the firmware the lock-step result against stock where it applies.

## Layout

| Path | What | Language |
|---|---|---|
| `crates/` | simulation engine, firmware sources, image tools, CLI `euc` | Rust |
| `firmware/` | our controller firmware (compat and clean profiles) | C11 |
| `app/` | EUC Studio desktop app | TypeScript, React, Tauri |
| `profiles/` | wheel definitions (board, plant, firmware sources) | TOML |
| `tests/data/` | real-wheel captures used as test data | text |
| `tools/` | vendor guard, git hooks, helper scripts | Python, shell |
| `docs/` | specifications and guides | Markdown |

## Before you open a pull request

- The vendor guard passes: `python3 tools/vendor-guard/check.py --all`.
- The checks for the area you touched pass (see the README of that area and the CI workflows in `.github/workflows/`).
- The pull request says what changed, why, and how you verified it.
