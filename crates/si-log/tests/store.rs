use si_log::{AffectSnapshot, EventKind, Log, TurnId};

fn temp_db() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mind.db");
    (dir, path)
}

/// An id for a row written by hand.
///
/// The log never edits a row, so the only way a malformed row gets into it
/// is by writing it directly. The id is a fixed UUID so the row can be
/// written twice across two runs without colliding.
const HAND_WRITTEN_ID: &str = "0192b3c4-5d6e-7f80-91a2-b3c4d5e6f708";

/// Writes one raw row into the log, the way a hand or an older build would.
///
/// The row is written with no affect state and no salience, so the payload
/// is the only thing under test.
fn write_raw_row(log: &Log, id: &str, payload: &str) {
    log.conn_for_test()
        .execute(
            "INSERT INTO event (id, ts_us, turn_id, payload, valence, arousal, tension, salience)
             VALUES (?1, 0, NULL, ?2, NULL, NULL, NULL, 0.0)",
            rusqlite::params![id, payload],
        )
        .expect("the raw row must be writable");
}

/// The payload column of one row, read straight out of SQLite.
fn payload_of(log: &Log, seq: u64) -> String {
    log.conn_for_test()
        .query_row(
            "SELECT payload FROM event WHERE seq = ?1",
            rusqlite::params![seq],
            |row| row.get(0),
        )
        .expect("the payload must be readable")
}

/// Every number an event kind carries must be finite.
fn assert_finite(kind: &EventKind) {
    match kind {
        EventKind::Thought { confidence, .. } => assert!(
            confidence.is_finite(),
            "the confidence must be finite, got {confidence}"
        ),
        EventKind::Felt { delta, after, .. } => {
            for axis in delta.iter().chain(after.iter()) {
                assert!(axis.is_finite(), "every axis must be finite, got {axis}");
            }
        }
        _ => {}
    }
}

/// The whole error chain of a failure, as one string.
///
/// The store wraps a failure to read a row in a SQLite conversion failure, so
/// the message that says what is actually wrong sits further down the chain.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(inner) = source {
        text.push_str(" | ");
        text.push_str(&inner.to_string());
        source = inner.source();
    }
    text
}

#[test]
fn append_assigns_monotonic_sequence_numbers() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    let first = log
        .append(
            EventKind::Responded {
                answer_ref: "a".into(),
            },
            None,
            None,
            0.0,
        )
        .expect("append");
    let second = log
        .append(
            EventKind::Responded {
                answer_ref: "b".into(),
            },
            None,
            None,
            0.0,
        )
        .expect("append");
    assert_eq!(first.seq, 1);
    assert_eq!(second.seq, 2);
    assert_eq!(log.count().expect("count"), 2);
}

#[test]
fn update_and_delete_are_rejected_by_trigger() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    log.append(
        EventKind::Responded {
            answer_ref: "a".into(),
        },
        None,
        None,
        0.0,
    )
    .expect("append");
    let update = log
        .conn_for_test()
        .execute("UPDATE event SET salience = 1.0", []);
    assert!(update.is_err(), "UPDATE on the event table must be denied");
    let delete = log.conn_for_test().execute("DELETE FROM event", []);
    assert!(delete.is_err(), "DELETE on the event table must be denied");
}

#[test]
fn events_carry_the_affect_snapshot_and_turn_id() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    let turn = TurnId("turn-1".into());
    let affect = AffectSnapshot {
        valence: 0.2,
        arousal: -0.1,
        tension: 0.3,
    };
    let event = log
        .append(
            EventKind::Felt {
                delta: [0.1, 0.0, 0.0],
                after: [0.2, -0.1, 0.3],
                label: "calm".into(),
            },
            Some(&turn),
            Some(affect),
            0.7,
        )
        .expect("append");
    assert_eq!(event.turn_id, Some(turn));
    assert_eq!(event.affect, Some(affect));
    assert_eq!(event.salience, 0.7);
    let fetched = log.event_at(event.seq).expect("event_at").expect("present");
    assert_eq!(fetched, event);
}

