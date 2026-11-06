use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use dopad_core::Revision;
use dopad_preview::{
    MarkdownBlock, MarkdownDocument, PreviewContent, PreviewError, PreviewResult, PreviewWarning,
    render_markdown, render_svg,
};
use gpui_kit::Context;

use super::DopadWindow;

const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(140);

#[cfg(test)]
fn supports_preview(path: Option<&Path>) -> bool {
    preview_kind(path, None).is_some()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreviewKind {
    Markdown,
    Svg,
}

fn preview_kind(path: Option<&Path>, title: Option<&str>) -> Option<PreviewKind> {
    path.or_else(|| title.map(Path::new))
        .and_then(Path::extension)
        .and_then(|extension| extension.to_str())
        .and_then(|extension| match extension.to_ascii_lowercase().as_str() {
            "md" | "markdown" => Some(PreviewKind::Markdown),
            "svg" => Some(PreviewKind::Svg),
            _ => None,
        })
}

pub(super) fn supports_tab_preview(tab: &super::DocumentTab) -> bool {
    matches!(tab.content, super::file_view::TabContent::Text)
        && preview_kind(tab.path.as_deref(), Some(&tab.title)).is_some()
}

pub(super) fn activate(window: &mut DopadWindow, index: usize) {
    let next_id = window.tabs.get(index).map(|tab| tab.id);
    if window
        .preview_pending
        .is_some_and(|(pending_id, _, _)| Some(pending_id) != next_id)
    {
        window.cancel_preview_task();
    }
    window.active = index;
    if let Some(tab) = window.tabs.get_mut(index)
        && tab.preview_mode
        && !supports_tab_preview(tab)
    {
        tab.preview_mode = false;
    }
}

pub(super) fn toggle_preview(window: &mut DopadWindow, cx: &mut Context<DopadWindow>) {
    let Some(tab) = window.tabs.get_mut(window.active) else {
        return;
    };
    if !supports_tab_preview(tab) {
        window.set_status("Preview is available for Markdown and SVG files.");
        cx.notify();
        return;
    }
    tab.preview_mode = !tab.preview_mode;
    let enabled = tab.preview_mode;
    window.clear_status();
    if enabled {
        window.request_preview(cx);
    } else {
        window.cancel_preview_task();
        if !window.settings.current().preview_cache_enabled {
            window.preview_cache = None;
            window.reset_preview_list();
        }
    }
    cx.notify();
}

impl DopadWindow {
    pub(super) fn ensure_preview(&mut self, cx: &mut Context<Self>) {
        if self
            .active_tab()
            .is_some_and(|tab| tab.preview_mode && !supports_tab_preview(tab))
        {
            if let Some(tab) = self.tabs.get_mut(self.active) {
                tab.preview_mode = false;
            }
            self.cancel_preview_task();
        }
        let Some(tab) = self.active_tab() else { return };
        if !matches!(tab.content, super::file_view::TabContent::Text)
            || !tab.preview_mode
            || !supports_tab_preview(tab)
        {
            return;
        }
        let id = tab.id;
        let Some(editor) = tab.editor.as_ref() else {
            return;
        };
        let revision = editor.revision();
        if self
            .preview_cache
            .as_ref()
            .is_some_and(|result| result.document_id == id && result.revision == revision)
            || self
                .preview_error
                .as_ref()
                .is_some_and(|(error_id, error_revision, _)| {
                    *error_id == id && *error_revision == revision
                })
            || self.preview_pending == Some((id, revision, self.preview_generation))
        {
            return;
        }
        self.request_preview(cx);
    }

    pub(super) fn request_preview(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else { return };
        if !supports_tab_preview(tab) {
            return;
        }
        let Some(ui) = tab.ui.as_ref() else { return };
        if !tab.preview_mode {
            return;
        }
        let id = tab.id;
        let Some(editor) = tab.editor.as_ref() else {
            return;
        };
        let revision = editor.revision();
        let svg = preview_kind(tab.path.as_deref(), Some(&tab.title)) == Some(PreviewKind::Svg);
        let rope = ui.read(cx).text().clone();

        self.cancel_preview_task();
        self.preview_generation = self.preview_generation.saturating_add(1);
        let generation = self.preview_generation;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.preview_cancel = Some(cancelled.clone());
        self.preview_pending = Some((id, revision, generation));
        self.preview_error = None;

        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PREVIEW_DEBOUNCE).await;
            if cancelled.load(Ordering::Relaxed) {
                return;
            }

            let parse_cancelled = cancelled.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let text = rope.to_string();
                    if parse_cancelled.load(Ordering::Relaxed) {
                        return Err(PreviewError::Cancelled);
                    }
                    if svg {
                        render_svg(id, revision, &text, &parse_cancelled)
                    } else {
                        render_markdown(id, revision, &text, &parse_cancelled)
                    }
                })
                .await;
            let _ = this.update(cx, |window, cx| {
                window.finish_preview(id, revision, generation, result, cx);
            });
        })
        .detach();
    }

    pub(super) fn invalidate_preview_for(&mut self, id: u64, cx: &mut Context<Self>) {
        if self
            .preview_error
            .as_ref()
            .is_some_and(|(error_id, _, _)| *error_id == id)
        {
            self.preview_error = None;
        }
        if self
            .active_tab()
            .is_some_and(|tab| tab.id == id && tab.preview_mode)
        {
            self.request_preview(cx);
        }
    }

    pub(super) fn document_path_changed(&mut self, id: u64, cx: &mut Context<Self>) {
        let supports_preview = self
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .is_some_and(supports_tab_preview);
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
            && !supports_preview
        {
            tab.preview_mode = false;
        }
        if self
            .preview_pending
            .is_some_and(|(pending_id, _, _)| pending_id == id)
        {
            self.cancel_preview_task();
        }
        if self
            .preview_cache
            .as_ref()
            .is_some_and(|result| result.document_id == id)
        {
            self.preview_cache = None;
            self.reset_preview_list();
        }
        if self
            .preview_error
            .as_ref()
            .is_some_and(|(error_id, _, _)| *error_id == id)
        {
            self.preview_error = None;
        }
        if self
            .active_tab()
            .is_some_and(|tab| tab.id == id && tab.preview_mode)
        {
            self.request_preview(cx);
        }
        cx.notify();
    }

    pub(super) fn cancel_preview_task(&mut self) {
        if let Some(token) = self.preview_cancel.take() {
            token.store(true, Ordering::Relaxed);
        }
        self.preview_pending = None;
        self.preview_generation = self.preview_generation.saturating_add(1);
    }

    fn finish_preview(
        &mut self,
        id: u64,
        revision: Revision,
        generation: u64,
        result: Result<PreviewResult, PreviewError>,
        cx: &mut Context<Self>,
    ) {
        if self.preview_pending != Some((id, revision, generation))
            || self.active_tab().is_none_or(|tab| {
                tab.id != id
                    || !tab.preview_mode
                    || tab
                        .editor
                        .as_ref()
                        .is_none_or(|editor| editor.revision() != revision)
            })
        {
            return;
        }
        self.preview_pending = None;
        self.preview_cancel = None;
        match result {
            Ok(result) if result.document_id == id && result.revision == revision => {
                self.preview_error = None;
                let previous = self.preview_cache.as_ref().map(|cached| &cached.content);
                self.update_preview_list(previous, &result.content);
                self.preview_cache = Some(result);
            }
            Ok(_) => return,
            Err(PreviewError::Cancelled) => {}
            Err(error) => {
                self.preview_error = Some((id, revision, error.to_string()));
                self.preview_cache = None;
                self.reset_preview_list();
            }
        }
        cx.notify();
    }

    fn update_preview_list(&self, previous: Option<&PreviewContent>, next: &PreviewContent) {
        let current = self.preview_list.item_count();
        let previous = previous.and_then(markdown_document);
        let next = markdown_document(next);
        let (Some(previous), Some(next)) = (previous, next) else {
            self.preview_list
                .splice(0..current, next.map_or(0, markdown_item_count));
            return;
        };

        let previous_count = markdown_item_count(previous);
        let next_count = markdown_item_count(next);
        if current != previous_count {
            self.preview_list.splice(0..current, next_count);
            return;
        }

        let common_limit = previous_count.min(next_count);
        let mut prefix = 0;
        while prefix < common_limit && markdown_items_equal(previous, prefix, next, prefix) {
            prefix += 1;
        }
        let mut suffix = 0;
        while suffix < previous_count.saturating_sub(prefix)
            && suffix < next_count.saturating_sub(prefix)
            && markdown_items_equal(
                previous,
                previous_count - suffix - 1,
                next,
                next_count - suffix - 1,
            )
        {
            suffix += 1;
        }

        let previous_end = previous_count - suffix;
        let next_end = next_count - suffix;
        if previous_count == next_count {
            if prefix < next_end {
                self.preview_list.remeasure_items(prefix..next_end);
            }
        } else {
            self.preview_list
                .splice(prefix..previous_end, next_end - prefix);
        }
    }

    pub(super) fn reset_preview_list(&self) {
        let current = self.preview_list.item_count();
        if current > 0 {
            self.preview_list.splice(0..current, 0);
        }
    }
}

