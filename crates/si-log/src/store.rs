// Rust guideline compliant 2026-03-30

//! The append-only store. Nothing here ever mutates or deletes a row.
//!
//! The `event` table carries triggers that abort `UPDATE` and `DELETE`, so a
//! correction is a new event rather than an edit. Everything downstream — the
//! aperture, the vault, the viewer — reads from this store, and this store is
//! the only source of truth.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;
use uuid::Uuid;

use crate::event::{AffectSnapshot, Event, EventId, EventKind, TurnId};
use crate::finite::{
    axis_marker, finite, reading_payload, report_payload_repair, Original, Repair,
    NON_FINITE_DEFAULT,
};

/// Failures the append-only store can produce.
#[derive(Debug, Error)]
pub enum LogError {
    /// The underlying SQLite call failed, or a file could not be prepared.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// An event id was already taken by a different event.
    ///
    /// Ids are UUIDv7, so a collision means the id generator or the caller
    /// handed out an id that is already in the log.
    #[error("event id collision: {0}")]
    IdCollision(Uuid),
    /// The requested sequence number is not in the log.
    #[error("event {0} does not exist")]
    NotFound(u64),
}

/// The event log. Append-only by construction: the `event` table denies UPDATE
/// and DELETE with triggers, so a correction is a new event rather than an edit.
#[derive(Debug)]
pub struct Log {
    conn: Connection,
}

/// Schema applied on open. Kept as a constant so it can be asserted against.
///
/// There is no `kind` column: the variant tag lives inside the `payload` JSON
/// (`{"kind":"noticed", ...}`), so the kind cannot disagree with the payload
/// that carries it. Filter by kind in SQL with
/// `json_extract(payload, '$.kind') = 'noticed'`.
///
/// The `event_no_replace` trigger closes the one hole the UPDATE and DELETE
/// triggers leave: `INSERT OR REPLACE` deletes the row it conflicts with
/// *without* firing the delete triggers, so an existing id could otherwise be
/// overwritten in place. The trigger refuses the insert instead.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS event (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    id         TEXT    NOT NULL UNIQUE,
    ts_us      INTEGER NOT NULL,
    turn_id    TEXT,
    payload    TEXT    NOT NULL,
    valence    REAL,
    arousal    REAL,
    tension    REAL,
    salience   REAL    NOT NULL DEFAULT 0.0
);
CREATE INDEX IF NOT EXISTS event_turn_idx ON event(turn_id);
CREATE TRIGGER IF NOT EXISTS event_no_update BEFORE UPDATE ON event
BEGIN SELECT RAISE(ABORT, 'event log is append-only'); END;
CREATE TRIGGER IF NOT EXISTS event_no_delete BEFORE DELETE ON event
BEGIN SELECT RAISE(ABORT, 'event log is append-only'); END;
CREATE TRIGGER IF NOT EXISTS event_no_replace BEFORE INSERT ON event
WHEN EXISTS (SELECT 1 FROM event WHERE id = NEW.id)
BEGIN SELECT RAISE(ABORT, 'event log is append-only'); END;
"#;

impl Log {
    /// Opens or creates the log at `path`, applying the schema.
    ///
    /// The parent directory is created when it does not exist, so a fresh
    /// path such as `~/.si-mind/mind.db` opens without a separate setup step.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::Sqlite`] when the directory cannot be created, the
    /// database file cannot be opened, or the schema cannot be applied.
    pub fn open(path: &Path) -> Result<Self, LogError> {
        if let Some(parent) = path.parent() {
            // `create_dir_all` succeeds when the directory already exists, and
            // treats the empty parent of a bare file name as nothing to do.
            std::fs::create_dir_all(parent).map_err(io_failure)?;
        }
        let conn = Connection::open(path)?;
        Self::prepare(conn)
    }

    /// Opens a log that lives only in memory. Used by tests.
    ///
    /// The log is gone when the [`Log`] is dropped, so nothing written to it
    /// outlives the process that made it.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::Sqlite`] when the in-memory database cannot be
    /// opened or the schema cannot be applied.
    pub fn open_in_memory() -> Result<Self, LogError> {
        Self::prepare(Connection::open_in_memory()?)
    }

