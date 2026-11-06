use std::{cell::Cell, collections::HashMap, ops::Range, rc::Rc, time::Duration};

use dopad_core::{
    ByteOffset, DraftCheckpoint, Encoding, EolKind, FileFingerprint, RecoveredDraft, SessionError,
    SessionManifest, SessionTab, TextSnapshot,
};
use dopad_editor::Editor;
use gpui_kit::component::{
    WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Context, Entity, ParentElement as _, Window, div,
};

use super::{DocumentTab, DopadWindow};
use crate::session::{SessionBootstrap, SessionCoordinator};

const CHECKPOINT_DEBOUNCE: Duration = Duration::from_millis(900);

pub(super) fn install_close_guard(window: &Window, cx: &App, view: &Entity<DopadWindow>) {
    let view = view.downgrade();
    window.on_window_should_close(cx, move |window, app| {
        let window_handle = window.window_handle();
        let Some(view) = view.upgrade() else {
            return true;
        };
        let request = app.update_entity(&view, |state, cx| {
            state.request_window_close(window_handle, cx)
        });
        match request {
            WindowCloseRequest::Confirm => {
                let was_confirmed = Rc::new(Cell::new(false));
                window.open_alert_dialog(app, move |alert, _, _| {
                    let confirm_view = view.downgrade();
                    let close_view = view.downgrade();
                    let confirmed = was_confirmed.clone();
                    let confirmed_on_close = was_confirmed.clone();
                    alert
                        .title("Unsaved changes")
                        .description(
                            "Unsaved drafts will be saved for recovery next time. Close this window?",
                        )
                        .confirm()
                        .ok_text("Close Window")
                        .cancel_text("Cancel")
                        .on_ok(move |_, _, app| {
                            confirmed.set(true);
                            let Some(confirm_view) = confirm_view.upgrade() else {
                                return true;
                            };
                            app.update_entity(&confirm_view, |state: &mut DopadWindow, cx| {
                                state.confirm_window_close(cx);
                            });
                            true
                        })
                        .on_close(move |_, _, app| {
                            if !confirmed_on_close.get()
                                && let Some(close_view) = close_view.upgrade()
                            {
                                app.update_entity(&close_view, |state: &mut DopadWindow, cx| {
                                    state.cancel_window_close(cx);
                                });
                            }
                        })
                });
                false
            }
            WindowCloseRequest::Blocked => false,
        }
    });
}

#[derive(PartialEq)]
pub(super) struct SessionSignature {
    active_tab_id: Option<u64>,
    dark_theme: bool,
    tabs: Vec<TabSignature>,
}

#[derive(PartialEq)]
struct TabSignature {
    id: u64,
    title: String,
    path: Option<std::path::PathBuf>,
    revision: u64,
    selection: Range<usize>,
    cursor: usize,
    encoding: Encoding,
    eol: EolKind,
    word_wrap: bool,
    font_size: u32,
    preview_mode: bool,
    dirty: bool,
}

struct PendingDraft {
    document_id: u64,
    revision: u64,
    snapshot: TextSnapshot,
}

struct PendingCheckpoint {
    manifest: SessionManifest,
    drafts: Vec<PendingDraft>,
}

pub(super) struct PendingWindowClose {
    checkpoint: PendingCheckpoint,
    has_dirty_documents: bool,
    coordinator: SessionCoordinator,
    window_handle: AnyWindowHandle,
}

enum WindowCloseRequest {
    Confirm,
    Blocked,
}

