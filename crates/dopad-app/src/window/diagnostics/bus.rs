use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use lsp_types::{NumberOrString, PublishDiagnosticsParams};

use super::DiagnosticKey;

pub(super) const MAX_PENDING_DOCUMENTS: usize = 128;
pub(super) const MAX_PENDING_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_DIAGNOSTICS_PER_DOCUMENT: usize = 512;
pub(super) const MAX_DIAGNOSTIC_MESSAGE_BYTES: usize = 2_048;
pub(super) const MAX_DIAGNOSTIC_FIELD_BYTES: usize = 256;

#[derive(Clone, Debug)]
pub(in crate::window) struct DiagnosticUpdate {
    pub workspace: PathBuf,
    pub server_id: String,
    pub params: PublishDiagnosticsParams,
}

#[derive(Default)]
struct PendingDiagnostics {
    updates: HashMap<DiagnosticKey, DiagnosticUpdate>,
    stopped_servers: HashSet<(PathBuf, String)>,
    bytes: usize,
}

#[derive(Default)]
pub(super) struct DiagnosticBatch {
    pub(super) updates: Vec<DiagnosticUpdate>,
    pub(super) stopped_servers: Vec<(PathBuf, String)>,
}

impl DiagnosticUpdate {
    pub(super) fn key(&self) -> DiagnosticKey {
        DiagnosticKey {
            workspace: self.workspace.clone(),
            server_id: self.server_id.clone(),
            uri: self.params.uri.as_str().to_owned(),
        }
    }
}

#[derive(Clone)]
pub(in crate::window) struct DiagnosticBus {
    pending: Arc<Mutex<PendingDiagnostics>>,
    wake_sender: async_channel::Sender<()>,
    wake_receiver: async_channel::Receiver<()>,
}

impl Default for DiagnosticBus {
    fn default() -> Self {
        let (wake_sender, wake_receiver) = async_channel::bounded(1);
        Self {
            pending: Arc::new(Mutex::new(PendingDiagnostics::default())),
            wake_sender,
            wake_receiver,
        }
    }
}

impl DiagnosticBus {
    pub(in crate::window) fn publish(&self, mut update: DiagnosticUpdate) {
        update
            .params
            .diagnostics
            .truncate(MAX_DIAGNOSTICS_PER_DOCUMENT);
        for diagnostic in &mut update.params.diagnostics {
            truncate_string(&mut diagnostic.message, MAX_DIAGNOSTIC_MESSAGE_BYTES);
            if let Some(source) = &mut diagnostic.source {
                truncate_string(source, MAX_DIAGNOSTIC_FIELD_BYTES);
            }
            if let Some(NumberOrString::String(code)) = &mut diagnostic.code {
                truncate_string(code, MAX_DIAGNOSTIC_FIELD_BYTES);
            }
            diagnostic.code_description = None;
            diagnostic.related_information = None;
            diagnostic.tags = None;
            diagnostic.data = None;
        }
        let key = update.key();
        let size = estimate_update_bytes(&update);
        if size > MAX_PENDING_BYTES {
            tracing::warn!("dropping oversized LSP diagnostics update");
            return;
        }
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(replaced) = pending.updates.remove(&key) {
            pending.bytes = pending
                .bytes
                .saturating_sub(estimate_update_bytes(&replaced));
        }
        while pending.updates.len() >= MAX_PENDING_DOCUMENTS
            || pending.bytes.saturating_add(size) > MAX_PENDING_BYTES
        {
            let Some(oldest) = pending.updates.keys().next().cloned() else {
                break;
            };
            if let Some(removed) = pending.updates.remove(&oldest) {
                pending.bytes = pending
                    .bytes
                    .saturating_sub(estimate_update_bytes(&removed));
            }
        }
        pending.bytes = pending.bytes.saturating_add(size);
        pending.updates.insert(key, update);
        let _ = self.wake_sender.try_send(());
    }

    pub(in crate::window) fn server_stopped(&self, workspace: PathBuf, server_id: String) {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stopped_servers
            .insert((workspace, server_id));
        let _ = self.wake_sender.try_send(());
    }

    pub(super) async fn changed(&self) -> bool {
        self.wake_receiver.recv().await.is_ok()
    }

    pub(super) fn drain(&self) -> DiagnosticBatch {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let batch = DiagnosticBatch {
            updates: pending.updates.drain().map(|(_, update)| update).collect(),
            stopped_servers: pending.stopped_servers.drain().collect(),
        };
        pending.bytes = 0;
        batch
    }
}

fn estimate_update_bytes(update: &DiagnosticUpdate) -> usize {
    update.workspace.as_os_str().as_encoded_bytes().len()
        + update.server_id.len()
        + update.params.uri.as_str().len()
        + update
            .params
            .diagnostics
            .iter()
            .map(|diagnostic| {
                diagnostic.message.len()
                    + diagnostic.source.as_ref().map_or(0, String::len)
                    + diagnostic.code.as_ref().map_or(0, |code| match code {
                        NumberOrString::Number(_) => 8,
                        NumberOrString::String(code) => code.len(),
                    })
                    + 64
            })
            .sum::<usize>()
}

fn truncate_string(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut boundary = max_bytes.saturating_sub(3);
    while !value.is_char_boundary(boundary) {
        boundary = boundary.saturating_sub(1);
    }
    value.truncate(boundary);
    value.push_str("...");
}

