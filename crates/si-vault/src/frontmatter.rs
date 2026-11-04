// Rust guideline compliant 2026-03-30

//! YAML frontmatter, split from the markdown body by a `---` fence.

use serde_yaml::Value;

/// Splits a document into its frontmatter block and its body.
///
/// The fence is matched line by line rather than by a literal `"---\n"`, so a
/// note whose lines end with CRLF is recognised exactly like one whose lines
/// end with LF. Matching the fence by literal text silently loses the
/// frontmatter of every CRLF note: the id falls back to the file name, the
/// tags come back empty, and the YAML block becomes body prose.
///
/// Returns `Ok(None, whole_document)` when there is no frontmatter, which is
/// how plain markdown notes stay valid notes. A document that opens with a
/// fence but never closes it is treated the same way: a half-written note must
/// still be readable.
pub fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let trimmed = text.strip_prefix('\u{feff}').unwrap_or(text);
    // The opening fence has to be the first line of the document.
    let Some(first_end) = trimmed.find('\n') else {
        return (None, trimmed);
    };
    if !is_fence(&trimmed[..first_end]) {
        return (None, trimmed);
    }
    let front_start = first_end + 1;
    let mut offset = front_start;
    loop {
        let Some(relative_end) = trimmed[offset..].find('\n') else {
            // The last line has no line ending, so it cannot close the block.
            return (None, trimmed);
        };
        let line_start = offset;
        let line_end = offset + relative_end;
        offset = line_end + 1;
        if is_fence(&trimmed[line_start..line_end]) {
            let front = &trimmed[front_start..line_start];
            // A CRLF document leaves the carriage return of the closing line's
            // ending at the end of the frontmatter block. Drop it, so the YAML
            // parser never sees a line break that is not one.
            let front = front.strip_suffix('\r').unwrap_or(front);
            return (Some(front), &trimmed[offset..]);
        }
    }
}

/// Whether a line is a frontmatter fence.
///
/// The line ending and any trailing spaces are ignored, so `---`, `--- ` and
/// `---\r` are all the same fence.
fn is_fence(line: &str) -> bool {
    line.trim_end() == "---"
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
