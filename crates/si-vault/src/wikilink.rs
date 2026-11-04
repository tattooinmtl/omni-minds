// Rust guideline compliant 2026-03-30

//! `[[wikilink]]` extraction.
//!
//! Fenced code blocks are skipped: a link mentioned inside a code sample is
//! documentation, not a relationship, and indexing it would invent edges the
//! agent never intended.
//!
//! A fence is closed only by a fence of the same character and at least the
//! same length, as CommonMark requires. A code sample that itself shows a
//! fence — which is exactly what a note *about* markdown looks like — must
//! not close the block early, and the links after the block must still be
//! found.

use regex::Regex;
use std::sync::OnceLock;

/// The pattern that recognises a wikilink.
///
/// Group 1 is the target, group 2 the optional `|label`. The target may not
/// contain `]` or `|`, so `[[a|b|c]]` reads as the target `a` with the label
/// `b|c` rather than as two nested links.
const WIKILINK_PATTERN: &str = r"\[\[([^\]\|]+)(?:\|([^\]]+))?\]\]";

/// The fewest fence markers that open a fenced code block.
///
/// CommonMark asks for three, and a shorter run of backticks is inline code
/// rather than a fence.
const MIN_FENCE_MARKERS: usize = 3;

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

/// Strips an anchor from a link target.
///
/// An anchor names a place inside the target note, not another note:
/// `[[note#heading]]` is a link to `note`, and Obsidian writes that form
/// natively. Left in place it would be an edge that can never resolve.
///
/// The body scanner and the graph builder share this, so the two routes
/// cannot drift apart and produce different edges for the same link.
pub(crate) fn target_without_anchor(raw: &str) -> &str {
    raw.split('#').next().unwrap_or(raw).trim()
}

/// Extracts every wikilink in `text`, skipping fenced code blocks.
///
/// The text is scanned line by line so that a fence can be tracked, and the
/// line endings are whatever the note was authored with: a note written on
/// Windows keeps its CRLF prose and still yields the same links.
pub fn extract_wikilinks(text: &str) -> Vec<Wikilink> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        match fence {
            Some((marker, length)) => {
                if closes_fence(trimmed, marker, length) {
                    fence = None;
                }
                continue;
            }
            None => {
                if let Some(opened) = opens_fence(trimmed) {
                    fence = Some(opened);
                    continue;
                }
            }
        }
        for capture in pattern().captures_iter(line) {
            let raw = capture
                .get(1)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            let target = target_without_anchor(&raw).to_string();
            if target.is_empty() {
                continue;
            }
            let label = capture.get(2).map(|m| m.as_str().trim().to_string());
            out.push(Wikilink { target, label });
        }
    }
    out
}

/// Whether a line opens a fenced code block, and with what.
///
/// The answer is the marker character and how many of them the line starts
/// with, because a closing fence has to match both.
fn opens_fence(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let count = line.chars().take_while(|c| *c == marker).count();
    if count < MIN_FENCE_MARKERS {
        return None;
    }
    // A backtick fence's info string may not contain a backtick.
    if marker == '`' && line[count..].contains('`') {
        return None;
    }
    Some((marker, count))
}

/// Whether a line closes an open fence.
///
/// The closing fence must use the same character and at least as many of them,
/// and nothing else. A shorter run of markers, or one followed by text, is
/// part of the code sample.
fn closes_fence(line: &str, marker: char, length: usize) -> bool {
    let trimmed = line.trim();
    let count = trimmed.chars().take_while(|c| *c == marker).count();
    count >= length && trimmed.chars().all(|c| c == marker)
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

    #[test]
    fn a_fence_is_closed_only_by_a_fence_of_the_same_kind() {
        // A code sample that itself shows a fence must not close the block
        // early, and the links after the block must still be found.
        let text = "```\n```rust\n[[not/a/link]]\n```\n[[real/link]]";
        let links = extract_wikilinks(text);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "real/link");
    }

    #[test]
    fn an_anchor_names_a_place_inside_the_target_note() {
        let text = "See [[beliefs/rust#history]] and [[skills/x]].";
        let links = extract_wikilinks(text);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "beliefs/rust");
        assert_eq!(links[1].target, "skills/x");
    }
}
