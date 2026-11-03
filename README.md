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
state, the personality projection, the 3D scene — is a pure function of it.

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
  si-vault/        markdown vault, wikilinks, frontmatter
  si-affect/       affect vector, 250-feeling lexicon, nearest-neighbour
  si-cognition/    the thinking loop, aperture, salience
  si-personality/  behavioural metrics, self-journals
  si-values/       explicit values floor + ablation state
  si-minimax/      MiniMax HTTP clients (chat/speech/image/video)
  si-router/       live model catalog + routing decisions
  si-view/         egui 3D viewer
  si-mind/         binaries: si-daemon, si-view
docs/superpowers/specs/
  2026-11-03-si-mind-design.md    the specification of record
crates/si-affect/data/feelings/   the 250-feeling lexicon (8 family files)
tools/
  validate_lexicon.py             lexicon schema/count/dup validator
```

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

## Model routing

The router selects the best model per task automatically. It discovers the
MiniMax catalog live rather than hard-coding it, so a model released tomorrow is
routable tomorrow.

- chat models — `GET /v1/models` (authoritative)
- speech / video / image — parsed from the public OpenAPI documents

Default chat model is `MiniMax-M3.1-Flash`. Video is H3 (`POST /v2/video_generation`).
Images are `image-01`. Speech is the `speech-2.8` family.

Routing targets a **capability slot**, never a model id, and every decision emits
a `routed` event carrying the reason — so the mind map shows *why* H3 was chosen
for a video, and shows a fallback firing when an entitlement lapses.

---

## Status

Design phase. The specification of record is
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
