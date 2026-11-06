use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use dopad_core::TextPosition;
use dopad_editor::SearchOptions;
use gpui_kit::Context;
use rfd::AsyncFileDialog;

use super::super::super::{DopadWindow, file_search, preview};
use super::super::model::{SearchPanelState, SearchScope};

pub(super) struct SearchNavigation {
    pub path: PathBuf,
    pub document_id: Option<u64>,
    pub revision: Option<dopad_core::Revision>,
    pub disk_identity: Option<file_search::DiskFileIdentity>,
    pub position: TextPosition,
    pub match_columns: usize,
}

pub(super) fn deactivate(search: &mut SearchPanelState) {
    if let Some(cancelled) = search.cancel.take() {
        cancelled.store(true, Ordering::Relaxed);
    }
    search.generation = search.generation.wrapping_add(1);
    search.running = false;
    search.status.clear();
    search.results.clear();
    search.collapsed.clear();
    search.summary = None;
    search.input = None;
    search.filter = None;
}

impl DopadWindow {
    pub(in crate::window) fn mark_search_stale(&mut self, cx: &mut Context<Self>) {
        self.sidebar.search.status = "File changed since search; run the search again".into();
        cx.notify();
    }

    pub(super) fn start_file_search(&mut self, cx: &mut Context<Self>) {
        let Some(input) = self.sidebar.search.input.as_ref() else {
            return;
        };
        let query = input.read(cx).value().to_string();
        if query.is_empty() {
            self.sidebar.search.status = "Enter a search term".into();
            self.sidebar.search.results.clear();
            cx.notify();
            return;
        }
        if let Some(cancel) = self.sidebar.search.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }

        let scope = self.sidebar.search.scope;
        let open_buffers = self.open_search_buffers(scope);
        let roots = self.search_roots(scope);
        if roots.is_empty() && open_buffers.is_empty() {
            self.sidebar.search.status = match scope {
                SearchScope::CurrentFolder => {
                    "Open a folder in Explorer before searching this scope".into()
                }
                SearchScope::CustomFolder => "Choose a custom folder before searching".into(),
                SearchScope::CurrentFile => "The current document has no searchable content".into(),
                SearchScope::OpenFiles => "No open files to search".into(),
            };
            cx.notify();
            return;
        }

