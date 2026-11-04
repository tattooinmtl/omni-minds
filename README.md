# SI.Mind

A continuously-running agent whose mind is a 3D navigable structure, and the
visualization that makes it visible.

The visualization is not a readout of a graph that happens to exist. It is the
instrument by which the agent is studied: you watch him think, watch him feel,
watch him write things down, and watch him become a personality nobody authored.

---

## The one principle

There is exactly one authoritative artefact: an **append-only event log**.
Every other component — the attention aperture, the markdown vault, the affect
state, the personality projection, the model router, the plan, the 3D scene —
is a pure function of it.

```
  events ──▶ event log (SQLite, append-only)
                 │
      ┌──────────┼───────────┬─────────────┐
      ▼          ▼           ▼             ▼
   aperture    vault      affect       personality
 (what's in   (markdown,   (vector +   (metrics +
  context)     wikilinks)   labels)     self-journals)
      └──────────┴───────────┴─────────────┘
                     │
                     ▼
              3D viewer (egui)
```

Replay is free. Personality is measurable. Nothing is lost. Ablations are honest.

---

## What it is not

- Not a chat assistant. It has a narrow job: think, remember, feel, and be seen.
- Not a multi-agent system. One mind.
- Not a cloud service. Local-first, single machine.
- Not a reproduction of a human mind. A working model of *agent* cognition.

---

## Layout

```
crates/
  si-log/          event types, SQLite log, replay
  si-vault/        markdown vault, wikilinks, frontmatter, OKF cards
  si-affect/       affect vector, 250-feeling lexicon, nearest-neighbour
  si-cognition/    the thinking loop, aperture, salience, seams
  si-personality/  behavioural metrics, self-journals
  si-values/       explicit values floor + ablation state
  si-provider/     provider registry, model catalog, credentials
  si-router/       capability slots + routing decisions
  si-minimax/      MiniMax HTTP clients (chat/speech/image/video)
  si-settings/     typed setting schema, defaults, reset
  si-overlay/      loop overlay stack + generator
  si-plan/         brainstorm/plan modes, lineage, timestamps
  si-view/         egui 3D viewer
  si-mind/         binaries: si-daemon, si-view
docs/superpowers/specs/
  2026-11-03-si-mind-design.md    the specification of record
crates/si-affect/data/feelings/   the 250-feeling lexicon (8 family files)
tools/
  validate_lexicon.py             lexicon schema/count/dup validator
```

---

## Providers

Six, all OpenAI-compatible at the chat path. Endpoints and catalogs verified
live on 2026-11-03.

| provider | base URL | models | notes |
|---|---|---|---|
| `minimax.io` | `api.minimax.io/v1` | 9 chat | + separate TTS / image / video APIs |
| `nvidia` | `integrate.api.nvidia.com/v1` | 85 | free tier; has `minimaxai/minimax-m3.1-flash` |
| `xkiro` | `api.xkiro.com/v1` | 103 | **catalog is open — no key needed**; 45 free |
| `atria` | `api.atria-asi.ai/v1` | key-gated | `Atria-Dawn-Preview` |
| `reflection` | `api.reflection.ai/openai/v1` | key-gated | `Beam-1.0-501B-A23B`; needs `reasoning_content` echoed back |
| `agnes` | `apihub.agnes-ai.com/v1` | 9 | includes **image** and **video** models |

Saving a key immediately fetches that provider's catalog and populates the model
dropdown. Catalogs are cached, so the dropdown works offline and a failed fetch
never loses the credential or blocks the loop.

Routing targets a **capability slot** (`reasoning`, `reasoning_fast`,
`reasoning_deep`, `speech`, `speech_realtime`, `speech_input`, `image`,
`video`, `overlay_author`), never a model id — and every decision is an event,
so the mind map shows *why* a model was chosen.

---

## Settings and the loop

Every knob the code actually reads is exposed, grouped into panels, and the UI
is **generated from a typed schema** — adding a setting is one field plus one
schema entry. Reset restores defaults per panel or globally; reset never
touches credentials, the log, the vault, or the personality baseline.

**Loop overlays.** The agentic loop is fixed Rust control flow and never changes
shape at runtime. What you modify is an **overlay**: a directive layered onto
the loop at named seams (pre-turn, post-turn, pre-tool, post-tool, termination,
retrieval). Overlays stack rather than replace, so "write on top" accumulates.

You pick a provider, then a model, then say what you want the loop to do (or
which skill to fold in). An AI call writes the directive, it is validated, and
it becomes part of every subsequent turn — with its author, model and
instruction visible in the mind map.

