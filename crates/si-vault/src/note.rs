// Rust guideline compliant 2026-03-30

//! A note: the vault's unit of memory.

use std::path::{Path, PathBuf};

use serde_yaml::Value;

use crate::frontmatter::{parse_frontmatter, render_frontmatter, split_frontmatter};
use crate::VaultError;

/// A markdown note with YAML frontmatter.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// Stable identifier, normally `<folder>/<slug>`.
    pub id: String,
    pub title: String,
    /// The OKF `type` field: `plan`, `brainstorm`, `belief`, `skill`, …
    pub note_type: String,
    pub tags: Vec<String>,
    /// Explicit links declared in frontmatter.
    pub links: Vec<String>,
    pub body: String,
    /// Frontmatter keys this build does not know about, preserved verbatim.
    pub extra: Value,
    pub path: PathBuf,
}

/// Parses a note from its on-disk text.
pub fn parse_note(text: &str, path: &Path) -> Result<Note, VaultError> {
    let (front, body) = split_frontmatter(text);
    // The frontmatter is parsed once. A vault re-reads its notes constantly,
    // so parsing the same block twice for every note is a real cost.
    let value = match front {
        Some(front) => Some(parse_frontmatter(front)?),
        None => None,
    };
    let mut extra = Value::Mapping(Default::default());
    if let Some(Value::Mapping(map)) = &value {
        for (key, val) in map {
            let key = key.as_str().unwrap_or_default().to_string();
            match key.as_str() {
                "id" | "title" | "type" | "tags" | "links" => {}
                _ => {
                    if let Some(mapping) = extra.as_mapping_mut() {
                        mapping.insert(Value::String(key), val.clone());
                    }
                }
            }
        }
    }
    // The remaining known fields are read from the same parsed mapping.
    let (id, title, note_type, tags, links) = match &value {
        Some(value) => read_known(value),
        None => (
            String::new(),
            String::new(),
            String::new(),
            Vec::new(),
            Vec::new(),
        ),
    };
    let id = if id.is_empty() { path_to_id(path) } else { id };
    let title = if title.is_empty() { id.clone() } else { title };
    // The blank line that separates the frontmatter from the body is not part
    // of the body. Stripping exactly one line ending keeps the body as it was
    // authored and makes `render_note` → `parse_note` an exact round trip. The
    // body keeps its own line endings: a note written on Windows still has
    // CRLF in its prose.
    let body = body
        .strip_prefix("\r\n")
        .or_else(|| body.strip_prefix('\n'))
        .unwrap_or(body);
    Ok(Note {
        id,
        title,
        note_type,
        tags,
        links,
        body: body.to_string(),
        extra,
        path: path.to_path_buf(),
    })
}

/// Renders a note back to on-disk text.
pub fn render_note(note: &Note) -> String {
    let mut mapping = serde_yaml::Mapping::new();
    mapping.insert(Value::String("id".into()), Value::String(note.id.clone()));
    mapping.insert(
        Value::String("title".into()),
        Value::String(note.title.clone()),
    );
    mapping.insert(
        Value::String("type".into()),
        Value::String(note.note_type.clone()),
    );
    mapping.insert(
        Value::String("tags".into()),
        Value::Sequence(note.tags.iter().cloned().map(Value::String).collect()),
    );
    mapping.insert(
        Value::String("links".into()),
        Value::Sequence(note.links.iter().cloned().map(Value::String).collect()),
    );
    if let Some(extra) = note.extra.as_mapping() {
        for (key, value) in extra {
            mapping.insert(key.clone(), value.clone());
        }
    }
    let mut out = render_frontmatter(&Value::Mapping(mapping));
    out.push('\n');
    out.push_str(&note.body);
    out
}

/// Derives a note id from a note's path.
///
/// This is the fallback for a note whose frontmatter carries no `id`. The id
/// is the path **without its extension**, so what the id keeps of the path is
/// the caller's choice: the vault passes a path relative to its own root, so a
/// note at `hand/no-id.md` is addressed as `hand/no-id` and two notes with the
/// same file name in different folders stay two notes. An id the author wrote
/// is never overridden — this is only the fallback.
pub fn path_to_id(path: &Path) -> String {
    path.with_extension("").to_string_lossy().replace('\\', "/")
}

fn read_known(value: &Value) -> (String, String, String, Vec<String>, Vec<String>) {
    let get = |key: &str| -> String {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let list = |key: &str| -> Vec<String> {
        value
            .get(key)
            .and_then(|v| v.as_sequence())
            .map(|s| {
                s.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    (
        get("id"),
        get("title"),
        get("type"),
        list("tags"),
        list("links"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_known_fields_and_preserves_unknown_ones() {
        let text = "---\nid: beliefs/rust\ntitle: Rust\ntype: belief\ntags: [lang, rust]\nlinks: [\"[[skills/x]]\"]\nconfidence: 0.8\n---\n\nBody text.\n";
        let note = parse_note(text, &PathBuf::from("beliefs/rust.md")).expect("parse");
        assert_eq!(note.id, "beliefs/rust");
        assert_eq!(note.title, "Rust");
        assert_eq!(note.note_type, "belief");
        assert_eq!(note.tags, vec!["lang".to_string(), "rust".to_string()]);
        assert_eq!(note.links, vec!["[[skills/x]]".to_string()]);
        assert_eq!(note.body.trim(), "Body text.");
        assert_eq!(
            note.extra.get("confidence").and_then(|v| v.as_f64()),
            Some(0.8)
        );
    }

    #[test]
    fn a_note_without_frontmatter_is_still_a_note() {
        let note = parse_note("Just text.\n", &PathBuf::from("plain.md")).expect("parse");
        assert_eq!(note.id, "plain");
        assert_eq!(note.title, "plain");
        assert!(note.tags.is_empty());
        assert_eq!(note.body.trim(), "Just text.");
    }

    #[test]
    fn rendering_round_trips_through_parsing() {
        let text = "---\nid: beliefs/rust\ntitle: Rust\ntype: belief\ntags: [rust]\nlinks: []\nconfidence: 0.8\n---\n\nBody.\n";
        let note = parse_note(text, &PathBuf::from("beliefs/rust.md")).expect("parse");
        let rendered = render_note(&note);
        let again = parse_note(&rendered, &PathBuf::from("beliefs/rust.md")).expect("parse");
        assert_eq!(note, again);
    }

    #[test]
    fn a_crlf_note_still_has_its_frontmatter() {
        let text = "---\r\nid: beliefs/rust\r\ntype: belief\r\n---\r\n\r\nBody\r\n";
        let note = parse_note(text, &PathBuf::from("elsewhere.md")).expect("parse");
        assert_eq!(
            note.id, "beliefs/rust",
            "the frontmatter must win over the file name"
        );
        assert_eq!(note.note_type, "belief");
        assert!(
            !note.body.contains("id:"),
            "the yaml must not leak into the body: {}",
            note.body
        );
        // The body keeps its own line endings, so a CRLF body must survive the
        // round trip unchanged.
        let rendered = render_note(&note);
        let again = parse_note(&rendered, &PathBuf::from("elsewhere.md")).expect("parse");
        assert_eq!(note, again, "a crlf body must survive the round trip");
        assert!(
            note.body.contains("\r\n"),
            "the body keeps its own line endings"
        );
    }

    #[test]
    fn an_empty_frontmatter_block_is_not_body_prose() {
        let text = "---\n---\n\nBody.\n";
        let note = parse_note(text, &PathBuf::from("elsewhere.md")).expect("parse");
        assert_eq!(note.body.trim(), "Body.");
    }
}
