use std::{collections::HashSet, path::PathBuf, sync::mpsc::SyncSender};

use dopad_core::TextPosition;
use dopad_ipc::{CliCommand, CliRequest, CliResponse, ResponseStatus, TargetKind};
use gpui_kit::Context;

use super::{DopadWindow, file_view, preview, sidebar};

pub(super) struct CliOpenState {
    loading: HashSet<u64>,
    waiting: HashSet<u64>,
    requires_text: HashSet<u64>,
    wait: bool,
    response: SyncSender<CliResponse>,
    error: Option<String>,
    cancelled: bool,
}

impl DopadWindow {
    pub(crate) fn handle_cli_request(
        &mut self,
        request: CliRequest,
        response: SyncSender<CliResponse>,
        cx: &mut Context<Self>,
    ) {
        if !self.session_bootstrap_complete {
            self.pending_cli_requests
                .push(crate::cli_ipc::UiMessage::Request { request, response });
            return;
        }
        self.process_cli_request(request, response, cx);
    }

    pub(crate) fn cancel_cli_request(&mut self, request_id: u64) {
        self.pending_cli_requests.retain(|message| match message {
            crate::cli_ipc::UiMessage::Request { request, .. } => request.request_id != request_id,
            crate::cli_ipc::UiMessage::Probe { .. } => true,
            crate::cli_ipc::UiMessage::CancelWait { .. } => true,
        });
        self.cli_open_states.remove(&request_id);
    }

    pub(super) fn flush_pending_cli_requests(&mut self, cx: &mut Context<Self>) {
        let requests = std::mem::take(&mut self.pending_cli_requests);
        for message in requests {
            if let crate::cli_ipc::UiMessage::Request { request, response } = message {
                self.process_cli_request(request, response, cx);
            }
        }
    }

    pub(super) fn complete_cli_document_load(
        &mut self,
        document_id: u64,
        opened: bool,
        is_text: bool,
    ) {
        let request_ids: Vec<u64> = self
            .cli_open_states
            .iter()
            .filter_map(|(request_id, state)| {
                state.loading.contains(&document_id).then_some(*request_id)
            })
            .collect();
        for request_id in request_ids {
            tracing::debug!(
                request_id,
                document_id,
                opened,
                is_text,
                "CLI requested file load completed"
            );
            let Some(state) = self.cli_open_states.get_mut(&request_id) else {
                continue;
            };
            state.loading.remove(&document_id);
            if !opened {
                state.error.get_or_insert_with(|| {
                    "Dopad could not read one of the requested files".into()
                });
            } else if state.requires_text.contains(&document_id) && !is_text {
                state.error.get_or_insert_with(|| {
                    "--wait and --goto require an editable text file".into()
                });
            }
            self.finish_cli_request_if_ready(request_id);
        }
    }

    pub(super) fn complete_cli_tab_close(&mut self, document_id: u64, was_dirty: bool) {
        let request_ids: Vec<u64> = self
            .cli_open_states
            .iter()
            .filter_map(|(request_id, state)| {
                (state.loading.contains(&document_id) || state.waiting.contains(&document_id))
                    .then_some(*request_id)
            })
            .collect();
        for request_id in request_ids {
            let Some(state) = self.cli_open_states.get_mut(&request_id) else {
                continue;
            };
            if state.loading.remove(&document_id) {
                state.error.get_or_insert_with(|| {
                    "A requested file tab was closed before it finished loading".into()
                });
            }
            if state.waiting.remove(&document_id) && was_dirty {
                state.cancelled = true;
                state.error.get_or_insert_with(|| {
                    "Wait cancelled because the tab closed with unsaved changes".into()
                });
            }
            self.finish_cli_request_if_ready(request_id);
        }
    }

