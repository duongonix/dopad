use std::fs;

use tempfile::tempdir;

use super::{PathOperation, apply_path_operation, delete_path, valid_name};

#[test]
fn names_must_be_single_safe_path_components() {
    assert_eq!(valid_name(" report.md ").expect("valid name"), "report.md");
    assert_eq!(valid_name("資料.rs").expect("unicode name"), "資料.rs");
    for invalid in ["", ".", "..", "../escape", "folder/file", "folder\\file"] {
        assert!(
            valid_name(invalid).is_err(),
            "accepted invalid name: {invalid:?}"
        );
    }
}

#[test]
fn create_uses_create_new_and_preserves_existing_file() {
    let workspace = tempdir().expect("temporary workspace");
    let existing = workspace.path().join("notes.txt");
    fs::write(&existing, "keep original").expect("write existing file");

    let result = apply_path_operation(
        PathOperation::CreateFile(workspace.path().to_path_buf()),
        "notes.txt",
        Some(workspace.path()),
    );

    assert!(result.is_err());
    assert_eq!(
        fs::read_to_string(existing).expect("read original"),
        "keep original"
    );
}

#[test]
fn delete_is_confined_to_the_workspace_and_removes_selected_item() {
    let workspace = tempdir().expect("temporary workspace");
    let file = workspace.path().join("remove.txt");
    fs::write(&file, "remove me").expect("write file");

    delete_path(&file, Some(workspace.path())).expect("delete selected file");
    assert!(!file.exists());
    assert!(delete_path(workspace.path(), Some(workspace.path())).is_err());
}
