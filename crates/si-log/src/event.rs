// Rust guideline compliant 2026-03-30

//! The event vocabulary. Every cognitive step the mind takes is one of these.
//!
//! Every number an event carries is repaired to a finite value on the way
//! into the log and on the way out of it. The log is append-only, so a
//! number that is not finite would be permanent: `serde_json` writes it as
//! `null`, and `null` is not a valid `f32`, so the event that carried it
//! would never be read back.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::finite::{
    deserialize_after, deserialize_arousal, deserialize_confidence, deserialize_delta,
    deserialize_salience, deserialize_tension, deserialize_valence, finite, repaired_vector,
    serialize_finite, serialize_finite_vector, Original, Repair,
};

/// Identity of a single event. UUIDv7 so ids sort by creation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventId(pub Uuid);

/// Identity of one user turn. Groups the events produced by a single query.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TurnId(pub String);

/// The affect state at the moment an event was emitted.
///
/// The three axes are the same three axes `si-affect` keeps: `valence`,
/// `arousal` and `tension`. Each is repaired to a finite value on the way
/// into the log and on the way out of it, so an affect state is never lost
/// to a number that cannot be stored.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AffectSnapshot {
    /// −1 displeasure, +1 pleasure.
    #[serde(
        serialize_with = "serialize_finite",
        deserialize_with = "deserialize_valence"
    )]
    pub valence: f32,
    /// −1 calm, +1 activated.
    #[serde(
        serialize_with = "serialize_finite",
        deserialize_with = "deserialize_arousal"
    )]
    pub arousal: f32,
    /// −1 in control, +1 threatened or uncertain.
    #[serde(
        serialize_with = "serialize_finite",
        deserialize_with = "deserialize_tension"
    )]
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
        /// How sure the mind was of the inference.
        ///
        /// Repaired to a finite value on the way into the log and on the way
        /// out of it, so a thought is never made unreadable by its number.
        #[serde(
            serialize_with = "serialize_finite",
            deserialize_with = "deserialize_confidence"
        )]
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
        /// How the affect state moved.
        ///
        /// Repaired to a finite vector on the way into the log and on the way
        /// out of it.
        #[serde(
            serialize_with = "serialize_finite_vector",
            deserialize_with = "deserialize_delta"
        )]
        delta: [f32; 3],
        /// The affect state after the movement.
        #[serde(
            serialize_with = "serialize_finite_vector",
            deserialize_with = "deserialize_after"
        )]
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
    /// How much this event pulls the mind's attention.
    ///
    /// Repaired to a finite value on the way into the log and on the way out
    /// of it, so a row is never made unreadable by its salience.
    #[serde(
        serialize_with = "serialize_finite",
        deserialize_with = "deserialize_salience"
    )]
    pub salience: f32,
}

