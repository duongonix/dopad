use std::{
    ffi::OsString,
    io, thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use dopad_ipc::{
    CliCommand, CliRequest, CliResponse, FrameDecoder, LocalSocketStream, PROTOCOL_VERSION,
    ResponsePhase, ResponseStatus, connect, read_frame, set_recv_timeout, write_frame,
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

pub fn run(args: impl IntoIterator<Item = OsString>) -> i32 {
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

    let (mut stream, launched) = match connect_or_launch() {
        Ok(connection) => connection,
        Err(error) => {
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
            eprintln!("dopad: {message}");
            code
        }
    }
}

fn connect_or_launch() -> Result<(LocalSocketStream, bool), String> {
    if let Ok(stream) = connect(CONNECT_TIMEOUT) {
        return Ok((stream, false));
    }
    let executable = locate_desktop()?;
    let mut child = launch_desktop(&executable)?;
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let mut delay = Duration::from_millis(30);
    loop {
        if let Ok(stream) = connect(CONNECT_TIMEOUT) {
            return Ok((stream, true));
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("could not check Dopad Desktop startup: {error}"))?
        {
            return Err(format!("Dopad Desktop exited during startup ({status})"));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            return Err("timed out waiting for Dopad Desktop IPC readiness".into());
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
    set_recv_timeout(stream, Some(REQUEST_TIMEOUT)).map_err(|error| {
        (
            EXIT_IPC,
            format!("could not configure IPC timeout: {error}"),
        )
    })?;
    write_frame(stream, request).map_err(ipc_error)?;
    let response: CliResponse = read_frame(stream).map_err(ipc_error)?;
    validate_response(&response, request.request_id)?;
    match response.phase {
        ResponsePhase::Complete => response_result(response, false),
        ResponsePhase::Accepted if wait => wait_for_completion(stream, request.request_id),
        ResponsePhase::Accepted => Err((
            EXIT_IPC,
            "Desktop left a request pending unexpectedly".into(),
        )),
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
    set_recv_timeout(stream, Some(WAIT_HEARTBEAT)).map_err(ipc_error)?;
    let mut decoder = FrameDecoder::default();
    loop {
        match decoder.read_available(stream).map_err(|error| {
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
                return response_result(response, true);
            }
            None => {
                write_frame(
                    stream,
                    &CliRequest {
                        version: PROTOCOL_VERSION,
                        request_id,
                        command: CliCommand::Ping { request_id },
                    },
                )
                .map_err(|error| {
                    (
                        EXIT_WAIT_CANCELLED,
                        format!("wait connection was interrupted: {error}"),
                    )
                })?;
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
