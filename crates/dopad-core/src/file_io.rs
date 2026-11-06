use std::{
    collections::hash_map::DefaultHasher,
    fs::{self, File, Metadata},
    hash::{Hash, Hasher},
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Utf16LeNoBom,
    Utf16BeNoBom,
}

impl Encoding {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Utf8 | Self::Utf8Bom => "UTF-8",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Utf16LeNoBom => "UTF-16 LE (no BOM)",
            Self::Utf16BeNoBom => "UTF-16 BE (no BOM)",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum EolKind {
    Lf,
    CrLf,
    Cr,
    Mixed,
    None,
}

impl EolKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::CrLf => "CRLF",
            Self::Cr => "CR",
            Self::Mixed => "Mixed",
            Self::None => "None",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileFingerprint {
    length: u64,
    modified: Option<SystemTime>,
    content_hash: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileFingerprintData {
    pub length: u64,
    pub modified_unix_nanos: Option<i128>,
    pub content_hash: u64,
}

impl FileFingerprint {
    fn new(bytes: &[u8], metadata: &Metadata) -> Self {
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            content_hash: hasher.finish(),
        }
    }

    pub fn to_data(self) -> FileFingerprintData {
        let modified_unix_nanos =
            self.modified
                .map(|modified| match modified.duration_since(UNIX_EPOCH) {
                    Ok(duration) => i128::try_from(duration.as_nanos()).unwrap_or(i128::MAX),
                    Err(error) => -i128::try_from(error.duration().as_nanos()).unwrap_or(i128::MAX),
                });
        FileFingerprintData {
            length: self.length,
            modified_unix_nanos,
            content_hash: self.content_hash,
        }
    }
}

impl From<FileFingerprintData> for FileFingerprint {
    fn from(data: FileFingerprintData) -> Self {
        let modified = data
            .modified_unix_nanos
            .and_then(system_time_from_unix_nanos);
        Self {
            length: data.length,
            modified,
            content_hash: data.content_hash,
        }
    }
}

fn system_time_from_unix_nanos(nanos: i128) -> Option<SystemTime> {
    let absolute = nanos.unsigned_abs();
    let seconds = u64::try_from(absolute / 1_000_000_000).ok()?;
    let subsecond_nanos = (absolute % 1_000_000_000) as u32;
    let duration = Duration::new(seconds, subsecond_nanos);
    if nanos >= 0 {
        UNIX_EPOCH.checked_add(duration)
    } else {
        UNIX_EPOCH.checked_sub(duration)
    }
}

#[derive(Debug)]
pub struct LoadedFile {
    pub path: PathBuf,
    pub text: String,
    pub encoding: Encoding,
    pub eol: EolKind,
    pub fingerprint: FileFingerprint,
}

#[derive(Debug)]
pub enum FileError {
    Io { path: PathBuf, source: io::Error },
    NotRegularFile(PathBuf),
    UnsupportedEncoding(PathBuf),
    InvalidText(PathBuf),
    ExternalModification(PathBuf),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::NotRegularFile(path) => write!(f, "{} is not a regular file", path.display()),
            Self::UnsupportedEncoding(path) => {
                write!(f, "{} has an unsupported text encoding", path.display())
            }
            Self::InvalidText(path) => {
                write!(
                    f,
                    "{} contains malformed text for its declared encoding",
                    path.display()
                )
            }
            Self::ExternalModification(path) => {
                write!(
                    f,
                    "{} changed outside Dopad; save was cancelled",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for FileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn read_file(path: impl AsRef<Path>) -> Result<LoadedFile, FileError> {
    read_file_with_options(path, true, Encoding::Utf8)
}

pub fn file_changed_since(
    path: impl AsRef<Path>,
    expected: FileFingerprint,
) -> Result<bool, FileError> {
    let path = path.as_ref();
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(source) => {
            return Err(FileError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if !metadata.is_file() {
        return Err(FileError::NotRegularFile(path.to_path_buf()));
    }
    Ok(metadata.len() != expected.length || metadata.modified().ok() != expected.modified)
}

pub fn read_file_with_options(
    path: impl AsRef<Path>,
    detect_encoding: bool,
    default_encoding: Encoding,
) -> Result<LoadedFile, FileError> {
    let requested = path.as_ref();
    let path = requested
        .canonicalize()
        .map_err(|source| io_error(requested, source))?;
    let file = File::open(&path).map_err(|source| io_error(&path, source))?;
    read_open_file_with_prefix(path, file, Vec::new(), detect_encoding, default_encoding)
}

/// Decodes one open file after its caller has consumed a prefix for content detection.
/// `prefix` must contain the bytes before the handle's current position.
pub fn read_open_file_with_prefix(
    path: impl AsRef<Path>,
    mut file: File,
    mut prefix: Vec<u8>,
    detect_encoding: bool,
    default_encoding: Encoding,
) -> Result<LoadedFile, FileError> {
    let path = path.as_ref().to_path_buf();
    let metadata = file.metadata().map_err(|source| io_error(&path, source))?;
    if !metadata.is_file() {
        return Err(FileError::NotRegularFile(path));
    }

    let position = file
        .stream_position()
        .map_err(|source| io_error(&path, source))?;
    if usize::try_from(position).ok() != Some(prefix.len()) || position > metadata.len() {
        return Err(io_error(
            &path,
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "the signature prefix does not match the open file position",
            ),
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len().min(16 * 1024 * 1024) as usize);
    bytes.append(&mut prefix);
    file.read_to_end(&mut bytes)
        .map_err(|source| io_error(&path, source))?;
    let after = file.metadata().map_err(|source| io_error(&path, source))?;
    if metadata.len() != after.len() || metadata.modified().ok() != after.modified().ok() {
        return Err(FileError::ExternalModification(path));
    }

    let fingerprint = FileFingerprint::new(&bytes, &after);
    let decoded = if detect_encoding || has_bom(&bytes) {
        decode(&bytes)
    } else {
        decode_as(&bytes, default_encoding)
    };
    let (encoding, text) = decoded.map_err(|problem| match problem {
        DecodeError::Unsupported => FileError::UnsupportedEncoding(path.clone()),
        DecodeError::Invalid => FileError::InvalidText(path.clone()),
    })?;
    let eol = detect_eol(&text);
    Ok(LoadedFile {
        path,
        text,
        encoding,
        eol,
        fingerprint,
    })
}

pub fn save_file_atomic(
    path: impl AsRef<Path>,
    text: &str,
    encoding: Encoding,
    expected: Option<FileFingerprint>,
) -> Result<FileFingerprint, FileError> {
    let requested = path.as_ref();
    let path = absolute_target(requested)?;
    let existing = match fs::metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(FileError::NotRegularFile(path));
            }
            Some(metadata)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(source) => return Err(io_error(&path, source)),
    };

    if let Some(expected) = expected {
        let current = existing
            .as_ref()
            .ok_or_else(|| FileError::ExternalModification(path.clone()))?;
        let current_bytes = fs::read(&path).map_err(|source| io_error(&path, source))?;
        if FileFingerprint::new(&current_bytes, current) != expected {
            return Err(FileError::ExternalModification(path));
        }
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = NamedTempFile::new_in(parent).map_err(|source| io_error(parent, source))?;
    if let Some(metadata) = &existing {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())
            .map_err(|source| io_error(&path, source))?;
    }
    let bytes = encode(text, encoding);
    temporary
        .write_all(&bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| io_error(&path, source))?;
    temporary
        .persist(&path)
        .map_err(|error| io_error(&path, error.error))?;

    #[cfg(unix)]
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| io_error(parent, source))?;

    let metadata = fs::metadata(&path).map_err(|source| io_error(&path, source))?;
    Ok(FileFingerprint::new(&bytes, &metadata))
}

fn absolute_target(path: &Path) -> Result<PathBuf, FileError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| io_error(path, source))?
            .join(path)
    };
    let parent = absolute.parent().unwrap_or_else(|| Path::new("."));
    let parent = parent
        .canonicalize()
        .map_err(|source| io_error(parent, source))?;
    let name = absolute.file_name().ok_or_else(|| FileError::Io {
        path: absolute.clone(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "missing file name"),
    })?;
    Ok(parent.join(name))
}

fn has_bom(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xEF, 0xBB, 0xBF])
        || bytes.starts_with(&[0xFF, 0xFE])
        || bytes.starts_with(&[0xFE, 0xFF])
        || bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF])
        || bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00])
}

