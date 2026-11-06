use std::path::{Path, PathBuf};

pub(super) fn same_path(left: &Path, right: &Path) -> bool {
    let left = canonical_or_original(left);
    let right = canonical_or_original(right);

    #[cfg(windows)]
    {
        windows_path_key(&left) == windows_path_key(&right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(windows)]
fn windows_path_key(path: &Path) -> String {
    let path = path.to_string_lossy().replace('/', "\\");
    let path = path
        .strip_prefix("\\\\?\\UNC\\")
        .map(|rest| format!("\\\\{rest}"))
        .or_else(|| path.strip_prefix("\\\\?\\").map(str::to_owned))
        .unwrap_or(path);
    path.to_lowercase()
}

#[cfg(all(test, windows))]
mod tests {
    use super::same_path;
    use std::path::Path;

    #[test]
    fn matches_case_variants_when_the_file_is_missing() {
        assert!(same_path(
            Path::new(r"C:\Dopad-Missing\Draft.txt"),
            Path::new(r"c:\dopad-missing\DRAFT.TXT"),
        ));
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::same_path;
    use std::path::Path;

    #[test]
    fn compares_paths_case_sensitively() {
        assert!(!same_path(Path::new("draft.txt"), Path::new("Draft.txt")));
    }
}
