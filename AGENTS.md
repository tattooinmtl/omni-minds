# SI.Mind — working agreements for agents and contributors

This file is the project-level memory. Anything here is a rule that must hold
across every change to this repository.

## What this project is

A continuously-running agent whose mind is a 3D navigable structure, plus the
visualization that makes it visible. The specification of record is
`docs/superpowers/specs/2026-11-03-si-mind-design.md`. Read it before changing
anything structural.

This project is **standalone**. It shares no code, no crates and no configuration
with any other repository. Reference other projects for ideas only; never import
from them.

## The invariants that are not negotiable

These are the project's core claims. Breaking any of them makes the whole thing
false, so they are enforced by tests rather than by good intentions.

1. **The event log is append-only.** `UPDATE` and `DELETE` on the `event` table
   are denied by trigger. Corrections are new events, never edits.
2. **The log is the only source of truth.** The aperture, the vault, the affect
   state, the personality projection and the 3D scene are all derived. If you
   find yourself storing derived state authoritatively, that is the bug.
3. **Nothing writes to `vault/self/` except the journal writer.** `vault/self/`
   holds his own account of himself. A seed written there would make his
   personality ours. Deleting a journal is equally forbidden. The guard matches
   the folder the way the filesystem does, not the way the id is spelled — on
   Windows `Self/`, `SELF/` and `self./` all name the same folder, so a guard
   that compared the raw string would let a seed in through the back door.
4. **All personality traits start at exactly 0.5.** No seed, no prior, no
   authored weights. Unbiased is the default.
5. **The feeling lexicon is neutral.** All 250 feelings are available at all
   times; none is weighted toward. Which one he feels is a result.
6. **The bias registry is closed.** Biases may only name the 13 cognition
   parameters the loop actually reads: `explore`, `retry_same`, `verify`,
   `persist`, `abandon`, `risk`, `ask`, `connect`, `create`, `revise`,
   `express`, `refuse`, `pace`. A bias key outside that set is a no-op and is
   rejected. A feeling with an empty bias is rejected — a feeling that cannot
   move behaviour is a label, not a feeling.
7. **Ablations are always logged.** Disabling the values layer or any other
   layer emits an `ablation` event and is visible in the run header, the Query
   Lens, and his journals. There is no path that disables a layer silently —
   an untraced ablation would invalidate every personality measurement.
8. **The values floor is explicit, versioned, and never model-authored.** See
   `si-values`.
9. **Overlays are text and cannot widen the floor.** A loop overlay is a
   prompt-level directive at named seams. It may narrow behaviour; it may
   never grant permission the code-enforced floor denies. An overlay that
   appears to do so is refused by the floor, and the refusal is logged.
10. **The loop's shape is fixed.** The agentic loop is Rust control flow and
    never changes shape at runtime. Only overlays layer onto it, and overlays
    stack rather than replace.
11. **Only the aperture compacts.** The event log is never compacted, so
    returning to a past moment is exact. A compaction writes a resumer into
    `context_history.md` first, in the agent's own words.
12. **Brainstorm mode gates execution.** He may think, read, recall and write
    notes; he may not execute plan steps or mutate anything outside the plan
    and the vault. The gate is enforced in code and refusals are logged.
13. **Every plan item has an origin.** Each item records the brainstorm
    passage, timestamp and event sequence that produced it, so a step can be
    traced back to the idea it came from.
14. **Provider keys are never in the log and never in a setting value.** They
    live in the `provider_credential` table, are masked in the UI, and are read
    from the environment first so a shell variable overrides a saved key.

## The falsification test

`cargo test --test personality_falsification` must pass. It wipes `vault/self/`,
resets the metrics, replays the same log, and asserts every trait lands on 0.5.
If it fails, a seed was smuggled in. Treat that failure as the most serious
signal in the project.

Two more tests guard the rest of the design:

- `cargo test --test ablation_audit` — every layer change leaves an `ablation`
  event.
- `cargo test --test plan_lineage` — every plan item resolves to an origin with
  a quote, and every gated action is refused and logged.

## Commands

```bash
# lexicon schema, duplicate id, count and reference validation
python tools/validate_lexicon.py

# rewrite every `nearest` list that disagrees with the rule: the three closest
# feelings by Euclidean distance over the three axes
python tools/validate_lexicon.py --fix

# regenerate the `nearest` lists themselves; reports "no change" when they are
# already correct, and is what `--fix` uses
python tools/regenerate_nearest.py
```

## Rust conventions

Microsoft Pragmatic Rust Guidelines, compliance date **2026-03-30**.

- No `Service`, `Manager` or `Factory` in type names (M-CONCISE-NAMES).
- `eyre`/`anyhow` in application crates; canonical error structs in library
  crates (M-APP-ERROR).
- `mimalloc` as the global allocator in both binaries (M-MIMALLOC-APPS).
- `tracing` with named events in `<component>.<operation>.<state>` dot notation
  and message templates; no string formatting in hot paths; redact sensitive
  fields (M-LOG-STRUCTURED).
- `#[expect(lint, reason = "...")]`, never `#[allow]`, for lint overrides
  (M-LINT-OVERRIDE-EXPECT).
- Magic values become named constants with a comment explaining why the value
  was chosen and what breaks if it changes (M-DOCUMENTED-MAGIC).
- Public types implement `Debug` (M-PUBLIC-DEBUG).
- Panics signal programming errors only; real failures are `Result::Err`
  (M-PANIC-IS-STOP).
- Long-running loops yield between chunks (M-YIELD-POINTS).
- Identify hot paths early and benchmark them with `criterion` (M-HOTPATH).

## Backend

MiniMax **international** (`api.minimax.io`), never `minimax.cn`. The API key
comes from the environment (`MINIMAX_API_KEY`) and must never be committed.

## Testing philosophy

Test the real thing. The viewer's correctness depends on wall-clock behaviour
(decay, timers, animation), so harnesses drive the real daemon over the real
event stream rather than mocking the clock. Assert over a window rather than at
a single instant, and a harness must not fail on its own teardown.
