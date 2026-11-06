use dopad_core::{ByteOffset, ByteRange, CoreError, Revision};
use dopad_editor::Editor;

#[test]
fn public_editor_api_keeps_snapshot_and_revision_consistent() {
    let mut editor = Editor::new("alpha\nβeta");
    let before = editor.snapshot();
    editor
        .set_selection(ByteOffset(0), ByteOffset(5))
        .expect("ASCII selection is valid");
    let changed = editor
        .replace_selection("Dopad")
        .expect("replacement should commit");

    assert!(changed.changed);
    assert_eq!(changed.revision, Revision(1));
    assert_eq!(editor.snapshot().to_string(), "Dopad\nβeta");
    assert_eq!(before.to_string(), "alpha\nβeta");
    assert_eq!(before.revision(), Revision(0));
    assert!(editor.is_dirty());
}

#[test]
fn invalid_selection_does_not_change_the_document() {
    let mut editor = Editor::new("aé中");
    let initial = editor.snapshot();
    let result = editor.set_selection(ByteOffset(2), ByteOffset(3));

    assert!(matches!(result, Err(CoreError::InvalidUtf8Boundary(_))));
    assert_eq!(editor.snapshot().to_string(), initial.to_string());
    assert_eq!(editor.revision(), Revision(0));
    assert_eq!(editor.selection().range(), ByteRange::new(0, 0));
}

#[test]
fn undo_restores_selection_and_saved_state() {
    let mut editor = Editor::new("notes");
    editor.mark_saved();
    editor
        .set_selection(ByteOffset(1), ByteOffset(4))
        .expect("selection is valid");
    editor
        .replace_selection("é")
        .expect("unicode replacement should commit");
    assert!(editor.undo().expect("undo should apply"));

    assert_eq!(editor.snapshot().to_string(), "notes");
    assert_eq!(editor.selection().range(), ByteRange::new(1, 4));
    assert!(!editor.is_dirty());
}
