use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use dopad_core::{EolKind, Revision, detect_eol};
use gpui_kit::{Context, Window};
use serde::de::{MapAccess, SeqAccess, Visitor};
use thiserror::Error;

use super::DopadWindow;

const MAX_FORMAT_BYTES: usize = 8 * 1024 * 1024;
const MAX_FORMAT_OUTPUT_BYTES: u64 = 32 * 1024 * 1024;
const RUSTFMT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Error)]
enum FormatError {
    #[error("No formatter is available for this file type.")]
    Unsupported,
    #[error(
        "Formatting mixed line endings is not supported; convert the file to one line-ending style first."
    )]
    MixedLineEndings,
    #[error("The file is larger than the 8 MiB formatting limit.")]
    TooLarge,
    #[error("rustfmt is not installed or could not be started.")]
    RustfmtUnavailable,
    #[error("rustfmt exceeded the 15 second time limit.")]
    TimedOut,
    #[error("rustfmt failed: {0}")]
    RustfmtFailed(String),
    #[error("The formatted output was not valid UTF-8.")]
    InvalidOutput,
    #[error("JSON formatting failed: {0}")]
    InvalidJson(String),
    #[error("JSON formatting failed because an object contains a duplicate key.")]
    DuplicateJsonKey,
    #[error("Formatting failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
enum FormatResult {
    Changed(String),
    Unchanged,
}

pub(super) fn supports_path(path: &Path) -> bool {
    matches!(formatter_for(path), Some(Formatter::Rust | Formatter::Json))
}

#[derive(Clone, Copy)]
enum Formatter {
    Rust,
    Json,
}

fn formatter_for(path: &Path) -> Option<Formatter> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "rs" => Some(Formatter::Rust),
        "json" => Some(Formatter::Json),
        _ => None,
    }
}

fn format_source(path: &Path, source: &str, eol: EolKind) -> Result<FormatResult, FormatError> {
    if source.len() > MAX_FORMAT_BYTES {
        return Err(FormatError::TooLarge);
    }
    if eol == EolKind::Mixed {
        return Err(FormatError::MixedLineEndings);
    }
    let formatter = formatter_for(path).ok_or(FormatError::Unsupported)?;
    let had_final_eol = source.ends_with(['\r', '\n']);
    let formatted = match formatter {
        Formatter::Rust => run_rustfmt(source)?,
        Formatter::Json => format_json(source)?,
    };
    let formatted = normalize_line_endings(&formatted, eol, had_final_eol);
    if formatted == source {
        Ok(FormatResult::Unchanged)
    } else {
        Ok(FormatResult::Changed(formatted))
    }
}

fn format_json(source: &str) -> Result<String, FormatError> {
    let UniqueJsonValue(value): UniqueJsonValue =
        serde_json::from_str(source).map_err(|error| {
            if error.to_string().contains("duplicate object key") {
                FormatError::DuplicateJsonKey
            } else {
                FormatError::InvalidJson(error.to_string())
            }
        })?;
    serde_json::to_string_pretty(&value)
        .map_err(|error| FormatError::InvalidJson(error.to_string()))
}

struct UniqueJsonValue(serde_json::Value);

impl<'de> serde::Deserialize<'de> for UniqueJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct UniqueJsonVisitor;

        impl<'de> Visitor<'de> for UniqueJsonVisitor {
            type Value = UniqueJsonValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON value without duplicate object keys")
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(serde_json::Value::Null))
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(value.into()))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(value.into()))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(value.into()))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                serde_json::Number::from_f64(value)
                    .map(serde_json::Value::Number)
                    .map(UniqueJsonValue)
                    .ok_or_else(|| E::custom("invalid JSON number"))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(value.into()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(value.into()))
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(UniqueJsonValue(value)) = sequence.next_element()? {
                    values.push(value);
                }
                Ok(UniqueJsonValue(values.into()))
            }

            fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = serde_json::Map::new();
                let mut keys = HashSet::new();
                while let Some(key) = object.next_key::<String>()? {
                    if !keys.insert(key.clone()) {
                        return Err(serde::de::Error::custom("duplicate object key"));
                    }
                    let UniqueJsonValue(value) = object.next_value()?;
                    values.insert(key, value);
                }
                Ok(UniqueJsonValue(values.into()))
            }
        }

        deserializer.deserialize_any(UniqueJsonVisitor)
    }
}

