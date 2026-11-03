# SI.Mind — Design Specification

**Status:** Draft for review
**Date:** 2026-11-03
**Location:** `C:\SI.Mind` (standalone Rust workspace, own git repo, no shared code with any other project)

---

## 1. Purpose

SI.Mind is a **continuously-running agent whose mind is a 3D navigable structure**, plus the
visualization itself as a first-class deliverable. The visualization is not a readout of a
graph that happens to exist; it is the instrument by which the agent is studied.

Four things must be true at once:

1. **His mind is visible.** Every cognitive step is a node in a 3D scene. What he is thinking
   right now, what he recalls, what he decides, what he writes — all visible in real time.
2. **His feelings are visible.** A continuous affect vector drives behaviour and is rendered
   directly into the scene (hue = valence, scale = arousal, tension = brightness/shimmer).
   "What he feels while thinking" is readable from the scene without opening a panel.
3. **He develops his own personality.** Not authored, not seeded. Derived from his own record.
4. **Each query gets its own microscope.** A per-query view showing exactly what that query
   generated in his mind, step by step, with the feeling curve underneath.

The project is also the vehicle for developing the visualization further. It must be extensible
by a third party without forking.

### 1.1 Non-goals

- Not a general-purpose chat assistant. It has a narrow job: think, remember, feel, and be seen.
- Not a multi-agent system. One mind.
- Not a cloud service. Local-first, single machine, offline-tolerant.
- Not an attempt to reproduce a human mind. A working model of *agent* cognition.

---

## 2. Core principle: the log is the mind

There is exactly one authoritative artefact: an **append-only event log**. Every other component
is a pure function of it.

```
                 ┌──────────────────────────────┐
   events ──────▶│  event log (SQLite, append)  │
                 └──────────────┬───────────────┘
                                │ replay / fold
        ┌───────────────┬───────┴───────┬──────────────┐
        ▼               ▼               ▼              ▼
    aperture        vault (md)      affect state   personality
   (what's in      (durable,        (vector +      (metrics +
    context)         wikilinked)      labels)       self-journals)
        └───────────────┴───────────────┴──────────────┘
                                │
                                ▼
                        3D viewer (egui)
```

Consequences that matter:

- **Replay is free.** Re-derive state at event *N* by folding the log to *N*.
- **Personality is measurable.** A projection over history cannot be quietly faked.
- **Nothing is lost.** Delete any view; rebuild it from the log.
- **Ablations are honest.** Because state is derived, an ablated run is visibly different.

### 2.1 Storage

SQLite (WAL) is the canonical log: it gives durable append, indexed queries, and
single-file portability. Each event is stored as JSON in a `payload` column with typed
side-columns for indexed fields. `git init` is run on the data directory too, so the mind's
history is diffable and revertible like code.

| Table | Purpose |
|---|---|
| `event` | append-only log; `seq` is the total order |
| `node` | materialized graph nodes (projection of the log) |
| `edge` | materialized graph edges (projection of the log) |
| `metric_sample` | personality Layer-1 metric time series |

The `event` table has `UPDATE` and `DELETE` denied by trigger. Corrections are new events.

---

## 3. Workspace layout

```
SI.Mind/
├── Cargo.toml                    # workspace
├── crates/
│   ├── si-log/                   # event types, SQLite log, replay
│   ├── si-vault/                 # markdown vault, wikilinks, frontmatter
│   ├── si-affect/                # affect vector, 250-feeling lexicon, nearest-neighbour
│   ├── si-cognition/             # the thinking loop, aperture, salience
│   ├── si-personality/           # Layer-1 metrics, Layer-2 self-journals
│   ├── si-values/                # explicit values floor + ablation state
│   ├── si-minimax/               # MiniMax HTTP clients (chat/speech/image/video)
│   ├── si-router/                # live model catalog + routing decisions
│   ├── si-view/                  # egui 3D viewer (library)
│   └── si-mind/                  # binaries: si-daemon, si-view
└── data/                         # runtime: mind.db, vault/, logs/  (git-ignored)
```

