//! GPUI-independent terminal process and emulator primitives.

mod emulator;
mod session;
mod shell;

pub use emulator::{TerminalCell, TerminalColor, TerminalLine, TerminalSnapshot};
pub use session::{PtySize, TerminalSession, TerminalSessionError};
pub use shell::{Shell, ShellFamily, default_shell, discover_shells};