        let options = SearchOptions {
            query,
            regex: self.sidebar.search.regex,
            case_sensitive: self.sidebar.search.match_case,
            whole_word: self.sidebar.search.whole_word,
            max_matches: 100_000,
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        self.sidebar.search.cancel = Some(cancelled.clone());
        self.sidebar.search.running = true;
        self.sidebar.search.status = "Searching… 0 files".into();
        self.sidebar.search.results.clear();
        self.sidebar.search.summary = None;
        self.sidebar.search.generation = self.sidebar.search.generation.wrapping_add(1);
        let generation = self.sidebar.search.generation;
        let (progress_tx, progress_rx) = async_channel::bounded::<usize>(1);
        cx.notify();

        cx.spawn(async move |this, cx| {
            while let Ok(scanned) = progress_rx.recv().await {
                let _ = this.update(cx, |this, cx| {
                    if this.sidebar.search.generation == generation && this.sidebar.search.running {
                        this.sidebar.search.status = format!("Searching… {scanned} files");
                        cx.notify();
                    }
                });
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    file_search::search_files(roots, open_buffers, options, cancelled, progress_tx)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.sidebar.search.generation != generation {
                    return;
                }
                this.sidebar.search.running = false;
                this.sidebar.search.cancel = None;
                match result {
                    Ok(result) => {
                        this.sidebar.search.status = if result.match_count == 0 {
                            "No results found".into()
                        } else {
                            String::new()
                        };
                        this.sidebar.search.results = result.files.clone();
                        this.sidebar.search.summary = Some(result);
                    }
                    Err(file_search::SearchFilesError::Cancelled) => {
                        this.sidebar.search.status = "Search cancelled".into();
                    }
                    Err(error) => {
                        this.sidebar.search.status = format!("Could not search files: {error}");
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_search_buffers(&self, scope: SearchScope) -> Vec<file_search::OpenBuffer> {
        self.tabs
            .iter()
            .enumerate()
            .filter(|(index, tab)| match scope {
                SearchScope::CurrentFolder => {
                    self.sidebar.explorer.root.as_ref().is_some_and(|root| {
                        tab.path.as_ref().is_some_and(|path| path.starts_with(root))
                    })
                }
                SearchScope::OpenFiles => true,
                SearchScope::CurrentFile => *index == self.active,
                SearchScope::CustomFolder => {
                    self.sidebar
                        .search
                        .custom_root
                        .as_ref()
                        .is_some_and(|root| {
                            tab.path.as_ref().is_some_and(|path| path.starts_with(root))
                        })
                }
            })
            .filter_map(|(_, tab)| {
                let snapshot = tab.editor.as_ref()?.snapshot();
                Some(file_search::OpenBuffer {
                    path: tab
                        .path
                        .clone()
                        .unwrap_or_else(|| PathBuf::from(format!("{}.untitled", tab.id))),
                    display_name: tab.title.clone(),
                    document_id: tab.id,
                    revision: snapshot.revision(),
                    snapshot,
                })
            })
            .collect()
    }

    fn search_roots(&self, scope: SearchScope) -> Vec<PathBuf> {
        match scope {
            SearchScope::CurrentFolder if self.sidebar.search.in_current_folder => {
                self.sidebar.explorer.root.iter().cloned().collect()
            }
            SearchScope::CurrentFolder => Vec::new(),
            SearchScope::OpenFiles => self
                .tabs
                .iter()
                .filter_map(|tab| tab.path.clone())
                .collect(),
            SearchScope::CurrentFile => self
                .active_tab()
                .and_then(|tab| tab.path.clone())
                .into_iter()
                .collect(),
            SearchScope::CustomFolder => self.sidebar.search.custom_root.iter().cloned().collect(),
        }
    }

    pub(super) fn pick_search_folder(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let Some(folder) = AsyncFileDialog::new()
                .set_title("Search in Folder")
                .pick_folder()
                .await
            else {
                return;
            };
            let path = folder.path().to_path_buf();
            let _ = this.update(cx, |this, cx| {
                this.sidebar.search.scope = SearchScope::CustomFolder;
                this.sidebar.search.custom_root = Some(path);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn navigate_to_search_result(
        &mut self,
        navigation: SearchNavigation,
        cx: &mut Context<Self>,
    ) {
        let SearchNavigation {
            path,
            document_id,
            revision,
            disk_identity,
            position,
            match_columns,
        } = navigation;
        if let Some(document_id) = document_id {
            let Some(index) = self.tabs.iter().position(|tab| tab.id == document_id) else {
                self.sidebar.search.status = "Search result is stale; run the search again".into();
                cx.notify();
                return;
            };
            let Some(editor) = self.tabs[index].editor.as_ref() else {
                self.sidebar.search.status = "Search result is no longer a text document".into();
                cx.notify();
                return;
            };
            let snapshot = editor.snapshot();
            if revision.is_some_and(|revision| snapshot.revision() != revision) {
                self.sidebar.search.status = "Document changed; run the search again".into();
                cx.notify();
                return;
            }
            let start = snapshot
                .position_to_byte_clamped(position)
                .map_or(snapshot.len_bytes(), |offset| offset.0);
            let end = snapshot
                .position_to_byte_clamped(TextPosition::new(
                    position.line,
                    position.column.saturating_add(match_columns),
                ))
                .map_or(start, |offset| offset.0);
            preview::activate(self, index);
            self.tabs[index].last_selection = start.min(end)..end;
            if let Some(state) = self.tabs[index].ui.as_ref() {
                state.update(cx, |state, cx| {
                    state.set_selected_range(start.min(end)..end, cx)
                });
            }
            cx.notify();
            return;
        }

        self.pending_search_navigation =
            Some((path.clone(), position, match_columns, disk_identity));
        self.open_paths(vec![path], cx);
    }
}
