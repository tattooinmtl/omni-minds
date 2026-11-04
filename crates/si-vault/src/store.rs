// Rust guideline compliant 2026-03-30

//! The vault on disk.

use std::path::{Path, PathBuf};

use crate::graph::{graph, Edge};
use crate::note::{parse_note, render_note, Note};
use crate::VaultError;

/// The folder the agent's own self-authored journals live in. Only the journal
/// writer may write here; everything else is refused, because a seed written
/// into `self/` would make his personality ours rather than his.
const SELF_DIR: &str = "self";

/// The prefix that puts a note inside `self/`.
const SELF_DIR_PREFIX: &str = concat!("self", "/");

/// Whether an id names a note inside `self/`.
///
/// The match is on the folder, not on a prefix: `selfish/notes` is an
/// ordinary folder and must not be refused.
fn is_self_id(id: &str) -> bool {
    let id = id.replace('\\', "/");
    id == SELF_DIR || id.starts_with(SELF_DIR_PREFIX)
}

/// Whether an id can be turned into a path inside the vault.
///
/// A note id is a `<folder>/<slug>` path relative to the vault root. An id
/// that walks out of the vault — with a `..` segment, or by being absolute —
/// would put a note outside the mind's own folder, so it is refused.
fn id_stays_inside_the_vault(id: &str) -> bool {
    let id = id.replace('\\', "/");
    !id.starts_with('/')
        && !id
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
}

/// A directory of markdown notes.
#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
}

impl Vault {
    /// Opens (and creates) a vault rooted at `root`.
    pub fn open(root: &Path) -> Result<Self, VaultError> {
        std::fs::create_dir_all(root).map_err(|source| VaultError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        Ok(Vault {
            root: root.to_path_buf(),
        })
    }

    /// The vault root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Writes a note, creating parent folders as needed.
    ///
    /// Refuses to write into `self/`; see `SELF_DIR`.
    pub fn write_note(&self, note: &Note) -> Result<(), VaultError> {
        if is_self_id(&note.id) {
            return Err(VaultError::SelfWriteRefused {
                action: "write",
                id: note.id.clone(),
            });
        }
        if !id_stays_inside_the_vault(&note.id) {
            return Err(VaultError::UnsafeId(note.id.clone()));
        }
        let path = self.path_for(&note.id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| VaultError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(&path, render_note(note)).map_err(|source| VaultError::Io { path, source })
    }

    /// Reads a note by id.
    ///
    /// Reading `self/` is allowed: the personality code reads the agent's own
    /// journals. Reading outside the vault is not.
    pub fn read_note(&self, id: &str) -> Result<Option<Note>, VaultError> {
        if !id_stays_inside_the_vault(id) {
            return Err(VaultError::UnsafeId(id.to_string()));
        }
        let path = self.path_for(id);
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path).map_err(|source| VaultError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(Some(parse_note(&text, &path)?))
    }

    /// Deletes a note. Returns whether anything was removed.
    ///
    /// Refuses to delete from `self/`: the journal writer owns those notes, and
    /// deleting one would destroy the record the personality is derived from.
    pub fn delete_note(&self, id: &str) -> Result<bool, VaultError> {
        if is_self_id(id) {
            return Err(VaultError::SelfWriteRefused {
                action: "delete",
                id: id.to_string(),
            });
        }
        if !id_stays_inside_the_vault(id) {
            return Err(VaultError::UnsafeId(id.to_string()));
        }
        let path = self.path_for(id);
        if !path.exists() {
            return Ok(false);
        }
        std::fs::remove_file(&path).map_err(|source| VaultError::Io { path, source })?;
        Ok(true)
    }

    /// Every note in the vault, sorted by id.
    pub fn notes(&self) -> Result<Vec<Note>, VaultError> {
        let mut out = Vec::new();
        self.walk(&self.root, &mut out)?;
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    /// The note graph over every note in the vault.
    pub fn graph(&self) -> Result<Vec<Edge>, VaultError> {
        Ok(graph(&self.notes()?))
    }

    fn path_for(&self, id: &str) -> PathBuf {
        self.root.join(format!("{}.md", id.replace('\\', "/")))
    }

    fn walk(&self, dir: &Path, out: &mut Vec<Note>) -> Result<(), VaultError> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(source) => {
                // A folder that cannot be read is a gap in the projection, not
                // a reason to lose every other note.
                tracing::warn!(
                    vault.folder.unreadable = %dir.display(),
                    error = %source,
                    "skipping a folder that could not be read"
                );
                return Ok(());
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.walk(&path, out)?;
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(source) => {
                    tracing::warn!(
                        vault.note.unreadable = %path.display(),
                        error = %source,
                        "skipping a note that could not be read"
                    );
                    continue;
                }
            };
            match parse_note(&text, &path) {
                Ok(note) => out.push(note),
                Err(source) => {
                    // One unreadable note must not make the whole vault
                    // unreadable: the mind would lose access to everything it
                    // knows because of a single bad file. The file is still on
                    // disk, and the log remains the source of truth.
                    tracing::warn!(
                        vault.note.unparsable = %path.display(),
                        error = %source,
                        "skipping a note that could not be parsed"
                    );
                }
            }
        }
        Ok(())
    }
}
