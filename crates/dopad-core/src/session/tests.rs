use tempfile::tempdir;

use super::*;

fn manifest(generation: u64, text_tab: bool) -> (SessionManifest, Vec<DraftCheckpoint>) {
    let tab = SessionTab {
        id: 7,
        title: "notes.txt".into(),
        path: None,
        encoding: Encoding::Utf8,
        eol: EolKind::Lf,
        fingerprint: None,
        revision: 3,
        cursor: 2,
        selection_start: 1,
        selection_end: 2,
        word_wrap: true,
        font_size: 14.0,
        preview_mode: false,
        draft_ref: None,
    };
    let manifest = SessionManifest {
        schema_version: SCHEMA_VERSION,
        generation,
        clean_shutdown: false,
        active_tab_id: Some(7),
        dark_theme: false,
        tabs: vec![tab],
    };
    let drafts = text_tab
        .then(|| DraftCheckpoint {
            document_id: 7,
            revision: 3,
            text: "recovered 世界\n".into(),
        })
        .into_iter()
        .collect();
    (manifest, drafts)
}

#[test]
fn checkpoints_round_trip_dirty_drafts_and_clean_shutdown_state() {
    let directory = tempdir().expect("session directory");
    let (store, previous) = SessionStore::open(directory.path()).expect("open store");
    assert!(previous.is_none());
    let (initial_manifest, drafts) = manifest(1, true);
    assert!(
        store
            .checkpoint(initial_manifest, drafts)
            .expect("checkpoint")
    );
    drop(store);

    let (store, recovered) = SessionStore::open(directory.path()).expect("reopen store");
    let recovered = recovered.expect("saved session");
    assert!(recovered.was_unclean);
    assert_eq!(recovered.manifest.active_tab_id, Some(7));
    assert_eq!(recovered.drafts[0].text, "recovered 世界\n");
    store.mark_clean_shutdown().expect("mark clean shutdown");
    drop(store);

    let (_, recovered) = SessionStore::open(directory.path()).expect("open after clean exit");
    assert!(!recovered.expect("saved session").was_unclean);
}

#[test]
fn stale_checkpoints_do_not_replace_newer_manifest() {
    let directory = tempdir().expect("session directory");
    let (store, _) = SessionStore::open(directory.path()).expect("open store");
    let (newer, drafts) = manifest(2, true);
    assert!(store.checkpoint(newer, drafts).expect("new checkpoint"));
    let (older, drafts) = manifest(1, true);
    assert!(
        !store
            .checkpoint(older, drafts)
            .expect("stale checkpoint skipped")
    );
    drop(store);

    let (_, recovered) = SessionStore::open(directory.path()).expect("reopen store");
    assert_eq!(recovered.expect("saved session").manifest.generation, 2);
}

#[test]
fn duplicate_draft_ids_reject_the_checkpoint_without_replacing_saved_state() {
    let directory = tempdir().expect("session directory");
    let (store, _) = SessionStore::open(directory.path()).expect("open store");
    let (initial, drafts) = manifest(1, true);
    assert!(
        store
            .checkpoint(initial, drafts)
            .expect("initial checkpoint")
    );

    let (mut duplicate, _) = manifest(2, true);
    duplicate.tabs[0].draft_ref = None;
    let duplicate_drafts = vec![
        DraftCheckpoint {
            document_id: 7,
            revision: 3,
            text: "first".into(),
        },
        DraftCheckpoint {
            document_id: 7,
            revision: 3,
            text: "second".into(),
        },
    ];
    assert!(matches!(
        store.checkpoint(duplicate, duplicate_drafts),
        Err(SessionError::InvalidManifest)
    ));
    drop(store);

    let (_, recovered) = SessionStore::open(directory.path()).expect("open saved state");
    let recovered = recovered.expect("previous draft is preserved");
    assert_eq!(recovered.manifest.generation, 1);
    assert_eq!(recovered.drafts[0].text, "recovered 世界\n");
}