No generated code runs in the daemon. Overlays are text, so they are
inspectable and diffable, and they **cannot widen the values floor** — the
floor is enforced in code, at the point an action is about to be taken.

---

## The feeling lexicon

250 feelings across eight families, each with a canonical position in a
three-axis affect space:

| axis | −1 | +1 |
|---|---|---|
| `valence` | displeasure | pleasure |
| `arousal` | calm | activated |
| `tension` | in control | threatened / uncertain |

The vector is the ground truth; the 250 are labels. Lookup returns the nearest
feeling **and its runner-up**, so a reading is precise rather than a bare name.

```
frustration (0.62)   runner-up: irritation (distance 0.11)
```

`tension` is not decoration: `anxiety` (fear without an object) and `dread`
(anticipatory threat) are valence+arousal-similar and cannot be separated
without it. Neither can `pride` (own competence) and `triumph` (obstacle
overcome).

Each feeling carries a `bias` map onto a **closed registry of 13 cognition
parameters** the loop actually reads (`explore`, `retry_same`, `verify`,
`persist`, `abandon`, `risk`, `ask`, `connect`, `create`, `revise`, `express`,
`refuse`, `pace`). A bias key outside that registry would be a no-op, so the
validator rejects it — and a feeling with an empty bias is rejected too,
because a feeling that cannot move behaviour is a label, not a feeling.

The lexicon is **neutral by construction** — all 250 available at all times,
none weighted, none seeded. Which one he feels is a result, not an input.

```bash
python tools/validate_lexicon.py          # check schema, dupes, count, refs
python tools/validate_lexicon.py --fix    # repair dangling `nearest` refs
```

---

## Personality is derived, never seeded

The rule: **if I could type it into a config, it isn't his.**

Layer 1 — behavioural metrics, pure arithmetic over the log, no LLM, unfakeable:
`persistence`, `exploration`, `verification`, `revision`, `recovery`, `drift`,
`consistency`, `affect_stability`. All start at exactly **0.5**.

Layer 2 — he reads his own metrics and recent events and writes his
interpretation into `vault/self/`, in his words. That note is an ordinary vault
note, so it is retrievable, so it changes his behaviour, which changes his
metrics. Nobody authored it; it accretes.

**The falsification test:** wipe `vault/self/`, reset the metrics, replay the same
log — every trait must land on 0.5. If any metric differs, a seed was smuggled in
and the claim is false. This runs as an integration test.

---

## Plan, brainstorm, and compaction

The agent **always writes a plan**, in two states:

- **Brainstorm** — a real gate. He may think, read, recall, write notes and
  maintain the plan, but may not execute plan steps or mutate anything outside
  the plan and the vault. The gate is enforced in code, and a refused action is
  logged so a mis-set mode is diagnosable.
- **Plan** — he executes.

Every plan item records its **origin**: the exact passage of the brainstorm that
produced it, with a timestamp and an event sequence number. An idea said in the
first minute of a session is still reachable as the origin of a step executed in
the fortieth.

**Only the aperture compacts — the log never does.** Before compacting, the
agent writes a *resumer* into a dedicated section of `vault/context_history.md`,
in his own words, and loads it back. Because the log survives, returning to any
past moment is exact rather than approximated: select it and the whole mind
reconstructs by folding the log to that sequence number.

---

## OKF indexing

Everything is indexed into one **Open Knowledge Format** tree in the vault —
plans, brainstorms, resumers, journals, moods, skills, overlays, provider
metadata, `agents.md`. One tree means one retrieval mechanism: the agent reaches
any of it through the same `[[wikilink]]` traversal it uses for everything else.

Each card carries `source_events` linking it back to the event range that
produced it, which is what keeps the tree a *projection* of the log rather than
a parallel source of truth. It also means the mind map's graph and the graph the
agent reasons over are the same graph.

---

## Status

Design phase, revision 2. The specification of record is
[`docs/superpowers/specs/2026-11-03-si-mind-design.md`](docs/superpowers/specs/2026-11-03-si-mind-design.md).
Implementation has not started.

---

## Conventions

Rust, following the Microsoft Pragmatic Rust Guidelines (compliance date
2026-03-30). No `Service` / `Manager` / `Factory` type names. `eyre` in
application crates, canonical error structs in library crates. `mimalloc` as the
global allocator. `tracing` with named events and message templates.
`#[expect(lint, reason = "...")]` for lint overrides. Yield points in
long-running loops.
