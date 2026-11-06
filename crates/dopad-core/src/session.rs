use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use thiserror::Error;

use crate::{Encoding, EolKind, FileFingerprintData};

const SCHEMA_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TAB_COUNT: usize = 512;
const MAX_DRAFT_BYTES: usize = 100 * 1024 * 1024;
const MAX_TOTAL_DRAFT_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionManifest {
    pub schema_version: u32,
    pub generation: u64,
    pub clean_shutdown: bool,
    pub active_tab_id: Option<u64>,
    pub dark_theme: bool,
    pub tabs: Vec<SessionTab>,
}

impl SessionManifest {
    pub fn empty() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            generation: 0,
            clean_shutdown: false,
            active_tab_id: None,
            dark_theme: true,
            tabs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionTab {
    pub id: u64,
    pub title: String,
    pub path: Option<PathBuf>,
    pub encoding: Encoding,
    pub eol: EolKind,
    pub fingerprint: Option<FileFingerprintData>,
    pub revision: u64,
    pub cursor: usize,
    pub selection_start: usize,
    pub selection_end: usize,
    pub word_wrap: bool,
    pub font_size: f32,
    pub preview_mode: bool,
    pub draft_ref: Option<DraftReference>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DraftReference {
    pub file_name: String,
    pub revision: u64,
    pub byte_len: u64,
    pub checksum: u64,
}

#[derive(Debug)]
pub struct DraftCheckpoint {
    pub document_id: u64,
    pub revision: u64,
    pub text: String,
}

#[derive(Debug)]
pub struct RecoveredDraft {
    pub document_id: u64,
    pub revision: u64,
    pub text: String,
}

#[derive(Debug)]
pub struct RecoveredSession {
    pub manifest: SessionManifest,
    pub drafts: Vec<RecoveredDraft>,
    pub was_unclean: bool,
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session storage {operation} failed: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("session storage is already in use by another Dopad instance")]
    AlreadyRunning,
    #[error("session manifest is corrupt")]
    CorruptManifest(#[source] serde_json::Error),
    #[error("session manifest could not be serialized")]
    ManifestEncoding(#[source] serde_json::Error),
    #[error("unsupported session schema version {0}")]
    UnsupportedVersion(u32),
    #[error("session manifest contains invalid data")]
    InvalidManifest,
    #[error("session data exceeds the configured size limit")]
    TooLarge,
    #[error("a recovery draft is missing, corrupt, or mismatched")]
    CorruptDraft,
    #[error("session state lock was poisoned")]
    StatePoisoned,
}

pub struct SessionStore {
    root: PathBuf,
    _lock: File,
    state: Mutex<SessionManifest>,
}

impl SessionStore {
    /// Opens and locks the session directory, returning its previously saved state.
    pub fn open(root: impl AsRef<Path>) -> Result<(Self, Option<RecoveredSession>), SessionError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|source| io_error("directory creation", source))?;
        set_private_directory(&root)?;

        let lock_path = root.join("session.lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|source| io_error("lock open", source))?;
        lock.try_lock().map_err(|_| SessionError::AlreadyRunning)?;

        let previous = read_manifest(&root)?;
        let was_unclean = previous
            .as_ref()
            .is_some_and(|manifest| !manifest.clean_shutdown);
        let recovered = previous
            .map(|manifest| read_recovered_session(&root, manifest, was_unclean))
            .transpose()?;

        let mut state = recovered
            .as_ref()
            .map(|session| session.manifest.clone())
            .unwrap_or_else(SessionManifest::empty);
        state.clean_shutdown = false;
        validate_manifest(&state)?;
        write_manifest(&root, &state)?;

        Ok((
            Self {
                root,
                _lock: lock,
                state: Mutex::new(state),
            },
            recovered,
        ))
    }

    pub fn next_generation(&self) -> Result<u64, SessionError> {
        let state = self.state.lock().map_err(|_| SessionError::StatePoisoned)?;
        Ok(state.generation.saturating_add(1))
    }

    /// Commits drafts first and the manifest last, preserving the previous checkpoint on failure.
    pub fn checkpoint(
        &self,
        mut manifest: SessionManifest,
        drafts: Vec<DraftCheckpoint>,
    ) -> Result<bool, SessionError> {
        let mut state = self.state.lock().map_err(|_| SessionError::StatePoisoned)?;
        if manifest.generation < state.generation {
            return Ok(false);
        }
        validate_manifest(&manifest)?;

        let mut draft_by_id = HashMap::with_capacity(drafts.len());
        let mut total_bytes = 0usize;
        for draft in drafts {
            if draft.text.len() > MAX_DRAFT_BYTES {
                return Err(SessionError::TooLarge);
            }
            total_bytes = total_bytes
                .checked_add(draft.text.len())
                .ok_or(SessionError::TooLarge)?;
            if total_bytes > MAX_TOTAL_DRAFT_BYTES {
                return Err(SessionError::TooLarge);
            }
            if draft_by_id.insert(draft.document_id, draft).is_some() {
                return Err(SessionError::InvalidManifest);
            }
        }

        let draft_dir = self.root.join("drafts");
        fs::create_dir_all(&draft_dir)
            .map_err(|source| io_error("draft directory creation", source))?;
        set_private_directory(&draft_dir)?;

        for tab in &mut manifest.tabs {
            let Some(draft) = draft_by_id.remove(&tab.id) else {
                tab.draft_ref = None;
                continue;
            };
            if draft.revision != tab.revision {
                return Err(SessionError::InvalidManifest);
            }
            let reference = draft_reference(tab.id, draft.revision, draft.text.as_bytes());
            write_draft(&draft_dir.join(&reference.file_name), draft.text.as_bytes())?;
            tab.draft_ref = Some(reference);
        }
        if !draft_by_id.is_empty() {
            return Err(SessionError::InvalidManifest);
        }

        manifest.clean_shutdown = state.clean_shutdown;
        validate_manifest(&manifest)?;
        write_manifest(&self.root, &manifest)?;
        *state = manifest;
        cleanup_orphan_drafts(&draft_dir, &state);
        Ok(true)
    }

    pub fn mark_clean_shutdown(&self) -> Result<(), SessionError> {
        let mut state = self.state.lock().map_err(|_| SessionError::StatePoisoned)?;
        state.clean_shutdown = true;
        if let Err(error) = write_manifest(&self.root, &state) {
            state.clean_shutdown = false;
            return Err(error);
        }
        Ok(())
    }

    /// Removes the current manifest and its drafts after an explicit user discard decision.
    pub fn discard(&self) -> Result<(), SessionError> {
        let mut state = self.state.lock().map_err(|_| SessionError::StatePoisoned)?;
        let manifest_path = self.root.join("session.json");
        match fs::remove_file(&manifest_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(io_error("manifest removal", source)),
        }
        let draft_dir = self.root.join("drafts");
        if draft_dir.exists() {
            fs::remove_dir_all(&draft_dir).map_err(|source| io_error("draft removal", source))?;
        }
        *state = SessionManifest::empty();
        state.clean_shutdown = false;
        write_manifest(&self.root, &state)
    }
}

fn read_manifest(root: &Path) -> Result<Option<SessionManifest>, SessionError> {
    let path = root.join("session.json");
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(io_error("manifest open", source)),
    };
    let metadata = file
        .metadata()
        .map_err(|source| io_error("manifest metadata read", source))?;
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(SessionError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|source| io_error("manifest read", source))?;
    let manifest: SessionManifest =
        serde_json::from_slice(&bytes).map_err(SessionError::CorruptManifest)?;
    validate_manifest(&manifest)?;
    Ok(Some(manifest))
}

fn read_recovered_session(
    root: &Path,
    manifest: SessionManifest,
    was_unclean: bool,
) -> Result<RecoveredSession, SessionError> {
    let draft_dir = root.join("drafts");
    let mut drafts = Vec::new();
    let mut total_bytes = 0usize;
    for tab in &manifest.tabs {
        let Some(reference) = &tab.draft_ref else {
            continue;
        };
        let path = draft_dir.join(&reference.file_name);
        let metadata = fs::metadata(&path).map_err(|_| SessionError::CorruptDraft)?;
        let expected_len =
            usize::try_from(reference.byte_len).map_err(|_| SessionError::TooLarge)?;
        if expected_len > MAX_DRAFT_BYTES || metadata.len() != reference.byte_len {
            return Err(SessionError::CorruptDraft);
        }
        total_bytes = total_bytes
            .checked_add(expected_len)
            .ok_or(SessionError::TooLarge)?;
        if total_bytes > MAX_TOTAL_DRAFT_BYTES {
            return Err(SessionError::TooLarge);
        }
        let bytes = fs::read(path).map_err(|_| SessionError::CorruptDraft)?;
        if checksum(&bytes) != reference.checksum {
            return Err(SessionError::CorruptDraft);
        }
        let text = String::from_utf8(bytes).map_err(|_| SessionError::CorruptDraft)?;
        drafts.push(RecoveredDraft {
            document_id: tab.id,
            revision: reference.revision,
            text,
        });
    }
    Ok(RecoveredSession {
        manifest,
        drafts,
        was_unclean,
    })
}

fn validate_manifest(manifest: &SessionManifest) -> Result<(), SessionError> {
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(SessionError::UnsupportedVersion(manifest.schema_version));
    }
    if manifest.tabs.len() > MAX_TAB_COUNT {
        return Err(SessionError::TooLarge);
    }
    let mut ids = HashSet::with_capacity(manifest.tabs.len());
    for tab in &manifest.tabs {
        if !ids.insert(tab.id)
            || !tab.font_size.is_finite()
            || !(8.0..=36.0).contains(&tab.font_size)
            || tab.selection_start > tab.selection_end
            || tab.draft_ref.as_ref().is_some_and(|reference| {
                reference.revision != tab.revision
                    || reference.file_name != draft_file_name(tab.id, reference.revision)
                    || reference.byte_len > MAX_DRAFT_BYTES as u64
            })
        {
            return Err(SessionError::InvalidManifest);
        }
    }
    if manifest
        .active_tab_id
        .is_some_and(|active_id| !ids.contains(&active_id))
    {
        return Err(SessionError::InvalidManifest);
    }
    Ok(())
}