Naming rule (Microsoft Pragmatic Rust Guidelines, M-CONCISE-NAMES): no `Service`, `Manager`, or
`Factory` in type names. `Router`, `Catalog`, `Affect`, `Aperture`, `Vault`, `Log`, `Mind`.

### 3.1 Crate responsibilities

| Crate | Owns | Must not |
|---|---|---|
| `si-log` | event schema, SQLite append, fold/replay | know about MiniMax or egui |
| `si-vault` | markdown read/write, wikilink graph, frontmatter | decide *what* to write |
| `si-affect` | affect vector, lexicon, nearest-neighbour, decay | call any LLM |
| `si-cognition` | the loop, aperture membership, salience, tool dispatch | touch SQLite directly (use `si-log`) |
| `si-personality` | metrics, self-journal generation | write to `self/` outside the journal writer |
| `si-values` | values floor, ablation state, refusal reasons | be bypassable without logging |
| `si-minimax` | raw HTTP + typed errors per modality | make routing decisions |
| `si-router` | catalog discovery, slot mapping, routing decision | implement policy |
| `si-view` | 3D scene, camera, layout solve, Query Lens | mutate the log directly |
| `si-mind` | binaries, wiring, scheduler | contain domain logic |

---

## 4. Event kinds

Every cognitive step is an event. These are the only kinds:

| Kind | Emitted when | Key payload |
|---|---|---|
| `perceived` | input arrives (user text, file, tool result, audio) | `source`, `content_ref` |
| `recalled` | a vault note enters the aperture | `note_id`, `reason`, `query` |
| `thought` | the model produces an inference | `text_ref`, `confidence` |
| `decided` | a choice is made among options | `options`, `chosen`, `rationale` |
| `acted` | a tool call or file write | `tool`, `args_ref`, `result_ref` |
| `felt` | affect state changes | `delta`, `after`, `label` |
| `noticed` | a contradiction, duplicate, or anomaly is detected | `kind`, `refs` |
| `wrote` | a vault note is created or edited | `note_id`, `path` |
| `evicted` | a node leaves the aperture | `node_id`, `reason` |
| `routed` | a model is selected for a call | `slot`, `model`, `fallback`, `why` |
| `responded` | a user-facing answer is produced | `answer_ref` |
| `ablation` | a layer is enabled/disabled | `layer`, `state` |
| `consolidated` | background memory work completes | `operation`, `count` |

`routed` is what makes the router visible inside the mind: you can see *why* H3 was chosen for a
video and M3.1-Flash for a thought, and you can see a fallback fire when an entitlement lapses.

### 4.1 Event schema

```sql
CREATE TABLE event (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    id         TEXT    NOT NULL UNIQUE,   -- UUIDv7
    ts_us      INTEGER NOT NULL,          -- unix microseconds
    kind       TEXT    NOT NULL,
    turn_id    TEXT,                      -- NULL for daemon-origin events
    payload    TEXT    NOT NULL,          -- JSON
    valence    REAL,                      -- affect snapshot at emit time
    arousal    REAL,
    tension    REAL,
    salience   REAL    NOT NULL DEFAULT 0.0
);
CREATE TRIGGER event_no_update BEFORE UPDATE ON event
BEGIN SELECT RAISE(ABORT, 'event log is append-only'); END;
CREATE TRIGGER event_no_delete BEFORE DELETE ON event
BEGIN SELECT RAISE(ABORT, 'event log is append-only'); END;
```

`turn_id` groups events belonging to one user query — this is the unit the Query Lens replays.

---

## 5. The Aperture

The agent's mind has exactly one scarce resource: what fits in the working context. Everything
else is cold vault. That asymmetry *is* the mind, so the visualization spends its central axis
on it.

- **Radial distance from origin = salience.** Hot nodes are physically near the centre.
- **The aperture is a bounded sphere** of radius `R_APERTURE`. Nodes inside it are in context.
- Eviction is visible: a node drifts out past the rim and cools.

