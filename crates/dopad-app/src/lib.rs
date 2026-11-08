#![forbid(unsafe_code)]

mod app;
mod cli_ipc;
mod error;
mod icon;
mod icon_theme;
mod logging;
mod process;
mod session;
mod settings;
mod syntax;
mod theme;
mod updater;
mod window;

pub use app::run;
