use crate::history::{EditKind, History, HistoryEntry, StateId};
use crate::{Cursor, CursorAffinity, Selection};
use dopad_core::{
    ByteOffset, ByteRange, CoreError, CoreResult, Edit, Revision, TextBuffer, TextPosition,
    TextSnapshot,
};

pub struct Editor {
    pub(super) buffer: TextBuffer,
    pub(super) selection: Selection,
    pub(super) history: History,
    pub(super) current_state: StateId,
    pub(super) next_state: u64,
    pub(super) saved_state: Option<StateId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EditResult {
    pub revision: Revision,
    pub changed: bool,
    pub undoable: bool,
}

impl Editor {
    pub fn new(text: &str) -> Self {
        Self {
            buffer: TextBuffer::new(text),
            selection: Selection::caret(ByteOffset(0)),
            history: History::new(),
            current_state: StateId(0),
            next_state: 1,
            saved_state: Some(StateId(0)),
        }
    }

    /// Creates an editor from a recovered draft without treating it as saved.
    pub fn new_recovered(text: &str) -> Self {
        Self {
            buffer: TextBuffer::new(text),
            selection: Selection::caret(ByteOffset(0)),
            history: History::new(),
            current_state: StateId(1),
            next_state: 2,
            saved_state: None,
        }
    }

    pub fn snapshot(&self) -> TextSnapshot {
        self.buffer.snapshot()
    }

    pub fn revision(&self) -> Revision {
        self.buffer.revision()
    }

    pub fn selection(&self) -> Selection {
        self.selection
    }

    pub fn cursor(&self) -> Cursor {
        self.selection.head()
    }

    pub fn is_dirty(&self) -> bool {
        self.saved_state != Some(self.current_state)
    }

    pub fn can_undo(&self) -> bool {
        self.history.undo_entry().is_some()
    }

    pub fn can_redo(&self) -> bool {
        self.history.redo_entry().is_some()
    }

    pub fn mark_saved(&mut self) {
        self.saved_state = Some(self.current_state);
        self.history.break_coalescing();
    }

    pub fn set_cursor(&mut self, offset: ByteOffset) -> CoreResult<()> {
        self.snapshot().validate_grapheme_boundary(offset)?;
        self.selection = Selection::caret(offset);
        self.history.break_coalescing();
        Ok(())
    }

    pub fn set_selection(&mut self, anchor: ByteOffset, head: ByteOffset) -> CoreResult<()> {
        let snapshot = self.snapshot();
        snapshot.validate_grapheme_boundary(anchor)?;
        snapshot.validate_grapheme_boundary(head)?;
        self.selection = Selection::new(anchor, head);
        self.history.break_coalescing();
        Ok(())
    }

    pub fn select_all(&mut self) {
        self.selection = Selection::new(ByteOffset(0), ByteOffset(self.snapshot().len_bytes()));
        self.history.break_coalescing();
    }

    pub fn collapse_selection(&mut self) {
        self.selection = Selection::caret(self.selection.head.offset);
        self.history.break_coalescing();
    }

    pub fn move_left(&mut self, extend: bool) -> CoreResult<()> {
        if !extend && !self.selection.is_empty() {
            return self.move_head(
                self.selection.range().start,
                false,
                CursorAffinity::Upstream,
                None,
            );
        }
        let previous = self
            .snapshot()
            .previous_grapheme_boundary(self.selection.head.offset)?
            .unwrap_or(ByteOffset(0));
        self.move_head(previous, extend, CursorAffinity::Upstream, None)
    }

    pub fn move_right(&mut self, extend: bool) -> CoreResult<()> {
        if !extend && !self.selection.is_empty() {
            return self.move_head(
                self.selection.range().end,
                false,
                CursorAffinity::Downstream,
                None,
            );
        }
        let next = self
            .snapshot()
            .next_grapheme_boundary(self.selection.head.offset)?
            .unwrap_or(ByteOffset(self.snapshot().len_bytes()));
        self.move_head(next, extend, CursorAffinity::Downstream, None)
    }

    pub fn move_home(&mut self, extend: bool) -> CoreResult<()> {
        let snapshot = self.snapshot();
        let position = snapshot.byte_to_position(self.selection.head.offset)?;
        let line_start = snapshot.line_content_range(position.line)?.start;
        self.move_head(line_start, extend, CursorAffinity::Upstream, None)
    }

    pub fn move_end(&mut self, extend: bool) -> CoreResult<()> {
        let snapshot = self.snapshot();
        let position = snapshot.byte_to_position(self.selection.head.offset)?;
        let line_end = snapshot.line_content_range(position.line)?.end;
        self.move_head(line_end, extend, CursorAffinity::Downstream, None)
    }

    pub fn move_up(&mut self, extend: bool) -> CoreResult<()> {
        self.move_vertical(-1, extend)
    }