#[test]
fn the_payload_column_holds_the_whole_tagged_event() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    let event = log
        .append(
            EventKind::Noticed {
                kind: "contradiction".into(),
                refs: vec!["a".into()],
            },
            None,
            None,
            0.0,
        )
        .expect("append");

    let payload: String = log
        .conn_for_test()
        .query_row(
            "SELECT payload FROM event WHERE seq = ?1",
            rusqlite::params![event.seq],
            |row| row.get(0),
        )
        .expect("payload");
    assert_eq!(
        payload,
        r#"{"kind":"noticed","notice_kind":"contradiction","refs":["a"]}"#
    );

    let fetched = log.event_at(event.seq).expect("event_at").expect("present");
    assert_eq!(fetched.kind, event.kind);
}

#[test]
fn the_event_table_has_no_kind_column() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    let mut stmt = log
        .conn_for_test()
        .prepare("PRAGMA table_info(event)")
        .expect("prepare");
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect::<Vec<String>>();
    assert!(
        !columns.iter().any(|name| name == "kind"),
        "the kind lives inside the payload, not in a column: {columns:?}"
    );
}

#[test]
fn a_replace_cannot_overwrite_an_existing_event() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    let event = log
        .append(
            EventKind::Responded {
                answer_ref: "a".into(),
            },
            None,
            None,
            0.0,
        )
        .expect("append");

    let replaced = log.conn_for_test().execute(
        "INSERT OR REPLACE INTO event (id, ts_us, payload, salience) VALUES (?1, 0, 'rewritten', 0.0)",
        rusqlite::params![event.id.0.to_string()],
    );
    assert!(
        replaced.is_err(),
        "a REPLACE must not overwrite an existing event"
    );

    let fetched = log.event_at(event.seq).expect("event_at").expect("present");
    assert_eq!(
        fetched, event,
        "the original event must survive the attempt"
    );
}

/// A number that is not finite must never reach the log.
///
/// The log is append-only and is the only source of truth, so a number that
/// is not finite is permanent damage: `serde_json` writes it as `null`, and
/// `null` is not a valid `f32`, so the row would never be read back.
#[test]
fn a_not_a_number_in_a_felt_delta_is_written_finite_and_reads_back_finite() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    let event = log
        .append(
            EventKind::Felt {
                delta: [f32::NAN, 0.0, -0.5],
                after: [0.2, -0.1, 0.3],
                label: "calm".into(),
            },
            None,
            None,
            0.5,
        )
        .expect("append");

    let payload = payload_of(&log, event.seq);
    assert!(
        !payload.contains("null"),
        "a non-finite number must never reach the log as a null, got {payload}"
    );

    let fetched = log.event_at(event.seq).expect("event_at").expect("present");
    assert_finite(&fetched.kind);
    let EventKind::Felt { delta, .. } = fetched.kind else {
        panic!("expected a felt event, got {:?}", fetched.kind);
    };
    assert!(
        delta[0].is_finite(),
        "the repaired axis must be finite, got {}",
        delta[0]
    );
}

/// A confidence that is not finite must never reach the log.
#[test]
fn a_not_a_number_in_a_confidence_is_written_finite_and_reads_back_finite() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    let event = log
        .append(
            EventKind::Thought {
                text_ref: "t".into(),
                confidence: f32::NAN,
            },
            None,
            None,
            0.5,
        )
        .expect("append");

    let payload = payload_of(&log, event.seq);
    assert!(
        !payload.contains("null"),
        "a non-finite number must never reach the log as a null, got {payload}"
    );

    let fetched = log.event_at(event.seq).expect("event_at").expect("present");
    assert_finite(&fetched.kind);
    let EventKind::Thought { confidence, .. } = fetched.kind else {
        panic!("expected a thought event, got {:?}", fetched.kind);
    };
    assert!(
        confidence.is_finite(),
        "the repaired confidence must be finite, got {confidence}"
    );
}

