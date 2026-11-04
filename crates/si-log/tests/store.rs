use si_log::{AffectSnapshot, EventKind, Log, TurnId};

fn temp_db() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mind.db");
    (dir, path)
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