    /// Applies the schema to a freshly opened connection.
    fn prepare(conn: Connection) -> Result<Self, LogError> {
        conn.execute_batch(SCHEMA)?;
        // A REPLACE deletes the row it conflicts with without firing the
        // delete triggers unless recursive triggers are on, which would let a
        // rewritten event slip past them. This log is append-only, so the
        // connection asks SQLite to fire them.
        conn.execute_batch("PRAGMA recursive_triggers = ON;")?;
        Ok(Log { conn })
    }

    /// Appends one event. The sequence number is assigned by SQLite.
    ///
    /// The event is written to the `event` table with the variant tag inside
    /// the `payload` JSON, and the returned [`Event`] carries the sequence
    /// number the row was given.
    ///
    /// Every number the event carries is repaired to a finite value before
    /// the row is written: the log is append-only, so a number that is not
    /// finite would be permanent, and SQLite cannot store a NaN at all. The
    /// event that comes back is the event the log holds, and a repair is
    /// reported as a `log.event.repaired` trace.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::Sqlite`] when the row cannot be written.
    ///
    /// # Panics
    ///
    /// Panics if the event kind cannot be serialized to JSON. Every
    /// [`EventKind`] is plain data, so a failure here is a programming error
    /// rather than a runtime condition.
    pub fn append(
        &self,
        kind: EventKind,
        turn_id: Option<&TurnId>,
        affect: Option<AffectSnapshot>,
        salience: f32,
    ) -> Result<Event, LogError> {
        let id = EventId(Uuid::now_v7());
        let ts_us = unix_micros();
        // The log is append-only, so a number that is not finite would be
        // permanent. It is repaired here, before the row is written, and
        // never after.
        let (kind, mut repairs) = kind.repaired();
        let payload = serde_json::to_string(&kind).expect("event kind serializes");
        let affect = match affect {
            Some(original) => {
                let snapshot = AffectSnapshot {
                    valence: finite(original.valence),
                    arousal: finite(original.arousal),
                    tension: finite(original.tension),
                };
                if let Some(original) = Original::of(original.valence) {
                    repairs.push(Repair {
                        field: "valence",
                        axis: None,
                        original,
                    });
                }
                if let Some(original) = Original::of(original.arousal) {
                    repairs.push(Repair {
                        field: "arousal",
                        axis: None,
                        original,
                    });
                }
                if let Some(original) = Original::of(original.tension) {
                    repairs.push(Repair {
                        field: "tension",
                        axis: None,
                        original,
                    });
                }
                Some(snapshot)
            }
            None => None,
        };
        let (valence, arousal, tension) = match affect {
            Some(a) => (Some(a.valence), Some(a.arousal), Some(a.tension)),
            None => (None, None, None),
        };
        let repaired_salience = finite(salience);
        if let Some(original) = Original::of(salience) {
            repairs.push(Repair {
                field: "salience",
                axis: None,
                original,
            });
        }
        for repair in &repairs {
            tracing::warn!(
                name: "log.event.repaired",
                row = %id.0,
                field = repair.field,
                axis = %axis_marker(repair.axis),
                original = %repair.original,
                "an event was written with a number that was not finite, repaired to a finite one"
            );
        }
        self.conn.execute(
            "INSERT INTO event (id, ts_us, turn_id, payload, valence, arousal, tension, salience)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id.0.to_string(),
                ts_us,
                turn_id.map(|t| t.0.clone()),
                payload,
                valence,
                arousal,
                tension,
                repaired_salience
            ],
        )?;
        // `seq` is the rowid alias, so the row just written already knows its
        // sequence number. Reading it back with `MAX(seq)` would cost a second
        // query on the hottest path in the system — every cognitive step
        // appends — and it becomes a race the moment the connection is shared.
        let seq = self.conn.last_insert_rowid() as u64;
        Ok(Event {
            id,
            seq,
            ts_us,
            turn_id: turn_id.cloned(),
            kind,
            affect,
            salience: repaired_salience,
        })
    }

    /// Fetches one event by sequence number.
    ///
    /// Returns `None` when the log holds no event with that sequence number.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::Sqlite`] when the row cannot be read, or when a
    /// stored row cannot be turned back into an [`Event`] — a payload that is
    /// not valid JSON for the event vocabulary, or an id that is not a UUID.
    pub fn event_at(&self, seq: u64) -> Result<Option<Event>, LogError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, seq, ts_us, turn_id, payload, valence, arousal, tension, salience
                 FROM event WHERE seq = ?1",
                params![seq],
                row_to_event,
            )
            .optional()?;
        Ok(row)
    }

    /// Fetches a page of events starting at `from_seq`, ascending.
    ///
    /// `from_seq` is inclusive and `limit` caps the number of events returned.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::Sqlite`] when the rows cannot be read, or when a
    /// stored row cannot be turned back into an [`Event`].
    pub fn events(&self, from_seq: u64, limit: u32) -> Result<Vec<Event>, LogError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, seq, ts_us, turn_id, payload, valence, arousal, tension, salience
             FROM event WHERE seq >= ?1 ORDER BY seq ASC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![from_seq, limit], row_to_event)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// The sequence number of the most recent event, or 0 when the log is empty.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::Sqlite`] when the query cannot be run.
    pub fn last_seq(&self) -> Result<u64, LogError> {
        let seq: Option<u64> = self
            .conn
            .query_row("SELECT MAX(seq) FROM event", [], |r| r.get(0))
            .optional()?
            .flatten();
        Ok(seq.unwrap_or(0))
    }

    /// Total number of events.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::Sqlite`] when the query cannot be run.
    pub fn count(&self) -> Result<u64, LogError> {
        let n: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM event", [], |r| r.get(0))?;
        Ok(n)
    }

    /// The raw connection, for tests that need to drive SQL directly.
    ///
    /// Exposed only behind the `test-support` feature so a production build
    /// cannot reach the connection and write around the append-only rules.
    #[cfg(feature = "test-support")]
    pub fn conn_for_test(&self) -> &rusqlite::Connection {
        &self.conn
    }
}