fn draft_reference(document_id: u64, revision: u64, text: &[u8]) -> DraftReference {
    DraftReference {
        file_name: draft_file_name(document_id, revision),
        revision,
        byte_len: text.len() as u64,
        checksum: checksum(text),
    }
}

fn draft_file_name(document_id: u64, revision: u64) -> String {
    format!("draft-{document_id}-{revision}.txt")
}

fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn write_draft(path: &Path, bytes: &[u8]) -> Result<(), SessionError> {
    let parent = path.parent().ok_or(SessionError::InvalidManifest)?;
    let mut temporary = NamedTempFile::new_in(parent)
        .map_err(|source| io_error("draft temporary file creation", source))?;
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| io_error("draft write", source))?;
    temporary
        .persist(path)
        .map_err(|error| io_error("draft replace", error.error))?;
    sync_directory(parent)?;
    Ok(())
}

fn write_manifest(root: &Path, manifest: &SessionManifest) -> Result<(), SessionError> {
    let mut temporary = NamedTempFile::new_in(root)
        .map_err(|source| io_error("manifest temporary file creation", source))?;
    serde_json::to_writer(&mut temporary, manifest).map_err(SessionError::ManifestEncoding)?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|source| io_error("manifest flush", source))?;
    temporary
        .persist(root.join("session.json"))
        .map_err(|error| io_error("manifest replace", error.error))?;
    sync_directory(root)
}

fn cleanup_orphan_drafts(draft_dir: &Path, manifest: &SessionManifest) {
    let referenced = manifest
        .tabs
        .iter()
        .filter_map(|tab| {
            tab.draft_ref
                .as_ref()
                .map(|reference| reference.file_name.as_str())
        })
        .collect::<HashSet<_>>();
    let Ok(entries) = fs::read_dir(draft_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "txt")
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| !referenced.contains(name))
        {
            let _ = fs::remove_file(path);
        }
    }
}

fn set_private_directory(_path: &Path) -> Result<(), SessionError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o700))
            .map_err(|source| io_error("directory permissions", source))?;
    }
    Ok(())
}

fn sync_directory(_path: &Path) -> Result<(), SessionError> {
    #[cfg(unix)]
    File::open(_path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| io_error("directory sync", source))?;
    Ok(())
}

fn io_error(operation: &'static str, source: io::Error) -> SessionError {
    SessionError::Io { operation, source }
}

#[cfg(test)]
mod tests;
