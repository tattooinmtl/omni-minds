// Rust guideline compliant 2026-03-30

//! Building the note graph from wikilinks, tags and frontmatter.

use crate::note::Note;
use crate::wikilink::{extract_wikilinks, target_without_anchor};

/// The frontmatter key that names the folder a note belongs to.
const PARENT_KEY: &str = "parent";

/// The prefix that marks an edge target as a tag rather than as a note.
///
/// A tag is not a note, so it needs a name that cannot collide with a note id.
const TAG_PREFIX: &str = "tag:";

/// Why two notes are connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// One note refers to another by wikilink or by a declared link.
    Link,
    /// One note is the folder parent of another.
    Hierarchy,
    /// One note carries a tag.
    Tag,
    /// One note claims to be false against another.
    Contradicts,
    /// One note replaces another.
    Supersedes,
    /// One note rests on the evidence of another.
    Grounds,
    /// One note came from another.
    Origin,
}

/// A directed relationship between two notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// The note the relationship starts from.
    pub from: String,
    /// The note the relationship points at.
    pub to: String,
    /// Why the two are related.
    pub kind: EdgeKind,
}

/// Builds every edge implied by a set of notes.
///
/// The `contradicts`, `supersedes`, `grounds` and `origin` kinds come from
/// frontmatter keys rather than from body links, because those relationships
/// are claims about meaning, not references.
pub fn graph(notes: &[Note]) -> Vec<Edge> {
    let mut edges = Vec::new();
    for note in notes {
        for link in extract_wikilinks(&note.body) {
            edges.push(Edge {
                from: note.id.clone(),
                to: link.target,
                kind: EdgeKind::Link,
            });
        }
        for declared in &note.links {
            let target = declared
                .trim()
                .trim_start_matches('[')
                .trim_end_matches(']');
            let target = target.split('|').next().unwrap_or(target).trim();
            // An anchor names a place inside the target note, not another note.
            let target = target_without_anchor(target);
            if !target.is_empty() {
                edges.push(Edge {
                    from: note.id.clone(),
                    to: target.to_string(),
                    kind: EdgeKind::Link,
                });
            }
        }
        for tag in &note.tags {
            edges.push(Edge {
                from: note.id.clone(),
                to: format!("{TAG_PREFIX}{tag}"),
                kind: EdgeKind::Tag,
            });
        }
        if let Some(parent) = note.extra.get(PARENT_KEY).and_then(|v| v.as_str()) {
            edges.push(Edge {
                from: parent.to_string(),
                to: note.id.clone(),
                kind: EdgeKind::Hierarchy,
            });
        }
        for (key, kind) in CLAIM_KEYS {
            if let Some(values) = note.extra.get(key).and_then(|v| v.as_sequence()) {
                for value in values {
                    if let Some(target) = value.as_str() {
                        edges.push(Edge {
                            from: note.id.clone(),
                            to: target.to_string(),
                            kind,
                        });
                    }
                }
            }
        }
    }
    edges
}

/// The frontmatter keys that state a claim about another note.
///
/// The key names are fixed by the note format: a key outside this table is
/// preserved in `Note::extra` but implies no relationship.
const CLAIM_KEYS: [(&str, EdgeKind); 4] = [
    ("contradicts", EdgeKind::Contradicts),
    ("supersedes", EdgeKind::Supersedes),
    ("grounds", EdgeKind::Grounds),
    ("origin", EdgeKind::Origin),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::parse_note;
    use std::path::PathBuf;

    #[test]
    fn frontmatter_claims_become_typed_edges() {
        let text = "---\nid: beliefs/a\ntype: belief\ncontradicts: [beliefs/b]\norigin: [brainstorms/x]\n---\n\nbody\n";
        let note = parse_note(text, &PathBuf::from("beliefs/a.md")).expect("parse");
        let edges = graph(&[note]);
        assert!(edges
            .iter()
            .any(|e| e.kind == EdgeKind::Contradicts && e.to == "beliefs/b"));
        assert!(edges
            .iter()
            .any(|e| e.kind == EdgeKind::Origin && e.to == "brainstorms/x"));
        // A declared link may carry an anchor. The anchor names a place inside
        // the target note, so the edge has to point at the note itself and
        // stay resolvable — the same rule the body route follows.
        let declared = "---\nid: beliefs/d\nlinks: [\"[[beliefs/c#history]]\"]\n---\n\nbody\n";
        let declared_note = parse_note(declared, &PathBuf::from("beliefs/d.md")).expect("parse");
        let declared_edges = graph(&[declared_note]);
        assert!(declared_edges
            .iter()
            .any(|e| e.kind == EdgeKind::Link && e.to == "beliefs/c"));
        assert!(
            !declared_edges.iter().any(|e| e.to.contains('#')),
            "an anchor must not become part of the edge target"
        );
    }
}
