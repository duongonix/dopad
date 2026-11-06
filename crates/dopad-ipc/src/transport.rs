use std::{
    collections::hash_map::DefaultHasher,
    env,
    hash::{Hash, Hasher},
    io,
    time::Duration,
};

#[cfg(unix)]
use interprocess::local_socket::{GenericFilePath, ToFsName};
#[cfg(windows)]
use interprocess::local_socket::{GenericNamespaced, ToNsName};
use interprocess::{
    ConnectWaitMode,
    local_socket::{ConnectOptions, ListenerNonblockingMode, ListenerOptions},
};
#[cfg(unix)]
use std::{fs, path::PathBuf};

use crate::{LocalSocketListener, LocalSocketStream};

#[cfg(unix)]
fn endpoint_path() -> io::Result<PathBuf> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    // Keep the socket inside an owner-only directory; the directory protects the
    // endpoint from other local users even during listener creation.
    let uid = unsafe { libc::geteuid() };
    let directory = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| {
            fs::metadata(path).is_ok_and(|metadata| metadata.is_dir() && metadata.uid() == uid)
        })
        .unwrap_or_else(|| env::temp_dir());
    let private_dir = directory.join(format!("dopad-{uid}"));
    match fs::create_dir(&private_dir) {
        Ok(()) => fs::set_permissions(&private_dir, fs::Permissions::from_mode(0o700))?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(&private_dir)?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata.uid() != uid {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Dopad IPC directory is not owned by the current user",
                ));
            }
            if metadata.permissions().mode() & 0o077 != 0 {
                fs::set_permissions(&private_dir, fs::Permissions::from_mode(0o700))?;
            }
        }
        Err(error) => return Err(error),
    }
    Ok(private_dir.join("dopad.sock"))
}

#[cfg(windows)]
fn endpoint_suffix() -> String {
    let identity = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("USERNAME"))
        .or_else(|| env::var_os("LOCALAPPDATA"))
        .unwrap_or_default();
    stable_hash(&identity).to_string()
}

fn stable_hash(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn endpoint_name() -> io::Result<interprocess::local_socket::Name<'static>> {
    #[cfg(windows)]
    {
        format!("dopad-ipc-{:016x}", stable_hash(&endpoint_suffix()))
            .to_ns_name::<GenericNamespaced>()
    }
    #[cfg(unix)]
    {
        endpoint_path()?.to_fs_name::<GenericFilePath>()
    }
}

pub fn bind() -> io::Result<LocalSocketListener> {
    let listener = bind_platform()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(endpoint_path()?, fs::Permissions::from_mode(0o600))?;
    }
    Ok(listener)
}

#[cfg(unix)]
fn bind_platform() -> io::Result<LocalSocketListener> {
    match create_listener() {
        Ok(listener) => Ok(listener),
        Err(bind_error) if is_address_in_use(&bind_error) => {
            match connect(Duration::from_millis(100)) {
                Ok(_) => Err(bind_error),
                Err(connect_error) if is_stale_endpoint_error(&connect_error) => {
                    remove_stale_socket()?;
                    create_listener()
                }
                Err(_) => Err(bind_error),
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn bind_platform() -> io::Result<LocalSocketListener> {
    create_listener()
}

#[cfg(unix)]
fn create_listener() -> io::Result<LocalSocketListener> {
    ListenerOptions::new()
        .name(endpoint_name()?)
        .nonblocking(ListenerNonblockingMode::Accept)
        .try_overwrite(false)
        .create_sync()
}

#[cfg(windows)]
fn create_listener() -> io::Result<LocalSocketListener> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt as _;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use widestring::U16CString;

    let descriptor = U16CString::from_str("D:P(A;;GA;;;OW)(A;;GA;;;SY)")
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let descriptor = SecurityDescriptor::deserialize(descriptor.as_ucstr())?;
    ListenerOptions::new()
        .name(endpoint_name()?)
        .security_descriptor(descriptor)
        .nonblocking(ListenerNonblockingMode::Accept)
        .try_overwrite(false)
        .create_sync()
}

#[cfg(unix)]
fn is_address_in_use(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::AddrInUse || error.kind() == io::ErrorKind::AlreadyExists
}

#[cfg(unix)]
fn is_stale_endpoint_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
    )
}

#[cfg(unix)]
fn remove_stale_socket() -> io::Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let path = endpoint_path()?;
    let metadata = fs::symlink_metadata(&path)?;
    let uid = unsafe { libc::geteuid() };
    if !metadata.file_type().is_socket() || metadata.uid() != uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to remove an IPC endpoint that is not an owned socket",
        ));
    }
    fs::remove_file(path)
}

