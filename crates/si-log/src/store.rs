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
        let payload = serde_json::to_string(&kind).expect("event kind serializes");
        let (valence, arousal, tension) = match affect {
            Some(a) => (Some(a.valence), Some(a.arousal), Some(a.tension)),
            None => (None, None, None),
        };
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
                salience
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
            salience,
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

    let kind: EventKind = serde_json::from_str(&payload).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            payload.len(),
            rusqlite::types::Type::Text,
            Box::new(e),
        )
    })?;

    let affect = match (valence, arousal, tension) {
        (Some(v), Some(a), Some(t)) => Some(AffectSnapshot {
            valence: v,
            arousal: a,
            tension: t,
        }),
        _ => None,
    };

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
        salience,
    })
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