fn decode_as(bytes: &[u8], encoding: Encoding) -> Result<(Encoding, String), DecodeError> {
    match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => std::str::from_utf8(bytes)
            .map(|text| (encoding, text.to_owned()))
            .map_err(|_| DecodeError::Invalid),
        Encoding::Utf16Le | Encoding::Utf16LeNoBom => {
            decode_utf16(bytes, true).map(|text| (encoding, text))
        }
        Encoding::Utf16Be | Encoding::Utf16BeNoBom => {
            decode_utf16(bytes, false).map(|text| (encoding, text))
        }
    }
}

fn decode(bytes: &[u8]) -> Result<(Encoding, String), DecodeError> {
    if bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) || bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00])
    {
        return Err(DecodeError::Unsupported);
    }
    if let Some(contents) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return std::str::from_utf8(contents)
            .map(|text| (Encoding::Utf8Bom, text.to_owned()))
            .map_err(|_| DecodeError::Invalid);
    }
    if let Some(contents) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16(contents, true).map(|text| (Encoding::Utf16Le, text));
    }
    if let Some(contents) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16(contents, false).map(|text| (Encoding::Utf16Be, text));
    }
    std::str::from_utf8(bytes)
        .map(|text| (Encoding::Utf8, text.to_owned()))
        .map_err(|_| DecodeError::Invalid)
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> Result<String, DecodeError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(DecodeError::Invalid);
    }
    let (pairs, _) = bytes.as_chunks::<2>();
    let units = pairs.iter().map(|pair| {
        if little_endian {
            u16::from_le_bytes(*pair)
        } else {
            u16::from_be_bytes(*pair)
        }
    });
    char::decode_utf16(units)
        .map(|item| item.map_err(|_| DecodeError::Invalid))
        .collect()
}