pub(super) fn show_recovery_prompt(
    state: &mut DopadWindow,
    window: &mut Window,
    cx: &mut Context<DopadWindow>,
) {
    if state.recovery_prompt.is_none() || state.recovery_prompt_open {
        return;
    }
    state.recovery_prompt_open = true;

    let view = cx.entity().downgrade();
    window.open_dialog(cx, move |dialog, _, _| {
        let was_resolved = Rc::new(Cell::new(false));
        let recover_view = view.clone();
        let discard_view = view.clone();
        let close_view = view.clone();
        let discard_resolved = was_resolved.clone();
        let recover_resolved = was_resolved.clone();
        let close_resolved = was_resolved.clone();

        let discard = Button::new("discard-session-recovery")
            .label("Discard")
            .danger()
            .on_click(move |_, window, app| {
                discard_resolved.set(true);
                if let Some(discard_view) = discard_view.upgrade() {
                    app.update_entity(&discard_view, |state: &mut DopadWindow, cx| {
                        state.resolve_recovery(false, cx);
                    });
                }
                window.close_dialog(app);
            });
        let recover = Button::new("recover-session")
            .label("Recover")
            .primary()
            .on_click(move |_, window, app| {
                recover_resolved.set(true);
                if let Some(recover_view) = recover_view.upgrade() {
                    app.update_entity(&recover_view, |state: &mut DopadWindow, cx| {
                        state.resolve_recovery(true, cx);
                    });
                }
                window.close_dialog(app);
            });

        dialog
            .title("Recover previous session?")
            .child(div().child("Dopad found unsaved drafts from an earlier run. Recover them now?"))
            .footer(DialogFooter::new().child(discard).child(recover))
            .on_close(move |_, _, app| {
                if !close_resolved.get()
                    && let Some(close_view) = close_view.upgrade()
                {
                    app.update_entity(&close_view, |state: &mut DopadWindow, cx| {
                        state.resolve_recovery(true, cx);
                    });
                }
            })
    });
}

pub(super) fn start_bootstrap(window: &mut DopadWindow, cx: &mut Context<DopadWindow>) {
    if window.session_bootstrap_started {
        return;
    }
    window.session_bootstrap_started = true;
    let coordinator = window.session.clone();
    cx.spawn(async move |this, cx| {
        let result = cx
            .background_executor()
            .spawn(async move { coordinator.initialize() })
            .await;
        let _ = this.update(cx, |window, cx| {
            window.finish_session_bootstrap(result, cx);
        });
    })
    .detach();
}

pub(super) fn schedule_checkpoint_if_changed(
    window: &mut DopadWindow,
    cx: &mut Context<DopadWindow>,
) {
    if window.window_close_pending {
        return;
    }
    if !window.session_bootstrap_complete {
        return;
    }
    let signature = session_signature(window, cx);
    if window.session_signature.as_ref() == Some(&signature) {
        return;
    }
    window.session_signature = Some(signature);
    window.session_generation = window.session_generation.saturating_add(1);
    let generation = window.session_generation;
    let coordinator = window.session.clone();

    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(CHECKPOINT_DEBOUNCE).await;
        let checkpoint = this
            .update(cx, |window, cx| {
                (window.session_generation == generation).then(|| collect_checkpoint(window, cx))
            })
            .ok()
            .flatten();
        let Some(checkpoint) = checkpoint else {
            return;
        };

        let result = cx
            .background_executor()
            .spawn(async move { persist_checkpoint(coordinator, checkpoint) })
            .await;
        let _ = this.update(cx, |window, cx| {
            if window.session_generation != generation {
                return;
            }
            match result {
                Ok(true) => window.session_status = None,
                Ok(false) => return,
                Err(error) => {
                    window.session_status =
                        Some(format!("Session recovery checkpoint failed: {error}"));
                }
            }
            cx.notify();
        });
    })
    .detach();
}

impl DopadWindow {
    fn request_window_close(
        &mut self,
        window_handle: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) -> WindowCloseRequest {
        if self.window_close_pending {
            return WindowCloseRequest::Blocked;
        }
        if !self.session_bootstrap_complete {
            self.session_status = Some("Session recovery is still initializing.".into());
            cx.notify();
            return WindowCloseRequest::Blocked;
        }

        let has_dirty_documents = self.tabs.iter().any(DocumentTab::is_dirty);
        if has_dirty_documents && !self.session.can_persist_recovery() {
            self.session_status = Some(
                "Save unsaved documents before closing this window; recovery storage is unavailable here."
                    .into(),
            );
            cx.notify();
            return WindowCloseRequest::Blocked;
        }

        self.window_close_pending = true;
        self.session_generation = self.session_generation.saturating_add(1);
        self.pending_window_close = Some(PendingWindowClose {
            checkpoint: collect_checkpoint(self, cx),
            has_dirty_documents,
            coordinator: self.session.clone(),
            window_handle,
        });
        if has_dirty_documents {
            cx.notify();
            return WindowCloseRequest::Confirm;
        }

        self.confirm_window_close(cx);
        WindowCloseRequest::Blocked
    }

