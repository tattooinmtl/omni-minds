// Rust guideline compliant 2026-03-30

//! The vault on disk.

use std::path::{Component, Path, PathBuf};

use crate::graph::{graph, Edge};
use crate::note::{parse_note, path_to_id, render_note, Note};
use crate::VaultError;

/// The folder the agent's own self-authored journals live in. Only the journal
/// writer may write here; everything else is refused, because a seed written
/// into `self/` would make his personality ours rather than his.
const SELF_DIR: &str = "self";

/// The extension every note in the vault carries.
const NOTE_EXTENSION: &str = "md";

/// How deep the vault may nest before the walk gives up.
///
/// The walk follows directories, and a directory link pointing back at an
/// ancestor would loop forever. The limit turns that from a stack overflow
/// into a skipped folder.
const MAX_FOLDER_DEPTH: usize = 32;

/// Normalises a path segment the way Windows does.
///
/// Windows ignores trailing dots and spaces in a name and compares names
/// without regard to case, so `Self`, `SELF` and `self.` all name the same
/// folder, and `.. ` is a parent-directory segment rather than an ordinary
/// name. Comparing the raw string lets a spelling of `self` past the guard
/// and `.. ` past the traversal check.
fn windows_segment(segment: &str) -> &str {
    segment.trim_end_matches(['.', ' '])
}

/// Whether an id names a note inside `self/`.
///
/// The match is on the vault's own folder, not on a prefix: `selfish/notes`
/// is an ordinary folder and must not be refused. Only the first segment is
/// compared, because `self/` is the folder directly under the vault root —
/// `other/self/notes` is somewhere else entirely.
fn is_self_id(id: &str) -> bool {
    let id = id.replace('\\', "/");
    match id.split('/').next() {
        Some(first) => windows_segment(first).eq_ignore_ascii_case(SELF_DIR),
        None => false,
    }
}

/// Whether an id can be turned into a path inside the vault.
///
/// A note id is a `<folder>/<slug>` path relative to the vault root. An id
/// that walks out of the vault — with a parent-directory segment, or by being
/// absolute in any of the ways Windows allows — would put a note outside the
/// mind's own folder, so it is refused.
fn id_stays_inside_the_vault(id: &str) -> bool {
    let id = id.replace('\\', "/");
    // A path that opens with a drive or a share is absolute, and joining it
    // onto the vault root replaces the root entirely: `C:/note` lands on the
    // C drive and `C:note` in the process's own directory.
    let path = Path::new(&id);
    if matches!(
        path.components().next(),
        Some(Component::Prefix(_)) | Some(Component::RootDir)
    ) {
        return false;
    }
    // A segment that is nothing but dots and spaces is a parent-directory
    // segment to Windows, whatever it looks like.
    !id.split('/')
        .any(|segment| windows_segment(segment).is_empty())
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
    ///
    /// This is the journal writer's route into `self/`: it is the only part of
    /// the vault that may write there, and it reaches the folder directly
    /// rather than through `write_note`, which refuses it. Every other caller
    /// gets a path it may read and display; the guard on `self/` is a policy
    /// of this type, not a filesystem permission.
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
        // Write to a sibling temporary file and rename it over the note. A
        // direct write truncates the note before the new bytes land, so a
        // failure part-way through would leave a half-written file — and a
        // half-written note does not parse, so the walk would drop it from
        // the projection without saying so.
        let temporary = temporary_path_for(&path);
        if let Err(source) = std::fs::write(&temporary, render_note(note)) {
            let _ = std::fs::remove_file(&temporary);
            return Err(VaultError::Io {
                path: temporary,
                source,
            });
        }
        if let Err(source) = std::fs::rename(&temporary, &path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(VaultError::Io { path, source });
        }
        Ok(())
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
        self.walk(&self.root, &mut out, 0)?;
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    /// The note graph over every note in the vault.
    pub fn graph(&self) -> Result<Vec<Edge>, VaultError> {
        Ok(graph(&self.notes()?))
    }

    /// The file a note id lives in.
    ///
    /// An id that already carries the extension is not given a second one.
    fn path_for(&self, id: &str) -> PathBuf {
        let id = id.replace('\\', "/");
        let suffix = format!(".{}", NOTE_EXTENSION);
        let stem = id.strip_suffix(suffix.as_str()).unwrap_or(&id);
        self.root.join(format!("{}.{}", stem, NOTE_EXTENSION))
    }

    /// The id of the note at `path`, taken from where it sits in the vault.
    ///
    /// The path is the note's address: `read_note(id)` looks for
    /// `<root>/<id>.md`. So a note without an `id` of its own is addressed by
    /// its place in the tree, and two notes with the same file name in
    /// different folders stay two notes.
    fn id_for(&self, path: &Path) -> String {
        let relative = path.strip_prefix(&self.root).unwrap_or(path);
        let relative = relative.to_string_lossy().replace('\\', "/");
        let suffix = format!(".{}", NOTE_EXTENSION);
        match relative.strip_suffix(suffix.as_str()).map(str::to_string) {
            Some(stem) => stem,
            None => relative,
        }
    }

    fn walk(&self, dir: &Path, out: &mut Vec<Note>, depth: usize) -> Result<(), VaultError> {
        if depth > MAX_FOLDER_DEPTH {
            tracing::warn!(path = %dir.display(), depth = depth, "vault.folder.too_deep: the vault nests deeper than the walk follows");
            return Ok(());
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(source) => {
                // A folder that cannot be read is a gap in the projection, not
                // a reason to lose every other note.
                tracing::warn!(path = %dir.display(), error = %source, "vault.folder.unreadable: skipping a folder that could not be read");
                return Ok(());
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(source) => {
                    // An entry that could not even be listed is a gap like any
                    // other: skip it, say so, and keep going.
                    tracing::warn!(path = %dir.display(), error = %source, "vault.entry.unreadable: skipping an entry that could not be listed");
                    continue;
                }
            };
            let path = entry.path();
            if path.is_dir() {
                self.walk(&path, out, depth + 1)?;
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some(NOTE_EXTENSION) {
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(source) => {
                    tracing::warn!(path = %path.display(), error = %source, "vault.note.unreadable: skipping a note that could not be read");
                    continue;
                }
            };
            match parse_note(&text, &path) {
                Ok(mut note) => {
                    // `parse_note` falls back to the file name when a note
                    // carries no `id` of its own, and two notes with the same
                    // name in different folders would then share one. The
                    // vault addresses notes by their place in the tree, so the
                    // fallback becomes the full path. An `id` the author
                    // wrote is left alone.
                    if note.id == path_to_id(&path) {
                        note.id = self.id_for(&path);
                    }
                    out.push(note);
                }
                Err(source) => {
                    // One unreadable note must not make the whole vault
                    // unreadable: the mind would lose access to everything it
                    // knows because of a single bad file. The file is still on
                    // disk, and the log remains the source of truth.
                    tracing::warn!(path = %path.display(), error = %source, "vault.note.unparsable: skipping a note that could not be parsed");
                }
            }
        }
        Ok(())
    }
}

/// The sibling temporary file a note is written through.
///
/// It sits next to the note so the rename stays on one filesystem, and it
/// ends in `.tmp` so the walk ignores it.
fn temporary_path_for(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}
