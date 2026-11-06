mod frame;
mod path;
mod protocol;
mod transport;

pub use frame::{FrameDecoder, read_frame, write_frame};
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
pub use transport::{bind, connect, endpoint_display};

pub fn set_recv_timeout(
    stream: &LocalSocketStream,
    timeout: Option<std::time::Duration>,
) -> std::io::Result<()> {
    use interprocess::local_socket::traits::Stream as _;
    stream.set_recv_timeout(timeout)
}
