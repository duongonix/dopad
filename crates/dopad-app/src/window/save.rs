use std::path::{Path, PathBuf};

use dopad_core::{Encoding, EolKind, FileFingerprint, Revision, save_file_atomic};
use gpui_kit::{App, Context};
use rfd::AsyncFileDialog;

use crate::settings::AppSettings;

use super::{DopadWindow, paths};

struct SaveCompletion {
    id: u64,
    path: PathBuf,
    encoding: Encoding,
    saved_revision: Revision,
    result: Result<(FileFingerprint, EolKind), dopad_core::FileError>,
    close_after: Option<u64>,
}

impl DopadWindow {
    pub(super) fn save_active(
        &mut self,
        force_dialog: bool,
        close_after: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        if tab.editor.is_none() {
            self.set_status("This file type is read-only.");
            cx.notify();
            return;
        }
        self.save_document(tab.id, force_dialog, close_after, cx);
    }

    pub(super) fn save_document(
        &mut self,
        id: u64,
        force_dialog: bool,
        close_after: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        if self.tabs[index].editor.is_none() {
            return;
        }
        let existing_path = self.tabs[index].path.clone();
        let title = self.tabs[index].title.clone();
        cx.spawn(async move |this, cx| {
            let path = if force_dialog || existing_path.is_none() {
                let mut dialog = AsyncFileDialog::new().set_title("Save text file as");
                if let Some(name) = existing_path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .and_then(|name| name.to_str())
                {
                    dialog = dialog.set_file_name(name);
                } else {
                    dialog = dialog.set_file_name(title);
                }
                let Some(file) = dialog.save_file().await else {
                    return;
                };
                Some(file.path().to_path_buf())
            } else {
                existing_path
            };
            let Some(path) = path else {
                return;
            };
            let snapshot = this.update(cx, |view, cx| view.snapshot_document(id, &path, cx));
            let Ok(Some((rope, encoding, eol, expected, revision))) = snapshot else {
                return;
            };
            let save_path = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let text = normalize_line_endings(&rope.to_string(), eol);
                    save_file_atomic(&save_path, &text, encoding, expected)
                        .map(|fingerprint| (fingerprint, eol))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.finish_save(
                    SaveCompletion {
                        id,
                        path,
                        encoding,
                        saved_revision: revision,
                        result,
                        close_after,
                    },
                    cx,
                );
            });
        })
        .detach();
    }

    fn snapshot_document(
        &self,
        id: u64,
        target: &Path,
        cx: &App,
    ) -> Option<(
        gpui_kit::component::input::Rope,
        Encoding,
        EolKind,
        Option<FileFingerprint>,
        Revision,
    )> {
        let tab = self.tabs.iter().find(|tab| tab.id == id)?;
        let ui = tab.ui.as_ref()?;
        let rope = ui.read(cx).text().clone();
        let same_path = tab
            .path
            .as_ref()
            .is_some_and(|path| paths::same_path(path, target));
        let expected = if same_path { tab.fingerprint } else { None };
        let settings = self.settings.current();
        let encoding = if settings.preserve_encoding {
            tab.encoding
        } else {
            settings.default_encoding
        };
        let encoding = apply_bom_behavior(encoding, tab.encoding, &settings.bom_behavior);
        Some((
            rope,
            encoding,
            tab.eol,
            expected,
            tab.editor.as_ref()?.revision(),
        ))
    }

    fn finish_save(&mut self, completion: SaveCompletion, cx: &mut Context<Self>) {
        match completion.result {
            Ok((fingerprint, eol)) => {
                let mut refresh_language = false;
                let mut path_changed = false;
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == completion.id) {
                    path_changed = tab.path.as_ref() != Some(&completion.path);
                    tab.path = Some(completion.path.clone());
                    tab.title = completion
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("Untitled")
                        .to_owned();
                    tab.encoding = completion.encoding;
                    tab.eol = eol;
                    tab.fingerprint = Some(fingerprint);
                    tab.external_change_detected = false;
                    refresh_language = tab.language_override.is_none();
                    if let Some(editor) = tab.editor.as_mut()
                        && editor.revision() == completion.saved_revision
                    {
                        editor.mark_saved();
                    }
                    self.clear_status();
                }
                if refresh_language {
                    self.refresh_auto_language(completion.id, cx);
                }
                if path_changed {
                    self.document_path_changed(completion.id, cx);
                }
                if completion.close_after == Some(completion.id)
                    && self
                        .tabs
                        .iter()
                        .find(|tab| tab.id == completion.id)
                        .is_some_and(|tab| !tab.is_dirty())
                {
                    self.remove_tab(completion.id);
                }
            }
            Err(error) => {
                if matches!(error, dopad_core::FileError::ExternalModification(_))
                    && let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == completion.id)
                {
                    tab.external_change_detected = true;
                }
                self.set_status(error.to_string());
                tracing::warn!(error = %error, "save file failed");
            }
        }
        cx.notify();
    }
}

fn apply_bom_behavior(encoding: Encoding, original: Encoding, behavior: &str) -> Encoding {
    let include_bom = match behavior {
        "Always include BOM" => true,
        "Never include BOM" => false,
        _ => encoding_has_bom(original),
    };
    if include_bom {
        with_bom(encoding)
    } else {
        without_bom(encoding)
    }
}

fn encoding_has_bom(encoding: Encoding) -> bool {
    matches!(
        encoding,
        Encoding::Utf8Bom | Encoding::Utf16Le | Encoding::Utf16Be
    )
}

fn with_bom(encoding: Encoding) -> Encoding {
    match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => Encoding::Utf8Bom,
        Encoding::Utf16Le | Encoding::Utf16LeNoBom => Encoding::Utf16Le,
        Encoding::Utf16Be | Encoding::Utf16BeNoBom => Encoding::Utf16Be,
    }
}

fn without_bom(encoding: Encoding) -> Encoding {
    match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => Encoding::Utf8,
        Encoding::Utf16Le | Encoding::Utf16LeNoBom => Encoding::Utf16LeNoBom,
        Encoding::Utf16Be | Encoding::Utf16BeNoBom => Encoding::Utf16BeNoBom,
    }
}

fn normalize_line_endings(text: &str, eol: EolKind) -> String {
    let line_ending = match eol {
        EolKind::Lf => "\n",
        EolKind::CrLf => "\r\n",
        EolKind::Cr => "\r",
        EolKind::Mixed | EolKind::None => return text.to_owned(),
    };
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace("\n", line_ending)
}

pub(super) fn apply_document_defaults(tab: &mut super::DocumentTab, settings: &AppSettings) {
    tab.word_wrap = settings.word_wrap;
    tab.font_size = f32::from(settings.editor_font_size);
    if tab.path.is_none() {
        tab.encoding = settings.default_encoding;
        tab.eol = settings.default_line_ending;
        let extension = if !settings.default_file_extension.is_empty() {
            settings.default_file_extension.as_str()
        } else {
            match settings.default_file_type.as_str() {
                "Markdown" => "md",
                "JSON" => "json",
                "XML" => "xml",
                _ => "",
            }
        };
        tab.title = if extension.is_empty() {
            "Untitled".into()
        } else {
            format!("Untitled.{extension}")
        };
        tab.detected_language = dopad_syntax::detect_language(Some(Path::new(&tab.title)), "");
    }
}
