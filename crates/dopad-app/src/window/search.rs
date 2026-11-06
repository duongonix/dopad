use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use dopad_core::{ByteRange, Revision};
use dopad_editor::{
    SearchError, SearchMatch, SearchOptions, SearchResult, expand_replacement,
    find_matches_in_range, replace_matches_text,
};
use gpui_kit::component::input::{RangeDecoration, RangeDecorationStyle};
use gpui_kit::{Context, Window, rgb};

use super::DopadWindow;

#[derive(Clone)]
pub(super) enum SearchAction {
    Update,
    Next,
    Previous,
    AddNextSelection,
    ReplaceCurrent(String),
    ReplaceAll(String),
}

#[derive(Default)]
pub(super) struct SearchTaskState {
    generation: u64,
    pub(super) pending: Option<(u64, Revision, u64)>,
    cancelled: Option<Arc<AtomicBool>>,
}

struct SearchCompletion {
    document_id: u64,
    revision: Revision,
    generation: u64,
    action: SearchAction,
    result: Result<(SearchResult, Option<ReplaceOutput>), SearchError>,
}

enum ReplaceOutput {
    One {
        matched: SearchMatch,
        replacement: String,
    },
    All {
        text: String,
        range: ByteRange,
        replacement: String,
    },
}

pub(super) fn start(
    view: &mut DopadWindow,
    options: SearchOptions,
    scope: Option<ByteRange>,
    action: SearchAction,
    cx: &mut Context<DopadWindow>,
) {
    let Some(tab) = view.active_tab() else {
        return;
    };
    let Some(editor) = tab.editor.as_ref() else {
        view.set_status("Search is available for text files.");
        cx.notify();
        return;
    };
    let document_id = tab.id;
    let snapshot = editor.snapshot();
    let selected_range = tab
        .ui
        .as_ref()
        .map(|state| state.read(cx).selected_range())
        .unwrap_or_default();
    let revision = snapshot.revision();
    cancel(&mut view.search_task);
    view.search_task.generation = view.search_task.generation.saturating_add(1);
    let generation = view.search_task.generation;
    let cancelled = Arc::new(AtomicBool::new(false));
    view.search_task.cancelled = Some(cancelled.clone());
    view.search_task.pending = Some((document_id, revision, generation));
    if matches!(&action, SearchAction::Update) {
        clear_search_decorations(view, document_id, cx);
    }

    let worker_options = options.clone();
    let worker_action = action.clone();
    cx.spawn(async move |this, cx| {
        if matches!(&worker_action, SearchAction::Update) {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(45))
                .await;
            if cancelled.load(Ordering::Relaxed) {
                return;
            }
        }
        let search_cancelled = cancelled.clone();
        let result = cx
            .background_executor()
            .spawn(async move {
                let found =
                    find_matches_in_range(&snapshot, &worker_options, scope, &search_cancelled)?;
                let source = match &worker_action {
                    SearchAction::ReplaceCurrent(_) | SearchAction::ReplaceAll(_) => {
                        Some(snapshot.to_string())
                    }
                    _ => None,
                };
                let replacement = match (&worker_action, source.as_deref()) {
                    (SearchAction::ReplaceCurrent(value), Some(source))
                        if !found.matches.is_empty() =>
                    {
                        let index = found
                            .matches
                            .iter()
                            .position(|item| {
                                item.range.start.0 == selected_range.start
                                    && item.range.end.0 == selected_range.end
                            })
                            .unwrap_or_else(|| next_match(&found.matches, selected_range.end));
                        let matched = found.matches[index];
                        Some(ReplaceOutput::One {
                            matched,
                            replacement: expand_replacement(
                                source,
                                matched,
                                &worker_options,
                                value,
                            )?,
                        })
                    }
                    (SearchAction::ReplaceAll(value), Some(source)) => {
                        let text =
                            replace_matches_text(source, &found.matches, &worker_options, value)?;
                        let (range, replacement) =
                            super::sync::minimal_diff_with_old_text(&snapshot, source, &text);
                        Some(ReplaceOutput::All {
                            text,
                            range,
                            replacement,
                        })
                    }
                    _ => None,
                };
                Ok((found, replacement))
            })
            .await;
        let _ = this.update_in(cx, |window, app_window, cx| {
            finish(
                window,
                SearchCompletion {
                    document_id,
                    revision,
                    generation,
                    action,
                    result,
                },
                app_window,
                cx,
            );
        });
    })
    .detach();
}

