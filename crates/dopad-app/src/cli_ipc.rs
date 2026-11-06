use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use dopad_ipc::{
    CliCommand, CliRequest, CliResponse, FrameDecoder, LocalSocketStream, PROTOCOL_VERSION,
    ResponsePhase, ResponseStatus, accept, bind, read_frame, set_recv_timeout, write_frame,
};

const MAX_CLIENTS: usize = 32;
const CLIENT_POLL: Duration = Duration::from_millis(500);
const INITIAL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) enum UiMessage {
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

pub(crate) fn forward_focus() -> io::Result<()> {
    let mut stream = dopad_ipc::connect(Duration::from_secs(1))?;
    set_recv_timeout(&stream, Some(Duration::from_secs(1)))?;
    let request = CliRequest {
        version: PROTOCOL_VERSION,
        request_id: 1,
        command: CliCommand::Focus,
    };
    write_frame(&mut stream, &request)?;
    let response: CliResponse = read_frame(&mut stream)?;
    if response.version != PROTOCOL_VERSION || response.status != ResponseStatus::Success {
        return Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            response
                .message
                .unwrap_or_else(|| "Desktop rejected focus request".into()),
        ));
    }
    Ok(())
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
    set_recv_timeout(&stream, Some(INITIAL_RESPONSE_TIMEOUT))?;
    let request: CliRequest = read_frame(&mut stream)?;
    let request_id = request.request_id;
    if request.version != PROTOCOL_VERSION {
        return write_frame(
            &mut stream,
            &response(
                request_id,
                ResponsePhase::Complete,
                ResponseStatus::VersionMismatch,
                "CLI protocol version mismatch",
            ),
        );
    }
    if let Err(error) = request.validate() {
        return write_frame(
            &mut stream,
            &response(
                request_id,
                ResponsePhase::Complete,
                ResponseStatus::InvalidRequest,
                &format!("invalid CLI request: {error:?}"),
            ),
        );
    }
    if matches!(&request.command, CliCommand::Ping { .. }) {
        return write_frame(
            &mut stream,
            &response(
                request_id,
                ResponsePhase::Complete,
                ResponseStatus::InvalidRequest,
                "unexpected ping",
            ),
        );
    }

    let (response_tx, response_rx) = sync_channel(2);
    sender
        .send_blocking(UiMessage::Request {
            request,
            response: response_tx,
        })
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Desktop UI is shutting down"))?;
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
    if let Err(error) = write_frame(&mut stream, &first) {
        if first.phase == ResponsePhase::Accepted {
            let _ = sender.try_send(UiMessage::CancelWait { request_id });
        }
        return Err(error);
    }
    if first.phase != ResponsePhase::Accepted {
        return Ok(());
    }
    if let Err(error) = set_recv_timeout(&stream, Some(CLIENT_POLL)) {
        let _ = sender.try_send(UiMessage::CancelWait { request_id });
        return Err(error);
    }
    let mut decoder = FrameDecoder::default();
    loop {
        match response_rx.try_recv() {
            Ok(final_response) => {
                write_frame(&mut stream, &final_response)?;
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
        let incoming = match decoder.read_available(&mut stream) {
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
            if let Err(error) = write_frame(
                &mut stream,
                &response(request_id, ResponsePhase::Pong, ResponseStatus::Success, ""),
            ) {
                let _ = sender.try_send(UiMessage::CancelWait { request_id });
                return Err(error);
            }
        }
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
