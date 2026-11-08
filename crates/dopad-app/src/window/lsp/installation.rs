use std::{path::PathBuf, process::Stdio, time::Duration};

use anyhow::{Context as _, anyhow};
use async_process::{Child, ChildStderr, ChildStdout};
use futures_lite::{
    future::{race, zip},
    io::{AsyncRead, AsyncReadExt},
};
use gpui_kit::BackgroundExecutor;

use super::{client::command_for, registry};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const VERIFY_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CAPTURE_BYTES: usize = 4096;

pub(super) async fn install(
    spec: &'static registry::ServerSpec,
    data_dir: PathBuf,
    cancel: async_channel::Receiver<()>,
    executor: &BackgroundExecutor,
) -> anyhow::Result<(PathBuf, String)> {
    let plan = registry::install_command(spec, &data_dir)
        .ok_or_else(|| anyhow!("{} does not have a safe automatic installer", spec.name))?;
    let string_args = plan
        .args
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let argument_refs = string_args.iter().map(String::as_str).collect::<Vec<_>>();
    let mut command = command_for(&plan.program, &argument_refs)?;
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
    let version = verify_version(&executable, spec.version_args, executor)
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
    Ok((executable, version))
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
    args: &[&str],
    executor: &BackgroundExecutor,
) -> anyhow::Result<String> {
    let mut command = command_for(executable, args)?;
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

struct ProcessOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

impl ProcessOutput {
    fn summary(&self) -> String {
        let output = [self.stderr.trim(), self.stdout.trim()]
            .into_iter()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("; ");
        if output.is_empty() {
            "no output".to_owned()
        } else {
            output
        }
    }
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
            "npm could not install {server_name}. Check that Node.js and npm are working, then retry."
        );
    }

    format!("The installer for {server_name} failed. Verify the required toolchain, then retry.")
}

async fn run_child(
    child: &mut Child,
    stdout: ChildStdout,
    stderr: ChildStderr,
    executor: &BackgroundExecutor,
    timeout: Duration,
    cancel: Option<&async_channel::Receiver<()>>,
) -> anyhow::Result<ProcessOutput> {
    let read = zip(read_stream(stdout), read_stream(stderr));
    let process = race(
        async { ProcessRace::Complete(zip(child.status(), read).await) },
        async {
            executor.timer(timeout).await;
            ProcessRace::TimedOut
        },
    );
    let outcome = if let Some(cancel) = cancel {
        race(process, async {
            let _ = cancel.recv().await;
            ProcessRace::Cancelled
        })
        .await
    } else {
        process.await
    };
    match outcome {
        ProcessRace::Complete((status, (stdout, stderr))) => {
            let status = status.context("could not wait for child process")?;
            Ok(ProcessOutput {
                success: status.success(),
                stdout,
                stderr,
            })
        }
        ProcessRace::TimedOut => {
            anyhow::bail!("process timed out after {} seconds", timeout.as_secs())
        }
        ProcessRace::Cancelled => anyhow::bail!("installation was cancelled"),
    }
}

enum ProcessRace<T> {
    Complete(T),
    TimedOut,
    Cancelled,
}

async fn read_stream<R: AsyncRead + Unpin>(mut stream: R) -> String {
    let mut captured = Vec::new();
    let mut buffer = [0; 512];
    while let Ok(length) = stream.read(&mut buffer).await {
        if length == 0 {
            break;
        }
        if captured.len() < MAX_CAPTURE_BYTES {
            let take = (MAX_CAPTURE_BYTES - captured.len()).min(length);
            captured.extend_from_slice(&buffer[..take]);
        }
    }
    String::from_utf8_lossy(&captured).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{ProcessOutput, install_failure_message};
    use crate::window::lsp::registry::InstallMethod;

    #[test]
    fn npm_registry_failures_show_actionable_message_without_raw_logs() {
        let output = ProcessOutput {
            success: false,
            stdout: String::new(),
            stderr: "FetchError request to https://registry.npmjs.org failed: EACCES".to_owned(),
        };

        let message =
            install_failure_message(InstallMethod::Npm, "TypeScript Language Server", &output);

        assert!(message.contains("internet connection or proxy"));
        assert!(!message.contains("FetchError"));
        assert!(!message.contains("registry.npmjs.org"));
    }

    #[test]
    fn npm_permission_failures_do_not_show_installer_output() {
        let output = ProcessOutput {
            success: false,
            stdout: String::new(),
            stderr: "npm ERR! EACCES: permission denied".to_owned(),
        };

        let message =
            install_failure_message(InstallMethod::Npm, "TypeScript Language Server", &output);

        assert!(message.contains("permission to write"));
        assert!(!message.contains("npm ERR!"));
    }
}