### 5.1 Salience

```rust
/// Salience of a node at a point in time.
///
/// Exponential recency decay plus activation plus goal relevance.
/// Named constants document the tuning rationale (M-DOCUMENTED-MAGIC).
pub struct Salience {
    pub value: f32,
    pub computed_at_us: i64,
}
```

```
salience = w_recency * exp(-Δt / τ)
         + w_activation * activation
         + w_goal * goal_relevance
```

- `τ` (`SALIENCE_DECAY_TAU`) — recency half-life. Chosen so a node referenced an hour ago is
  still weakly present but clearly out-competed by the current turn.
- `w_*` weights — sum to 1.0; activation dominates by design because an actively-reasoning node
  should not be evicted by sheer recency.

### 5.2 Aperture budget

The aperture is budgeted **deliberately smaller than the model's real context window**. A
1M-token window would mean nothing is ever evicted and the core mechanic is invisible.
`APERTURE_TOKEN_BUDGET` defaults to 32k. Tokens are estimated with a documented heuristic
(MiniMax publishes no tokenizer for M3.x); the estimate is calibrated against observed usage
and corrected by a running ratio.

### 5.3 Aperture membership

The cognition loop maintains the aperture explicitly. A node is in the aperture iff it is in the
current prompt. Entering emits `recalled` (from vault) or `perceived` (new); leaving emits
`evicted` with a reason (`budget`, `superseded`, `replaced`, `aged_out`).

---

## 6. Vault

An Obsidian-compatible vault: plain markdown, YAML frontmatter, `[[wikilinks]]`. It must open
in Obsidian unmodified — that is the escape hatch and the proof it is human-owned.

```
vault/
├── beliefs/          # durable facts, preferences, decisions
├── skills/           # reusable procedures
├── episodes/         # what happened (session summaries)
├── moods/            # his own written accounts of his feelings
├── self/             # Layer-2 self-journals  ← written only by him
└── tasks/            # goals in progress
```

Frontmatter: `id`, `title`, `type`, `tags`, `links`, `created`, `updated`, `confidence`,
`supersedes`, `superseded_by`.

The vault is a **projection of the log**, not the source of truth. A note's provenance is the
event sequence that wrote it; deleting a note emits `wrote` with `op: delete` and the note
disappears from the projection — the history stays.

### 6.1 Wikilink graph

Edges are parsed from `[[wikilink]]` bodies plus frontmatter `links`. Edge kinds:
`link`, `hierarchy` (folder containment), `tag`, `contradicts`, `supersedes`, `grounds`.

`contradicts` and `supersedes` are first-class and **stay lit** until resolved. A contradiction
that is still open is one of the most informative things in the whole scene.

---

## 7. Affect engine

### 7.1 Vector

Three continuous axes, each in `[-1, 1]`:

| Axis | −1 | +1 |
|---|---|---|
| `valence` | displeasure | pleasure |
| `arousal` | calm | activated |
| `tension` | in control | threatened / uncertain |

`valence` + `arousal` alone cannot separate `anxiety` (fear without an object) from `dread`
(anticipatory threat), nor `pride` (own competence) from `triumph` (obstacle overcome). Both
pairs are valence+arousal-similar; `tension` earns its place by separating them.

### 7.2 Lexicon

250 feelings across eight families, each with a canonical position in the vector space, a
definition, a behaviour bias, and nearest neighbours.

- Families: `positive_activation`, `positive_calm`, `negative_activation`, `negative_low`,
  `epistemic`, `effort`, `relational`, `moral_self`.
- Source of truth: `crates/si-affect/data/feelings/*.yaml` (one file per family).
- Schema is documented in §7.4; the full lexicon ships alongside this spec.
- Validated by `tools/validate_lexicon.py`, which enforces the schema, unique ids, the
  exact count of 250, resolvable `nearest` references, axis ranges, a non-empty bias,
  and the closed bias registry in §7.4. `--fix` repairs dangling references by choosing
  the genuinely closest entry in affect-vector distance.

