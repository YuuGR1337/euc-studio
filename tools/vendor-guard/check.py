#!/usr/bin/env python3
"""Vendor guard: keep vendor firmware material out of the public euc-studio repo.

The public repo may describe how stock firmware behaves, but it must never contain the firmware itself, decompiled
or disassembled code, Ghidra databases, or long runs of the original bytes. This script enforces that, as a
pre-commit hook (``--staged``) and in CI (``--all``).

Checks, in order of cost:
  1. path     blocked locations and file types (``research/``, Ghidra projects, ...)
  2. sha256   exact copies of known vendor files
  3. token    decompiler auto symbols (FUN_ / DAT_ plus an 8-digit address), decompiler types and disassembly
              listing lines
  4. image    files that look like a Cortex-M flash image (initial SP + Thumb reset vector) outside build outputs
  5. stock    any 16-byte run of a stock image inside a file, as raw bytes or as hex text ("deep" check; needs the
              stock images, found in the private research folder or given with --stock-dir)

Rules live in ``denylist.toml`` next to this file. Python 3.11+ standard library only.
"""

from __future__ import annotations

import argparse
import fnmatch
import hashlib
import os
import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
WINDOW = 16  # bytes per shingle in the deep check
STOCK_STEP = 4  # stride when indexing stock images (candidate files are scanned at every offset)
MIN_DISTINCT = 6  # skip low-entropy windows (padding, zero fill) that would match anything
HEX_RUN = re.compile(rb"(?:(?:0x)?[0-9A-Fa-f]{2}[\s,:]{0,2}){16,}")
HEX_PAIR = re.compile(rb"(?:0x)?([0-9A-Fa-f]{2})")


@dataclass(frozen=True)
class Finding:
    path: str
    rule: str
    detail: str

    def __str__(self) -> str:
        return f"{self.path}: [{self.rule}] {self.detail}"


@dataclass(frozen=True)
class Rules:
    sha256: dict[str, str]
    paths: list[tuple[str, str]]
    tokens: list[tuple[str, re.Pattern[bytes], str]]
    image_allow: list[str]
    allow: list[tuple[str, set[str], str]]


def load_rules(path: Path) -> Rules:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    return Rules(
        sha256={k.lower(): v for k, v in data.get("sha256", {}).items()},
        paths=[(p["glob"], p["reason"]) for p in data.get("path", [])],
        tokens=[
            (t["name"], re.compile(t["regex"].encode(), re.MULTILINE), t["reason"]) for t in data.get("token", [])
        ],
        image_allow=list(data.get("image", {}).get("allow", [])),
        allow=[(a["path"], set(a["rules"]), a["reason"]) for a in data.get("allow", [])],
    )


def matches(path: str, pattern: str) -> bool:
    """Glob match where ``**`` spans directories and a bare name matches in any directory."""
    if fnmatch.fnmatchcase(path, pattern):
        return True
    if pattern.startswith("**/") and fnmatch.fnmatchcase(path, pattern[3:]):
        return True
    return False


def allowed(rules: Rules, path: str, rule: str) -> bool:
    for pattern, names, _ in rules.allow:
        if matches(path, pattern) and (rule in names or rule.split(":")[0] + ":*" in names):
            return True
    return False


def looks_like_cortex_m_image(blob: bytes) -> bool:
    if len(blob) < 64:
        return False
    sp = int.from_bytes(blob[0:4], "little")
    reset = int.from_bytes(blob[4:8], "little")
    sp_ok = 0x2000_0000 <= sp <= 0x2010_0000 and sp % 8 == 0
    reset_ok = reset & 1 == 1 and 0x0800_0000 <= reset < 0x0810_0000
    return sp_ok and reset_ok


class StockIndex:
    """Set of 16-byte windows taken from the stock images, used to spot copied byte runs."""

    def __init__(self, images: list[Path]) -> None:
        self.names = [p.name for p in images]
        self.windows: set[bytes] = set()
        for image in images:
            blob = image.read_bytes()
            for i in range(0, len(blob) - WINDOW + 1, STOCK_STEP):
                w = blob[i : i + WINDOW]
                if len(set(w)) >= MIN_DISTINCT:
                    self.windows.add(w)

    def first_hit(self, blob: bytes) -> int | None:
        windows = self.windows
        for i in range(len(blob) - WINDOW + 1):
            w = blob[i : i + WINDOW]
            if w in windows:
                return i
        return None


def hex_text_runs(blob: bytes) -> list[bytes]:
    """Decode runs of hex byte text ("55 aa 19", "0x55, 0xaa") into raw bytes."""
    out = []
    for run in HEX_RUN.finditer(blob):
        out.append(bytes(int(m.group(1), 16) for m in HEX_PAIR.finditer(run.group(0))))
    return out


