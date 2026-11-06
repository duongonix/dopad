use dopad_core::{ByteRange, Revision};
use dopad_editor::{SearchMatch, SearchOptions};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{App, AppContext as _, Context, Entity, WeakEntity, Window};

use super::{DopadWindow, search, search::SearchAction};

mod render;

pub(super) struct FindWidget {
    parent: WeakEntity<DopadWindow>,
    find: Entity<InputState>,
    replace: Entity<InputState>,
    visible: bool,
    replace_mode: bool,
    match_case: bool,
    whole_word: bool,
    regex: bool,
    find_in_selection: bool,
    scope: Option<ByteRange>,
    submission_generation: u64,
    matches: Vec<SearchMatch>,
    current: Option<usize>,
    matches_document: Option<u64>,
    matches_revision: Option<Revision>,
    error: Option<String>,
    result_message: Option<String>,
}

impl FindWidget {
    pub(super) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        parent: WeakEntity<DopadWindow>,
        selected_text: String,
    ) -> Self {
        let find = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
        let replace = cx.new(|cx| InputState::new(window, cx).placeholder("Replace"));
        if !selected_text.is_empty() {
            find.update(cx, |state, cx| state.set_value(selected_text, window, cx));
        }
        cx.subscribe(&find, |this, _, event: &InputEvent, cx| match event {
            InputEvent::Change => this.submit(SearchAction::Update, cx),
            InputEvent::PressEnter { shift, .. } => this.submit(
                if *shift {
                    SearchAction::Previous
                } else {
                    SearchAction::Next
                },
                cx,
            ),
            InputEvent::Focus | InputEvent::Blur => {}
        })
        .detach();
        cx.subscribe(&replace, |this, _, event: &InputEvent, cx| match event {
            InputEvent::Change => {
                this.result_message = None;
                cx.notify();
            }
            InputEvent::PressEnter { shift, .. } => this.submit(
                if *shift {
                    SearchAction::Previous
                } else {
                    SearchAction::Next
                },
                cx,
            ),
            InputEvent::Focus | InputEvent::Blur => {}
        })
        .detach();

        Self {
            parent,
            find,
            replace,
            visible: false,
            replace_mode: false,
            match_case: false,
            whole_word: false,
            regex: false,
            find_in_selection: false,
            scope: None,
            submission_generation: 0,
            matches: Vec::new(),
            current: None,
            matches_document: None,
            matches_revision: None,
            error: None,
            result_message: None,
        }
    }

    pub(super) fn is_visible(&self) -> bool {
        self.visible
    }

    pub(super) fn document_changed(&mut self, cx: &mut Context<Self>) {
        self.submission_generation = self.submission_generation.wrapping_add(1);
        self.scope = None;
        self.find_in_selection = false;
        self.matches.clear();
        self.current = None;
        self.matches_document = None;
        self.matches_revision = None;
        self.error = None;
        self.result_message = None;
        cx.notify();
    }

    pub(super) fn search_request(&self, cx: &App) -> (SearchOptions, Option<ByteRange>) {
        let mut options = SearchOptions::literal(self.find.read(cx).value().to_string());
        options.case_sensitive = self.match_case;
        options.whole_word = self.whole_word;
        options.regex = self.regex;
        (options, self.scope)
    }

    pub(super) fn open(
        &mut self,
        replace_mode: bool,
        selected_text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.visible = true;
        self.replace_mode = replace_mode;
        self.error = None;
        self.result_message = None;
        if !selected_text.is_empty() && self.find.read(cx).value().is_empty() {
            self.find
                .update(cx, |state, cx| state.set_value(selected_text, window, cx));
        }
        if replace_mode {
            self.replace.update(cx, |state, cx| state.focus(window, cx));
        } else {
            self.find.update(cx, |state, cx| state.focus(window, cx));
        }
        self.submit(SearchAction::Update, cx);
        cx.notify();
    }

    pub(super) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.hide();
        self.find_in_selection = false;
        self.scope = None;
        if let Some(parent) = self.parent.upgrade() {
            parent.update(cx, |view, cx| {
                search::cancel(&mut view.search_task);
                search::clear_all(view, cx);
                if let Some(state) = view.active_tab().and_then(|tab| tab.ui.clone()) {
                    state.update(cx, |state, cx| state.focus(window, cx));
                }
            });
        }
        cx.notify();
    }

    pub(super) fn hide(&mut self) {
        self.visible = false;
        self.submission_generation = self.submission_generation.wrapping_add(1);
    }

    pub(super) fn set_results(
        &mut self,
        document_id: u64,
        revision: Revision,
        matches: Vec<SearchMatch>,
        current: Option<usize>,
        error: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.matches = matches;
        self.current = current;
        self.matches_document = Some(document_id);
        self.matches_revision = Some(revision);
        self.error = error;
        cx.notify();
    }

    pub(super) fn set_result_message(&mut self, message: String, cx: &mut Context<Self>) {
        self.result_message = Some(message);
        cx.notify();
    }

    pub(super) fn navigate_next(&mut self, cx: &mut Context<Self>) {
        self.navigate(false, cx);
    }

    pub(super) fn navigate_previous(&mut self, cx: &mut Context<Self>) {
        self.navigate(true, cx);
    }

    fn navigate(&mut self, previous: bool, cx: &mut Context<Self>) {
        let mut options = SearchOptions::literal(self.find.read(cx).value().to_string());
        options.case_sensitive = self.match_case;
        options.whole_word = self.whole_word;
        options.regex = self.regex;
        let scope = self.scope;
        let action = if previous {
            SearchAction::Previous
        } else {
            SearchAction::Next
        };
        self.submission_generation = self.submission_generation.wrapping_add(1);
        let generation = self.submission_generation;
        let matches = self.matches.clone();
        let matches_document = self.matches_document;
        let matches_revision = self.matches_revision;
        self.error = None;
        self.result_message = None;
        let parent = self.parent.clone();
        cx.spawn(async move |_, cx| {
            let Some(parent) = parent.upgrade() else {
                return;
            };
            parent.update(cx, |view, cx| {
                let Some(widget) = view.find_widget.clone() else {
                    return;
                };
                let stale_widget = {
                    let state = widget.read(cx);
                    !state.visible || state.submission_generation != generation
                };
                if stale_widget {
                    return;
                }
                let active = view.active_tab().and_then(|tab| {
                    Some((tab.id, tab.editor.as_ref()?.revision(), tab.ui.clone()?))
                });
                let Some((document_id, revision, state)) = active else {
                    return;
                };
                let selected = state.read(cx).selected_range();
                let cache_is_current = {
                    let widget_state = widget.read(cx);
                    widget_state.matches_document == matches_document
                        && widget_state.matches_revision == matches_revision
                        && widget_state.matches == matches
                };
                let cached = matches_document == Some(document_id)
                    && matches_revision == Some(revision)
                    && cache_is_current
                    && !matches.is_empty();
                if cached {
                    let index = if previous {
                        matches
                            .iter()
                            .rposition(|item| item.range.end.0 <= selected.start)
                            .unwrap_or(matches.len() - 1)
                    } else {
                        matches
                            .iter()
                            .position(|item| item.range.start.0 >= selected.end)
                            .unwrap_or(0)
                    };
                    let matched = matches[index];
                    search::cancel(&mut view.search_task);
                    state.update(cx, |state, cx| {
                        state.set_selected_range(matched.range.start.0..matched.range.end.0, cx)
                    });
                    search::highlight_current(view, document_id, matched, cx);
                    widget.update(cx, |widget, cx| {
                        widget.current = Some(index);
                        widget.result_message = None;
                        cx.notify();
                    });
                } else {
                    search::start(view, options, scope, action, cx);
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn toggle_case(&mut self, cx: &mut Context<Self>) {
        self.match_case = !self.match_case;
        self.submit(SearchAction::Update, cx);
    }

    pub(super) fn toggle_whole_word(&mut self, cx: &mut Context<Self>) {
        self.whole_word = !self.whole_word;
        self.submit(SearchAction::Update, cx);
    }

    pub(super) fn toggle_regex(&mut self, cx: &mut Context<Self>) {
        self.regex = !self.regex;
        self.submit(SearchAction::Update, cx);
    }

    pub(super) fn toggle_find_in_selection(
        &mut self,
        selected: Option<ByteRange>,
        cx: &mut Context<Self>,
    ) {
        if self.find_in_selection {
            self.find_in_selection = false;
            self.scope = None;
        } else if let Some(scope) = selected {
            self.find_in_selection = true;
            self.scope = Some(scope);
        } else {
            self.result_message = Some("Select text to search within it".into());
            cx.notify();
            return;
        }
        self.submit(SearchAction::Update, cx);
    }

    pub(super) fn add_next_match_to_selection(&mut self, cx: &mut Context<Self>) {
        if self.matches.is_empty() {
            return;
        }
        let next = self
            .current
            .map_or(0, |index| (index + 1) % self.matches.len());
        let matched = self.matches[next];
        let document_id = self.matches_document;
        let revision = self.matches_revision;
        let generation = self.submission_generation;
        let parent = self.parent.clone();
        cx.spawn(async move |_, cx| {
            let Some(parent) = parent.upgrade() else {
                return;
            };
            parent.update(cx, |view, cx| {
                let Some(widget) = view.find_widget.clone() else {
                    return;
                };
                let valid_widget_cache = {
                    let widget = widget.read(cx);
                    widget.visible
                        && widget.submission_generation == generation
                        && widget.matches_document == document_id
                        && widget.matches_revision == revision
                        && widget.matches.contains(&matched)
                };
                if !valid_widget_cache {
                    return;
                }
                let Some(tab) = view.active_tab() else { return };
                if Some(tab.id) != document_id
                    || tab.editor.as_ref().map(|editor| editor.revision()) != revision
                {
                    return;
                }
                let Some(state) = tab.ui.clone() else { return };
                let active_document = tab.id;
                state.update(cx, |state, cx| {
                    state.add_selection(matched.range.start.0..matched.range.end.0, cx)
                });
                search::highlight_current(view, active_document, matched, cx);
                widget.update(cx, |widget, cx| {
                    widget.current = Some(next);
                    widget.result_message = None;
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn active_selection(&self, cx: &Context<Self>) -> Option<ByteRange> {
        let parent = self.parent.upgrade()?;
        let tab = parent.read(cx).active_tab()?;
        let state = tab.ui.as_ref()?.read(cx);
        let range = state.selected_range();
        (range.start < range.end).then_some(ByteRange::new(range.start, range.end))
    }

    fn submit(&mut self, action: SearchAction, cx: &mut Context<Self>) {
        let is_update = matches!(&action, SearchAction::Update);
        if is_update {
            self.matches.clear();
            self.current = None;
            self.matches_document = None;
            self.matches_revision = None;
        }
        self.error = None;
        self.result_message = None;
        let mut options = SearchOptions::literal(self.find.read(cx).value().to_string());
        options.case_sensitive = self.match_case;
        options.whole_word = self.whole_word;
        options.regex = self.regex;
        self.submission_generation = self.submission_generation.wrapping_add(1);
        let generation = self.submission_generation;
        let scope = self.scope;
        let parent = self.parent.clone();
        // The parent can be leased while an action opens Find; defer the update to avoid re-entry.
        cx.spawn(async move |_, cx| {
            let Some(parent) = parent.upgrade() else {
                return;
            };
            parent.update(cx, |view, cx| {
                let Some(widget) = view.find_widget.as_ref() else {
                    return;
                };
                let current = {
                    let widget = widget.read(cx);
                    widget.visible && widget.submission_generation == generation
                };
                if current {
                    search::start(view, options, scope, action, cx);
                }
            });
        })
        .detach();
        cx.notify();
    }
}
