use std::{
    collections::HashMap,
    env,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use dopad_core::{
    DraftCheckpoint, FileError, LoadedFile, RecoveredSession, SessionError, SessionManifest,
    SessionStore,
};

#[derive(Clone)]
pub(super) struct SessionCoordinator {
    root: Option<PathBuf>,
    store: Arc<Mutex<Option<Arc<SessionStore>>>>,
    clean_shutdown_on_exit: Arc<AtomicBool>,
}

pub(super) struct SessionBootstrap {
    pub recovered: Option<RecoveredSession>,
    pub disk_files: HashMap<u64, Result<LoadedFile, FileError>>,
    pub next_generation: u64,
}

impl SessionCoordinator {
    pub fn new() -> Self {
        Self {
            root: session_directory(),
            store: Arc::new(Mutex::new(None)),
            clean_shutdown_on_exit: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn ephemeral() -> Self {
        Self {
            root: None,
            store: Arc::new(Mutex::new(None)),
            clean_shutdown_on_exit: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn initialize(&self) -> Result<Option<SessionBootstrap>, SessionError> {
        let Some(root) = &self.root else {
            return Ok(None);
        };
        let (store, recovered) = SessionStore::open(root)?;
        let store = Arc::new(store);
        let next_generation = store.next_generation()?;
        let mut disk_files = HashMap::new();
        if let Some(recovered) = &recovered {
            for tab in recovered
                .manifest
                .tabs
                .iter()
                .filter(|tab| tab.draft_ref.is_none())
            {
                let Some(path) = &tab.path else { continue };
                disk_files.insert(tab.id, dopad_core::read_file(path));
            }
        }
        let mut current = self.store.lock().map_err(|_| SessionError::StatePoisoned)?;
        *current = Some(store);
        Ok(Some(SessionBootstrap {
            recovered,
            disk_files,
            next_generation,
        }))
    }

    pub fn checkpoint(
        &self,
        manifest: SessionManifest,
        drafts: Vec<DraftCheckpoint>,
    ) -> Result<bool, SessionError> {
        let store = self.store.lock().map_err(|_| SessionError::StatePoisoned)?;
        let Some(store) = store.as_ref() else {
            return Ok(false);
        };
        store.checkpoint(manifest, drafts)
    }

    pub fn discard(&self) -> Result<(), SessionError> {
        let store = self.store.lock().map_err(|_| SessionError::StatePoisoned)?;
        if let Some(store) = store.as_ref() {
            store.discard()?;
        }
        Ok(())
    }

    pub fn mark_clean_shutdown(&self) -> Result<(), SessionError> {
        if !self.clean_shutdown_on_exit.load(Ordering::Acquire) {
            return Ok(());
        }
        let store = self.store.lock().map_err(|_| SessionError::StatePoisoned)?;
        if let Some(store) = store.as_ref() {
            store.mark_clean_shutdown()?;
        }
        Ok(())
    }

    pub fn preserve_recovery_on_exit(&self) {
        self.clean_shutdown_on_exit.store(false, Ordering::Release);
    }

    pub fn can_persist_recovery(&self) -> bool {
        self.root.is_some()
    }
}

fn session_directory() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        env::var_os("LOCALAPPDATA")
            .or_else(|| env::var_os("APPDATA"))
            .map(|root| PathBuf::from(root).join("Dopad").join("Recovery"))
    }

    #[cfg(target_os = "macos")]
    {
        env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("Dopad")
                .join("Recovery")
        })
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(root) = env::var_os("XDG_STATE_HOME").map(PathBuf::from)
            && root.is_absolute()
        {
            return Some(root.join("dopad"));
        }
        env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join(".local")
                .join("state")
                .join("dopad")
        })
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let _ = env::var_os("HOME");
        None
    }
}
