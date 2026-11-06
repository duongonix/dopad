use crate::{ByteOffset, ByteRange, CoreError, CoreResult, Revision, TextPosition};
use ropey::Rope;
use std::fmt;
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

pub struct TextBuffer {
    pub(crate) rope: Rope,
    pub(crate) revision: Revision,
}

/// An immutable, cheaply cloned view of one document revision.
#[derive(Clone)]
pub struct TextSnapshot {
    rope: Rope,
    revision: Revision,
}

impl TextBuffer {
    pub fn new(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            revision: Revision::default(),
        }
    }

    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub fn snapshot(&self) -> TextSnapshot {
        TextSnapshot {
            rope: self.rope.clone(),
            revision: self.revision,
        }
    }
}

impl TextSnapshot {
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    pub fn slice(&self, range: ByteRange) -> CoreResult<String> {
        self.validate_range(range)?;
        let slice = self.rope.get_byte_slice(range.start.0..range.end.0).ok_or(
            CoreError::InvalidRange {
                start: range.start.0,
                end: range.end.0,
                len: self.len_bytes(),
            },
        )?;
        Ok(slice.to_string())
    }

    pub fn validate_range(&self, range: ByteRange) -> CoreResult<()> {
        if range.start.0 > range.end.0 || range.end.0 > self.len_bytes() {
            return Err(CoreError::InvalidRange {
                start: range.start.0,
                end: range.end.0,
                len: self.len_bytes(),
            });
        }
        self.validate_char_boundary(range.start)?;
        self.validate_char_boundary(range.end)
    }

    pub fn validate_char_boundary(&self, offset: ByteOffset) -> CoreResult<()> {
        let len = self.len_bytes();
        if offset.0 > len {
            return Err(CoreError::OutOfBounds {
                offset: offset.0,
                len,
            });
        }
        if offset.0 == 0 || offset.0 == len {
            return Ok(());
        }
        let (chunk, start, _, _) = self.rope.chunk_at_byte(offset.0);
        if chunk.is_char_boundary(offset.0 - start) {
            Ok(())
        } else {
            Err(CoreError::InvalidUtf8Boundary(offset))
        }
    }

    pub(crate) fn char_index(&self, offset: ByteOffset) -> CoreResult<usize> {
        self.validate_char_boundary(offset)?;
        self.rope
            .try_byte_to_char(offset.0)
            .map_err(|_| CoreError::InvalidUtf8Boundary(offset))
    }

    pub fn is_grapheme_boundary(&self, offset: ByteOffset) -> CoreResult<bool> {
        self.validate_char_boundary(offset)?;
        let len = self.len_bytes();
        if offset.0 == 0 || offset.0 == len {
            return Ok(true);
        }

        let (chunk, chunk_start, _, _) = self.rope.chunk_at_byte(offset.0);
        let mut cursor = GraphemeCursor::new(offset.0, len, false);
        loop {
            match cursor.is_boundary(chunk, chunk_start) {
                Ok(is_boundary) => return Ok(is_boundary),
                Err(GraphemeIncomplete::PreContext(requested)) => {
                    self.provide_context(&mut cursor, requested)?;
                }
                Err(_) => return Err(CoreError::GraphemeResolution),
            }
        }
    }

    pub fn validate_grapheme_boundary(&self, offset: ByteOffset) -> CoreResult<()> {
        if self.is_grapheme_boundary(offset)? {
            Ok(())
        } else {
            Err(CoreError::InvalidGraphemeBoundary(offset))
        }
    }

    pub fn next_grapheme_boundary(&self, offset: ByteOffset) -> CoreResult<Option<ByteOffset>> {
        self.validate_char_boundary(offset)?;
        if offset.0 == self.len_bytes() {
            return Ok(None);
        }

        let (mut chunk, mut chunk_start, _, _) = self.rope.chunk_at_byte(offset.0);
        let mut cursor = GraphemeCursor::new(offset.0, self.len_bytes(), false);
        loop {
            match cursor.next_boundary(chunk, chunk_start) {
                Ok(boundary) => return Ok(boundary.map(ByteOffset)),
                Err(GraphemeIncomplete::NextChunk) => {
                    let next_start = chunk_start + chunk.len();
                    if next_start >= self.len_bytes() {
                        return Err(CoreError::GraphemeResolution);
                    }
                    (chunk, chunk_start, _, _) = self.rope.chunk_at_byte(next_start);
                }
                Err(GraphemeIncomplete::PreContext(requested)) => {
                    self.provide_context(&mut cursor, requested)?;
                }
                Err(_) => return Err(CoreError::GraphemeResolution),
            }
        }
    }

