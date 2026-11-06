use std::ops::Range;

use dopad_core::{ByteOffset, ByteRange, EolKind, TextBuffer, TextSnapshot, detect_eol};
use dopad_editor::Editor;

pub(super) fn local_change_range(
    old_len: usize,
    new_len: usize,
    old_selection: &Range<usize>,
    cursor: usize,
    new_selection: &Range<usize>,
) -> Option<(Range<usize>, Range<usize>)> {
    let removed = old_selection.end.checked_sub(old_selection.start)?;
    let old_cursor = old_selection.end;
    let change = old_selection.start..old_selection.start + removed;

    if new_selection.start != new_selection.end {
        return None;
    }
    if removed == 0 {
        if new_len > old_len {
            let inserted = new_len - old_len;
            return (cursor == old_cursor + inserted)
                .then_some((old_cursor..old_cursor, old_cursor..old_cursor + inserted));
        }
        if new_len < old_len {
            let deleted = old_len - new_len;
            if cursor < old_cursor && old_cursor - cursor == deleted {
                return Some((cursor..old_cursor, cursor..cursor));
            }
            if cursor == old_cursor {
                return Some((old_cursor..old_cursor + deleted, old_cursor..old_cursor));
            }
        }
        return None;
    }

    let unchanged_len = old_len.checked_sub(removed)?;
    let inserted = new_len.checked_sub(unchanged_len)?;
    let start = change.start;
    (cursor == start + inserted).then_some((change, start..start + inserted))
}

pub(super) fn oriented_selection(range: &Range<usize>, head: usize) -> Option<(usize, usize)> {
    if head < range.start || head > range.end {
        return None;
    }

    let anchor = if head == range.start {
        range.end
    } else {
        range.start
    };
    Some((anchor, head))
}

pub(super) fn apply_minimal_diff(
    editor: &mut Editor,
    new_text: &str,
) -> dopad_core::CoreResult<()> {
    let old_snapshot = editor.snapshot();
    let (range, replacement) = minimal_diff(&old_snapshot, new_text);
    if editor.undo_matches(range, &replacement) {
        editor.undo()?;
    } else if editor.redo_matches(range, &replacement) {
        editor.redo()?;
    } else {
        editor.replace_range(range, &replacement)?;
    }
    Ok(())
}

pub(super) fn minimal_diff(old_snapshot: &TextSnapshot, new_text: &str) -> (ByteRange, String) {
    let old_text = old_snapshot.to_string();
    minimal_diff_with_old_text(old_snapshot, &old_text, new_text)
}

pub(super) fn minimal_diff_with_old_text(
    old_snapshot: &TextSnapshot,
    old_text: &str,
    new_text: &str,
) -> (ByteRange, String) {
    let new_snapshot = TextBuffer::new(new_text).snapshot();
    let mut prefix = old_text
        .bytes()
        .zip(new_text.bytes())
        .take_while(|(left, right)| left == right)
        .count();
    while !old_text.is_char_boundary(prefix)
        || !new_text.is_char_boundary(prefix)
        || old_snapshot
            .validate_grapheme_boundary(ByteOffset(prefix))
            .is_err()
        || new_snapshot
            .validate_grapheme_boundary(ByteOffset(prefix))
            .is_err()
    {
        prefix -= 1;
    }
    let mut suffix = old_text[prefix..]
        .bytes()
        .rev()
        .zip(new_text[prefix..].bytes().rev())
        .take_while(|(left, right)| left == right)
        .count();
    while !old_text.is_char_boundary(old_text.len() - suffix)
        || !new_text.is_char_boundary(new_text.len() - suffix)
        || old_snapshot
            .validate_grapheme_boundary(ByteOffset(old_text.len() - suffix))
            .is_err()
        || new_snapshot
            .validate_grapheme_boundary(ByteOffset(new_text.len() - suffix))
            .is_err()
    {
        suffix -= 1;
    }
    let range = ByteRange::new(prefix, old_text.len() - suffix);
    let replacement = new_text[prefix..new_text.len() - suffix].to_owned();
    (range, replacement)
}