    fn process_cli_request(
        &mut self,
        request: CliRequest,
        response: SyncSender<CliResponse>,
        cx: &mut Context<Self>,
    ) {
        let request_id = request.request_id;
        let CliCommand::Open {
            targets,
            wait,
            goto,
            ..
        } = request.command
        else {
            let _ = response.send(crate::cli_ipc::complete_response(
                request_id,
                ResponseStatus::Success,
                None,
            ));
            return;
        };

        let decoded_targets = match decode_targets(targets) {
            Ok(targets) => targets,
            Err(message) => {
                let _ = response.send(crate::cli_ipc::complete_response(
                    request_id,
                    ResponseStatus::InvalidRequest,
                    Some(message),
                ));
                return;
            }
        };
        tracing::debug!(
            request_id,
            target_count = decoded_targets.len(),
            wait,
            "opening CLI file/folder targets"
        );

        let mut document_ids = Vec::new();
        for (path, kind) in decoded_targets {
            match kind {
                TargetKind::Directory => sidebar::show_explorer_root(self, path, cx),
                TargetKind::File => {
                    document_ids.extend(self.open_paths_internal(vec![path], false, cx))
                }
            }
        }
        document_ids.sort_unstable();
        document_ids.dedup();

        let navigation_id = if let Some(navigation) = goto {
            let path = match navigation.path.into_path() {
                Ok(path) => path,
                Err(error) => {
                    let _ = response.send(crate::cli_ipc::complete_response(
                        request_id,
                        ResponseStatus::InvalidRequest,
                        Some(format!("invalid --goto path: {error}")),
                    ));
                    return;
                }
            };
            let Some(index) = self.tabs.iter().position(|tab| {
                tab.path
                    .as_deref()
                    .is_some_and(|tab_path| super::paths::same_path(tab_path, &path))
            }) else {
                let _ = response.send(crate::cli_ipc::complete_response(
                    request_id,
                    ResponseStatus::OpenFailed,
                    Some("Dopad could not open the --goto target".into()),
                ));
                return;
            };
            let id = self.tabs[index].id;
            preview::activate(self, index);
            self.tabs[index].preview_mode = false;
            let position = TextPosition::new(
                navigation.line.saturating_sub(1),
                navigation.column.unwrap_or(1).saturating_sub(1),
            );
            if !set_navigation(self, index, position, cx) {
                let _ = response.send(crate::cli_ipc::complete_response(
                    request_id,
                    ResponseStatus::OpenFailed,
                    Some("--goto target is not an editable text document".into()),
                ));
                return;
            }
            Some(id)
        } else {
            None
        };

        let mut state = CliOpenState {
            loading: HashSet::new(),
            waiting: HashSet::new(),
            requires_text: HashSet::new(),
            wait,
            response: response.clone(),
            error: None,
            cancelled: false,
        };
        for document_id in document_ids {
            let Some(tab) = self.tabs.iter().find(|tab| tab.id == document_id) else {
                continue;
            };
            match &tab.content {
                file_view::TabContent::Loading => {
                    state.loading.insert(document_id);
                    if wait || navigation_id == Some(document_id) {
                        state.waiting.insert(document_id);
                        state.requires_text.insert(document_id);
                    }
                }
                file_view::TabContent::Text => {
                    if wait {
                        state.waiting.insert(document_id);
                        state.requires_text.insert(document_id);
                    }
                }
                file_view::TabContent::Error(_) => {
                    state.error.get_or_insert_with(|| {
                        "Dopad could not read one of the requested files".into()
                    });
                }
                file_view::TabContent::Image(_) | file_view::TabContent::Unsupported(_) => {
                    if wait || navigation_id == Some(document_id) {
                        state.error.get_or_insert_with(|| {
                            "--wait and --goto require an editable text file".into()
                        });
                    }
                }
            }
        }
        if let Some(navigation_id) = navigation_id
            && !state.loading.contains(&navigation_id)
        {
            state.requires_text.insert(navigation_id);
        }
        if let Some(error) = state.error.take() {
            let _ = response.send(crate::cli_ipc::complete_response(
                request_id,
                ResponseStatus::OpenFailed,
                Some(error),
            ));
            return;
        }
        if !wait {
            let _ = response.send(crate::cli_ipc::accepted_response(request_id));
            return;
        }
        if wait {
            let _ = response.send(crate::cli_ipc::accepted_response(request_id));
        }
        if state.loading.is_empty() {
            if wait && !state.waiting.is_empty() {
                self.cli_open_states.insert(request_id, state);
            } else if wait {
                let _ = response.send(crate::cli_ipc::wait_response(
                    request_id,
                    ResponseStatus::Success,
                    None,
                ));
            } else {
                let _ = response.send(crate::cli_ipc::complete_response(
                    request_id,
                    ResponseStatus::Success,
                    None,
                ));
            }
        } else {
            self.cli_open_states.insert(request_id, state);
        }
    }

    fn finish_cli_request_if_ready(&mut self, request_id: u64) {
        let Some(state) = self.cli_open_states.get(&request_id) else {
            return;
        };
        if !state.loading.is_empty()
            || (state.wait && !state.waiting.is_empty() && state.error.is_none())
        {
            return;
        }
        let Some(state) = self.cli_open_states.remove(&request_id) else {
            return;
        };
        if let Some(error) = state.error {
            let response = if state.wait {
                let status = if state.cancelled {
                    ResponseStatus::WaitCancelled
                } else {
                    ResponseStatus::OpenFailed
                };
                crate::cli_ipc::wait_response(request_id, status, Some(error))
            } else {
                crate::cli_ipc::complete_response(
                    request_id,
                    ResponseStatus::OpenFailed,
                    Some(error),
                )
            };
            let _ = state.response.send(response);
        } else if state.wait && state.waiting.is_empty() {
            let _ = state.response.send(crate::cli_ipc::wait_response(
                request_id,
                ResponseStatus::Success,
                None,
            ));
        } else if !state.wait {
            let _ = state.response.send(crate::cli_ipc::complete_response(
                request_id,
                ResponseStatus::Success,
                None,
            ));
        }
    }
}

fn decode_targets(
    targets: Vec<dopad_ipc::OpenTarget>,
) -> Result<Vec<(PathBuf, TargetKind)>, String> {
    targets
        .into_iter()
        .map(|target| {
            target
                .path
                .into_path()
                .map(|path| (path, target.kind))
                .map_err(|error| format!("invalid native path encoding: {error}"))
        })
        .collect()
}

fn set_navigation(
    window: &mut DopadWindow,
    index: usize,
    position: TextPosition,
    cx: &mut Context<DopadWindow>,
) -> bool {
    let Some(tab) = window.tabs.get_mut(index) else {
        return false;
    };
    if !matches!(
        tab.content,
        file_view::TabContent::Loading | file_view::TabContent::Text
    ) {
        return false;
    }
    if let (Some(editor), Some(state)) = (tab.editor.as_ref(), tab.ui.as_ref()) {
        let offset = editor
            .snapshot()
            .position_to_byte_clamped(position)
            .map_or(0, |offset| offset.0);
        state.update(cx, |state, cx| state.set_selected_range(offset..offset, cx));
    } else {
        tab.pending_goto = Some(position);
    }
    true
}
