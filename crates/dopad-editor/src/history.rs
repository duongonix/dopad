use crate::Selection;
use dopad_core::Edit;
use std::collections::VecDeque;

const MAX_HISTORY_BYTES: usize = 32 * 1024 * 1024;
const MAX_HISTORY_ENTRIES: usize = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EditKind {
    Typing,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StateId(pub(crate) u64);

pub(crate) struct HistoryEntry {
    pub(crate) forward: Vec<Edit>,
    pub(crate) inverse: Vec<Edit>,
    pub(crate) before_state: StateId,
    pub(crate) after_state: StateId,
    pub(crate) before_selection: Selection,
    pub(crate) after_selection: Selection,
    kind: EditKind,
    bytes: usize,
}

impl HistoryEntry {
    pub(crate) fn new(
        forward: Vec<Edit>,
        inverse: Vec<Edit>,
        before_state: StateId,
        after_state: StateId,
        before_selection: Selection,
        after_selection: Selection,
        kind: EditKind,
    ) -> Self {
        let bytes = payload_bytes(&forward) + payload_bytes(&inverse);
        Self {
            forward,
            inverse,
            before_state,
            after_state,
            before_selection,
            after_selection,
            kind,
            bytes,
        }
    }
}

pub(crate) struct History {
    undo: VecDeque<HistoryEntry>,
    redo: VecDeque<HistoryEntry>,
    used_bytes: usize,
    coalescing_barrier: bool,
}

impl History {
    pub(crate) fn new() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            used_bytes: 0,
            coalescing_barrier: false,
        }
    }

    pub(crate) fn break_coalescing(&mut self) {
        self.coalescing_barrier = true;
    }

    pub(crate) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.used_bytes = 0;
        self.coalescing_barrier = true;
    }

    pub(crate) fn record(&mut self, entry: HistoryEntry) -> bool {
        self.clear_redo();
        let allow_coalescing = !self.coalescing_barrier;
        self.coalescing_barrier = false;
        if entry.bytes > MAX_HISTORY_BYTES {
            self.clear();
            return false;
        }

        if allow_coalescing && entry.kind == EditKind::Typing && self.try_coalesce(&entry) {
            return true;
        }

        self.used_bytes += entry.bytes;
        self.undo.push_back(entry);
        self.trim();
        true
    }

    pub(crate) fn undo_entry(&self) -> Option<&HistoryEntry> {
        self.undo.back()
    }

    pub(crate) fn redo_entry(&self) -> Option<&HistoryEntry> {
        self.redo.back()
    }

    pub(crate) fn finish_undo(&mut self) {
        if let Some(entry) = self.undo.pop_back() {
            self.redo.push_back(entry);
        }
        self.break_coalescing();
    }

    pub(crate) fn finish_redo(&mut self) {
        if let Some(entry) = self.redo.pop_back() {
            self.undo.push_back(entry);
        }
        self.trim();
        self.break_coalescing();
    }

    fn clear_redo(&mut self) {
        while let Some(entry) = self.redo.pop_back() {
            self.used_bytes = self.used_bytes.saturating_sub(entry.bytes);
        }
    }

    fn try_coalesce(&mut self, entry: &HistoryEntry) -> bool {
        let Some(previous) = self.undo.back_mut() else {
            return false;
        };
        if previous.kind != EditKind::Typing
            || previous.after_state != entry.before_state
            || previous.forward.len() != 1
            || entry.forward.len() != 1
            || previous.inverse.len() != 1
            || entry.inverse.len() != 1
        {
            return false;
        }

        let previous_edit = &previous.forward[0];
        let next_edit = &entry.forward[0];
        if !previous_edit.range.is_empty()
            || !next_edit.range.is_empty()
            || previous_edit.range.start.0 + previous_edit.replacement.len()
                != next_edit.range.start.0
        {
            return false;
        }
        let merged_len = previous_edit.replacement.len() + next_edit.replacement.len();
        let merged_bytes = merged_len;
        if merged_bytes > MAX_HISTORY_BYTES {
            return false;
        }

        let old_bytes = previous.bytes;
        previous.forward[0]
            .replacement
            .push_str(&next_edit.replacement);
        previous.inverse[0].range.end.0 = previous.inverse[0].range.start.0 + merged_len;
        previous.after_state = entry.after_state;
        previous.after_selection = entry.after_selection;
        previous.bytes = merged_bytes;
        self.used_bytes = self.used_bytes - old_bytes + merged_bytes;
        self.trim();
        true
    }

    fn trim(&mut self) {
        while self.used_bytes > MAX_HISTORY_BYTES || self.undo.len() > MAX_HISTORY_ENTRIES {
            let Some(entry) = self.undo.pop_front() else {
                break;
            };
            self.used_bytes = self.used_bytes.saturating_sub(entry.bytes);
        }
    }
}

fn payload_bytes(edits: &[Edit]) -> usize {
    edits.iter().map(|edit| edit.replacement.len()).sum()
}
