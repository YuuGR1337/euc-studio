# Vendor guard

euc-studio documents how stock EUC firmware behaves so that our own firmware can match it and improve on it. It never
redistributes the vendor firmware itself. The guard enforces that line on every commit and in CI.

## What it blocks

| Rule | Catches |
|---|---|
| `path` | `research/` (the private research repo), Ghidra/IDA/Hopper databases, decompiler output files |
| `sha256` | exact copies of known stock images and their decompressed data |
| `token` | decompiler auto symbols and types, disassembly listing lines of vendor flash |
| `image` | any file that looks like a Cortex-M flash image, outside our own build folders |
| `stock` | any 16-byte run of a stock image inside a file, as raw bytes, hex text or a C byte array |

The `stock` rule needs the stock images. It finds them in the private research folder next to the main checkout, in
`$EUC_STOCK_DIR`, or with `--stock-dir`. Only files whose sha256 is a known stock build are used. CI downloads the
official images from the vendor's update server for this check and never stores them.

## Use

```sh
git config core.hooksPath tools/hooks          # once per clone: run the guard before each commit
python3 tools/vendor-guard/check.py --all       # whole tree (tracked and untracked files)
python3 tools/vendor-guard/check.py --staged    # what the pre-commit hook runs
python3 tools/vendor-guard/check.py --all --stock-dir DIR --require-deep   # fail if the deep check cannot run
python3 -m unittest discover -s tools/vendor-guard -p 'test_*.py'
```

## What is allowed

Facts about the hardware and the firmware's behavior are fine: pin maps, register addresses, scales, rates, protocol
frames, command letters, tuning constants, flash page addresses, and a minimal symbol map of RAM variables with our own
names. Write them in your own words and code.

If a rule fires on something legitimate, add a narrow `[[allow]]` entry to `denylist.toml` with the path, the rule it
waives and the reason. Never widen a rule to make a commit pass.
