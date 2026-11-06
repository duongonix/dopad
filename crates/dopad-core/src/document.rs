use crate::{ByteRange, CoreError, CoreResult, Revision, TextBuffer};
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Edit {
    pub range: ByteRange,
    pub replacement: String,
}

impl Edit {
    pub fn new(range: ByteRange, replacement: impl Into<String>) -> Self {
        Self {
            range,
            replacement: replacement.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppliedEditBatch {
    pub before_revision: Revision,
    pub revision: Revision,
    pub inverse: Vec<Edit>,
    pub changed: bool,
}

struct ValidatedEdit<'a> {
    edit: &'a Edit,
    start_char: usize,
    end_char: usize,
    deleted: String,
}

impl TextBuffer {
    /// Validates a batch against one revision, then commits it atomically.
    pub fn apply_edits(
        &mut self,
        expected_revision: Revision,
        edits: &[Edit],
    ) -> CoreResult<AppliedEditBatch> {
        let before_revision = self.revision;
        if expected_revision != before_revision {
            return Err(CoreError::StaleRevision {
                expected: expected_revision,
                actual: before_revision,
            });
        }
        if edits.is_empty() {
            return Ok(AppliedEditBatch {
                before_revision,
                revision: before_revision,
                inverse: Vec::new(),
                changed: false,
            });
        }

        let snapshot = self.snapshot();
        let mut order: Vec<usize> = (0..edits.len()).collect();
        order.sort_by_key(|&index| (edits[index].range.start, edits[index].range.end));
        for pair in order.windows(2) {
            let previous = edits[pair[0]].range;
            let current = edits[pair[1]].range;
            if previous.end > current.start || previous.start == current.start {
                return Err(CoreError::OverlappingEdits);
            }
        }

        let mut validated = Vec::with_capacity(edits.len());
        for index in order {
            let edit = &edits[index];
            snapshot.validate_range(edit.range)?;
            let deleted = snapshot.slice(edit.range)?;
            if deleted == edit.replacement {
                continue;
            }
            validated.push(ValidatedEdit {
                edit,
                start_char: snapshot.char_index(edit.range.start)?,
                end_char: snapshot.char_index(edit.range.end)?,
                deleted,
            });
        }

        if validated.is_empty() {
            return Ok(AppliedEditBatch {
                before_revision,
                revision: before_revision,
                inverse: Vec::new(),
                changed: false,
            });
        }
        let next_revision = before_revision.next().ok_or(CoreError::RevisionExhausted)?;

        let mut inverse = Vec::with_capacity(validated.len());
        let mut byte_shift: i128 = 0;
        for item in &validated {
            let new_start = (item.edit.range.start.0 as i128)
                .checked_add(byte_shift)
                .and_then(|value| usize::try_from(value).ok())
                .ok_or(CoreError::BufferMutation)?;
            let new_end = new_start
                .checked_add(item.edit.replacement.len())
                .ok_or(CoreError::BufferMutation)?;
            inverse.push(Edit::new(
                ByteRange::new(new_start, new_end),
                item.deleted.clone(),
            ));
            byte_shift += item.edit.replacement.len() as i128 - item.edit.range.len() as i128;
        }

        // Rope's clone shares unchanged tree nodes; mutations happen on a candidate so
        // an unexpected library error cannot leave the visible buffer half edited.
        let mut candidate = self.rope.clone();
        for item in validated.iter().rev() {
            if item.start_char != item.end_char {
                candidate
                    .try_remove(item.start_char..item.end_char)
                    .map_err(|_| CoreError::BufferMutation)?;
            }
            if !item.edit.replacement.is_empty() {
                candidate
                    .try_insert(item.start_char, &item.edit.replacement)
                    .map_err(|_| CoreError::BufferMutation)?;
            }
        }

        self.rope = candidate;
        self.revision = next_revision;
        Ok(AppliedEditBatch {
            before_revision,
            revision: next_revision,
            inverse,
            changed: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ByteOffset, TextSnapshot};

    fn text(buffer: &TextBuffer) -> String {
        buffer.snapshot().to_string()
    }

    #[test]
    fn applies_unicode_edits_and_returns_inverse_ranges() {
        let mut buffer = TextBuffer::new("aé中");
        let result = buffer
            .apply_edits(Revision(0), &[Edit::new(ByteRange::new(1, 3), "🙂")])
            .unwrap();
        assert_eq!(text(&buffer), "a🙂中");
        assert_eq!(result.revision, Revision(1));
        assert_eq!(result.inverse[0].range, ByteRange::new(1, 5));
        assert_eq!(result.inverse[0].replacement, "é");

        let undone = buffer.apply_edits(Revision(1), &result.inverse).unwrap();
        assert_eq!(text(&buffer), "aé中");
        assert_eq!(undone.revision, Revision(2));
    }

    #[test]
    fn descending_batch_edits_keep_original_offsets_and_are_atomic() {
        let mut buffer = TextBuffer::new("abcdef");
        buffer
            .apply_edits(
                Revision(0),
                &[
                    Edit::new(ByteRange::new(1, 2), "XYZ"),
                    Edit::new(ByteRange::new(4, 5), "Q"),
                ],
            )
            .unwrap();
        assert_eq!(text(&buffer), "aXYZcdQf");

        let before_error = text(&buffer);
        let revision = buffer.revision();
        let error = buffer.apply_edits(
            revision,
            &[
                Edit::new(ByteRange::new(0, 3), ""),
                Edit::new(ByteRange::new(2, 4), ""),
            ],
        );
        assert_eq!(error, Err(CoreError::OverlappingEdits));
        assert_eq!(text(&buffer), before_error);
        assert_eq!(buffer.revision(), revision);
    }

    #[test]
    fn invalid_offset_and_stale_revision_do_not_mutate() {
        let mut buffer = TextBuffer::new("é");
        let initial = buffer.snapshot();
        assert!(matches!(
            buffer.apply_edits(Revision(0), &[Edit::new(ByteRange::new(1, 1), "x")]),
            Err(CoreError::InvalidUtf8Boundary(ByteOffset(1)))
        ));
        assert_eq!(buffer.revision(), Revision(0));
        assert_eq!(buffer.snapshot().to_string(), initial.to_string());
        assert!(matches!(
            buffer.apply_edits(Revision(9), &[Edit::new(ByteRange::new(0, 0), "x")]),
            Err(CoreError::StaleRevision { .. })
        ));
        assert_eq!(buffer.snapshot().to_string(), "é");
    }

    #[test]
    fn snapshots_remain_stable_after_a_commit() {
        let mut buffer = TextBuffer::new("before");
        let snapshot: TextSnapshot = buffer.snapshot();
        buffer
            .apply_edits(Revision(0), &[Edit::new(ByteRange::new(0, 6), "after")])
            .unwrap();
        assert_eq!(snapshot.to_string(), "before");
        assert_eq!(snapshot.revision(), Revision(0));
        assert_eq!(text(&buffer), "after");
    }
}
