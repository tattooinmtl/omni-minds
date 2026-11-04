// Rust guideline compliant 2026-03-30

//! The affect engine: a continuous three-axis state, labelled by a lexicon of
//! 250 feelings, whose labels move real behaviour.

pub mod affect;
pub mod bias;

pub use affect::Affect;
pub use bias::{CognitionParameter, CognitionParameters};
