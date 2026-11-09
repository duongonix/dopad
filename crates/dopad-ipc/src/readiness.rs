use std::{
    io, thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::io::Read;

#[cfg(windows)]
use std::os::windows::io::{AsHandle, AsRawHandle};

#[cfg(windows)]
use windows_sys::Win32::System::Pipes::PeekNamedPipe;

use crate::{
    CliCommand, CliRequest, CliResponse, FrameDecoder, LocalSocketStream, PROTOCOL_VERSION,
    ResponsePhase, ResponseStatus, write_frame_timeout,
};

const READ_POLL_MIN: Duration = Duration::from_millis(1);
const READ_POLL_MAX: Duration = Duration::from_millis(10);

pub fn read_available(
    stream: &mut LocalSocketStream,
    decoder: &mut FrameDecoder,
) -> io::Result<Option<Vec<u8>>> {
    #[cfg(windows)]
    {
        decoder.read_available(&mut PeekPipeReader(stream))
    }
    #[cfg(not(windows))]
    {
        decoder.read_available(stream)
    }
}

#[cfg(windows)]
struct PeekPipeReader<'a>(&'a mut LocalSocketStream);

#[cfg(windows)]
impl Read for PeekPipeReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let mut available = 0;
        // Peek distinguishes an idle nonblocking named pipe from a disconnected peer, for which
        // ReadFileEx may otherwise report zero bytes and look like EOF.
        let handle = match &*self.0 {
            LocalSocketStream::NamedPipe(pipe) => pipe.as_handle(),
        };
        let success = unsafe {
            PeekNamedPipe(
                handle.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if success == 0 {
            return Err(io::Error::last_os_error());
        }
        if available == 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let available = usize::try_from(available).unwrap_or(usize::MAX);
        let length = buffer.len().min(available);
        self.0.read(&mut buffer[..length])
    }
}

pub fn read_frame_timeout<T: serde::de::DeserializeOwned>(
    stream: &mut LocalSocketStream,
    timeout: Duration,
) -> io::Result<T> {
    let deadline = Instant::now() + timeout;
    let mut poll = READ_POLL_MIN;
    let mut decoder = FrameDecoder::default();
    let frame = loop {
        match read_available(stream, &mut decoder) {
            Ok(Some(frame)) => break frame,
            Ok(None) if Instant::now() >= deadline => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "timed out waiting for an IPC frame",
                ));
            }
            Ok(None) => {
                thread::sleep(poll.min(deadline.saturating_duration_since(Instant::now())));
                poll = (poll * 2).min(READ_POLL_MAX);
            }
            Err(error) => return Err(error),
        }
    };
    serde_json::from_slice(&frame)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Confirms that the Desktop protocol and GPUI request receiver are ready.
/// Returns `false` for a responsive pre-handshake v1 server.
pub fn probe(
    stream: &mut LocalSocketStream,
    timeout: Duration,
    request_id: u64,
) -> io::Result<bool> {
    write_frame_timeout(
        stream,
        &CliRequest {
            version: PROTOCOL_VERSION,
            request_id,
            command: CliCommand::Ping { request_id },
        },
        timeout,
    )
    .map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("could not send readiness probe: {error}"),
        )
    })?;

    let response: CliResponse = read_frame_timeout(stream, timeout).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("could not read readiness response: {error}"),
        )
    })?;
    if response.version != PROTOCOL_VERSION || response.request_id != request_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IPC readiness response did not match the probe",
        ));
    }
    if response.phase == ResponsePhase::Pong && response.status == ResponseStatus::Success {
        return Ok(true);
    }
    if response.phase == ResponsePhase::Complete
        && response.status == ResponseStatus::InvalidRequest
        && response.message.as_deref() == Some("unexpected ping")
    {
        return Ok(false);
    }
    Err(io::Error::new(
        io::ErrorKind::ConnectionAborted,
        response
            .message
            .unwrap_or_else(|| "Desktop did not complete the IPC readiness probe".into()),
    ))
}