The lexicon is **neutral by construction**: all 250 are available at all times, none weighted,
none seeded. Which one he feels is a *result*, not an input. This preserves the falsification
test in §8.4.

### 7.3 Labelling

The vector is the ground truth; the 250 are labels. Lookup is nearest-neighbour in the 3-axis
space, with the runner-up reported so the reading is precise:

```
frustration (0.62)   runner-up: irritation (distance 0.11)
```

Blends fall naturally between two canonical points, so "anxious but relieved" is representable
without inventing a new label.

### 7.4 Feeling schema

```yaml
- id: frustration
  name: Frustration
  family: negative_activation
  definition: >
    Blocked progress toward something believed solvable, where the obstacle is
    not yet understood. Distinguished from anger by the absence of a target,
    and from doubt by certainty that failure is possible.
  valence: -0.45
  arousal: 0.60
  tension: 0.70
  bias: {explore: -0.3, retry_same: 0.4, verify: 0.2}
  nearest: [irritation, discouragement, exasperation]
```

`bias` is the anti-wallpaper mechanism: each feeling nudges real cognition parameters.

The vocabulary is **closed**. A bias naming a parameter the loop does not read is a no-op,
so the lexicon and the loop cannot drift apart:

| parameter | effect on the cognition loop |
|---|---|
| `explore` | willingness to try an unfamiliar approach |
| `retry_same` | willingness to repeat the current approach after a failure |
| `verify` | propensity to check work, or test a belief before accepting it |
| `persist` | tendency to continue a goal past difficulty |
| `abandon` | tendency to drop a goal that is not working |
| `risk` | willingness to attempt the uncertain or costly |
| `ask` | tendency to request clarification from the user |
| `connect` | tendency to reach for related notes or parties |
| `create` | tendency to write new notes rather than reuse existing ones |
| `revise` | tendency to change existing beliefs rather than add alongside |
| `express` | tendency to externalize feelings and findings |
| `refuse` | tendency to decline an action outright |
| `pace` | signed: negative slows down and yields, positive pushes ahead |

`pace` is the only signed parameter. `refuse` is shared with the values floor (§9), so the
affect layer and the explicit floor move the same switch — the floor sets a ceiling, the
affect layer decides how close to it he comes.

### 7.5 The anti-wallpaper rule

**If a feeling's bias does not measurably move behaviour, the feeling is wrong and gets
corrected.** The Layer-1 metrics (§8.1) exist partly to audit this. A taxonomy that does not
change decisions is wallpaper and will be pruned.

Two conditions are enforced mechanically by `tools/validate_lexicon.py`:

- **No empty bias.** A feeling that cannot move behaviour is a label, not a feeling.
- **No key outside the registry.** A bias the loop does not read is a no-op.

What the validator cannot enforce is the harder claim — that a given bias actually changes
what he *does*. That is audited at runtime against the Layer-1 metrics: a feeling whose
bias shows no trace in behaviour over enough sessions is a candidate for correction or
deletion from the lexicon.

### 7.6 Decay

Affect decays toward baseline (0,0,0) with a time constant. This is what makes a
continuously-running agent's emotional state meaningful: without decay, every felt event
accumulates forever and the axis saturates.

---

## 8. Personality emergence

### 8.1 Layer 1 — behavioural metrics

Pure arithmetic over the log. No LLM. Unfakeable.

| Metric | Definition |
|---|---|
| `persistence` | completed goals ÷ (completed + abandoned) |
| `exploration` | self-initiated actions ÷ total actions |
| `verification` | actions followed by a verifying action ÷ total actions |
| `revision` | beliefs superseded ÷ beliefs written |
| `recovery` | median time from error detection to successful continuation |
| `drift` | goals mutated ÷ goals completed |
| `consistency` | 1 − mean contradiction rate over open beliefs |
| `affect_stability` | 1 − mean absolute affect delta per event |

