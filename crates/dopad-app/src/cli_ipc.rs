use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use dopad_ipc::{
    CliCommand, CliRequest, CliResponse, FrameDecoder, LocalSocketStream, NativePath, OpenTarget,
    PROTOCOL_VERSION, ResponsePhase, ResponseStatus, TargetKind, accept, bind, read_available,
    read_frame_timeout, write_frame_timeout,
};

const MAX_CLIENTS: usize = 32;
const CLIENT_POLL: Duration = Duration::from_millis(500);
const INITIAL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);
const INSTANCE_PROBE_TIMEOUT: Duration = Duration::from_millis(250);

mod forward;
pub(crate) use forward::forward_launch;

pub(crate) enum UiMessage {
    Probe {
        request_id: u64,
        response: SyncSender<CliResponse>,
    },
    Request {
        request: CliRequest,
        response: SyncSender<CliResponse>,
    },
    CancelWait {
        request_id: u64,
    },
}

pub(crate) struct ServerHandle {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ServerHandle {
    pub(crate) fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::warn!("CLI IPC server thread panicked during shutdown");
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn start_server(sender: async_channel::Sender<UiMessage>) -> io::Result<ServerHandle> {
    let listener = bind()?;
    tracing::info!(endpoint = %dopad_ipc::endpoint_display(), "CLI IPC endpoint initialized");
    let running = Arc::new(AtomicBool::new(true));
    let server_running = running.clone();
    let server_thread = thread::Builder::new()
        .name("dopad-cli-ipc".into())
        .spawn(move || accept_loop(listener, sender, server_running))?;
    Ok(ServerHandle {
        running,
        thread: Some(server_thread),
    })
}

pub(crate) fn try_forward_launch(paths: &[PathBuf]) -> io::Result<bool> {
    let mut stream = match dopad_ipc::connect(INSTANCE_PROBE_TIMEOUT) {
        Ok(stream) => stream,
        Err(_) => return Ok(false),
    };
    let request = if paths.is_empty() {
        CliRequest {
            version: PROTOCOL_VERSION,
            request_id: u64::from(std::process::id()),
            command: CliCommand::Focus,
        }
    } else {
        open_paths_request(paths)?
    };
    write_frame_timeout(&mut stream, &request, INITIAL_RESPONSE_TIMEOUT)?;
    let response: CliResponse = read_frame_timeout(&mut stream, INITIAL_RESPONSE_TIMEOUT)?;
    if response.version != PROTOCOL_VERSION
        || response.request_id != request.request_id
        || !matches!(
            response.phase,
            ResponsePhase::Complete | ResponsePhase::Accepted
        )
        || response.status != ResponseStatus::Success
    {
        return Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            response
                .message
                .unwrap_or_else(|| "running Dopad instance rejected the launch request".into()),
        ));
    }
    Ok(true)
}

fn open_paths_request(paths: &[PathBuf]) -> io::Result<CliRequest> {
    if paths.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file-open forwarding requires at least one path",
        ));
    }
    let request = CliRequest {
        version: PROTOCOL_VERSION,
        request_id: new_request_id(),
        command: CliCommand::Open {
            targets: paths
                .iter()
                .map(|path| OpenTarget {
                    path: NativePath::from_path(path),
                    kind: if path.is_dir() {
                        TargetKind::Directory
                    } else {
                        TargetKind::File
                    },
                })
                .collect(),
            new_window: false,
            reuse_window: true,
            wait: false,
            goto: None,
        },
    };
    request
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, format!("{error:?}")))?;
    Ok(request)
}

fn new_request_id() -> u64 {
    static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    nanos
        ^ u64::from(std::process::id()).rotate_left(23)
        ^ NEXT_REQUEST.fetch_add(1, Ordering::Relaxed)
}

