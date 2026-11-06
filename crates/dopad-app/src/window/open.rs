use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use dopad_core::TextPosition;
use gpui_kit::Context;
use rfd::AsyncFileDialog;

use super::{DocumentTab, DopadWindow, file_view, preview};
use crate::settings::save_file;

const LOADING_INDICATOR_DELAY: Duration = Duration::from_millis(120);

impl DopadWindow {
    pub(super) fn open_dialog(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let selected = AsyncFileDialog::new()
                .set_title("Open file")
                .pick_files()
                .await;
            let Some(files) = selected else { return };
            let paths = files
                .into_iter()
                .map(|file| file.path().to_path_buf())
                .collect();
            let _ = this.update(cx, |view, cx| view.open_paths(paths, cx));
        })
        .detach();
    }

    pub(super) fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let _ = self.open_paths_internal(paths, false, cx);
    }

    pub(super) fn open_startup_paths(&mut self, cx: &mut Context<Self>) {
        let mut paths = std::mem::take(&mut self.pending_startup_paths);
        if paths.is_empty() {
            let settings = self.settings.current();
            if settings.show_recent_files && !settings.restore_session {
                paths.extend(
                    settings
                        .recent_files
                        .into_iter()
                        .take(usize::from(settings.recent_file_limit)),
                );
            }
        }
        let _ = self.open_paths_internal(paths, true, cx);
        self.flush_pending_cli_requests(cx);
    }

    pub(super) fn open_paths_internal(
        &mut self,
        paths: Vec<PathBuf>,
        mut replace_empty_tab: bool,
        cx: &mut Context<Self>,
    ) -> Vec<u64> {
        let mut opened_ids = Vec::with_capacity(paths.len());
        for path in paths {
            if let Some(index) = existing_tab_index(&self.tabs, &path) {
                opened_ids.push(self.tabs[index].id);
                preview::activate(self, index);
                continue;
            }

            if replace_empty_tab && self.tabs.len() == 1 && self.tabs[0].is_replaceable_blank() {
                self.tabs.clear();
                self.active = 0;
            }
            replace_empty_tab = false;

            let id = self.next_id;
            let mut tab = DocumentTab::loading(id, path.clone());
            super::apply_document_defaults(&mut tab, &self.settings.current());
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
            self.next_id = self.next_id.saturating_add(1);
            opened_ids.push(id);
            self.start_file_load(id, path, cx);
        }
        self.clear_status();
        cx.notify();
        opened_ids
    }

    pub(super) fn start_file_load(
        &mut self,
        document_id: u64,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if !self.pending_file_loads.insert(document_id) {
            return;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        self.pending_loading_indicators
            .insert(document_id, cancelled.clone());
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == document_id) {
            tab.loading_indicator_visible = false;
        }
        let indicator_cancelled = cancelled.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(LOADING_INDICATOR_DELAY)
                .await;
            if indicator_cancelled.load(Ordering::Acquire) {
                return;
            }
            let _ = this.update(cx, |window, cx| {
                if indicator_cancelled.load(Ordering::Acquire)
                    || !window.pending_file_loads.contains(&document_id)
                {
                    return;
                }
                let Some(tab) = window.tabs.iter_mut().find(|tab| tab.id == document_id) else {
                    return;
                };
                if matches!(tab.content, file_view::TabContent::Loading) {
                    tab.loading_indicator_visible = true;
                    cx.notify();
                }
            });
        })
        .detach();

        let settings = self.settings.current();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    file_view::load_file(path, settings.detect_encoding, settings.default_encoding)
                })
                .await;
            let _ = this.update(cx, |window, cx| {
                window.finish_file_load(document_id, result, cx);
            });
        })
        .detach();
    }

    fn finish_file_load(
        &mut self,
        document_id: u64,
        result: Result<(PathBuf, file_view::LoadedFileView), file_view::FileOpenError>,
        cx: &mut Context<Self>,
    ) {
        self.pending_file_loads.remove(&document_id);
        self.cancel_file_loading_indicator(document_id);
        let Some(index) = self.tabs.iter().position(|tab| tab.id == document_id) else {
            return;
        };
        let fallback_path = self.tabs[index]
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from(&self.tabs[index].title));
        let pending_goto = self.tabs[index].pending_goto.take();
        let mut opened = false;
        let mut is_text = false;

        match result {
            Ok((path, loaded)) => {
                opened = true;
                self.tabs[index].path = Some(path.clone());
                match loaded {
                    file_view::LoadedFileView::Text(loaded) => {
                        is_text = true;
                        let mut tab = DocumentTab::loaded(document_id, *loaded);
                        tab.pending_goto = pending_goto;
                        self.tabs[index] = tab;
                        super::apply_document_defaults(
                            &mut self.tabs[index],
                            &self.settings.current(),
                        );
                        self.apply_pending_search_navigation(&path, cx);
                    }
                    file_view::LoadedFileView::Image(loaded) => {
                        let file_view::LoadedImage { info, render_image } = loaded;
                        let memory = usize::try_from(info.width)
                            .ok()
                            .and_then(|width| {
                                usize::try_from(info.height)
                                    .ok()
                                    .and_then(|height| width.checked_mul(height))
                            })
                            .and_then(|pixels| pixels.checked_mul(4))
                            .unwrap_or(usize::MAX);
                        self.image_cache.insert(document_id, render_image, memory);
                        self.tabs[index] = DocumentTab::image(document_id, path.clone(), info);
                        super::apply_document_defaults(
                            &mut self.tabs[index],
                            &self.settings.current(),
                        );
                    }
                    file_view::LoadedFileView::Unsupported(info) => {
                        self.tabs[index] =
                            DocumentTab::unsupported(document_id, path.clone(), info);
                        super::apply_document_defaults(
                            &mut self.tabs[index],
                            &self.settings.current(),
                        );
                    }
                }
                self.record_recent_paths(vec![path.clone()], cx);
            }
            Err(error) => {
                self.set_status(error.message.clone());
                self.tabs[index] = DocumentTab::error(document_id, fallback_path, error);
            }
        }
        self.complete_cli_document_load(document_id, opened, is_text);
        cx.notify();
    }

    pub(super) fn cancel_file_loading_indicator(&mut self, document_id: u64) {
        if let Some(cancelled) = self.pending_loading_indicators.remove(&document_id) {
            cancelled.store(true, Ordering::Release);
        }
    }

    fn apply_pending_search_navigation(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let Some((pending_path, position, match_columns, disk_identity)) =
            self.pending_search_navigation.take()
        else {
            return;
        };
        if !super::paths::same_path(path, &pending_path) {
            self.pending_search_navigation =
                Some((pending_path, position, match_columns, disk_identity));
            return;
        }
        let Some(index) = self.tabs.iter().position(|tab| {
            tab.path
                .as_ref()
                .is_some_and(|tab_path| super::paths::same_path(tab_path, path))
        }) else {
            return;
        };
        let disk_result_is_stale = disk_identity.is_some_and(|expected| {
            self.tabs[index].fingerprint.is_none_or(|fingerprint| {
                let actual = fingerprint.to_data();
                actual.length != expected.length
                    || actual.modified_unix_nanos != expected.modified_unix_nanos
            })
        });
        if disk_result_is_stale {
            self.mark_search_stale(cx);
            return;
        }
        let Some(editor) = self.tabs[index].editor.as_mut() else {
            return;
        };
        let snapshot = editor.snapshot();
        let start = snapshot
            .position_to_byte_clamped(position)
            .map_or(snapshot.len_bytes(), |offset| offset.0);
        let end = snapshot
            .position_to_byte_clamped(TextPosition::new(
                position.line,
                position.column.saturating_add(match_columns),
            ))
            .map_or(start, |offset| offset.0);
        let selection = start.min(end)..end;
        self.tabs[index].last_selection = selection.clone();
        if let Some(state) = self.tabs[index].ui.as_ref() {
            state.update(cx, |state, cx| state.set_selected_range(selection, cx));
        }
        preview::activate(self, index);
    }

    pub(super) fn retry_active_file(&mut self, cx: &mut Context<Self>) {
        let Some((id, path)) = self
            .active_tab()
            .and_then(|tab| Some((tab.id, tab.path.clone()?)))
        else {
            return;
        };
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.content = file_view::TabContent::Loading;
            tab.loading_indicator_visible = false;
            tab.editor = None;
            tab.ui = None;
        }
        self.clear_status();
        self.start_file_load(id, path, cx);
        cx.notify();
    }

    pub(super) fn open_active_file_externally(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else { return };
        let Some(path) = tab.path.clone() else { return };
        let id = tab.id;
        let supported_external = matches!(
            &tab.content,
            file_view::TabContent::Unsupported(info) if info.can_open_externally
        );
        if !supported_external || !file_view::can_open_externally(&path) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { file_view::open_externally(&path) })
                .await;
            let _ = this.update(cx, |window, cx| {
                if let Some(tab) = window.tabs.iter_mut().find(|tab| tab.id == id)
                    && let file_view::TabContent::Unsupported(info) = &mut tab.content
                {
                    info.external_error = result.err();
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn record_recent_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let service = self.settings.clone();
        let mut settings = service.current();
        for path in paths {
            settings
                .recent_files
                .retain(|recent| !super::paths::same_path(recent, &path));
            settings.recent_files.insert(0, path);
        }
        settings
            .recent_files
            .truncate(usize::from(settings.recent_file_limit));
        service.replace(settings.clone());

        let Some(settings_path) = service.path() else {
            return;
        };
        cx.background_executor()
            .spawn(async move {
                if let Err(error) = save_file(&settings_path, &settings) {
                    tracing::warn!(error = %error, "recent files could not be persisted");
                }
            })
            .detach();
    }
}

fn existing_tab_index(tabs: &[DocumentTab], path: &Path) -> Option<usize> {
    tabs.iter().position(|tab| {
        tab.path
            .as_ref()
            .is_some_and(|open_path| super::paths::same_path(open_path, path))
    })
}

#[cfg(test)]
mod tests {
    use super::{DocumentTab, existing_tab_index};
    use std::path::PathBuf;

    #[test]
    fn opening_the_same_normalized_path_reuses_the_existing_tab() {
        let directory = tempfile::tempdir().expect("create test directory");
        let path = directory.path().join("image.png");
        std::fs::write(&path, b"not loaded by this lookup").expect("create test file");
        let tabs = [DocumentTab::loading(1, path.clone())];
        let alternate = path
            .parent()
            .expect("test path has a parent")
            .join(".")
            .join("image.png");
        assert_eq!(existing_tab_index(&tabs, &alternate), Some(0));
    }

    #[test]
    fn image_loading_tab_has_no_text_editor_buffer() {
        let tab = DocumentTab::loading(1, PathBuf::from("photo.png"));
        assert!(tab.editor.is_none());
        assert!(tab.ui.is_none());
        assert!(!tab.is_dirty());
        assert!(!tab.loading_indicator_visible);
    }
}