fn markdown_document(content: &PreviewContent) -> Option<&MarkdownDocument> {
    match content {
        PreviewContent::Markdown(document) => Some(document),
        PreviewContent::Svg(_) => None,
    }
}

fn markdown_item_count(document: &MarkdownDocument) -> usize {
    document
        .warnings
        .len()
        .saturating_add(document.blocks.len())
}

fn markdown_items_equal(
    left: &MarkdownDocument,
    left_index: usize,
    right: &MarkdownDocument,
    right_index: usize,
) -> bool {
    match (
        markdown_item(left, left_index),
        markdown_item(right, right_index),
    ) {
        (Some(MarkdownItem::Warning(left)), Some(MarkdownItem::Warning(right))) => left == right,
        (Some(MarkdownItem::Block(left)), Some(MarkdownItem::Block(right))) => left == right,
        _ => false,
    }
}

enum MarkdownItem<'a> {
    Warning(PreviewWarning),
    Block(&'a MarkdownBlock),
}

fn markdown_item(document: &MarkdownDocument, index: usize) -> Option<MarkdownItem<'_>> {
    if let Some(warning) = document.warnings.get(index) {
        return Some(MarkdownItem::Warning(*warning));
    }
    document
        .blocks
        .get(index.saturating_sub(document.warnings.len()))
        .map(MarkdownItem::Block)
}

#[cfg(test)]
mod tests {
    use super::{PreviewKind, preview_kind, supports_preview, supports_tab_preview};
    use crate::window::DocumentTab;
    use std::path::Path;

    #[test]
    fn preview_capability_is_limited_to_markdown_and_svg_extensions() {
        for name in ["readme.md", "notes.MARKDOWN", "image.SvG"] {
            assert!(supports_preview(Some(Path::new(name))), "{name}");
        }
        for name in ["main.rs", "readme.txt", "diagram.svgz", "archive.md.zip"] {
            assert!(!supports_preview(Some(Path::new(name))), "{name}");
        }
        assert_eq!(
            preview_kind(None, Some("Untitled.md")),
            Some(PreviewKind::Markdown)
        );

        let mut tab = DocumentTab::blank(1);
        tab.title = "Untitled.md".into();
        assert!(supports_tab_preview(&tab));
        tab.title = "source.rs".into();
        assert!(!supports_tab_preview(&tab));
        tab.title = "diagram.svg".into();
        assert!(supports_tab_preview(&tab));
    }
}
