use std::sync::atomic::Ordering;

use super::super::DopadWindow;
use super::model::{SearchScope, SidebarState};
use crate::theme::Palette;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Disableable, Icon, Sizable, Size,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    input::{Input, InputState},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::{
    Anchor, AppContext as _, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Styled, Window, div, px, rgb,
};

mod results;
mod runner;

pub(super) fn ensure_input(
    sidebar: &mut SidebarState,
    window: &mut Window,
    cx: &mut Context<DopadWindow>,
) {
    if sidebar.search.input.is_none() {
        sidebar.search.input =
            Some(cx.new(|cx| InputState::new(window, cx).placeholder("Search in files")));
    }
    if sidebar.search.filter.is_none() {
        sidebar.search.filter =
            Some(cx.new(|cx| InputState::new(window, cx).placeholder("Filter results")));
    }
}

pub(super) fn deactivate(sidebar: &mut SidebarState) {
    runner::deactivate(&mut sidebar.search);
}

pub(super) fn render(
    window: &mut DopadWindow,
    ui_window: &mut Window,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    ensure_input(&mut window.sidebar, ui_window, cx);
    let query = window
        .sidebar
        .search
        .input
        .as_ref()
        .expect("search input initialized");
    let scope = scope_label(&window.sidebar.search.scope);
    let selected_scope = window.sidebar.search.scope;
    let query_is_valid = !query.read(cx).value().is_empty();
    let scope_is_valid = match selected_scope {
        SearchScope::CurrentFolder => window.sidebar.explorer.root.as_ref().is_some_and(|root| {
            window.sidebar.search.in_current_folder
                || window
                    .tabs
                    .iter()
                    .any(|tab| tab.path.as_ref().is_some_and(|path| path.starts_with(root)))
        }),
        SearchScope::OpenFiles => !window.tabs.is_empty(),
        SearchScope::CurrentFile => window.active_tab().is_some(),
        SearchScope::CustomFolder => window.sidebar.search.custom_root.is_some(),
    };
    let can_start_search = query_is_valid && scope_is_valid;
    let entity = cx.entity();
    let scope_menu = Button::new("search-scope")
        .child(
            div().flex().items_center().gap_1().child(scope).child(
                Icon::new(IconName::ChevronDown)
                    .with_size(Size::Size(px(12.)))
                    .text_color(rgb(palette.secondary)),
            ),
        )
        .xsmall()
        .outline()
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |mut menu, _, _| {
            for (label, target) in [
                ("Current folder", SearchScope::CurrentFolder),
                ("Open files", SearchScope::OpenFiles),
                ("Current file", SearchScope::CurrentFile),
            ] {
                let entity = entity.clone();
                let item = PopupMenuItem::new(label)
                    .checked(selected_scope == target)
                    .on_click(move |_, _, app| {
                        entity.update(app, |this, cx| {
                            this.sidebar.search.scope = target;
                            cx.notify();
                        });
                    });
                menu = menu.item(item);
            }
            let entity = entity.clone();
            menu.item(
                PopupMenuItem::new("Custom folder")
                    .checked(selected_scope == SearchScope::CustomFolder)
                    .on_click(move |_, _, app| {
                        entity.update(app, |this, cx| this.pick_search_folder(cx));
                    }),
            )
        });

    let search_button = if window.sidebar.search.running {
        let cancel = window.sidebar.search.cancel.clone();
        Button::new("search-files-cancel")
            .label("Cancel Search")
            .outline()
            .w_full()
            .on_click(move |_, _, _| {
                if let Some(cancel) = cancel.as_ref() {
                    cancel.store(true, Ordering::Relaxed);
                }
            })
    } else {
        Button::new("search-files-run")
            .label("Search")
            .primary()
            .w_full()
            .disabled(!can_start_search)
            .on_click(cx.listener(|this, _, _, cx| this.start_file_search(cx)))
    };

    let case = Checkbox::new("search-files-case")
        .label("Match Case")
        .checked(window.sidebar.search.match_case)
        .on_change(cx.listener(|this, value, _, cx| {
            this.sidebar.search.match_case = *value;
            cx.notify();
        }));
    let whole_word = Checkbox::new("search-files-whole-word")
        .label("Whole Word")
        .checked(window.sidebar.search.whole_word)
        .on_change(cx.listener(|this, value, _, cx| {
            this.sidebar.search.whole_word = *value;
            cx.notify();
        }));
    let regex = Checkbox::new("search-files-regex")
        .label("Regex")
        .checked(window.sidebar.search.regex)
        .on_change(cx.listener(|this, value, _, cx| {
            this.sidebar.search.regex = *value;
            cx.notify();
        }));
    let in_folder = Checkbox::new("search-files-in-folder")
        .label("In Folder")
        .checked(window.sidebar.search.in_current_folder)
        .on_change(cx.listener(|this, value, _, cx| {
            this.sidebar.search.in_current_folder = *value;
            cx.notify();
        }));
    let clear = Button::new("search-files-clear")
        .icon(IconName::X)
        .xsmall()
        .compact()
        .ghost()
        .tooltip("Clear query")
        .on_click(cx.listener(|this, _, window, cx| {
            if let Some(input) = this.sidebar.search.input.as_ref() {
                input.update(cx, |input, cx| input.set_value(String::new(), window, cx));
            }
            cx.notify();
        }));

    let header = div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .px(px(12.))
        .py(px(8.))
        .child(
            div()
                .text_sm()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(rgb(palette.text))
                .child("Search in Files"),
        )
        .child(
            Button::new("search-close-sidebar")
                .icon(IconName::X)
                .xsmall()
                .compact()
                .ghost()
                .tooltip("Close Search")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.close_sidebar_panel(cx);
                })),
        );
    let options = div()
        .w_full()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(12.))
        .gap_y(px(4.))
        .child(case)
        .child(whole_word)
        .child(regex)
        .child(in_folder);

    let query_input = div()
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
            if event.keystroke.key == "enter" {
                this.start_file_search(cx);
            }
        }))
        .child(Input::new(query));
    let mut content = div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .p(px(10.))
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap_1()
                .child(div().flex_1().min_w_0().child(query_input))
                .child(clear),
        )
        .child(options)
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_1().min_w_0().child(scope_menu))
                .child(search_button),
        );

    let status = window.sidebar.search.status.clone();
    if !status.is_empty() {
        let error = status.starts_with("Could not")
            || status.starts_with("Invalid")
            || status.starts_with("File changed")
            || status.starts_with("Document changed")
            || status.starts_with("Search result is stale");
        content = content.child(
            div()
                .text_xs()
                .text_color(rgb(if error { 0xB73E43 } else { palette.secondary }))
                .child(status),
        );
    }
    content = content.child(results::render_results(window, palette, cx));

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

fn scope_label(scope: &SearchScope) -> &'static str {
    match scope {
        SearchScope::CurrentFolder => "Current folder",
        SearchScope::OpenFiles => "Open files",
        SearchScope::CurrentFile => "Current file",
        SearchScope::CustomFolder => "Custom folder",
    }
}
