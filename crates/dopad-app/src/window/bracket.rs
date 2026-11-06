use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use dopad_core::Revision;
use gpui_kit::component::input::{
    EditorState, RangeDecoration, RangeDecorationStyle, Rope, RopeExt,
};
use gpui_kit::{Context, Entity, rgb};

use super::DopadWindow;

#[derive(Default)]
pub(super) struct BracketTaskState {
    generation: u64,
    pending: Option<(u64, Revision, u64)>,
    cancelled: Option<Arc<AtomicBool>>,
}

pub(super) fn request(
    window: &mut DopadWindow,
    document_id: u64,
    state: &Entity<EditorState>,
    selection: std::ops::Range<usize>,
    cx: &mut Context<DopadWindow>,
) {
    cancel(&mut window.bracket_task);
    let Some(tab) = window.tabs.iter().find(|tab| tab.id == document_id) else {
        return;
    };
    let Some(editor) = tab.editor.as_ref() else {
        return;
    };
    if selection.start != selection.end {
        clear(window, document_id, cx);
        return;
    }
    let rope = state.read(cx).text().clone();
    let cursor = selection.end;
    let Some(target) = adjacent_bracket(&rope, cursor) else {
        clear(window, document_id, cx);
        return;
    };
    let _ = target;
    let revision = editor.revision();
    let path = tab.path.clone();
    window.bracket_task.generation = window.bracket_task.generation.saturating_add(1);
    let generation = window.bracket_task.generation;
    let cancelled = Arc::new(AtomicBool::new(false));
    window.bracket_task.cancelled = Some(cancelled.clone());
    window.bracket_task.pending = Some((document_id, revision, generation));

    cx.spawn(async move |this, cx| {
        let range = cx
            .background_executor()
            .spawn(async move {
                let text = rope.to_string();
                if cancelled.load(Ordering::Relaxed) {
                    return None;
                }
                let prefix_end = text.floor_char_boundary(256.min(text.len()));
                let language = dopad_syntax::detect_language(path.as_deref(), &text[..prefix_end]);
                dopad_syntax::matching_bracket_with_cancel(&text, cursor, language, &cancelled)
            })
            .await;
        let _ = this.update_in(cx, |window, _, cx| {
            if window.bracket_task.pending != Some((document_id, revision, generation))
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
            window.bracket_task.pending = None;
            window.bracket_task.cancelled = None;
            let decorations = range
                .filter(|range| range.end > range.start + 1)
                .map(|range| {
                    vec![
                        RangeDecoration::new(range.start..range.start + 1)
                            .with_style(RangeDecorationStyle::Frame)
                            .with_color(rgb(0xb65f32).into()),
                        RangeDecoration::new(range.end - 1..range.end)
                            .with_style(RangeDecorationStyle::Frame)
                            .with_color(rgb(0xb65f32).into()),
                    ]
                })
                .unwrap_or_default();
            if let Some(collection) = window
                .tabs
                .iter()
                .find(|tab| tab.id == document_id)
                .and_then(|tab| tab.bracket_decorations.as_ref())
            {
                collection.set(decorations, cx);
            }
        });
    })
    .detach();
}

pub(super) fn cancel(task: &mut BracketTaskState) {
    if let Some(cancelled) = task.cancelled.take() {
        cancelled.store(true, Ordering::Relaxed);
    }
    task.pending = None;
    task.generation = task.generation.saturating_add(1);
}

fn clear(window: &DopadWindow, document_id: u64, cx: &mut Context<DopadWindow>) {
    if let Some(collection) = window
        .tabs
        .iter()
        .find(|tab| tab.id == document_id)
        .and_then(|tab| tab.bracket_decorations.as_ref())
    {
        collection.clear(cx);
    }
}

fn adjacent_bracket(rope: &Rope, cursor: usize) -> Option<usize> {
    if let Some(current) = rope.char_at(cursor)
        && matches!(current, '(' | ')' | '[' | ']' | '{' | '}')
    {
        return Some(cursor);
    }
    let previous = cursor.checked_sub(1)?;
    rope.char_at(previous)
        .filter(|ch| matches!(ch, '(' | ')' | '[' | ']' | '{' | '}'))
        .map(|_| previous)
}
