use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use dopad_ipc::{NativePath, OpenTarget, TargetKind};

use crate::args::{CommandLine, NavigationSpec};

#[derive(Debug, Default)]
pub struct ResolvedInputs {
    pub targets: Vec<OpenTarget>,
    pub goto: Option<dopad_ipc::GoTo>,
    pub diagnostics: Vec<String>,
}

pub fn resolve_inputs(command: &CommandLine) -> Result<ResolvedInputs, String> {
    let mut paths = command.paths.clone();
    if let Some(navigation) = &command.goto {
        paths.push(navigation.path.clone());
    }
    let cwd = paths
        .iter()
        .any(|path| !path.is_absolute())
        .then(std::env::current_dir)
        .transpose()
        .map_err(|error| format!("cannot determine current directory: {error}"))?;
    let base = cwd.as_deref().unwrap_or_else(|| Path::new(""));

    let goto_path = command
        .goto
        .as_ref()
        .map(|navigation| absolute_lexical(base, &navigation.path));
    let mut resolved = ResolvedInputs::default();
    for path in paths {
        let absolute = absolute_lexical(base, &path);
        match fs::metadata(&absolute) {
            Ok(metadata) if metadata.is_dir() => {
                if command.wait {
                    resolved.diagnostics.push(format!(
                        "dopad: --wait only supports files: {}",
                        display_path(&absolute)
                    ));
                } else {
                    resolved.targets.push(OpenTarget {
                        path: NativePath::from_path(&absolute),
                        kind: TargetKind::Directory,
                    });
                }
            }
            Ok(metadata) if metadata.is_file() => resolved.targets.push(OpenTarget {
                path: NativePath::from_path(&absolute),
                kind: TargetKind::File,
            }),
            Ok(_) => resolved.diagnostics.push(format!(
                "dopad: unsupported filesystem object: {}",
                display_path(&absolute)
            )),
            Err(error) => resolved.diagnostics.push(format!(
                "dopad: cannot access {}: {error}",
                display_path(&absolute)
            )),
        }
    }
    if let Some(NavigationSpec { line, column, .. }) = &command.goto
        && let Some(path) = goto_path
    {
        resolved.goto = Some(dopad_ipc::GoTo {
            path: NativePath::from_path(&path),
            line: *line,
            column: *column,
        });
    }
    Ok(resolved)
}

pub fn absolute_lexical(cwd: &Path, path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

pub fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use crate::args::CommandLine;

    use super::{absolute_lexical, resolve_inputs};

    #[test]
    fn relative_paths_are_resolved_against_captured_cwd() {
        assert_eq!(
            absolute_lexical(Path::new("C:/work/project"), Path::new("../README.md")),
            Path::new("C:/work/README.md")
        );
    }

    #[test]
    fn absolute_paths_are_not_rebased() {
        assert_eq!(
            absolute_lexical(Path::new("C:/work"), Path::new("C:/notes.txt")),
            Path::new("C:/notes.txt")
        );
    }

    #[test]
    fn resolves_files_and_directories_in_order_and_keeps_valid_targets() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let file = directory.path().join("notes with spaces.txt");
        let child_directory = directory.path().join("src");
        fs::write(&file, "draft").expect("temporary file");
        fs::create_dir(&child_directory).expect("child directory");
        let missing = directory.path().join("missing.md");
        let command = CommandLine {
            paths: vec![file.clone(), child_directory.clone(), missing],
            ..CommandLine::default()
        };

        let resolved = resolve_inputs(&command).expect("absolute paths should resolve");
        assert_eq!(resolved.targets.len(), 2);
        assert_eq!(resolved.diagnostics.len(), 1);
        assert_eq!(resolved.targets[0].kind, dopad_ipc::TargetKind::File);
        assert_eq!(resolved.targets[1].kind, dopad_ipc::TargetKind::Directory);
        assert_eq!(
            resolved.targets[0]
                .path
                .clone()
                .into_path()
                .expect("native file path"),
            PathBuf::from(&file)
        );
        assert_eq!(
            resolved.targets[1]
                .path
                .clone()
                .into_path()
                .expect("native directory path"),
            PathBuf::from(&child_directory)
        );
    }
}
