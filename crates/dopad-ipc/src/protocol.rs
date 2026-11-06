use serde::{Deserialize, Serialize};

use crate::NativePath;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_OPEN_TARGETS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CliRequest {
    pub version: u16,
    pub request_id: u64,
    pub command: CliCommand,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum CliCommand {
    Focus,
    Open {
        targets: Vec<OpenTarget>,
        new_window: bool,
        reuse_window: bool,
        wait: bool,
        goto: Option<GoTo>,
    },
    Ping {
        request_id: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenTarget {
    pub path: NativePath,
    pub kind: TargetKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GoTo {
    pub path: NativePath,
    pub line: usize,
    pub column: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CliResponse {
    pub version: u16,
    pub request_id: u64,
    pub phase: ResponsePhase,
    pub status: ResponseStatus,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponsePhase {
    Complete,
    Accepted,
    WaitComplete,
    Pong,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Success,
    InvalidRequest,
    VersionMismatch,
    OpenFailed,
    WaitCancelled,
    InternalError,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    InvalidVersion,
    TooManyTargets,
    EmptyOpenRequest,
    ConflictingWindowMode,
    WaitRequiresFile,
    InvalidNavigation,
}

impl CliRequest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::InvalidVersion);
        }
        if let CliCommand::Open {
            targets,
            new_window,
            reuse_window,
            wait,
            goto,
        } = &self.command
        {
            if targets.len() > MAX_OPEN_TARGETS {
                return Err(ProtocolError::TooManyTargets);
            }
            if *new_window && *reuse_window {
                return Err(ProtocolError::ConflictingWindowMode);
            }
            if targets.is_empty() && (*wait || goto.is_some()) {
                return Err(ProtocolError::EmptyOpenRequest);
            }
            if *wait && targets.iter().any(|target| target.kind != TargetKind::File) {
                return Err(ProtocolError::WaitRequiresFile);
            }
            if goto.as_ref().is_some_and(|navigation| {
                navigation.line == 0 || navigation.column.is_some_and(|column| column == 0)
            }) {
                return Err(ProtocolError::InvalidNavigation);
            }
        }
        Ok(())
    }
}
