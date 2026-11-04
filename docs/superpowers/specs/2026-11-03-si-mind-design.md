# SI.Mind — Design Specification

**Status:** Draft for review (rev. 2 — multi-provider, settings, overlays, plan modes, compaction, OKF)
**Date:** 2026-11-03
**Location:** `C:\SI.Mind` (standalone Rust workspace, own git repo, no shared code with any other project)

---

## 1. Purpose

SI.Mind is a **continuously-running agent whose mind is a 3D navigable structure**, plus the
visualization itself as a first-class deliverable. The visualization is not a readout of a
graph that happens to exist; it is the instrument by which the agent is studied.

Six things must be true at once:

1. **His mind is visible.** Every cognitive step is a node in a 3D scene. What he is thinking
   right now, what he recalls, what he decides, what he writes — all visible in real time.
2. **His feelings are visible.** A continuous affect vector drives behaviour and is rendered
   directly into the scene (hue = valence, scale = arousal, tension = brightness/shimmer).
3. **He develops his own personality.** Not authored, not seeded. Derived from his own record.
4. **Each query gets its own microscope.** A per-query view showing exactly what that query
   generated in his mind, step by step, with the feeling curve underneath.
5. **He can be pointed at any provider.** Six providers, live-discovered catalogs, a settings
   surface that controls every knob the code actually reads, and a loop you can write on top of
   without recompiling anything.
6. **Nothing he does is lost.** Plans, brainstorm origins, compaction summaries, journals and
   moods are all indexed into one OKF tree, so any past moment can be traced back to.

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
- **Compaction never destroys.** Only the *aperture* compacts. The log does not, so returning
  to a past moment is exact rather than approximated (§16).
- **Ablations are honest.** Because state is derived, an ablated run is visibly different.

### 2.1 Storage

SQLite (WAL) is the canonical log: durable append, indexed queries, single-file portability.
Each event is stored as JSON in a `payload` column with typed side-columns for indexed fields.
`git init` is run on the data directory too, so the mind's history is diffable and revertible
like code.

| Table | Purpose |
|---|---|
| `event` | append-only log; `seq` is the total order |
| `node` | materialized graph nodes (projection of the log) |
| `edge` | materialized graph edges (projection of the log) |
| `metric_sample` | personality Layer-1 metric time series |
| `setting` | current settings, with schema version |
| `catalog_model` | last known model catalog per provider (dropdown cache) |
| `overlay` | loop overlay stack |
| `provider_credential` | provider keys; never written to the log payload |

The `event` table has `UPDATE` and `DELETE` denied by trigger. Corrections are new events.

---

## 3. Workspace layout

