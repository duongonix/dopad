use gpui_kit::{Action, App, AppContext as _, Context, KeyBinding, Window};

use super::{
    CloseDocument, DopadWindow, DuplicateLine, Find, GoToLine, IgnoreShortcut, MoveLineDown,
    MoveLineUp, NewDocument, OpenDocument, Replace, ResetZoom, SaveAsDocument, SaveDocument,
    ToggleComment, TogglePreview, ToggleWrap, ZoomIn, ZoomOut, editing, preview,
};
use crate::settings::AppSettings;

pub(super) enum FindUiAction {
    Next,
    Previous,
    Case,
    WholeWord,
    Regex,
    Selection,
}

mod specs;
pub(super) use specs::{
    choice_keys, conflicts, display_key, effective_default_key, effective_key, key_from_display,
    specs,
};

fn load_binding<A: Action + 'static>(key: &str, action: A, cx: &App) -> anyhow::Result<KeyBinding> {
    KeyBinding::load(key, Box::new(action), None, false, None, cx).map_err(Into::into)
}

pub(super) fn masked_bindings(settings: &AppSettings, cx: &App) -> anyhow::Result<Vec<KeyBinding>> {
    specs()
        .iter()
        .map(|spec| load_binding(&effective_key(settings, spec.id), IgnoreShortcut, cx))
        .collect()
}

pub(super) fn bindings(settings: &AppSettings, cx: &App) -> anyhow::Result<Vec<KeyBinding>> {
    let key = |id| effective_key(settings, id);
    Ok(vec![
        load_binding(&key("new_document"), NewDocument, cx)?,
        load_binding(&key("open_document"), OpenDocument, cx)?,
        load_binding(&key("save"), SaveDocument, cx)?,
        load_binding(&key("save_as"), SaveAsDocument, cx)?,
        load_binding(&key("close_tab"), CloseDocument, cx)?,
        load_binding(&key("find"), Find, cx)?,
        load_binding(&key("replace"), Replace, cx)?,
        load_binding(&key("find_next"), super::FindNext, cx)?,
        load_binding(&key("find_previous"), super::FindPrevious, cx)?,
        load_binding(&key("find_case"), super::ToggleFindCase, cx)?,
        load_binding(&key("find_whole_word"), super::ToggleFindWholeWord, cx)?,
        load_binding(&key("find_regex"), super::ToggleFindRegex, cx)?,
        load_binding(&key("find_selection"), super::FindSelection, cx)?,
        load_binding(&key("add_next_match"), super::AddNextMatch, cx)?,
        load_binding(&key("close_find"), super::CloseFind, cx)?,
        load_binding(&key("toggle_wrap"), ToggleWrap, cx)?,
        load_binding(&key("zoom_in"), ZoomIn, cx)?,
        load_binding(&key("zoom_in_alternate"), ZoomIn, cx)?,
        load_binding(&key("zoom_out"), ZoomOut, cx)?,
        load_binding(&key("reset_zoom"), ResetZoom, cx)?,
        load_binding(&key("preview"), TogglePreview, cx)?,
        load_binding(&key("go_to_line"), GoToLine, cx)?,
        load_binding(&key("duplicate_line"), DuplicateLine, cx)?,
        load_binding(&key("toggle_comment"), ToggleComment, cx)?,
        load_binding(&key("move_line_up"), MoveLineUp, cx)?,
        load_binding(&key("move_line_down"), MoveLineDown, cx)?,
        load_binding(&key("format_document"), super::FormatDocument, cx)?,
    ])
}

