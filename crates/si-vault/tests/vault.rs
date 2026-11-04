use si_vault::{Note, Vault};

fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

fn note(id: &str, body: &str) -> Note {
    Note {
        id: id.to_string(),
        title: id.to_string(),
        note_type: "belief".to_string(),
        tags: vec!["t".to_string()],
        links: vec![],
        body: body.to_string(),
        extra: serde_yaml::Value::Mapping(Default::default()),
        path: Default::default(),
    }
}

#[test]
fn notes_round_trip_through_the_filesystem() {
    let (_dir, v) = vault();
    v.write_note(&note("beliefs/rust", "Rust is fast."))
        .expect("write");
    let read = v.read_note("beliefs/rust").expect("read").expect("present");
    assert_eq!(read.body.trim(), "Rust is fast.");
}

#[test]
fn writing_to_self_is_refused() {
    let (_dir, v) = vault();
    let result = v.write_note(&note("self/2026-11-03", "my own words"));
    assert!(
        result.is_err(),
        "vault/self/ must only be written by the journal writer"
    );
}

#[test]
fn deleting_a_note_removes_it_from_the_projection() {
    let (_dir, v) = vault();
    v.write_note(&note("beliefs/a", "a")).expect("write");
    assert!(v.delete_note("beliefs/a").expect("delete"));
    assert!(v.read_note("beliefs/a").expect("read").is_none());
}

#[test]
fn the_graph_links_notes_by_wikilink() {
    let (_dir, v) = vault();
    v.write_note(&note("beliefs/a", "see [[beliefs/b]]"))
        .expect("write");
    v.write_note(&note("beliefs/b", "b")).expect("write");
    let edges = v.graph().expect("graph");
    assert!(edges
        .iter()
        .any(|e| e.from == "beliefs/a" && e.to == "beliefs/b"));
}

#[test]
fn deleting_from_self_is_refused() {
    let (_dir, v) = vault();
    let result = v.delete_note("self/2026-11-03");
    assert!(
        result.is_err(),
        "the journal writer owns the notes in self/"
    );
}

#[test]
fn a_note_id_cannot_escape_the_vault() {
    let (_dir, v) = vault();
    for id in ["../escaped", "beliefs/../../escaped", "/absolute"] {
        assert!(
            v.write_note(&note(id, "x")).is_err(),
            "{id} must not be written outside the vault"
        );
    }
}

#[test]
fn a_folder_that_starts_with_self_is_not_self() {
    let (_dir, v) = vault();
    v.write_note(&note("selfish/notes", "ordinary"))
        .expect("write");
    assert!(v.read_note("selfish/notes").expect("read").is_some());
}

#[test]
fn one_unreadable_note_does_not_hide_the_others() {
    let (_dir, v) = vault();
    v.write_note(&note("beliefs/a", "a")).expect("write");
    v.write_note(&note("beliefs/b", "b")).expect("write");
    // A note whose frontmatter is not valid YAML.
    std::fs::write(
        v.root().join("beliefs/broken.md"),
        "---\nid: [unclosed\n---\n\nbody\n",
    )
    .expect("write the broken note");
    let notes = v.notes().expect("notes");
    assert!(
        notes.iter().any(|n| n.id == "beliefs/a"),
        "a broken note must not hide the rest of the vault"
    );
    assert!(notes.iter().all(|n| n.id != "beliefs/broken"));
}
