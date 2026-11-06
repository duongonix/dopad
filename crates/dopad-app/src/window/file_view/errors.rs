use std::path::{Path, PathBuf};

use dopad_core::FileError;

use super::{FileOpenError, FileOpenErrorKind};

pub(super) fn io_error(
    path: &Path,
    file_type: &str,
    file_size: Option<u64>,
    kind: std::io::ErrorKind,
    detail: String,
) -> FileOpenError {
    let (error_kind, message) = match kind {
        std::io::ErrorKind::PermissionDenied => (
            FileOpenErrorKind::PermissionDenied,
            "Permission denied. Dopad cannot read this file.".into(),
        ),
        std::io::ErrorKind::NotFound => (
            FileOpenErrorKind::NotFound,
            "This file no longer exists at its current path.".into(),
        ),
        _ => (
            FileOpenErrorKind::Io,
            "Dopad could not read this file.".into(),
        ),
    };
    tracing::debug!(path = %path.display(), error = %detail, "file view I/O failed");
    FileOpenError {
        kind: error_kind,
        file_type: file_type.to_owned(),
        file_size,
        message,
        detail,
    }
}

pub(super) fn file_error(
    path: PathBuf,
    file_type: String,
    file_size: u64,
    error: FileError,
) -> FileOpenError {
    let (kind, message) = match &error {
        FileError::Io { source, .. } if source.kind() == std::io::ErrorKind::PermissionDenied => (
            FileOpenErrorKind::PermissionDenied,
            "Permission denied. Dopad cannot read this file.".into(),
        ),
        FileError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => (
            FileOpenErrorKind::NotFound,
            "This file no longer exists at its current path.".into(),
        ),
        FileError::UnsupportedEncoding(_) => (
            FileOpenErrorKind::UnsupportedEncoding,
            "Dopad does not support this text encoding.".into(),
        ),
        FileError::InvalidText(_) => (
            FileOpenErrorKind::InvalidText,
            "Dopad could not decode this file as text.".into(),
        ),
        _ => (
            FileOpenErrorKind::Io,
            "Dopad could not read this file.".into(),
        ),
    };
    tracing::debug!(path = %path.display(), error = %error, "text file view failed");
    FileOpenError {
        kind,
        file_type,
        file_size: Some(file_size),
        message,
        detail: error.to_string(),
    }
}

pub(super) fn invalid_image(file_type: &str, file_size: u64, detail: String) -> FileOpenError {
    image_error(
        FileOpenErrorKind::InvalidImage,
        file_type,
        Some(file_size),
        "Dopad could not decode this image. The file may be corrupted or invalid.",
        detail,
    )
}

pub(super) fn image_error(
    kind: FileOpenErrorKind,
    file_type: &str,
    file_size: Option<u64>,
    message: &str,
    detail: String,
) -> FileOpenError {
    FileOpenError {
        kind,
        file_type: file_type.to_owned(),
        file_size,
        message: message.to_owned(),
        detail,
    }
}
