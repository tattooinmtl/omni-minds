// Rust guideline compliant 2026-03-30

//! Append-only event log: the single source of truth for the mind.

/// Crate version, used in the run header so a log can be attributed to a build.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