pub(super) fn cancel(task: &mut SearchTaskState) {
    if let Some(cancelled) = task.cancelled.take() {
        cancelled.store(true, Ordering::Relaxed);
    }
    task.pending = None;
}

fn finish(
    window: &mut DopadWindow,
    completion: SearchCompletion,
    app_window: &mut Window,
    cx: &mut Context<DopadWindow>,
) {
    let SearchCompletion {
        document_id,
        revision,
        generation,
        action,
        result,
    } = completion;
    if window.search_task.pending != Some((document_id, revision, generation))
        || window.active_tab().is_none_or(|tab| {
            tab.id != document_id
                || tab
                    .editor
                    .as_ref()
                    .is_none_or(|editor| editor.revision() != revision)
        })
    {
        return;
    }
    window.search_task.pending = None;
    window.search_task.cancelled = None;

    let (result, replaced_text) = match result {
        Ok(result) => result,
        Err(SearchError::Cancelled) => return,
        Err(error) => {
            clear_search_decorations(window, document_id, cx);
            if let Some(widget) = window.find_widget.as_ref() {
                widget.update(cx, |widget, cx| {
                    widget.set_results(
                        document_id,
                        revision,
                        Vec::new(),
                        None,
                        Some(error.to_string()),
                        cx,
                    );
                });
            }
            cx.notify();
            return;
        }
    };

    let Some(state) = window
        .tabs
        .iter()
        .find(|tab| tab.id == document_id)
        .and_then(|tab| tab.ui.clone())
    else {
        return;
    };
    let decorations = result
        .matches
        .iter()
        .map(|matched| {
            RangeDecoration::new(matched.range.start.0..matched.range.end.0)
                .with_style(RangeDecorationStyle::Fill)
                .with_color(rgb(crate::theme::palette(window.dark_theme).selection).into())
        })
        .collect();
    if let Some(collection) = window
        .tabs
        .iter()
        .find(|tab| tab.id == document_id)
        .and_then(|tab| tab.search_decorations.as_ref())
    {
        collection.set(decorations, cx);
    }

    if result.matches.is_empty() {
        if let Some(widget) = window.find_widget.as_ref() {
            widget.update(cx, |widget, cx| {
                widget.set_results(
                    document_id,
                    revision,
                    result.matches.clone(),
                    None,
                    None,
                    cx,
                );
            });
        }
        cx.notify();
        return;
    }

    let current_index = match action {
        SearchAction::Update
        | SearchAction::Next
        | SearchAction::Previous
        | SearchAction::AddNextSelection => {
            let selected = state.read(cx).selected_range();
            let index = match action {
                SearchAction::Previous => previous_match(&result.matches, selected.start),
                SearchAction::AddNextSelection => next_match(&result.matches, selected.end),
                SearchAction::Update => result
                    .matches
                    .iter()
                    .position(|item| {
                        item.range.start.0 == selected.start && item.range.end.0 == selected.end
                    })
                    .unwrap_or_else(|| next_match(&result.matches, selected.start)),
                _ => next_match(&result.matches, selected.end),
            };
            let matched = result.matches[index].range;
            state.update(cx, |state, cx| match action {
                SearchAction::AddNextSelection => {
                    state.add_selection(matched.start.0..matched.end.0, cx)
                }
                _ => state.set_selected_range(matched.start.0..matched.end.0, cx),
            });
            if let Some(collection) = window
                .tabs
                .iter()
                .find(|tab| tab.id == document_id)
                .and_then(|tab| tab.current_search_decorations.as_ref())
            {
                collection.set(
                    vec![
                        RangeDecoration::new(matched.start.0..matched.end.0)
                            .with_style(RangeDecorationStyle::Fill)
                            .with_color(
                                rgb(crate::theme::palette(window.dark_theme).search_current).into(),
                            ),
                    ],
                    cx,
                );
            }
            Some(index)
        }
        SearchAction::ReplaceCurrent(_) => {
            let Some(ReplaceOutput::One {
                matched,
                replacement,
            }) = replaced_text
            else {
                return;
            };
            clear_search_decorations(window, document_id, cx);
            state.update(cx, |state, cx| {
                state.set_selected_range(matched.range.start.0..matched.range.end.0, cx);
                state.replace(replacement, app_window, cx);
            });
            if let Some(widget) = window.find_widget.as_ref() {
                widget.update(cx, |widget, cx| {
                    widget.set_result_message("Replaced 1 occurrence".into(), cx);
                });
            }
            None
        }
        SearchAction::ReplaceAll(_) => {
            let Some(ReplaceOutput::All {
                text,
                range,
                replacement,
            }) = replaced_text
            else {
                return;
            };
            let count = result.matches.len();
            let edit = window
                .tabs
                .iter_mut()
                .find(|tab| tab.id == document_id)
                .and_then(|tab| {
                    Some(
                        tab.editor
                            .as_mut()?
                            .replace_range(range, &replacement)
                            .inspect(|edit| {
                                if edit.changed {
                                    tab.eol = dopad_core::detect_eol(&text);
                                }
                            }),
                    )
                });
            let changed = match edit {
                Some(Ok(edit)) => edit.changed,
                Some(Err(error)) => {
                    if let Some(widget) = window.find_widget.as_ref() {
                        widget.update(cx, |widget, cx| {
                            widget.set_result_message(format!("Replace All failed: {error}"), cx);
                        });
                    }
                    cx.notify();
                    return;
                }
                None => return,
            };
            clear_search_decorations(window, document_id, cx);
            if changed {
                window.suppress_ui_sync = Some(document_id);
                state.update(cx, |state, cx| state.replace_all(text, app_window, cx));
                window.search_task.generation = window.search_task.generation.saturating_add(1);
                if let Some(widget) = window.find_widget.as_ref() {
                    widget.update(cx, |widget, cx| {
                        widget.set_result_message(format!("Replaced {count} occurrences"), cx);
                    });
                }
                let request = window
                    .find_widget
                    .as_ref()
                    .filter(|widget| widget.read(cx).is_visible())
                    .map(|widget| widget.read(cx).search_request(cx));
                if let Some((options, scope)) = request {
                    start(window, options, scope, SearchAction::Update, cx);
                }
            } else {
                if let Some(widget) = window.find_widget.as_ref() {
                    widget.update(cx, |widget, cx| {
                        widget.set_result_message("Replacement text is unchanged".into(), cx);
                    });
                }
            }
            None
        }
    };
    if let Some(widget) = window.find_widget.as_ref() {
        widget.update(cx, |widget, cx| {
            widget.set_results(
                document_id,
                revision,
                result.matches.clone(),
                current_index,
                None,
                cx,
            );
        });
    }
    cx.notify();
}

