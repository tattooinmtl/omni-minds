// Rust guideline compliant 2026-03-30

//! The vault: an Obsidian-compatible markdown store of everything the mind knows.

pub mod frontmatter;
pub mod graph;
pub mod note;
pub mod wikilink;

use thiserror::Error;

/// Failures the vault can produce.
#[derive(Debug, Error)]
pub enum VaultError {
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("io error at {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("note {0} not found")]
    NotFound(String),
}

#[doc(inline)]
pub use graph::{graph, Edge, EdgeKind};
#[doc(inline)]
pub use note::{parse_note, render_note, Note};
#[doc(inline)]
pub use wikilink::{extract_wikilinks, Wikilink};