/// Reads one row of the `event` table into an [`Event`].
fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    let id: String = row.get(0)?;
    let seq: u64 = row.get(1)?;
    let ts_us: i64 = row.get(2)?;
    let turn_id: Option<String> = row.get(3)?;
    let payload: String = row.get(4)?;
    let valence: Option<f32> = row.get(5)?;
    let arousal: Option<f32> = row.get(6)?;
    let tension: Option<f32> = row.get(7)?;
    let salience: f32 = row.get(8)?;

    let kind: EventKind = read_payload(&id, &payload)?;

    let affect = read_affect(&id, valence, arousal, tension);

    Ok(Event {
        id: EventId(Uuid::parse_str(&id).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                id.len(),
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })?),
        seq,
        ts_us,
        turn_id: turn_id.map(TurnId),
        kind,
        affect,
        salience: read_number(&id, "salience", salience),
    })
}

/// Reads the payload of one row, reporting every repair it makes.
///
/// A payload that holds a `null` where a number belongs is the shape every
/// pre-fix build wrote for a number that was not finite, so the repair is
/// reported with the row it was in: a log that quietly turned one into a
/// zero would be claiming a number the mind never had.
fn read_payload(id: &str, payload: &str) -> Result<EventKind, rusqlite::Error> {
    let (kind, repairs) = reading_payload(|| serde_json::from_str::<EventKind>(payload));
    for repair in &repairs {
        report_payload_repair(Some(id), repair);
    }
    kind.map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            payload.len(),
            rusqlite::types::Type::Text,
            Box::new(e),
        )
    })
}

/// Reads the affect snapshot out of the three columns.
///
/// A row with no affect at all records none, and that is not a fault: the
/// store writes three `NULL`s when an event carries no affect state.
///
/// A row with some but not all of them can only have come from a NaN axis,
/// which SQLite stores as `NULL`. The axes that are there are real values and
/// are kept; the ones that are missing are repaired and reported. A partial
/// snapshot is never discarded.
fn read_affect(
    id: &str,
    valence: Option<f32>,
    arousal: Option<f32>,
    tension: Option<f32>,
) -> Option<AffectSnapshot> {
    match (valence, arousal, tension) {
        (None, None, None) => None,
        (valence, arousal, tension) => Some(AffectSnapshot {
            valence: read_axis(id, "valence", valence),
            arousal: read_axis(id, "arousal", arousal),
            tension: read_axis(id, "tension", tension),
        }),
    }
}