fn normalize_line_endings(formatted: &str, eol: EolKind, final_eol: bool) -> String {
    let normalized = formatted.replace("\r\n", "\n").replace('\r', "\n");
    let body = normalized.trim_end_matches('\n');
    let mut output = String::with_capacity(normalized.len() + 8);
    let line_ending = match eol {
        EolKind::CrLf => "\r\n",
        EolKind::Cr => "\r",
        EolKind::Lf | EolKind::None | EolKind::Mixed => "\n",
    };
    for (index, part) in body.split('\n').enumerate() {
        if index > 0 {
            output.push_str(line_ending);
        }
        output.push_str(part);
    }
    if final_eol && !body.is_empty() {
        output.push_str(line_ending);
    }
    output
}

fn run_rustfmt(source: &str) -> Result<String, FormatError> {
    let stdout = tempfile::NamedTempFile::new()?;
    let stderr = tempfile::NamedTempFile::new()?;
    let mut command = Command::new("rustfmt");
    command
        .args([
            "--edition",
            "2024",
            "--emit",
            "stdout",
            "--config",
            "skip_children=true",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::from(stdout.as_file().try_clone()?))
        .stderr(Stdio::from(stderr.as_file().try_clone()?));
    crate::process::suppress_console(&mut command);
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            FormatError::RustfmtUnavailable
        } else {
            FormatError::Io(error)
        }
    })?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| FormatError::Io(std::io::Error::other("rustfmt stdin was not available")))?;
    let (status, write_result) = thread::scope(|scope| {
        let writer = scope.spawn(|| {
            let mut stdin = stdin;
            stdin.write_all(source.as_bytes())
        });
        let status = wait_with_timeout(&mut child, RUSTFMT_TIMEOUT);
        let write_result = writer
            .join()
            .map_err(|_| FormatError::Io(std::io::Error::other("rustfmt input task panicked")))
            .and_then(|result| result.map_err(FormatError::Io));
        (status, write_result)
    });
    let status = status?;
    if !status.success() {
        return Err(FormatError::RustfmtFailed(read_stderr(&stderr)?));
    }
    write_result?;
    let output_len = fs::metadata(stdout.path())?.len();
    if output_len > MAX_FORMAT_OUTPUT_BYTES {
        return Err(FormatError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(output_len as usize);
    File::open(stdout.path())?.read_to_end(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| FormatError::InvalidOutput)
}

fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Result<ExitStatus, FormatError> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
            let _ = child.wait();
            return Err(FormatError::TimedOut);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn read_stderr(file: &tempfile::NamedTempFile) -> Result<String, FormatError> {
    let stderr = File::open(file.path())?;
    let mut bytes = Vec::new();
    stderr.take(8192).read_to_end(&mut bytes)?;
    let message = String::from_utf8_lossy(&bytes).trim().to_owned();
    Ok(if message.is_empty() {
        "the formatter rejected the source file".to_owned()
    } else {
        message
    })
}

impl DopadWindow {
    pub(super) fn format_document(&mut self, _ui_window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        if tab.editor.is_none() {
            self.set_status("Format Document is only available for text files.");
            cx.notify();
            return;
        }
        let Some(path) = tab.path.clone() else {
            self.set_status(FormatError::Unsupported.to_string());
            cx.notify();
            return;
        };
        if !supports_path(&path) {
            self.set_status(FormatError::Unsupported.to_string());
            cx.notify();
            return;
        }
        if self.pending_formats.contains(&tab.id) {
            return;
        }

        let id = tab.id;
        let revision = tab.editor.as_ref().map(dopad_editor::Editor::revision);
        let Some(revision) = revision else {
            return;
        };
        let snapshot = tab.editor.as_ref().map(dopad_editor::Editor::snapshot);
        let Some(snapshot) = snapshot else {
            return;
        };
        let eol = tab.eol;
        let title = tab.title.clone();
        self.pending_formats.insert(id);
        self.set_status(format!("Formatting {title}…"));
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let source = snapshot.to_string();
                    format_source(&path, &source, eol)
                })
                .await;
            let _ = this.update_in(cx, |window, ui_window, cx| {
                finish_format(window, id, revision, result, ui_window, cx);
            });
        })
        .detach();
    }
}