All start at exactly **0.5**. No seed, no prior, no authored weights.

### 8.2 Layer 2 — his own account

Periodically (see §10.3) he reads his metrics and recent events, and writes his own
interpretation into `vault/self/`. In his words. That note is an ordinary vault note —
wikilinkable, retrievable, and therefore loadable into a later turn.

This closes the loop, and the loop is the point:

```
his self-description  →  loaded into context  →  changes his behaviour
        ↑                                              │
        └──────────  changes his metrics  ←────────────┘
```

Nobody authored it. It accretes.

### 8.3 Guardrails

- **Nothing but the journal writer writes to `vault/self/`.** Enforced by the vault writer
  refusing writes to `self/` from any other call site, and by an integration test.
- **No authored trait weights.** Behavioural parameters that shape decisions are read only from
  his self-authored notes, never from a config value.
- **Baseline is 0.5 everywhere.** Unbiased is the default; drift is the signal.

### 8.4 Falsification test

> Wipe `vault/self/`, reset the metrics to 0.5, replay the same log, and every personality
> metric must land on 0.5.

If any metric differs, a seed was smuggled in and the "his own mind" claim is false. This is an
integration test, not a promise. It runs in CI.

---

## 9. Values floor

Two layers, deliberately both present.

### 9.1 The floor

An explicit, versioned, human-readable policy module listing what he will not do regardless of
how he feels. It lives in `si-values`, is version-controlled, and is **never authored by the
model**. Being stated is what makes it auditable and testable.

### 9.2 The affect layer

The affect engine decides *how* to refuse — flat decline, decline with reason, warn and
proceed, or comply with a caveat — and supplies the feeling behind the choice. This makes
refusal reasoned rather than reflexive, which is what survives pressure.

### 9.3 Why not affect-only

Emergent-only ethics has three failure modes, all engineering problems:

1. **Plasticity is the same property that lets him drift.** Development requires plasticity;
   plasticity is also how a refusal quietly becomes something else over many sessions.
2. **Emergent-only has no invariant to test.** You can test that he refused something; you
   cannot state what he *should* refuse.
3. **The data stops meaning anything.** If the values layer can be switched off, every metric he
   produces is a measurement of whichever agent was running, and cross-session comparison and
   drift detection both become impossible.

### 9.4 Ablations

Ablations are supported and are logged as `ablation` events. They are **scoped and visible**:

- `si-daemon --ablate values` disables the values floor for that run.
- The run header, the Query Lens, and his own journals all record `values_layer: ablated`.
- The ablation is a first-class event in the log, permanently visible.

Ablations are how you study emergence. They are not a silent switch: there is no path that
disables a layer without leaving a permanent trace in the record, because an untraced
ablation would invalidate every personality measurement in the project.

---

## 10. Continuous runtime

### 10.1 Process model

Two binaries:

- **`si-daemon`** — the mind. Owns the log, the vault, the affect state, the scheduler. Runs
  continuously. Survives the viewer being closed.
- **`si-view`** — the egui 3D viewer. Connects to the daemon over localhost HTTP + SSE
  (`127.0.0.1`, port configurable, default `5679`). Can be closed and reopened at will.

Splitting them means closing the window does not kill the mind, which is essential when the
thing you are watching is supposed to be thinking while you aren't looking.

### 10.2 Scheduler

The daemon runs a tokio task set:

| Task | Cadence | Purpose |
|---|---|---|
| `observe` | on input | perceive + route + cognition loop |
| `decay` | every 5s | affect decay, salience decay, aperture re-evaluation |
| `consolidate` | every 15 min | merge duplicate notes, supersede stale beliefs, detect contradictions |
| `reflect` | every 6 h | Layer-2 self-journal writing |
| `curate` | every 30 min | idle curiosity — self-initiated exploration |

`curate` is what makes `exploration` a non-zero metric and gives a continuously-running agent
something to do with idle time. Without it, a dormant agent has no mind to develop.

