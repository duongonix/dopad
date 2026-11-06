use std::{
    fs::{self, OpenOptions},
    path::{Component, Path, PathBuf},
    process::Command,
};

use gpui_kit::component::{
    WindowExt as _,
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    input::{Input, InputState},
    menu::PopupMenuItem,
};
use gpui_kit::{AppContext as _, Context, ParentElement as _, Window, div};

use super::super::DopadWindow;

#[cfg(test)]
#[path = "operations/tests.rs"]
mod tests;

#[derive(Clone)]
pub(super) enum PathOperation {
    CreateFile(PathBuf),
    CreateFolder(PathBuf),
    Rename(PathBuf),
}

#[derive(Clone)]
pub(super) enum DestructiveOperation {
    Delete(PathBuf),
}

impl DopadWindow {
    pub(super) fn prompt_path_operation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        operation: PathOperation,
    ) {
        let (title, placeholder, initial) = match &operation {
            PathOperation::CreateFile(_) => ("New File", "File name", "".to_owned()),
            PathOperation::CreateFolder(_) => ("New Folder", "Folder name", "".to_owned()),
            PathOperation::Rename(path) => (
                "Rename",
                "New name",
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned(),
            ),
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        if !initial.is_empty() {
            input.update(cx, |state, cx| state.set_value(initial, window, cx));
        }
        let parent = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let input_for_create = input.clone();
            let parent_for_create = parent.clone();
            let action = operation.clone();
            let create = Button::new("path-operation-confirm")
                .label("Create")
                .primary()
                .on_click(move |_, window, cx| {
                    let name = input_for_create.read(cx).value().to_string();
                    window.close_dialog(cx);
                    let Some(parent) = parent_for_create.upgrade() else {
                        return;
                    };
                    let action = action.clone();
                    cx.update_entity(&parent, move |this: &mut DopadWindow, cx| {
                        this.perform_path_operation(action, name, cx);
                    });
                });
            let cancel = Button::new("path-operation-cancel")
                .label("Cancel")
                .ghost()
                .on_click(|_, window, cx| window.close_dialog(cx));
            dialog
                .title(title)
                .child(div().child(Input::new(&input)))
                .footer(DialogFooter::new().child(cancel).child(create))
        });
    }

    pub(super) fn confirm_delete(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        path: PathBuf,
    ) {
        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("this item")
            .to_owned();
        let parent = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let parent_for_delete = parent.clone();
            let path_for_delete = path.clone();
            let delete = Button::new("explorer-delete-confirm")
                .label("Delete")
                .danger()
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    let Some(parent) = parent_for_delete.upgrade() else {
                        return;
                    };
                    cx.update_entity(&parent, |this: &mut DopadWindow, cx| {
                        this.perform_destructive_operation(
                            DestructiveOperation::Delete(path_for_delete.clone()),
                            cx,
                        );
                    });
                });
            let cancel = Button::new("explorer-delete-cancel")
                .label("Cancel")
                .ghost()
                .on_click(|_, window, cx| window.close_dialog(cx));
            dialog
                .title("Delete item?")
                .child(div().child(format!(
                    "Delete {title}? Files inside a folder will also be removed."
                )))
                .footer(DialogFooter::new().child(cancel).child(delete))
        });
    }

    pub(super) fn perform_path_operation(
        &mut self,
        operation: PathOperation,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let name = match valid_name(&name) {
            Ok(name) => name,
            Err(error) => {
                self.set_status(error.to_string());
                cx.notify();
                return;
            }
        };
        let root = self.sidebar.explorer.root.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { apply_path_operation(operation, &name, root.as_deref()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(PathResult::CreatedFile(path)) => {
                        this.set_status(format!("Created {}", path.display()));
                        this.open_paths(vec![path.clone()], cx);
                        if this
                            .sidebar
                            .explorer
                            .root
                            .as_ref()
                            .is_some_and(|root| path.starts_with(root))
                        {
                            this.refresh_explorer(cx);
                        }
                    }
                    Ok(PathResult::CreatedFolder(path)) => {
                        this.set_status(format!("Created {}", path.display()));
                        if this
                            .sidebar
                            .explorer
                            .root
                            .as_ref()
                            .is_some_and(|root| path.starts_with(root))
                        {
                            this.refresh_explorer(cx);
                        }
                    }
                    Ok(PathResult::Renamed(source, destination)) => {
                        this.remap_renamed_path(&source, &destination, cx);
                        this.set_status(format!("Renamed to {}", destination.display()));
                    }
                    Err(error) => this.set_status(format!("File operation failed: {error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn perform_destructive_operation(
        &mut self,
        operation: DestructiveOperation,
        cx: &mut Context<Self>,
    ) {
        let root = self.sidebar.explorer.root.clone();
        let DestructiveOperation::Delete(path) = operation;
        let worker_path = path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { delete_path(&worker_path, root.as_deref()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.reconcile_deleted_path(&path, cx);
                        this.set_status(format!("Deleted {}", path.display()));
                        if this
                            .sidebar
                            .explorer
                            .root
                            .as_ref()
                            .is_some_and(|root| path.starts_with(root))
                        {
                            this.refresh_explorer(cx);
                        }
                    }
                    Err(error) => this.set_status(format!("Delete failed: {error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn reveal_in_file_manager(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { reveal_path(&path) })
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |this, cx| {
                    this.set_status(format!("Could not reveal file: {error}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn remap_renamed_path(&mut self, source: &Path, destination: &Path, cx: &mut Context<Self>) {
        for tab in &mut self.tabs {
            let Some(path) = tab.path.as_mut() else {
                continue;
            };
            if let Ok(suffix) = path.strip_prefix(source) {
                *path = destination.join(suffix);
                tab.title = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Untitled")
                    .to_owned();
            }
        }
        if let Some(root) = self.sidebar.explorer.root.as_mut()
            && let Ok(suffix) = root.strip_prefix(source)
        {
            *root = destination.join(suffix);
        }
        self.sidebar.explorer.expanded = std::mem::take(&mut self.sidebar.explorer.expanded)
            .into_iter()
            .map(|path| {
                path.strip_prefix(source)
                    .map_or(path.clone(), |suffix| destination.join(suffix))
            })
            .collect();
        if let Some(selected) = self.sidebar.explorer.selected.as_mut()
            && let Ok(suffix) = selected.strip_prefix(source)
        {
            *selected = destination.join(suffix);
        }
        self.sidebar.explorer.entries.clear();
        self.sidebar.explorer.loading.clear();
        self.sidebar.explorer.generation = self.sidebar.explorer.generation.wrapping_add(1);
        self.cancel_search_results();
        self.refresh_explorer(cx);
    }

    fn reconcile_deleted_path(&mut self, removed: &Path, cx: &mut Context<Self>) {
        let mut close_ids = Vec::new();
        for tab in &mut self.tabs {
            let Some(path) = tab.path.as_ref() else {
                continue;
            };
            if path == removed || path.starts_with(removed) {
                if tab.is_dirty() {
                    tab.path = None;
                    tab.fingerprint = None;
                    tab.external_change_detected = false;
                } else {
                    close_ids.push(tab.id);
                }
            }
        }
        for id in close_ids {
            self.remove_tab(id);
        }
        if let Some(root) = self.sidebar.explorer.root.as_ref()
            && (root == removed || root.starts_with(removed))
        {
            self.sidebar.explorer.root = None;
            self.sidebar.explorer.expanded.clear();
        }
        self.sidebar.explorer.selected = None;
        self.sidebar.explorer.entries.clear();
        self.sidebar.explorer.loading.clear();
        self.sidebar.explorer.generation = self.sidebar.explorer.generation.wrapping_add(1);
        self.cancel_search_results();
        cx.notify();
    }

    fn cancel_search_results(&mut self) {
        if let Some(cancel) = self.sidebar.search.cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.sidebar.search.generation = self.sidebar.search.generation.wrapping_add(1);
        self.sidebar.search.running = false;
        self.sidebar.search.results.clear();
        self.sidebar.search.summary = None;
        self.sidebar.search.status = "Files changed; run the search again".into();
    }
}

pub(super) fn item_create_file(
    parent: PathBuf,
    view: gpui_kit::WeakEntity<DopadWindow>,
) -> PopupMenuItem {
    PopupMenuItem::new("New File").on_click(move |_, window, app| {
        if let Err(error) = view.update(app, |this, cx| {
            this.prompt_path_operation(window, cx, PathOperation::CreateFile(parent.clone()));
        }) {
            tracing::debug!(error = ?error, "window closed before creating file");
        }
    })
}

pub(super) fn item_create_folder(
    parent: PathBuf,
    view: gpui_kit::WeakEntity<DopadWindow>,
) -> PopupMenuItem {
    PopupMenuItem::new("New Folder").on_click(move |_, window, app| {
        if let Err(error) = view.update(app, |this, cx| {
            this.prompt_path_operation(window, cx, PathOperation::CreateFolder(parent.clone()));
        }) {
            tracing::debug!(error = ?error, "window closed before creating folder");
        }
    })
}

pub(super) fn item_rename(path: PathBuf, view: gpui_kit::WeakEntity<DopadWindow>) -> PopupMenuItem {
    PopupMenuItem::new("Rename").on_click(move |_, window, app| {
        let path = path.clone();
        if let Err(error) = view.update(app, |this, cx| {
            this.prompt_path_operation(window, cx, PathOperation::Rename(path));
        }) {
            tracing::debug!(error = ?error, "window closed before renaming item");
        }
    })
}

pub(super) fn item_delete(path: PathBuf, view: gpui_kit::WeakEntity<DopadWindow>) -> PopupMenuItem {
    PopupMenuItem::new("Delete").on_click(move |_, window, app| {
        let path = path.clone();
        if let Err(error) = view.update(app, |this, cx| this.confirm_delete(window, cx, path)) {
            tracing::debug!(error = ?error, "window closed before deleting item");
        }
    })
}

enum PathResult {
    CreatedFile(PathBuf),
    CreatedFolder(PathBuf),
    Renamed(PathBuf, PathBuf),
}

fn apply_path_operation(
    operation: PathOperation,
    name: &str,
    root: Option<&Path>,
) -> anyhow::Result<PathResult> {
    let (parent, target, rename_source) = match &operation {
        PathOperation::CreateFile(parent) | PathOperation::CreateFolder(parent) => {
            let target = parent.join(name);
            (parent.as_path(), target, None)
        }
        PathOperation::Rename(source) => {
            let parent = source
                .parent()
                .ok_or_else(|| anyhow::anyhow!("item has no parent folder"))?;
            (parent, parent.join(name), Some(source.as_path()))
        }
    };
    ensure_inside_root(parent, root)?;
    if let Some(source) = rename_source {
        let source_metadata = fs::symlink_metadata(source)?;
        anyhow::ensure!(
            !source_metadata.file_type().is_symlink(),
            "symbolic links cannot be renamed from Explorer"
        );
        anyhow::ensure!(
            fs::symlink_metadata(&target)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound),
            "an item with that name already exists"
        );
        fs::rename(source, &target)?;
        return Ok(PathResult::Renamed(source.to_path_buf(), target));
    }
    match operation {
        PathOperation::CreateFile(_) => {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            Ok(PathResult::CreatedFile(target))
        }
        PathOperation::CreateFolder(_) => {
            fs::create_dir(&target)?;
            Ok(PathResult::CreatedFolder(target))
        }
        PathOperation::Rename(_) => anyhow::bail!("rename operation is missing its source path"),
    }
}

fn delete_path(path: &Path, root: Option<&Path>) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("item has no parent folder"))?;
    ensure_inside_root(parent, root)?;
    anyhow::ensure!(
        Some(path) != root,
        "the workspace root cannot be deleted here"
    );
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        !metadata.file_type().is_symlink(),
        "symbolic links cannot be deleted from Explorer"
    );
    if metadata.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn ensure_inside_root(parent: &Path, root: Option<&Path>) -> anyhow::Result<()> {
    let root = root.ok_or_else(|| anyhow::anyhow!("open a workspace folder first"))?;
    let canonical_root = root.canonicalize()?;
    let canonical_parent = parent.canonicalize()?;
    anyhow::ensure!(
        canonical_parent.starts_with(canonical_root),
        "file operation escaped the opened workspace"
    );
    anyhow::ensure!(
        fs::symlink_metadata(parent)?.is_dir(),
        "parent path is not a directory"
    );
    Ok(())
}

fn valid_name(value: &str) -> anyhow::Result<String> {
    let value = value.trim();
    anyhow::ensure!(!value.is_empty(), "Name cannot be empty");
    anyhow::ensure!(!value.contains('\0'), "Name contains an invalid character");
    anyhow::ensure!(
        !value.contains('/') && !value.contains('\\'),
        "Enter a single file or folder name without path separators"
    );
    let mut components = Path::new(value).components();
    anyhow::ensure!(
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none(),
        "Enter a single file or folder name without path separators"
    );
    Ok(value.to_owned())
}

fn reveal_path(path: &Path) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("explorer.exe");
        command.arg("/select,").arg(path);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg("-R").arg(path);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(path.parent().unwrap_or(path));
        command
    };
    let status = command.status()?;
    anyhow::ensure!(status.success(), "file manager returned {status}");
    Ok(())
}
