use gpui_kit::component::input::{EditorState, InputEvent, TabSize};
use gpui_kit::{AppContext, Context, Window};

use super::DopadWindow;

impl DopadWindow {
    pub(super) fn ensure_active_ui(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        if tab.ui.is_some() || tab.editor.is_none() {
            return;
        }
        let text = tab.pending_ui_text.take().unwrap_or_default();
        let language = tab.language();
        let soft_wrap = tab.word_wrap;
        let settings = self.settings.current();
        let large_file = settings.large_file_mode
            && text.len()
                >= usize::from(settings.syntax_highlighting_threshold_mb)
                    .saturating_mul(1024 * 1024);
        let language_name = if large_file {
            "text"
        } else {
            language.gpui_name().unwrap_or("text")
        };
        let state = cx.new(|cx| {
            EditorState::new(window, cx)
                .line_number(settings.show_line_numbers)
                .searchable(false)
                .soft_wrap(soft_wrap)
                .auto_close(settings.auto_close_brackets)
                .smart_indent(settings.smart_indent)
                .indent_guides(settings.indent_guides)
                .folding(!large_file)
                // Start plain so query registration cannot delay the editor's first paint.
                .language("text")
                .tab_size(TabSize {
                    tab_size: usize::from(settings.tab_size),
                    hard_tabs: !settings.insert_spaces,
                })
                .default_value(text)
        });
        let (search_decorations, current_search_decorations, bracket_decorations) =
            state.update(cx, |state, cx| {
                (
                    state.create_range_decorations_collection(Vec::new(), cx),
                    state.create_range_decorations_collection(Vec::new(), cx),
                    state.create_range_decorations_collection(Vec::new(), cx),
                )
            });
        if let Some(position) = tab.pending_goto.take() {
            let offset = tab.editor.as_ref().map_or(0, |editor| {
                editor
                    .snapshot()
                    .position_to_byte_clamped(position)
                    .map_or(0, |offset| offset.0)
            });
            state.update(cx, |state, cx| state.set_selected_range(offset..offset, cx));
        } else if tab.last_selection != (0..0) {
            let selection = tab.last_selection.clone();
            state.update(cx, |state, cx| state.set_selected_range(selection, cx));
        }
        let tab_id = tab.id;
        tab.last_selection = state.read(cx).selected_range();
        tab.search_decorations = Some(search_decorations);
        tab.current_search_decorations = Some(current_search_decorations);
        tab.bracket_decorations = Some(bracket_decorations);
        cx.subscribe(&state, move |this, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.sync_ui_change(tab_id, &state, cx);
            }
        })
        .detach();
        cx.observe(&state, move |this, state, cx| {
            this.remember_selection(tab_id, &state, cx);
        })
        .detach();
        tab.ui = Some(state);
        self.schedule_highlighter(tab_id, language_name, cx);
    }
}
