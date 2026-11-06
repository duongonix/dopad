use std::{
    io::{Read, Write},
    path::Path,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use async_channel::{Receiver, Sender};
use portable_pty::{
    Child, ChildKiller, CommandBuilder, MasterPty, PtySize as NativePtySize, native_pty_system,
};
use thiserror::Error;

use crate::{
    TerminalSnapshot,
    emulator::{Emulator, TerminalSize},
    shell::Shell,
};

const OUTPUT_BUFFER: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PtySize {
    pub columns: u16,
    pub rows: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl Default for PtySize {
    fn default() -> Self {
        Self {
            columns: 100,
            rows: 32,
            pixel_width: 8,
            pixel_height: 18,
        }
    }
}

impl From<PtySize> for NativePtySize {
    fn from(size: PtySize) -> Self {
        Self {
            cols: size.columns.max(2),
            rows: size.rows.max(1),
            pixel_width: size.pixel_width,
            pixel_height: size.pixel_height,
        }
    }
}

#[derive(Debug, Error)]
pub enum TerminalSessionError {
    #[error("could not create PTY: {0}")]
    Pty(#[from] anyhow::Error),
    #[error("could not start shell: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("terminal writer is no longer available")]
    Closed,
}

enum Command {
    Input(Vec<u8>),
    Resize(PtySize),
    Output,
    Stop,
}

pub struct TerminalSession {
    commands: mpsc::Sender<Command>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    snapshot: Arc<Mutex<Arc<TerminalSnapshot>>>,
    updates: Receiver<()>,
}

impl TerminalSession {
    pub fn spawn(shell: &Shell, cwd: &Path, size: PtySize) -> Result<Self, TerminalSessionError> {
        let pty = native_pty_system();
        let pair = pty.openpty(size.into())?;
        let mut command = CommandBuilder::new(&shell.program);
        for arg in &shell.args {
            command.arg(arg);
        }
        command.cwd(cwd);
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let mut killer = child.clone_killer();
        let master = Arc::new(Mutex::new(pair.master));
        let (response_tx, response_rx) = mpsc::channel();
        let emulator = Arc::new(Mutex::new(Emulator::new(
            TerminalSize {
                columns: usize::from(size.columns),
                rows: usize::from(size.rows),
            },
            response_tx,
        )));
        let snapshot = Arc::new(Mutex::new(Arc::new(
            emulator
                .lock()
                .map(|emulator| emulator.snapshot())
                .unwrap_or_default(),
        )));
        let (update_tx, updates) = async_channel::bounded(1);
        let (commands, command_rx) = mpsc::channel();

        if let Err(error) = start_reader(
            reader,
            child,
            emulator.clone(),
            snapshot.clone(),
            commands.clone(),
            response_rx,
            update_tx.clone(),
        ) {
            let _ = killer.kill();
            return Err(error.into());
        }
        if let Err(error) = start_writer(
            writer,
            master,
            emulator,
            snapshot.clone(),
            command_rx,
            update_tx,
        ) {
            let _ = killer.kill();
            return Err(error.into());
        }

        Ok(Self {
            commands,
            killer,
            snapshot,
            updates,
        })
    }

    pub fn input(&self, bytes: impl Into<Vec<u8>>) -> Result<(), TerminalSessionError> {
        self.commands
            .send(Command::Input(bytes.into()))
            .map_err(|_| TerminalSessionError::Closed)
    }

    pub fn resize(&self, size: PtySize) -> Result<(), TerminalSessionError> {
        self.commands
            .send(Command::Resize(size))
            .map_err(|_| TerminalSessionError::Closed)
    }

    pub fn snapshot(&self) -> Arc<TerminalSnapshot> {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_else(|_| Arc::new(TerminalSnapshot::default()))
    }

    pub fn updates(&self) -> Receiver<()> {
        self.updates.clone()
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        let _ = self.killer.kill();
    }
}

fn start_reader<R: Read + Send + 'static>(
    mut reader: R,
    mut child: Box<dyn Child + Send + Sync>,
    emulator: Arc<Mutex<Emulator>>,
    snapshot: Arc<Mutex<Arc<TerminalSnapshot>>>,
    commands: mpsc::Sender<Command>,
    responses: mpsc::Receiver<Vec<u8>>,
    updates: Sender<()>,
) -> Result<(), std::io::Error> {
    thread::Builder::new()
        .name("dopad-terminal-reader".into())
        .spawn(move || {
            let mut buffer = vec![0; OUTPUT_BUFFER];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(length) => {
                        let Ok(mut terminal) = emulator.lock() else {
                            break;
                        };
                        terminal.advance(&buffer[..length]);
                        for response in responses.try_iter() {
                            let _ = commands.send(Command::Input(response));
                        }
                        drop(terminal);
                        let _ = commands.send(Command::Output);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        if let Ok(mut current) = snapshot.lock() {
                            let mut next = (**current).clone();
                            next.error = Some(error.to_string());
                            *current = Arc::new(next);
                        }
                        let _ = updates.try_send(());
                        break;
                    }
                }
            }
            publish_snapshot(&emulator, &snapshot, &updates);
            if let Ok(status) = child.wait() {
                if let Ok(mut current) = snapshot.lock() {
                    let mut next = (**current).clone();
                    next.exit_status = Some(status.to_string());
                    *current = Arc::new(next);
                }
                let _ = updates.try_send(());
            }
        })
        .map(|_| ())
}

