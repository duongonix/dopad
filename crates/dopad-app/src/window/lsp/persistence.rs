use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ServerMetadata {
    pub server_id: String,
    pub version: String,
    pub executable: PathBuf,
    #[serde(default)]
    pub launch_args: Vec<String>,
    #[serde(default)]
    pub runtime_version: Option<String>,
    #[serde(default)]
    pub installation_verified: bool,
    #[serde(default)]
    pub lsp_initialized: bool,
    pub source: String,
    pub verified_at_unix_seconds: u64,
}

#[derive(Default, Deserialize, Serialize)]
struct MetadataFile {
    version: u32,
    servers: HashMap<String, ServerMetadata>,
}

static SAVE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(super) fn load(path: &Path) -> HashMap<String, ServerMetadata> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<MetadataFile>(&bytes).ok())
        .filter(|file| file.version == 1)
        .map_or_else(HashMap::new, |file| file.servers)
}

pub(super) fn save(path: &Path, servers: &HashMap<String, ServerMetadata>) -> anyhow::Result<()> {
    let _guard = SAVE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| anyhow::anyhow!("LSP metadata save lock poisoned"))?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("LSP metadata path has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut merged = load(path);
    for (key, server) in servers {
        let is_newer = merged.get(key).is_none_or(|current| {
            server.verified_at_unix_seconds >= current.verified_at_unix_seconds
        });
        if is_newer {
            merged.insert(key.clone(), server.clone());
        }
    }
    let bytes = serde_json::to_vec_pretty(&MetadataFile {
        version: 1,
        servers: merged,
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}

pub(super) fn verified_now(
    server_id: &str,
    version: String,
    executable: PathBuf,
    launch_args: &[OsString],
    runtime_version: Option<String>,
    source: &str,
) -> ServerMetadata {
    ServerMetadata {
        server_id: server_id.to_owned(),
        version,
        executable,
        launch_args: launch_args
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect(),
        runtime_version,
        installation_verified: true,
        lsp_initialized: false,
        source: source.to_owned(),
        verified_at_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_round_trips_and_rejects_unknown_schema_versions() {
        let dir = tempfile::tempdir().expect("temp directory");
        let path = dir.path().join("servers.json");
        let mut servers = HashMap::new();
        servers.insert(
            "rust-analyzer".into(),
            verified_now(
                "rust-analyzer",
                "1.0".into(),
                PathBuf::from("ra"),
                &[],
                None,
                "system",
            ),
        );
        save(&path, &servers).expect("write metadata");
        assert_eq!(load(&path).get("rust-analyzer").unwrap().version, "1.0");
        fs::write(&path, br#"{"version":99,"servers":{}}"#).expect("replace metadata");
        assert!(load(&path).is_empty());
    }

    #[test]
    fn independent_server_writes_merge_instead_of_overwriting_each_other() {
        let dir = tempfile::tempdir().expect("temp directory");
        let path = dir.path().join("servers.json");
        let first = HashMap::from([(
            "rust-analyzer".into(),
            verified_now(
                "rust-analyzer",
                "1".into(),
                PathBuf::from("ra"),
                &[],
                None,
                "system",
            ),
        )]);
        let second = HashMap::from([(
            "gopls".into(),
            verified_now(
                "gopls",
                "1".into(),
                PathBuf::from("gopls"),
                &[],
                None,
                "system",
            ),
        )]);

        save(&path, &first).expect("write first server");
        save(&path, &second).expect("merge second server");

        let loaded = load(&path);
        assert!(loaded.contains_key("rust-analyzer"));
        assert!(loaded.contains_key("gopls"));
    }
}