    pub fn previous_grapheme_boundary(&self, offset: ByteOffset) -> CoreResult<Option<ByteOffset>> {
        self.validate_char_boundary(offset)?;
        if offset.0 == 0 {
            return Ok(None);
        }

        let (mut chunk, mut chunk_start, _, _) = self.rope.chunk_at_byte(offset.0);
        let mut cursor = GraphemeCursor::new(offset.0, self.len_bytes(), false);
        loop {
            match cursor.prev_boundary(chunk, chunk_start) {
                Ok(boundary) => return Ok(boundary.map(ByteOffset)),
                Err(GraphemeIncomplete::PrevChunk) => {
                    if chunk_start == 0 {
                        return Err(CoreError::GraphemeResolution);
                    }
                    (chunk, chunk_start, _, _) = self.rope.chunk_at_byte(chunk_start - 1);
                }
                Err(GraphemeIncomplete::PreContext(requested)) => {
                    self.provide_context(&mut cursor, requested)?;
                }
                Err(_) => return Err(CoreError::GraphemeResolution),
            }
        }
    }

    pub fn line_content_range(&self, line: usize) -> CoreResult<ByteRange> {
        let Some(slice) = self.rope.get_line(line) else {
            return Err(CoreError::InvalidPosition { line, column: 0 });
        };
        let start = self.rope.line_to_byte(line);
        let mut content_bytes = slice.len_bytes();
        let last_index = slice.len_chars().checked_sub(1);
        match last_index.and_then(|index| slice.get_char(index)) {
            Some('\n') => {
                content_bytes -= 1;
                if last_index
                    .and_then(|index| index.checked_sub(1))
                    .and_then(|index| slice.get_char(index))
                    == Some('\r')
                {
                    content_bytes -= 1;
                }
            }
            Some('\r') => content_bytes -= 1,
            _ => {}
        }
        Ok(ByteRange::new(start, start + content_bytes))
    }

    pub fn byte_to_position(&self, offset: ByteOffset) -> CoreResult<TextPosition> {
        self.validate_grapheme_boundary(offset)?;
        let line = self.rope.byte_to_line(offset.0);
        let content = self.line_content_range(line)?;
        if offset.0 > content.end.0 {
            return Err(CoreError::InvalidPosition { line, column: 0 });
        }
        let mut cursor = content.start;
        let mut column = 0;
        while cursor < offset {
            let next = self
                .next_grapheme_boundary(cursor)?
                .ok_or(CoreError::GraphemeResolution)?;
            if next > content.end {
                return Err(CoreError::InvalidPosition { line, column });
            }
            cursor = next;
            column += 1;
        }
        Ok(TextPosition::new(line, column))
    }

    pub fn position_to_byte(&self, position: TextPosition) -> CoreResult<ByteOffset> {
        self.position_to_byte_with_clamp(position, false)
    }

    pub fn position_to_byte_clamped(&self, position: TextPosition) -> CoreResult<ByteOffset> {
        self.position_to_byte_with_clamp(position, true)
    }

    fn position_to_byte_with_clamp(
        &self,
        position: TextPosition,
        clamp: bool,
    ) -> CoreResult<ByteOffset> {
        let content = self.line_content_range(position.line)?;
        let mut cursor = content.start;
        for _ in 0..position.column {
            if cursor == content.end {
                if clamp {
                    return Ok(cursor);
                }
                return Err(CoreError::InvalidPosition {
                    line: position.line,
                    column: position.column,
                });
            }
            cursor = self
                .next_grapheme_boundary(cursor)?
                .ok_or(CoreError::GraphemeResolution)?;
            if cursor > content.end {
                return Err(CoreError::InvalidPosition {
                    line: position.line,
                    column: position.column,
                });
            }
        }
        Ok(cursor)
    }