pub fn connect(timeout: Duration) -> io::Result<LocalSocketStream> {
    ConnectOptions::new()
        .name(endpoint_name()?)
        .wait_mode(ConnectWaitMode::Timeout(timeout))
        .connect_sync()
}

pub fn endpoint_display() -> String {
    #[cfg(windows)]
    {
        format!(
            r"\\.\pipe\dopad-ipc-{:016x}",
            stable_hash(&endpoint_suffix())
        )
    }
    #[cfg(unix)]
    {
        endpoint_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "<unavailable>".into())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };

    use interprocess::{
        ConnectWaitMode,
        local_socket::{ConnectOptions, ListenerNonblockingMode, ListenerOptions},
    };

    use crate::{
        CliCommand, CliRequest, CliResponse, PROTOCOL_VERSION, ResponsePhase, ResponseStatus,
        accept, read_frame, write_frame,
    };

    use super::endpoint_display;

    static NEXT_TEST_NAME: AtomicU64 = AtomicU64::new(1);

    fn test_name() -> interprocess::local_socket::Name<'static> {
        let id = NEXT_TEST_NAME.fetch_add(1, Ordering::Relaxed);
        #[cfg(windows)]
        {
            use interprocess::local_socket::{GenericNamespaced, ToNsName};
            format!("dopad-ipc-test-{}-{id}", std::process::id())
                .to_ns_name::<GenericNamespaced>()
                .expect("test pipe name should be valid")
        }
        #[cfg(unix)]
        {
            use interprocess::local_socket::{GenericFilePath, ToFsName};
            std::env::temp_dir()
                .join(format!("dopad-ipc-test-{}-{id}.sock", std::process::id()))
                .to_fs_name::<GenericFilePath>()
                .expect("test socket path should be valid")
        }
    }

    #[test]
    fn endpoint_is_local_and_user_scoped() {
        let name = endpoint_display();
        assert!(name.contains("dopad"));
        #[cfg(windows)]
        assert!(name.starts_with(r"\\.\pipe\"));
        #[cfg(unix)]
        assert!(name.ends_with("dopad.sock"));
    }

    #[test]
    fn local_transport_round_trips_a_versioned_request() {
        let name = test_name();
        let listener = ListenerOptions::new()
            .name(name.clone())
            .nonblocking(ListenerNonblockingMode::Accept)
            .create_sync()
            .expect("listener should bind");
        let idle_result = accept(&listener);
        assert!(matches!(
            idle_result,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ));

        let mut client = ConnectOptions::new()
            .name(name)
            .wait_mode(ConnectWaitMode::Timeout(Duration::from_secs(2)))
            .connect_sync()
            .expect("client should connect");
        let mut server = accept(&listener).expect("pending client should be accepted");
        write_frame(
            &mut client,
            &CliRequest {
                version: PROTOCOL_VERSION,
                request_id: 19,
                command: CliCommand::Focus,
            },
        )
        .expect("request should encode");
        let request: CliRequest = read_frame(&mut server).expect("request should decode");
        assert_eq!(request.version, PROTOCOL_VERSION);
        assert_eq!(request.request_id, 19);
        assert_eq!(request.command, CliCommand::Focus);
        write_frame(
            &mut server,
            &CliResponse {
                version: PROTOCOL_VERSION,
                request_id: request.request_id,
                phase: ResponsePhase::Complete,
                status: ResponseStatus::Success,
                message: None,
            },
        )
        .expect("response should encode");

        let response: CliResponse = read_frame(&mut client).expect("response should decode");
        assert_eq!(response.request_id, 19);
        assert_eq!(response.status, ResponseStatus::Success);
    }
}
