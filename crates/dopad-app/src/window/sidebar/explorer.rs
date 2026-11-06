use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
    },
};

use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{
    Disableable, Icon, Sizable, Size,
    button::{Button, ButtonVariants},
};
use gpui_kit::{Anchor, Context, IntoElement, ParentElement, Styled, div, px, rgb};
use rfd::AsyncFileDialog;

use super::super::DopadWindow;
use super::model::{DirectoryListing, ExplorerEntry, ExplorerState};
use super::operations;
use crate::icon_theme::{IconThemeManager, create_icon_image_cache};
use crate::theme::Palette;

mod rows;

const MAX_DIRECTORY_ENTRIES: usize = 10_000;

pub(super) fn render(
    window: &mut DopadWindow,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    let root = window.sidebar.explorer.root.clone();
    let heading = root
        .as_ref()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("No folder open")
        .to_owned();
    let open_folder = Button::new("explorer-open-folder")
        .icon(IconName::FolderPlus)
        .xsmall()
        .compact()
        .ghost()
        .tooltip("Open Folder")
        .accessibility_label("Open folder")
        .on_click(cx.listener(|this, _, _, cx| this.open_folder_dialog(cx)));
    let refresh = Button::new("explorer-refresh")
        .icon(IconName::RefreshCw)
        .xsmall()
        .compact()
        .ghost()
        .tooltip("Refresh Explorer")
        .accessibility_label("Refresh Explorer")
        .disabled(root.is_none())
        .on_click(cx.listener(|this, _, _, cx| this.refresh_explorer(cx)));
    let collapse = Button::new("explorer-collapse-all")
        .icon(IconName::ListCollapse)
        .xsmall()
        .compact()
        .ghost()
        .tooltip("Collapse All")
        .accessibility_label("Collapse all folders")
        .disabled(window.sidebar.explorer.expanded.is_empty())
        .on_click(cx.listener(|this, _, _, cx| {
            this.sidebar.explorer.expanded.clear();
            cx.notify();
        }));
    let actions = root.as_ref().map(|root| {
        let root = root.clone();
        let view = cx.entity().downgrade();
        Button::new("explorer-actions")
            .icon(IconName::Ellipsis)
            .xsmall()
            .compact()
            .ghost()
            .tooltip("Workspace actions")
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |mut menu, _, _| {
                menu = menu
                    .item(operations::item_create_file(root.clone(), view.clone()))
                    .item(operations::item_create_folder(root.clone(), view.clone()))
                    .separator();
                let refresh_view = view.clone();
                menu = menu.item(PopupMenuItem::new("Refresh").on_click(move |_, _, app| {
                    if let Err(error) = refresh_view.update(app, |this, cx| this.refresh_explorer(cx)) {
                        tracing::debug!(error = ?error, "explorer window closed before refresh");
                    }
                }));
                let collapse_view = view.clone();
                menu.item(
                    PopupMenuItem::new("Collapse All").on_click(move |_, _, app| {
                        if let Err(error) = collapse_view.update(app, |this, cx| {
                            this.sidebar.explorer.expanded.clear();
                            cx.notify();
                        }) {
                            tracing::debug!(error = ?error, "explorer window closed before collapse");
                        }
                    }),
                )
            })
    });
    let header_actions = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_1()
        .child(open_folder)
        .child(refresh)
        .child(collapse)
        .children(actions);
    let header = div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .px(px(12.))
        .py(px(8.))
        .child(
            div()
                .min_w_0()
                .text_sm()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_ellipsis()
                .text_color(rgb(palette.text))
                .child(heading),
        )
        .child(header_actions);

    let mut content = div().flex_1().min_h_0().w_full().flex().flex_col();
    if let Some(root) = root {
        let root_has_entries = matches!(
            window.sidebar.explorer.entries.get(&root),
            Some(DirectoryListing::Ready(entries) | DirectoryListing::Truncated(entries, _))
                if !entries.is_empty()
        );
        let loading = window.sidebar.explorer.loading.contains(&root);
        if loading && !root_has_entries {
            content = content.child(
                div()
                    .px(px(14.))
                    .py(px(12.))
                    .text_sm()
                    .text_color(rgb(palette.secondary))
                    .child("Loading folder contents…"),
            );
        } else if !loading
            && matches!(
                window.sidebar.explorer.entries.get(&root),
                Some(DirectoryListing::Ready(_))
            )
            && !root_has_entries
        {
            content = content.child(
                div()
                    .px(px(14.))
                    .py(px(12.))
                    .text_sm()
                    .text_color(rgb(palette.secondary))
                    .child("This folder is empty"),
            );
        }
        if let Some(DirectoryListing::Truncated(_, omitted)) =
            window.sidebar.explorer.entries.get(&root)
        {
            content = content.child(
                div()
                    .px(px(14.))
                    .py(px(6.))
                    .text_xs()
                    .text_color(rgb(palette.muted))
                    .child(format!(
                        "Showing first {MAX_DIRECTORY_ENTRIES} items; {omitted} omitted"
                    )),
            );
        }
        if let Some(DirectoryListing::Error(error)) = window.sidebar.explorer.entries.get(&root) {
            content = content.child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .text_xs()
                    .text_color(rgb(0xB73E43))
                    .child(error.clone()),
            );
        }
        content = content.child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(rows::render_rows(window, &root, palette, cx)),
        );
    } else {
        let view = cx.entity();
        content = content
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                Icon::new(IconName::FolderOpen)
                    .with_size(Size::Size(px(22.)))
                    .text_color(rgb(palette.muted)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(palette.secondary))
                    .child("Open a folder to browse its files"),
            )
            .child(
                Button::new("explorer-open-folder-empty")
                    .label("Open Folder")
                    .outline()
                    .on_click(move |_, _, app| {
                        view.update(app, |this, cx| this.open_folder_dialog(cx));
                    }),
            );
    }

    div()
        .size_full()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(palette.canvas))
        .border_r_1()
        .border_color(rgb(palette.border))
        .child(header)
        .child(content)
        .into_any_element()
}

