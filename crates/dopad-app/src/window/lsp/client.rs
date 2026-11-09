use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context as _, anyhow};
use async_process::{ChildStderr, ChildStdin, ChildStdout, Command};
use futures_lite::{
    future::race,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
};
use gpui_kit::BackgroundExecutor;
use serde_json::{Value, json};

use crate::window::diagnostics::{DiagnosticBus, DiagnosticUpdate};

const MAX_HEADER_BYTES: usize = 8 * 1024;
const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

type PendingRequests = Arc<Mutex<HashMap<i64, async_channel::Sender<Value>>>>;

#[derive(Clone)]
pub(super) struct LspClient {
    outgoing: async_channel::Sender<Value>,
    pending: PendingRequests,
    next_id: Arc<AtomicI64>,
}

impl LspClient {
    pub(super) async fn start(
        executable: PathBuf,
        args: &[&str],
        root: &Path,
        executor: &BackgroundExecutor,
        diagnostics: DiagnosticBus,
        server_id: String,
    ) -> anyhow::Result<Self> {
        let root_uri = url::Url::from_file_path(root)
            .map_err(|_| anyhow!("workspace path cannot be represented as a file URI"))?
            .to_string();
        let mut command = command_for(&executable, args)?;
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().with_context(|| {
            format!(
                "could not start language server at {}",
                executable.display()
            )
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("LSP stdin is unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("LSP stdout is unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("LSP stderr is unavailable"))?;

        let (outgoing, outgoing_rx) = async_channel::bounded(128);
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let client = Self {
            outgoing: outgoing.clone(),
            pending: pending.clone(),
            next_id: Arc::new(AtomicI64::new(1)),
        };
        executor.spawn(writer_loop(stdin, outgoing_rx)).detach();
        executor
            .spawn(reader_loop(
                stdout,
                outgoing,
                pending,
                root_uri.clone(),
                diagnostics,
                server_id,
                root.to_path_buf(),
            ))
            .detach();
        executor.spawn(stderr_loop(stderr)).detach();
        executor
            .spawn(async move {
                match child.status().await {
                    Ok(status) => tracing::debug!(?status, "language server process exited"),
                    Err(error) => tracing::debug!(error = %error, "language server wait failed"),
                }
            })
            .detach();

        let folder_name = root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace");
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "Dopad", "version": env!("CARGO_PKG_VERSION") },
            "locale": "en-US",
            "rootUri": root_uri,
            "workspaceFolders": [{ "uri": root_uri, "name": folder_name }],
            "capabilities": {
                "workspace": { "applyEdit": false, "workspaceFolders": true, "configuration": true },
                "textDocument": {
                    "synchronization": { "dynamicRegistration": false, "willSave": false, "willSaveWaitUntil": false, "didSave": false },
                    "completion": { "dynamicRegistration": false, "completionItem": {
                        "snippetSupport": false,
                        "commitCharactersSupport": true,
                        "documentationFormat": ["markdown", "plaintext"],
                        "deprecatedSupport": true,
                        "preselectSupport": true,
                        "resolveSupport": { "properties": ["documentation", "detail"] }
                    }}
                }
            }
        });
        let initialized = match client
            .request("initialize", params, executor, REQUEST_TIMEOUT)
            .await
        {
            Ok(initialized) => initialized,
            Err(error) => {
                client.abort();
                return Err(error).context("LSP initialize request failed");
            }
        };
        let capabilities = initialized
            .get("capabilities")
            .cloned()
            .unwrap_or(Value::Null);
        tracing::debug!(server_capabilities = %capabilities, "language server initialized");
        if let Err(error) = client.notify("initialized", json!({})) {
            client.abort();
            return Err(error).context("could not notify LSP that initialization completed");
        }
        Ok(client)
    }

    pub(super) fn notify(&self, method: &str, params: Value) -> anyhow::Result<()> {
        self.outgoing
            .try_send(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .map_err(|error| anyhow!("could not queue LSP notification {method}: {error}"))
    }

    pub(super) async fn request(
        &self,
        method: &str,
        params: Value,
        executor: &BackgroundExecutor,
        timeout: Duration,
    ) -> anyhow::Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if id <= 0 {
            anyhow::bail!("LSP request identifier exhausted");
        }
        let (response_tx, response_rx) = async_channel::bounded(1);
        self.pending
            .lock()
            .map_err(|_| anyhow!("LSP pending-request lock poisoned"))?
            .insert(id, response_tx);
        let _guard = PendingGuard {
            id,
            pending: self.pending.clone(),
        };
        self.outgoing
            .try_send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .map_err(|error| anyhow!("could not queue LSP request {method}: {error}"))?;

        let outcome = race(
            async { RequestRace::Received(response_rx.recv().await) },
            async {
                executor.timer(timeout).await;
                RequestRace::TimedOut
            },
        )
        .await;
        let response = match outcome {
            RequestRace::Received(Ok(response)) => response,
            RequestRace::Received(Err(_)) => {
                anyhow::bail!("language server disconnected during {method}")
            }
            RequestRace::TimedOut => anyhow::bail!("language server request timed out: {method}"),
        };
        if let Some(error) = response.get("error") {
            anyhow::bail!("language server rejected {method}: {error}");
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    pub(super) fn close(&self) {
        let _ = self.request_shutdown();
        self.outgoing.close();
    }

    fn request_shutdown(&self) -> anyhow::Result<()> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.outgoing
            .try_send(json!({ "jsonrpc": "2.0", "id": id, "method": "shutdown", "params": null }))
            .map_err(|error| anyhow!("could not queue LSP shutdown: {error}"))?;
        self.outgoing
            .try_send(json!({ "jsonrpc": "2.0", "method": "exit", "params": null }))
            .map_err(|error| anyhow!("could not queue LSP exit: {error}"))
    }

    fn abort(&self) {
        self.outgoing.close();
    }
}