fn encode(text: &str, encoding: Encoding) -> Vec<u8> {
    match encoding {
        Encoding::Utf8 => text.as_bytes().to_vec(),
        Encoding::Utf8Bom => [b"\xEF\xBB\xBF".as_slice(), text.as_bytes()].concat(),
        Encoding::Utf16Le | Encoding::Utf16Be | Encoding::Utf16LeNoBom | Encoding::Utf16BeNoBom => {
            let mut bytes = Vec::with_capacity(2 + text.len() * 2);
            if matches!(encoding, Encoding::Utf16Le | Encoding::Utf16LeNoBom) {
                if encoding == Encoding::Utf16Le {
                    bytes.extend_from_slice(&[0xFF, 0xFE]);
                }
                bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            } else {
                if encoding == Encoding::Utf16Be {
                    bytes.extend_from_slice(&[0xFE, 0xFF]);
                }
                bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
            }
            bytes
        }
    }
}

pub fn detect_eol(text: &str) -> EolKind {
    let bytes = text.as_bytes();
    let (mut lf, mut crlf, mut cr) = (false, false, false);
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                crlf = true;
                index += 2;
            }
            b'\r' => {
                cr = true;
                index += 1;
            }
            b'\n' => {
                lf = true;
                index += 1;
            }
            _ => index += 1,
        }
    }
    match (lf, crlf, cr) {
        (false, false, false) => EolKind::None,
        (true, false, false) => EolKind::Lf,
        (false, true, false) => EolKind::CrLf,
        (false, false, true) => EolKind::Cr,
        _ => EolKind::Mixed,
    }
}

fn io_error(path: &Path, source: io::Error) -> FileError {
    FileError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Debug)]
enum DecodeError {
    Unsupported,
    Invalid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_each_line_ending_and_mixed_files() {
        assert_eq!(detect_eol("a\nb"), EolKind::Lf);
        assert_eq!(detect_eol("a\r\nb"), EolKind::CrLf);
        assert_eq!(detect_eol("a\rb"), EolKind::Cr);
        assert_eq!(detect_eol("a\r\nb\nc"), EolKind::Mixed);
        assert_eq!(detect_eol("a"), EolKind::None);
    }

    #[test]
    fn read_open_file_consumes_signature_prefix_without_reading_it_twice() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("sniffed.txt");
        fs::write(&path, "Dopad 世界\n").expect("write text fixture");
        let mut file = File::open(&path).expect("open text fixture");
        let mut prefix = [0; 5];
        file.read_exact(&mut prefix).expect("read signature prefix");
        assert_eq!(&prefix, b"Dopad");

