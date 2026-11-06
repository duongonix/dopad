use std::{ffi::OsString, path::PathBuf};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommandLine {
    pub paths: Vec<PathBuf>,
    pub goto: Option<NavigationSpec>,
    pub new_window: bool,
    pub reuse_window: bool,
    pub wait: bool,
    pub help: bool,
    pub version: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NavigationSpec {
    pub path: PathBuf,
    pub line: usize,
    pub column: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError(pub String);

pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<CommandLine, ParseError> {
    let mut command = CommandLine::default();
    let mut args = args.into_iter().peekable();
    let mut paths_only = false;
    while let Some(argument) = args.next() {
        if paths_only {
            command.paths.push(PathBuf::from(argument));
            continue;
        }
        if argument == "--" {
            paths_only = true;
        } else if argument == "--help" || argument == "-h" {
            command.help = true;
        } else if argument == "--version" {
            command.version = true;
        } else if argument == "--new-window" || argument == "-n" {
            command.new_window = true;
        } else if argument == "--reuse-window" || argument == "-r" {
            command.reuse_window = true;
        } else if argument == "--wait" {
            command.wait = true;
        } else if argument == "--goto" || argument == "-g" {
            let value = args
                .next()
                .ok_or_else(|| ParseError("--goto requires a file:line[:column] value".into()))?;
            command.goto = Some(parse_navigation(value)?);
        } else if let Some(value) = argument
            .to_str()
            .and_then(|text| text.strip_prefix("--goto="))
        {
            command.goto = Some(parse_navigation(OsString::from(value))?);
        } else if argument.to_str().is_some_and(|text| text.starts_with('-')) {
            return Err(ParseError(format!(
                "unknown option: {}",
                argument.to_string_lossy()
            )));
        } else {
            command.paths.push(PathBuf::from(argument));
        }
    }

    if command.new_window && command.reuse_window {
        return Err(ParseError(
            "conflicting options: --new-window and --reuse-window".into(),
        ));
    }
    if command.help || command.version {
        if command.help && command.version {
            return Err(ParseError("--help and --version cannot be combined".into()));
        }
        if command.wait
            || command.goto.is_some()
            || command.new_window
            || command.reuse_window
            || !command.paths.is_empty()
        {
            return Err(ParseError(
                "--help/--version cannot be combined with open options".into(),
            ));
        }
    }
    if command.goto.is_some() && !command.paths.is_empty() {
        return Err(ParseError(
            "--goto cannot be combined with positional paths".into(),
        ));
    }
    if command.wait && command.paths.is_empty() && command.goto.is_none() {
        return Err(ParseError("--wait requires at least one file".into()));
    }
    Ok(command)
}

fn parse_navigation(value: OsString) -> Result<NavigationSpec, ParseError> {
    let components = split_goto_components(&value);
    let (path, line, column) = match components.as_slice() {
        [path, line] => (path.clone(), parse_number(line)?, None),
        [path, line, column] => (
            path.clone(),
            parse_number(line)?,
            Some(parse_number(column)?),
        ),
        _ => {
            return Err(ParseError(
                "invalid --goto value; expected file:line[:column]".into(),
            ));
        }
    };
    if line == 0 || column.is_some_and(|column| column == 0) {
        return Err(ParseError("line and column numbers start at 1".into()));
    }
    if path.is_empty() {
        return Err(ParseError("--goto requires a file path".into()));
    }
    Ok(NavigationSpec {
        path: PathBuf::from(path),
        line,
        column,
    })
}

#[cfg(unix)]
fn split_goto_components(value: &std::ffi::OsStr) -> Vec<std::ffi::OsString> {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let mut pieces = value.as_bytes().rsplitn(3, |byte| *byte == b':');
    let last = pieces.next().unwrap_or_default();
    let previous = pieces.next();
    let path = pieces.next();
    match (previous, path) {
        (Some(column), Some(path))
            if parse_ascii_number(last).is_some() && parse_ascii_number(column).is_some() =>
        {
            vec![
                std::ffi::OsString::from_vec(path.to_vec()),
                std::ffi::OsString::from_vec(column.to_vec()),
                std::ffi::OsString::from_vec(last.to_vec()),
            ]
        }
        (Some(line), Some(path)) if parse_ascii_number(last).is_some() => vec![
            std::ffi::OsString::from_vec(path.to_vec()),
            std::ffi::OsString::from_vec(line.to_vec()),
        ],
        (Some(path), None) if parse_ascii_number(last).is_some() => vec![
            std::ffi::OsString::from_vec(path.to_vec()),
            std::ffi::OsString::from_vec(last.to_vec()),
        ],
        _ => vec![value.to_os_string()],
    }
}

#[cfg(windows)]
fn split_goto_components(value: &std::ffi::OsStr) -> Vec<std::ffi::OsString> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let units: Vec<u16> = value.encode_wide().collect();
    let separators: Vec<usize> = units
        .iter()
        .enumerate()
        .filter_map(|(index, unit)| (*unit == u16::from(b':')).then_some(index))
        .collect();
    for split_count in [2, 1] {
        if separators.len() < split_count {
            continue;
        }
        let last_separator = separators[separators.len() - 1];
        let last = String::from_utf16_lossy(&units[last_separator + 1..]);
        let Some(last_number) = parse_ascii_number(last.as_bytes()) else {
            continue;
        };
        if split_count == 2 {
            let previous_separator = separators[separators.len() - 2];
            let previous = String::from_utf16_lossy(&units[previous_separator + 1..last_separator]);
            if let Some(line) = parse_ascii_number(previous.as_bytes()) {
                return vec![
                    OsString::from_wide(&units[..previous_separator]),
                    OsString::from(line.to_string()),
                    OsString::from(last_number.to_string()),
                ];
            }
        }
        return vec![
            OsString::from_wide(&units[..last_separator]),
            OsString::from(last_number.to_string()),
        ];
    }
    vec![value.to_os_string()]
}

fn parse_number(value: &std::ffi::OsStr) -> Result<usize, ParseError> {
    value
        .to_str()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| ParseError("invalid line or column number in --goto".into()))
}

