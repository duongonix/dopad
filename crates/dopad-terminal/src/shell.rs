#[cfg(not(windows))]
use std::path::Path;
use std::{env, ffi::OsString, path::PathBuf};

#[cfg(windows)]
use std::process::Command;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellFamily {
    PowerShell,
    CommandPrompt,
    Bash,
    Zsh,
    Fish,
    Wsl,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Shell {
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub family: ShellFamily,
}

pub fn discover_shells() -> Vec<Shell> {
    let mut shells = Vec::new();
    #[cfg(windows)]
    discover_windows(&mut shells);
    #[cfg(not(windows))]
    discover_unix(&mut shells);
    shells
}

pub fn default_shell(shells: &[Shell]) -> Option<Shell> {
    #[cfg(windows)]
    let preferred = [
        ShellFamily::PowerShell,
        ShellFamily::CommandPrompt,
        ShellFamily::Wsl,
        ShellFamily::Bash,
    ];
    #[cfg(not(windows))]
    let preferred = [ShellFamily::Zsh, ShellFamily::Bash, ShellFamily::Fish];
    preferred
        .iter()
        .find_map(|family| shells.iter().find(|shell| shell.family == *family).cloned())
        .or_else(|| shells.first().cloned())
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let extensions: &[&str] = if cfg!(windows) { &[".exe", ""] } else { &[""] };
    let paths = env::var_os("PATH")?;
    env::split_paths(&paths)
        .flat_map(|directory| {
            extensions
                .iter()
                .map(move |suffix| directory.join(format!("{name}{suffix}")))
        })
        .find(|path| path.is_file())
}

fn add_shell(output: &mut Vec<Shell>, shell: Shell) {
    if !output
        .iter()
        .any(|existing| existing.program == shell.program && existing.args == shell.args)
    {
        output.push(shell);
    }
}

#[cfg(windows)]
fn discover_windows(output: &mut Vec<Shell>) {
    for (command, name, family, args) in [
        (
            "pwsh",
            "PowerShell 7",
            ShellFamily::PowerShell,
            vec!["-NoLogo"],
        ),
        (
            "powershell",
            "Windows PowerShell",
            ShellFamily::PowerShell,
            vec!["-NoLogo"],
        ),
        ("cmd", "Command Prompt", ShellFamily::CommandPrompt, vec![]),
        ("bash", "Git Bash", ShellFamily::Bash, vec!["--login", "-i"]),
    ] {
        if let Some(program) = find_on_path(command) {
            add_shell(
                output,
                Shell {
                    name: name.into(),
                    program,
                    args: args.into_iter().map(OsString::from).collect(),
                    family,
                },
            );
        }
    }

    for variable in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = env::var_os(variable).map(PathBuf::from) {
            let program = root.join("Git\\bin\\bash.exe");
            if program.is_file() {
                add_shell(
                    output,
                    Shell {
                        name: "Git Bash".into(),
                        program,
                        args: ["--login", "-i"].into_iter().map(OsString::from).collect(),
                        family: ShellFamily::Bash,
                    },
                );
            }
        }
    }

    if let Some(wsl) = find_on_path("wsl") {
        // WSL output is UTF-16LE on Windows; discovery is performed only after opening Terminal.
        if let Ok(result) = Command::new(&wsl).args(["--list", "--quiet"]).output()
            && result.status.success()
        {
            let (units, _) = result.stdout.as_chunks::<2>();
            let text = String::from_utf16_lossy(
                &units
                    .iter()
                    .map(|bytes| u16::from_le_bytes(*bytes))
                    .collect::<Vec<_>>(),
            );
            for distribution in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                let distribution = distribution
                    .trim_start_matches('*')
                    .trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
                if distribution.is_empty() {
                    continue;
                }
                add_shell(
                    output,
                    Shell {
                        name: format!("WSL ({distribution})"),
                        program: wsl.clone(),
                        args: ["--distribution".into(), distribution.into()].into(),
                        family: ShellFamily::Wsl,
                    },
                );
            }
        }
    }
}

#[cfg(not(windows))]
fn discover_unix(output: &mut Vec<Shell>) {
    if let Some(program) = env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        let family = family_for(&program);
        add_shell(
            output,
            Shell {
                name: shell_name(&program),
                program,
                args: Vec::new(),
                family,
            },
        );
    }
    for (command, family) in [
        ("zsh", ShellFamily::Zsh),
        ("bash", ShellFamily::Bash),
        ("fish", ShellFamily::Fish),
    ] {
        if let Some(program) = find_on_path(command) {
            add_shell(
                output,
                Shell {
                    name: shell_name(&program),
                    program,
                    args: Vec::new(),
                    family,
                },
            );
        }
    }
}

#[cfg(not(windows))]
fn family_for(program: &Path) -> ShellFamily {
    match program
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
    {
        "fish" => ShellFamily::Fish,
        "zsh" => ShellFamily::Zsh,
        _ => ShellFamily::Bash,
    }
}

#[cfg(not(windows))]
fn shell_name(program: &Path) -> String {
    program
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Shell")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_prefers_a_supported_system_shell() {
        let shells = vec![
            Shell {
                name: "Other".into(),
                program: "other".into(),
                args: vec![],
                family: ShellFamily::Fish,
            },
            Shell {
                name: "Preferred".into(),
                program: "preferred".into(),
                args: vec![],
                family: if cfg!(windows) {
                    ShellFamily::PowerShell
                } else {
                    ShellFamily::Zsh
                },
            },
        ];
        assert_eq!(
            default_shell(&shells).map(|shell| shell.name),
            Some("Preferred".into())
        );
    }
}
