use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable, Size};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    ClipboardItem, Context, Entity, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, ScrollStrategy, StatefulInteractiveElement, Styled, StyledImage,
    UniformListScrollHandle, div, img, px, rgb,
};

use super::super::super::DopadWindow;
use super::super::{model::SidebarState, operations};
use crate::icon_theme::{FileEntryKind, IconImageCache, IconRequest, IconThemeManager};
use crate::theme::Palette;

const MAX_TREE_DEPTH: usize = 128;
const MAX_VISIBLE_ROWS: usize = 20_000;

struct ExplorerRow {
    path: PathBuf,
    name: String,
    is_dir: bool,
    depth: usize,
    expanded: bool,
    loading: bool,
    selected: bool,
    active_file: bool,
}

#[derive(Clone, Copy)]
struct RowColors {
    text: u32,
    secondary: u32,
    accent: u32,
    tab_hover: u32,
}

pub(super) fn render_rows(
    window: &mut DopadWindow,
    root: &Path,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    let mut rows = Vec::new();
    let active_path = window.active_tab().and_then(|tab| tab.path.as_deref());
    flatten_directory(&window.sidebar, active_path, root, 0, &mut rows);
    let colors = RowColors {
        text: palette.text,
        secondary: palette.secondary,
        accent: palette.accent,
        tab_hover: palette.tab_hover,
    };
    let root = root.to_path_buf();
    let rows = Arc::new(rows);
    let focus_handle = window
        .sidebar
        .explorer
        .focus_handle
        .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
        .clone();
    let scroll_handle = window.sidebar.explorer.scroll_handle.clone();
    let icon_theme = window.sidebar.explorer.icon_theme.clone();
    let icon_cache = window.sidebar.explorer.icon_cache.clone();
    let keyboard_rows = rows.clone();
    let keyboard_scroll = scroll_handle.clone();
    let keyboard_focus = focus_handle.clone();
    gpui_kit::uniform_list(
        "explorer-rows",
        rows.len(),
        cx.processor(move |_this, range: std::ops::Range<usize>, _window, cx| {
            range
                .map(|index| {
                    render_row(
                        &rows[index],
                        &root,
                        colors,
                        &focus_handle,
                        icon_theme.as_deref(),
                        icon_cache.as_ref(),
                        cx,
                    )
                })
                .collect::<Vec<_>>()
        }),
    )
    .track_scroll(&scroll_handle)
    .track_focus(&keyboard_focus)
    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
        handle_key_down(
            this,
            &keyboard_rows,
            &keyboard_scroll,
            &keyboard_focus,
            &event.keystroke.key,
            window,
            cx,
        );
    }))
    .h_full()
    .into_any_element()
}

