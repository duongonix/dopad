use std::{
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context as _, anyhow};
use gpui_kit::BackgroundExecutor;

use super::{client::command_for, client::command_for_os, registry};

#[path = "installation/node_runtime.rs"]
mod node_runtime;
mod process;
#[cfg(test)]
#[path = "installation/tests.rs"]
mod tests;

use process::{ProcessOutput, run_child};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const VERIFY_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone, Debug)]
pub(super) struct InstallProgress {
    pub message: String,
    pub fraction: Option<f32>,
}

#[derive(Clone, Debug)]
pub(super) struct InstalledServer {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub version: String,
    pub runtime_version: Option<String>,
    pub source: String,
}

pub(super) fn report_progress(
    sender: &async_channel::Sender<InstallProgress>,
    message: &str,
    fraction: Option<f32>,
) {
    let _ = sender.try_send(InstallProgress {
        message: message.chars().take(160).collect(),
        fraction: fraction.map(|value| value.clamp(0.0, 1.0)),
    });
}

pub(super) async fn install(
    spec: &'static registry::ServerSpec,
    data_dir: PathBuf,
    cancel: async_channel::Receiver<()>,
    progress: async_channel::Sender<InstallProgress>,
    executor: &BackgroundExecutor,
) -> anyhow::Result<InstalledServer> {
    if spec.method == registry::InstallMethod::Npm {
        return install_npm(spec, data_dir, cancel, progress, executor).await;
    }
    let plan = registry::install_command(spec, &data_dir)
        .ok_or_else(|| anyhow!("{} does not have a safe automatic installer", spec.name))?;
    report_progress(&progress, "Installing language server", None);
    let mut command = command_for_os(&plan.program, &plan.args)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command.envs(plan.env.iter().map(|(key, value)| (key, value)));
    let mut child = command
        .spawn()
        .with_context(|| format!("could not start installer {}", plan.program.display()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("installer stdout is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("installer stderr is unavailable"))?;
    let result = run_child(
        &mut child,
        stdout,
        stderr,
        executor,
        INSTALL_TIMEOUT,
        Some(&cancel),
    )
    .await;
    let output = result?;
    if !output.success {
        tracing::debug!(
            server = spec.id,
            output = %output.summary(),
            "language server installer exited unsuccessfully"
        );
        anyhow::bail!(
            "{}",
            install_failure_message(spec.method, spec.name, &output)
        );
    }

    let mut executable = plan.expected_executable.filter(|path| path.is_file());
    if executable.is_none() {
        executable = match spec.method {
            registry::InstallMethod::Rustup => {
                query_installed_path(&plan.program, &["which", spec.executable], executor).await
            }
            registry::InstallMethod::PythonPip => {
                query_python_scripts(&plan.program, executor).await
            }
            _ => None,
        };
    }
    let executable = executable
        .or_else(|| registry::find_executable(spec.executable))
        .ok_or_else(|| {
            anyhow!(
                "installer finished, but {} is not available on PATH",
                spec.executable
            )
        })?;
    let launch_args = spec.args.iter().map(OsString::from).collect::<Vec<_>>();
    let version_args = spec
        .version_args
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    report_progress(&progress, "Verifying installed language server", None);
    let version = verify_version(&executable, &version_args, executor)
        .await
        .map_err(|error| {
            tracing::debug!(
                server = spec.id,
                path = %executable.display(),
                error = %error,
                "installed language server failed executable verification"
            );
            anyhow!("{} was installed but could not be verified", spec.name)
        })?;
    Ok(InstalledServer {
        executable,
        args: launch_args,
        version,
        runtime_version: None,
        source: format!("installed via {:?}", spec.method),
    })
}

async fn install_npm(
    spec: &'static registry::ServerSpec,
    data_dir: PathBuf,
    cancel: async_channel::Receiver<()>,
    progress: async_channel::Sender<InstallProgress>,
    executor: &BackgroundExecutor,
) -> anyhow::Result<InstalledServer> {
    let runtime =
        node_runtime::resolve(data_dir.clone(), cancel.clone(), progress.clone(), executor).await?;
    let prefix = data_dir.join("lsp").join("npm");
    let cache = data_dir.join("lsp").join("npm-cache");
    fs::create_dir_all(&prefix).context("could not create Dopad language server directory")?;
    fs::create_dir_all(&cache).context("could not create Dopad npm cache directory")?;
    report_progress(&progress, "Installing language server packages", None);
    let mut arguments = vec![
        runtime.npm_cli.clone().into_os_string(),
        "install".into(),
        "--prefix".into(),
        prefix.clone().into_os_string(),
        "--no-audit".into(),
        "--no-fund".into(),
        "--ignore-scripts".into(),
        "--progress=false".into(),
        "--cache".into(),
        cache.into_os_string(),
    ];
    for package in spec.package.split_whitespace() {
        anyhow::ensure!(
            !package.starts_with('-') && package.contains('@'),
            "language server package is not pinned to an exact version"
        );
        arguments.push(package.into());
    }
    let mut command = command_for_os(&runtime.node, &arguments)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().with_context(|| {
        format!(
            "could not start managed npm using {}",
            runtime.node.display()
        )
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("npm installer stdout is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("npm installer stderr is unavailable"))?;
    let output = run_child(
        &mut child,
        stdout,
        stderr,
        executor,
        INSTALL_TIMEOUT,
        Some(&cancel),
    )
    .await?;
    if !output.success {
        tracing::warn!(
            server = spec.id,
            output = %output.summary(),
            "managed npm package installation failed"
        );
        anyhow::bail!("{}", npm_failure_message(spec.name, &output));
    }
    report_progress(&progress, "Verifying installed language server", None);
    let entrypoint = resolve_npm_entrypoint(&prefix, spec)?;
    let mut launch_args = vec![entrypoint.clone().into_os_string()];
    launch_args.extend(spec.args.iter().map(OsString::from));
    let mut version_args = vec![entrypoint.into_os_string()];
    version_args.extend(spec.version_args.iter().map(OsString::from));
    let version = verify_version(&runtime.node, &version_args, executor)
        .await
        .with_context(|| format!("{} was installed but could not be verified", spec.name))?;
    tracing::info!(
        server = spec.id,
        runtime_version = %runtime.version,
        managed_runtime = runtime.managed,
        "managed language server installation verified"
    );
    Ok(InstalledServer {
        executable: runtime.node,
        args: launch_args,
        version,
        runtime_version: Some(runtime.version),
        source: if runtime.managed {
            "Dopad managed Node.js".to_owned()
        } else {
            "system Node.js runtime".to_owned()
        },
    })
}

fn resolve_npm_entrypoint(prefix: &Path, spec: &registry::ServerSpec) -> anyhow::Result<PathBuf> {
    let node_modules = prefix.join("node_modules");
    for package_spec in spec.package.split_whitespace() {
        let (package, _) = package_spec
            .rsplit_once('@')
            .ok_or_else(|| anyhow!("language server package version is not pinned"))?;
        let package_root = node_modules.join(package);
        let manifest_path = package_root.join("package.json");
        let Ok(bytes) = fs::read(&manifest_path) else {
            continue;
        };
        let manifest: serde_json::Value =
            serde_json::from_slice(&bytes).context("installed package manifest is invalid")?;
        let Some(bin) = manifest.get("bin") else {
            continue;
        };
        let relative = if let Some(path) = bin.as_str() {
            (package.rsplit('/').next() == Some(spec.executable)).then_some(path)
        } else {
            bin.get(spec.executable).and_then(serde_json::Value::as_str)
        };
        let Some(relative) = relative else {
            continue;
        };
        let relative = Path::new(relative);
        anyhow::ensure!(
            relative
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
            "installed package executable path is unsafe"
        );
        let candidate = package_root.join(relative);
        let canonical_root = package_root.canonicalize()?;
        let canonical_entry = candidate.canonicalize()?;
        anyhow::ensure!(
            canonical_entry.starts_with(&canonical_root) && canonical_entry.is_file(),
            "installed package executable escapes its package directory"
        );
        return Ok(canonical_entry);
    }
    anyhow::bail!("installed language server package was not found")
}

async fn query_installed_path(
    program: &std::path::Path,
    args: &[&str],
    executor: &BackgroundExecutor,
) -> Option<PathBuf> {
    run_short_command(program, args, executor)
        .await
        .ok()
        .filter(|output| output.success)
        .map(|output| PathBuf::from(output.stdout.trim()))
        .filter(|path| path.is_file())
}

async fn query_python_scripts(
    program: &std::path::Path,
    executor: &BackgroundExecutor,
) -> Option<PathBuf> {
    const SCRIPT_DIR_QUERY: &str = "import os,sysconfig; print(sysconfig.get_path('scripts', 'nt_user' if os.name == 'nt' else 'posix_user'))";
    let output = run_short_command(program, &["-c", SCRIPT_DIR_QUERY], executor)
        .await
        .ok()?
        .stdout;
    let binary = if cfg!(windows) {
        "basedpyright-langserver.exe"
    } else {
        "basedpyright-langserver"
    };
    let path = PathBuf::from(output.trim()).join(binary);
    path.is_file().then_some(path)
}

async fn run_short_command(
    program: &std::path::Path,
    args: &[&str],
    executor: &BackgroundExecutor,
) -> anyhow::Result<ProcessOutput> {
    let mut command = command_for(program, args)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .context("could not query installed server path")?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("query stdout is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("query stderr is unavailable"))?;
    run_child(&mut child, stdout, stderr, executor, VERIFY_TIMEOUT, None).await
}

pub(super) async fn verify_version(
    executable: &std::path::Path,
    args: &[OsString],
    executor: &BackgroundExecutor,
) -> anyhow::Result<String> {
    let mut command = command_for_os(executable, args)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .with_context(|| format!("could not verify {}", executable.display()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("version stdout is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("version stderr is unavailable"))?;
    let output = run_child(&mut child, stdout, stderr, executor, VERIFY_TIMEOUT, None).await?;
    anyhow::ensure!(
        output.success,
        "server version check failed: {}",
        output.summary()
    );
    let version = output
        .stdout
        .lines()
        .chain(output.stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("unknown")
        .chars()
        .take(120)
        .collect::<String>();
    Ok(version)
}

fn install_failure_message(
    method: registry::InstallMethod,
    server_name: &str,
    output: &ProcessOutput,
) -> String {
    let output_text = format!("{}\n{}", output.stderr, output.stdout).to_ascii_lowercase();
    if method == registry::InstallMethod::Npm {
        let registry_unreachable = [
            "fetcherror",
            "econnrefused",
            "econnreset",
            "etimedout",
            "enotfound",
            "eai_again",
        ]
        .iter()
        .any(|marker| output_text.contains(marker))
            || (output_text.contains("registry.npmjs.org") && output_text.contains("eacces"));
        if registry_unreachable {
            return format!(
                "Could not reach the npm registry while installing {server_name}. Check the internet connection or proxy, then retry."
            );
        }
        if ["eacces", "eperm", "permission denied"]
            .iter()
            .any(|marker| output_text.contains(marker))
        {
            return format!(
                "npm does not have permission to write files while installing {server_name}. Check the Dopad app data folder permissions, then retry."
            );
        }
        return format!(
            "Dopad could not install {server_name} from the package registry. Check the network and Dopad data folder permissions, then retry."
        );
    }

    format!("The installer for {server_name} failed. Verify the required toolchain, then retry.")
}

fn npm_failure_message(server_name: &str, output: &ProcessOutput) -> String {
    let output_text = format!("{}\n{}", output.stderr, output.stdout).to_ascii_lowercase();
    if [
        "fetcherror",
        "econnrefused",
        "econnreset",
        "etimedout",
        "enotfound",
        "eai_again",
    ]
    .iter()
    .any(|marker| output_text.contains(marker))
    {
        format!(
            "Dopad could not reach the package registry while installing {server_name}. Check the internet connection or proxy, then retry."
        )
    } else if ["eacces", "eperm", "permission denied"]
        .iter()
        .any(|marker| output_text.contains(marker))
    {
        format!(
            "Dopad cannot write the {server_name} installation in its app data directory. Check folder permissions, then retry."
        )
    } else {
        format!(
            "Dopad could not install {server_name}. Retry the installation; detailed output is available in the LSP logs."
        )
    }
}