impl EventKind {
    /// The event with every number it carries repaired to a finite value, and
    /// the names of the fields that had to be repaired.
    ///
    /// The log is append-only, so a number that is not finite would be
    /// permanent: `serde_json` writes it as `null`, and `null` is not a valid
    /// `f32` to read back, so the event that carried it would never be read
    /// again. The repair happens before the event is appended, never after.
    ///
    /// A field that already holds a finite number is returned unchanged, so a
    /// log written today holds exactly the bytes it always held.
    ///
    /// A variant that carries a number must be listed here as well as in the
    /// serde attributes above: the attributes keep the log clean whatever the
    /// caller does, and this keeps the event the caller is handed the same
    /// event the log holds.
    ///
    /// The event is taken by value, so the repair costs no copy of it: this
    /// runs on the path every cognitive step takes.
    pub(crate) fn repaired(self) -> (EventKind, Vec<Repair>) {
        match self {
            EventKind::Thought {
                text_ref,
                confidence,
            } => {
                let mut repairs = Vec::new();
                let confidence = match Original::of(confidence) {
                    Some(original) => {
                        repairs.push(Repair {
                            field: "confidence",
                            axis: None,
                            original,
                        });
                        finite(confidence)
                    }
                    None => confidence,
                };
                (
                    EventKind::Thought {
                        text_ref,
                        confidence,
                    },
                    repairs,
                )
            }
            EventKind::Felt {
                delta,
                after,
                label,
            } => {
                let mut repairs = Vec::new();
                let delta = repaired_vector("delta", delta, &mut repairs);
                let after = repaired_vector("after", after, &mut repairs);
                (
                    EventKind::Felt {
                        delta,
                        after,
                        label,
                    },
                    repairs,
                )
            }
            other => (other, Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact JSON a `thought` event is written to the log as.
    ///
    /// The number is a plain JSON number, not a string and not a null: a log
    /// written today has to read identically tomorrow.
    const THOUGHT_PAYLOAD_JSON: &str = r#"{"kind":"thought","text_ref":"t","confidence":0.5}"#;

    /// The exact JSON a `felt` event is written to the log as.
    ///
    /// The two vectors are plain JSON arrays of three numbers.
    const FELT_PAYLOAD_JSON: &str =
        r#"{"kind":"felt","delta":[0.1,0.0,-0.5],"after":[0.2,-0.1,0.3],"label":"calm"}"#;

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

    /// A finite event is written exactly as it always was.
    ///
    /// The repair must not touch a number that is already fine, so the wire
    /// format of a finite event is pinned here: a log written today has to
    /// read identically tomorrow. The plain `serde_json` output of the same
    /// numbers is asserted alongside it, which is what the payload looked
    /// like before the repair was added.
    #[test]
    fn the_wire_format_of_a_finite_event_is_unchanged() {
        let thought = EventKind::Thought {
            text_ref: "t".into(),
            confidence: 0.5,
        };
        assert_eq!(
            serde_json::to_string(&thought).expect("serialize"),
            THOUGHT_PAYLOAD_JSON,
            "the thought payload shape must not drift"
        );
        assert_eq!(
            serde_json::to_string(&0.5f32).expect("serialize"),
            "0.5",
            "a finite number is written as a plain JSON number"
        );

        let felt = EventKind::Felt {
            delta: [0.1, 0.0, -0.5],
            after: [0.2, -0.1, 0.3],
            label: "calm".into(),
        };
        assert_eq!(
            serde_json::to_string(&felt).expect("serialize"),
            FELT_PAYLOAD_JSON,
            "the felt payload shape must not drift"
        );
        assert_eq!(
            serde_json::to_string(&[0.1f32, 0.0, -0.5]).expect("serialize"),
            "[0.1,0.0,-0.5]",
            "an affect vector is written as a plain JSON array of three numbers"
        );

        // And the same JSON reads back into the same events.
        let back: EventKind = serde_json::from_str(THOUGHT_PAYLOAD_JSON).expect("deserialize");
        assert_eq!(
            back, thought,
            "the thought payload must round-trip unchanged"
        );
        let back: EventKind = serde_json::from_str(FELT_PAYLOAD_JSON).expect("deserialize");
        assert_eq!(back, felt, "the felt payload must round-trip unchanged");
    }

    /// A number that is not finite never reaches the log.
    ///
    /// `serde_json` writes a NaN or an infinity as `null`, and `null` is not
    /// a valid `f32`, so an event written with one would never be read back.
    #[test]
    fn a_not_a_number_is_written_as_a_finite_number() {
        let thought = EventKind::Thought {
            text_ref: "t".into(),
            confidence: f32::NAN,
        };
        let json = serde_json::to_string(&thought).expect("serialize");
        assert!(
            !json.contains("null"),
            "a non-finite number must never reach the log as a null, got {json}"
        );
        let back: EventKind = serde_json::from_str(&json).expect("deserialize");
        let EventKind::Thought { confidence, .. } = back else {
            panic!("expected a thought event, got {back:?}");
        };
        assert!(
            confidence.is_finite(),
            "the confidence must be finite, got {confidence}"
        );

        let felt = EventKind::Felt {
            delta: [f32::NAN, 0.0, f32::INFINITY],
            after: [0.0, f32::NEG_INFINITY, 0.0],
            label: "calm".into(),
        };
        let json = serde_json::to_string(&felt).expect("serialize");
        assert!(
            !json.contains("null"),
            "a non-finite number must never reach the log as a null, got {json}"
        );
        let back: EventKind = serde_json::from_str(&json).expect("deserialize");
        let EventKind::Felt { delta, after, .. } = back else {
            panic!("expected a felt event, got {back:?}");
        };
        for axis in delta.iter().chain(after.iter()) {
            assert!(axis.is_finite(), "every axis must be finite, got {axis}");
        }
    }

    /// A payload that holds a `null` where a number belongs reads back.
    ///
    /// The `null` is how a number that is not finite reaches the log, so a
    /// payload that holds one must not make its own row unreadable.
    #[test]
    fn a_null_where_a_number_belongs_reads_back_as_a_finite_number() {
        let back: EventKind =
            serde_json::from_str(r#"{"kind":"thought","text_ref":"t","confidence":null}"#)
                .expect("deserialize");
        let EventKind::Thought { confidence, .. } = back else {
            panic!("expected a thought event, got {back:?}");
        };
        assert!(
            confidence.is_finite(),
            "a null must read back as a finite number, got {confidence}"
        );

        let back: EventKind = serde_json::from_str(
            r#"{"kind":"felt","delta":[null,0.1,null],"after":[0.2,null,0.3],"label":"calm"}"#,
        )
        .expect("deserialize");
        let EventKind::Felt { delta, after, .. } = back else {
            panic!("expected a felt event, got {back:?}");
        };
        for axis in delta.iter().chain(after.iter()) {
            assert!(axis.is_finite(), "every axis must be finite, got {axis}");
        }
    }

    /// A missing field is still an error.
    ///
    /// The repair is for a malformed number, not for an absent one: a number
    /// nobody wrote must not be invented, or the log would claim a fact
    /// about the mind that the mind never had.
    #[test]
    fn a_missing_field_is_still_an_error() {
        let error = serde_json::from_str::<EventKind>(r#"{"kind":"thought","text_ref":"t"}"#)
            .expect_err("a missing field must still be an error")
            .to_string();
        assert!(
            error.contains("missing field `confidence`"),
            "a missing confidence must still be reported as missing, got {error}"
        );

        let error = serde_json::from_str::<AffectSnapshot>(r#"{"arousal":0.1,"tension":0.2}"#)
            .expect_err("a missing field must still be an error")
            .to_string();
        assert!(
            error.contains("missing field `valence`"),
            "a missing valence must still be reported as missing, got {error}"
        );
    }

    /// The repair says what it repaired.
    ///
    /// A repair is worth a trace, so the field that had to be repaired, and
    /// what that field held, are named rather than swallowed.
    #[test]
    fn the_repair_names_the_fields_it_repaired() {
        let thought = EventKind::Thought {
            text_ref: "t".into(),
            confidence: f32::NAN,
        };
        let serialized = serde_json::to_string(&thought).expect("serialize");
        let (repaired, repairs) = thought.repaired();
        assert_eq!(repairs.len(), 1, "the confidence was repaired");
        assert_eq!(repairs[0].field, "confidence");
        assert_eq!(repairs[0].original, Original::NotANumber);
        assert_eq!(
            serde_json::to_string(&repaired).expect("serialize"),
            serialized,
            "the repaired event and the serialised event must be the same event"
        );

        let felt = EventKind::Felt {
            delta: [f32::NAN, 0.0, 0.0],
            after: [0.0, 0.0, 0.0],
            label: "calm".into(),
        };
        let serialized = serde_json::to_string(&felt).expect("serialize");
        let (repaired, repairs) = felt.repaired();
        assert_eq!(repairs.len(), 1, "only the delta was repaired");
        assert_eq!(repairs[0].field, "delta");
        assert_eq!(repairs[0].axis, Some(0), "the first axis was repaired");
        assert_eq!(
            serde_json::to_string(&repaired).expect("serialize"),
            serialized,
            "the repaired event and the serialised event must be the same event"
        );

        let finite = EventKind::Thought {
            text_ref: "t".into(),
            confidence: 0.5,
        };
        let expected = finite.clone();
        let (repaired, repairs) = finite.repaired();
        assert!(
            repairs.is_empty(),
            "a finite event needs no repair, got {repairs:?}"
        );
        assert_eq!(repaired, expected, "a finite event is returned unchanged");
    }

    /// The two infinities are different facts, and must not report alike.
    ///
    /// Both are repaired to the same finite value, so only the report can
    /// tell them apart.
    #[test]
    fn the_two_infinities_are_reported_apart() {
        let positive = EventKind::Thought {
            text_ref: "t".into(),
            confidence: f32::INFINITY,
        };
        let negative = EventKind::Thought {
            text_ref: "t".into(),
            confidence: f32::NEG_INFINITY,
        };
        let (_, repairs) = positive.repaired();
        let positive_original = repairs[0].original;
        let (_, repairs) = negative.repaired();
        let negative_original = repairs[0].original;
        assert_eq!(positive_original, Original::PositiveInfinity);
        assert_eq!(negative_original, Original::NegativeInfinity);
        assert_ne!(
            positive_original, negative_original,
            "the two infinities must not report the same thing"
        );
    }
}
