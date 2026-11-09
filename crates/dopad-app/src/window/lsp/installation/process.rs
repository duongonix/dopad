use std::time::Duration;

use anyhow::Context as _;
use async_process::{Child, ChildStderr, ChildStdout};
use futures_lite::{
    future::{race, zip},
    io::{AsyncRead, AsyncReadExt},
};
use gpui_kit::BackgroundExecutor;

const MAX_CAPTURE_BYTES: usize = 4096;

pub(super) struct ProcessOutput {
    pub(super) success: bool,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

impl ProcessOutput {
    pub(super) fn summary(&self) -> String {
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

pub(super) async fn run_child(
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