fn finish_format(
    window: &mut DopadWindow,
    id: u64,
    revision: Revision,
    result: Result<FormatResult, FormatError>,
    ui_window: &mut Window,
    cx: &mut Context<DopadWindow>,
) {
    window.pending_formats.remove(&id);
    let Some(index) = window.tabs.iter().position(|tab| tab.id == id) else {
        return;
    };
    if window.tabs[index]
        .editor
        .as_ref()
        .is_none_or(|editor| editor.revision() != revision)
    {
        if window.active_tab().is_some_and(|tab| tab.id == id) {
            window.set_status("Format result discarded because the document changed.");
            cx.notify();
        }
        return;
    }

    match result {
        Ok(FormatResult::Unchanged) => {
            if window.active_tab().is_some_and(|tab| tab.id == id) {
                window.set_status("Document is already formatted.");
                cx.notify();
            }
        }
        Ok(FormatResult::Changed(text)) => {
            let Some(state) = window.tabs[index].ui.as_ref().cloned() else {
                window.set_status("The editor view is not ready to apply formatting.");
                cx.notify();
                return;
            };
            window.tabs[index].eol = detect_eol(&text);
            let title = window.tabs[index].title.clone();
            state.update(cx, |state, cx| state.replace_all(text, ui_window, cx));
            window.set_status(format!("Formatted {title}."));
            cx.notify();
        }
        Err(error) => {
            if window.active_tab().is_some_and(|tab| tab.id == id) {
                window.set_status(error.to_string());
                cx.notify();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatter_support_is_explicit_and_case_insensitive() {
        assert!(supports_path(Path::new("src/main.rs")));
        assert!(supports_path(Path::new("config.JSON")));
        assert!(!supports_path(Path::new("readme.md")));
        assert!(!supports_path(Path::new("archive.zip")));
    }

    #[test]
    fn json_formatting_keeps_key_order_and_line_endings() {
        let source = "{\"z\":1,\"a\":2}\r\n";
        let result =
            format_source(Path::new("data.json"), source, EolKind::CrLf).expect("format JSON");
        let FormatResult::Changed(formatted) = result else {
            panic!("expected formatted JSON");
        };
        assert_eq!(formatted, "{\r\n  \"z\": 1,\r\n  \"a\": 2\r\n}\r\n");
    }

    #[test]
    fn json_formatting_preserves_missing_final_line_ending() {
        let result =
            format_source(Path::new("data.json"), "{\"a\":1}", EolKind::None).expect("format JSON");
        let FormatResult::Changed(formatted) = result else {
            panic!("expected formatted JSON");
        };
        assert_eq!(formatted, "{\n  \"a\": 1\n}");
    }

    #[test]
    fn invalid_json_and_mixed_endings_fail_without_replacing_source() {
        assert!(matches!(
            format_source(Path::new("data.json"), "{broken", EolKind::Lf),
            Err(FormatError::InvalidJson(_))
        ));
        assert!(matches!(
            format_source(Path::new("data.json"), "{}", EolKind::Mixed),
            Err(FormatError::MixedLineEndings)
        ));
        assert!(matches!(
            format_source(
                Path::new("data.json"),
                "{\"outer\":{\"same\":1,\"same\":2}}",
                EolKind::Lf
            ),
            Err(FormatError::DuplicateJsonKey)
        ));
    }

    #[test]
    fn rustfmt_formats_source_without_touching_a_user_file() {
        let result = format_source(
            Path::new("main.rs"),
            "fn main(){println!(\"ok\");}",
            EolKind::None,
        )
        .expect("run rustfmt");
        let FormatResult::Changed(formatted) = result else {
            panic!("expected rustfmt output");
        };
        assert_eq!(formatted, "fn main() {\n    println!(\"ok\");\n}");
    }

    #[test]
    fn rustfmt_parse_failure_does_not_produce_a_formatted_result() {
        let error = run_rustfmt("fn main( {").expect_err("invalid Rust must fail");
        assert!(matches!(error, FormatError::RustfmtFailed(_)));
    }
}
