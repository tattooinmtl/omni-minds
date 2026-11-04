// Rust guideline compliant 2026-03-30

//! The event vocabulary. Every cognitive step the mind takes is one of these.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identity of a single event. UUIDv7 so ids sort by creation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventId(pub Uuid);

/// Identity of one user turn. Groups the events produced by a single query.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TurnId(pub String);

/// The affect state at the moment an event was emitted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AffectSnapshot {
    pub valence: f32,
    pub arousal: f32,
    pub tension: f32,
}

/// How a note was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteOp {
    Create,
    Edit,
    Delete,
}

/// Why a node left the attention aperture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvictReason {
    Budget,
    Superseded,
    Replaced,
    AgedOut,
    Compacted,
}

/// Every kind of thing that can happen to the mind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    Perceived {
        source: String,
        content_ref: String,
    },
    Recalled {
        note_id: String,
        reason: String,
        query: String,
    },
    Thought {
        text_ref: String,
        confidence: f32,
    },
    Decided {
        options: Vec<String>,
        chosen: String,
        rationale: String,
    },
    Acted {
        tool: String,
        args_ref: String,
        result_ref: String,
    },
    Felt {
        delta: [f32; 3],
        after: [f32; 3],
        label: String,
    },
    Noticed {
        /// What was noticed: `contradiction`, `duplicate`, `anomaly`, ...
        ///
        /// Serialized as `notice_kind`, not `kind`. The internally tagged
        /// representation already spends the `kind` key on the variant tag
        /// itself, and serde rejects a variant field that shares the tag's
        /// name. The Rust field keeps the spec's name so every consumer
        /// matches `EventKind::Noticed { kind, .. }` unchanged.
        #[serde(rename = "notice_kind")]
        kind: String,
        refs: Vec<String>,
    },
    Wrote {
        note_id: String,
        path: String,
        op: WriteOp,
    },
    Evicted {
        node_id: String,
        reason: EvictReason,
    },
    Routed {
        slot: String,
        provider: String,
        model: String,
        fallback: bool,
        why: String,
    },
    Responded {
        answer_ref: String,
    },
    Ablation {
        layer: String,
        state: bool,
    },
    Consolidated {
        operation: String,
        count: u32,
    },
    PlanWritten {
        plan_id: String,
        mode: String,
        diff_ref: String,
    },
    ModeChanged {
        from: String,
        to: String,
        approved_by: String,
    },
    OverlayApplied {
        overlay_id: String,
        seams: Vec<String>,
        author: String,
        removed: bool,
    },
    Compacted {
        turn_id: Option<TurnId>,
        resumer_ref: String,
        tokens_before: u32,
        tokens_after: u32,
    },
    SettingChanged {
        key: String,
        from: serde_json::Value,
        to: serde_json::Value,
    },
}