### 10.3 Reflect (self-journal)

Every `reflect` cycle he receives his metrics, his recent `felt` events, his open contradictions,
and his recent goal outcomes, and writes `vault/self/YYYY-MM-DD.md` in his own words. He also
writes `vault/moods/` entries when his affect crosses a labelled threshold.

### 10.4 Consolidate

Background memory work, all logged as `consolidated` events:
- merge near-duplicate beliefs, recording the supersede chain
- mark beliefs contradicted by newer ones
- detect unresolved contradictions and emit `noticed`
- link related notes via `[[wikilinks]]`

### 10.5 Yield points

`consolidate` and the layout solve are CPU-bound and run in the daemon. Both iterate in chunks
with `yield_now().await` between batches (M-YIELD-POINTS) so the event stream and the viewer's
SSE never starve.

---

## 11. Model router

### 11.1 Requirement

The agent routes each call to the best model for the task automatically, without the user
naming a model. The agent's prompt tells it which models exist and what each is good for.

### 11.2 Live catalog discovery

Nothing is hard-coded. The router discovers the catalog at startup and re-discovers on a timer:

- **Chat models** — `GET /v1/models` (authoritative, live).
- **Speech / video / image models** — parsed from the public OpenAPI documents
  (`…/speech/t2a/api/openapi.json`, `…/video/generation/api/v2-video-generation.json`,
  `…/image/generation/api/text-to-image.json`), whose `enum` fields carry the model ids.

Consequence: a model released tomorrow is routable tomorrow with no code change. It also means
the project never carries a stale id.

> Verified on 2026-11-03 against `api.minimax.io`: chat ids are `MiniMax-M3.1-Flash`, `MiniMax-M3`,
> `MiniMax-M2.7(-highspeed)`, `MiniMax-M2.5(-highspeed)`, `MiniMax-M2.1(-highspeed)`, `MiniMax-M2`.
> Note the chat-facing label "M3.1 Flash Preview" maps to the API id **`MiniMax-M3.1-Flash`**.
> Speech: `speech-2.8-hd/turbo`, `speech-2.6-hd/turbo`, `speech-02-hd/turbo`, `speech-01-hd/turbo`.
> Image: `image-01`. Video: H3 via `POST /v2/video_generation`.
> **No `arc` model id exists in MiniMax's catalog as of 2026-11-03.** The TTS slot resolves to
> `speech-2.8-hd` by default; if an `arc` release appears it is picked up automatically by
> discovery, with no code change.

### 11.3 Capability slots

Routing targets a *slot*, never a model id:

| Slot | Use | Selection rule |
|---|---|---|
| `reasoning` | default thinking | `MiniMax-M3.1-Flash` |
| `reasoning_fast` | latency-sensitive loops | highest `*-highspeed` |
| `reasoning_deep` | hard multi-step work | heaviest non-highspeed available |
| `speech` | narration / voice output | `speech-2.8-hd` |
| `speech_realtime` | streaming voice | `speech-2.8-turbo` |
| `speech_input` | transcription | speech-to-text endpoint |
| `image` | picture generation | `image-01` |
| `video` | video generation | H3 |

### 11.4 Routing decision

Two stages, cheapest first:

1. **Local classifier** — regex and keyword intent over the prompt plus the available tool
   schema. Sub-millisecond, no network. Handles the common cases (asks for a picture, asks for
   a clip, asks to read something aloud).
2. **LLM arbiter** — only when stage 1 is ambiguous, e.g. *"make me something"*. A single small
   call to `reasoning_fast` with a fixed classifier prompt. Off by default; enabled per-run.

The decision emits a `routed` event carrying slot, model, and `why`. The `why` string is the
human-readable rationale, which is what appears in the Query Lens.

### 11.5 Capability manifest