    pub fn move_down(&mut self, extend: bool) -> CoreResult<()> {
        self.move_vertical(1, extend)
    }

    pub fn move_word_left(&mut self, extend: bool) -> CoreResult<()> {
        let snapshot = self.snapshot();
        let mut cursor = self.selection.head.offset;
        let Some(mut previous) = snapshot.previous_grapheme_boundary(cursor)? else {
            return self.move_head(cursor, extend, CursorAffinity::Upstream, None);
        };
        let mut kind = cluster_kind(&snapshot.slice(ByteRange {
            start: previous,
            end: cursor,
        })?);
        while kind == WordKind::Whitespace {
            cursor = previous;
            let Some(next) = snapshot.previous_grapheme_boundary(cursor)? else {
                return self.move_head(ByteOffset(0), extend, CursorAffinity::Upstream, None);
            };
            previous = next;
            kind = cluster_kind(&snapshot.slice(ByteRange {
                start: previous,
                end: cursor,
            })?);
        }
        if kind == WordKind::Word {
            while let Some(next) = snapshot.previous_grapheme_boundary(previous)? {
                let candidate = snapshot.slice(ByteRange {
                    start: next,
                    end: previous,
                })?;
                if cluster_kind(&candidate) != WordKind::Word {
                    break;
                }
                previous = next;
            }
        }
        self.move_head(previous, extend, CursorAffinity::Upstream, None)
    }

    pub fn move_word_right(&mut self, extend: bool) -> CoreResult<()> {
        let snapshot = self.snapshot();
        let mut cursor = self.selection.head.offset;
        let Some(mut next) = snapshot.next_grapheme_boundary(cursor)? else {
            return self.move_head(cursor, extend, CursorAffinity::Downstream, None);
        };
        let mut kind = cluster_kind(&snapshot.slice(ByteRange {
            start: cursor,
            end: next,
        })?);
        while kind == WordKind::Whitespace {
            cursor = next;
            let Some(boundary) = snapshot.next_grapheme_boundary(cursor)? else {
                return self.move_head(
                    ByteOffset(snapshot.len_bytes()),
                    extend,
                    CursorAffinity::Downstream,
                    None,
                );
            };
            next = boundary;
            kind = cluster_kind(&snapshot.slice(ByteRange {
                start: cursor,
                end: next,
            })?);
        }
        if kind == WordKind::Word {
            cursor = next;
            while let Some(boundary) = snapshot.next_grapheme_boundary(cursor)? {
                let candidate = snapshot.slice(ByteRange {
                    start: cursor,
                    end: boundary,
                })?;
                if cluster_kind(&candidate) != WordKind::Word {
                    break;
                }
                next = boundary;
                cursor = boundary;
            }
        }
        self.move_head(next, extend, CursorAffinity::Downstream, None)
    }

    pub fn replace_selection(&mut self, replacement: &str) -> CoreResult<EditResult> {
        self.replace_range_as(self.selection.range(), replacement, EditKind::Other)
    }

    /// Inserts text from a typing event and coalesces adjacent input for undo.
    pub fn type_text(&mut self, text: &str) -> CoreResult<EditResult> {
        self.replace_range_as(self.selection.range(), text, EditKind::Typing)
    }

    pub fn replace_range(&mut self, range: ByteRange, replacement: &str) -> CoreResult<EditResult> {
        self.replace_range_as(range, replacement, EditKind::Other)
    }

    pub fn replace_all(&mut self, replacement: &str) -> CoreResult<EditResult> {
        let range = ByteRange::new(0, self.snapshot().len_bytes());
        self.replace_range_as(range, replacement, EditKind::Other)
    }

    pub fn delete_backward(&mut self) -> CoreResult<EditResult> {
        if !self.selection.is_empty() {
            return self.replace_range_as(self.selection.range(), "", EditKind::Other);
        }
        let cursor = self.selection.head.offset;
        let start = self
            .snapshot()
            .previous_grapheme_boundary(cursor)?
            .unwrap_or(cursor);
        self.replace_range_as(ByteRange { start, end: cursor }, "", EditKind::Other)
    }

    pub fn delete_forward(&mut self) -> CoreResult<EditResult> {
        if !self.selection.is_empty() {
            return self.replace_range_as(self.selection.range(), "", EditKind::Other);
        }
        let cursor = self.selection.head.offset;
        let end = self
            .snapshot()
            .next_grapheme_boundary(cursor)?
            .unwrap_or(cursor);
        self.replace_range_as(ByteRange { start: cursor, end }, "", EditKind::Other)
    }