impl DopadWindow {
    pub(super) fn activate_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        super::search::cancel(&mut self.search_task);
        super::search::clear_all(self, cx);
        super::bracket::cancel(&mut self.bracket_task);
        preview::activate(self, index);
        self.clear_status();
        if self.active_tab().is_some_and(|tab| tab.preview_mode) {
            self.ensure_preview(cx);
        }
        if let Some(widget) = self.find_widget.as_ref().cloned()
            && widget.read(cx).is_visible()
        {
            widget.update(cx, |widget, cx| widget.document_changed(cx));
        }
        let request = self
            .find_widget
            .as_ref()
            .filter(|widget| widget.read(cx).is_visible())
            .map(|widget| widget.read(cx).search_request(cx));
        if let Some((options, scope)) = request {
            super::search::start(
                self,
                options,
                scope,
                super::search::SearchAction::Update,
                cx,
            );
        }
        cx.notify();
    }

    pub(super) fn open_search(
        &mut self,
        replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab().is_some_and(|tab| tab.editor.is_some()) {
            return;
        }
        let selected_text = self
            .active_tab()
            .and_then(|tab| {
                let state = tab.ui.as_ref()?.read(cx);
                let range = state.selected_range();
                (range.start < range.end)
                    .then(|| state.text().slice(range.start..range.end).to_string())
            })
            .unwrap_or_default();
        let widget = if let Some(widget) = self.find_widget.as_ref() {
            widget.clone()
        } else {
            let parent = cx.entity().downgrade();
            let widget = cx.new(|cx| {
                super::find_widget::FindWidget::new(window, cx, parent, selected_text.clone())
            });
            self.find_widget = Some(widget.clone());
            widget
        };
        widget.update(cx, |widget, cx| {
            widget.open(replace, selected_text, window, cx);
        });
    }

    pub(super) fn close_search_widget(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(widget) = self.find_widget.as_ref().cloned() else {
            return;
        };
        if !widget.read(cx).is_visible() {
            return;
        }
        super::search::cancel(&mut self.search_task);
        super::search::clear_all(self, cx);
        widget.update(cx, |widget, _| widget.hide());
        if let Some(state) = self.active_tab().and_then(|tab| tab.ui.clone()) {
            state.update(cx, |state, cx| state.focus(window, cx));
        }
    }

    pub(super) fn find_action(
        &mut self,
        action: FindUiAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab().is_some_and(|tab| tab.editor.is_some()) {
            return;
        }
        if self
            .find_widget
            .as_ref()
            .is_none_or(|widget| !widget.read(cx).is_visible())
        {
            self.open_search(false, window, cx);
        }
        let Some(widget) = self.find_widget.as_ref().cloned() else {
            return;
        };
        let selected = if matches!(action, FindUiAction::Selection) {
            self.active_tab().and_then(|tab| {
                let state = tab.ui.as_ref()?.read(cx);
                let range = state.selected_range();
                (range.start < range.end)
                    .then_some(dopad_core::ByteRange::new(range.start, range.end))
            })
        } else {
            None
        };
        widget.update(cx, |widget, cx| match action {
            FindUiAction::Next => widget.navigate_next(cx),
            FindUiAction::Previous => widget.navigate_previous(cx),
            FindUiAction::Case => widget.toggle_case(cx),
            FindUiAction::WholeWord => widget.toggle_whole_word(cx),
            FindUiAction::Regex => widget.toggle_regex(cx),
            FindUiAction::Selection => widget.toggle_find_in_selection(selected, cx),
        });
    }

    pub(super) fn add_next_match(&mut self, cx: &mut Context<Self>) {
        if let Some(widget) = self.find_widget.as_ref().cloned()
            && widget.read(cx).is_visible()
        {
            widget.update(cx, |widget, cx| widget.add_next_match_to_selection(cx));
            return;
        }
        let Some(state) = self.active_tab().and_then(|tab| tab.ui.clone()) else {
            return;
        };
        let (range, query) = {
            let state = state.read(cx);
            let selected = state.selected_range();
            let rope = state.text();
            let range = if selected.start < selected.end {
                selected
            } else {
                let mut start = rope.byte_to_char_idx(state.cursor());
                let mut end = start;
                let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
                while start > 0 && is_word(rope.char(start - 1)) {
                    start -= 1;
                }
                while end < rope.len_chars() && is_word(rope.char(end)) {
                    end += 1;
                }
                rope.char_to_byte_idx(start)..rope.char_to_byte_idx(end)
            };
            if range.start == range.end {
                return;
            }
            (range.clone(), rope.slice(range).to_string())
        };
        if query.is_empty() {
            return;
        }
        state.update(cx, |state, cx| state.set_selected_range(range, cx));
        super::search::start(
            self,
            dopad_editor::SearchOptions::literal(query),
            None,
            super::search::SearchAction::AddNextSelection,
            cx,
        );
    }

    pub(super) fn toggle_wrap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active_tab().is_some_and(|tab| tab.editor.is_some()) {
            return;
        }
        let mut settings = self.settings.current();
        settings.word_wrap = !settings.word_wrap;
        self.settings.replace(settings.clone());
        self.apply_word_wrap_only(settings.word_wrap, window, cx);

        if let Some(path) = self.settings.path() {
            let view = cx.entity();
            cx.spawn(async move |_, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move { crate::settings::save_file(&path, &settings) })
                    .await;
                if let Err(error) = result {
                    view.update(cx, |this, cx| {
                        this.set_status(format!("Word Wrap could not be saved: {error}"));
                        cx.notify();
                    });
                }
            })
            .detach();
        }
    }

    pub(super) fn adjust_zoom(&mut self, delta: f32, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if let super::file_view::TabContent::Image(state) = &mut tab.content {
                state.zoom_by(if delta >= 0.0 { 1.2 } else { 1.0 / 1.2 }, state.fit_zoom);
                cx.notify();
                return;
            }
            if tab.editor.is_none() {
                return;
            }
            tab.font_size = (tab.font_size + delta).clamp(8.0, 36.0);
        }
        cx.notify();
    }

    pub(super) fn reset_zoom(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if let super::file_view::TabContent::Image(state) = &mut tab.content {
                state.reset_zoom();
                cx.notify();
                return;
            }
            if tab.editor.is_none() {
                return;
            }
            tab.font_size = f32::from(self.settings.current().editor_font_size);
        }
        cx.notify();
    }

    pub(super) fn toggle_preview(&mut self, cx: &mut Context<Self>) {
        preview::toggle_preview(self, cx);
    }

    pub(super) fn duplicate_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_current_lines(editing::duplicate_lines, window, cx);
    }

    pub(super) fn toggle_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let Some(state) = tab.ui.clone() else {
            return;
        };
        let text = state.read(cx).text().to_string();
        let prefix = &text[..text.floor_char_boundary(256.min(text.len()))];
        let language = dopad_syntax::detect_language(tab.path.as_deref(), prefix);
        let Some(markers) = language.comment_markers() else {
            self.set_status("Comment toggling is unavailable for plain text.");
            cx.notify();
            return;
        };
        let selection = state.read(cx).selected_range();
        let Some(edit) = editing::toggle_comments(&text, selection, markers) else {
            return;
        };
        state.update(cx, |state, cx| {
            state.set_selected_range(edit.range, cx);
            state.replace(edit.replacement, window, cx);
        });
    }

    pub(super) fn move_line(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_current_lines(
            |text, selection, _eol| editing::move_lines(text, selection, delta),
            window,
            cx,
        );
    }

    fn edit_current_lines(
        &mut self,
        operation: impl FnOnce(
            &str,
            std::ops::Range<usize>,
            dopad_core::EolKind,
        ) -> Option<editing::LineEdit>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let Some(state) = tab.ui.clone() else {
            return;
        };
        let selection = state.read(cx).selected_range();
        let text = state.read(cx).text().to_string();
        let Some(edit) = operation(&text, selection, tab.eol) else {
            return;
        };
        let replacement = edit.replacement;
        let changed_range = edit.range;
        state.update(cx, |state, cx| {
            state.set_selected_range(changed_range, cx);
            state.replace(replacement, window, cx);
        });
    }

    pub(super) fn go_to_line_column(&mut self, line: usize, column: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let Some(editor) = tab.editor.as_ref() else {
            return;
        };
        let Some(state) = tab.ui.clone() else { return };
        let snapshot = editor.snapshot();
        let line_index = line
            .saturating_sub(1)
            .min(snapshot.line_count().saturating_sub(1));
        let offset = snapshot
            .position_to_byte_clamped(dopad_core::TextPosition::new(
                line_index,
                column.saturating_sub(1),
            ))
            .map_or(0, |offset| offset.0);
        state.update(cx, |state, cx| {
            state.set_selected_range(offset..offset, cx);
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::settings::AppSettings;

    use super::{choice_keys, conflicts, display_key, key_from_display};

    #[test]
    fn shortcut_choices_round_trip_for_display_and_storage() {
        for key in choice_keys() {
            let label = display_key(&key);
            assert_eq!(
                key_from_display(&label).as_deref(),
                Some(key.as_str()),
                "{key} should round trip through {label}"
            );
        }
    }

    #[test]
    fn standalone_keys_and_minus_shortcut_parse_from_settings() {
        for (label, expected) in [
            ("F3", "f3"),
            ("Shift+F3", "shift-f3"),
            ("Escape", "escape"),
            ("Ctrl+-", "ctrl--"),
            ("Cmd+Alt+F", "cmd-alt-f"),
            ("Ctrl+Alt+Shift+Down", "ctrl-alt-shift-down"),
        ] {
            assert_eq!(key_from_display(label).as_deref(), Some(expected));
        }
    }

    #[test]
    fn shortcut_conflicts_report_both_actions() {
        let settings = AppSettings {
            shortcut_overrides: BTreeMap::from([("save".into(), "ctrl-n".into())]),
            ..AppSettings::default()
        };
        let conflicts = conflicts(&settings);
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].contains("New document"));
        assert!(conflicts[0].contains("Save"));
    }
}
