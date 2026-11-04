// Rust guideline compliant 2026-03-30

//! Append-only event log: the single source of truth for the mind.

pub mod event;
pub mod fold;
pub mod store;

#[doc(inline)]
pub use event::{AffectSnapshot, Event, EventId, EventKind, EvictReason, TurnId, WriteOp};
#[doc(inline)]
pub use fold::Project;
#[doc(inline)]
pub use store::{Log, LogError};

/// The crate version string, inherited from the workspace package version.
///
/// `env!("CARGO_PKG_VERSION")` resolves to the inherited workspace version
/// (`0.1.0` today), so this is the same for every crate in the workspace and
/// changes only on a workspace version bump. It does not identify a build.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
