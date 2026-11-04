#!/usr/bin/env python3
"""Validate the SI.Mind feeling lexicon.

Checks, in order:
  1. every entry has the required schema fields
  2. ids are unique across all family files
  3. axis values are within [-1, 1], and are whole numbers of hundredths, so
     the `nearest` rule is defined for them
  4. every feeling carries a non-empty bias, every bias key is inside the
     closed registry, and every bias value is within [-1, 1]
  5. no two feelings share a canonical point
  6. every `nearest` list is exactly the three nearest feelings by exact
     distance, never names the feeling itself, and never repeats an id
  7. the total entry count equals EXPECTED_TOTAL

`nearest` references resolve against the whole lexicon, not against one
family: `pride` (positive_activation) is genuinely nearest to `self-trust`
(moral_self), so a cross-family reference is correct rather than a defect.

`nearest` is a derived field. The rule that defines it — the three closest
canonical points, compared as exact integers, ties broken by the position of
the feeling in the embedded lexicon — lives in `tools/regenerate_nearest.py`,
and this validator checks the data against that same rule, so the lists cannot
drift away from their own definition the way hand-written ones did.

The comparison is exact: every coordinate is a hundredth, so the squared
distance scaled by 10000 is an exact integer, and the rule compares those
integers. Comparing floating-point distances cannot do this — the square root
of the same squared sum rounds differently depending on the order the terms
were added, so two feelings that are exactly equidistant come out at different
distances and float noise picks the neighbour instead of the rule.

Run:  python tools/validate_lexicon.py [--fix]
       --fix rewrites every `nearest` list to the three nearest feelings by
             exact distance, and reports how many lists it rewrote.

Exit code 0 when valid, 1 otherwise.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

# The parser and the rule that defines `nearest` live with the tool that writes
# it, so the validator and the regenerator cannot disagree about what a
# correct list is. The sibling module is imported by path, so both tools work
# from any working directory. The bytecode cache that import would leave in a
# hand-maintained tools directory is noise, so it is not written.
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from regenerate_nearest import (  # noqa: E402  (the path has to be set first)
    DATA_DIR,
    Entry,
    NEAREST_COUNT,
    canonical_nearest,
    lexicon_paths,
    parse_files,
    write_nearest,
)

EXPECTED_TOTAL = 250

# The closed set of cognition parameters the loop actually reads. A bias naming
# anything outside this registry is a no-op, so the lexicon and the loop cannot
# drift apart. `pace` is signed: negative slows down and yields, positive pushes on.
BIAS_REGISTRY = {
    "explore": "willingness to try an unfamiliar approach",
    "retry_same": "willingness to repeat the current approach after a failure",
    "verify": "propensity to check work or test a belief before accepting it",
    "persist": "tendency to continue a goal past difficulty",
    "abandon": "tendency to drop a goal that is not working",
    "risk": "willingness to attempt the uncertain or costly",
    "ask": "tendency to request clarification from the user",
    "connect": "tendency to reach for related notes or parties",
    "create": "tendency to write new notes rather than reuse existing ones",
    "revise": "tendency to change existing beliefs rather than add alongside",
    "express": "tendency to externalize feelings and findings",
    "refuse": "tendency to decline an action outright",
    "pace": "signed pacing: negative slows down and yields, positive pushes ahead",
}

REQUIRED_FIELDS = ("id", "name", "family", "definition", "valence", "arousal", "tension", "bias", "nearest")


def validate(entries: list[Entry]) -> list[str]:
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
            elif entry.hundredths(axis) is None:
                # The rule is defined on whole hundredths, so a coordinate
                # written to a finer scale has no exact nearest neighbours.
                problems.append(
                    f"{entry.location}: axis '{axis}'={value} is not a whole number of "
                    f"hundredths, so the nearest rule is not defined for it (id={entry.id})"
                )

        # The anti-wallpaper rule, enforced: a feeling must be able to move behaviour,
        # and it may only move parameters the cognition loop actually reads.
        if not entry.bias:
            problems.append(
                f"{entry.location}: empty bias - a feeling that cannot move behaviour is a label, "
                f"not a feeling (id={entry.id})"
            )
        unknown = sorted(set(entry.bias) - set(BIAS_REGISTRY))
        if unknown:
            problems.append(
                f"{entry.location}: bias key(s) {unknown} outside the registry (id={entry.id})"
            )
        for name, amount in sorted(entry.bias.items()):
            if not -1.0 <= amount <= 1.0:
                problems.append(
                    f"{entry.location}: bias '{name}'={amount} outside [-1, 1] (id={entry.id})"
                )

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

    # Two feelings at one canonical point make the labeller's choice between
    # them an accident of file order, and leave the later of the two unable to
    # be the primary label at all. The point has to be impossible to write.
    points: dict[tuple[int, int, int], Entry] = {}
    for entry in entries:
        point = entry.exact_point()
        if point is None:
            continue
        first = points.get(point)
        if first is None:
            points[point] = entry
        else:
            problems.append(
                f"{entry.location}: canonical point ({entry.fields['valence']}, "
                f"{entry.fields['arousal']}, {entry.fields['tension']}) is shared with "
                f"'{first.id}' at {first.location} - two feelings at one point make the "
                f"labeller's choice an accident (id={entry.id})"
            )

    # `nearest` is derived: the three closest canonical points, compared as
    # exact integers, ties broken by the position of the feeling in the
    # embedded lexicon. The same rule that writes the lists checks them, so
    # the data cannot drift from its definition.
    canonical = canonical_nearest(entries)
    all_ids = {entry.id for entry in entries}

    for entry in entries:
        # Cross-family neighbours are intentional: `pride` (positive_activation) is
        # genuinely nearest to `self-trust` (moral_self). Resolution is global.
        dangling = [ref for ref in entry.nearest if ref not in all_ids]
        if dangling:
            problems.append(
                f"{entry.location}: dangling nearest reference(s) {dangling} (id={entry.id}, family={entry.family})"
            )

        if entry.id in entry.nearest:
            problems.append(
                f"{entry.location}: nearest names the feeling itself ('{entry.id}') (id={entry.id})"
            )

        repeated = sorted({ref for ref in entry.nearest if entry.nearest.count(ref) > 1})
        if repeated:
            problems.append(
                f"{entry.location}: nearest repeats {repeated} (id={entry.id})"
            )

        expected = canonical.get(entry.id, [])
        if entry.nearest != expected:
            problems.append(
                f"{entry.location}: nearest is not the {NEAREST_COUNT} nearest feelings by "
                f"exact distance - expected {expected}, found {entry.nearest} (id={entry.id})"
            )

    if len(entries) != EXPECTED_TOTAL:
        problems.append(f"entry count is {len(entries)}, expected {EXPECTED_TOTAL}")

    return problems


def apply_nearest_fixes(entries: list[Entry]) -> int:
    """Rewrite every `nearest` list to the canonical three nearest ids.

    The rule is the one the validator checks, so a repaired lexicon is one the
    checks then accept: `--fix` cannot write a list the validator would
    reject. Returns the number of lists rewritten.
    """
    nearest = canonical_nearest(entries)
    stale = sum(1 for entry in entries if entry.nearest != nearest.get(entry.id, []))
    write_nearest(entries, nearest)
    return stale


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate the SI.Mind feeling lexicon.")
    parser.add_argument(
        "--fix",
        action="store_true",
        help="rewrite every nearest list to the three nearest feelings by distance",
    )
    args = parser.parse_args()

    paths = lexicon_paths()
    if not paths:
        print(f"no lexicon files found under {DATA_DIR}", file=sys.stderr)
        return 1

    entries = parse_files(paths)
    problems = validate(entries)

    if args.fix:
        fixes = apply_nearest_fixes(entries)
        if fixes:
            print(f"repaired {fixes} nearest list(s)")
        entries = parse_files(paths)
        problems = validate(entries)

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
