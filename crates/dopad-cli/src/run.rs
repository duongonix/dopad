use std::{
    ffi::OsString,
    io,
    sync::OnceLock,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use dopad_ipc::{
    CliCommand, CliRequest, CliResponse, FrameDecoder, LocalSocketStream, PROTOCOL_VERSION,
    ResponsePhase, ResponseStatus, connect, endpoint_display, probe, read_available,
    read_frame_timeout, write_frame_timeout,
};

use crate::{
    EXIT_ARGUMENTS, EXIT_ERROR, EXIT_IPC, EXIT_OPEN, EXIT_SUCCESS, EXIT_WAIT_CANCELLED, VERSION,
    args::parse_args,
    help_text,
    launch::{launch_desktop, locate_desktop},
    paths::resolve_inputs,
};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(130);
const WAIT_HEARTBEAT: Duration = Duration::from_millis(500);
static CLI_LOG_ENABLED: OnceLock<bool> = OnceLock::new();

pub fn run(args: impl IntoIterator<Item = OsString>) -> i32 {
    log_event("cli_started", None, None);
    let command = match parse_args(args) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("dopad: {}", error.0);
            return EXIT_ARGUMENTS;
        }
    };
    if command.help {
        print!("{}", help_text());
        return EXIT_SUCCESS;
    }
    if command.version {
        println!("dopad {VERSION}");
        return EXIT_SUCCESS;
    }

    let inputs = match resolve_inputs(&command) {
        Ok(inputs) => inputs,
        Err(error) => {
            eprintln!("dopad: {error}");
            return EXIT_ERROR;
        }
    };
    for diagnostic in &inputs.diagnostics {
        eprintln!("{diagnostic}");
    }
    let had_open_errors = !inputs.diagnostics.is_empty();
    if inputs.targets.is_empty() && !command.new_window && command.goto.is_none() {
        if had_open_errors {
            return EXIT_OPEN;
        }
        if command.wait {
            eprintln!("dopad: --wait requires an existing file target");
            return EXIT_ARGUMENTS;
        }
    }
    if inputs.targets.is_empty() && command.goto.is_some() {
        return EXIT_OPEN;
    }

    let request_id = new_request_id();
    let mut request = CliRequest {
        version: PROTOCOL_VERSION,
        request_id,
        command: if inputs.targets.is_empty() && !command.new_window {
            CliCommand::Focus
        } else {
            CliCommand::Open {
                targets: inputs.targets,
                new_window: command.new_window,
                reuse_window: command.reuse_window,
                wait: command.wait,
                goto: inputs.goto,
            }
        },
    };
    if let Err(error) = request.validate() {
        eprintln!("dopad: invalid request: {error:?}");
        return EXIT_ARGUMENTS;
    }

    let (mut stream, launched) = match connect_or_launch(request.request_id) {
        Ok(connection) => connection,
        Err(error) => {
            log_event("ipc_error", Some(request.request_id), None);
            eprintln!("dopad: {error}");
            return EXIT_IPC;
        }
    };
    if launched && let CliCommand::Open { new_window, .. } = &mut request.command {
        // The freshly launched Desktop already owns the requested new window.
        *new_window = false;
    }

    match send_request(&mut stream, &request, command.wait) {
        Ok(()) if had_open_errors => EXIT_OPEN,
        Ok(()) => EXIT_SUCCESS,
        Err((code, message)) => {
            log_event("ipc_error", Some(request.request_id), None);
            eprintln!("dopad: {message}");
            code
        }
    }
}

