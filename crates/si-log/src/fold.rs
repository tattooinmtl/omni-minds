// Rust guideline compliant 2026-03-30

//! Deriving state from the log.
//!
//! A `Project` is a fold over events. Because the log is append-only and
//! totally ordered, folding to sequence N reproduces exactly the state the
//! mind had at that moment — which is what makes "go back to where I was"
//! exact rather than approximate.

use crate::event::Event;
use crate::store::{Log, LogError};

/// The sequence number the first event of a log carries.
///
/// SQLite's `AUTOINCREMENT` hands out 1 for the first row, and the
/// append-only triggers mean a row can never be removed to make room for
/// another, so a fold always starts here.
const FIRST_EVENT_SEQ: u64 = 1;

/// How many events one page of a fold reads at a time.
///
/// Folding reads the whole log in pages rather than in one query, so a long
/// log does not have to fit in memory at once. The size only trades queries
/// against memory; it does not change the result.
const FOLD_PAGE_SIZE: u32 = 512;

/// Folds a stream of events into a state.
pub trait Project {
    /// The state this projection produces.
    type State;
    /// Folds one event into the accumulator.
    fn apply(&mut self, event: &Event);
    /// Produces the final state.
    fn finish(self) -> Self::State;
}

impl Log {
    /// Folds every event up to and including `seq` through `projector`.
    ///
    /// Passing a sequence beyond the end folds the whole log.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::Sqlite`] when a page of events cannot be read, or
    /// when a stored row cannot be turned back into an [`Event`] — a payload
    /// that is not valid JSON for the event vocabulary, or an id that is not
    /// a UUID.
    pub fn fold_to<P: Project>(&self, seq: u64, mut projector: P) -> Result<P::State, LogError> {
        let mut cursor = FIRST_EVENT_SEQ;
        loop {
            let page = self.events(cursor, FOLD_PAGE_SIZE)?;
            if page.is_empty() {
                break;
            }
            // The page is iterated by reference so it is still available for
            // the length check below.
            for event in &page {
                if event.seq > seq {
                    return Ok(projector.finish());
                }
                projector.apply(event);
                cursor = event.seq + 1;
            }
            // The cast is lossless: the page size fits in a `usize` on every
            // target Rust supports.
            if page.len() < FOLD_PAGE_SIZE as usize {
                break;
            }
        }
        Ok(projector.finish())
    }
}