/// A payload that holds a `null` where a number belongs must still read.
///
/// The `null` is how a number that is not finite reaches the log, so a row
/// that holds one must not be made unreadable: the row is the only record
/// of a step the mind took.
#[test]
fn a_payload_row_with_a_null_float_reads_back_as_a_finite_value() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    write_raw_row(
        &log,
        HAND_WRITTEN_ID,
        r#"{"kind":"thought","text_ref":"t","confidence":null}"#,
    );

    let fetched = log
        .event_at(1)
        .expect("the row must be readable")
        .expect("the row must be present");
    let EventKind::Thought { confidence, .. } = fetched.kind else {
        panic!("expected a thought event, got {:?}", fetched.kind);
    };
    assert!(
        confidence.is_finite(),
        "a null must read back as a finite number, got {confidence}"
    );
}

/// A payload that is missing a field is still an error.
///
/// The repair is for a malformed number, not for an absent one: a number
/// nobody wrote must not be invented, or the log would claim a fact about
/// the mind that the mind never had.
#[test]
fn a_payload_row_with_a_missing_field_still_errors() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    write_raw_row(
        &log,
        HAND_WRITTEN_ID,
        r#"{"kind":"thought","text_ref":"t"}"#,
    );

    let failure = log
        .event_at(1)
        .expect_err("a missing field must still fail the row");
    let chain = error_chain(&failure);
    assert!(
        chain.contains("missing field `confidence`"),
        "a missing confidence must still be reported as missing, got {chain}"
    );
}

/// A salience that is not finite must never reach the log.
///
/// SQLite cannot store a NaN at all: it turns it into `NULL`, which the
/// `NOT NULL` constraint on the column refuses. The step the mind took would
/// be lost with it.
#[test]
fn a_not_a_number_salience_is_written_finite_and_the_row_reads_back() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    let event = log
        .append(
            EventKind::Responded {
                answer_ref: "a".into(),
            },
            None,
            None,
            f32::NAN,
        )
        .expect("append");

    let salience: f64 = log
        .conn_for_test()
        .query_row(
            "SELECT salience FROM event WHERE seq = ?1",
            rusqlite::params![event.seq],
            |row| row.get(0),
        )
        .expect("the salience must be readable");
    assert!(
        salience.is_finite(),
        "the salience in the log must be finite, got {salience}"
    );

    let fetched = log.event_at(event.seq).expect("event_at").expect("present");
    assert!(
        fetched.salience.is_finite(),
        "the salience must read back finite, got {}",
        fetched.salience
    );
}

/// An affect axis that is not finite must never reach the log.
///
/// A NaN axis is stored as `NULL` by SQLite, so the affect state would be
/// dropped from the row without a sound — the state would be gone from the
/// only record of it.
#[test]
fn a_not_a_number_affect_axis_is_written_finite_and_the_affect_is_still_present() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    let event = log
        .append(
            EventKind::Felt {
                delta: [0.1, 0.0, 0.0],
                after: [0.2, -0.1, 0.3],
                label: "calm".into(),
            },
            None,
            Some(AffectSnapshot {
                valence: f32::NAN,
                arousal: 0.1,
                tension: 0.2,
            }),
            0.5,
        )
        .expect("append");

    let valence: f64 = log
        .conn_for_test()
        .query_row(
            "SELECT valence FROM event WHERE seq = ?1",
            rusqlite::params![event.seq],
            |row| row.get(0),
        )
        .expect("the valence must be readable");
    assert!(
        valence.is_finite(),
        "the valence in the log must be finite, got {valence}"
    );

    let fetched = log.event_at(event.seq).expect("event_at").expect("present");
    let snapshot = fetched
        .affect
        .expect("the affect state must still be present in the row");
    assert!(
        snapshot.valence.is_finite(),
        "the valence must read back finite, got {}",
        snapshot.valence
    );
    assert_eq!(snapshot.arousal, 0.1, "the other axes must be untouched");
    assert_eq!(snapshot.tension, 0.2, "the other axes must be untouched");
}

