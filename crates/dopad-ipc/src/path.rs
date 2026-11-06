use std::{ffi::OsString, io, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "platform", content = "units", rename_all = "snake_case")]
pub enum NativePath {
    #[cfg(unix)]
    Unix(Vec<u8>),
    #[cfg(windows)]
    Windows(Vec<u16>),
}

impl NativePath {
    pub fn from_path(path: &std::path::Path) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Self::Unix(path.as_os_str().as_bytes().to_vec())
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            Self::Windows(path.as_os_str().encode_wide().collect())
        }
    }

    pub fn into_path(self) -> io::Result<PathBuf> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            match self {
                Self::Unix(bytes) => Ok(PathBuf::from(OsString::from_vec(bytes))),
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            match self {
                Self::Windows(units) => Ok(PathBuf::from(OsString::from_wide(&units))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::NativePath;

    #[test]
    fn native_path_round_trips_unicode_without_loss() {
        let path = Path::new("folder/資料/notes.txt");
        let encoded = NativePath::from_path(path);
        assert_eq!(encoded.into_path().expect("path should decode"), path);
    }

    #[cfg(unix)]
    #[test]
    fn unix_path_round_trips_non_utf8_bytes() {
        use std::os::unix::ffi::OsStringExt;

        let os_path = std::ffi::OsString::from_vec(vec![b'f', b'/', 0xFF]);
        let path = Path::new(&os_path);
        assert_eq!(
            NativePath::from_path(path)
                .into_path()
                .expect("native path should decode"),
            path
        );
    }
}
