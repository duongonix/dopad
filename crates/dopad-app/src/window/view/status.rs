use dopad_core::ByteOffset;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Disableable, Icon, Sizable, Size,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
    status_bar::StatusBar,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{Anchor, Context, IntoElement, ParentElement, Styled, Window, div, px, rgb};

use crate::theme::Palette;
use crate::window::{DopadWindow, bottom_panel::BottomPanelView, preview};
use dopad_syntax::Language;

const TERMINAL_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/></svg>"#;

pub(super) fn render(
    window: &DopadWindow,
    preview_mode: bool,
    ui_window: &Window,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    let (line, column, encoding, eol, language_name, active_document) =
        window
            .active_tab()
            .map_or((1, 1, "UTF-8", "LF", "Plain Text", None), |tab| {
                let (line, column) = tab
                    .ui
                    .as_ref()
                    .and_then(|state| {
                        let cursor = state.read(cx).cursor();
                        tab.editor
                            .as_ref()?
                            .snapshot()
                            .byte_to_position(ByteOffset(cursor))
                            .ok()
                            .map(|position| (position.line + 1, position.column + 1))
                    })
                    .unwrap_or((1, 1));
                (
                    line,
                    column,
                    tab.encoding.label(),
                    tab.eol.label(),
                    tab.language().display_name(),
                    Some(tab.id),
                )
            });

    let update_available = window.updater.read(cx).state.has_update();
    let sidebar_visible = window.sidebar.mode.is_visible();
    let left = div()
        .flex()
        .items_center()
        .gap_1()
        .child(
            Button::new("status-sidebar")
                .child(
                    Icon::new(if sidebar_visible {
                        IconName::PanelLeftClose
                    } else {
                        IconName::PanelLeftOpen
                    })
                    .with_size(Size::Size(px(17.))),
                )
                .xsmall()
                .compact()
                .ghost()
                .when(sidebar_visible, |button| {
                    button.text_color(rgb(palette.accent))
                })
                .tooltip(if sidebar_visible {
                    "Hide Sidebar"
                } else {
                    "Show Sidebar"
                })
                .accessibility_label(if sidebar_visible {
                    "Hide Sidebar"
                } else {
                    "Show Sidebar"
                })
                .on_click(cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx))),
        )
        .child(
            Button::new("status-terminal")
                .child(
                    Icon::default()
                        .data(TERMINAL_ICON)
                        .with_size(Size::Size(px(17.))),
                )
                .xsmall()
                .compact()
                .ghost()
                .when(
                    window.bottom_panel.active == Some(BottomPanelView::Terminal),
                    |button| button.text_color(rgb(palette.accent)),
                )
                .tooltip(
                    if window.bottom_panel.active == Some(BottomPanelView::Terminal) {
                        "Hide Terminal"
                    } else {
                        "Open Terminal"
                    },
                )
                .accessibility_label(
                    if window.bottom_panel.active == Some(BottomPanelView::Terminal) {
                        "Hide Terminal"
                    } else {
                        "Open Terminal"
                    },
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_terminal(window, cx))),
        )
        .child(render_problems_shortcut(window, palette, cx))
        .child(
            Button::new("status-update")
                .child(
                    div()
                        .relative()
                        .size(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(IconName::Download).with_size(Size::Size(px(17.))))
                        .when(update_available, |icon| {
                            icon.child(
                                div()
                                    .absolute()
                                    .top(px(0.))
                                    .right(px(0.))
                                    .size(px(7.))
                                    .rounded_full()
                                    .bg(rgb(0xD94B43))
                                    .border_1()
                                    .border_color(rgb(palette.chrome)),
                            )
                        }),
                )
                .xsmall()
                .compact()
                .ghost()
                .tooltip(if update_available {
                    "Update available"
                } else {
                    "Check for updates"
                })
                .accessibility_label(if update_available {
                    "Update available"
                } else {
                    "Check for updates"
                })
                .on_click(cx.listener(|this, _, window, cx| this.open_update(window, cx))),
        );
    let viewer_status = window.active_tab().and_then(|tab| match &tab.content {
        super::super::file_view::TabContent::Image(state) => {
            let sidebar = window
                .sidebar
                .mode
                .has_panel()
                .then_some(window.sidebar.width);
            Some(format!(
                "{} · {} × {} · {} · {}%",
                state.info.format.label(),
                state.info.width,
                state.info.height,
                super::super::file_view::format_file_size(state.info.file_size),
                super::super::image_preview::zoom_percent(
                    state,
                    ui_window,
                    sidebar,
                    window.settings.current().show_status_bar,
                ),
            ))
        }
        super::super::file_view::TabContent::Unsupported(info) => Some(format!(
            "{} · {}",
            info.file_type,
            super::super::file_view::format_file_size(info.file_size)
        )),
        super::super::file_view::TabContent::Error(error) => Some(format!(
            "{} · {}",
            error.file_type,
            error
                .file_size
                .map(super::super::file_view::format_file_size)
                .unwrap_or_else(|| "Unknown size".to_owned())
        )),
        _ => None,
    });
    let preview_available = window
        .active_tab()
        .is_some_and(preview::supports_tab_preview);
    let language_mode = render_language_menu(
        language_name,
        active_document,
        window
            .active_tab()
            .is_some_and(|tab| tab.language_override.is_none()),
        window.active_tab().map(|tab| tab.language()),
        palette,
        cx,
    );
    let mode_icon = if preview_mode {
        IconName::Eye
    } else {
        IconName::Pencil
    };
    let mode = preview_available.then(|| {
        Button::new("status-mode")
            .child(Icon::new(mode_icon).with_size(Size::Size(px(15.))))
            .xsmall()
            .compact()
            .ghost()
            .text_color(rgb(palette.accent))
            .tooltip(if preview_mode {
                "Return to edit mode"
            } else {
                "Switch to preview mode"
            })
            .accessibility_label(if preview_mode {
                "Switch to edit mode"
            } else {
                "Switch to preview mode"
            })
            .on_click(cx.listener(|this, _, _, cx| this.toggle_preview(cx)))
    });
    let right = if let Some(viewer_status) = viewer_status {
        div().flex().items_center().gap_4().child(viewer_status)
    } else {
        let mut right = div()
            .flex()
            .items_center()
            .gap_4()
            .child(format!("Ln {line}, Col {column}"))
            .child(encoding)
            .child(eol)
            .child(language_mode);
        if let Some(mode) = mode {
            right = right.child(mode);
        }
        right
    };

    let status = StatusBar::new()
        .left(left)
        .right(right)
        .bg(rgb(palette.chrome))
        .border_color(rgb(palette.border))
        .text_color(rgb(palette.secondary))
        .text_size(px(11.))
        .px(px(14.))
        .h(px(28.));
    status.into_any_element()
}