fn connect_or_launch(request_id: u64) -> Result<(LocalSocketStream, bool), String> {
    match connect(CONNECT_TIMEOUT) {
        Ok(mut stream) => {
            log_event("desktop_endpoint_connected", Some(request_id), None);
            match probe(&mut stream, STARTUP_TIMEOUT, request_id) {
                Ok(true) => {
                    log_event("ipc_ready", Some(request_id), None);
                    return Ok((stream, false));
                }
                Ok(false) => {
                    return connect(CONNECT_TIMEOUT)
                    .map(|stream| (stream, false))
                    .map_err(|error| {
                        format!(
                            "the active Desktop at {} answered the legacy readiness probe, but its request pipe could not be reopened: {error}",
                            endpoint_display()
                        )
                    });
                }
                Err(error) => {
                    log_event(
                        "ipc_readiness_error",
                        Some(request_id),
                        Some(error_kind_name(error.kind())),
                    );
                    return Err(format!(
                        "connected to Desktop IPC endpoint {}, but its main-window readiness probe failed: {error}",
                        endpoint_display()
                    ));
                }
            }
        }
        Err(error) => {
            log_event(
                "desktop_endpoint_unavailable",
                Some(request_id),
                Some(error_kind_name(error.kind())),
            );
        }
    }

    let executable = locate_desktop()?;
    log_event("desktop_launch_requested", Some(request_id), None);
    let mut child = launch_desktop(&executable)?;
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let mut delay = Duration::from_millis(30);
    let mut last_connect_error = None;
    loop {
        match connect(CONNECT_TIMEOUT) {
            Ok(mut stream) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                match probe(&mut stream, remaining, request_id) {
                    Ok(true) => {
                        log_event("ipc_ready", Some(request_id), None);
                        return Ok((stream, true));
                    }
                    Ok(false) => {
                        return connect(remaining.min(Duration::from_secs(3)))
                            .map(|stream| (stream, true))
                            .map_err(|error| {
                                format!(
                                    "Desktop IPC endpoint {} answered the legacy readiness probe, but the request pipe could not be reopened: {error}",
                                    endpoint_display()
                                )
                            });
                    }
                    Err(error) if Instant::now() >= deadline => {
                        log_event("ipc_readiness_timeout", Some(request_id), None);
                        return Err(format!(
                            "Desktop IPC endpoint {} accepted a connection, but its main-window readiness probe did not complete within {} seconds: {error}",
                            endpoint_display(),
                            STARTUP_TIMEOUT.as_secs()
                        ));
                    }
                    Err(error) if error.kind() != io::ErrorKind::TimedOut => {
                        log_event(
                            "ipc_readiness_error",
                            Some(request_id),
                            Some(error_kind_name(error.kind())),
                        );
                        return Err(format!(
                            "Desktop IPC endpoint {} failed its main-window readiness probe: {error}",
                            endpoint_display()
                        ));
                    }
                    Err(_) => {}
                }
            }
            Err(error) => last_connect_error = Some(error),
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("could not check Dopad Desktop startup: {error}"))?
        {
            log_event("desktop_exited_during_startup", Some(request_id), None);
            return Err(format!(
                "Dopad Desktop ({}) exited before its IPC endpoint became ready ({status})",
                executable.display()
            ));
        }
        if Instant::now() >= deadline {
            log_event("ipc_readiness_timeout", Some(request_id), None);
            return Err(format!(
                "Dopad Desktop ({}) stayed running, but IPC endpoint {} did not complete readiness within {} seconds; last connection error: {}",
                executable.display(),
                endpoint_display(),
                STARTUP_TIMEOUT.as_secs(),
                last_connect_error
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "no connection attempt completed".into())
            ));
        }
        thread::sleep(delay);
        delay = (delay + Duration::from_millis(30)).min(Duration::from_millis(250));
    }
}

fn send_request(
    stream: &mut LocalSocketStream,
    request: &CliRequest,
    wait: bool,
) -> Result<(), (i32, String)> {
    write_frame_timeout(stream, request, REQUEST_TIMEOUT).map_err(ipc_error)?;
    log_event("ipc_request_sent", Some(request.request_id), None);
    let response: CliResponse = read_frame_timeout(stream, REQUEST_TIMEOUT).map_err(ipc_error)?;
    validate_response(&response, request.request_id)?;
    log_event(
        "ipc_request_acknowledged",
        Some(request.request_id),
        Some(match &response.phase {
            ResponsePhase::Accepted => "accepted",
            ResponsePhase::Complete => "complete",
            _ => "unexpected",
        }),
    );
    match response.phase {
        ResponsePhase::Complete => response_result(response, false),
        ResponsePhase::Accepted if wait => wait_for_completion(stream, request.request_id),
        ResponsePhase::Accepted => Ok(()),
        _ => Err((
            EXIT_IPC,
            "Desktop returned an unexpected IPC response".into(),
        )),
    }
}