fn accept_loop(
    listener: dopad_ipc::LocalSocketListener,
    sender: async_channel::Sender<UiMessage>,
    running: Arc<AtomicBool>,
) {
    const ACCEPT_POLL: Duration = Duration::from_millis(25);
    let active = Arc::new(AtomicUsize::new(0));
    while running.load(Ordering::Acquire) {
        match accept(&listener) {
            Ok(stream) => {
                if active.fetch_add(1, Ordering::AcqRel) >= MAX_CLIENTS {
                    active.fetch_sub(1, Ordering::AcqRel);
                    drop(stream);
                    continue;
                }
                let clients = active.clone();
                let sender = sender.clone();
                let result = thread::Builder::new()
                    .name("dopad-cli-client".into())
                    .spawn(move || {
                        let _active = ActiveClient(clients);
                        if let Err(error) = serve_client(stream, sender) {
                            tracing::debug!(error = %error, "CLI IPC client disconnected");
                        }
                    });
                if let Err(error) = result {
                    active.fetch_sub(1, Ordering::AcqRel);
                    tracing::warn!(error = %error, "failed to start CLI IPC client thread");
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                tracing::warn!(error = %error, "CLI IPC accept failed");
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

struct ActiveClient(Arc<AtomicUsize>);

impl Drop for ActiveClient {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn serve_client(
    mut stream: LocalSocketStream,
    sender: async_channel::Sender<UiMessage>,
) -> io::Result<()> {
    tracing::debug!("CLI IPC client connected");
    let mut request: CliRequest = read_frame_timeout(&mut stream, INITIAL_RESPONSE_TIMEOUT)?;
    loop {
        let request_id = request.request_id;
        tracing::debug!(
            request_id,
            command = command_name(&request.command),
            "CLI IPC request received"
        );
        if request.version != PROTOCOL_VERSION {
            return write_frame_timeout(
                &mut stream,
                &response(
                    request_id,
                    ResponsePhase::Complete,
                    ResponseStatus::VersionMismatch,
                    "CLI protocol version mismatch",
                ),
                INITIAL_RESPONSE_TIMEOUT,
            );
        }
        if let Err(error) = request.validate() {
            return write_frame_timeout(
                &mut stream,
                &response(
                    request_id,
                    ResponsePhase::Complete,
                    ResponseStatus::InvalidRequest,
                    &format!("invalid CLI request: {error:?}"),
                ),
                INITIAL_RESPONSE_TIMEOUT,
            );
        }
        let probe_id = match &request.command {
            CliCommand::Ping { request_id } => *request_id,
            _ => break,
        };
        if probe_id != request_id {
            return write_frame_timeout(
                &mut stream,
                &response(
                    request_id,
                    ResponsePhase::Complete,
                    ResponseStatus::InvalidRequest,
                    "readiness probe request ID mismatch",
                ),
                INITIAL_RESPONSE_TIMEOUT,
            );
        }
        let (probe_tx, probe_rx) = sync_channel(1);
        sender
            .send_blocking(UiMessage::Probe {
                request_id,
                response: probe_tx,
            })
            .map_err(|_| {
                io::Error::new(io::ErrorKind::BrokenPipe, "Desktop UI is shutting down")
            })?;
        let ready = probe_rx
            .recv_timeout(INITIAL_RESPONSE_TIMEOUT)
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Desktop main-window IPC receiver did not become ready",
                )
            })?;
        write_frame_timeout(&mut stream, &ready, INITIAL_RESPONSE_TIMEOUT)?;
        request = match read_frame_timeout(&mut stream, INITIAL_RESPONSE_TIMEOUT) {
            Ok(request) => request,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        };
    }
    let request_id = request.request_id;

    let wait_for_completion = matches!(&request.command, CliCommand::Open { wait: true, .. });
    let (response_tx, response_rx) = sync_channel(2);
    sender
        .send_blocking(UiMessage::Request {
            request,
            response: response_tx,
        })
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Desktop UI is shutting down"))?;
    tracing::debug!(request_id, "CLI IPC request queued for the main window");
    let first = match response_rx.recv_timeout(INITIAL_RESPONSE_TIMEOUT) {
        Ok(response) => response,
        Err(_) => {
            let _ = sender.try_send(UiMessage::CancelWait { request_id });
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Desktop did not process the request in time",
            ));
        }
    };
    if let Err(error) = write_frame_timeout(&mut stream, &first, INITIAL_RESPONSE_TIMEOUT) {
        if first.phase == ResponsePhase::Accepted {
            let _ = sender.try_send(UiMessage::CancelWait { request_id });
        }
        return Err(error);
    }
    tracing::debug!(
        request_id,
        accepted = first.phase == ResponsePhase::Accepted,
        "Desktop acknowledged the CLI IPC request"
    );
    if first.phase != ResponsePhase::Accepted {
        return Ok(());
    }
    if !wait_for_completion {
        return Ok(());
    }
    let mut decoder = FrameDecoder::default();
    loop {
        match response_rx.try_recv() {
            Ok(final_response) => {
                write_frame_timeout(&mut stream, &final_response, INITIAL_RESPONSE_TIMEOUT)?;
                return Ok(());
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                let _ = sender.try_send(UiMessage::CancelWait { request_id });
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Desktop closed the pending CLI request",
                ));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        let incoming = match read_available(&mut stream, &mut decoder) {
            Ok(incoming) => incoming,
            Err(error) => {
                let _ = sender.try_send(UiMessage::CancelWait { request_id });
                return Err(error);
            }
        };
        if let Some(bytes) = incoming {
            let ping: CliRequest = serde_json::from_slice(&bytes).map_err(|error| {
                let _ = sender.try_send(UiMessage::CancelWait { request_id });
                io::Error::new(io::ErrorKind::InvalidData, error)
            })?;
            if ping.version != PROTOCOL_VERSION
                || ping.request_id != request_id
                || ping.command != (CliCommand::Ping { request_id })
            {
                let _ = sender.try_send(UiMessage::CancelWait { request_id });
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid wait heartbeat",
                ));
            }
            if let Err(error) = write_frame_timeout(
                &mut stream,
                &response(request_id, ResponsePhase::Pong, ResponseStatus::Success, ""),
                INITIAL_RESPONSE_TIMEOUT,
            ) {
                let _ = sender.try_send(UiMessage::CancelWait { request_id });
                return Err(error);
            }
        } else {
            thread::sleep(CLIENT_POLL);
        }
    }
}