pub(super) fn ensure_icon_theme(window: &mut DopadWindow, cx: &mut Context<DopadWindow>) {
    if window.sidebar.explorer.icon_cache.is_none() {
        window.sidebar.explorer.icon_cache = Some(create_icon_image_cache(cx));
    }
    if window.sidebar.explorer.icon_theme.is_some()
        || window.sidebar.explorer.icon_theme_loading
        || window.sidebar.explorer.icon_theme_failed
    {
        return;
    }

    let theme_id = window.settings.current().file_icon_theme;
    let generation = window.sidebar.explorer.icon_theme_generation;
    window.sidebar.explorer.icon_theme_loading = true;
    cx.spawn(async move |this, cx| {
        let result = cx
            .background_executor()
            .spawn(async move { IconThemeManager::load(&theme_id).map(Arc::new) })
            .await;
        let _ = this.update(cx, |this, cx| {
            let explorer = &mut this.sidebar.explorer;
            if explorer.icon_theme_generation != generation {
                return;
            }
            explorer.icon_theme_loading = false;
            match result {
                Ok(manager) => explorer.icon_theme = Some(manager),
                Err(error) => {
                    explorer.icon_theme_failed = true;
                    tracing::warn!(error = %error, "Material icon theme unavailable; using built-in Explorer icons");
                }
            }
            cx.notify();
        });
    })
    .detach();
}