fn wait_for_completion(
    stream: &mut LocalSocketStream,
    request_id: u64,
) -> Result<(), (i32, String)> {
    let mut decoder = FrameDecoder::default();
    loop {
        match read_available(stream, &mut decoder).map_err(|error| {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                (
                    EXIT_WAIT_CANCELLED,
                    "Dopad Desktop disconnected before the wait completed".into(),
                )
            } else {
                ipc_error(error)
            }
        })? {
            Some(frame) => {
                let response: CliResponse = serde_json::from_slice(&frame)
                    .map_err(|error| (EXIT_IPC, format!("invalid Desktop response: {error}")))?;
                validate_response(&response, request_id)?;
                if response.phase == ResponsePhase::Pong {
                    continue;
                }
                if response.phase != ResponsePhase::WaitComplete {
                    return Err((
                        EXIT_IPC,
                        "Desktop returned an unexpected wait response".into(),
                    ));
                }
                log_event("wait_completed", Some(request_id), None);
                return response_result(response, true);
            }
            None => {
                write_frame_timeout(
                    stream,
                    &CliRequest {
                        version: PROTOCOL_VERSION,
                        request_id,
                        command: CliCommand::Ping { request_id },
                    },
                    WAIT_HEARTBEAT,
                )
                .map_err(|error| {
                    (
                        EXIT_WAIT_CANCELLED,
                        format!("wait connection was interrupted: {error}"),
                    )
                })?;
                thread::sleep(WAIT_HEARTBEAT);
            }
        }
    }
}

fn validate_response(response: &CliResponse, request_id: u64) -> Result<(), (i32, String)> {
    if response.version != PROTOCOL_VERSION {
        return Err((
            EXIT_IPC,
            "Desktop uses an incompatible CLI protocol version".into(),
        ));
    }
    if response.request_id != request_id {
        return Err((
            EXIT_IPC,
            "Desktop response did not match this request".into(),
        ));
    }
    Ok(())
}

fn response_result(response: CliResponse, waiting: bool) -> Result<(), (i32, String)> {
    if response.status == ResponseStatus::Success {
        return Ok(());
    }
    let code = if response.status == ResponseStatus::WaitCancelled {
        EXIT_WAIT_CANCELLED
    } else if response.status == ResponseStatus::OpenFailed {
        EXIT_OPEN
    } else {
        EXIT_IPC
    };
    let message = response.message.unwrap_or_else(|| {
        if waiting {
            "wait operation did not complete".into()
        } else {
            "Desktop rejected the request".into()
        }
    });
    Err((code, message))
}

fn ipc_error(error: io::Error) -> (i32, String) {
    (
        EXIT_IPC,
        format!("unable to communicate with Dopad Desktop: {error}"),
    )
}

fn log_event(event: &str, request_id: Option<u64>, detail: Option<&str>) {
    let enabled = CLI_LOG_ENABLED
        .get_or_init(|| std::env::var("DOPAD_CLI_LOG").is_ok_and(|value| value == "1"));
    if !enabled {
        return;
    }
    eprintln!(
        "{}",
        serde_json::json!({
            "component": "dopad-cli",
            "event": event,
            "request_id": request_id,
            "detail": detail,
        })
    );
}

fn error_kind_name(kind: io::ErrorKind) -> &'static str {
    match kind {
        io::ErrorKind::TimedOut => "timed_out",
        io::ErrorKind::NotFound => "not_found",
        io::ErrorKind::ConnectionRefused => "connection_refused",
        io::ErrorKind::PermissionDenied => "permission_denied",
        io::ErrorKind::WouldBlock => "would_block",
        _ => "other",
    }
}

fn new_request_id() -> u64 {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    time ^ u64::from(std::process::id()).rotate_left(23)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::new_request_id;
    use crate::args::parse_args;

    #[test]
    fn help_and_version_are_local_parse_paths() {
        assert!(parse_args([OsString::from("--help")]).expect("help").help);
        assert!(
            parse_args([OsString::from("--version")])
                .expect("version")
                .version
        );
    }

    #[test]
    fn request_ids_are_nonzero_for_regular_invocations() {
        assert_ne!(new_request_id(), 0);
    }
}