    fn confirm_window_close(&mut self, cx: &mut Context<Self>) {
        let Some(PendingWindowClose {
            checkpoint,
            has_dirty_documents,
            coordinator,
            window_handle,
        }) = self.pending_window_close.take()
        else {
            return;
        };

        cx.spawn(async move |this, cx| {
            let coordinator_for_worker = coordinator.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let persisted = persist_checkpoint(coordinator_for_worker.clone(), checkpoint)
                        .map_err(|error| error.to_string())?;
                    if has_dirty_documents && !persisted {
                        return Err(
                            "Recovery storage is unavailable; save the document before closing."
                                .to_owned(),
                        );
                    }
                    if has_dirty_documents {
                        coordinator_for_worker.preserve_recovery_on_exit();
                    } else {
                        coordinator_for_worker
                            .mark_clean_shutdown()
                            .map_err(|error| error.to_string())?;
                    }
                    Ok::<(), String>(())
                })
                .await;

            let should_close = this.update(cx, |window, cx| match result {
                Ok(()) => true,
                Err(error) => {
                    window.window_close_pending = false;
                    window.session_signature = None;
                    window.session_status = Some(format!("Could not close safely: {error}"));
                    cx.notify();
                    false
                }
            });
            if matches!(should_close, Ok(true)) {
                let _ = window_handle.update(cx, |_, window, app| {
                    let is_last_window = app.windows().len() <= 1;
                    window.remove_window();
                    if is_last_window {
                        app.quit();
                    }
                });
            }
        })
        .detach();
    }

    fn cancel_window_close(&mut self, cx: &mut Context<Self>) {
        self.pending_window_close = None;
        self.window_close_pending = false;
        self.session_signature = None;
        cx.notify();
    }

    fn finish_session_bootstrap(
        &mut self,
        result: Result<Option<SessionBootstrap>, SessionError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(None) => {
                self.session_bootstrap_complete = true;
                self.session_status =
                    Some("Session recovery is unavailable on this system.".into());
                self.open_startup_paths(cx);
                cx.notify();
            }
            Ok(Some(bootstrap)) => {
                self.session_generation = bootstrap.next_generation;
                let has_unclean_drafts = bootstrap
                    .recovered
                    .as_ref()
                    .is_some_and(|session| session.was_unclean && !session.drafts.is_empty());
                if has_unclean_drafts {
                    self.recovery_prompt = Some(bootstrap);
                    self.recovery_prompt_open = false;
                    cx.notify();
                } else if !self.settings.current().restore_session {
                    self.session_bootstrap_complete = true;
                    self.session_signature = None;
                    self.open_startup_paths(cx);
                    cx.notify();
                } else {
                    self.restore_session(bootstrap, cx);
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "session recovery initialization failed");
                self.session_bootstrap_complete = true;
                self.session_status = Some(format!("Session recovery is unavailable: {error}"));
                self.open_startup_paths(cx);
                cx.notify();
            }
        }
    }

    fn resolve_recovery(&mut self, recover: bool, cx: &mut Context<Self>) {
        let Some(bootstrap) = self.recovery_prompt.take() else {
            return;
        };
        self.recovery_prompt_open = false;
        if recover {
            self.restore_session(bootstrap, cx);
            return;
        }

        let session = self.session.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { session.discard() })
                .await;
            let _ = this.update(cx, |window, cx| {
                window.session_bootstrap_complete = true;
                window.session_signature = None;
                window.session_status = result
                    .err()
                    .map(|error| format!("Could not discard recovery drafts: {error}"));
                window.open_startup_paths(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn restore_session(&mut self, bootstrap: SessionBootstrap, cx: &mut Context<Self>) {
        let Some(recovered) = bootstrap.recovered else {
            self.session_bootstrap_complete = true;
            self.session_signature = None;
            self.open_startup_paths(cx);
            cx.notify();
            return;
        };

        let mut drafts = recovered
            .drafts
            .into_iter()
            .map(|draft| (draft.document_id, draft))
            .collect::<HashMap<_, _>>();
        let mut disk_files = bootstrap.disk_files;
        let manifest = recovered.manifest;
        let mut tabs = Vec::with_capacity(manifest.tabs.len());
        let mut missing_files = 0;

        for saved in manifest.tabs {
            let tab = if let Some(draft) = drafts.remove(&saved.id) {
                restored_draft_tab(saved, draft)
            } else if saved.path.is_some() {
                match disk_files.remove(&saved.id) {
                    Some(Ok(loaded)) => restored_disk_tab(saved, loaded),
                    Some(Err(_)) | None => {
                        missing_files += 1;
                        continue;
                    }
                }
            } else {
                restored_empty_tab(saved)
            };
            tabs.push(tab);
        }

        apply_word_wrap_setting(&mut tabs, self.settings.current().word_wrap);
        self.tabs = tabs;
        if self.tabs.is_empty() {
            self.tabs.push(DocumentTab::blank(1));
            self.next_id = 2;
            self.active = 0;
        } else {
            self.active = manifest
                .active_tab_id
                .and_then(|id| self.tabs.iter().position(|tab| tab.id == id))
                .unwrap_or(0);
            self.next_id = self
                .tabs
                .iter()
                .map(|tab| tab.id)
                .max()
                .and_then(|id| id.checked_add(1))
                .unwrap_or(1);
        }
        self.dark_theme = false;
        crate::theme::apply(self.dark_theme, cx);
        self.session_bootstrap_complete = true;
        self.session_signature = None;
        self.session_status = (missing_files > 0).then(|| {
            format!("Could not reopen {missing_files} file(s) from the previous session.")
        });
        self.open_startup_paths(cx);
        cx.notify();
    }
}

pub(super) fn apply_word_wrap_setting(tabs: &mut [DocumentTab], enabled: bool) {
    for tab in tabs.iter_mut().filter(|tab| tab.editor.is_some()) {
        tab.word_wrap = enabled;
    }
}

fn persist_checkpoint(
    coordinator: crate::session::SessionCoordinator,
    checkpoint: PendingCheckpoint,
) -> Result<bool, SessionError> {
    let drafts = checkpoint
        .drafts
        .into_iter()
        .map(|draft| DraftCheckpoint {
            document_id: draft.document_id,
            revision: draft.revision,
            text: draft.snapshot.to_string(),
        })
        .collect();
    coordinator.checkpoint(checkpoint.manifest, drafts)
}

fn restored_draft_tab(saved: SessionTab, draft: RecoveredDraft) -> DocumentTab {
    let mut editor = Editor::new_recovered(&draft.text);
    let selection = restored_selection(&mut editor, &saved);
    let language_path = saved
        .path
        .as_deref()
        .or_else(|| Some(std::path::Path::new(&saved.title)));
    let detected_language = dopad_syntax::detect_language(language_path, &draft.text);
    DocumentTab {
        id: saved.id,
        title: saved.title,
        path: saved.path,
        encoding: saved.encoding,
        eol: saved.eol,
        fingerprint: saved.fingerprint.map(FileFingerprint::from),
        external_change_detected: false,
        editor: Some(editor),
        content: super::file_view::TabContent::Text,
        loading_indicator_visible: false,
        ui: None,
        search_decorations: None,
        current_search_decorations: None,
        bracket_decorations: None,
        pending_ui_text: Some(draft.text),
        last_selection: selection,
        word_wrap: saved.word_wrap,
        font_size: saved.font_size,
        preview_mode: saved.preview_mode,
        detected_language,
        language_override: None,
        pending_goto: None,
    }
}

fn restored_disk_tab(saved: SessionTab, loaded: dopad_core::LoadedFile) -> DocumentTab {
    let mut tab = DocumentTab::loaded_from_file(saved.id, loaded);
    tab.title = saved.title.clone();
    tab.word_wrap = saved.word_wrap;
    tab.font_size = saved.font_size;
    tab.preview_mode = saved.preview_mode;
    if let Some(editor) = tab.editor.as_mut() {
        tab.last_selection = restored_selection(editor, &saved);
    }
    tab
}

fn restored_empty_tab(saved: SessionTab) -> DocumentTab {
    let mut tab = DocumentTab::blank(saved.id);
    tab.title = saved.title.clone();
    tab.detected_language =
        dopad_syntax::detect_language(Some(std::path::Path::new(&tab.title)), "");
    tab.encoding = saved.encoding;
    tab.eol = saved.eol;
    tab.word_wrap = saved.word_wrap;
    tab.font_size = saved.font_size;
    tab.preview_mode = saved.preview_mode;
    if let Some(editor) = tab.editor.as_mut() {
        tab.last_selection = restored_selection(editor, &saved);
    }
    tab
}

fn restored_selection(editor: &mut Editor, saved: &SessionTab) -> Range<usize> {
    let snapshot = editor.snapshot();
    let len = snapshot.len_bytes();
    let start = saved.selection_start.min(len);
    let end = saved.selection_end.min(len).max(start);
    if editor
        .set_selection(ByteOffset(start), ByteOffset(end))
        .is_ok()
    {
        return start..end;
    }
    let cursor = saved.cursor.min(len);
    if editor.set_cursor(ByteOffset(cursor)).is_ok() {
        cursor..cursor
    } else {
        0..0
    }
}

fn session_signature(window: &DopadWindow, cx: &Context<DopadWindow>) -> SessionSignature {
    let tabs = window
        .tabs
        .iter()
        .filter_map(|tab| {
            let editor = tab.editor.as_ref()?;
            let (selection, cursor) = tab.ui.as_ref().map_or_else(
                || (tab.last_selection.clone(), editor.cursor().offset().0),
                |state| {
                    let state = state.read(cx);
                    (state.selected_range(), state.cursor())
                },
            );
            Some(TabSignature {
                id: tab.id,
                title: tab.title.clone(),
                path: tab.path.clone(),
                revision: editor.revision().0,
                selection,
                cursor,
                encoding: tab.encoding,
                eol: tab.eol,
                word_wrap: tab.word_wrap,
                font_size: tab.font_size.to_bits(),
                preview_mode: tab.preview_mode,
                dirty: tab.is_dirty(),
            })
        })
        .collect();
    SessionSignature {
        active_tab_id: window
            .active_tab()
            .filter(|tab| tab.editor.is_some())
            .map(|tab| tab.id),
        dark_theme: window.dark_theme,
        tabs,
    }
}

fn collect_checkpoint(window: &DopadWindow, cx: &Context<DopadWindow>) -> PendingCheckpoint {
    let mut drafts = Vec::new();
    let tabs = window
        .tabs
        .iter()
        .filter_map(|tab| {
            let editor = tab.editor.as_ref()?;
            let (selection, cursor) = tab.ui.as_ref().map_or_else(
                || (tab.last_selection.clone(), editor.cursor().offset().0),
                |state| {
                    let state = state.read(cx);
                    (state.selected_range(), state.cursor())
                },
            );
            let revision = editor.revision();
            if tab.is_dirty() {
                drafts.push(PendingDraft {
                    document_id: tab.id,
                    revision: revision.0,
                    snapshot: editor.snapshot(),
                });
            }
            Some(SessionTab {
                id: tab.id,
                title: tab.title.clone(),
                path: tab.path.clone(),
                encoding: tab.encoding,
                eol: tab.eol,
                fingerprint: tab.fingerprint.map(FileFingerprint::to_data),
                revision: revision.0,
                cursor,
                selection_start: selection.start,
                selection_end: selection.end,
                word_wrap: tab.word_wrap,
                font_size: tab.font_size,
                preview_mode: tab.preview_mode,
                draft_ref: None,
            })
        })
        .collect();
    PendingCheckpoint {
        manifest: SessionManifest {
            schema_version: 1,
            generation: window.session_generation,
            clean_shutdown: false,
            active_tab_id: window
                .active_tab()
                .filter(|tab| tab.editor.is_some())
                .map(|tab| tab.id),
            dark_theme: window.dark_theme,
            tabs,
        },
        drafts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_wrap_setting_updates_all_open_text_tabs() {
        let mut tabs = [DocumentTab::blank(1), DocumentTab::blank(2)];
        apply_word_wrap_setting(&mut tabs, true);
        assert!(tabs.iter().all(|tab| tab.word_wrap));

        apply_word_wrap_setting(&mut tabs, false);
        assert!(tabs.iter().all(|tab| !tab.word_wrap));
    }
}
