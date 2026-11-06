mod explorer;
mod model;
mod operations;
mod search_panel;

use std::sync::Arc;

use gpui_kit::{Context, Entity, IntoElement, Window};

use super::DopadWindow;
use crate::icon_theme::{IconImageCache, IconThemeManager};
use crate::theme::Palette;

pub(super) use model::{SidebarMode, SidebarState};

pub(super) fn root_folder(state: &SidebarState) -> Option<std::path::PathBuf> {
    state.explorer.root.clone()
}

pub(super) fn ensure_file_icon_theme(window: &mut DopadWindow, cx: &mut Context<DopadWindow>) {
    explorer::ensure_icon_theme(window, cx);
}

pub(super) fn file_icon_resources(
    window: &DopadWindow,
) -> (
    Option<Arc<IconThemeManager>>,
    Option<Entity<IconImageCache>>,
) {
    (
        window.sidebar.explorer.icon_theme.clone(),
        window.sidebar.explorer.icon_cache.clone(),
    )
}

pub(super) fn render(
    window: &mut DopadWindow,
    ui_window: &mut Window,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    match window.sidebar.mode {
        SidebarMode::Closed => gpui_kit::div().into_any_element(),
        SidebarMode::Explorer => explorer::render(window, palette, cx),
        SidebarMode::Search => search_panel::render(window, ui_window, palette, cx),
    }
}

impl DopadWindow {
    pub(super) fn toggle_explorer(&mut self, cx: &mut Context<Self>) {
        let mode = if self.sidebar.mode == SidebarMode::Explorer {
            SidebarMode::Closed
        } else {
            SidebarMode::Explorer
        };
        set_mode(self, mode, cx);
        cx.notify();
    }

    pub(super) fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = if self.sidebar.mode == SidebarMode::Search {
            SidebarMode::Closed
        } else {
            SidebarMode::Search
        };
        set_mode(self, mode, cx);
        if mode == SidebarMode::Search {
            search_panel::ensure_input(&mut self.sidebar, window, cx);
        }
        cx.notify();
    }
}

pub(super) fn show_explorer_root(
    window: &mut DopadWindow,
    path: std::path::PathBuf,
    cx: &mut Context<DopadWindow>,
) {
    transition_mode(window, SidebarMode::Explorer);
    explorer::ensure_icon_theme(window, cx);
    explorer::replace_root(window, path, cx);
}

fn set_mode(window: &mut DopadWindow, mode: SidebarMode, cx: &mut Context<DopadWindow>) {
    transition_mode(window, mode);
    if mode == SidebarMode::Explorer {
        explorer::ensure_icon_theme(window, cx);
        explorer::ensure_root_loaded(window, cx);
    }
}

fn transition_mode(window: &mut DopadWindow, mode: SidebarMode) {
    if window.sidebar.mode != mode {
        match window.sidebar.mode {
            SidebarMode::Explorer => explorer::deactivate(&mut window.sidebar.explorer),
            SidebarMode::Search => search_panel::deactivate(&mut window.sidebar),
            SidebarMode::Closed => {}
        }
        window.sidebar.mode = mode;
    }
}