enum RequestRace {
    Received(Result<Value, async_channel::RecvError>),
    TimedOut,
}

struct PendingGuard {
    id: i64,
    pending: PendingRequests,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&self.id);
        }
    }
}

pub(super) fn command_for(executable: &Path, args: &[&str]) -> anyhow::Result<Command> {
    #[cfg(windows)]
    if executable
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        })
    {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/S", "/V:OFF", "/C"]);
        command.arg(command_script_line(executable, args)?);
        crate::process::suppress_async_console(&mut command);
        return Ok(command);
    }
    let mut command = Command::new(executable);
    command.args(args);
    crate::process::suppress_async_console(&mut command);
    Ok(command)
}

#[cfg(any(windows, test))]
fn command_script_line(executable: &Path, args: &[&str]) -> anyhow::Result<String> {
    let values = std::iter::once(executable.to_string_lossy().into_owned())
        .chain(args.iter().map(|argument| (*argument).to_owned()))
        .collect::<Vec<_>>();
    if values.iter().any(|argument| {
        argument
            .chars()
            .any(|character| matches!(character, '"' | '%' | '!' | '\r' | '\n'))
    }) {
        anyhow::bail!("refusing unsafe command script argument");
    }
    Ok(values
        .into_iter()
        .map(|argument| format!("\"{argument}\""))
        .collect::<Vec<_>>()
        .join(" "))
}

#[cfg(test)]
mod command_script_tests {
    use super::command_script_line;
    use std::path::Path;

    #[test]
    fn quotes_command_script_paths_and_arguments_with_spaces() {
        let line = command_script_line(
            Path::new(r"C:\Users\Example User\AppData\npm.cmd"),
            &[
                "install",
                "--prefix",
                r"C:\Users\Example User\Dopad Data",
                "package",
            ],
        )
        .expect("ordinary command arguments should be accepted");
        assert_eq!(
            line,
            r#""C:\Users\Example User\AppData\npm.cmd" "install" "--prefix" "C:\Users\Example User\Dopad Data" "package""#
        );
    }

    #[test]
    fn rejects_command_shell_expansion_in_paths_or_arguments() {
        assert!(command_script_line(Path::new(r"C:\Tools\npm.cmd"), &["%TEMP%"]).is_err());
        assert!(command_script_line(Path::new(r"C:\Unsafe%PATH%\npm.cmd"), &[]).is_err());
    }
}
async fn writer_loop(mut stdin: ChildStdin, receiver: async_channel::Receiver<Value>) {
    while let Ok(message) = receiver.recv().await {
        let bytes = match serde_json::to_vec(&message) {
            Ok(bytes) if bytes.len() <= MAX_MESSAGE_BYTES => bytes,
            Ok(_) => {
                tracing::warn!("dropping oversized outgoing LSP message");
                continue;
            }
            Err(error) => {
                tracing::warn!(error = %error, "could not serialize outgoing LSP message");
                continue;
            }
        };
        let header = format!("Content-Length: {}\r\n\r\n", bytes.len());
        if stdin.write_all(header.as_bytes()).await.is_err()
            || stdin.write_all(&bytes).await.is_err()
            || stdin.flush().await.is_err()
        {
            break;
        }
    }
}