fn next_match(matches: &[SearchMatch], from: usize) -> usize {
    matches
        .iter()
        .position(|item| item.range.start.0 >= from)
        .unwrap_or(0)
}

fn previous_match(matches: &[SearchMatch], from: usize) -> usize {
    matches
        .iter()
        .rposition(|item| item.range.end.0 <= from)
        .unwrap_or(matches.len() - 1)
}

fn clear_search_decorations(window: &DopadWindow, document_id: u64, cx: &mut Context<DopadWindow>) {
    if let Some(tab) = window.tabs.iter().find(|tab| tab.id == document_id) {
        if let Some(collection) = tab.search_decorations.as_ref() {
            collection.clear(cx);
        }
        if let Some(collection) = tab.current_search_decorations.as_ref() {
            collection.clear(cx);
        }
    }
}

pub(super) fn clear_all(window: &DopadWindow, cx: &mut Context<DopadWindow>) {
    for tab in &window.tabs {
        if let Some(collection) = tab.search_decorations.as_ref() {
            collection.clear(cx);
        }
        if let Some(collection) = tab.current_search_decorations.as_ref() {
            collection.clear(cx);
        }
    }
}

pub(super) fn highlight_current(
    window: &DopadWindow,
    document_id: u64,
    matched: SearchMatch,
    cx: &mut Context<DopadWindow>,
) {
    if let Some(collection) = window
        .tabs
        .iter()
        .find(|tab| tab.id == document_id)
        .and_then(|tab| tab.current_search_decorations.as_ref())
    {
        collection.set(
            vec![
                RangeDecoration::new(matched.range.start.0..matched.range.end.0)
                    .with_style(RangeDecorationStyle::Fill)
                    .with_color(
                        rgb(crate::theme::palette(window.dark_theme).search_current).into(),
                    ),
            ],
            cx,
        );
    }
}
