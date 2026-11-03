#!/usr/bin/env python3
"""Validate the SI.Mind feeling lexicon.

Checks, in order:
  1. every entry has the required schema fields
  2. ids are unique across all family files
  3. every `nearest` reference resolves to a real id in the same file's family
  4. axis values are within [-1, 1]
  5. the total entry count equals EXPECTED_TOTAL

Run:  python tools/validate_lexicon.py [--fix]
       --fix rewrites dangling `nearest` references to the nearest valid ids
             in the same family, and reports (but does not perform) trimming.

Exit code 0 when valid, 1 otherwise.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

EXPECTED_TOTAL = 250
DATA_DIR = Path(__file__).resolve().parent.parent / "crates" / "si-affect" / "data" / "feelings"

REQUIRED_FIELDS = ("id", "name", "family", "definition", "valence", "arousal", "tension", "bias", "nearest")

ENTRY_RE = re.compile(r"^- id: (?P<id>.+)$")
FIELD_RE = re.compile(r"^  (?P<key>[a-z_]+):(?P<value>.*)$")
NEAREST_RE = re.compile(r"^\[(?P<items>.*)\]$")


class Entry:
    """One feeling, with the file it came from and its line offset."""

    def __init__(self, entry_id: str, family: str, path: Path, line_no: int) -> None:
        self.id = entry_id
        self.family = family
        self.path = path
        self.line_no = line_no
        self.fields: dict[str, str] = {}
        self.nearest: list[str] = []

    @property
    def location(self) -> str:
        return f"{self.path.name}:{self.line_no}"

    def axis(self, key: str) -> float | None:
        raw = self.fields.get(key, "").strip()
        try:
            return float(raw)
        except ValueError:
            return None


def parse_files(paths: list[Path]) -> list[Entry]:
    """Parse the lexicon YAML files into entries without requiring a YAML dependency."""
    entries: list[Entry] = []
    for path in paths:
        current: Entry | None = None
        for line_no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            stripped = line.strip()
            if stripped.startswith("#") or not stripped:
                continue
            id_match = ENTRY_RE.match(line)
            if id_match:
                entry_id = id_match.group("id").strip()
                current = Entry(entry_id, "", path, line_no)
                current.fields["id"] = entry_id
                entries.append(current)
                continue
            field_match = FIELD_RE.match(line)
            if field_match and current is not None:
                key = field_match.group("key")
                value = field_match.group("value").strip()
                current.fields[key] = value
                if key == "family":
                    current.family = value
                if key == "nearest":
                    nearest_match = NEAREST_RE.match(value)
                    if nearest_match:
                        current.nearest = [
                            item.strip() for item in nearest_match.group("items").split(",") if item.strip()
                        ]
    return entries


def validate(entries: list[Entry], fix: bool) -> list[str]:
    """Return a list of problem descriptions; empty means the lexicon is valid."""
    problems: list[str] = []

    for entry in entries:
        for field in REQUIRED_FIELDS:
            if field not in entry.fields:
                problems.append(f"{entry.location}: missing field '{field}' (id={entry.id})")

        for axis in ("valence", "arousal", "tension"):
            value = entry.axis(axis)
            if value is None:
                problems.append(f"{entry.location}: axis '{axis}' is not a number (id={entry.id})")
            elif not -1.0 <= value <= 1.0:
                problems.append(f"{entry.location}: axis '{axis}'={value} outside [-1, 1] (id={entry.id})")

    seen: dict[str, Entry] = {}
    for entry in entries:
        if entry.id in seen:
            first = seen[entry.id]
            problems.append(
                f"duplicate id '{entry.id}': {first.location} and {entry.location} "
                f"(families {first.family} / {entry.family})"
            )
        else:
            seen[entry.id] = entry

    by_family: dict[str, set[str]] = {}
    all_ids = {entry.id for entry in entries}
    for entry in entries:
        by_family.setdefault(entry.family, set()).add(entry.id)

    for entry in entries:
        # Cross-family neighbours are intentional: `pride` (positive_activation) is
        # genuinely nearest to `self-trust` (moral_self). Resolution is global.
        dangling = [ref for ref in entry.nearest if ref not in all_ids]
        if dangling:
            problems.append(
                f"{entry.location}: dangling nearest reference(s) {dangling} (id={entry.id}, family={entry.family})"
            )

    if len(entries) != EXPECTED_TOTAL:
        problems.append(f"entry count is {len(entries)}, expected {EXPECTED_TOTAL}")

    return problems


def apply_nearest_fixes(entries: list[Entry]) -> int:
    """Rewrite dangling `nearest` refs to the genuinely closest valid entries.

    Distance is Euclidean in the three-axis affect space, so a replacement is
    semantically the nearest feeling rather than merely the first available one.
    Returns the number of entries rewritten.
    """
    valid = {entry.id: entry for entry in entries}
    fixes = 0

    for entry in entries:
        if not entry.nearest:
            continue
        kept = [ref for ref in entry.nearest if ref in valid]
        if len(kept) == len(entry.nearest):
            continue

        origin = (entry.axis("valence"), entry.axis("arousal"), entry.axis("tension"))
        candidates = [
            other
            for other in entries
            if other.id != entry.id and other.id not in kept and other.id in valid
        ]

        def distance(other: Entry) -> float:
            if None in origin:
                return 0.0
            target = (other.axis("valence"), other.axis("arousal"), other.axis("tension"))
            return sum((a - b) ** 2 for a, b in zip(origin, target)) ** 0.5

        candidates.sort(key=distance)
        needed = len(entry.nearest) - len(kept)
        kept.extend(other.id for other in candidates[:needed])
        entry.nearest = kept[: len(entry.nearest)]
        entry.fields["nearest"] = "[" + ", ".join(entry.nearest) + "]"
        fixes += 1
    return fixes


def write_back(entries: list[Entry]) -> None:
    """Rewrite each source file, substituting updated `nearest` values in place."""
    by_path: dict[Path, list[Entry]] = {}
    for entry in entries:
        by_path.setdefault(entry.path, []).append(entry)

    for path, path_entries in by_path.items():
        lines = path.read_text(encoding="utf-8").splitlines()
        for entry in path_entries:
            index = entry.line_no - 1
            for offset in range(index, min(index + 12, len(lines))):
                if lines[offset].lstrip().startswith("nearest:"):
                    lines[offset] = f"  nearest: [{', '.join(entry.nearest)}]"
                    break
        path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate the SI.Mind feeling lexicon.")
    parser.add_argument("--fix", action="store_true", help="rewrite dangling nearest references in place")
    args = parser.parse_args()

    paths = sorted(DATA_DIR.glob("*.yaml"))
    if not paths:
        print(f"no lexicon files found under {DATA_DIR}", file=sys.stderr)
        return 1

    entries = parse_files(paths)
    problems = validate(entries, args.fix)

    if args.fix:
        fixes = apply_nearest_fixes(entries)
        if fixes:
            write_back(entries)
            print(f"repaired {fixes} dangling nearest reference set(s)")
        entries = parse_files(paths)
        problems = validate(entries, args.fix)

    counts: dict[str, int] = {}
    for entry in entries:
        counts[entry.family] = counts.get(entry.family, 0) + 1

    print(f"files:   {len(paths)}")
    print(f"entries: {len(entries)} (expected {EXPECTED_TOTAL})")
    for family in sorted(counts):
        print(f"  {family:<22} {counts[family]}")

    if problems:
        print(f"\n{len(problems)} problem(s):", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1

    print("\nlexicon valid")
    return 0


if __name__ == "__main__":
    sys.exit(main())
