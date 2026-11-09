mod bus;

pub(super) use bus::{DiagnosticBus, DiagnosticUpdate};

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use lsp_types::{Diagnostic, DiagnosticSeverity};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DiagnosticKey {
    workspace: PathBuf,
    server_id: String,
    uri: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct DiagnosticCounts {
    pub errors: usize,
    pub warnings: usize,
    pub total: usize,
}

#[derive(Default)]
pub(super) struct DiagnosticStore {
    documents: HashMap<DiagnosticKey, StoredDiagnostics>,
    workspace_counts: HashMap<PathBuf, DiagnosticCounts>,
    workspace_revisions: HashMap<PathBuf, u64>,
    revision: u64,
}

#[derive(Clone)]
pub(super) struct StoredDiagnostics {
    pub workspace: PathBuf,
    pub server_id: String,
    pub path: PathBuf,
    pub version: Option<i32>,
    pub diagnostics: Arc<[Diagnostic]>,
}

impl DiagnosticStore {
    pub(super) fn revision_for(&self, workspace: Option<&PathBuf>) -> u64 {
        workspace
            .and_then(|workspace| self.workspace_revisions.get(workspace).copied())
            .unwrap_or_default()
    }

    pub(super) fn counts(&self, workspace: Option<&PathBuf>) -> DiagnosticCounts {
        workspace
            .and_then(|workspace| self.workspace_counts.get(workspace).copied())
            .unwrap_or_default()
    }

    pub(super) fn apply(&mut self, update: DiagnosticUpdate, current_version: Option<i32>) -> bool {
        let key = update.key();
        let incoming_version = update.params.version;
        if let (Some(incoming), Some(current)) = (incoming_version, current_version)
            && incoming < current
        {
            return false;
        }

        let Some(path) = url::Url::parse(update.params.uri.as_str())
            .ok()
            .and_then(|uri| uri.to_file_path().ok())
        else {
            tracing::debug!(
                uri = update.params.uri.as_str(),
                "ignoring non-file LSP diagnostics URI"
            );
            return false;
        };
        let old = self.documents.get(&key);
        if old.is_some_and(|old| {
            old.version
                .zip(incoming_version)
                .is_some_and(|(old, incoming)| incoming < old)
        }) {
            return false;
        }
        if old.is_some_and(|old| old.diagnostics.as_ref() == update.params.diagnostics.as_slice()) {
            if let Some(old) = self.documents.get_mut(&key) {
                old.version = incoming_version.or(old.version);
            }
            return false;
        }

        if let Some(old) = self.documents.remove(&key) {
            update_counts(
                &mut self.workspace_counts,
                &old.workspace,
                count_diagnostics(&old.diagnostics),
                false,
            );
        }
        let workspace = update.workspace.clone();
        let diagnostics: Arc<[Diagnostic]> = update.params.diagnostics.into();
        update_counts(
            &mut self.workspace_counts,
            &workspace,
            count_diagnostics(&diagnostics),
            true,
        );
        self.documents.insert(
            key,
            StoredDiagnostics {
                workspace: update.workspace,
                server_id: update.server_id,
                path,
                version: incoming_version,
                diagnostics,
            },
        );
        self.revision = self.revision.wrapping_add(1);
        bump_revision(&mut self.workspace_revisions, &workspace);
        true
    }

    pub(super) fn remove_uri(&mut self, uri: &str) -> bool {
        let keys = self
            .documents
            .keys()
            .filter(|key| key.uri == uri)
            .cloned()
            .collect::<Vec<_>>();
        if keys.is_empty() {
            return false;
        }
        for key in keys {
            if let Some(document) = self.documents.remove(&key) {
                bump_revision(&mut self.workspace_revisions, &document.workspace);
                update_counts(
                    &mut self.workspace_counts,
                    &document.workspace,
                    count_diagnostics(&document.diagnostics),
                    false,
                );
            }
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub(super) fn remove_server_workspace(&mut self, workspace: &PathBuf, server_id: &str) -> bool {
        let keys = self
            .documents
            .keys()
            .filter(|key| &key.workspace == workspace && key.server_id == server_id)
            .cloned()
            .collect::<Vec<_>>();
        if keys.is_empty() {
            return false;
        }
        for key in keys {
            if let Some(document) = self.documents.remove(&key) {
                update_counts(
                    &mut self.workspace_counts,
                    &document.workspace,
                    count_diagnostics(&document.diagnostics),
                    false,
                );
            }
        }
        bump_revision(&mut self.workspace_revisions, workspace);
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub(super) fn documents_for(
        &self,
        workspace: Option<&PathBuf>,
    ) -> impl Iterator<Item = &StoredDiagnostics> {
        self.documents.values().filter(move |document| {
            workspace.is_none_or(|workspace| &document.workspace == workspace)
        })
    }
}

fn bump_revision(revisions: &mut HashMap<PathBuf, u64>, workspace: &Path) {
    let revision = revisions.entry(workspace.to_path_buf()).or_default();
    *revision = revision.wrapping_add(1);
}

impl super::DopadWindow {
    pub(super) fn start_diagnostic_listener(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if self.diagnostics_listener_started {
            return;
        }
        self.diagnostics_listener_started = true;
        let bus = self.diagnostic_bus.clone();
        cx.spawn(async move |this, cx| {
            while bus.changed().await {
                let batch = bus.drain();
                if batch.updates.is_empty() && batch.stopped_servers.is_empty() {
                    continue;
                }
                let _ = this.update(cx, |window, cx| {
                    let workspace = window.current_diagnostic_workspace();
                    let before = window.diagnostics.counts(workspace.as_ref());
                    let mut changed = false;
                    for update in batch.updates {
                        let uri = update.params.uri.as_str().to_owned();
                        let version = window.lsp.current_document_version(
                            &uri,
                            &update.server_id,
                            &update.workspace,
                        );
                        let Some(version) = version else {
                            continue;
                        };
                        changed |= window.diagnostics.apply(update, Some(version));
                    }
                    for (workspace, server_id) in batch.stopped_servers {
                        changed |= window
                            .diagnostics
                            .remove_server_workspace(&workspace, &server_id);
                    }
                    if !changed {
                        return;
                    }
                    let after = window.diagnostics.counts(workspace.as_ref());
                    let problems_open = window
                        .bottom_panel
                        .is_active(super::bottom_panel::BottomPanelView::Problems);
                    if problems_open {
                        window.bottom_panel.invalidate_problems();
                    }
                    if problems_open || before != after {
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    pub(super) fn open_problem(
        &mut self,
        path: PathBuf,
        line: usize,
        column: usize,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        let opened = self.open_paths_internal(vec![path.clone()], false, cx);
        let Some(document_id) = opened.first().copied() else {
            return;
        };
        let Some(index) = self.tabs.iter().position(|tab| tab.id == document_id) else {
            return;
        };
        self.active = index;
        if self.tabs[index].editor.is_some() {
            self.go_to_lsp_position(line, column, cx);
        } else {
            self.pending_problem_navigation.insert(path, (line, column));
        }
        cx.notify();
    }

    pub(super) fn apply_pending_problem_navigation(
        &mut self,
        path: &std::path::Path,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        let pending_path = self
            .pending_problem_navigation
            .keys()
            .find(|pending| super::paths::same_path(pending, path))
            .cloned();
        let Some(pending_path) = pending_path else {
            return;
        };
        let Some((line, column)) = self.pending_problem_navigation.remove(&pending_path) else {
            return;
        };
        let Some(index) = self.tabs.iter().position(|tab| {
            tab.path
                .as_deref()
                .is_some_and(|tab_path| super::paths::same_path(tab_path, path))
        }) else {
            return;
        };
        self.active = index;
        self.go_to_lsp_position(line, column, cx);
    }

    pub(super) fn discard_pending_problem_navigation(&mut self, path: &std::path::Path) {
        if let Some(key) = self
            .pending_problem_navigation
            .keys()
            .find(|key| super::paths::same_path(key, path))
            .cloned()
        {
            self.pending_problem_navigation.remove(&key);
        }
    }
}

fn count_diagnostics(diagnostics: &[Diagnostic]) -> DiagnosticCounts {
    diagnostics
        .iter()
        .fold(DiagnosticCounts::default(), |mut counts, diagnostic| {
            match diagnostic.severity {
                Some(DiagnosticSeverity::ERROR) => counts.errors += 1,
                Some(DiagnosticSeverity::WARNING) => counts.warnings += 1,
                _ => {}
            }
            counts.total += 1;
            counts
        })
}

fn update_counts(
    counts: &mut HashMap<PathBuf, DiagnosticCounts>,
    workspace: &PathBuf,
    delta: DiagnosticCounts,
    add: bool,
) {
    let is_empty = {
        let entry = counts.entry(workspace.clone()).or_default();
        if add {
            entry.errors = entry.errors.saturating_add(delta.errors);
            entry.warnings = entry.warnings.saturating_add(delta.warnings);
            entry.total = entry.total.saturating_add(delta.total);
        } else {
            entry.errors = entry.errors.saturating_sub(delta.errors);
            entry.warnings = entry.warnings.saturating_sub(delta.warnings);
            entry.total = entry.total.saturating_sub(delta.total);
        }
        entry.total == 0
    };
    if is_empty {
        counts.remove(workspace);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

    use super::bus::{MAX_DIAGNOSTIC_MESSAGE_BYTES, MAX_DIAGNOSTICS_PER_DOCUMENT};
    use super::{DiagnosticBus, DiagnosticCounts, DiagnosticStore, DiagnosticUpdate};

    fn file_uri(path: &Path) -> lsp_types::Uri {
        let url = url::Url::from_file_path(path).expect("test path is a file URI");
        url.as_str().parse().expect("valid LSP URI")
    }

    fn update(workspace: &Path, diagnostics: Vec<Diagnostic>) -> DiagnosticUpdate {
        DiagnosticUpdate {
            workspace: workspace.to_path_buf(),
            server_id: "rust-analyzer".into(),
            params: lsp_types::PublishDiagnosticsParams {
                uri: file_uri(&workspace.join("src/main.rs")),
                diagnostics,
                version: Some(3),
            },
        }
    }

    fn diagnostic(severity: DiagnosticSeverity, message: &str) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            severity: Some(severity),
            code: Some(NumberOrString::String("test-code".into())),
            source: Some("test-server".into()),
            message: message.into(),
            ..Diagnostic::default()
        }
    }

    #[test]
    fn store_updates_counts_and_clears_diagnostics_by_uri() {
        let workspace = std::env::temp_dir().join("dopad-problems-test");
        let mut store = DiagnosticStore::default();
        assert!(store.apply(
            update(
                &workspace,
                vec![
                    diagnostic(DiagnosticSeverity::ERROR, "error"),
                    diagnostic(DiagnosticSeverity::WARNING, "warning"),
                ],
            ),
            Some(3),
        ));
        assert_eq!(
            store.counts(Some(&workspace)),
            DiagnosticCounts {
                errors: 1,
                warnings: 1,
                total: 2,
            }
        );
        let uri = url::Url::from_file_path(workspace.join("src/main.rs"))
            .expect("test path is a file URI")
            .to_string();
        assert!(store.remove_uri(&uri));
        assert_eq!(store.counts(Some(&workspace)), DiagnosticCounts::default());
    }

    #[test]
    fn store_ignores_stale_and_duplicate_updates() {
        let mut store = DiagnosticStore::default();
        let workspace = std::env::temp_dir().join("dopad-stale-diagnostics-test");
        let current = update(&workspace, vec![diagnostic(DiagnosticSeverity::ERROR, "e")]);
        assert!(!store.apply(current.clone(), Some(4)));
        assert!(store.apply(current.clone(), Some(3)));
        assert!(!store.apply(current, Some(3)));
    }

    #[test]
    fn pending_updates_coalesce_by_document() {
        let bus = DiagnosticBus::default();
        let workspace = std::env::temp_dir().join("dopad-coalesced-diagnostics-test");
        bus.publish(update(
            &workspace,
            vec![diagnostic(DiagnosticSeverity::ERROR, "old")],
        ));
        bus.publish(update(
            &workspace,
            vec![diagnostic(DiagnosticSeverity::WARNING, "new")],
        ));
        let pending = bus.drain();
        assert_eq!(pending.updates.len(), 1);
        assert_eq!(pending.updates[0].params.diagnostics[0].message, "new");
    }

    #[test]
    fn pending_updates_bound_diagnostic_count_and_message_size() {
        let bus = DiagnosticBus::default();
        let workspace = std::env::temp_dir().join("dopad-limited-diagnostics-test");
        let mut update = update(
            &workspace,
            (0..MAX_DIAGNOSTICS_PER_DOCUMENT + 8)
                .map(|_| {
                    diagnostic(
                        DiagnosticSeverity::ERROR,
                        &"x".repeat(MAX_DIAGNOSTIC_MESSAGE_BYTES * 2),
                    )
                })
                .collect(),
        );
        update.params.diagnostics[0].data = Some(serde_json::json!({"large": "discarded"}));
        bus.publish(update);
        let batch = bus.drain();
        assert_eq!(
            batch.updates[0].params.diagnostics.len(),
            MAX_DIAGNOSTICS_PER_DOCUMENT
        );
        assert!(
            batch.updates[0].params.diagnostics[0].message.len() <= MAX_DIAGNOSTIC_MESSAGE_BYTES
        );
        assert!(batch.updates[0].params.diagnostics[0].data.is_none());
    }

    #[test]
    fn server_restart_removes_only_that_server_diagnostics() {
        let workspace = std::env::temp_dir().join("dopad-lsp-restart-diagnostics-test");
        let mut store = DiagnosticStore::default();
        let first = update(
            &workspace,
            vec![diagnostic(DiagnosticSeverity::ERROR, "one")],
        );
        let mut second = first.clone();
        second.server_id = "typescript-language-server".into();
        assert!(store.apply(first, Some(3)));
        assert!(store.apply(second, Some(3)));
        assert_eq!(store.counts(Some(&workspace)).errors, 2);
        assert!(store.remove_server_workspace(&workspace, "rust-analyzer"));
        assert_eq!(store.counts(Some(&workspace)).errors, 1);
        assert_eq!(
            store
                .documents_for(Some(&workspace))
                .next()
                .map(|doc| doc.server_id.as_str()),
            Some("typescript-language-server")
        );
    }
}