fn command_name(command: &CliCommand) -> &'static str {
    match command {
        CliCommand::Focus => "focus",
        CliCommand::Open { .. } => "open",
        CliCommand::Ping { .. } => "ping",
    }
}

fn response(
    request_id: u64,
    phase: ResponsePhase,
    status: ResponseStatus,
    message: &str,
) -> CliResponse {
    CliResponse {
        version: PROTOCOL_VERSION,
        request_id,
        phase,
        status,
        message: (!message.is_empty()).then(|| message.to_owned()),
    }
}

pub(crate) fn complete_response(
    request_id: u64,
    status: ResponseStatus,
    message: Option<String>,
) -> CliResponse {
    CliResponse {
        version: PROTOCOL_VERSION,
        request_id,
        phase: ResponsePhase::Complete,
        status,
        message,
    }
}

pub(crate) fn ready_response(request_id: u64) -> CliResponse {
    CliResponse {
        version: PROTOCOL_VERSION,
        request_id,
        phase: ResponsePhase::Pong,
        status: ResponseStatus::Success,
        message: None,
    }
}

pub(crate) fn accepted_response(request_id: u64) -> CliResponse {
    CliResponse {
        version: PROTOCOL_VERSION,
        request_id,
        phase: ResponsePhase::Accepted,
        status: ResponseStatus::Success,
        message: None,
    }
}

pub(crate) fn wait_response(
    request_id: u64,
    status: ResponseStatus,
    message: Option<String>,
) -> CliResponse {
    CliResponse {
        version: PROTOCOL_VERSION,
        request_id,
        phase: ResponsePhase::WaitComplete,
        status,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::open_paths_request;
    use dopad_ipc::{CliCommand, TargetKind};
    use std::path::PathBuf;

    #[test]
    fn startup_open_request_preserves_unicode_paths_and_routes_directories() {
        let file = PathBuf::from(r"C:\Users\Duong\Desktop\Bản nháp.txt");
        let request = open_paths_request(&[file.clone(), PathBuf::from(".")])
            .expect("valid startup paths should make a request");
        let CliCommand::Open {
            targets,
            new_window,
            reuse_window,
            wait,
            goto,
        } = request.command
        else {
            panic!("startup arguments must be forwarded as an open request");
        };

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].path.clone().into_path().unwrap(), file);
        assert_eq!(targets[0].kind, TargetKind::File);
        assert_eq!(targets[1].kind, TargetKind::Directory);
        assert!(!new_window);
        assert!(reuse_window);
        assert!(!wait);
        assert_eq!(goto, None);
    }

    #[test]
    fn startup_open_request_rejects_empty_paths() {
        assert!(open_paths_request(&[]).is_err());
    }
}
