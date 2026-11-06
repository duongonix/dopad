#![forbid(unsafe_code)]

//! UI-independent, revisioned text primitives for Dopad.

mod buffer;
mod document;
mod error;
mod file_io;
mod session;
mod types;

pub use buffer::{TextBuffer, TextSnapshot};
pub use document::{AppliedEditBatch, Edit};
pub use error::{CoreError, CoreResult};
pub use file_io::{
    Encoding, EolKind, FileError, FileFingerprint, FileFingerprintData, LoadedFile, detect_eol,
    file_changed_since, read_file, read_file_with_options, read_open_file_with_prefix,
    save_file_atomic,
};
pub use session::{
    DraftCheckpoint, DraftReference, RecoveredDraft, RecoveredSession, SessionError,
    SessionManifest, SessionStore, SessionTab,
};
pub use types::{ByteOffset, ByteRange, Revision, TextPosition};
