use dopad_core::{ByteOffset, ByteRange};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CursorAffinity {
    Upstream,
    #[default]
    Downstream,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cursor {
    pub(crate) offset: ByteOffset,
    pub(crate) affinity: CursorAffinity,
    pub(crate) preferred_column: Option<usize>,
}

impl Cursor {
    pub const fn offset(self) -> ByteOffset {
        self.offset
    }

    pub const fn affinity(self) -> CursorAffinity {
        self.affinity
    }

    pub const fn preferred_column(self) -> Option<usize> {
        self.preferred_column
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    pub(crate) anchor: Cursor,
    pub(crate) head: Cursor,
}

impl Selection {
    pub const fn caret(offset: ByteOffset) -> Self {
        let cursor = Cursor {
            offset,
            affinity: CursorAffinity::Downstream,
            preferred_column: None,
        };
        Self {
            anchor: cursor,
            head: cursor,
        }
    }

    pub const fn new(anchor: ByteOffset, head: ByteOffset) -> Self {
        Self {
            anchor: Cursor {
                offset: anchor,
                affinity: CursorAffinity::Downstream,
                preferred_column: None,
            },
            head: Cursor {
                offset: head,
                affinity: CursorAffinity::Downstream,
                preferred_column: None,
            },
        }
    }

    pub const fn anchor(self) -> Cursor {
        self.anchor
    }

    pub const fn head(self) -> Cursor {
        self.head
    }

    pub const fn is_empty(self) -> bool {
        self.anchor.offset.0 == self.head.offset.0
    }

    pub fn range(self) -> ByteRange {
        let start = self.anchor.offset.min(self.head.offset);
        let end = self.anchor.offset.max(self.head.offset);
        ByteRange { start, end }
    }
}