pub(super) fn update_eol_after_edit(current: &mut EolKind, inserted: &str) {
    let inserted_eol = detect_eol(inserted);
    if inserted_eol == EolKind::None {
        return;
    }
    *current = match (*current, inserted_eol) {
        (EolKind::None, kind) => kind,
        (existing, added) if existing == added => existing,
        _ => EolKind::Mixed,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use dopad_core::Revision;

    #[test]
    fn detects_local_insert_delete_and_selection_replacement() {
        assert_eq!(
            local_change_range(4, 5, &(2..2), 3, &(3..3)),
            Some((2..2, 2..3))
        );
        assert_eq!(
            local_change_range(5, 4, &(3..3), 2, &(2..2)),
            Some((2..3, 2..2))
        );
        assert_eq!(
            local_change_range(5, 4, &(2..2), 2, &(2..2)),
            Some((2..3, 2..2))
        );
        assert_eq!(
            local_change_range(8, 6, &(2..5), 3, &(3..3)),
            Some((2..5, 2..3))
        );
        assert_eq!(
            local_change_range(8, 5, &(2..5), 2, &(2..2)),
            Some((2..5, 2..2))
        );
    }

    #[test]
    fn oriented_selection_preserves_the_active_head_and_rejects_invalid_ranges() {
        assert_eq!(oriented_selection(&(2..7), 7), Some((2, 7)));
        assert_eq!(oriented_selection(&(2..7), 2), Some((7, 2)));
        assert_eq!(oriented_selection(&(4..4), 4), Some((4, 4)));
        assert_eq!(oriented_selection(&(2..7), 8), None);
    }

    #[test]
    fn history_sync_restores_saved_state_on_undo_and_redo() {
        let mut editor = Editor::new("");
        editor.type_text("abc").expect("type text");
        let erase = ByteRange::new(0, 3);
        assert!(editor.undo_matches(erase, ""));
        editor.undo().expect("undo");
        assert!(!editor.is_dirty());
        assert!(editor.redo_matches(ByteRange::new(0, 0), "abc"));
        editor.redo().expect("redo");
        assert!(editor.is_dirty());
        assert_eq!(editor.snapshot().to_string(), "abc");
    }

    #[test]
    fn multi_cursor_transaction_syncs_as_one_undoable_core_edit() {
        let mut editor = Editor::new("a,b");
        apply_minimal_diff(&mut editor, "xa,xb").expect("sync multi-cursor edit");
        assert!(editor.is_dirty());
        assert_eq!(editor.snapshot().to_string(), "xa,xb");

        apply_minimal_diff(&mut editor, "a,b").expect("sync multi-cursor undo");
        assert!(!editor.is_dirty());
        assert_eq!(editor.snapshot().to_string(), "a,b");

        apply_minimal_diff(&mut editor, "xa,xb").expect("sync multi-cursor redo");
        assert!(editor.is_dirty());
        assert_eq!(editor.snapshot().to_string(), "xa,xb");
    }

    #[test]
    fn minimal_diff_respects_grapheme_boundaries() {
        let mut editor = Editor::new("e\u{301}x");
        apply_minimal_diff(&mut editor, "éx").expect("replace grapheme");
        assert_eq!(editor.snapshot().to_string(), "éx");
        assert_eq!(editor.revision(), Revision(1));
    }

    #[test]
    fn replace_all_diff_matches_a_single_undoable_core_edit() {
        let original = "a xx b xx c";
        let replaced = "a y b y c";
        let mut editor = Editor::new(original);
        let (range, replacement) = minimal_diff(&editor.snapshot(), replaced);

        assert_eq!(range, ByteRange::new(2, 9));
        assert_eq!(replacement, "y b y");
        editor
            .replace_range(range, &replacement)
            .expect("apply one replace-all edit");
        assert_eq!(editor.snapshot().to_string(), replaced);

        let (undo_range, undo_replacement) = minimal_diff(&editor.snapshot(), original);
        assert!(editor.undo_matches(undo_range, &undo_replacement));
        editor.undo().expect("undo replace all");
        assert_eq!(editor.snapshot().to_string(), original);
        assert!(!editor.is_dirty());
    }
}
