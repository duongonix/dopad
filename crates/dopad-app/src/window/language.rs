use std::path::Path;

use dopad_core::{ByteOffset, ByteRange};
use dopad_syntax::Language;
use gpui_kit::Context;

use super::{DocumentTab, DopadWindow};

const LANGUAGE_SNIFF_BYTES: usize = 256;

impl DopadWindow {
    pub(super) fn set_language_mode(
        &mut self,
        document_id: u64,
        override_language: Option<Language>,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == document_id) else {
            return;
        };
        let Some(editor) = self.tabs[index].editor.as_ref() else {
            return;
        };
        let detected = if override_language.is_none() {
            detect_tab_language(&self.tabs[index])
        } else {
            self.tabs[index].detected_language
        };
        let language = override_language.unwrap_or(detected);
        let language_changed = self.tabs[index].language() != language;
        let settings = self.settings.current();
        let large_file = settings.large_file_mode
            && editor.snapshot().len_bytes()
                >= usize::from(settings.syntax_highlighting_threshold_mb)
                    .saturating_mul(1024 * 1024);
        let language_name = if large_file {
            "text"
        } else {
            language.gpui_name().unwrap_or("text")
        };

        let ui = {
            let tab = &mut self.tabs[index];
            tab.detected_language = detected;
            tab.language_override = override_language;
            tab.ui.clone()
        };
        if language_changed {
            self.close_lsp_document(document_id);
        }
        if language_name == "text" {
            if let Some(ui) = ui {
                ui.update(cx, |state, cx| state.set_highlighter("text", cx));
            }
        } else {
            self.schedule_highlighter(document_id, language_name, cx);
        }
        cx.notify();
    }

    pub(super) fn refresh_auto_language(&mut self, document_id: u64, cx: &mut Context<Self>) {
        if self
            .tabs
            .iter()
            .any(|tab| tab.id == document_id && tab.language_override.is_none())
        {
            self.set_language_mode(document_id, None, cx);
        }
    }
}

impl DopadWindow {
    pub(super) fn schedule_highlighter(
        &mut self,
        document_id: u64,
        language_name: &'static str,
        cx: &mut Context<Self>,
    ) {
        if language_name == "text" {
            return;
        }
        let key = (document_id, language_name.to_owned());
        if !self.pending_highlighters.insert(key.clone()) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let register_name = language_name.to_owned();
            cx.background_executor()
                .spawn(async move {
                    crate::syntax::register_highlight_queries_for(&register_name);
                })
                .await;
            let _ = this.update(cx, |window, cx| {
                window.pending_highlighters.remove(&key);
                let settings = window.settings.current();
                let Some(tab) = window.tabs.iter().find(|tab| tab.id == document_id) else {
                    return;
                };
                let Some(editor) = tab.editor.as_ref() else {
                    return;
                };
                let large_file = settings.large_file_mode
                    && editor.snapshot().len_bytes()
                        >= usize::from(settings.syntax_highlighting_threshold_mb)
                            .saturating_mul(1024 * 1024);
                let current_name = if large_file {
                    "text"
                } else {
                    tab.language().gpui_name().unwrap_or("text")
                };
                if current_name != language_name {
                    return;
                }
                let Some(ui) = tab.ui.clone() else {
                    return;
                };
                ui.update(cx, |state, cx| state.set_highlighter(language_name, cx));
                cx.notify();
            });
        })
        .detach();
    }
}

fn detect_tab_language(tab: &DocumentTab) -> Language {
    let Some(editor) = tab.editor.as_ref() else {
        return Language::PlainText;
    };
    let snapshot = editor.snapshot();
    let mut prefix_end = LANGUAGE_SNIFF_BYTES.min(snapshot.len_bytes());
    while prefix_end > 0
        && snapshot
            .validate_char_boundary(ByteOffset(prefix_end))
            .is_err()
    {
        prefix_end -= 1;
    }
    let prefix = match snapshot.slice(ByteRange::new(0, prefix_end)) {
        Ok(prefix) => prefix,
        Err(error) => {
            tracing::warn!(document_id = tab.id, error = %error, "could not read the language detection prefix");
            return Language::PlainText;
        }
    };
    let path = tab.path.as_deref().or_else(|| Some(Path::new(&tab.title)));
    dopad_syntax::detect_language(path, &prefix)
}