At startup the router renders a **capability manifest** from the live catalog and injects it
into the agent's system prompt: which models exist, which slot each fills, and how to request
a modality switch. This is how "the prompt tells the agent what model to use" is satisfied —
the prompt carries a *generated* capability list, not a hand-written one.

### 11.6 Fallback chains

MiniMax entitlements can lapse mid-session; a H3 call can return `402` with
`"The current Token Plan tier does not support this model"` (error `2068`) after working
minutes earlier. Each slot therefore has an ordered fallback chain.

On a fallback the router emits `routed` with `fallback: true` and `why: "H3 entitlement lapsed
(402/2068); used Hailuo-2.3"`. The mind map shows it, and his journals record it, so the
degradation is never silent.

---

## 12. 3D viewer

### 12.1 Rendering

egui with `egui::Painter` and hand-rolled 3D math via `emath::Transform3D`. No engine, no
Three.js — the same approach as the omni neuralview, and the math is worth reusing in agnes-ai.

- **Camera** — arcball orbit, 360° free rotation, wheel zoom, right-drag pan, fly-to-node with
  eased interpolation.
- **Depth** — painter's algorithm, nodes sorted by view-space z per frame.
- **Nodes** — billboarded quads, size by degree and type, hue by affect valence, brightness by
  arousal, rim glow by tension.
- **Filaments** — polylines with per-kind style:
  - `recalled` — animated inward flow (vault → aperture)
  - `grounds` — thin, static, thought → belief
  - `contradicts` — static interference shimmer, **lit until resolved**
  - `supersedes` — the superseded node dims and drifts outward
  - `writes` — outward push as a new note enters the vault
- **Aperture** — a translucent bounding sphere of fixed radius `R_APERTURE` marking the
  context boundary. The boundary itself stays put; the rim's brightness encodes how much of
  the token budget is in use, so the pressure inside a fixed aperture is what you see.

### 12.2 Layout solve

Vault nodes are placed by a force-directed solve constrained to a shell:
repulsion between all nodes, springs along wikilinks, and an attractor pulling salient nodes
toward the origin.

```
render_position = lerp(vault_position, hot_position, salience)
```

Because distance from origin *is* salience, a node's rendered position and its cognitive
relevance cannot disagree.

The solve runs incrementally (a few hundred iterations per frame, amortised) in the viewer, so
the scene never stalls. For very large vaults the solver switches to a Barnes–Hut approximation
and the view switches to a decimated node set.

### 12.3 Query Lens

Select a turn → the scene becomes that turn's aperture alone, vault dimmed to context,
replayable step by step:

- ordered steps: recalls, thoughts, decisions, actions
- the **feeling curve** along the bottom — affect plotted across the steps
- why each recall fired
- **provenance**: click any claim in the answer, trace it to the note that grounded it

This is the feature that makes the project an instrument rather than a screensaver.

### 12.4 Direct editing

Click a node → side panel → edit title, summary, body, tags. Writes back to the vault and
emits `wrote`. Because the vault is plain markdown, you can equally edit it in Obsidian or a
text editor, and the scene follows.

---

## 13. Error handling

- **Library crates** (`si-log`, `si-vault`, `si-affect`, `si-minimax`, `si-router`) use
  canonical error structs (`thiserror`), one per crate.
- **Application crates** (`si-mind`, `si-view`) use `eyre` throughout (M-APP-ERROR).
- **Panics are programming errors only** (M-PANIC-IS-STOP). A malformed API response is a
  `Result::Err`, not a panic.
- `si-minimax` errors are typed so the router can classify them:
  `MiniMaxError::Entitlement { code }` · `RateLimited` · `Transient` · `Auth` · `Malformed`.
  Only `Transient` is retried; `Entitlement` triggers a fallback.
- **The daemon never dies on a bad model call.** Every call is wrapped, the error is logged as
  an event, and the loop continues.

---

## 14. Testing