fn render_row(
    row: &ExplorerRow,
    root: &Path,
    colors: RowColors,
    focus_handle: &FocusHandle,
    icon_theme: Option<&IconThemeManager>,
    icon_cache: Option<&Entity<IconImageCache>>,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    let fallback_icon = match (row.is_dir, row.expanded) {
        (true, true) => IconName::FolderOpen,
        (true, false) => IconName::Folder,
        (false, _) => file_icon(&row.path),
    };
    let request = IconRequest {
        name: &row.name,
        kind: if row.is_dir {
            FileEntryKind::Directory
        } else {
            FileEntryKind::File
        },
        expanded: row.expanded,
    };
    let icon_element = icon_theme
        .and_then(|theme| theme.resolve_icon(&request))
        .map(|icon_id| {
            let fallback_color = if row.is_dir {
                colors.accent
            } else {
                colors.secondary
            };
            let mut image = img(icon_id.shared_resource_path())
                .size(px(16.))
                .with_fallback(move || {
                    Icon::new(fallback_icon)
                        .with_size(Size::Size(px(16.)))
                        .text_color(rgb(fallback_color))
                        .into_any_element()
                });
            if let Some(cache) = icon_cache {
                image = image.image_cache(cache);
            }
            image.into_any_element()
        })
        .unwrap_or_else(|| {
            Icon::new(fallback_icon)
                .with_size(Size::Size(px(16.)))
                .text_color(rgb(if row.is_dir {
                    colors.accent
                } else {
                    colors.secondary
                }))
                .into_any_element()
        });
    let chevron = if row.loading {
        div().size(px(12.))
    } else if row.is_dir {
        div().size(px(12.)).child(
            Icon::new(if row.expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .with_size(Size::Size(px(12.)))
            .text_color(rgb(colors.secondary)),
        )
    } else {
        div().size(px(12.))
    };
    let path_for_click = row.path.clone();
    let is_dir = row.is_dir;
    let click_focus = focus_handle.clone();
    let element = div()
        .id(format!("explorer-row-{}", row_id(&row.path)))
        .w_full()
        .h(px(27.))
        .flex()
        .items_center()
        .gap_1()
        .pr(px(8.))
        .pl(px(8. + (row.depth.min(MAX_TREE_DEPTH) as f32 * 14.)))
        .text_size(px(12.))
        .text_ellipsis()
        .text_color(rgb(colors.text))
        .when(row.selected || row.active_file, |this| {
            this.bg(rgb(colors.tab_hover))
        })
        .when(!row.selected && !row.active_file, |this| {
            this.hover(|style| style.bg(rgb(colors.tab_hover)))
        })
        .child(chevron)
        .child(icon_element)
        .child(div().min_w_0().text_ellipsis().child(row.name.clone()));
    let element = if is_dir {
        element.on_click(cx.listener(move |this, _, window, cx| {
            this.sidebar.explorer.selected = Some(path_for_click.clone());
            click_focus.focus(window, cx);
            this.toggle_explorer_directory(&path_for_click, cx);
        }))
    } else {
        element.on_click(cx.listener(move |this, _, window, cx| {
            this.sidebar.explorer.selected = Some(path_for_click.clone());
            click_focus.focus(window, cx);
            this.open_paths(vec![path_for_click.clone()], cx);
        }))
    };
    let menu_path = row.path.clone();
    let menu_root = root.to_path_buf();
    let view = cx.entity().downgrade();
    element
        .context_menu(move |mut menu, _, _| {
            let copy_path = menu_path.clone();
            menu = menu.item(PopupMenuItem::new("Copy Path").on_click(move |_, _, app| {
                app.write_to_clipboard(ClipboardItem::new_string(
                    copy_path.to_string_lossy().into_owned(),
                ));
            }));
            if let Ok(relative) = menu_path.strip_prefix(&menu_root) {
                let relative = relative.to_string_lossy().into_owned();
                menu = menu.item(PopupMenuItem::new("Copy Relative Path").on_click(
                    move |_, _, app| {
                        app.write_to_clipboard(ClipboardItem::new_string(relative.clone()));
                    },
                ));
            }
            if is_dir {
                menu = menu
                    .item(operations::item_create_file(
                        menu_path.clone(),
                        view.clone(),
                    ))
                    .item(operations::item_create_folder(
                        menu_path.clone(),
                        view.clone(),
                    ))
                    .separator();
            } else {
                let open_view = view.clone();
                let open_path = menu_path.clone();
                menu = menu.item(PopupMenuItem::new("Open").on_click(move |_, _, app| {
                    if let Err(error) = open_view.update(app, |this, cx| {
                        this.sidebar.explorer.selected = Some(open_path.clone());
                        this.open_paths(vec![open_path.clone()], cx);
                    }) {
                        tracing::debug!(error = ?error, "explorer window closed before opening file");
                    }
                }));
            }
            let reveal_view = view.clone();
            let reveal_path = menu_path.clone();
            menu = menu.item(PopupMenuItem::new("Reveal in File Manager").on_click(
                move |_, _, app| {
                    if let Err(error) = reveal_view.update(app, |this, cx| {
                        this.reveal_in_file_manager(reveal_path.clone(), cx);
                    }) {
                        tracing::debug!(error = ?error, "explorer window closed before reveal");
                    }
                },
            ));
            menu.separator()
                .item(operations::item_rename(menu_path.clone(), view.clone()))
                .item(operations::item_delete(menu_path.clone(), view.clone()))
        })
        .into_any_element()
}

fn handle_key_down(
    this: &mut DopadWindow,
    rows: &[ExplorerRow],
    scroll_handle: &UniformListScrollHandle,
    focus_handle: &FocusHandle,
    key: &str,
    window: &mut gpui_kit::Window,
    cx: &mut Context<DopadWindow>,
) {
    if rows.is_empty() {
        return;
    }

    let selected_index = this
        .sidebar
        .explorer
        .selected
        .as_ref()
        .and_then(|selected| rows.iter().position(|row| &row.path == selected));

    match key {
        "up" | "down" => {
            let Some(index) = keyboard_target_index(key, selected_index, rows.len()) else {
                return;
            };
            this.sidebar.explorer.selected = Some(rows[index].path.clone());
            scroll_handle.scroll_to_item(index, ScrollStrategy::Nearest);
            focus_handle.focus(window, cx);
            cx.notify();
        }
        "enter" => {
            let Some(index) = selected_index else {
                return;
            };
            let row = &rows[index];
            this.sidebar.explorer.selected = Some(row.path.clone());
            if row.is_dir {
                this.toggle_explorer_directory(&row.path, cx);
            } else {
                this.open_paths(vec![row.path.clone()], cx);
            }
        }
        "left" => {
            let Some(index) = selected_index else {
                return;
            };
            let row = &rows[index];
            if row.is_dir && row.expanded {
                this.sidebar.explorer.expanded.remove(&row.path);
                cx.notify();
            }
        }
        "right" => {
            let Some(index) = selected_index else {
                return;
            };
            let row = &rows[index];
            if row.is_dir && !row.expanded {
                this.toggle_explorer_directory(&row.path, cx);
            }
        }
        _ => {}
    }
}

fn keyboard_target_index(
    key: &str,
    selected_index: Option<usize>,
    row_count: usize,
) -> Option<usize> {
    if row_count == 0 {
        return None;
    }
    match (key, selected_index) {
        ("up", Some(index)) => Some(index.saturating_sub(1)),
        ("down", Some(index)) => Some(index.saturating_add(1).min(row_count - 1)),
        ("up" | "down", None) => Some(0),
        _ => None,
    }
}

fn flatten_directory(
    sidebar: &SidebarState,
    active_path: Option<&Path>,
    directory: &Path,
    depth: usize,
    rows: &mut Vec<ExplorerRow>,
) {
    if depth > MAX_TREE_DEPTH || rows.len() >= MAX_VISIBLE_ROWS {
        return;
    }
    let entries = match sidebar.explorer.entries.get(directory) {
        Some(
            super::super::model::DirectoryListing::Ready(entries)
            | super::super::model::DirectoryListing::Truncated(entries, _),
        ) => entries,
        _ => return,
    };
    for entry in entries {
        if rows.len() >= MAX_VISIBLE_ROWS {
            break;
        }
        let expanded = sidebar.explorer.expanded.contains(&entry.path);
        let loading = sidebar.explorer.loading.contains(&entry.path);
        rows.push(ExplorerRow {
            path: entry.path.clone(),
            name: entry.name.clone(),
            is_dir: entry.is_dir,
            depth,
            expanded,
            loading,
            selected: sidebar.explorer.selected.as_ref() == Some(&entry.path),
            active_file: active_path == Some(entry.path.as_path()),
        });
        if entry.is_dir && expanded {
            flatten_directory(sidebar, active_path, &entry.path, depth + 1, rows);
        }
    }
}

fn file_icon(path: &Path) -> IconName {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "c" | "h") => {
            IconName::FileCode
        }
        Some("md" | "txt" | "rst") => IconName::FileText,
        _ => IconName::File,
    }
}

fn row_id(path: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::keyboard_target_index;

    #[test]
    fn explorer_keyboard_navigation_selects_and_clamps_rows() {
        assert_eq!(keyboard_target_index("down", None, 4), Some(0));
        assert_eq!(keyboard_target_index("up", Some(0), 4), Some(0));
        assert_eq!(keyboard_target_index("down", Some(2), 4), Some(3));
        assert_eq!(keyboard_target_index("down", Some(3), 4), Some(3));
        assert_eq!(keyboard_target_index("up", Some(3), 4), Some(2));
        assert_eq!(keyboard_target_index("down", None, 0), None);
        assert_eq!(keyboard_target_index("left", Some(0), 4), None);
    }
}