/// Reads one affect axis, which a row may not hold at all.
fn read_axis(id: &str, field: &'static str, value: Option<f32>) -> f32 {
    match value {
        Some(value) => read_number(id, field, value),
        None => {
            report_column_repair(id, field, Original::Missing);
            NON_FINITE_DEFAULT
        }
    }
}

/// Reads one number that is stored, repairing it when it is not finite.
fn read_number(id: &str, field: &'static str, value: f32) -> f32 {
    let repaired = finite(value);
    if repaired != value {
        if let Some(original) = Original::of(value) {
            report_column_repair(id, field, original);
        }
    }
    repaired
}

/// Reports a number that was repaired on the way out of the log.
///
/// Nothing this crate writes is ever not finite, so a number that needs
/// repairing came from a log written before the repair, or from a hand. The
/// repair is reported rather than silent: a log that holds one is telling
/// something about how it was written.
fn report_column_repair(id: &str, field: &'static str, original: Original) {
    tracing::warn!(
        name: "log.column.repaired",
        row = %id,
        field = field,
        original = %original,
        "a stored number was not a number and has been repaired"
    );
}

/// Current wall-clock time in microseconds since the Unix epoch.
fn unix_micros() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

/// Maps a filesystem failure onto the store's error type.
///
/// [`LogError`] carries the three variants the plan fixes, so a failure to
/// create the database directory is reported as the I/O failure it is:
/// `SQLITE_IOERR` with the operating system's message attached.
fn io_failure(err: std::io::Error) -> LogError {
    LogError::Sqlite(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
        Some(err.to_string()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fmt::Debug;
    use std::sync::{Arc, Mutex};

    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::subscriber::{self, Subscriber};
    use tracing::{Event, Metadata};

    /// One event the recorder caught.
    #[derive(Debug, Clone)]
    struct Reported {
        /// The name of the event.
        name: String,
        /// The row the event was about.
        row: Option<String>,
        /// The field the event was about.
        field: Option<String>,
        /// Which axis of the vector, when the field carries one.
        axis: Option<String>,
        /// What the number was before it was repaired.
        original: Option<String>,
        /// The message of the event.
        message: String,
    }

    /// A subscriber that keeps the events it is given.
    ///
    /// The crate has no dependency on a tracing subscriber, so the recorder
    /// is written here rather than pulled in: the store only needs to know
    /// that a repair was reported, and what it said about it.
    #[derive(Clone, Default)]
    struct Recorder {
        events: Arc<Mutex<Vec<Reported>>>,
    }

    impl Recorder {
        /// Every event that was reported.
        fn reported(&self) -> Vec<Reported> {
            self.events
                .lock()
                .expect("the recorder must not be poisoned")
                .clone()
        }

        /// The name of every event that was reported.
        fn names(&self) -> Vec<String> {
            self.reported()
                .into_iter()
                .map(|event| event.name)
                .collect()
        }

        /// The one event with that name.
        fn named(&self, name: &str) -> Reported {
            self.reported()
                .into_iter()
                .find(|event| event.name == name)
                .unwrap_or_else(|| panic!("the {name} event must be reported"))
        }
    }

    impl Subscriber for Recorder {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &Event<'_>) {
            let mut fields = Fields::default();
            event.record(&mut fields);
            // `name: "..."` in the tracing macro is the name of the event, not
            // a field, so the name comes from the metadata.
            self.events
                .lock()
                .expect("the recorder must not be poisoned")
                .push(Reported {
                    name: event.metadata().name().to_string(),
                    row: fields.row,
                    field: fields.field,
                    axis: fields.axis,
                    original: fields.original,
                    message: fields.message,
                });
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    /// The fields of one store event.
    #[derive(Default)]
    struct Fields {
        row: Option<String>,
        field: Option<String>,
        axis: Option<String>,
        original: Option<String>,
        message: String,
    }

    impl Visit for Fields {
        fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
            match field.name() {
                "row" => self.row = Some(format!("{value:?}")),
                "axis" => self.axis = Some(format!("{value:?}")),
                "original" => self.original = Some(format!("{value:?}")),
                "message" => self.message = format!("{value:?}"),
                _ => {}
            }
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            match field.name() {
                "row" => self.row = Some(value.to_string()),
                "field" => self.field = Some(value.to_string()),
                "message" => self.message = value.to_string(),
                _ => {}
            }
        }
    }

    /// An id for a row written by hand.
    ///
    /// The log never edits a row, so the only way a damaged row gets into it
    /// is by writing it directly.
    const HAND_WRITTEN_ID: &str = "0192b3c4-5d6e-7f80-91a2-b3c4d5e6f708";

    /// Writes one raw row into the log at `path`, the way a hand would.
    ///
    /// The store's own connection is only exposed to the integration tests,
    /// so the row is written through a second connection to the same file.
    fn write_raw_row(
        path: &std::path::Path,
        id: &str,
        payload: &str,
        valence: Option<f32>,
        arousal: Option<f32>,
        tension: Option<f32>,
    ) {
        let conn = rusqlite::Connection::open(path).expect("the connection must open");
        conn.execute(
            "INSERT INTO event (id, ts_us, turn_id, payload, valence, arousal, tension, salience)
             VALUES (?1, 0, NULL, ?2, ?3, ?4, ?5, 0.0)",
            rusqlite::params![id, payload, valence, arousal, tension],
        )
        .expect("the raw row must be writable");
    }

    /// A log on disk, for the tests that need a row written by hand.
    fn log_on_disk() -> (tempfile::TempDir, std::path::PathBuf, Log) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mind.db");
        let log = Log::open(&path).expect("open");
        (dir, path, log)
    }

    /// A repair is reported, not swallowed.
    ///
    /// A number that is not finite is a fault in whatever produced it, and a
    /// fault that is repaired silently is a fault that happens again.
    #[test]
    fn a_repair_is_reported_rather_than_silent() {
        let log = Log::open_in_memory().expect("open");
        let recorder = Recorder::default();
        subscriber::with_default(recorder.clone(), || {
            log.append(
                EventKind::Thought {
                    text_ref: "t".into(),
                    confidence: f32::NAN,
                },
                None,
                Some(AffectSnapshot {
                    valence: f32::NAN,
                    arousal: 0.1,
                    tension: 0.2,
                }),
                f32::NAN,
            )
            .expect("append");
        });
        let names = recorder.names();
        assert!(
            names.contains(&"log.event.repaired".to_string()),
            "a repair must be reported, got {names:?}"
        );
    }

    /// A repair on the way in names the row and what the number was.
    ///
    /// `+Inf` and `-Inf` are both repaired to the same finite value, so only
    /// the report can tell them apart.
    #[test]
    fn a_write_repair_names_the_row_and_what_the_number_was() {
        let log = Log::open_in_memory().expect("open");
        let recorder = Recorder::default();
        let event = subscriber::with_default(recorder.clone(), || {
            log.append(
                EventKind::Thought {
                    text_ref: "t".into(),
                    confidence: f32::NEG_INFINITY,
                },
                None,
                None,
                0.5,
            )
            .expect("append")
        });
        let row = event.id.0.to_string();
        let repair = recorder.named("log.event.repaired");
        assert_eq!(
            repair.row.as_deref(),
            Some(row.as_str()),
            "the repair must name the row it belongs to"
        );
        assert_eq!(repair.field.as_deref(), Some("confidence"));
        assert_eq!(
            repair.original.as_deref(),
            Some("negative infinity"),
            "the report must say which infinity it was"
        );
    }

    /// A repair on the way out of the log names the row and the field.
    ///
    /// A payload that holds a `null` where a number belongs is the shape
    /// every pre-fix build wrote for a number that was not finite, so this
    /// is the common case rather than an edge case.
    #[test]
    fn a_payload_repair_names_the_row_and_the_field() {
        let (_dir, path, log) = log_on_disk();
        write_raw_row(
            &path,
            HAND_WRITTEN_ID,
            r#"{"kind":"thought","text_ref":"t","confidence":null}"#,
            None,
            None,
            None,
        );
        let recorder = Recorder::default();
        subscriber::with_default(recorder.clone(), || {
            log.event_at(1).expect("event_at").expect("present");
        });
        let repair = recorder.named("log.payload.repaired");
        assert_eq!(
            repair.row.as_deref(),
            Some(HAND_WRITTEN_ID),
            "the repair must name the row it belongs to"
        );
        assert_eq!(repair.field.as_deref(), Some("confidence"));
        assert_eq!(
            repair.original.as_deref(),
            Some("null"),
            "the report must say the payload held a null"
        );
        assert!(
            repair.message.contains("repaired"),
            "the report must say what happened, got {}",
            repair.message
        );
    }

    /// A repair inside a vector names the axis it was in.
    ///
    /// A vector carries three numbers, and a repair of one of them has to say
    /// which one.
    #[test]
    fn a_payload_repair_names_the_axis_it_was_in() {
        let (_dir, path, log) = log_on_disk();
        write_raw_row(
            &path,
            HAND_WRITTEN_ID,
            r#"{"kind":"felt","delta":[0.1,null,0.0],"after":[0.2,-0.1,0.3],"label":"calm"}"#,
            None,
            None,
            None,
        );
        let recorder = Recorder::default();
        subscriber::with_default(recorder.clone(), || {
            log.event_at(1).expect("event_at").expect("present");
        });
        let repair = recorder.named("log.payload.repaired");
        assert_eq!(repair.field.as_deref(), Some("delta"));
        assert_eq!(
            repair.axis.as_deref(),
            Some("1"),
            "the report must say which axis was repaired"
        );
        assert_eq!(repair.original.as_deref(), Some("null"));
    }

    /// A partially lost affect snapshot is kept, and the loss is reported.
    ///
    /// A pre-fix build wrote a NaN axis as `NULL`, so a row can hold some of
    /// its axes and not the others. The axes that are there are real values
    /// and are kept; the ones that are missing are repaired and reported.
    #[test]
    fn a_partially_lost_affect_snapshot_is_kept_and_reported() {
        let (_dir, path, log) = log_on_disk();
        write_raw_row(
            &path,
            HAND_WRITTEN_ID,
            r#"{"kind":"felt","delta":[0.1,0.0,0.0],"after":[0.2,-0.1,0.3],"label":"calm"}"#,
            None,
            Some(0.1),
            Some(0.2),
        );
        let recorder = Recorder::default();
        let fetched = subscriber::with_default(recorder.clone(), || {
            log.event_at(1).expect("event_at").expect("present")
        });
        let affect = fetched
            .affect
            .expect("the affect state must not be discarded");
        assert_eq!(
            affect.valence, 0.0,
            "the missing axis is repaired to the neutral zero"
        );
        assert_eq!(affect.arousal, 0.1, "the axes that are there are kept");
        assert_eq!(affect.tension, 0.2, "the axes that are there are kept");
        let repair = recorder.named("log.column.repaired");
        assert_eq!(
            repair.row.as_deref(),
            Some(HAND_WRITTEN_ID),
            "the repair must name the row it belongs to"
        );
        assert_eq!(repair.field.as_deref(), Some("valence"));
        assert_eq!(
            repair.original.as_deref(),
            Some("missing"),
            "the report must say the axis was missing"
        );
    }

    /// A row with no affect at all records none, and says nothing.
    ///
    /// Three `NULL`s is what the store writes for an event that carries no
    /// affect state, so it is not damage and must not be reported.
    #[test]
    fn a_row_with_no_affect_at_all_is_not_reported() {
        let (_dir, path, log) = log_on_disk();
        write_raw_row(
            &path,
            HAND_WRITTEN_ID,
            r#"{"kind":"thought","text_ref":"t","confidence":0.5}"#,
            None,
            None,
            None,
        );
        let recorder = Recorder::default();
        let fetched = subscriber::with_default(recorder.clone(), || {
            log.event_at(1).expect("event_at").expect("present")
        });
        assert!(
            fetched.affect.is_none(),
            "a row with three null axes records no affect state"
        );
        assert!(
            recorder.names().is_empty(),
            "a row that records no affect state is not damage, got {:?}",
            recorder.names()
        );
    }

    /// An event that needs no repair says nothing.
    #[test]
    fn an_event_that_needs_no_repair_says_nothing() {
        let log = Log::open_in_memory().expect("open");
        let recorder = Recorder::default();
        subscriber::with_default(recorder.clone(), || {
            log.append(
                EventKind::Thought {
                    text_ref: "t".into(),
                    confidence: 0.5,
                },
                None,
                Some(AffectSnapshot {
                    valence: 0.1,
                    arousal: 0.2,
                    tension: 0.3,
                }),
                0.5,
            )
            .expect("append");
        });
        let names = recorder.names();
        assert!(
            names.is_empty(),
            "a finite event needs no repair, got {names:?}"
        );
    }
}
