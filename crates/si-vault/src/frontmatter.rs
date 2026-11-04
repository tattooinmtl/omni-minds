// Rust guideline compliant 2026-03-30

//! YAML frontmatter, split from the markdown body by a `---` fence.

use serde_yaml::Value;

/// Splits a document into its frontmatter block and its body.
///
/// Returns `Ok(None, whole_document)` when there is no frontmatter, which is
/// how plain markdown notes stay valid notes.
pub fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let trimmed = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = trimmed.strip_prefix("---\n") else {
        return (None, trimmed);
    };
    match rest.split_once("\n---\n") {
        Some((front, body)) => (Some(front), body),
        // A document that opens with a fence but never closes it is treated as
        // having no frontmatter rather than as an error: a half-written note
        // must still be readable.
        None => (None, trimmed),
    }
}

/// Parses a frontmatter block into a YAML mapping.
pub fn parse_frontmatter(front: &str) -> Result<Value, serde_yaml::Error> {
    serde_yaml::from_str(front)
}

/// Renders a YAML mapping back into a frontmatter block.
pub fn render_frontmatter(value: &Value) -> String {
    let body = serde_yaml::to_string(value).unwrap_or_default();
    format!("---\n{}---\n", body)
}
