mod frame;
mod path;
mod protocol;
mod readiness;
mod transport;

pub use frame::{FrameDecoder, read_frame, write_frame, write_frame_timeout};
pub use interprocess::local_socket::prelude::{LocalSocketListener, LocalSocketStream};

pub fn accept(listener: &LocalSocketListener) -> std::io::Result<LocalSocketStream> {
    use interprocess::local_socket::traits::Listener as _;
    listener.accept()
}
pub use path::NativePath;
pub use protocol::{
    CliCommand, CliRequest, CliResponse, GoTo, MAX_FRAME_BYTES, MAX_OPEN_TARGETS, OpenTarget,
    PROTOCOL_VERSION, ProtocolError, ResponsePhase, ResponseStatus, TargetKind,
};
pub use readiness::{probe, read_available, read_frame_timeout};
pub use transport::{bind, connect, endpoint_display};