fn render_problems_shortcut(
    window: &DopadWindow,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) -> impl IntoElement + use<> {
    let counts = window.current_diagnostic_counts();
    let active = window.bottom_panel.active == Some(BottomPanelView::Problems);
    Button::new("status-problems")
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(3.))
                .child(
                    Icon::new(IconName::CircleX)
                        .with_size(Size::Size(px(14.)))
                        .text_color(rgb(if counts.errors == 0 {
                            palette.secondary
                        } else {
                            0xB04B43
                        })),
                )
                .child(
                    div()
                        .text_color(rgb(palette.secondary))
                        .child(counts.errors.to_string()),
                )
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .with_size(Size::Size(px(14.)))
                        .text_color(rgb(if counts.warnings == 0 {
                            palette.secondary
                        } else {
                            0x9A6A22
                        })),
                )
                .child(
                    div()
                        .text_color(rgb(palette.secondary))
                        .child(counts.warnings.to_string()),
                ),
        )
        .xsmall()
        .compact()
        .ghost()
        .when(active, |button| {
            button
                .bg(rgb(palette.selection))
                .text_color(rgb(palette.accent))
        })
        .tooltip(format!(
            "Problems: {} errors, {} warnings",
            counts.errors, counts.warnings
        ))
        .accessibility_label(format!(
            "Problems, {} errors, {} warnings",
            counts.errors, counts.warnings
        ))
        .on_click(cx.listener(|this, _, window, cx| this.toggle_problems(window, cx)))
}

fn render_language_menu(
    label: &'static str,
    document_id: Option<u64>,
    auto_detect: bool,
    selected: Option<Language>,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) -> impl IntoElement + use<> {
    let entity = cx.entity();
    Button::new("language-mode")
        .child(
            div().flex().items_center().gap_1().child(label).child(
                Icon::new(IconName::ChevronDown)
                    .with_size(Size::Size(px(12.)))
                    .text_color(rgb(palette.secondary)),
            ),
        )
        .xsmall()
        .compact()
        .ghost()
        .text_color(rgb(palette.secondary))
        .tooltip("Select language mode")
        .accessibility_label(format!("Language mode: {label}"))
        .disabled(document_id.is_none())
        .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
            let auto_entity = entity.clone();
            let auto_item = PopupMenuItem::new("Auto Detect")
                .checked(auto_detect)
                .on_click(move |_, _, app| {
                    if let Some(document_id) = document_id {
                        auto_entity.update(app, |this, cx| {
                            this.set_language_mode(document_id, None, cx);
                        });
                    }
                });
            menu = menu.item(auto_item).separator();

            for language in Language::choices() {
                let language = *language;
                let item_entity = entity.clone();
                let item = PopupMenuItem::new(language.display_name())
                    .checked(!auto_detect && selected == Some(language))
                    .on_click(move |_, _, app| {
                        if let Some(document_id) = document_id {
                            item_entity.update(app, |this, cx| {
                                this.set_language_mode(document_id, Some(language), cx);
                            });
                        }
                    });
                menu = menu.item(item);
            }
            menu.scrollable(true)
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn syntax_status_uses_the_language_label_instead_of_the_filename_extension() {
        assert_eq!(dopad_syntax::Language::Markdown.display_name(), "Markdown");
        assert_eq!(
            dopad_syntax::Language::PowerShell.display_name(),
            "PowerShell"
        );
    }
}

