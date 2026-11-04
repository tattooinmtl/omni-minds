// Rust guideline compliant 2026-03-30

//! `[[wikilink]]` extraction.
//!
//! Fenced code blocks are skipped: a link mentioned inside a code sample is
//! documentation, not a relationship, and indexing it would invent edges the
//! agent never intended.

use regex::Regex;
use std::sync::OnceLock;

/// The pattern that recognises a wikilink.
///
/// Group 1 is the target, group 2 the optional `|label`. The target may not
/// contain `]` or `|`, so `[[a|b|c]]` reads as the target `a` with the label
/// `b|c` rather than as two nested links.
const WIKILINK_PATTERN: &str = r"\[\[([^\]\|]+)(?:\|([^\]]+))?\]\]";

/// The line prefixes that open and close a fenced code block.
///
/// Both the backtick and the tilde fence are recognised, because a note may use
/// either and a link inside the wrong one would silently become an edge. A
/// marker is matched at the start of the line only after leading whitespace is
/// removed, so an indented fence still opens a block.
const FENCE_MARKERS: [&str; 2] = ["```", "~~~"];

/// A `[[target]]` or `[[target|label]]` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wikilink {
    /// The note the link points at, e.g. `beliefs/rust`.
    pub target: String,
    /// The text shown instead of the target, when the link carries one.
    pub label: Option<String>,
}

/// The compiled wikilink pattern.
///
/// The pattern is a constant, so the compiled regex can only ever hold one
/// value and the cached copy cannot disagree with itself across crate
/// versions; the static is a compile-time saving, not shared state.
fn pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(WIKILINK_PATTERN).expect("the wikilink pattern is a constant and must compile")
    })
}

/// Extracts every wikilink in `text`, skipping fenced code blocks.
///
/// The text is scanned line by line so that a fence can be tracked, and the
/// line endings are whatever the note was authored with: a note written on
/// Windows keeps its CRLF prose and still yields the same links.
pub fn extract_wikilinks(text: &str) -> Vec<Wikilink> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if FENCE_MARKERS
            .iter()
            .any(|marker| trimmed.starts_with(marker))
        {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        for capture in pattern().captures_iter(line) {
            let target = capture
                .get(1)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            if target.is_empty() {
                continue;
            }
            let label = capture.get(2).map(|m| m.as_str().trim().to_string());
            out.push(Wikilink { target, label });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_and_labelled_wikilinks() {
        let text = "See [[beliefs/rust]] and [[skills/x|the X skill]].";
        let links = extract_wikilinks(text);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "beliefs/rust");
        assert_eq!(links[0].label, None);
        assert_eq!(links[1].target, "skills/x");
        assert_eq!(links[1].label.as_deref(), Some("the X skill"));
    }

    #[test]
    fn ignores_a_wikilink_inside_a_fenced_code_block() {
        let text = "```\n[[not/a/link]]\n```\n[[real/link]]";
        let links = extract_wikilinks(text);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "real/link");
    }
}