    fn provide_context(&self, cursor: &mut GraphemeCursor, requested: usize) -> CoreResult<()> {
        if requested == 0 || requested > self.len_bytes() {
            return Err(CoreError::GraphemeResolution);
        }
        let (context, start, _, _) = self.rope.chunk_at_byte(requested - 1);
        let prefix_len = requested - start;
        let Some(prefix) = context.get(..prefix_len) else {
            return Err(CoreError::GraphemeResolution);
        };
        cursor.provide_context(prefix, start);
        Ok(())
    }
}

impl fmt::Display for TextSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for chunk in self.rope.chunks() {
            f.write_str(chunk)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_utf8_bytes_to_grapheme_positions() {
        let text = TextBuffer::new("aé中").snapshot();
        assert_eq!(
            text.byte_to_position(ByteOffset(0)).unwrap(),
            TextPosition::new(0, 0)
        );
        assert_eq!(
            text.byte_to_position(ByteOffset(1)).unwrap(),
            TextPosition::new(0, 1)
        );
        assert_eq!(
            text.byte_to_position(ByteOffset(3)).unwrap(),
            TextPosition::new(0, 2)
        );
        assert_eq!(
            text.position_to_byte(TextPosition::new(0, 2)).unwrap(),
            ByteOffset(3)
        );
        assert!(matches!(
            text.slice(ByteRange::new(2, 3)),
            Err(CoreError::InvalidUtf8Boundary(_))
        ));
    }

    #[test]
    fn grapheme_navigation_keeps_combining_and_emoji_sequences_together() {
        let value = "e\u{301}👩‍💻🇹🇼";
        let text = TextBuffer::new(value).snapshot();
        let first = text.next_grapheme_boundary(ByteOffset(0)).unwrap().unwrap();
        let second = text.next_grapheme_boundary(first).unwrap().unwrap();
        let third = text.next_grapheme_boundary(second).unwrap().unwrap();
        assert_eq!(first.0, "e\u{301}".len());
        assert_eq!(second.0 - first.0, "👩‍💻".len());
        assert_eq!(third.0 - second.0, "🇹🇼".len());
        assert_eq!(
            text.previous_grapheme_boundary(third).unwrap(),
            Some(second)
        );
        assert!(matches!(
            text.validate_grapheme_boundary(ByteOffset(1)),
            Err(CoreError::InvalidGraphemeBoundary(_))
        ));
    }

    #[test]
    fn grapheme_navigation_handles_a_cluster_split_across_rope_chunks() {
        let mut value = "a".repeat(1023);
        value.push('e');
        value.push('\u{301}');
        value.push('x');
        let text = TextBuffer::new(&value).snapshot();
        let start = ByteOffset(1023);
        let end = ByteOffset(1023 + "e\u{301}".len());

        assert_eq!(text.next_grapheme_boundary(start).unwrap(), Some(end));
        assert_eq!(text.previous_grapheme_boundary(end).unwrap(), Some(start));
        assert!(!text.is_grapheme_boundary(ByteOffset(1024)).unwrap());
    }

    #[test]
    fn line_positions_exclude_lf_and_crlf_endings() {
        let text = TextBuffer::new("one\r\ntwo\n").snapshot();
        assert_eq!(text.line_count(), 3);
        assert_eq!(text.line_content_range(0).unwrap(), ByteRange::new(0, 3));
        assert_eq!(text.line_content_range(1).unwrap(), ByteRange::new(5, 8));
        assert_eq!(
            text.position_to_byte(TextPosition::new(1, 3)).unwrap(),
            ByteOffset(8)
        );
        assert_eq!(
            text.byte_to_position(ByteOffset(5)).unwrap(),
            TextPosition::new(1, 0)
        );
    }

    #[test]
    fn line_positions_exclude_standalone_cr_endings() {
        let text = TextBuffer::new("one\rtwo").snapshot();
        assert_eq!(text.line_count(), 2);
        assert_eq!(text.line_content_range(0).unwrap(), ByteRange::new(0, 3));
        assert_eq!(
            text.byte_to_position(ByteOffset(4)).unwrap(),
            TextPosition::new(1, 0)
        );
    }

    #[test]
    fn empty_buffer_has_one_addressable_line() {
        let text = TextBuffer::new("").snapshot();
        assert_eq!(text.line_count(), 1);
        assert_eq!(
            text.position_to_byte(TextPosition::new(0, 0)).unwrap(),
            ByteOffset(0)
        );
        assert!(
            text.next_grapheme_boundary(ByteOffset(0))
                .unwrap()
                .is_none()
        );
    }
}
