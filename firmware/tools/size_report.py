#!/usr/bin/env python3
"""Per-module flash and RAM report for a firmware image, read from the GNU ld map file.

Usage (the build runs this after every target link):

    size_report.py --map build/target-clean/euc-fw-begode-extreme.map \\
                   --bin build/target-clean/euc-fw-begode-extreme.bin \\
                   --budget size-budget.toml --board begode-extreme \\
                   --out build/target-clean/size-report.json

Exit status: 0 within budget (warnings are printed), 1 over a fail limit, 2 unreadable input.

How bytes are counted
  - Memory regions come from the map's "Memory Configuration" table (FLASH and RAM).
  - Each input section is charged to the module of the object file it comes from: the source directory
    relative to firmware/ (for example "ports/stm32f1"), "libgcc" for libgcc.a members, "(fill)" for alignment
    padding inside a section, "(padding)" for gaps between sections. Module totals add up to the image size.
  - Flash: every output section whose load address is in FLASH, except NOBITS sections (.bss, .stack,
    .noinit), which have no bytes in the image. .data counts twice: initial values in flash, copy in RAM.
  - Static RAM: .data + .bss. The stack reservation is reported on its own and is not part of that budget.
  - Image size: last flash-loaded byte + 1 minus the FLASH origin. With --bin it must equal the .bin length.

Python 3.11+ standard library only.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath

NOBITS_SECTIONS = {".bss", ".stack", ".noinit"}
STACK_SECTION = ".stack"

# ".text  0x08003000  0x1a4 load address 0x0800d000" (an output section; the name may sit alone on its line).
OUTPUT_SECTION_RE = re.compile(
    r"^(?P<name>\.?[\w.$/]+)?\s+0x(?P<vma>[0-9a-fA-F]+)\s+0x(?P<size>[0-9a-fA-F]+)"
    r"(?:\s+load address 0x(?P<lma>[0-9a-fA-F]+))?\s*$"
)
# " .text.main  0x08003200  0x10 path/to/object.obj" or " *fill*  0x0800320a  0x2"
INPUT_SECTION_RE = re.compile(
    r"^ (?P<name>\S+)?\s+0x(?P<vma>[0-9a-fA-F]+)\s+0x(?P<size>[0-9a-fA-F]+)(?:\s+(?P<origin>.+?))?\s*$"
)
REGION_RE = re.compile(r"^(?P<name>\w+)\s+0x(?P<origin>[0-9a-fA-F]+)\s+0x(?P<length>[0-9a-fA-F]+)")
CMAKE_OBJECT_DIR_RE = re.compile(r"(^|/)CMakeFiles/[^/]+\.dir/")


@dataclass
class Region:
    name: str
    origin: int
    length: int

    def contains(self, address: int) -> bool:
        return self.origin <= address < self.origin + self.length


@dataclass
class OutputSection:
    name: str
    vma: int
    size: int
    lma: int

    @property
    def nobits(self) -> bool:
        return self.name in NOBITS_SECTIONS


@dataclass
class ModuleTotals:
    flash_bytes: int = 0
    ram_bytes: int = 0
    sections: dict[str, int] = field(default_factory=dict)


def module_of(origin: str | None, build_dir: PurePosixPath) -> str:
    """Maps the object file named in the map to a firmware module name.

    The build tree mirrors the source tree, so an object's directory relative to the build directory (the
    directory of the map file) is its source directory relative to firmware/.
    """
    if origin is None:
        return "(fill)"
    if origin == "linker stubs":
        return "(linker)"
    archive = re.match(r"^(?P<archive>.+?\.a)\((?P<member>.+)\)$", origin)
    if archive:
        archive_path = PurePosixPath(archive.group("archive"))
        if archive_path.name == "libgcc.a":
            return "libgcc"
        return _build_relative(archive_path.parent, build_dir) or archive_path.stem
    object_path = PurePosixPath(CMAKE_OBJECT_DIR_RE.sub(r"\1", origin))
    return _build_relative(object_path.parent, build_dir) or object_path.name


def _build_relative(directory: PurePosixPath, build_dir: PurePosixPath) -> str:
    """Directory relative to the build directory; "" for the build directory itself."""
    if directory.is_absolute() and directory.is_relative_to(build_dir):
        directory = directory.relative_to(build_dir)
    return "" if str(directory) == "." else str(directory)


def parse_map(
    text: str, build_dir: PurePosixPath
) -> tuple[dict[str, Region], list[OutputSection], dict[str, ModuleTotals]]:
    regions: dict[str, Region] = {}
    sections: list[OutputSection] = []
    contributions: list[tuple[OutputSection, str, int]] = []

    lines = text.splitlines()
    in_regions = False
    in_layout = False
    current: OutputSection | None = None
    pending_output_name: str | None = None
    pending_input_name: str | None = None

    for line in lines:
        if line.startswith("Memory Configuration"):
            in_regions = True
            continue
        if line.startswith("Linker script and memory map"):
            in_regions = False
            in_layout = True
            continue
        if in_regions:
            match = REGION_RE.match(line)
            if match and match.group("name") != "Name":
                regions[match.group("name")] = Region(
                    match.group("name"), int(match.group("origin"), 16), int(match.group("length"), 16)
                )
            continue
        if not in_layout or not line.strip():
            continue

        if not line[0].isspace():
            # Output section header, possibly with the numbers on the next line.
            match = OUTPUT_SECTION_RE.match(line)
            if match and match.group("name"):
                current = _output_section(match.group("name"), match)
                sections.append(current)
                pending_output_name = None
            elif re.match(r"^\.?[\w.$/]+\s*$", line):
                pending_output_name = line.strip()
                current = None
            else:
                current = None
            pending_input_name = None
            continue

        if pending_output_name is not None:
            match = OUTPUT_SECTION_RE.match(pending_output_name + line)
            if match:
                current = _output_section(pending_output_name, match)
                sections.append(current)
            pending_output_name = None
            continue

        if current is None:
            continue

        match = INPUT_SECTION_RE.match(line)
        if match is None:
            # " .text.long_function_name" alone: the numbers follow on the next line.
            stripped = line.strip()
            if line.startswith(" ") and not line.startswith("  ") and " " not in stripped:
                pending_input_name = stripped
            continue
        name = match.group("name") or pending_input_name
        pending_input_name = None
        if name is None or name.startswith("*(") or name.startswith("0x"):
            continue
        size = int(match.group("size"), 16)
        if size == 0:
            continue
        origin = None if name == "*fill*" else match.group("origin")
        contributions.append((current, module_of(origin, build_dir), size))

    modules: dict[str, ModuleTotals] = {}
    flash = regions.get("FLASH")
    ram = regions.get("RAM")
    for section, module, size in contributions:
        if section.name == STACK_SECTION:
            continue  # reported on its own, not charged to a module
        totals = modules.setdefault(module, ModuleTotals())
        totals.sections[section.name] = totals.sections.get(section.name, 0) + size
        if flash and flash.contains(section.lma) and not section.nobits:
            totals.flash_bytes += size
        if ram and ram.contains(section.vma) and section.name != STACK_SECTION:
            totals.ram_bytes += size
    return regions, sections, modules


def _output_section(name: str, match: re.Match[str]) -> OutputSection:
    vma = int(match.group("vma"), 16)
    lma = int(match.group("lma"), 16) if match.group("lma") else vma
    return OutputSection(name, vma, int(match.group("size"), 16), lma)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--map", required=True, type=Path, help="GNU ld map file of the image")
    parser.add_argument("--bin", type=Path, help="raw image; its length must match the computed image size")
    parser.add_argument("--budget", required=True, type=Path, help="size-budget.toml")
    parser.add_argument("--board", required=True, help="board id: the table to use in the budget file")
    parser.add_argument("--out", required=True, type=Path, help="JSON report to write")
    args = parser.parse_args()

    try:
        map_text = args.map.read_text(encoding="utf-8", errors="replace")
        budget_all = tomllib.loads(args.budget.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        print(f"size_report: {error}", file=sys.stderr)
        return 2
    if args.board not in budget_all:
        print(f"size_report: no [{args.board}] table in {args.budget}", file=sys.stderr)
        return 2
    budget = budget_all[args.board]

    regions, sections, modules = parse_map(map_text, PurePosixPath(args.map.resolve().parent.as_posix()))
    if "FLASH" not in regions or "RAM" not in regions:
        print(f"size_report: {args.map} has no FLASH and RAM regions", file=sys.stderr)
        return 2
    flash, ram = regions["FLASH"], regions["RAM"]

    flash_loaded = [s for s in sections if s.size and not s.nobits and flash.contains(s.lma)]
    image_bytes = max((s.lma + s.size for s in flash_loaded), default=flash.origin) - flash.origin
    static_ram_bytes = sum(s.size for s in sections if ram.contains(s.vma) and s.name != STACK_SECTION)
    stack_bytes = sum(s.size for s in sections if s.name == STACK_SECTION)

    # Gaps between output sections (for example the 16 bytes before .euc_info) are image bytes too.
    padding_bytes = image_bytes - sum(t.flash_bytes for t in modules.values())
    if padding_bytes > 0:
        modules.setdefault("(padding)", ModuleTotals()).flash_bytes += padding_bytes

    messages: list[str] = []
    status = "ok"
    if args.bin is not None:
        bin_bytes = args.bin.stat().st_size
        if bin_bytes != image_bytes:
            messages.append(f"{args.bin.name} is {bin_bytes} B but the map gives an image of {image_bytes} B")
            status = "fail"
    if image_bytes > budget["flash_fail_bytes"]:
        messages.append(f"flash {image_bytes} B is over the limit of {budget['flash_fail_bytes']} B")
        status = "fail"
    elif image_bytes > budget["flash_warn_bytes"]:
        messages.append(f"flash {image_bytes} B is over the warning level of {budget['flash_warn_bytes']} B")
        status = "warn" if status == "ok" else status
    if static_ram_bytes > budget["ram_static_fail_bytes"]:
        limit = budget["ram_static_fail_bytes"]
        messages.append(f"static RAM {static_ram_bytes} B is over the limit of {limit} B")
        status = "fail"

    report = {
        "schema": 1,
        "map": args.map.name,
        "board": args.board,
        "status": status,
        "messages": messages,
        "flash": {
            "origin": flash.origin,
            "region_bytes": flash.length,
            "image_bytes": image_bytes,
            "warn_bytes": budget["flash_warn_bytes"],
            "fail_bytes": budget["flash_fail_bytes"],
        },
        "ram": {
            "origin": ram.origin,
            "region_bytes": ram.length,
            "static_bytes": static_ram_bytes,
            "stack_bytes": stack_bytes,
            "static_fail_bytes": budget["ram_static_fail_bytes"],
        },
        "sections": [
            {"name": s.name, "vma": s.vma, "lma": s.lma, "size_bytes": s.size}
            for s in sections
            if s.size and (flash.contains(s.lma) or ram.contains(s.vma))
        ],
        "modules": [
            {"module": name, "flash_bytes": t.flash_bytes, "ram_bytes": t.ram_bytes, "sections": t.sections}
            for name, t in sorted(modules.items(), key=lambda item: (-item[1].flash_bytes, item[0]))
        ],
    }
    args.out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    print(
        f"{args.map.stem}: flash {image_bytes} / {budget['flash_fail_bytes']} B "
        f"({100.0 * image_bytes / budget['flash_fail_bytes']:.1f} %), "
        f"static RAM {static_ram_bytes} / {budget['ram_static_fail_bytes']} B, stack {stack_bytes} B"
    )
    print(f"  {'module':<32} {'flash B':>8} {'RAM B':>8}")
    for entry in report["modules"]:
        print(f"  {entry['module']:<32} {entry['flash_bytes']:>8} {entry['ram_bytes']:>8}")
    for message in messages:
        print(f"  {status.upper()}: {message}")
    return 1 if status == "fail" else 0


if __name__ == "__main__":
    sys.exit(main())
