use std::{
    env,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

pub fn locate_desktop() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("DOPAD_GUI_EXECUTABLE").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "DOPAD_GUI_EXECUTABLE does not point to a file: {}",
            path.display()
        ));
    }
    let current =
        env::current_exe().map_err(|error| format!("cannot locate dopad executable: {error}"))?;
    desktop_candidates(&current)
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| "Dopad Desktop executable was not found beside the CLI installation".into())
}

pub fn launch_desktop(path: &Path) -> Result<Child, String> {
    Command::new(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("failed to start Dopad Desktop: {error}"))
}

fn desktop_candidates(cli: &Path) -> Vec<PathBuf> {
    let Some(parent) = cli.parent() else {
        return Vec::new();
    };
    let mut candidates = vec![
        parent.join(if cfg!(windows) {
            "dopad-app.exe"
        } else {
            "dopad-app"
        }),
        parent.join(if cfg!(windows) {
            "dopad-gui.exe"
        } else {
            "dopad-gui"
        }),
        parent.join(if cfg!(windows) { "Dopad.exe" } else { "Dopad" }),
    ];
    if let Some(parent_parent) = parent.parent() {
        candidates.push(parent_parent.join(if cfg!(windows) { "Dopad.exe" } else { "Dopad" }));
        candidates.push(parent_parent.join(if cfg!(windows) {
            "dopad-gui.exe"
        } else {
            "dopad-gui"
        }));
        // Linux package layout: /usr/bin/dopad -> /usr/lib/dopad/dopad-gui.
        candidates.push(parent_parent.join("lib/dopad/dopad-gui"));
    }
    // macOS app bundle layout: Contents/Resources/bin/dopad.
    for ancestor in cli.ancestors().take(5) {
        if ancestor.file_name().is_some_and(|name| name == "Resources")
            && let Some(contents) = ancestor.parent()
        {
            candidates.push(contents.join("MacOS").join("Dopad"));
        }
    }
    candidates
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::desktop_candidates;

    #[test]
    fn recognizes_development_and_linux_package_layouts() {
        let development = desktop_candidates(Path::new("C:/dopad/target/debug/dopad.exe"));
        let desktop_name = if cfg!(windows) {
            "dopad-app.exe"
        } else {
            "dopad-app"
        };
        assert!(development.iter().any(|path| path.ends_with(desktop_name)));
        let linux = desktop_candidates(Path::new("/usr/bin/dopad"));
        assert!(
            linux
                .iter()
                .any(|path| path == Path::new("/usr/lib/dopad/dopad-gui"))
        );
    }
}