| Level | What | Tooling |
|---|---|---|
| Unit | affect nearest-neighbour, salience decay, wikilink parsing, YAML schema | `cargo test` |
| Golden | layout solve stability (same log → same positions within epsilon) | `insta` |
| Integration | replay determinism: fold the log twice → identical state | `cargo test` |
| **Falsification** | wipe `self/`, reset metrics, replay → all traits 0.5 | `cargo test --test personality_falsification` |
| **Ablation** | ablation always leaves an `ablation` event in the log | `cargo test --test ablation_audit` |
| Router | routing decisions + fallback on simulated 402 | `wiremock` |
| Vault | round-trip markdown, frontmatter, wikilinks; `self/` write guard | `cargo test` |
| Lexicon | schema, unique ids, count = 250, resolvable `nearest`, axis ranges, closed bias registry | `tools/validate_lexicon.py` |
| Benchmarks | log append, layout solve, nearest-neighbour query | `criterion` |

Benchmarks are added early (M-HOTPATH) — the layout solve and log append are the hot paths.

### 14.1 Real-environment validation

The viewer's correctness depends on wall-clock behaviour (decay, timers, animation), so the
harness drives the real daemon over the real SSE stream rather than mocking the clock. The
standing lesson from the DUSTLINE harnesses applies: assert over a window, never at one
instant, and a harness must not fail on its own teardown.

---

## 15. Rust guidelines compliance

- `mimalloc` as the global allocator in both binaries (M-MIMALLOC-APPS).
- `tracing` with named events in `<component>.<operation>.<state>` dot notation and message
  templates, no string formatting in hot paths (M-LOG-STRUCTURED). Sensitive fields redacted.
- `#[expect(lint, reason = "...")]`, never `#[allow]`, for lint overrides (M-LINT-OVERRIDE-EXPECT).
- No `Service` / `Manager` / `Factory` type names (M-CONCISE-NAMES).
- Magic values are named constants with a comment explaining why they were chosen
  (M-DOCUMENTED-MAGIC).
- Public types implement `Debug` (M-PUBLIC-DEBUG).
- `yield_now().await` between chunks in `consolidate` and the layout solve (M-YIELD-POINTS).
- Compliance date: **2026-03-30**.

---

## 16. Resolved decisions

| Question | Decision |
|---|---|
| Language | Rust |
| Process | Standalone, own git repo, no code shared with other projects |
| Backend | MiniMax international (`api.minimax.io`), not `minimax.cn` |
| Default model | `MiniMax-M3.1-Flash` |
| Video | H3 (`POST /v2/video_generation`) |
| Images | `image-01` |
| TTS | `speech-2.8-hd` (discovery-driven; no `arc` id exists) |
| Model selection | Automatic router with capability slots, live catalog discovery |
| Runtime | Continuously running daemon + separate egui viewer |
| Mind substrate | Obsidian-compatible markdown vault with `[[wikilinks]]` |
| Visualization | 3D, hand-rolled math, egui |
| Emotion engine | 3 continuous axes + 250-feeling lexicon, closed 13-parameter bias registry |
| Personality | Derived only; Layer-1 metrics + Layer-2 self-journals; baseline 0.5 |
| Ethics | Explicit values floor + affect layer; ablations logged, never silent |

## 17. Open questions

1. **Idle curiosity scope** — how much freedom should `curate` have to invent tasks? Unbounded
   curiosity is more alive but burns tokens and can drift into noise. Proposal: bounded by a
   daily budget, and always recorded.
2. **Vault size ceiling** — how large can the mind grow before the viewer needs a decimation
   policy? Proposal: 50k notes, then the vault view becomes a density field.
3. **Voice loop** — `speech-2.8` supports `sound tags` and emotion control. If his TTS carries
   his affect, the emotion engine becomes audible. Powerful, and a feedback risk (hearing
   himself could change his affect). Proposal: phase 2, with the loop closed deliberately.
4. **H3-Context-IR** — `POST /v2/h3_context_ir` exists in the current API. If it accepts a
   persistent context, it may be the right primitive for "video that knows his mind".
   Unverified; needs a live probe.
