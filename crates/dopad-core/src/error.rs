use crate::{ByteOffset, Revision};
use std::fmt;

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreError {
    InvalidRange {
        start: usize,
        end: usize,
        len: usize,
    },
    OutOfBounds {
        offset: usize,
        len: usize,
    },
    InvalidUtf8Boundary(ByteOffset),
    InvalidGraphemeBoundary(ByteOffset),
    InvalidPosition {
        line: usize,
        column: usize,
    },
    StaleRevision {
        expected: Revision,
        actual: Revision,
    },
    OverlappingEdits,
    RevisionExhausted,
    BufferMutation,
    GraphemeResolution,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRange { start, end, len } => {
                write!(
                    f,
                    "invalid byte range {start}..{end} for text of {len} bytes"
                )
            }
            Self::OutOfBounds { offset, len } => {
                write!(f, "byte offset {offset} exceeds text length {len}")
            }
            Self::InvalidUtf8Boundary(offset) => {
                write!(f, "byte offset {} is not a UTF-8 boundary", offset.0)
            }
            Self::InvalidGraphemeBoundary(offset) => {
                write!(f, "byte offset {} is not a grapheme boundary", offset.0)
            }
            Self::InvalidPosition { line, column } => {
                write!(f, "text position ({line}, {column}) is out of bounds")
            }
            Self::StaleRevision { expected, actual } => {
                write!(
                    f,
                    "stale revision {}, current revision is {}",
                    expected.0, actual.0
                )
            }
            Self::OverlappingEdits => f.write_str("edit batch contains overlapping ranges"),
            Self::RevisionExhausted => f.write_str("document revision counter is exhausted"),
            Self::BufferMutation => f.write_str("validated buffer mutation was rejected"),
            Self::GraphemeResolution => f.write_str("could not resolve grapheme boundary"),
        }
    }
}

impl std::error::Error for CoreError {}
