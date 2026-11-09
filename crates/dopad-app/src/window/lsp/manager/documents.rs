use std::{ops::Range, path::PathBuf, rc::Rc};

use gpui_kit::Context;
use lsp_types::CompletionItem;

use super::{
    ClientKey, InstallPrompt, LspClient, LspDocument, PromptPhase, ServerBinary, ServerSpec,
    detect_server, language_id, persist_metadata, persistence, registry, spec_key, workspace_root,
};
use crate::window::{DopadWindow, file_view};

struct AttachLspDocument {
    document_id: u64,
    language: dopad_syntax::Language,
    path: PathBuf,
    spec: &'static ServerSpec,
    client: LspClient,
    workspace: PathBuf,
}

fn completion_highlight_ranges(_: &str, _: &CompletionItem) -> Vec<Range<usize>> {
    Vec::new()
}

impl DopadWindow {
    pub(in crate::window) fn ensure_active_lsp(&mut self, cx: &mut Context<Self>) {
        if !self.lsp.delay_elapsed {
            return;
        }
        let Some((document_id, path, language, language_overridden, has_editor)) =
            self.active_tab().and_then(|tab| {
                Some((
                    tab.id,
                    tab.path.clone()?,
                    tab.language(),
                    tab.language_override.is_some(),
                    matches!(tab.content, file_view::TabContent::Text) && tab.editor.is_some(),
                ))
            })
        else {
            return;
        };
        if !has_editor || self.lsp.documents.contains_key(&document_id) {
            return;
        }
        let Some(spec) = (if language_overridden {
            registry::for_language(language)
        } else {
            registry::for_file(language, &path)
        }) else {
            return;
        };
        if self.lsp.dismissed.contains(spec.id)
            || self.lsp.starting_documents.contains(&document_id)
        {
            return;
        }
        if let Some(binary) = self.lsp.binaries.get(&spec_key(spec)).cloned() {
            self.start_document_server(document_id, path, language, spec, binary, cx);
            return;
        }
        if self.lsp.checked.contains(spec.id) {
            return;
        }
        if !self.lsp.checking.insert(spec.id.to_owned()) {
            return;
        }
        self.lsp.starting_documents.insert(document_id);
        let metadata_path = self.lsp.metadata_path.clone().or_else(|| {
            crate::settings::app_data_dir()
                .map(|directory| directory.join("lsp").join("servers.json"))
        });
        self.lsp.metadata_path = metadata_path.clone();
        let persisted = self.lsp.metadata.clone();
        let metadata_loaded = self.lsp.metadata_loaded;
        cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            let result = executor
                .clone()
                .spawn(async move {
                    detect_server(spec, persisted, metadata_loaded, metadata_path, &executor).await
                })
                .await;
            let _ = this.update(cx, |window, cx| {
                window.lsp.checking.remove(spec.id);
                window.lsp.starting_documents.remove(&document_id);
                window.lsp.checked.insert(spec.id.to_owned());
                match result {
                    Ok(detection) => {
                        if !detection.metadata.is_empty() || !window.lsp.metadata_loaded {
                            window.lsp.metadata = detection.metadata;
                        }
                        window.lsp.metadata_loaded = true;
                        if let Some(binary) = detection.binary {
                            window.lsp.binaries.insert(spec_key(spec), binary);
                            window.ensure_active_lsp(cx);
                        } else {
                            let phase = if spec.method == registry::InstallMethod::Manual {
                                PromptPhase::Manual
                            } else {
                                PromptPhase::Missing
                            };
                            window.lsp.enqueue_prompt(InstallPrompt {
                                spec,
                                phase,
                                target_document: Some(document_id),
                            });
                            cx.notify();
                        }
                    }
                    Err(error) => {
                        window.lsp.enqueue_prompt(InstallPrompt {
                            spec,
                            phase: PromptPhase::Error(error.to_string()),
                            target_document: Some(document_id),
                        });
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn start_document_server(
        &mut self,
        document_id: u64,
        path: PathBuf,
        language: dopad_syntax::Language,
        spec: &'static ServerSpec,
        binary: ServerBinary,
        cx: &mut Context<Self>,
    ) {
        self.start_diagnostic_listener(cx);
        if !self.lsp.starting_documents.insert(document_id) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            let path_for_root = path.clone();
            let root = executor
                .clone()
                .spawn(async move { workspace_root(&path_for_root) })
                .await;
            let key = ClientKey {
                server_id: spec.id.to_owned(),
                root: root.clone(),
            };
            let existing = this
                .update(cx, |window, _| window.lsp.clients.get(&key).cloned())
                .ok()
                .flatten();
            if let Some(client) = existing {
                let _ = this.update(cx, |window, cx| {
                    window.lsp.starting_documents.remove(&document_id);
                    window.attach_lsp_document(
                        AttachLspDocument {
                            document_id,
                            language,
                            path: path.clone(),
                            spec,
                            client,
                            workspace: root.clone(),
                        },
                        cx,
                    );
                });
                return;
            }
            let should_start = this
                .update(cx, |window, _| {
                    !window.lsp.clients.contains_key(&key)
                        && window.lsp.starting_clients.insert(key.clone())
                })
                .unwrap_or(false);
            if !should_start {
                let _ = this.update(cx, |window, _| {
                    window.lsp.starting_documents.remove(&document_id);
                });
                return;
            }
            let executable = binary.executable.clone();
            let args = spec.args;
            let metadata_key = spec_key(spec);
            let diagnostics = this
                .update(cx, |window, _| window.diagnostic_bus.clone())
                .unwrap_or_default();
            let server_id = spec.id.to_owned();
            let result = executor
                .clone()
                .spawn(async move {
                    LspClient::start(executable, args, &root, &executor, diagnostics, server_id)
                        .await
                })
                .await;
            let abandoned_client = result.as_ref().ok().cloned();
            let updated = this.update(cx, |window, cx| {
                window.lsp.starting_clients.remove(&key);
                window.lsp.starting_documents.remove(&document_id);
                match result {
                    Ok(client) => {
                        let client_root = key.root.clone();
                        window.lsp.clients.insert(key, client.clone());
                        let metadata = persistence::verified_now(
                            spec.id,
                            binary.version,
                            binary.executable,
                            &binary.source,
                        );
                        window.lsp.metadata.insert(spec_key(spec), metadata);
                        persist_metadata(
                            window.lsp.metadata_path.clone(),
                            window.lsp.metadata.clone(),
                            cx,
                        );
                        window.attach_lsp_document(
                            AttachLspDocument {
                                document_id,
                                language,
                                path: path.clone(),
                                spec,
                                client,
                                workspace: client_root,
                            },
                            cx,
                        );
                        window.dismiss_lsp_prompt(spec.id);
                        window.ensure_active_lsp(cx);
                    }
                    Err(error) => {
                        window.lsp.binaries.remove(&metadata_key);
                        if window.lsp.metadata.remove(&metadata_key).is_some() {
                            persist_metadata(
                                window.lsp.metadata_path.clone(),
                                window.lsp.metadata.clone(),
                                cx,
                            );
                        }
                        window.lsp.enqueue_prompt(InstallPrompt {
                            spec,
                            phase: PromptPhase::Error(format!(
                                "Could not start {}: {error:#}",
                                spec.name
                            )),
                            target_document: Some(document_id),
                        });
                        cx.notify();
                    }
                }
            });
            if updated.is_err()
                && let Some(client) = abandoned_client
            {
                client.close();
            }
        })
        .detach();
    }

    fn attach_lsp_document(&mut self, attach: AttachLspDocument, cx: &mut Context<Self>) {
        let AttachLspDocument {
            document_id,
            language,
            path,
            spec,
            client,
            workspace,
        } = attach;
        if self.lsp.documents.contains_key(&document_id) {
            return;
        }
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == document_id) else {
            return;
        };
        if !matches!(tab.content, file_view::TabContent::Text) {
            return;
        }
        let Some(ui) = tab.ui.clone() else {
            self.lsp.starting_documents.remove(&document_id);
            return;
        };
        let uri = match url::Url::from_file_path(&path) {
            Ok(uri) => uri.to_string(),
            Err(_) => {
                tracing::warn!(path = %path.display(), "LSP cannot represent this file path as a URI");
                return;
            }
        };
        let language_id = language_id(language, &path, spec);
        let document = LspDocument::new(client, uri, language_id, spec.id.to_owned(), workspace);
        let provider = document.provider();
        let rope = ui.read(cx).text().clone();
        ui.update(cx, |state, _| {
            let lsp = state.lsp_mut();
            lsp.completion_provider = Some(provider);
            lsp.set_completion_highlighter(Rc::new(completion_highlight_ranges));
        });
        self.lsp.documents.insert(document_id, document.clone());
        let executor = cx.background_executor().clone();
        executor
            .spawn(async move {
                let text = rope.to_string();
                if let Err(error) = document.did_open(text) {
                    tracing::warn!(error = %error, "could not send LSP didOpen");
                }
            })
            .detach();
        cx.notify();
    }

    pub(in crate::window) fn sync_lsp_change(
        &self,
        document_id: u64,
        range: Option<lsp_types::Range>,
        range_length: Option<u32>,
        inserted: String,
    ) {
        if let Some(document) = self.lsp.documents.get(&document_id)
            && let Err(error) = document.did_change(range, range_length, inserted)
        {
            tracing::debug!(document_id, error = %error, "LSP change notification was not queued");
        }
    }

    pub(in crate::window) fn close_lsp_document(&mut self, document_id: u64) {
        if let Some(uri) = self.lsp.close_document(document_id) {
            self.diagnostics.remove_uri(&uri);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::completion_highlight_ranges;
    use lsp_types::CompletionItem;

    #[test]
    fn completion_labels_inherit_theme_text_instead_of_blue_match_highlights() {
        let item = CompletionItem {
            label: "publish".to_owned(),
            ..CompletionItem::default()
        };

        assert!(completion_highlight_ranges("pub", &item).is_empty());
    }
}

