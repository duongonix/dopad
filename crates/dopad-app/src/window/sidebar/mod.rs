mod explorer;
mod model;
mod operations;
mod search_panel;

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Disableable, Icon, Sizable, Size,
    button::{Button, ButtonVariants},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{Context, Entity, IntoElement, ParentElement, Styled, Window, div, px, rgb};

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
        SidebarMode::Rail => gpui_kit::div().into_any_element(),
        SidebarMode::Explorer => explorer::render(window, palette, cx),
        SidebarMode::Search => search_panel::render(window, ui_window, palette, cx),
    }
}

pub(super) fn render_navigation_rail(
    window: &mut DopadWindow,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    let explorer_active = window.sidebar.mode == SidebarMode::Explorer;
    let search_active = window.sidebar.mode == SidebarMode::Search;
    let explorer = Button::new("navigation-explorer")
        .child(Icon::new(IconName::Folder).with_size(Size::Size(px(19.))))
        .compact()
        .ghost()
        .w(px(36.))
        .h(px(36.))
        .when(explorer_active, |button| {
            button
                .bg(rgb(palette.tab_hover))
                .text_color(rgb(palette.accent))
        })
        .tooltip("Explorer")
        .accessibility_label("Toggle Explorer")
        .on_click(cx.listener(|this, _, _, cx| this.toggle_explorer(cx)));
    let search = Button::new("navigation-search")
        .child(Icon::new(IconName::Search).with_size(Size::Size(px(19.))))
        .compact()
        .ghost()
        .w(px(36.))
        .h(px(36.))
        .when(search_active, |button| {
            button
                .bg(rgb(palette.tab_hover))
                .text_color(rgb(palette.accent))
        })
        .tooltip("Search in Files")
        .accessibility_label("Toggle Search in Files")
        .on_click(cx.listener(|this, _, window, cx| this.toggle_search(window, cx)));
    let extensions = Button::new("navigation-extensions")
        .child(Icon::new(IconName::Puzzle).with_size(Size::Size(px(19.))))
        .compact()
        .ghost()
        .w(px(36.))
        .h(px(36.))
        .disabled(true)
        .tooltip("Extensions — Coming later")
        .accessibility_label("Extensions, coming later");

    div()
        .w(px(48.))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .pt(px(10.))
        .bg(rgb(palette.chrome))
        .border_r_1()
        .border_color(rgb(palette.border))
        .child(explorer)
        .child(search)
        .child(extensions)
        .into_any_element()
}

impl DopadWindow {
    pub(super) fn toggle_explorer(&mut self, cx: &mut Context<Self>) {
        let mode = self.sidebar.mode.toggle_panel(SidebarMode::Explorer);
        set_mode(self, mode, cx);
        cx.notify();
    }

    pub(super) fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.sidebar.mode.toggle_sidebar(self.sidebar.last_panel);
        set_mode(self, mode, cx);
        if mode == SidebarMode::Search {
            search_panel::ensure_input(&mut self.sidebar, window, cx);
        }
        cx.notify();
    }

    pub(super) fn close_sidebar_panel(&mut self, cx: &mut Context<Self>) {
        set_mode(self, SidebarMode::Rail, cx);
        cx.notify();
    }

    pub(super) fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.sidebar.mode.toggle_panel(SidebarMode::Search);
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
    if matches!(mode, SidebarMode::Explorer | SidebarMode::Search) {
        window.sidebar.last_panel = mode;
    }
    if window.sidebar.mode != mode {
        match window.sidebar.mode {
            SidebarMode::Explorer => explorer::deactivate(&mut window.sidebar.explorer),
            SidebarMode::Search => search_panel::deactivate(&mut window.sidebar),
            SidebarMode::Closed | SidebarMode::Rail => {}
        }
        window.sidebar.mode = mode;
    }
}
