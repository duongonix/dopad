mod actions;
mod documents;

use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    time::Duration,
};

use gpui_kit::{BackgroundExecutor, Context};

use super::{
    client::LspClient,
    installation,
    persistence::{self, ServerMetadata},
    provider::LspDocument,
    registry::{self, ServerSpec},
};
use crate::window::DopadWindow;

const STARTUP_DELAY: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ClientKey {
    server_id: String,
    root: PathBuf,
}

#[derive(Clone)]
struct ServerBinary {
    executable: PathBuf,
    version: String,
    source: String,
}

#[derive(Clone)]
pub(super) enum PromptPhase {
    Missing,
    Installing,
    Starting,
    Error(String),
    Manual,
}

#[derive(Clone)]
pub(super) struct InstallPrompt {
    pub spec: &'static ServerSpec,
    pub phase: PromptPhase,
    target_document: Option<u64>,
}

#[derive(Default)]
pub(in crate::window) struct LspManager {
    delay_scheduled: bool,
    delay_elapsed: bool,
    dismissed: HashSet<String>,
    checking: HashSet<String>,
    checked: HashSet<String>,
    starting_documents: HashSet<u64>,
    starting_clients: HashSet<ClientKey>,
    binaries: HashMap<String, ServerBinary>,
    clients: HashMap<ClientKey, LspClient>,
    documents: HashMap<u64, LspDocument>,
    prompts: VecDeque<InstallPrompt>,
    current_prompt: Option<InstallPrompt>,
    metadata: HashMap<String, ServerMetadata>,
    metadata_path: Option<PathBuf>,
    metadata_loaded: bool,
    install_cancel: Option<async_channel::Sender<()>>,
}

impl LspManager {
    pub(in crate::window) fn schedule_startup_delay(&mut self, cx: &mut Context<DopadWindow>) {
        if self.delay_scheduled {
            return;
        }
        self.delay_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(STARTUP_DELAY).await;
            let _ = this.update(cx, |window, cx| {
                window.lsp.delay_elapsed = true;
                window.ensure_active_lsp(cx);
            });
        })
        .detach();
    }

    pub(super) fn prompt(&self) -> Option<InstallPrompt> {
        self.current_prompt.clone()
    }

    pub(super) fn close_document(&mut self, document_id: u64) {
        self.starting_documents.remove(&document_id);
        if let Some(document) = self.documents.remove(&document_id) {
            document.did_close();
        }
    }

    fn next_prompt(&mut self) {
        self.current_prompt = self.prompts.pop_front();
    }

    fn enqueue_prompt(&mut self, prompt: InstallPrompt) {
        let id = prompt.spec.id;
        if self.dismissed.contains(id)
            || self
                .current_prompt
                .as_ref()
                .is_some_and(|current| current.spec.id == id)
            || self.prompts.iter().any(|current| current.spec.id == id)
        {
            return;
        }
        if self.current_prompt.is_none() {
            self.current_prompt = Some(prompt);
        } else {
            self.prompts.push_back(prompt);
        }
    }
}

impl Drop for LspManager {
    fn drop(&mut self) {
        if let Some(cancel) = self.install_cancel.take() {
            let _ = cancel.try_send(());
        }
        for document in self.documents.values() {
            document.did_close();
        }
        for client in self.clients.values() {
            client.close();
        }
    }
}

struct Detection {
    binary: Option<ServerBinary>,
    metadata: HashMap<String, ServerMetadata>,
}

async fn detect_server(
    spec: &'static ServerSpec,
    mut metadata: HashMap<String, ServerMetadata>,
    metadata_loaded: bool,
    metadata_path: Option<PathBuf>,
    executor: &BackgroundExecutor,
) -> anyhow::Result<Detection> {
    if !metadata_loaded && let Some(path) = metadata_path.as_deref() {
        metadata = persistence::load(path);
    }
    let cached_candidate = metadata
        .get(&spec_key(spec))
        .filter(|cached| cached.executable.is_file())
        .map(|cached| (cached.executable.clone(), cached.source.clone()));
    let app_data_executable = app_data_npm_executable(spec, metadata_path.as_deref());
    let mut candidates = Vec::with_capacity(3);
    candidates.extend(cached_candidate);
    candidates.extend(app_data_executable.map(|path| (path, "Dopad app data".to_owned())));
    candidates.extend(
        registry::find_executable(spec.executable).map(|path| (path, "system PATH".to_owned())),
    );
    let mut seen_paths = HashSet::new();
    let mut binary = None;
    for (executable, source) in candidates {
        if !seen_paths.insert(executable.clone()) {
            continue;
        }
        let version = installation::verify_version(&executable, spec.version_args, executor).await;
        if let Some(verified) = verified_server_binary(executable, source, version, spec.id) {
            binary = Some(verified);
            break;
        }
    }
    Ok(Detection { binary, metadata })
}

