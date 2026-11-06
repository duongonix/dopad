use std::{ops::Range, path::PathBuf};

use dopad_core::{Encoding, EolKind, FileFingerprint, LoadedFile, TextPosition};
use dopad_editor::Editor;
use dopad_syntax::Language;
use gpui_kit::Entity;
use gpui_kit::component::input::{EditorState, RangeDecorationCollection};

use super::file_view;

pub struct DocumentTab {
    pub id: u64,
    pub title: String,
    pub path: Option<PathBuf>,
    pub encoding: Encoding,
    pub eol: EolKind,
    pub fingerprint: Option<FileFingerprint>,
    pub external_change_detected: bool,
    pub editor: Option<Editor>,
    pub(super) content: file_view::TabContent,
    pub(super) loading_indicator_visible: bool,
    pub ui: Option<Entity<EditorState>>,
    pub search_decorations: Option<RangeDecorationCollection>,
    pub current_search_decorations: Option<RangeDecorationCollection>,
    pub bracket_decorations: Option<RangeDecorationCollection>,
    pub pending_ui_text: Option<String>,
    pub last_selection: Range<usize>,
    pub word_wrap: bool,
    pub font_size: f32,
    pub preview_mode: bool,
    pub detected_language: Language,
    pub language_override: Option<Language>,
    pub pending_goto: Option<TextPosition>,
}

impl DocumentTab {
    pub(super) fn loading(id: u64, path: PathBuf) -> Self {
        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Untitled")
            .to_owned();
        Self {
            id,
            title,
            path: Some(path),
            encoding: Encoding::Utf8,
            eol: EolKind::Lf,
            fingerprint: None,
            external_change_detected: false,
            editor: None,
            content: file_view::TabContent::Loading,
            loading_indicator_visible: false,
            ui: None,
            search_decorations: None,
            current_search_decorations: None,
            bracket_decorations: None,
            pending_ui_text: None,
            last_selection: 0..0,
            word_wrap: false,
            font_size: 14.0,
            preview_mode: false,
            detected_language: Language::PlainText,
            language_override: None,
            pending_goto: None,
        }
    }

    pub(super) fn unsupported(
        id: u64,
        path: PathBuf,
        info: file_view::UnsupportedFileInfo,
    ) -> Self {
        let mut tab = Self::loading(id, path);
        tab.content = file_view::TabContent::Unsupported(info);
        tab
    }

    pub(super) fn error(id: u64, path: PathBuf, error: file_view::FileOpenError) -> Self {
        let mut tab = Self::loading(id, path);
        tab.content = file_view::TabContent::Error(error);
        tab
    }

    pub(super) fn image(id: u64, path: PathBuf, info: file_view::ImageFileInfo) -> Self {
        let mut tab = Self::loading(id, path);
        tab.content = file_view::TabContent::Image(file_view::ImageTabState::new(info));
        tab
    }

    pub(super) fn blank(id: u64) -> Self {
        Self {
            id,
            title: "Untitled".into(),
            path: None,
            encoding: Encoding::Utf8,
            eol: EolKind::Lf,
            fingerprint: None,
            external_change_detected: false,
            editor: Some(Editor::new("")),
            content: file_view::TabContent::Text,
            loading_indicator_visible: false,
            ui: None,
            search_decorations: None,
            current_search_decorations: None,
            bracket_decorations: None,
            pending_ui_text: Some(String::new()),
            last_selection: 0..0,
            word_wrap: false,
            font_size: 14.0,
            preview_mode: false,
            detected_language: Language::PlainText,
            language_override: None,
            pending_goto: None,
        }
    }

    pub(super) fn loaded(id: u64, loaded: file_view::LoadedTextFile) -> Self {
        let title = loaded
            .loaded
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Untitled")
            .to_owned();
        Self {
            id,
            title,
            path: Some(loaded.loaded.path),
            encoding: loaded.loaded.encoding,
            eol: loaded.loaded.eol,
            fingerprint: Some(loaded.loaded.fingerprint),
            external_change_detected: false,
            editor: Some(loaded.editor),
            content: file_view::TabContent::Text,
            loading_indicator_visible: false,
            ui: None,
            search_decorations: None,
            current_search_decorations: None,
            bracket_decorations: None,
            pending_ui_text: Some(loaded.loaded.text),
            last_selection: 0..0,
            word_wrap: false,
            font_size: 14.0,
            preview_mode: false,
            detected_language: loaded.language,
            language_override: None,
            pending_goto: None,
        }
    }

    pub(super) fn loaded_from_file(id: u64, loaded: LoadedFile) -> Self {
        let editor = Editor::new(&loaded.text);
        let language = dopad_syntax::detect_language(Some(&loaded.path), &loaded.text);
        Self::loaded(
            id,
            file_view::LoadedTextFile {
                loaded,
                editor,
                language,
            },
        )
    }

    pub fn is_dirty(&self) -> bool {
        self.editor.as_ref().is_some_and(Editor::is_dirty)
    }

    pub fn language(&self) -> Language {
        self.language_override.unwrap_or(self.detected_language)
    }

    pub(super) fn is_replaceable_blank(&self) -> bool {
        self.path.is_none()
            && matches!(self.content, file_view::TabContent::Text)
            && self
                .editor
                .as_ref()
                .is_some_and(|editor| !editor.is_dirty() && editor.snapshot().is_empty())
    }
}