#[test]
fn failed_clean_shutdown_write_keeps_the_in_memory_state_unclean() {
    let directory = tempdir().expect("session directory");
    let (store, _) = SessionStore::open(directory.path()).expect("open store");
    let (initial_manifest, drafts) = manifest(1, true);
    assert!(
        store
            .checkpoint(initial_manifest, drafts)
            .expect("checkpoint")
    );

    let manifest_path = directory.path().join("session.json");
    fs::remove_file(&manifest_path).expect("remove manifest");
    fs::create_dir(&manifest_path).expect("block manifest replacement");
    assert!(store.mark_clean_shutdown().is_err());
    fs::remove_dir(&manifest_path).expect("remove blocking directory");

    let (next_manifest, drafts) = manifest(2, true);
    assert!(
        store
            .checkpoint(next_manifest, drafts)
            .expect("checkpoint after failure")
    );
    drop(store);

    let (_, recovered) = SessionStore::open(directory.path()).expect("reopen session");
    assert!(recovered.expect("recovered state").was_unclean);
}

#[test]
fn refuses_corrupt_drafts_without_discarding_manifest() {
    let directory = tempdir().expect("session directory");
    let (store, _) = SessionStore::open(directory.path()).expect("open store");
    let (initial_manifest, drafts) = manifest(1, true);
    store
        .checkpoint(initial_manifest, drafts)
        .expect("checkpoint");
    let draft = directory.path().join("drafts/draft-7-3.txt");
    fs::write(draft, b"tampered").expect("tamper draft");
    drop(store);

    assert!(matches!(
        SessionStore::open(directory.path()),
        Err(SessionError::CorruptDraft)
    ));
    assert!(directory.path().join("session.json").exists());
}

#[test]
fn rejects_a_second_instance_and_releases_lock_on_drop() {
    let directory = tempdir().expect("session directory");
    let (store, _) = SessionStore::open(directory.path()).expect("open store");
    assert!(matches!(
        SessionStore::open(directory.path()),
        Err(SessionError::AlreadyRunning)
    ));
    drop(store);
    assert!(SessionStore::open(directory.path()).is_ok());
}

#[test]
fn oversized_checkpoint_keeps_the_previous_generation() {
    let directory = tempdir().expect("session directory");
    let (store, _) = SessionStore::open(directory.path()).expect("open store");
    let (initial_manifest, drafts) = manifest(1, true);
    store
        .checkpoint(initial_manifest, drafts)
        .expect("checkpoint");
    let mut oversized = manifest(2, true).1;
    oversized[0].text = "x".repeat(MAX_DRAFT_BYTES + 1);
    assert!(matches!(
        store.checkpoint(manifest(2, true).0, oversized),
        Err(SessionError::TooLarge)
    ));
    drop(store);

    let (_, recovered) = SessionStore::open(directory.path()).expect("reopen store");
    assert_eq!(
        recovered
            .expect("old checkpoint remains")
            .manifest
            .generation,
        1
    );
}

#[test]
fn rejects_unknown_schema_and_path_traversal_references() {
    let directory = tempdir().expect("session directory");
    let mut invalid = SessionManifest::empty();
    invalid.schema_version = SCHEMA_VERSION + 1;
    fs::write(
        directory.path().join("session.json"),
        serde_json::to_vec(&invalid).expect("encode manifest"),
    )
    .expect("write manifest");
    assert!(matches!(
        SessionStore::open(directory.path()),
        Err(SessionError::UnsupportedVersion(_))
    ));

    let mut invalid = SessionManifest::empty();
    invalid.active_tab_id = Some(1);
    invalid.tabs.push(SessionTab {
        id: 1,
        title: "notes".into(),
        path: None,
        encoding: Encoding::Utf8,
        eol: EolKind::Lf,
        fingerprint: None,
        revision: 1,
        cursor: 0,
        selection_start: 0,
        selection_end: 0,
        word_wrap: true,
        font_size: 14.0,
        preview_mode: false,
        draft_ref: Some(DraftReference {
            file_name: "../outside.txt".into(),
            revision: 1,
            byte_len: 0,
            checksum: checksum(b""),
        }),
    });
    assert!(matches!(
        validate_manifest(&invalid),
        Err(SessionError::InvalidManifest)
    ));
}
