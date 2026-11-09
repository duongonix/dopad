use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};

use anyhow::{Context as _, anyhow};
use sha2::{Digest, Sha256};
use tempfile::Builder;

use super::{InstallProgress, report_progress};
use archive::{extract_tar_gz, extract_zip, final_runtime_path};

#[path = "node_runtime/archive.rs"]
mod archive;

const NODE_VERSION: &str = "22.23.4";
const MIN_NODE_VERSION: &str = "22.22.2";
const MAX_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(8 * 60);
static RUNTIME_INSTALL_LOCK: OnceLock<(async_channel::Receiver<()>, async_channel::Sender<()>)> =
    OnceLock::new();

#[derive(Clone, Debug, Eq, PartialEq)]
struct NodeTarget {
    id: &'static str,
    archive: &'static str,
    sha256: &'static str,
}

#[derive(Clone, Debug)]
pub(super) struct NodeRuntime {
    pub node: PathBuf,
    pub npm_cli: PathBuf,
    pub version: String,
    pub managed: bool,
}

pub(super) async fn resolve(
    data_dir: PathBuf,
    cancel: async_channel::Receiver<()>,
    progress: async_channel::Sender<InstallProgress>,
    executor: &gpui_kit::BackgroundExecutor,
) -> anyhow::Result<NodeRuntime> {
    let (available, release) = RUNTIME_INSTALL_LOCK.get_or_init(|| {
        let (release, available) = async_channel::bounded(1);
        let _ = release.try_send(());
        (available, release)
    });
    available
        .recv()
        .await
        .map_err(|_| anyhow!("Node.js runtime install lock was closed"))?;
    let _lock = RuntimeInstallLock(release.clone());
    report_progress(&progress, "Checking for a compatible Node.js runtime", None);
    if let Some(runtime) = find_system_runtime(executor).await {
        tracing::info!(
            path = %runtime.node.display(),
            version = %runtime.version,
            "reusing compatible system Node.js runtime for LSP"
        );
        return Ok(runtime);
    }

    let target = current_target()?;
    let runtimes = data_dir.join("lsp").join("runtimes");
    fs::create_dir_all(&runtimes).context("could not create Dopad runtime directory")?;
    let final_dir = runtimes.join(format!("node-v{NODE_VERSION}-{}", target.id));
    let (node, npm_cli) = runtime_paths(&final_dir)?;
    if node.is_file() && npm_cli.is_file() {
        let version = verify_node(&node, executor).await?;
        verify_npm(&node, &npm_cli, executor).await?;
        return Ok(NodeRuntime {
            node,
            npm_cli,
            version,
            managed: true,
        });
    }
    anyhow::ensure!(
        !final_dir.exists(),
        "the managed Node.js runtime is incomplete; remove {} and retry",
        final_dir.display()
    );

    let progress_for_thread = progress.clone();
    let cancel_for_thread = cancel.clone();
    let runtimes_for_thread = runtimes.clone();
    let target_for_thread = target.clone();
    let (result_tx, result_rx) = async_channel::bounded(1);
    std::thread::Builder::new()
        .name("dopad-node-runtime-install".to_owned())
        .spawn(move || {
            let result = download_and_extract(
                &target_for_thread,
                &runtimes_for_thread,
                &cancel_for_thread,
                &progress_for_thread,
            );
            let _ = result_tx.send_blocking(result);
        })
        .context("could not start runtime download worker")?;
    let installed_dir = result_rx
        .recv()
        .await
        .map_err(|_| anyhow!("runtime download worker stopped unexpectedly"))??;
    anyhow::ensure!(installed_dir == final_dir, "runtime install path mismatch");
    let (node, npm_cli) = runtime_paths(&installed_dir)?;
    let version = verify_node(&node, executor).await?;
    anyhow::ensure!(
        npm_cli.is_file(),
        "downloaded Node.js runtime has no bundled npm"
    );
    verify_npm(&node, &npm_cli, executor).await?;
    Ok(NodeRuntime {
        node,
        npm_cli,
        version,
        managed: true,
    })
}

async fn find_system_runtime(executor: &gpui_kit::BackgroundExecutor) -> Option<NodeRuntime> {
    let node = super::super::registry::find_executable("node")?;
    let version = verify_node(&node, executor).await.ok()?;
    let npm_cli = find_npm_cli(&node)?;
    verify_npm(&node, &npm_cli, executor).await.ok()?;
    Some(NodeRuntime {
        node,
        npm_cli,
        version,
        managed: false,
    })
}

struct RuntimeInstallLock(async_channel::Sender<()>);

impl Drop for RuntimeInstallLock {
    fn drop(&mut self) {
        let _ = self.0.try_send(());
    }
}