fn verified_server_binary(
    executable: PathBuf,
    source: String,
    version: anyhow::Result<String>,
    server_id: &str,
) -> Option<ServerBinary> {
    match version {
        Ok(version) => Some(ServerBinary {
            executable,
            version,
            source,
        }),
        Err(error) => {
            tracing::debug!(
                server = server_id,
                path = %executable.display(),
                error = %error,
                "language server candidate failed executable verification"
            );
            None
        }
    }
}

fn app_data_npm_executable(spec: &ServerSpec, metadata_path: Option<&Path>) -> Option<PathBuf> {
    if spec.method != registry::InstallMethod::Npm {
        return None;
    }
    let lsp_directory = metadata_path?.parent()?;
    let executable = if cfg!(windows) {
        format!("{}.cmd", spec.executable)
    } else {
        spec.executable.to_owned()
    };
    let path = lsp_directory
        .join("npm")
        .join("node_modules")
        .join(".bin")
        .join(executable);
    path.is_file().then_some(path)
}

fn persist_metadata(
    path: Option<PathBuf>,
    metadata: HashMap<String, ServerMetadata>,
    cx: &mut Context<DopadWindow>,
) {
    let Some(path) = path else { return };
    cx.background_executor()
        .spawn(async move {
            if let Err(error) = persistence::save(&path, &metadata) {
                tracing::warn!(error = %error, "LSP metadata could not be persisted");
            }
        })
        .detach();
}

fn workspace_root(path: &Path) -> PathBuf {
    const MARKERS: &[&str] = &[
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "go.mod",
        "compile_commands.json",
        "deno.json",
        "deno.jsonc",
        "pom.xml",
        "build.gradle",
        "Gemfile",
        "composer.json",
    ];
    let start = path.parent().unwrap_or(path);
    let mut fallback = start.to_path_buf();
    for directory in start.ancestors() {
        fallback = directory.to_path_buf();
        if MARKERS.iter().any(|marker| directory.join(marker).exists())
            || directory.join(".git").exists()
        {
            return directory.to_path_buf();
        }
    }
    fallback
}

fn language_id(language: dopad_syntax::Language, path: &Path, spec: &ServerSpec) -> String {
    if language == dopad_syntax::Language::PlainText
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("lua"))
    {
        return "lua".to_owned();
    }
    match language {
        dopad_syntax::Language::Rust => "rust",
        dopad_syntax::Language::JavaScript => "javascript",
        dopad_syntax::Language::TypeScript => "typescript",
        dopad_syntax::Language::Tsx => "typescriptreact",
        dopad_syntax::Language::Python => "python",
        dopad_syntax::Language::C => "c",
        dopad_syntax::Language::Cpp => "cpp",
        dopad_syntax::Language::Go => "go",
        dopad_syntax::Language::Html => "html",
        dopad_syntax::Language::Css | dopad_syntax::Language::Scss => "css",
        dopad_syntax::Language::Json => "json",
        _ => return spec.language.to_ascii_lowercase(),
    }
    .to_owned()
}

fn spec_key(spec: &ServerSpec) -> String {
    format!("{}:{}", spec.id, spec.executable)
}

#[cfg(test)]
mod lsp_detection_tests {
    use super::verified_server_binary;
    use std::path::PathBuf;

    #[test]
    fn does_not_accept_a_rustup_proxy_when_version_probe_fails() {
        let binary = verified_server_binary(
            PathBuf::from("rust-analyzer.exe"),
            "system PATH".to_owned(),
            Err(anyhow::anyhow!("unknown binary in active toolchain")),
            "rust-analyzer",
        );

        assert!(binary.is_none());
    }

    #[test]
    fn accepts_only_a_binary_with_a_successful_version_probe() {
        let binary = verified_server_binary(
            PathBuf::from("rust-analyzer.exe"),
            "system PATH".to_owned(),
            Ok("rust-analyzer 1.0".to_owned()),
            "rust-analyzer",
        )
        .expect("a successful version probe verifies the executable");

        assert_eq!(binary.version, "rust-analyzer 1.0");
        assert_eq!(binary.source, "system PATH");
    }
}