/// The wire format of a finite event must not drift.
///
/// A log written today has to read identically tomorrow, so the payload
/// column is pinned to the exact bytes it has always held.
#[test]
fn the_wire_format_for_a_finite_event_is_unchanged() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    let thought = log
        .append(
            EventKind::Thought {
                text_ref: "t".into(),
                confidence: 0.5,
            },
            None,
            None,
            0.5,
        )
        .expect("append");
    assert_eq!(
        payload_of(&log, thought.seq),
        r#"{"kind":"thought","text_ref":"t","confidence":0.5}"#,
        "the thought payload must not drift"
    );

    let felt = log
        .append(
            EventKind::Felt {
                delta: [0.1, 0.0, -0.5],
                after: [0.2, -0.1, 0.3],
                label: "calm".into(),
            },
            None,
            None,
            0.5,
        )
        .expect("append");
    assert_eq!(
        payload_of(&log, felt.seq),
        r#"{"kind":"felt","delta":[0.1,0.0,-0.5],"after":[0.2,-0.1,0.3],"label":"calm"}"#,
        "the felt payload must not drift"
    );
}

/// A partially lost affect snapshot must not be discarded.
///
/// A pre-fix build wrote a NaN axis as `NULL`, so a row can hold some of its
/// axes and not the others. The axes that are there are real values, and the
/// ones that are missing are repaired rather than thrown away with the rest.
#[test]
fn a_partially_lost_affect_snapshot_is_kept() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    log.conn_for_test()
        .execute(
            "INSERT INTO event (id, ts_us, turn_id, payload, valence, arousal, tension, salience)
             VALUES (?1, 0, NULL, ?2, NULL, 0.1, 0.2, 0.0)",
            rusqlite::params![
                HAND_WRITTEN_ID,
                r#"{"kind":"felt","delta":[0.1,0.0,0.0],"after":[0.2,-0.1,0.3],"label":"calm"}"#
            ],
        )
        .expect("the raw row must be writable");

    let fetched = log.event_at(1).expect("event_at").expect("present");
    let affect = fetched
        .affect
        .expect("the affect state must not be discarded");
    assert_eq!(
        affect.valence, 0.0,
        "the missing axis is repaired to the neutral zero"
    );
    assert_eq!(affect.arousal, 0.1, "the axes that are there are kept");
    assert_eq!(affect.tension, 0.2, "the axes that are there are kept");
}

/// A row with no affect at all records none.
///
/// Three `NULL`s is what the store writes for an event that carries no
/// affect state, so it is not damage.
#[test]
fn a_row_with_no_affect_at_all_records_none() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    write_raw_row(
        &log,
        HAND_WRITTEN_ID,
        r#"{"kind":"thought","text_ref":"t","confidence":0.5}"#,
    );

    let fetched = log.event_at(1).expect("event_at").expect("present");
    assert!(
        fetched.affect.is_none(),
        "a row that records no affect state must read back with none"
    );
}

/// A repaired row is as append-only as any other.
///
/// The repair happens before the row is written, and it never goes through an
/// `UPDATE` or a `DELETE`: the trigger still refuses both, and a `REPLACE`
/// still cannot overwrite the row in place.
#[test]
fn a_repaired_row_is_still_append_only() {
    let (_dir, path) = temp_db();
    let log = Log::open(&path).expect("open");
    let event = log
        .append(
            EventKind::Felt {
                delta: [f32::NAN, 0.0, 0.0],
                after: [0.0, 0.0, 0.0],
                label: "calm".into(),
            },
            None,
            Some(AffectSnapshot {
                valence: f32::NAN,
                arousal: f32::INFINITY,
                tension: 0.2,
            }),
            f32::NAN,
        )
        .expect("append");

    let update = log
        .conn_for_test()
        .execute("UPDATE event SET salience = 1.0", []);
    assert!(update.is_err(), "UPDATE on the event table must be denied");

    let delete = log.conn_for_test().execute("DELETE FROM event", []);
    assert!(delete.is_err(), "DELETE on the event table must be denied");

    let replaced = log.conn_for_test().execute(
        "INSERT OR REPLACE INTO event (id, ts_us, payload, salience) VALUES (?1, 0, 'rewritten', 0.0)",
        rusqlite::params![event.id.0.to_string()],
    );
    assert!(
        replaced.is_err(),
        "a REPLACE must not overwrite a repaired event"
    );

    let fetched = log.event_at(event.seq).expect("event_at").expect("present");
    assert_eq!(
        fetched, event,
        "the repaired event must survive the attempt to change it"
    );
}