fn publish_snapshot(
    emulator: &Mutex<Emulator>,
    snapshot: &Mutex<Arc<TerminalSnapshot>>,
    updates: &Sender<()>,
) {
    let Ok(terminal) = emulator.lock() else {
        return;
    };
    let mut next = terminal.snapshot();
    drop(terminal);
    if let Ok(mut current) = snapshot.lock() {
        next.exit_status = current.exit_status.clone();
        next.error = current.error.clone();
        *current = Arc::new(next);
    }
    let _ = updates.try_send(());
}

fn start_writer<W: Write + Send + 'static>(
    mut writer: W,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    emulator: Arc<Mutex<Emulator>>,
    snapshot: Arc<Mutex<Arc<TerminalSnapshot>>>,
    commands: mpsc::Receiver<Command>,
    updates: Sender<()>,
) -> Result<(), std::io::Error> {
    thread::Builder::new()
        .name("dopad-terminal-writer".into())
        .spawn(move || {
            let mut publish_deadline: Option<Instant> = None;
            loop {
                let command = if let Some(deadline) = publish_deadline {
                    let wait = deadline.saturating_duration_since(Instant::now());
                    if wait.is_zero() {
                        publish_snapshot(&emulator, &snapshot, &updates);
                        publish_deadline = None;
                        continue;
                    }
                    match commands.recv_timeout(wait) {
                        Ok(command) => command,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            publish_snapshot(&emulator, &snapshot, &updates);
                            publish_deadline = None;
                            continue;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                } else {
                    match commands.recv() {
                        Ok(command) => command,
                        Err(_) => break,
                    }
                };
                match command {
                    Command::Input(bytes) => {
                        if writer
                            .write_all(&bytes)
                            .and_then(|_| writer.flush())
                            .is_err()
                        {
                            break;
                        }
                    }
                    Command::Output => {
                        publish_deadline
                            .get_or_insert_with(|| Instant::now() + Duration::from_millis(16));
                    }
                    Command::Resize(size) => {
                        let native: NativePtySize = size.into();
                        let resized_pty = master
                            .lock()
                            .is_ok_and(|master| master.resize(native).is_ok());
                        if !resized_pty {
                            continue;
                        }
                        if let Ok(mut terminal) = emulator.lock() {
                            terminal.resize(TerminalSize {
                                columns: usize::from(size.columns),
                                rows: usize::from(size.rows),
                            });
                            let next = terminal.snapshot();
                            if let Ok(mut current) = snapshot.lock() {
                                *current = Arc::new(next);
                            }
                            let _ = updates.try_send(());
                            publish_deadline = None;
                        }
                    }
                    Command::Stop => break,
                }
            }
        })
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };

    use super::*;
    use crate::{ShellFamily, TerminalSnapshot};

    #[test]
    fn captures_output_from_a_real_shell_pty() {
        let shell = test_shell();
        let cwd = std::env::current_dir().expect("test working directory is available");
        let session = TerminalSession::spawn(&shell, &cwd, PtySize::default())
            .expect("real shell PTY starts");
        #[cfg(windows)]
        session
            .input(b"echo DOPAD_PTY_SMOKE\r".to_vec())
            .expect("interactive shell accepts input");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut captured = TerminalSnapshot::default();
        while Instant::now() < deadline {
            captured = (*session.snapshot()).clone();
            let output: String = captured
                .lines
                .iter()
                .flat_map(|line| line.0.iter().map(|cell| cell.character))
                .collect();
            if output.contains("DOPAD_PTY_SMOKE") {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let output: String = captured
            .lines
            .iter()
            .flat_map(|line| line.0.iter().map(|cell| cell.character))
            .collect();
        assert!(
            output.contains("DOPAD_PTY_SMOKE"),
            "shell output did not reach the terminal grid; status={:?}, error={:?}, output={output:?}",
            captured.exit_status,
            captured.error
        );
    }

    #[cfg(windows)]
    fn test_shell() -> Shell {
        Shell {
            name: "cmd test".into(),
            program: std::env::var_os("COMSPEC")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("cmd.exe")),
            args: Vec::new(),
            family: ShellFamily::CommandPrompt,
        }
    }

    #[cfg(unix)]
    fn test_shell() -> Shell {
        Shell {
            name: "sh test".into(),
            program: PathBuf::from("/bin/sh"),
            args: ["-c".into(), "printf DOPAD_PTY_SMOKE".into()].to_vec(),
            family: ShellFamily::Bash,
        }
    }
}