impl DopadWindow {
    pub(in crate::window) fn open_folder_dialog(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let Some(folder) = AsyncFileDialog::new()
                .set_title("Open Folder")
                .pick_folder()
                .await
            else {
                return;
            };
            let path = folder.path().to_path_buf();
            let _ = this.update(cx, |this, cx| {
                super::show_explorer_root(this, path, cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn toggle_explorer_directory(&mut self, path: &Path, cx: &mut Context<Self>) {
        let path = path.to_path_buf();
        if self.sidebar.explorer.expanded.remove(&path) {
            cx.notify();
            return;
        }
        self.sidebar.explorer.expanded.insert(path.clone());
        if !self.sidebar.explorer.entries.contains_key(&path) {
            self.load_explorer_directory(path, cx);
        }
        cx.notify();
    }

    pub(super) fn refresh_explorer(&mut self, cx: &mut Context<Self>) {
        let paths = self
            .sidebar
            .explorer
            .expanded
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        cancel_explorer_work(&mut self.sidebar.explorer);
        for path in paths {
            self.sidebar.explorer.entries.remove(&path);
            self.load_explorer_directory(path, cx);
        }
        cx.notify();
    }

    fn load_explorer_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.sidebar.explorer.loading.insert(path.clone()) {
            return;
        }
        let generation = self.sidebar.explorer.generation;
        let cancelled = self.sidebar.explorer.cancelled.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let loaded_path = path.clone();
            let worker_cancelled = cancelled.clone();
            let listing = cx
                .background_executor()
                .spawn(async move { read_directory(&loaded_path, &worker_cancelled) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.sidebar.explorer.generation != generation {
                    return;
                }
                this.sidebar.explorer.loading.remove(&path);
                if let Some(listing) = listing {
                    this.sidebar.explorer.entries.insert(path, listing);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

pub(super) fn replace_root(window: &mut DopadWindow, path: PathBuf, cx: &mut Context<DopadWindow>) {
    cancel_explorer_work(&mut window.sidebar.explorer);
    window.sidebar.explorer.root = Some(path.clone());
    window.sidebar.explorer.entries.clear();
    window.sidebar.explorer.expanded.clear();
    window.sidebar.explorer.selected = None;
    window.sidebar.explorer.expanded.insert(path.clone());
    window.load_explorer_directory(path, cx);
}

pub(super) fn ensure_root_loaded(window: &mut DopadWindow, cx: &mut Context<DopadWindow>) {
    let Some(root) = window.sidebar.explorer.root.clone() else {
        return;
    };
    if !window.sidebar.explorer.entries.contains_key(&root)
        && !window.sidebar.explorer.loading.contains(&root)
    {
        window.sidebar.explorer.expanded.insert(root.clone());
        window.load_explorer_directory(root, cx);
    }
}

pub(super) fn deactivate(state: &mut ExplorerState) {
    cancel_explorer_work(state);
    state.icon_theme_generation = state.icon_theme_generation.wrapping_add(1);
    state.icon_theme_loading = false;
    state.icon_theme = None;
    state.icon_cache = None;
    state.entries.clear();
    state.expanded.clear();
    state.loading.clear();
    state.selected = None;
}

fn cancel_explorer_work(state: &mut ExplorerState) {
    state.cancelled.store(true, AtomicOrdering::Relaxed);
    state.generation = state.generation.wrapping_add(1);
    state.cancelled = Arc::new(AtomicBool::new(false));
    state.loading.clear();
}

fn read_directory(path: &Path, cancelled: &AtomicBool) -> Option<DirectoryListing> {
    if cancelled.load(AtomicOrdering::Relaxed) {
        return None;
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => {
            return Some(DirectoryListing::Error(format!(
                "Could not read folder: {error}"
            )));
        }
    };
    let mut result = Vec::new();
    let mut truncated = 0usize;
    for entry in entries {
        if cancelled.load(AtomicOrdering::Relaxed) {
            return None;
        }
        match entry {
            Ok(entry) => {
                let kind = match entry.file_type() {
                    Ok(kind) if kind.is_dir() => true,
                    Ok(kind) if kind.is_file() => false,
                    // Do not follow directory or file symlinks in the explorer.
                    Ok(_) => continue,
                    Err(_) => continue,
                };
                if result.len() == MAX_DIRECTORY_ENTRIES {
                    truncated += 1;
                    continue;
                }
                result.push(ExplorerEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path: entry.path(),
                    is_dir: kind,
                });
            }
            Err(_) => truncated += 1,
        }
    }
    result.sort_by(|left, right| match (left.is_dir, right.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
    });
    if truncated > 0 {
        Some(DirectoryListing::Truncated(result, truncated))
    } else {
        Some(DirectoryListing::Ready(result))
    }
}

#[cfg(test)]
mod icon_theme_state_tests {
    use super::super::model::ExplorerState;
    use super::deactivate;

    #[test]
    fn closing_explorer_drops_icon_state_and_invalidates_in_flight_load() {
        let mut state = ExplorerState {
            icon_theme_loading: true,
            ..ExplorerState::default()
        };
        let generation = state.icon_theme_generation;

        deactivate(&mut state);

        assert_eq!(state.icon_theme_generation, generation.wrapping_add(1));
        assert!(!state.icon_theme_loading);
        assert!(state.icon_theme.is_none());
        assert!(state.icon_cache.is_none());
    }
}
