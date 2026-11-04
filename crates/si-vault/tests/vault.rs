use si_vault::{Note, Vault, VaultError};

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

/// The spellings of the vault's own `self/` folder that Windows resolves to
/// the same directory.
const SELF_SPELLINGS: [&str; 4] = ["self/entry", "Self/entry", "SELF/entry", "self./entry"];

#[test]
fn notes_round_trip_through_the_filesystem() {
    let (_dir, v) = vault();
    v.write_note(&note("beliefs/rust", "Rust is fast."))
        .expect("write");
    let read = v.read_note("beliefs/rust").expect("read").expect("present");
    assert_eq!(
        read.body, "Rust is fast.",
        "the body must come back exactly"
    );
}

#[test]
fn writing_to_self_is_refused() {
    let (_dir, v) = vault();
    for id in SELF_SPELLINGS {
        let result = v.write_note(&note(id, "my own words"));
        assert!(
            matches!(result, Err(VaultError::SelfWriteRefused { .. })),
            "{id} must be refused with the self-write error, got {result:?}"
        );
    }
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
    let (dir, v) = vault();
    std::fs::create_dir_all(dir.path().join("self")).expect("self folder");
    std::fs::write(
        dir.path().join("self").join("journal.md"),
        "---\nid: self/journal\ntype: journal\n---\n\nmy own words\n",
    )
    .expect("seed the journal");
    for id in SELF_SPELLINGS {
        let result = v.delete_note(id);
        assert!(
            matches!(result, Err(VaultError::SelfWriteRefused { .. })),
            "{id} must be refused with the self-write error, got {result:?}"
        );
    }
    assert!(
        dir.path().join("self").join("journal.md").exists(),
        "the journal must survive every spelling of its own folder"
    );
}

#[test]
fn a_note_id_cannot_escape_the_vault() {
    let (dir, v) = vault();
    for id in [
        "../escaped",
        "beliefs/../../escaped",
        "/absolute",
        "C:/escaped",
        "C:escaped",
        "//server/share/escaped",
        ".. /escaped",
        ".../escaped",
    ] {
        assert!(
            v.write_note(&note(id, "x")).is_err(),
            "{id} must not be written outside the vault"
        );
        assert!(
            v.read_note(id).is_err(),
            "{id} must not be read from outside the vault"
        );
    }
    assert!(
        !dir.path()
            .parent()
            .expect("parent")
            .join("escaped.md")
            .exists(),
        "nothing may land beside the vault"
    );
}

#[test]
fn a_folder_that_starts_with_self_is_not_self() {
    let (_dir, v) = vault();
    for id in ["selfish/notes", "self.journal/notes", "other/self/notes"] {
        v.write_note(&note(id, "ordinary")).expect("write");
        assert!(
            v.read_note(id).expect("read").is_some(),
            "{id} is an ordinary folder"
        );
    }
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

#[test]
fn two_notes_with_the_same_file_name_get_different_ids() {
    let (dir, v) = vault();
    for folder in ["hand", "other"] {
        std::fs::create_dir_all(dir.path().join(folder)).expect("folder");
        std::fs::write(
            dir.path().join(folder).join("no-id.md"),
            "---\ntype: note\n---\n\nbody\n",
        )
        .expect("seed");
    }
    let ids: Vec<String> = v
        .notes()
        .expect("notes")
        .into_iter()
        .map(|n| n.id)
        .collect();
    assert!(ids.contains(&"hand/no-id".to_string()), "got {ids:?}");
    assert!(ids.contains(&"other/no-id".to_string()), "got {ids:?}");
}

#[test]
fn writing_a_note_leaves_no_temporary_file_behind() {
    let (dir, v) = vault();
    v.write_note(&note("beliefs/a", "a")).expect("write");
    let mut names: Vec<String> = std::fs::read_dir(dir.path().join("beliefs"))
        .expect("read")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["a.md".to_string()],
        "the write must not leave a temp file"
    );
}

#[test]
fn an_id_that_already_names_the_extension_is_not_doubled() {
    let (dir, v) = vault();
    v.write_note(&note("beliefs/a.md", "a")).expect("write");
    assert!(dir.path().join("beliefs").join("a.md").exists());
    assert!(!dir.path().join("beliefs").join("a.md.md").exists());
}

#[test]
fn notes_are_sorted_by_id() {
    let (_dir, v) = vault();
    for id in ["beliefs/c", "beliefs/a", "beliefs/b"] {
        v.write_note(&note(id, "x")).expect("write");
    }
    let ids: Vec<String> = v
        .notes()
        .expect("notes")
        .into_iter()
        .map(|n| n.id)
        .collect();
    assert_eq!(ids, vec!["beliefs/a", "beliefs/b", "beliefs/c"]);
}

#[test]
fn a_crlf_body_survives_the_round_trip() {
    let (_dir, v) = vault();
    v.write_note(&note("beliefs/crlf", "line one\r\nline two\r\n"))
        .expect("write");
    let read = v.read_note("beliefs/crlf").expect("read").expect("present");
    assert_eq!(read.body, "line one\r\nline two\r\n");
}