    pub fn undo(&mut self) -> CoreResult<bool> {
        let Some(entry) = self.history.undo_entry() else {
            return Ok(false);
        };
        let revision = self.revision();
        let state = entry.before_state;
        let selection = entry.before_selection;
        self.buffer.apply_edits(revision, &entry.inverse)?;
        self.history.finish_undo();
        self.current_state = state;
        self.selection = selection;
        Ok(true)
    }

    pub fn undo_matches(&self, range: ByteRange, replacement: &str) -> bool {
        self.history.undo_entry().is_some_and(|entry| {
            entry.inverse.len() == 1
                && entry.inverse[0].range == range
                && entry.inverse[0].replacement == replacement
        })
    }

    pub fn redo(&mut self) -> CoreResult<bool> {
        let Some(entry) = self.history.redo_entry() else {
            return Ok(false);
        };
        let revision = self.revision();
        let state = entry.after_state;
        let selection = entry.after_selection;
        self.buffer.apply_edits(revision, &entry.forward)?;
        self.history.finish_redo();
        self.current_state = state;
        self.selection = selection;
        Ok(true)
    }

    pub fn redo_matches(&self, range: ByteRange, replacement: &str) -> bool {
        self.history.redo_entry().is_some_and(|entry| {
            entry.forward.len() == 1
                && entry.forward[0].range == range
                && entry.forward[0].replacement == replacement
        })
    }

    fn move_vertical(&mut self, delta: isize, extend: bool) -> CoreResult<()> {
        let snapshot = self.snapshot();
        let current = snapshot.byte_to_position(self.selection.head.offset)?;
        let preferred = self
            .selection
            .head
            .preferred_column
            .unwrap_or(current.column);
        let target_line = if delta < 0 {
            current.line.saturating_sub(delta.unsigned_abs())
        } else {
            current.line.saturating_add(delta as usize)
        };
        if target_line >= snapshot.line_count() {
            self.history.break_coalescing();
            return Ok(());
        }
        let target =
            snapshot.position_to_byte_clamped(TextPosition::new(target_line, preferred))?;
        self.move_head(
            target,
            extend,
            if delta < 0 {
                CursorAffinity::Upstream
            } else {
                CursorAffinity::Downstream
            },
            Some(preferred),
        )
    }

    fn move_head(
        &mut self,
        offset: ByteOffset,
        extend: bool,
        affinity: CursorAffinity,
        preferred_column: Option<usize>,
    ) -> CoreResult<()> {
        self.snapshot().validate_grapheme_boundary(offset)?;
        let head = Cursor {
            offset,
            affinity,
            preferred_column,
        };
        if extend {
            self.selection.head = head;
        } else {
            self.selection = Selection { anchor: head, head };
        }
        self.history.break_coalescing();
        Ok(())
    }

    fn replace_range_as(
        &mut self,
        range: ByteRange,
        replacement: &str,
        kind: EditKind,
    ) -> CoreResult<EditResult> {
        let snapshot = self.snapshot();
        snapshot.validate_grapheme_boundary(range.start)?;
        snapshot.validate_grapheme_boundary(range.end)?;
        let before_selection = self.selection;
        let before_state = self.current_state;
        let after_state = StateId(self.next_state);
        let next_state = self
            .next_state
            .checked_add(1)
            .ok_or(CoreError::RevisionExhausted)?;
        let target = ByteOffset(
            range
                .start
                .0
                .checked_add(replacement.len())
                .ok_or(CoreError::BufferMutation)?,
        );
        let edit = Edit::new(range, replacement);
        let outcome = self
            .buffer
            .apply_edits(self.revision(), std::slice::from_ref(&edit))?;
        if !outcome.changed {
            return Ok(EditResult {
                revision: outcome.revision,
                changed: false,
                undoable: false,
            });
        }

        let after_snapshot = self.snapshot();
        let caret = if matches!(after_snapshot.is_grapheme_boundary(target), Ok(true)) {
            target
        } else {
            after_snapshot
                .next_grapheme_boundary(target)
                .ok()
                .flatten()
                .unwrap_or(ByteOffset(after_snapshot.len_bytes()))
        };
        let after_selection = Selection::caret(caret);
        let undoable = self.history.record(HistoryEntry::new(
            vec![edit],
            outcome.inverse,
            before_state,
            after_state,
            before_selection,
            after_selection,
            kind,
        ));
        self.current_state = after_state;
        self.next_state = next_state;
        self.selection = after_selection;
        Ok(EditResult {
            revision: outcome.revision,
            changed: true,
            undoable,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WordKind {
    Whitespace,
    Word,
    Punctuation,
}

fn cluster_kind(cluster: &str) -> WordKind {
    match cluster.chars().next() {
        Some(character) if character.is_whitespace() => WordKind::Whitespace,
        Some(character) if character.is_alphanumeric() || character == '_' => WordKind::Word,
        _ => WordKind::Punctuation,
    }
}
