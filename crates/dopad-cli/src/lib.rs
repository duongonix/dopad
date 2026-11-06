pub mod args;
pub mod launch;
pub mod paths;
pub mod run;

pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_ARGUMENTS: i32 = 2;
pub const EXIT_OPEN: i32 = 3;
pub const EXIT_IPC: i32 = 4;
pub const EXIT_WAIT_CANCELLED: i32 = 5;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn help_text() -> &'static str {
    "Dopad — lightweight native text editor\n\nUsage: dopad [options] [file-or-folder ...]\n\nOptions:\n  -n, --new-window       Open targets in a new editor window\n  -r, --reuse-window     Reuse an existing editor window\n  -g, --goto LOCATION    Open file at LINE[:COLUMN] (1-based)\n      --wait             Wait until the opened text tab is closed\n      --version          Print version\n  -h, --help             Print this help\n      --                 Treat remaining arguments as paths\n\nExamples:\n  dopad README.md\n  dopad .\n  dopad src/main.rs README.md\n  dopad -g src/main.rs:42:8\n  dopad --wait COMMIT_EDITMSG\n\nExit codes: 0 success, 1 general error, 2 invalid arguments,\n3 path/open failure, 4 desktop/IPC failure, 5 wait cancelled.\n"
}
