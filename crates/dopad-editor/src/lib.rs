#![forbid(unsafe_code)]

//! Headless cursor, selection, editing, and bounded undo/redo state.

mod cursor;
mod editor;
mod history;
mod search;

pub use cursor::{Cursor, CursorAffinity, Selection};
pub use editor::{EditResult, Editor};
pub use search::{
    SearchError, SearchMatch, SearchOptions, SearchResult, compile_matcher, expand_replacement,
    find_matches, find_matches_in_range, is_whole_word, replace_matches_text,
};

#[cfg(test)]
mod tests {
    use super::*;
    use dopad_core::{ByteOffset, ByteRange, CoreError, TextPosition};

    fn text(editor: &Editor) -> String {
        editor.snapshot().to_string()
    }

    #[test]
    fn cursor_moves_by_grapheme_and_rejects_interior_offsets() {
        let mut editor = Editor::new("e\u{301}👩‍💻x");
        editor.move_right(false).unwrap();
        assert_eq!(editor.cursor().offset(), ByteOffset("e\u{301}".len()));
        editor.move_right(false).unwrap();
        assert_eq!(editor.cursor().offset(), ByteOffset("e\u{301}👩‍💻".len()));
        editor.move_left(false).unwrap();
        assert_eq!(editor.cursor().offset(), ByteOffset("e\u{301}".len()));
        assert!(matches!(
            editor.set_cursor(ByteOffset(1)),
            Err(CoreError::InvalidGraphemeBoundary(_))
        ));
    }

    #[test]
    fn selection_can_extend_and_collapse_to_directional_edge() {
        let mut editor = Editor::new("alpha\nbeta");
        editor.set_cursor(ByteOffset(2)).unwrap();
        editor.move_right(true).unwrap();
        assert_eq!(editor.selection().range(), ByteRange::new(2, 3));
        editor.move_left(false).unwrap();
        assert!(editor.selection().is_empty());
        assert_eq!(editor.cursor().offset(), ByteOffset(2));
        editor.set_selection(ByteOffset(9), ByteOffset(2)).unwrap();
        assert_eq!(editor.selection().range(), ByteRange::new(2, 9));
    }

    #[test]
    fn vertical_movement_preserves_preferred_grapheme_column() {
        let mut editor = Editor::new("abcdef\nxy\nabcdef");
        editor.set_cursor(ByteOffset(5)).unwrap();
        editor.move_down(false).unwrap();
        assert_eq!(editor.cursor().offset(), ByteOffset(9));
        assert_eq!(editor.cursor().preferred_column(), Some(5));
        editor.move_down(false).unwrap();
        assert_eq!(editor.cursor().offset(), ByteOffset(15));
        assert_eq!(
            editor
                .snapshot()
                .byte_to_position(editor.cursor().offset())
                .unwrap(),
            TextPosition::new(2, 5)
        );
    }

    #[test]
    fn word_movement_uses_unicode_word_characters_and_whitespace() {
        let mut editor = Editor::new("你好, world");
        editor.move_word_right(false).unwrap();
        assert_eq!(editor.cursor().offset(), ByteOffset("你好".len()));
        editor.move_word_right(false).unwrap();
        assert_eq!(editor.cursor().offset(), ByteOffset("你好,".len()));
        editor.move_word_right(false).unwrap();
        assert_eq!(editor.cursor().offset(), ByteOffset("你好, world".len()));
        editor.move_word_left(false).unwrap();
        assert_eq!(editor.cursor().offset(), ByteOffset("你好, ".len()));
    }

    #[test]
    fn typing_coalesces_and_undo_redo_revisions_remain_monotonic() {
        let mut editor = Editor::new("");
        editor.type_text("d").unwrap();
        editor.type_text("o").unwrap();
        editor.type_text("p").unwrap();
        assert_eq!(text(&editor), "dop");
        assert_eq!(editor.revision().0, 3);
        assert!(editor.undo().unwrap());
        assert_eq!(text(&editor), "");
        assert_eq!(editor.revision().0, 4);
        assert!(editor.redo().unwrap());
        assert_eq!(text(&editor), "dop");
        assert_eq!(editor.revision().0, 5);
        assert!(!editor.redo().unwrap());
    }

    #[test]
    fn undo_and_redo_capabilities_follow_history() {
        let mut editor = Editor::new("");
        assert!(!editor.can_undo());
        assert!(!editor.can_redo());

        editor.type_text("a").unwrap();
        assert!(editor.can_undo());
        assert!(!editor.can_redo());

        editor.undo().unwrap();
        assert!(!editor.can_undo());
        assert!(editor.can_redo());

        editor.redo().unwrap();
        assert!(editor.can_undo());
        assert!(!editor.can_redo());
    }

    #[test]
    fn savepoint_is_state_based_and_redo_is_cleared_on_branch() {
        let mut editor = Editor::new("");
        editor.type_text("a").unwrap();
        editor.mark_saved();
        editor.type_text("b").unwrap();
        assert!(editor.is_dirty());
        assert!(editor.undo().unwrap());
        assert_eq!(text(&editor), "a");
        assert!(!editor.is_dirty());
        assert!(editor.redo().unwrap());
        assert!(editor.is_dirty());
        assert!(editor.undo().unwrap());
        editor.type_text("c").unwrap();
        assert_eq!(text(&editor), "ac");
        assert!(!editor.redo().unwrap());
    }

    #[test]
    fn recovered_draft_is_dirty_without_inventing_undo_history() {
        let mut editor = Editor::new_recovered("draft 世界");
        assert_eq!(text(&editor), "draft 世界");
        assert!(editor.is_dirty());
        assert!(!editor.undo().unwrap());
        editor.mark_saved();
        assert!(!editor.is_dirty());
    }

    #[test]
    fn replacement_and_delete_are_reversible_for_unicode_selection() {
        let mut editor = Editor::new("aé中");
        editor.set_selection(ByteOffset(1), ByteOffset(6)).unwrap();
        editor.replace_selection("🙂").unwrap();
        assert_eq!(text(&editor), "a🙂");
        assert!(editor.undo().unwrap());
        assert_eq!(text(&editor), "aé中");
        assert_eq!(editor.selection().range(), ByteRange::new(1, 6));
        editor.set_cursor(ByteOffset(3)).unwrap();
        editor.delete_backward().unwrap();
        assert_eq!(text(&editor), "a中");
        assert!(editor.undo().unwrap());
        assert_eq!(text(&editor), "aé中");
    }

    #[test]
    fn inserted_combining_mark_keeps_the_caret_on_a_grapheme_boundary() {
        let mut editor = Editor::new("\u{301}x");
        editor.replace_selection("e").unwrap();
        assert_eq!(text(&editor), "e\u{301}x");
        assert_eq!(editor.cursor().offset(), ByteOffset("e\u{301}".len()));
        assert!(
            editor
                .snapshot()
                .is_grapheme_boundary(editor.cursor().offset())
                .unwrap()
        );
    }

    #[test]
    fn edit_after_undo_invalidates_redo_and_noop_does_not_advance_revision() {
        let mut editor = Editor::new("x");
        let unchanged = editor.replace_selection("").unwrap();
        assert!(!unchanged.changed);
        assert_eq!(editor.revision().0, 0);
        editor.replace_range(ByteRange::new(0, 1), "y").unwrap();
        assert!(editor.undo().unwrap());
        editor.type_text("z").unwrap();
        assert_eq!(text(&editor), "zx");
        assert!(!editor.redo().unwrap());
    }
}