```
SI.Mind/
├── Cargo.toml                    # workspace
├── crates/
│   ├── si-log/                   # event types, SQLite log, replay
│   ├── si-vault/                 # markdown vault, wikilinks, frontmatter, OKF cards
│   ├── si-affect/                # affect vector, 250-feeling lexicon, nearest-neighbour
│   ├── si-cognition/             # the thinking loop, aperture, salience, seams
│   ├── si-personality/           # Layer-1 metrics, Layer-2 self-journals
│   ├── si-values/                # explicit values floor + ablation state
│   ├── si-provider/              # provider registry, model catalog, credentials
│   ├── si-router/                # capability slots + routing decisions
│   ├── si-minimax/               # MiniMax HTTP clients (chat/speech/image/video)
│   ├── si-settings/              # typed setting schema, defaults, reset
│   ├── si-overlay/               # loop overlay stack + generator
│   ├── si-plan/                  # brainstorm/plan modes, lineage, timestamps
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
| `si-vault` | markdown read/write, wikilink graph, frontmatter, OKF cards | decide *what* to write |
| `si-affect` | affect vector, lexicon, nearest-neighbour, decay | call any LLM |
| `si-cognition` | the loop, aperture membership, salience, seams, tool dispatch | touch SQLite directly (use `si-log`) |
| `si-personality` | metrics, self-journal generation | write to `self/` outside the journal writer |
| `si-values` | values floor, ablation state, refusal reasons | be bypassable without logging |
| `si-provider` | provider registry, catalog fetch/parse, credential store | make routing decisions |
| `si-router` | capability slots, routing decision, fallback chains | implement policy |
| `si-minimax` | raw HTTP + typed errors per modality | make routing decisions |
| `si-settings` | setting schema, defaults, load/save, reset | hold business logic |
| `si-overlay` | overlay stack, seams, generator contract | bypass `si-values` |
| `si-plan` | plan/brainstorm documents, lineage, mode state machine | execute plan steps |
| `si-view` | 3D scene, camera, layout solve, panels, Query Lens | mutate the log directly |

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
| `noticed` | a contradiction, duplicate, or anomaly is detected | `kind`, `refs` (serialized as `notice_kind` — see below) |
| `wrote` | a vault note is created, edited, or deleted | `note_id`, `path`, `op` (`create`/`edit`/`delete`) |
| `evicted` | a node leaves the aperture | `node_id`, `reason` |
| `routed` | a model is selected for a call | `slot`, `provider`, `model`, `fallback`, `why` |
| `responded` | a user-facing answer is produced | `answer_ref` |
| `ablation` | a layer is enabled/disabled | `layer`, `state` |
| `consolidated` | background memory work completes | `operation`, `count` |
| `plan_written` | the plan document changes | `plan_id`, `mode`, `diff_ref` |
| `mode_changed` | brainstorm ⇄ plan transition | `from`, `to`, `approved_by` |
| `overlay_applied` | a loop overlay is pushed or removed | `overlay_id`, `seams`, `author`, `removed` |
| `compacted` | the aperture compacts | `turn_id`, `resumer_ref`, `tokens_before`, `tokens_after` |
| `setting_changed` | a setting is written | `key`, `from`, `to` |

`routed` makes the router visible inside the mind: you can see *why* H3 was chosen for a video
and M3.1-Flash for a thought, and you can see a fallback fire when an entitlement lapses.

Each kind is stored as an internally tagged JSON object, so the kind's tag key is `kind`
inside the payload. That key is spent on the tag, so a variant's own field cannot also be
called `kind`: the `noticed` variant's field is serialized as `notice_kind`. The Rust field
keeps the name `kind`, so every consumer reads `EventKind::Noticed { kind, .. }` unchanged.

### 4.1 Event schema

```sql
CREATE TABLE event (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    id         TEXT    NOT NULL UNIQUE,   -- UUIDv7
    ts_us      INTEGER NOT NULL,          -- unix microseconds
    turn_id    TEXT,                      -- NULL for daemon-origin events
    payload    TEXT    NOT NULL,          -- JSON, internally tagged: {"kind":"felt", ...}
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

There is no separate `kind` column. The kind is the tag inside the `payload` JSON, so the
column and the payload it duplicates cannot drift apart. Filter by kind in SQL with
`json_extract(payload, '$.kind') = 'felt'`.

`turn_id` groups events belonging to one user query — the unit the Query Lens replays.

---

## 5. The Aperture

The agent's mind has exactly one scarce resource: what fits in the working context. Everything
else is cold vault. That asymmetry *is* the mind, so the visualization spends its central axis
on it.

- **Radial distance from origin = salience.** Hot nodes are physically near the centre.
- **The aperture is a bounded sphere** of radius `R_APERTURE`. Nodes inside it are in context.
- Eviction is visible: a node drifts out past the rim and cools.

### 5.1 Salience

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
`evicted` with a reason (`budget`, `superseded`, `replaced`, `aged_out`, `compacted`).

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
├── tasks/            # goals in progress
├── plans/            # plan documents (§15)
├── brainstorms/      # brainstorm notes (§15)
├── resumers/         # compaction summaries (§16)
├── overlays/         # loop overlay directives (§13)
├── knowledge/        # the OKF tree (§17)
├── agents.md         # his standing instructions (§14)
└── context_history.md  # human-readable compaction history (§16)
```

Frontmatter: `id`, `title`, `type`, `tags`, `links`, `created`, `updated`, `confidence`,
`supersedes`, `superseded_by`.

The vault is a **projection of the log**, not the source of truth. A note's provenance is the
event sequence that wrote it; deleting a note emits `wrote` with `op: delete` and the note
disappears from the projection — the history stays.

### 6.1 Wikilink graph

Edges are parsed from `[[wikilink]]` bodies plus frontmatter `links`. Edge kinds:
`link`, `hierarchy` (folder containment), `tag`, `contradicts`, `supersedes`, `grounds`,
`origin` (a plan item's lineage back to its brainstorm).

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
- Validated by `tools/validate_lexicon.py`, which enforces the schema, unique ids, the exact
  count of 250, resolvable `nearest` references, axis ranges, a non-empty bias, and the closed
  bias registry in §7.4. `--fix` repairs dangling references by choosing the genuinely closest
  entry in affect-vector distance.

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

### 9.5 Overlays cannot widen the floor

A loop overlay (§13) is a prompt-level directive. It can *narrow* behaviour — ask for more
verification, more patience, a refusal to proceed without confirmation. It **cannot** widen the
floor. The floor is enforced in code, at the point where an action is about to be taken, not in
the prompt where an overlay lives. An overlay that appears to grant permission the floor denies
is refused by the floor and the refusal is logged.

---

## 10. Continuous runtime

### 10.1 Process model

Two binaries:

- **`si-daemon`** — the mind. Owns the log, the vault, the affect state, the scheduler, the
  provider registry. Runs continuously. Survives the viewer being closed.
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
- re-index the OKF tree (§17) for anything changed since the last pass

### 10.5 Yield points

`consolidate`, the OKF indexer, and the layout solve are CPU-bound and run in the daemon. Each
iterates in chunks with `yield_now().await` between batches (M-YIELD-POINTS) so the event
stream and the viewer's SSE never starve.

---

## 11. Provider registry and model router

### 11.1 The provider set

Six providers, all OpenAI-compatible at the chat path. Endpoints verified live on 2026-11-03.

| id | base URL | chat path | key form | env aliases | `/v1/models` without a key |
|---|---|---|---|---|---|
| `minimax.io` | `https://api.minimax.io/v1` | `POST /chat/completions` | Bearer | `MINIMAX_API_KEY` | no |
| `nvidia` | `https://integrate.api.nvidia.com/v1` | `POST /chat/completions` | Bearer `nvapi-` | `NVIDIA_API_KEY`, `OMNI_NVIDIA_KEY` | no |
| `xkiro` | `https://api.xkiro.com/v1` | `POST /chat/completions` | Bearer `gsk_` | `OMNI_XKIRO_KEY` | **yes — HTTP 200 unauthenticated** |
| `atria` | `https://api.atria-asi.ai/v1` | `POST /chat/completions` | Bearer `atr_` | `ATRIA_API_KEY`, `OMNI_ATRIA_KEY` | no — key-gated (401) |
| `reflection` | `https://api.reflection.ai/openai/v1` | `POST /chat/completions` | Bearer | `REFLECTION_API_KEY`, `OMNI_REFLECTION_KEY` | no — key-gated (401) |
| `agnes` | `https://apihub.agnes-ai.com/v1` | `POST /chat/completions` | Bearer | `OMNI_AGNES_KEY` | no |

Catalog sizes observed on 2026-11-03:

| provider | models | notable |
|---|---|---|
| `minimax.io` | 9 chat | plus separate `/v1/t2a_v2`, `/v1/image_generation`, `/v2/video_generation` APIs |
| `nvidia` | 85 | free tier via `build.nvidia.com`; includes `minimaxai/minimax-m3.1-flash` |
| `xkiro` | 103 | 45 `free` / 41 `paid` / 17 `premium`; richest metadata of the six |
| `atria` | key-gated | `Atria-Dawn-Preview` known; no published limits |
| `reflection` | key-gated | `Beam-1.0-501B-A23B` — 1,048,576 context, 131,072 output ceiling |
| `agnes` | 9 | includes **image** (`agnes-image-2.5-flash`) and **video** (`agnes-video-2.5`) models |

### 11.2 Provider descriptor

```rust
/// A configured LLM provider.
///
/// The registry is static and code-owned: base URLs are compile-time facts, not
/// user settings, so a typo cannot silently point the mind at the wrong host.
pub struct Provider {
    pub id: ProviderId,
    pub label: &'static str,
    pub base_url: Cow<'static, str>,
    pub api: ApiStyle,
    pub auth: AuthStyle,
    /// Environment variables that may carry this provider's key, in priority order.
    pub key_env_aliases: &'static [&'static str],
    /// True when the catalog endpoint answers without a credential.
    pub lists_models_without_key: bool,
    pub reasoning: ReasoningStyle,
    pub native_tools: bool,
    pub supports_multi_account: bool,
}
```

`ReasoningStyle` is one of:

- `None` — the provider takes no reasoning parameter (`xkiro`, `atria`, `agnes`).
- `Effort` — a `reasoning_effort` parameter, with per-model levels where published. MiniMax
  international accepts it (verified live 2026-11-03: `low` → 8 reasoning tokens in ~2.9 s,
  `high` → 13 in ~3.2 s), so `minimax.io` and `nvidia` are both `Effort`.
- `EchoReasoning` — `reflection` only: Beam requires every earlier assistant message to carry
  back the `reasoning_content` that produced it, or the request fails with
  *"Missing required parameter: `messages[2].reasoning_content`"*. This is a **superset** of
  `Effort`: reflection also always receives an explicit `reasoning_effort`, because omitting it
  makes the server apply its own `max` default — the most expensive tier.

### 11.3 Normalized model descriptor

Providers return incompatible shapes. The OpenAI standard gives only `id`; xkiro returns
`display_name`, `context_length`, `max_output_tokens`, `capabilities{vision,tools,reasoning}`,
`reasoning_efforts{levels,default}`, `pricing` and `access_tier`. The registry normalizes both
into one descriptor:

```rust
/// A model as the mind understands it, normalized across provider shapes.
pub struct ModelDescriptor {
    pub provider: ProviderId,
    /// The wire id exactly as the provider expects it.
    pub id: String,
    pub display_name: String,
    pub context_window: Option<u32>,
    pub max_output: Option<u32>,
    pub capabilities: Capabilities,
    pub reasoning_levels: Vec<ReasoningLevel>,
    pub access: AccessTier,
    pub pricing: Option<Pricing>,
    pub source: ModelSource,
}

pub struct Capabilities {
    pub vision: bool,
    pub tools: bool,
    pub reasoning: bool,
}

pub enum AccessTier { Free, Paid, Premium, Unknown }
pub enum ModelSource { Catalog, Pinned }
```

A per-provider parser turns each response into `Vec<ModelDescriptor>`. The OpenAI-standard
parser fills only `id` and leaves the rest `None`/`Unknown`; the xkiro parser fills everything.
Descriptors are merged with a small **pinned table** of known-good limits (context window, output
cap) so a provider that publishes nothing still yields usable values.

### 11.4 Fetching the catalog, and when

```
GET {base_url}/models      →  parse per provider  →  merge pinned table
                            →  write catalog_model table  →  populate the dropdown
```

Rules:

- **On key save.** Saving a provider key immediately triggers a fetch for that provider. The key
  is written first, so a failed fetch never loses the credential.
- **On daemon start.** Every provider with a stored key is fetched. xkiro is fetched even
  without a key, because its catalog is open — the dropdown is populated before a credential
  exists.
- **On demand.** A "refresh" action per provider, and a global refresh.
- **Periodic.** Every 6 hours, for providers whose catalog moves.
- **Failure is non-fatal.** The key stays saved, the dropdown shows the last known catalog plus
  a "fetch failed — retry" affordance, and the error is logged. The mind keeps working on the
  cached catalog.
- **Never blocks the loop.** Catalog fetches happen on a background task; the cognition loop
  never waits on a provider catalog.

### 11.5 Capability slots

Routing targets a *slot*, never a model id:

| Slot | Use | Default resolution |
|---|---|---|
| `reasoning` | default thinking | the configured default provider/model |
| `reasoning_fast` | latency-sensitive loops | a `highspeed` model on the same provider, else the default |
| `reasoning_deep` | hard multi-step work | the largest-context model the provider offers |
| `speech` | narration / voice output | `speech-2.8-hd` on minimax.io |
| `speech_realtime` | streaming voice | `speech-2.8-turbo` on minimax.io |
| `speech_input` | transcription | minimax.io speech-to-text |
| `image` | picture generation | `image-01` on minimax.io, or `agnes-image-*` on agnes |
| `video` | video generation | H3 on minimax.io, or `agnes-video-*` on agnes |
| `overlay_author` | writes loop overlays (§13) | the provider/model chosen in the overlay panel |

Each slot resolves against **all enabled providers**, so a slot can be satisfied by whichever
provider is available, and the resolution is recorded.

### 11.6 Routing decision

Two stages, cheapest first:

1. **Local classifier** — regex and keyword intent over the prompt plus the available tool
   schema. Sub-millisecond, no network. Handles the common cases (asks for a picture, asks for
   a clip, asks to read something aloud).
2. **LLM arbiter** — only when stage 1 is ambiguous, e.g. *"make me something"*. A single small
   call to `reasoning_fast` with a fixed classifier prompt. Off by default; enabled per-run.

The decision emits a `routed` event carrying slot, provider, model and `why`. The `why` string
is the human-readable rationale, which is what appears in the Query Lens.

### 11.7 Capability manifest

At startup the router renders a **capability manifest** from the live catalog and injects it
into the agent's system prompt: which providers are reachable, which models exist, which slot
each fills, and how to request a modality switch. This is how "the prompt tells the agent what
model to use" is satisfied — the prompt carries a *generated* capability list, not a
hand-written one.

### 11.8 Fallback chains

Entitlements lapse mid-session. A H3 call can return `402` with
`"The current Token Plan tier does not support this model"` (error `2068`) after working minutes
earlier; NVIDIA returns `429` on exhausted free quota; a provider can simply be down.

Each slot therefore has an ordered fallback chain across providers. On a fallback the router
emits `routed` with `fallback: true` and a `why` naming the original provider, the error class,
and the replacement. The mind map shows it and his journals record it, so degradation is never
silent.

### 11.9 Multi-account

`nvidia` and `agnes` support several accounts against one endpoint. When a key is rate-limited
the router fails over to the next account rather than waiting. Accounts are listed per provider
in the settings panel (§12), with the active one marked.

### 11.10 Token and reasoning budgeting

Verified against `api.minimax.io` on 2026-11-03:

- **Reasoning tokens count against `max_tokens`.** A request with `max_tokens: 8` returned
  `finish_reason: "length"` with `content: ""` and 57 reasoning tokens — the reasoning consumed
  the entire budget. A `max_tokens` set near the expected *content* length therefore yields
  empty answers, not truncated ones. The router budgets `max_tokens` against reasoning plus
  content, with headroom.
- **`reasoning_effort` is accepted** (`low`, `high`; also `minimal`/`medium` per the provider).
  Measured on an identical prompt: `low` produced 8 reasoning tokens in ~2.9 s, `high` produced
  13 in ~3.2 s, both correct.

Consequences:

- The `reasoning` and `reasoning_fast` slots pin `reasoning_effort: "low"`. The default
  cognition path pays a small reasoning tax it does not need, and latency is the scarce
  resource for a continuously-running agent.
- The `reasoning_deep` slot raises the effort and the `max_tokens` floor together.
- `finish_reason: "length"` with empty content is treated as a **budget** failure, not a
  content failure, and retried with a larger budget rather than surfaced as an empty answer.
- `reflection` uses `Effort` style, but its Beam models reason on every request regardless, and
  omitting the parameter makes the server apply its own `max` default — the most expensive tier.
  The router therefore always sends an explicit effort for that provider.

---

## 12. Settings surface

Every knob the code actually reads is exposed, grouped into panels. The UI is **generated from
a schema**, not hand-written, so adding a setting is one struct field plus one schema entry.

### 12.1 Setting schema

```rust
/// One adjustable setting, with enough metadata to render it and to restore it.
pub struct SettingSpec {
    pub key: SettingKey,
    pub label: &'static str,
    pub group: SettingGroup,
    pub kind: SettingKind,
    pub default: SettingValue,
    pub help: &'static str,
}

pub enum SettingKind {
    Bool,
    Int { min: i64, max: i64 },
    Float { min: f32, max: f32 },
    Text,
    Secret,
    Enum { variants: &'static [&'static str] },
    /// A model chosen from the live catalog of a provider.
    Model { provider: ProviderId },
}

pub enum SettingGroup {
    Providers, Model, Cognition, Affect, Personality,
    Values, Vault, Plan, Overlay, Viewer,
}
```

`SettingValue` is the union of the kinds above. Settings are stored in the `setting` table as
typed rows, so a corrupt or unknown value falls back to the schema default rather than
propagating.

### 12.2 Panels

| Panel | Settings |
|---|---|
| **Providers** | per provider: enabled, base URL (code-owned default, advanced override), API key (masked), accounts list, active account, "fetch models now", catalog status, "use as default" |
| **Model** | active provider, active model (from the fetched catalog), reasoning effort, temperature, top_p, max output tokens, seed, stream on/off |
| **Cognition** | aperture token budget, salience decay τ, the three salience weights, max tool iterations, per-provider tool-iteration cap, consolidate / reflect / curate intervals, idle-curiosity daily budget |
| **Affect** | decay time constant, baseline, affect gain, bias scale, label reporting threshold, emotion-to-behaviour strength |
| **Personality** | metric sample windows, reflect cadence, journal location, baseline (locked at 0.5, read-only, with the falsification test shown inline) |
| **Values** | floor file path, refusal verbosity, ablation switches (each one logs an `ablation` event when changed) |
| **Vault / OKF** | vault path, knowledge root, indexing toggles per card type, `context_history.md` path |
| **Plan** | default mode, always-write-a-plan toggle, brainstorm retention (keep / prune), plan retention window |
| **Overlay** | overlay generator provider, overlay generator model, overlay stack list, clear-stack button |
| **Viewer** | scene density, aperture opacity, filament animation speed, backdrop, camera reset |

### 12.3 Reset

- **Per panel** and **global.** Reset restores the schema defaults for the affected keys and
  writes a `setting_changed` event for each.
- Reset never touches credentials, the log, the vault, or the personality baseline. Those are
  not settings: the baseline is 0.5 by law (§8.3), and credentials are not defaults.
- Reset of the Overlay panel clears the overlay stack (§13), which is a separate, explicit
  action from resetting scalar settings.

### 12.4 Credential handling

- Keys are stored in the `provider_credential` table, never in the event log payload and never
  in `settings.json`-style files that get copied around.
- The UI masks them and shows only a fingerprint (last four) for verification.
- Keys are read from the environment first (the alias list in §11.1), then the credential
  table. Environment wins, so a shell variable overrides a saved key.
- The key is never logged. `tracing` events about providers record provider id, not key material.

---

## 13. Loop overlays

### 13.1 What an overlay is

The agentic loop is **fixed Rust control flow** in `si-cognition`. It never changes shape at
runtime. What is modifiable is an **overlay**: a directive layered onto the loop at defined
seams. Overlays stack rather than replace, so "write on top" accumulates instead of destroying
what is there.

No generated code runs in the daemon. An overlay is text, which makes it inspectable, diffable
and reviewable — and it means the loop's safety properties stay in code where they can be
tested (§9.5).

### 13.2 Overlay structure

```rust
/// A directive layered onto the cognition loop at named seams.
pub struct Overlay {
    pub id: OverlayId,
    pub created_at_us: i64,
    pub author: Author,
    /// What the user asked for, verbatim.
    pub instruction: String,
    /// The generated directive.
    pub directive: String,
    pub seams: Vec<Seam>,
    /// Skills this overlay brings into the loop.
    pub skills: Vec<SkillRef>,
    /// The overlay this one stacks on, if any.
    pub base_overlay: Option<OverlayId>,
}

pub enum Seam {
    PreTurn, PostTurn, PreTool, PostTool, Termination, Retrieval,
}

pub enum Author {
    User,
    Model { provider: ProviderId, model: String },
}
```

### 13.3 Seams

| Seam | Effect |
|---|---|
| `PreTurn` | text appended to the system prompt before the turn |
| `PostTurn` | text appended after the turn's reasoning, before the answer |
| `PreTool` | guidance shown to the model before it calls a tool |
| `PostTool` | guidance applied to the tool result before it re-enters the loop |
| `Termination` | criteria for ending the turn, in addition to the built-in ones |
| `Retrieval` | additional guidance for note recall and wikilink traversal |

Prompt seams **concatenate** in stack order. `Termination` is the exception: it unions, so an
overlay can only *add* a stop condition, never remove the built-in ones.

### 13.4 Generating an overlay

The settings panel offers a provider dropdown, then a model dropdown populated from that
provider's fetched catalog, then a free-text instruction. On submit:

1. The generator model receives: the current base loop description, the current overlay stack,
   the target seams, and the user's instruction.
2. It returns a structured directive: seam assignment, the directive text, and any skills to
   incorporate.
3. The overlay is validated: unknown seams are rejected, the directive is length-capped, and
   the values floor is checked (§9.5).
4. It is persisted and an `overlay_applied` event is emitted.

The overlay is then part of every subsequent turn, and its author, model and instruction are
visible in the mind map.

### 13.5 Skills in an overlay

An overlay may reference a skill by id. A skill is a markdown/OKF card in `vault/skills/`
describing a procedure he may invoke. Incorporating a skill registers it in the loop's available
skill set for the overlay's lifetime — it adds a procedure, not an execution primitive.

### 13.6 Removing overlays

- Remove one overlay: the stack beneath it becomes effective again.
- Clear the stack: the built-in base loop is restored.
- Every removal is logged as `overlay_applied` with `removed: true`, so the history of what was
  layered on and when is reconstructable.

---

## 14. Editable AGENTS.md

`vault/agents.md` is his standing instructions: the rules he operates under, loaded into the
aperture every turn. It is editable from the settings panel as a text area.

- **Edits save to the vault** and emit a `wrote` event, so the change is in the log and
  reconstructable.
- **Revisions are kept.** Each save appends a revision; the panel can diff against the previous
  revision.
- **Effect is on the next turn.** The file is read fresh each turn, so an edit does not
  retroactively change a turn already in flight.
- **Reset restores the shipped default**, which lives in the crate as a `include_str!` constant.
- **It participates in OKF indexing** (§17) like any other card, so the agent can retrieve its
  own rules, and a rule can be linked to the plan or journal that produced it.

The AGENTS.md content is *not* part of the values floor (§9.1). The floor is enforced in code;
AGENTS.md shapes behaviour but cannot widen the floor.

---

## 15. Plan and brainstorm modes

### 15.1 Two states

| Mode | What he may do |
|---|---|
| `Brainstorm` | think, read, recall, write notes, maintain the plan — but **not** execute plan steps or mutate anything outside the plan and the vault |
| `Plan` | execute the plan |

Brainstorm mode is a **real gate**, not a label. It removes the temptation to act before the
thinking is settled, and it makes the mode transition a meaningful event rather than a
cosmetic one.

The gate is enforced in code at the point an action is about to be taken, the same place the
values floor is enforced (§9.5). Attempting a gated action is refused and logged — the refusal
is visible, so a mis-set mode is diagnosable rather than mysterious.

### 15.2 The agent always writes a plan

The loop always maintains a plan document, in both modes. In `Brainstorm` the plan holds
hypotheses, options and open threads; in `Plan` it becomes ordered steps with acceptance
criteria. There is never a moment where no plan exists.

### 15.3 Traceability back to the original idea

Every plan item records its origin:

```rust
/// Where a plan item came from, so any step can be traced to the idea that
/// produced it — including an idea that was expressed much earlier.
pub struct Origin {
    pub brainstorm_id: Option<String>,
    pub brainstorm_ts_us: i64,
    /// The exact passage the idea arrived in.
    pub quote: String,
    pub event_seq: u64,
}
```

The Plan panel shows each item's lineage as a chain — item → brainstorm note → the moment in the
timeline where the user said it. Clicking a link seeks the timeline to that moment, and the
scene reconstructs the mind at that point (§16.3).

This is the "kept for later" requirement: an idea said in the first minute of a session is still
reachable as the origin of a step executed in the fortieth.

### 15.4 Timestamps

Every plan item, brainstorm note, resumer and event carries a wall-clock timestamp and a
monotonic sequence number. The pair makes a moment addressable from two directions: by calendar
("what was I doing on Tuesday") and by order ("what was I doing when that happened").

The timestamp is also what the compaction resumer anchors to (§16).

### 15.5 Mode transitions

- The user sets the mode directly.
- The agent may *propose* a transition — typically `Brainstorm → Plan` once it believes the plan
  is coherent — but a proposed transition requires the user's approval. The agent cannot promote
  itself out of Brainstorm.
- Every transition emits `mode_changed` with who approved it.

### 15.6 Plan persistence

Plans live as markdown in `vault/plans/` and brainstorm notes in `vault/brainstorms/`, both
indexed into the OKF tree (§17). The panel renders the current plan, its mode, and the lineage
of each item.

---

## 16. Compaction and context history

### 16.1 Only the aperture compacts

The log is never compacted. When the aperture nears its token budget, the agent compacts *its
working context* — summarizing what it was holding — and continues. The event history is
untouched, so returning to any past moment is exact.

### 16.2 The resumer

Before compacting, the agent writes a **resumer**: a summary sufficient to resume cold. It goes
into a dedicated section of `context_history.md`:

```markdown
# Context History

## Resumers

### 2026-11-03T21:40Z — turn 412, before compaction
- **Goal:** finish the provider registry section of the spec
- **Open threads:** atria's key-gated catalog; whether xkiro's free tier covers video
- **Decisions made:** overlay = prompt layer, not codegen; brainstorm gates execution
- **Next action:** write §11.8 fallback chains
- **Origin:** [[plans/2026-11-03-provider-registry]]
```

The resumer is then loaded into the new aperture, so after compaction he knows where he was.
It is also indexed as an OKF card (`resumers/`), so it is retrievable like everything else.

The resumer is written **by the agent**, in its own words — it is part of the mind's record, not
a system-generated summary that would bypass his own voice.

### 16.3 Returning to a past context

The viewer timeline is navigable. Selecting any past moment reconstructs the mind's state at
that point by folding the log to that sequence number — aperture contents, affect state, plan
mode, overlay stack, everything.

Compaction boundaries are marked on the timeline, so a stretch where the aperture was rewritten
is visibly explained rather than silently missing. A user reading back through the log can see
the resumer that stood in for it.

### 16.4 What is compacted

Only the aperture contents: recalled notes, thought text, and tool results. The following are
never compacted away:

- the plan document and its lineage
- open contradictions
- the values floor and the overlay stack
- his own journals and moods
- the resumers themselves

---

## 17. OKF indexing

Everything the agent produces is indexed into one **Open Knowledge Format** tree inside the
vault, so the agent can retrieve any of it through the same `[[wikilink]]` mechanism used for
everything else.

```
vault/knowledge/
├── plans/<date>-<slug>.md
├── brainstorms/<date>-<slug>.md
├── resumers/<date>-<slug>.md
├── journals/<date>.md
├── moods/<date>-<slug>.md
├── skills/<slug>.md
├── overlays/<id>.md
├── providers/<provider>.md
└── index.md
```

`vault/agents.md` (§14) is **indexed in place** rather than copied into this tree —
it is already a valid OKF card, and duplicating it would give the agent two
copies of his own rules to disagree with.

### 17.1 Card format

Standard OKF: YAML frontmatter plus a markdown body.

```yaml
---
id: plan/2026-11-03-provider-registry
title: Provider registry and model router
type: plan
tags: [router, providers]
links: ["[[brainstorms/2026-11-03-03-multiple-providers]]", "[[agents]]"]
created: 2026-11-03T18:12:00Z
updated: 2026-11-03T21:40:00Z
source_events: [1042, 1051, 1187]
---
```

`source_events` links the card back to the event range that produced it, which is what makes
the OKF tree a *projection* of the log rather than a parallel source of truth.

### 17.2 The indexer

A background pass, run after each `consolidate` cycle and on demand:

1. Walk the vault for eligible cards (the set of card types is configurable in §12).
2. Parse frontmatter and `[[wikilinks]]`.
3. Write or update the card body.
4. Rebuild `knowledge/index.md` and the `origin` edges from plan lineage (§15.3).
5. Emit `consolidated` with the count.

The indexer yields between batches (§10.5) and is safe to interrupt: it is idempotent and
re-derives from the log.

### 17.3 Why one tree

A single index means one retrieval mechanism. The agent does not need a special path to
remember a brainstorm or fetch a resumer — it asks the vault, and the answer is the same
`[[wikilink]]` traversal it uses for everything else. It also means the mind map's graph is
the same graph the agent reasons over, so what you see in the 3D scene *is* what it can reach.

---

## 18. The 3D viewer

### 18.1 Rendering

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
  - `origin` — the lineage filament from a plan item back to its brainstorm note
  - `writes` — outward push as a new note enters the vault
- **Aperture** — a translucent bounding sphere of fixed radius `R_APERTURE` marking the context
  boundary. The boundary itself stays put; the rim's brightness encodes how much of the token
  budget is in use, so the pressure inside a fixed aperture is what you see.

### 18.2 Layout solve

Vault nodes are placed by a force-directed solve constrained to a shell: repulsion between all
nodes, springs along wikilinks, and an attractor pulling salient nodes toward the origin.

```
render_position = lerp(vault_position, hot_position, salience)
```

Because distance from origin *is* salience, a node's rendered position and its cognitive
relevance cannot disagree.

The solve runs incrementally (a few hundred iterations per frame, amortised) in the viewer, so
the scene never stalls. For very large vaults the solver switches to a Barnes–Hut approximation
and the view switches to a decimated node set.

### 18.3 Query Lens

Select a turn → the scene becomes that turn's aperture alone, vault dimmed to context,
replayable step by step:

- ordered steps: recalls, thoughts, decisions, actions
- the **feeling curve** along the bottom — affect plotted across the steps
- why each recall fired
- **provenance**: click any claim in the answer, trace it to the note that grounded it

This is the feature that makes the project an instrument rather than a screensaver.

### 18.4 Panels

The viewer carries the panels the design calls for, all reading and writing through the daemon:

| Panel | Contents |
|---|---|
| **Settings** | the §12 panels, generated from the setting schema |
| **Providers** | key entry, save-and-fetch, model dropdown (§11) |
| **Overlay** | provider → model → instruction; the stack; clear (§13) |
| **AGENTS.md** | editable text area with revision diff (§14) |
| **Plan** | current mode, plan document, item lineage, mode switch (§15) |
| **Context History** | `context_history.md` rendered, resumers navigable (§16) |
| **Viewer** | scene controls (§12.2) |

### 18.5 Direct editing

Click a node → side panel → edit title, summary, body, tags. Writes back to the vault and
emits `wrote`. Because the vault is plain markdown, you can equally edit it in Obsidian or a
text editor, and the scene follows.

---

## 19. Error handling

- **Library crates** (`si-log`, `si-vault`, `si-affect`, `si-minimax`, `si-provider`,
  `si-router`) use canonical error structs (`thiserror`), one per crate.
- **Application crates** (`si-mind`, `si-view`) use `eyre` throughout (M-APP-ERROR).
- **Panics are programming errors only** (M-PANIC-IS-STOP). A malformed API response is a
  `Result::Err`, not a panic.
- **Provider errors are typed** so the router can classify them:
  `ProviderError::Entitlement { code }` · `RateLimited` · `Transient` · `Auth` · `Malformed`.
  Only `Transient` is retried; `Entitlement` triggers a fallback; `RateLimited` triggers
  multi-account failover, then a fallback.
- **The daemon never dies on a bad model call.** Every call is wrapped, the error is logged as
  an event, and the loop continues.
- **A failed catalog fetch never loses the key** and never blocks the loop (§11.4).

---

## 20. Testing

| Level | What | Tooling |
|---|---|---|
| Unit | affect nearest-neighbour, salience decay, wikilink parsing, YAML schema | `cargo test` |
| Unit | setting schema → UI, defaults, reset, corrupt-value fallback | `cargo test` |
| Unit | overlay seam composition, stack order, termination-union | `cargo test` |
| Unit | provider parsers: OpenAI-standard and xkiro extended shapes | `cargo test` |
| Golden | layout solve stability (same log → same positions within epsilon) | `insta` |
| Integration | replay determinism: fold the log twice → identical state | `cargo test` |
| Integration | mode gate: a gated action in Brainstorm is refused and logged | `cargo test` |
| Integration | lineage: every plan item resolves to an origin with a quote | `cargo test` |
| **Falsification** | wipe `self/`, reset metrics, replay → all traits 0.5 | `cargo test --test personality_falsification` |
| **Ablation** | ablation always leaves an `ablation` event in the log | `cargo test --test ablation_audit` |
| Router | routing decisions + fallback on simulated 402 / 429 | `wiremock` |
| Vault | round-trip markdown, frontmatter, wikilinks; `self/` write guard | `cargo test` |
| Lexicon | schema, unique ids, count = 250, resolvable `nearest`, axis ranges, closed bias registry | `tools/validate_lexicon.py` |
| Benchmarks | log append, layout solve, nearest-neighbour query, catalog parse | `criterion` |

Benchmarks are added early (M-HOTPATH) — the layout solve, log append and catalog parse are the
hot paths.

### 20.1 Real-environment validation

The viewer's correctness depends on wall-clock behaviour (decay, timers, animation), so the
harness drives the real daemon over the real SSE stream rather than mocking the clock. The
standing lesson from the DUSTLINE harnesses applies: assert over a window, never at one
instant, and a harness must not fail on its own teardown.

Provider behaviour that only shows up live — entitlement lapses, `finish_reason: "length"`
budget exhaustion, `reasoning_content` echo requirements — is verified against the real APIs
and recorded in this spec, not assumed.

---

## 21. Rust guidelines compliance

- `mimalloc` as the global allocator in both binaries (M-MIMALLOC-APPS).
- `tracing` with named events in `<component>.<operation>.<state>` dot notation and message
  templates, no string formatting in hot paths (M-LOG-STRUCTURED). Sensitive fields redacted.
- `#[expect(lint, reason = "...")]`, never `#[allow]`, for lint overrides (M-LINT-OVERRIDE-EXPECT).
- No `Service` / `Manager` / `Factory` type names (M-CONCISE-NAMES).
- Magic values are named constants with a comment explaining why they were chosen
  (M-DOCUMENTED-MAGIC).
- Public types implement `Debug` (M-PUBLIC-DEBUG).
- `yield_now().await` between chunks in `consolidate`, the OKF indexer and the layout solve
  (M-YIELD-POINTS).
- Compliance date: **2026-03-30**.

---

## 22. Resolved decisions

| Question | Decision |
|---|---|
| Language | Rust |
| Process | Standalone, own git repo, no code shared with other projects |
| Backend | Six providers: minimax.io, nvidia, xkiro, atria, reflection, agnes |
| Default model | `MiniMax-M3.1-Flash` on minimax.io (`-Preview` is an alias of it) |
| Video | H3 (`POST /v2/video_generation`) or `agnes-video-*` |
| Images | `image-01` or `agnes-image-*` |
| TTS | `speech-2.8-hd` (discovery-driven; no `arc` id exists) |
| Model selection | Automatic router with capability slots, live catalog discovery |
| Catalog fetch | On key save, on start, on demand, every 6h; failure is non-fatal |
| Runtime | Continuously running daemon + separate egui viewer |
| Mind substrate | Obsidian-compatible markdown vault with `[[wikilinks]]` |
| Visualization | 3D, hand-rolled math, egui |
| Emotion engine | 3 continuous axes + 250-feeling lexicon, closed 13-parameter bias registry |
| Personality | Derived only; Layer-1 metrics + Layer-2 self-journals; baseline 0.5 |
| Ethics | Explicit values floor + affect layer; ablations logged, never silent |
| Loop override | **Prompt layered on top of the loop**, not recompiled code |
| Overlays | Stack, don't replace; text only; cannot widen the values floor |
| Brainstorm mode | **Gates execution** — may think/write, may not execute plan steps |
| AGENTS.md | Editable from the settings panel, revisioned, effective next turn |
| Compaction | Aperture compacts, log never does; resumer written to `context_history.md` |
| OKF | One tree, indexed from the log, retrieved like any other wikilink |

## 23. Open questions

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
5. **Overlay seam for values** — the floor is enforced in code (§9.5), so an overlay cannot
   widen it. But an overlay *can* request more caution, which raises refusals. Should an
   overlay be able to raise the floor, or only the code?
6. **Plan retention** — plans accumulate. When does a finished plan get pruned, and does a
   pruned plan's lineage remain traceable? Proposal: the card stays in the OKF tree with a
   `retired` tag even after the working copy is removed.