fn find_npm_cli(node: &Path) -> Option<PathBuf> {
    let parent = node.parent()?;
    let candidates = [
        parent.join("node_modules/npm/bin/npm-cli.js"),
        parent.join("lib/node_modules/npm/bin/npm-cli.js"),
        parent.join("../lib/node_modules/npm/bin/npm-cli.js"),
    ];
    candidates.into_iter().find(|path| path.is_file())
}

async fn verify_node(
    node: &Path,
    executor: &gpui_kit::BackgroundExecutor,
) -> anyhow::Result<String> {
    let version = super::verify_version(node, &[OsString::from("--version")], executor)
        .await
        .with_context(|| format!("could not query Node.js at {}", node.display()))?;
    ensure_compatible_version(&version)?;
    Ok(version)
}

async fn verify_npm(
    node: &Path,
    npm_cli: &Path,
    executor: &gpui_kit::BackgroundExecutor,
) -> anyhow::Result<String> {
    let args = [npm_cli.as_os_str().to_owned(), "--version".into()];
    let version = super::verify_version(node, &args, executor)
        .await
        .context("could not query the bundled npm CLI")?;
    semver::Version::parse(&version).context("npm returned an invalid version")?;
    Ok(version)
}

fn ensure_compatible_version(version: &str) -> anyhow::Result<()> {
    let parsed = parse_node_version(version)?;
    let minimum = semver::Version::parse(MIN_NODE_VERSION)?;
    anyhow::ensure!(
        parsed >= minimum,
        "Node.js {MIN_NODE_VERSION} or newer is required"
    );
    Ok(())
}

fn parse_node_version(version: &str) -> anyhow::Result<semver::Version> {
    let version = version.trim().trim_start_matches('v');
    semver::Version::parse(version).context("Node.js returned an invalid version")
}

