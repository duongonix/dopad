use std::{
    io,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use dopad_ipc::{
    CliCommand, CliRequest, CliResponse, PROTOCOL_VERSION, ResponsePhase, ResponseStatus,
    read_frame_timeout, write_frame_timeout,
};

use super::{new_request_id, open_paths_request};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const FORWARD_TIMEOUT: Duration = Duration::from_secs(15);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) fn forward_launch(paths: &[PathBuf]) -> io::Result<()> {
    let request = if paths.is_empty() {
        CliRequest {
            version: PROTOCOL_VERSION,
            request_id: new_request_id(),
            command: CliCommand::Focus,
        }
    } else {
        open_paths_request(paths)?
    };
    let deadline = Instant::now() + FORWARD_TIMEOUT;
    let mut delay = Duration::from_millis(30);
    let mut last_error = None;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match dopad_ipc::connect(remaining.min(CONNECT_TIMEOUT)) {
            Ok(mut stream) => {
                write_frame_timeout(&mut stream, &request, RESPONSE_TIMEOUT)?;
                let response: CliResponse = read_frame_timeout(&mut stream, RESPONSE_TIMEOUT)?;
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
                        response.message.unwrap_or_else(|| {
                            "running Dopad instance rejected the launch request".into()
                        }),
                    ));
                }
                return Ok(());
            }
            Err(error) => {
                last_error = Some(error);
                thread::sleep(delay.min(remaining));
                delay = (delay + Duration::from_millis(30)).min(Duration::from_millis(250));
            }
        }
    }

    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "could not connect to the active Desktop IPC endpoint {} within {} seconds: {}",
            dopad_ipc::endpoint_display(),
            FORWARD_TIMEOUT.as_secs(),
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "no connection attempt completed".into())
        ),
    ))
}