#[cfg(unix)]
fn parse_ascii_number(value: &[u8]) -> Option<usize> {
    std::str::from_utf8(value).ok()?.parse().ok()
}

#[cfg(windows)]
fn parse_ascii_number(value: &[u8]) -> Option<usize> {
    std::str::from_utf8(value).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::PathBuf};

    use super::{parse_args, parse_navigation};

    #[test]
    fn parses_multiple_files_and_window_options() {
        let parsed = parse_args([
            "-n".into(),
            "src/main.rs".into(),
            "notes with spaces.txt".into(),
        ])
        .expect("valid args");
        assert!(parsed.new_window);
        assert_eq!(parsed.paths.len(), 2);
    }

    #[test]
    fn parses_empty_and_unicode_paths_with_spaces() {
        let empty = parse_args(std::iter::empty::<OsString>()).expect("no arguments are valid");
        assert_eq!(empty, Default::default());

        let parsed = parse_args(["notes with spaces.txt".into(), "資料.md".into()])
            .expect("paths should remain opaque arguments");
        assert_eq!(
            parsed.paths,
            [
                PathBuf::from("notes with spaces.txt"),
                PathBuf::from("資料.md")
            ]
        );
    }

    #[test]
    fn long_flags_and_path_terminator_match_short_forms() {
        let parsed = parse_args([
            "--reuse-window".into(),
            "--wait".into(),
            "--".into(),
            "-draft.txt".into(),
        ])
        .expect("valid long options and path terminator");
        assert!(parsed.reuse_window);
        assert!(parsed.wait);
        assert_eq!(parsed.paths, [PathBuf::from("-draft.txt")]);
    }

    #[test]
    fn help_and_version_are_local_commands() {
        assert!(parse_args(["--help".into()]).expect("help parses").help);
        assert!(
            parse_args(["--version".into()])
                .expect("version parses")
                .version
        );
        assert!(parse_args(["--help".into(), "--version".into()]).is_err());
        assert!(parse_args(["--help".into(), "file.txt".into()]).is_err());
    }

    #[test]
    fn rejects_conflicting_window_modes_and_bad_options() {
        assert!(parse_args(["-n".into(), "-r".into()]).is_err());
        assert!(parse_args(["--install-extension".into()]).is_err());
        assert!(parse_args(["--wait".into()]).is_err());
    }

    #[test]
    fn parses_goto_with_windows_drive_letter() {
        let parsed = parse_navigation(OsString::from(r"C:\Projects\Dopad\main.rs:42:8"))
            .expect("valid windows location");
        assert_eq!(parsed.path, PathBuf::from(r"C:\Projects\Dopad\main.rs"));
        assert_eq!((parsed.line, parsed.column), (42, Some(8)));
    }

    #[test]
    fn rejects_zero_navigation_indices() {
        assert!(parse_navigation(OsString::from("main.rs:0")).is_err());
        assert!(parse_navigation(OsString::from("main.rs:1:0")).is_err());
    }
}
