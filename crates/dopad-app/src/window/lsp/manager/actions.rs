use std::path::PathBuf;

use futures_lite::future::race;
use gpui_kit::Context;

use super::{
    PromptPhase, ServerBinary, ServerSpec, installation, persist_metadata, persistence, registry,
    spec_key,
};
use crate::window::DopadWindow;

enum InstallEvent {
    Progress(Result<installation::InstallProgress, async_channel::RecvError>),
    Finished(Result<anyhow::Result<installation::InstalledServer>, async_channel::RecvError>),
}

impl DopadWindow {
    pub(in crate::window) fn cancel_lsp_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(prompt) = self.lsp.current_prompt.take() else {
            return;
        };
        if matches!(
            prompt.phase,
            PromptPhase::Installing { .. } | PromptPhase::Starting
        ) {
            self.lsp.current_prompt = Some(prompt);
            return;
        }
        self.lsp.dismissed.insert(prompt.spec.id.to_owned());
        self.lsp.next_prompt();
        cx.notify();
    }

    pub(in crate::window) fn install_lsp_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(mut prompt) = self.lsp.current_prompt.take() else {
            return;
        };
        if prompt.spec.method == registry::InstallMethod::Manual {
            self.lsp.current_prompt = Some(prompt);
            return;
        }
        if matches!(
            prompt.phase,
            PromptPhase::Installing { .. } | PromptPhase::Starting
        ) {
            self.lsp.current_prompt = Some(prompt);
            return;
        }
        let Some(data_dir) = crate::settings::app_data_dir() else {
            prompt.phase =
                PromptPhase::Error("Dopad could not resolve its user data directory.".to_owned());
            self.lsp.current_prompt = Some(prompt);
            cx.notify();
            return;
        };
        prompt.phase = PromptPhase::Installing {
            message: "Preparing installation".to_owned(),
            progress: None,
        };
        let spec = prompt.spec;
        let target_document = prompt.target_document;
        self.lsp.current_prompt = Some(prompt);
        let (cancel_tx, cancel_rx) = async_channel::bounded(1);
        self.lsp.install_cancel = Some(cancel_tx);
        cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            let (progress_tx, progress_rx) = async_channel::bounded(16);
            let install_executor = executor.clone();
            let (finished_tx, finished_rx) = async_channel::bounded(1);
            executor
                .clone()
                .spawn(async move {
                    let result = installation::install(
                        spec,
                        data_dir,
                        cancel_rx,
                        progress_tx,
                        &install_executor,
                    )
                    .await;
                    let _ = finished_tx.send(result).await;
                })
                .detach();
            let result = loop {
                let event = race(
                    async { InstallEvent::Progress(progress_rx.recv().await) },
                    async { InstallEvent::Finished(finished_rx.recv().await) },
                )
                .await;
                match event {
                    InstallEvent::Progress(Ok(progress)) => {
                        let _ = this.update(cx, |window, cx| {
                            let Some(prompt) = window
                                .lsp
                                .current_prompt
                                .as_mut()
                                .filter(|prompt| prompt.spec.id == spec.id)
                            else {
                                return;
                            };
                            if !matches!(prompt.phase, PromptPhase::Installing { .. }) {
                                return;
                            }
                            let changed = !matches!(
                                &prompt.phase,
                                PromptPhase::Installing { message, progress: current }
                                    if message == &progress.message && current == &progress.fraction
                            );
                            if changed {
                                prompt.phase = PromptPhase::Installing {
                                    message: progress.message,
                                    progress: progress.fraction,
                                };
                                cx.notify();
                            }
                        });
                    }
                    InstallEvent::Progress(Err(_)) => {
                        break match finished_rx.recv().await {
                            Ok(result) => result,
                            Err(_) => Err(anyhow::anyhow!("LSP installer ended without a result")),
                        };
                    }
                    InstallEvent::Finished(Ok(result)) => break result,
                    InstallEvent::Finished(Err(_)) => {
                        break Err(anyhow::anyhow!("LSP installer ended without a result"));
                    }
                }
            };
            let _ = this.update(cx, |window, cx| {
                window.lsp.install_cancel.take();
                match result {
                    Ok(installed) => {
                        let metadata = persistence::verified_now(
                            spec.id,
                            installed.version.clone(),
                            installed.executable.clone(),
                            &installed.args,
                            installed.runtime_version.clone(),
                            &installed.source,
                        );
                        window.lsp.metadata.insert(spec_key(spec), metadata);
                        persist_metadata(
                            window.lsp.metadata_path.clone(),
                            window.lsp.metadata.clone(),
                            cx,
                        );
                        window.lsp.binaries.insert(
                            spec_key(spec),
                            ServerBinary {
                                executable: installed.executable,
                                args: installed.args,
                                version: installed.version,
                                runtime_version: installed.runtime_version,
                                source: installed.source,
                            },
                        );
                        if let Some(prompt) = window
                            .lsp
                            .current_prompt
                            .as_mut()
                            .filter(|p| p.spec.id == spec.id)
                        {
                            prompt.phase = PromptPhase::Starting;
                        }
                        if let Some(document_id) = target_document
                            && let Some((path, language)) = window
                                .tabs
                                .iter()
                                .find(|tab| tab.id == document_id)
                                .and_then(|tab| tab.path.clone().map(|path| (path, tab.language())))
                        {
                            window.ensure_lsp_document(document_id, path, language, spec, cx);
                        } else {
                            window.ensure_active_lsp(cx);
                        }
                        cx.notify();
                    }
                    Err(error) => {
                        if let Some(prompt) = window
                            .lsp
                            .current_prompt
                            .as_mut()
                            .filter(|p| p.spec.id == spec.id)
                        {
                            prompt.phase = PromptPhase::Error(error.to_string());
                        }
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(in crate::window) fn retry_lsp_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(prompt) = self.lsp.current_prompt.clone() else {
            return;
        };
        if let PromptPhase::Error(_) = prompt.phase {
            self.lsp.current_prompt = None;
            if let Some(document_id) = prompt.target_document
                && let Some(tab) = self.tabs.iter().find(|tab| tab.id == document_id)
                && let (Some(path), language) = (tab.path.clone(), tab.language())
            {
                if let Some(binary) = self.lsp.binaries.get(&spec_key(prompt.spec)).cloned() {
                    self.start_document_server(
                        document_id,
                        path,
                        language,
                        prompt.spec,
                        binary,
                        cx,
                    );
                    cx.notify();
                    return;
                }
                self.lsp.checked.remove(prompt.spec.id);
                self.ensure_lsp_document(document_id, path, language, prompt.spec, cx);
                cx.notify();
                return;
            }
            self.lsp.checked.remove(prompt.spec.id);
            self.ensure_active_lsp(cx);
            cx.notify();
        } else {
            self.install_lsp_prompt(cx);
        }
    }

    pub(in crate::window) fn open_lsp_install_guide(&self, cx: &gpui_kit::App) {
        if let Some(prompt) = self.lsp.current_prompt.as_ref() {
            let url = prompt.spec.manual_url;
            cx.background_executor().spawn(async move {
                if let Err(error) = open::that(url) {
                    tracing::warn!(error = %error, "could not open language server installation guide");
                }
            }).detach();
        }
    }

    pub(super) fn dismiss_lsp_prompt(&mut self, server_id: &str) {
        if self
            .lsp
            .current_prompt
            .as_ref()
            .is_some_and(|prompt| prompt.spec.id == server_id)
        {
            self.lsp.current_prompt = None;
            self.lsp.next_prompt();
        }
        self.lsp
            .prompts
            .retain(|prompt| prompt.spec.id != server_id);
    }

    fn ensure_lsp_document(
        &mut self,
        id: u64,
        path: PathBuf,
        language: dopad_syntax::Language,
        spec: &'static ServerSpec,
        cx: &mut Context<Self>,
    ) {
        let Some(binary) = self.lsp.binaries.get(&spec_key(spec)).cloned() else {
            self.ensure_active_lsp(cx);
            return;
        };
        self.start_document_server(id, path, language, spec, binary, cx);
    }
}