        let loaded = read_open_file_with_prefix(&path, file, prefix.to_vec(), true, Encoding::Utf8)
            .expect("decode after signature sniffing");
        assert_eq!(loaded.text, "Dopad 世界\n");
    }

    #[test]
    fn file_fingerprint_serialization_round_trips_system_times() {
        for modified in [
            UNIX_EPOCH.checked_add(Duration::new(1_700_000_000, 123_456_789)),
            UNIX_EPOCH.checked_sub(Duration::new(1, 987_654_321)),
            Some(UNIX_EPOCH),
        ] {
            let fingerprint = FileFingerprint {
                length: 17,
                modified,
                content_hash: 0x1234,
            };
            assert_eq!(FileFingerprint::from(fingerprint.to_data()), fingerprint);
        }

        let out_of_range = FileFingerprint::from(FileFingerprintData {
            length: 0,
            modified_unix_nanos: Some(i128::MAX),
            content_hash: 0,
        });
        assert_eq!(out_of_range.modified, None);
    }

    #[test]
    fn detects_external_file_metadata_changes_and_deletion() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("watch.txt");
        fs::write(&path, "one").expect("write original file");
        let original = read_file(&path).expect("read original file");
        assert!(!file_changed_since(&path, original.fingerprint).expect("check unchanged file"));

        fs::write(&path, "a longer replacement").expect("replace file externally");
        assert!(file_changed_since(&path, original.fingerprint).expect("check changed file"));

        fs::remove_file(&path).expect("remove file externally");
        assert!(file_changed_since(&path, original.fingerprint).expect("check deleted file"));
    }

    #[test]
    fn decodes_and_encodes_bom_variants_without_replacing_invalid_text() {
        for (encoding, bytes) in [
            (Encoding::Utf8, b"text".to_vec()),
            (Encoding::Utf8Bom, b"\xEF\xBB\xBFtext".to_vec()),
            (Encoding::Utf16Le, b"\xFF\xFEt\0e\0x\0t\0".to_vec()),
            (Encoding::Utf16Be, b"\xFE\xFF\0t\0e\0x\0t".to_vec()),
        ] {
            assert_eq!(
                decode(&bytes).expect("valid encoding"),
                (encoding, "text".into())
            );
            assert_eq!(
                decode(&encode("text", encoding)).expect("round trip"),
                (encoding, "text".into())
            );
        }
        let unicode = "Dopad 世界🙂\r\n";
        for encoding in [
            Encoding::Utf8,
            Encoding::Utf8Bom,
            Encoding::Utf16Le,
            Encoding::Utf16Be,
        ] {
            assert_eq!(
                decode(&encode(unicode, encoding)).expect("unicode round trip"),
                (encoding, unicode.into())
            );
        }
        assert!(matches!(decode(b"\xFF\xFE\0"), Err(DecodeError::Invalid)));
        assert!(matches!(
            decode(b"\xFF\xFE\0\xD8"),
            Err(DecodeError::Invalid)
        ));
        assert!(matches!(
            decode(b"\xFF\xFE\x00\xDC"),
            Err(DecodeError::Invalid)
        ));
        assert!(matches!(decode(b"\xFF"), Err(DecodeError::Invalid)));
        assert!(matches!(
            decode(b"\xFF\xFE\0\0"),
            Err(DecodeError::Unsupported)
        ));
    }

    #[test]
    fn configured_encoding_reads_and_writes_utf16_without_bom() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("utf16.txt");
        let bytes = [b't', 0, b'e', 0, b'x', 0, b't', 0];
        fs::write(&path, bytes).expect("write no-BOM UTF-16");
        let loaded = read_file_with_options(&path, false, Encoding::Utf16LeNoBom)
            .expect("decode configured UTF-16");
        assert_eq!(loaded.encoding, Encoding::Utf16LeNoBom);
        assert_eq!(loaded.text, "text");
        assert_eq!(encode("text", Encoding::Utf16LeNoBom), bytes);
        let wrong_default =
            read_file_with_options(&path, false, Encoding::Utf8).expect("valid UTF-8 bytes");
        assert_ne!(wrong_default.text, "text");
    }

    #[test]
    fn atomic_save_preserves_encoding_and_detects_external_changes() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("notes.txt");
        fs::write(&path, b"\xEF\xBB\xBFold\r\n").expect("seed file");
        let loaded = read_file(&path).expect("load file");
        assert_eq!(loaded.eol, EolKind::CrLf);
        let receipt = save_file_atomic(&path, "new\r\n", loaded.encoding, Some(loaded.fingerprint))
            .expect("save unchanged file");
        assert_eq!(
            fs::read(&path).expect("saved bytes"),
            b"\xEF\xBB\xBFnew\r\n"
        );
        fs::write(&path, b"someone else's edit").expect("external edit");
        assert!(matches!(
            save_file_atomic(&path, "overwrite", Encoding::Utf8, Some(receipt)),
            Err(FileError::ExternalModification(_))
        ));
        assert_eq!(
            fs::read(&path).expect("preserved external bytes"),
            b"someone else's edit"
        );
    }
}