def check_file(rules: Rules, path: str, blob: bytes, stock: StockIndex | None) -> list[Finding]:
    found: list[Finding] = []

    def add(rule: str, detail: str) -> None:
        if not allowed(rules, path, rule):
            found.append(Finding(path, rule, detail))

    for pattern, reason in rules.paths:
        if matches(path, pattern):
            add(f"path:{pattern}", reason)

    digest = hashlib.sha256(blob).hexdigest()
    if digest in rules.sha256:
        add("sha256", f"exact copy of vendor file: {rules.sha256[digest]}")

    for name, regex, reason in rules.tokens:
        m = regex.search(blob)
        if m:
            line = blob.count(b"\n", 0, m.start()) + 1
            add(f"token:{name}", f"line {line}: {reason}")

    if looks_like_cortex_m_image(blob) and not any(matches(path, p) for p in rules.image_allow):
        add("image", "looks like a Cortex-M flash image (initial SP + Thumb reset vector)")

    if stock is not None:
        at = stock.first_hit(blob)
        if at is not None:
            add("stock", f"16-byte run of a stock image at byte offset {at}")
        else:
            for run in hex_text_runs(blob):
                if stock.first_hit(run) is not None:
                    add("stock", "hex text that decodes to a 16-byte run of a stock image")
                    break
    return found


def git(*args: str) -> bytes:
    return subprocess.run(["git", *args], check=True, capture_output=True).stdout


def repo_files(staged: bool) -> list[tuple[str, bytes]]:
    if staged:
        names = git("diff", "--cached", "--name-only", "-z", "--diff-filter=ACMR").split(b"\0")
        return [(n.decode(), git("cat-file", "blob", f":{n.decode()}")) for n in names if n]
    names = git("ls-files", "-z", "--cached", "--others", "--exclude-standard").split(b"\0")
    out = []
    for n in names:
        if n and os.path.isfile(n):
            out.append((n.decode(), Path(n.decode()).read_bytes()))
    return out


def find_stock_dir(explicit: str | None) -> Path | None:
    """Stock images: --stock-dir, $EUC_STOCK_DIR, or the private research folder next to the main checkout."""
    if explicit:
        return Path(explicit)
    if os.environ.get("EUC_STOCK_DIR"):
        return Path(os.environ["EUC_STOCK_DIR"])
    try:
        common = Path(git("rev-parse", "--path-format=absolute", "--git-common-dir").decode().strip())
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None
    candidate = common.parent / "research" / "begode-firmware"
    return candidate if candidate.is_dir() else None


def stock_images(directory: Path, rules: Rules) -> list[Path]:
    """Only files whose sha256 is a known vendor image count, so a stray file never weakens the check."""
    images = []
    for p in sorted(directory.glob("*.bin")):
        if hashlib.sha256(p.read_bytes()).hexdigest() in rules.sha256:
            images.append(p)
    return images


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--staged", action="store_true", help="check the staged content (pre-commit)")
    mode.add_argument("--all", action="store_true", help="check every tracked and untracked, not ignored file (default)")
    ap.add_argument("--stock-dir", help="folder with the stock images for the deep check")
    ap.add_argument("--require-deep", action="store_true", help="fail if the deep check cannot run")
    ap.add_argument("--rules", default=str(HERE / "denylist.toml"))
    args = ap.parse_args(argv)

    rules = load_rules(Path(args.rules).resolve())
    os.chdir(git("rev-parse", "--show-toplevel").decode().strip())
    stock = None
    stock_dir = find_stock_dir(args.stock_dir)
    if stock_dir is not None:
        images = stock_images(stock_dir, rules)
        if images:
            stock = StockIndex(images)
    if stock is None:
        msg = "vendor-guard: deep check skipped (no stock images found; set --stock-dir or EUC_STOCK_DIR)"
        if args.require_deep:
            print(msg.replace("skipped", "required but unavailable"), file=sys.stderr)
            return 2
        print(msg, file=sys.stderr)

    files = repo_files(args.staged)
    findings = [f for path, blob in files for f in check_file(rules, path, blob, stock)]
    deep = f", deep check against {len(stock.names)} stock images" if stock else ""
    if findings:
        print(f"vendor-guard: {len(findings)} problem(s) in {len(files)} files{deep}:", file=sys.stderr)
        for f in findings:
            print(f"  {f}", file=sys.stderr)
        print("Vendor material belongs in the private research repo, never here. See tools/vendor-guard/README.md.",
              file=sys.stderr)
        return 1
    print(f"vendor-guard: {len(files)} files clean{deep}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