/// One immutable entry in the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub seq: u64,
    pub ts_us: i64,
    pub turn_id: Option<TurnId>,
    pub kind: EventKind,
    pub affect: Option<AffectSnapshot>,
    pub salience: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_round_trips_through_json_with_a_stable_tag() {
        let kind = EventKind::Routed {
            slot: "reasoning".into(),
            provider: "minimax.io".into(),
            model: "MiniMax-M3.1-Flash".into(),
            fallback: false,
            why: "default slot".into(),
        };
        let json = serde_json::to_value(&kind).expect("serialize");
        assert_eq!(json["kind"], "routed");
        let back: EventKind = serde_json::from_value(json).expect("deserialize");
        assert!(matches!(back, EventKind::Routed { .. }));
    }

    #[test]
    fn every_kind_has_a_distinct_serde_tag() {
        let kinds = vec![
            EventKind::Perceived {
                source: "user".into(),
                content_ref: "c".into(),
            },
            EventKind::Recalled {
                note_id: "n".into(),
                reason: "r".into(),
                query: "q".into(),
            },
            EventKind::Thought {
                text_ref: "t".into(),
                confidence: 0.5,
            },
            EventKind::Decided {
                options: vec![],
                chosen: "c".into(),
                rationale: "r".into(),
            },
            EventKind::Acted {
                tool: "t".into(),
                args_ref: "a".into(),
                result_ref: "r".into(),
            },
            EventKind::Felt {
                delta: [0.0; 3],
                after: [0.0; 3],
                label: "calm".into(),
            },
            EventKind::Noticed {
                kind: "contradiction".into(),
                refs: vec![],
            },
            EventKind::Wrote {
                note_id: "n".into(),
                path: "p".into(),
                op: WriteOp::Create,
            },
            EventKind::Evicted {
                node_id: "n".into(),
                reason: EvictReason::Budget,
            },
            EventKind::Routed {
                slot: "s".into(),
                provider: "p".into(),
                model: "m".into(),
                fallback: false,
                why: "w".into(),
            },
            EventKind::Responded {
                answer_ref: "a".into(),
            },
            EventKind::Ablation {
                layer: "values".into(),
                state: false,
            },
            EventKind::Consolidated {
                operation: "merge".into(),
                count: 1,
            },
            EventKind::PlanWritten {
                plan_id: "p".into(),
                mode: "brainstorm".into(),
                diff_ref: "d".into(),
            },
            EventKind::ModeChanged {
                from: "brainstorm".into(),
                to: "plan".into(),
                approved_by: "user".into(),
            },
            EventKind::OverlayApplied {
                overlay_id: "o".into(),
                seams: vec![],
                author: "user".into(),
                removed: false,
            },
            EventKind::Compacted {
                turn_id: None,
                resumer_ref: "r".into(),
                tokens_before: 100,
                tokens_after: 10,
            },
            EventKind::SettingChanged {
                key: "k".into(),
                from: serde_json::Value::Null,
                to: serde_json::Value::Null,
            },
        ];
        let mut tags: Vec<String> = kinds
            .iter()
            .map(|k| {
                serde_json::to_value(k).expect("serialize")["kind"]
                    .as_str()
                    .expect("tag")
                    .to_string()
            })
            .collect();
        tags.sort();
        let count = tags.len();
        tags.dedup();
        assert_eq!(tags.len(), count, "event kind serde tags must be unique");
    }

    /// The exact JSON a `noticed` event is written to the log as.
    ///
    /// The tag key is `kind` and the payload key is `notice_kind`: serde
    /// spends the `kind` key on the variant tag, so the payload field cannot
    /// share that name. Changing this string invalidates every `noticed`
    /// event already in the log.
    const NOTICED_PAYLOAD_JSON: &str =
        r#"{"kind":"noticed","notice_kind":"contradiction","refs":["a"]}"#;

    /// Pins the wire format: the 18 tag strings from spec §4, and the shape
    /// and round trip of the `noticed` payload.
    ///
    /// The tags are written into the `payload` column of the event table and
    /// read back by every later component (`json_extract(payload, '$.kind')`).
    /// A drifting tag or key keeps the crate compiling and leaves the other
    /// tests green, so the format is asserted here rather than assumed.
    #[test]
    fn the_wire_format_is_pinned() {
        // The 18 kinds, each paired with the tag the spec of record gives it
        // (`docs/superpowers/specs/2026-11-03-si-mind-design.md` §4).
        let kinds = vec![
            (
                "perceived",
                EventKind::Perceived {
                    source: "user".into(),
                    content_ref: "c".into(),
                },
            ),
            (
                "recalled",
                EventKind::Recalled {
                    note_id: "n".into(),
                    reason: "r".into(),
                    query: "q".into(),
                },
            ),
            (
                "thought",
                EventKind::Thought {
                    text_ref: "t".into(),
                    confidence: 0.5,
                },
            ),
            (
                "decided",
                EventKind::Decided {
                    options: vec![],
                    chosen: "c".into(),
                    rationale: "r".into(),
                },
            ),
            (
                "acted",
                EventKind::Acted {
                    tool: "t".into(),
                    args_ref: "a".into(),
                    result_ref: "r".into(),
                },
            ),
            (
                "felt",
                EventKind::Felt {
                    delta: [0.0; 3],
                    after: [0.0; 3],
                    label: "calm".into(),
                },
            ),
            (
                "noticed",
                EventKind::Noticed {
                    kind: "contradiction".into(),
                    refs: vec!["a".into()],
                },
            ),
            (
                "wrote",
                EventKind::Wrote {
                    note_id: "n".into(),
                    path: "p".into(),
                    op: WriteOp::Create,
                },
            ),
            (
                "evicted",
                EventKind::Evicted {
                    node_id: "n".into(),
                    reason: EvictReason::Budget,
                },
            ),
            (
                "routed",
                EventKind::Routed {
                    slot: "s".into(),
                    provider: "p".into(),
                    model: "m".into(),
                    fallback: false,
                    why: "w".into(),
                },
            ),
            (
                "responded",
                EventKind::Responded {
                    answer_ref: "a".into(),
                },
            ),
            (
                "ablation",
                EventKind::Ablation {
                    layer: "values".into(),
                    state: false,
                },
            ),
            (
                "consolidated",
                EventKind::Consolidated {
                    operation: "merge".into(),
                    count: 1,
                },
            ),
            (
                "plan_written",
                EventKind::PlanWritten {
                    plan_id: "p".into(),
                    mode: "brainstorm".into(),
                    diff_ref: "d".into(),
                },
            ),
            (
                "mode_changed",
                EventKind::ModeChanged {
                    from: "brainstorm".into(),
                    to: "plan".into(),
                    approved_by: "user".into(),
                },
            ),
            (
                "overlay_applied",
                EventKind::OverlayApplied {
                    overlay_id: "o".into(),
                    seams: vec![],
                    author: "user".into(),
                    removed: false,
                },
            ),
            (
                "compacted",
                EventKind::Compacted {
                    turn_id: None,
                    resumer_ref: "r".into(),
                    tokens_before: 100,
                    tokens_after: 10,
                },
            ),
            (
                "setting_changed",
                EventKind::SettingChanged {
                    key: "k".into(),
                    from: serde_json::Value::Null,
                    to: serde_json::Value::Null,
                },
            ),
        ];
        assert_eq!(kinds.len(), 18, "the spec lists exactly 18 event kinds");
        for (expected_tag, kind) in &kinds {
            let json = serde_json::to_value(kind).expect("serialize");
            assert_eq!(
                json["kind"], *expected_tag,
                "the {} tag must not drift from spec §4",
                expected_tag
            );
        }

        // The `noticed` payload is written under `notice_kind`, not `kind`.
        let noticed = EventKind::Noticed {
            kind: "contradiction".into(),
            refs: vec!["a".into()],
        };
        assert_eq!(
            serde_json::to_string(&noticed).expect("serialize"),
            NOTICED_PAYLOAD_JSON,
            "the noticed payload shape must not drift"
        );

        // And the same JSON reads back into the same event.
        let back: EventKind = serde_json::from_str(NOTICED_PAYLOAD_JSON).expect("deserialize");
        assert!(
            matches!(&back, EventKind::Noticed { kind, .. } if kind == "contradiction"),
            "a noticed payload must deserialize into a noticed event, got {back:?}"
        );
        assert_eq!(
            back, noticed,
            "the noticed payload must round-trip unchanged"
        );
    }
}