fn current_target() -> anyhow::Result<NodeTarget> {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => NodeTarget {
            id: "win-x64",
            archive: "zip",
            sha256: "cbc74b5e0982c9dbb41525fd8fb5dcd32fbee237863f7829072f4639a1ce5f20",
        },
        ("windows", "aarch64") => NodeTarget {
            id: "win-arm64",
            archive: "zip",
            sha256: "a8b8cd79cbbb3062435f449591d98dd63730bbbf30ce2d7cc3a6376c236cb111",
        },
        ("macos", "x86_64") => NodeTarget {
            id: "darwin-x64",
            archive: "tar.gz",
            sha256: "efcc2b0db6f94bf53eddb038fe2b370277a042bd29e4f19b8f8e23598795b819",
        },
        ("macos", "aarch64") => NodeTarget {
            id: "darwin-arm64",
            archive: "tar.gz",
            sha256: "c9e1dbc748b33ed2d14f42445191ecaf14c56259c7a9a26d0042c49ec2b66428",
        },
        ("linux", "x86_64") => NodeTarget {
            id: "linux-x64",
            archive: "tar.gz",
            sha256: "b8492284c76a54baf58d61668dd0e41ee0fc98adabf9503d30f05d08f55b4406",
        },
        ("linux", "aarch64") => NodeTarget {
            id: "linux-arm64",
            archive: "tar.gz",
            sha256: "f7a1e486dbc8b286847815b477d4fb5e9fb963ed4489c3e1588853d685f589f9",
        },
        _ => anyhow::bail!(
            "managed Node.js installation is not supported on {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    };
    Ok(target)
}

fn runtime_paths(directory: &Path) -> anyhow::Result<(PathBuf, PathBuf)> {
    #[cfg(windows)]
    let paths = (
        directory.join("node.exe"),
        directory.join("node_modules/npm/bin/npm-cli.js"),
    );
    #[cfg(not(windows))]
    let paths = (
        directory.join("bin/node"),
        directory.join("lib/node_modules/npm/bin/npm-cli.js"),
    );
    Ok(paths)
}

fn download_and_extract(
    target: &NodeTarget,
    runtimes: &Path,
    cancel: &async_channel::Receiver<()>,
    progress: &async_channel::Sender<InstallProgress>,
) -> anyhow::Result<PathBuf> {
    report_progress(progress, "Downloading managed Node.js", None);
    let file_name = format!("node-v{NODE_VERSION}-{}.{}", target.id, target.archive);
    let url = format!("https://nodejs.org/download/release/v{NODE_VERSION}/{file_name}");
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(DOWNLOAD_TIMEOUT))
            .user_agent(concat!("Dopad/", env!("CARGO_PKG_VERSION")))
            .build(),
    );
    let mut response = agent
        .get(&url)
        .call()
        .map_err(|error| anyhow!("could not download managed Node.js: {error}"))?;
    let content_length = response.body().content_length();
    if let Some(length) = content_length {
        anyhow::ensure!(
            length > 0 && length <= MAX_ARCHIVE_BYTES,
            "Node.js archive size is invalid"
        );
    }
    let mut temporary = Builder::new()
        .prefix("node-runtime-")
        .suffix(if target.archive == "zip" {
            ".zip"
        } else {
            ".tar.gz"
        })
        .tempfile_in(runtimes)?;
    let mut reader = response.body_mut().as_reader();
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut received = 0u64;
    let mut reported = Instant::now();
    loop {
        anyhow::ensure!(cancel.try_recv().is_err(), "Node.js download was cancelled");
        let count = reader
            .read(&mut buffer)
            .context("Node.js download failed")?;
        if count == 0 {
            break;
        }
        received = received
            .checked_add(count as u64)
            .ok_or_else(|| anyhow!("Node.js download size overflow"))?;
        anyhow::ensure!(
            received <= MAX_ARCHIVE_BYTES,
            "Node.js archive exceeds the size limit"
        );
        temporary.write_all(&buffer[..count])?;
        digest.update(&buffer[..count]);
        if reported.elapsed() >= Duration::from_millis(150) {
            report_progress(
                progress,
                "Downloading managed Node.js",
                content_length.map(|total| received as f32 / total as f32),
            );
            reported = Instant::now();
        }
    }
    anyhow::ensure!(received > 0, "Node.js download was empty");
    if let Some(length) = content_length {
        anyhow::ensure!(received == length, "Node.js download was incomplete");
    }
    temporary.as_file().sync_all()?;
    let actual = format!("{:x}", digest.finalize());
    anyhow::ensure!(
        actual == target.sha256,
        "Node.js download checksum verification failed"
    );
    report_progress(progress, "Verified Node.js download", Some(1.0));

    let stage =
        tempfile::tempdir_in(runtimes).context("could not create runtime staging directory")?;
    report_progress(progress, "Extracting managed Node.js", None);
    let archive_path = temporary.path().to_path_buf();
    if target.archive == "zip" {
        extract_zip(&archive_path, stage.path())?;
    } else {
        extract_tar_gz(&archive_path, stage.path())?;
    }
    anyhow::ensure!(
        cancel.try_recv().is_err(),
        "Node.js installation was cancelled"
    );
    let top = stage
        .path()
        .join(format!("node-v{NODE_VERSION}-{}", target.id));
    anyhow::ensure!(
        top.is_dir(),
        "Node.js archive has an unexpected directory layout"
    );
    let (node, npm_cli) = runtime_paths(&top)?;
    anyhow::ensure!(
        node.is_file() && npm_cli.is_file(),
        "Node.js archive is missing runtime files"
    );
    if !final_runtime_path(runtimes, target).exists() {
        fs::rename(&top, final_runtime_path(runtimes, target))
            .context("could not commit managed Node.js installation")?;
    }
    drop(stage);
    let final_dir = final_runtime_path(runtimes, target);
    let (node, npm_cli) = runtime_paths(&final_dir)?;
    anyhow::ensure!(
        node.is_file() && npm_cli.is_file(),
        "managed Node.js runtime verification failed"
    );
    report_progress(progress, "Managed Node.js is ready", Some(1.0));
    Ok(final_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_runtime_target_has_trusted_digest_for_supported_architectures() {
        for target in [
            NodeTarget {
                id: "win-x64",
                archive: "zip",
                sha256: "cbc74b5e0982c9dbb41525fd8fb5dcd32fbee237863f7829072f4639a1ce5f20",
            },
            NodeTarget {
                id: "win-arm64",
                archive: "zip",
                sha256: "a8b8cd79cbbb3062435f449591d98dd63730bbbf30ce2d7cc3a6376c236cb111",
            },
            NodeTarget {
                id: "darwin-x64",
                archive: "tar.gz",
                sha256: "efcc2b0db6f94bf53eddb038fe2b370277a042bd29e4f19b8f8e23598795b819",
            },
            NodeTarget {
                id: "darwin-arm64",
                archive: "tar.gz",
                sha256: "c9e1dbc748b33ed2d14f42445191ecaf14c56259c7a9a26d0042c49ec2b66428",
            },
            NodeTarget {
                id: "linux-x64",
                archive: "tar.gz",
                sha256: "b8492284c76a54baf58d61668dd0e41ee0fc98adabf9503d30f05d08f55b4406",
            },
            NodeTarget {
                id: "linux-arm64",
                archive: "tar.gz",
                sha256: "f7a1e486dbc8b286847815b477d4fb5e9fb963ed4489c3e1588853d685f589f9",
            },
        ] {
            assert_eq!(target.sha256.len(), 64);
            assert!(target.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn rejects_absolute_and_parent_archive_paths() {
        assert!(archive::validate_relative_path(Path::new("../outside")).is_err());
        assert!(archive::validate_relative_path(Path::new("/outside")).is_err());
        assert!(archive::validate_relative_path(Path::new("node/bin/node")).is_ok());
    }

    #[test]
    fn runtime_version_is_checked_against_language_server_minimum() {
        assert!(ensure_compatible_version("v22.22.2").is_ok());
        assert!(ensure_compatible_version("v22.22.1").is_err());
    }
}
