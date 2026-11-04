#!/usr/bin/env python3
"""Regenerate the `nearest` lists of the SI.Mind feeling lexicon.

`nearest` is a **derived** field, not a curated one. It holds the ids of the
three other feelings whose canonical points are closest to this one's, measured
as Euclidean distance in the three-axis affect space:

    nearest(f) = the three ids closest to f, excluding f itself

Ties are broken by the position of the feeling in the embedded lexicon — file
order first, then line order within a file — so the result is deterministic.
`tools/validate_lexicon.py` recomputes this rule with the same code that
writes it, and refuses a lexicon whose `nearest` lists disagree with it, which
is what stops the data drifting away from its own definition again.

The lexicon files are hand-maintained YAML with a header comment and a
documented schema, so this tool rewrites the `nearest:` line of every feeling
and nothing else. A run that changes nothing leaves the files byte-identical,
line endings included.

Run:  python tools/regenerate_nearest.py [--check]
       --check reports what would change and exits 1 if anything would,
       without writing anything.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

# How many neighbours a feeling names. Three is the number the brief fixes:
# enough to describe a neighbourhood, few enough to stay readable in the data.
NEAREST_COUNT = 3

DATA_DIR = Path(__file__).resolve().parent.parent / "crates" / "si-affect" / "data" / "feelings"

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
        self.nearest_line_no = 0
        self.bias: dict[str, float] = {}

    @property
    def location(self) -> str:
        return f"{self.path.name}:{self.line_no}"

    def axis(self, key: str) -> float | None:
        raw = self.fields.get(key, "").strip()
        try:
            return float(raw)
        except ValueError:
            return None

    def point(self) -> tuple[float | None, float | None, float | None]:
        """The canonical point in the three-axis affect space."""
        return (self.axis("valence"), self.axis("arousal"), self.axis("tension"))


def lexicon_paths() -> list[Path]:
    """The lexicon files, in embedded order: file order, then line order."""
    return sorted(DATA_DIR.glob("*.yaml"))


def parse_files(paths: list[Path]) -> list[Entry]:
    """Parse the lexicon YAML files into entries without requiring a YAML dependency.

    The files are read line by line rather than through a YAML library, so the
    formatting, the comments and the key order of a hand-maintained file are
    never in question: the parser only ever reads, and only ever writes back
    the exact `nearest:` line it found.
    """
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
                    current.nearest_line_no = line_no
                    nearest_match = NEAREST_RE.match(value)
                    if nearest_match:
                        current.nearest = [
                            item.strip()
                            for item in nearest_match.group("items").split(",")
                            if item.strip()
                        ]
                if key == "bias":
                    for pair in value.strip("{}").split(","):
                        if ":" not in pair:
                            continue
                        name, _, amount = pair.partition(":")
                        try:
                            current.bias[name.strip()] = float(amount.strip())
                        except ValueError:
                            pass
    return entries


def distance(a: tuple[float | None, ...], b: tuple[float | None, ...]) -> float:
    """Euclidean distance between two canonical points.

    A point with an unreadable axis is infinitely far from everything: the
    validator reports the unreadable axis separately, and a feeling whose
    position cannot be read must never win a nearest-neighbour contest.
    """
    if any(value is None for value in (*a, *b)):
        return float("inf")
    return sum((x - y) ** 2 for x, y in zip(a, b)) ** 0.5


def canonical_nearest(entries: list[Entry]) -> dict[str, list[str]]:
    """The canonical `nearest` list for every feeling.

    The three closest canonical points, excluding the feeling itself. Ties are
    broken by the position of the feeling in the embedded lexicon — file order,
    then line order — which is the index of the entry in `entries`.

    Ids are taken once, at their first occurrence, so a list never repeats an
    id even if the data does: a duplicate id is a defect the validator reports
    separately, and it must not be able to produce a list the validator would
    then reject.
    """
    unique: list[Entry] = []
    seen: set[str] = set()
    for entry in entries:
        if entry.id in seen:
            continue
        seen.add(entry.id)
        unique.append(entry)

    points = {entry.id: entry.point() for entry in unique}
    order = {entry.id: index for index, entry in enumerate(unique)}

    nearest: dict[str, list[str]] = {}
    for entry in unique:
        ranked = sorted(
            (distance(points[entry.id], points[other.id]), order[other.id], other.id)
            for other in unique
            if other.id != entry.id
        )
        nearest[entry.id] = [item[2] for item in ranked[:NEAREST_COUNT]]
    return nearest


def write_nearest(entries: list[Entry], nearest: dict[str, list[str]]) -> int:
    """Rewrite the `nearest:` line of every feeling, and nothing else.

    A file whose lists already match is not written at all, so a run that
    changes nothing leaves the working tree byte-identical. Returns the number
    of lines rewritten.
    """
    by_path: dict[Path, list[Entry]] = {}
    for entry in entries:
        by_path.setdefault(entry.path, []).append(entry)

    written = 0
    for path, path_entries in by_path.items():
        with path.open("r", encoding="utf-8", newline="") as handle:
            lines = handle.read().splitlines(keepends=True)

        changed = False
        for entry in path_entries:
            if entry.nearest_line_no <= 0:
                # A feeling with no `nearest:` line has nothing to rewrite.
                # The validator reports the missing field; this tool leaves the
                # file alone rather than inventing a line for it.
                continue
            index = entry.nearest_line_no - 1
            line = lines[index]
            body = line.rstrip("\r\n")
            ending = line[len(body) :]
            indent = body[: len(body) - len(body.lstrip())]
            rewritten = f"{indent}nearest: [{', '.join(nearest[entry.id])}]"
            if rewritten != body:
                lines[index] = rewritten + ending
                changed = True
                written += 1

        if changed:
            with path.open("w", encoding="utf-8", newline="") as handle:
                handle.write("".join(lines))

    return written


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Regenerate the derived `nearest` lists of the SI.Mind feeling lexicon."
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="report what would change and exit 1 if anything would, without writing",
    )
    args = parser.parse_args()

    paths = lexicon_paths()
    if not paths:
        print(f"no lexicon files found under {DATA_DIR}", file=sys.stderr)
        return 1

    entries = parse_files(paths)
    nearest = canonical_nearest(entries)

    without_line = [entry for entry in entries if entry.nearest_line_no <= 0]
    if without_line:
        print(f"{len(without_line)} feeling(s) have no `nearest:` line:", file=sys.stderr)
        for entry in without_line[:20]:
            print(f"  - {entry.location}: (id={entry.id})", file=sys.stderr)
        return 1

    stale = [entry for entry in entries if entry.nearest != nearest[entry.id]]

    print(f"files:   {len(paths)}")
    print(f"entries: {len(entries)}")
    print(f"nearest: {len(entries)} lists, {len(stale)} disagree with the rule")

    if args.check:
        if stale:
            print(f"\n{len(stale)} list(s) would be rewritten:", file=sys.stderr)
            for entry in stale[:20]:
                print(
                    f"  - {entry.location}: {entry.nearest} -> {nearest[entry.id]} (id={entry.id})",
                    file=sys.stderr,
                )
            return 1
        print("\nnearest lists already match the rule")
        return 0

    written = write_nearest(entries, nearest)
    if written:
        print(f"\nrewrote {written} nearest list(s) across {len(paths)} file(s)")
    else:
        print("\nno change: every nearest list already matches the rule")
    return 0


if __name__ == "__main__":
    sys.exit(main())