async fn reader_loop(
    stdout: ChildStdout,
    outgoing: async_channel::Sender<Value>,
    pending: PendingRequests,
    root_uri: String,
    diagnostics: DiagnosticBus,
    server_id: String,
    workspace: PathBuf,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let message = match read_message(&mut reader).await {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(error) => {
                tracing::warn!(error = %error, "language server protocol stream ended with invalid data");
                break;
            }
        };
        if let Some(id) = message.get("id").and_then(Value::as_i64) {
            if let Some(method) = message.get("method").and_then(Value::as_str) {
                let result = client_request_result(method, &message, &root_uri);
                let response = match result {
                    Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    Err(message) => {
                        json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": message } })
                    }
                };
                let _ = outgoing.try_send(response);
            } else if let Ok(mut requests) = pending.lock()
                && let Some(sender) = requests.remove(&id)
            {
                let _ = sender.try_send(message);
            }
        } else if message.get("method").and_then(Value::as_str)
            == Some("textDocument/publishDiagnostics")
            && let Some(params) = message.get("params")
        {
            match serde_json::from_value::<lsp_types::PublishDiagnosticsParams>(params.clone()) {
                Ok(params) => diagnostics.publish(DiagnosticUpdate {
                    workspace: workspace.clone(),
                    server_id: server_id.clone(),
                    params,
                }),
                Err(error) => {
                    tracing::warn!(error = %error, "invalid LSP diagnostics notification")
                }
            }
        }
    }
    if let Ok(mut requests) = pending.lock() {
        requests.clear();
    }
    diagnostics.server_stopped(workspace, server_id);
}

fn client_request_result(method: &str, message: &Value, root_uri: &str) -> Result<Value, String> {
    match method {
        "workspace/configuration" => {
            let count = message
                .pointer("/params/items")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            Ok(Value::Array(vec![Value::Null; count]))
        }
        "workspace/workspaceFolders" => {
            let name = url::Url::parse(root_uri)
                .ok()
                .and_then(|uri| uri.path_segments()?.next_back().map(str::to_owned))
                .unwrap_or_else(|| "workspace".to_owned());
            Ok(json!([{ "uri": root_uri, "name": name }]))
        }
        "window/workDoneProgress/create"
        | "client/registerCapability"
        | "client/unregisterCapability"
        | "window/showMessageRequest" => Ok(Value::Null),
        "workspace/applyEdit" => {
            Ok(json!({ "applied": false, "failureReason": "Dopad does not apply workspace edits" }))
        }
        _ => Err(format!("unsupported client request: {method}")),
    }
}

async fn stderr_loop(mut stderr: ChildStderr) {
    let mut buffer = [0u8; 1024];
    loop {
        match stderr.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(length) => {
                let text = String::from_utf8_lossy(&buffer[..length]);
                let text = text.trim();
                if !text.is_empty() {
                    tracing::debug!(target: "dopad::lsp", output = %text.chars().take(300).collect::<String>(), "language server output");
                }
            }
        }
    }
}

async fn read_message(reader: &mut BufReader<ChildStdout>) -> io::Result<Option<Value>> {
    let mut content_length = None;
    let mut header_bytes = 0usize;
    loop {
        let mut line = Vec::new();
        let count = reader.read_until(b'\n', &mut line).await?;
        if count == 0 {
            return Ok(None);
        }
        header_bytes = header_bytes.saturating_add(count);
        if header_bytes > MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "LSP header too large",
            ));
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
        if let Some(separator) = line.iter().position(|byte| *byte == b':')
            && let (name, value) = (&line[..separator], &line[separator + 1..])
            && name.eq_ignore_ascii_case(b"content-length")
        {
            let value = std::str::from_utf8(value)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid LSP length"))?
                .trim();
            content_length =
                Some(value.parse::<usize>().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid LSP length")
                })?);
        }
    }
    let length = content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing LSP Content-Length"))?;
    if length > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "LSP message too large",
        ));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).await?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_answers_only_safe_server_requests() {
        assert_eq!(
            client_request_result(
                "workspace/configuration",
                &json!({ "params": { "items": [1, 2] } }),
                "file:///workspace"
            )
            .unwrap(),
            json!([null, null])
        );
        assert!(
            client_request_result("workspace/executeCommand", &json!({}), "file:///workspace")
                .is_err()
        );
        assert_eq!(
            client_request_result("workspace/applyEdit", &json!({}), "file:///workspace").unwrap()
                ["applied"],
            false
        );
    }
}

