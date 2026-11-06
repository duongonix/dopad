use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Disableable, Icon,
    button::Button,
    checkbox::Checkbox,
    menu::{DropdownMenu, PopupMenuItem},
    select::Select,
};
use gpui_kit::{App, Entity, IntoElement, ParentElement, Styled, div, px, rgb};

use super::{selects::SettingSelectState, theme};

pub(super) type ChangeHandler = Rc<dyn Fn(&str, &mut App)>;
pub(super) type BoolChangeHandler = Rc<dyn Fn(bool, &mut App)>;

pub(super) fn section(title: &'static str, body: impl IntoElement) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .mb(px(26.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(16.))
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::TEXT_PRIMARY))
                        .child(title),
                )
                .child(div().h(px(1.)).w_full().bg(rgb(theme::BORDER))),
        )
        .child(body)
}

pub(super) fn row(label: &'static str, control: impl IntoElement) -> impl IntoElement {
    div()
        .h(px(40.))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex_1()
                .text_size(px(14.))
                .text_color(rgb(theme::TEXT_PRIMARY))
                .child(label),
        )
        .child(control)
}

pub(super) fn check_row(
    id: &'static str,
    label: &'static str,
    checked: bool,
    enabled: bool,
    change: Option<BoolChangeHandler>,
) -> impl IntoElement {
    div()
        .h(px(36.))
        .w_full()
        .flex()
        .items_center()
        .child(checkbox(id, label, checked, enabled, change))
}

pub(super) fn check_row_with_description(
    id: &'static str,
    label: &'static str,
    description: &'static str,
    checked: bool,
    enabled: bool,
    change: Option<BoolChangeHandler>,
) -> impl IntoElement {
    let checkbox = Checkbox::new(id)
        .label(label)
        .accessibility_label(label)
        .checked(checked)
        .disabled(!enabled);
    let checkbox = if let Some(change) = change {
        checkbox.on_change(move |value, _, app| change(*value, app))
    } else {
        checkbox
    };

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .child(checkbox)
        .child(
            div()
                .pl(px(26.))
                .text_size(px(12.))
                .text_color(rgb(theme::TEXT_SECONDARY))
                .child(description),
        )
}

pub(super) fn checkbox(
    id: &'static str,
    label: &'static str,
    checked: bool,
    enabled: bool,
    change: Option<BoolChangeHandler>,
) -> impl IntoElement {
    let checkbox = Checkbox::new(id)
        .label(label)
        .checked(checked)
        .disabled(!enabled);
    if let Some(change) = change {
        checkbox.on_change(move |value, _, app| change(*value, app))
    } else {
        checkbox
    }
}

pub(super) fn select(
    id: &'static str,
    value: impl Into<String>,
    options: Vec<(impl Into<String>, bool)>,
    width: f32,
    enabled: bool,
    change: Option<ChangeHandler>,
) -> impl IntoElement {
    let value = value.into();
    let options = options
        .into_iter()
        .map(|(label, available)| (label.into(), available))
        .collect::<Vec<(String, bool)>>();
    let mut button = Button::new(id)
        .accessibility_label(format!("Select {value}"))
        .outline()
        .disabled(!enabled)
        .w(px(width))
        .h(px(34.))
        .px(px(10.))
        .bg(rgb(theme::BG_PRIMARY))
        .border_1()
        .border_color(rgb(theme::INPUT_BORDER));
    if !enabled {
        button = button.tooltip("This setting is not available yet");
    }
    button
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .flex_1()
                        .text_size(px(14.))
                        .text_color(rgb(theme::TEXT_PRIMARY))
                        .child(value.clone()),
                )
                .child(
                    Icon::new(IconName::ChevronDown)
                        .size(px(16.))
                        .text_color(rgb(theme::TEXT_SECONDARY)),
                ),
        )
        .dropdown_menu(move |mut menu, _, _| {
            for (label, available) in &options {
                let label = label.clone();
                let change = change.clone();
                menu = menu.item(
                    PopupMenuItem::new(label.clone())
                        .checked(label == value)
                        .disabled(!available || change.is_none())
                        .on_click(move |_, _, app| {
                            if let Some(change) = &change {
                                change(&label, app);
                            }
                        }),
                );
            }
            menu
        })
}

pub(super) fn kit_select(
    id: &'static str,
    accessibility_label: impl Into<gpui_kit::SharedString>,
    search_placeholder: impl Into<gpui_kit::SharedString>,
    state: &Entity<SettingSelectState>,
    width: f32,
) -> impl IntoElement {
    div().w(px(width)).h(px(34.)).flex_shrink_0().child(
        Select::new(state)
            .id(id)
            .accessibility_label(accessibility_label)
            .search_placeholder(search_placeholder)
            .menu_max_h(px(320.))
            .appearance(false)
            .size_full()
            .px(px(10.))
            .border_1()
            .border_color(rgb(theme::INPUT_BORDER))
            .bg(rgb(theme::BG_PRIMARY))
            .text_size(px(14.))
            .text_color(rgb(theme::TEXT_PRIMARY)),
    )
}

pub(super) fn unavailable(value: &'static str) -> impl IntoElement {
    div()
        .w(px(170.))
        .h(px(34.))
        .flex()
        .items_center()
        .justify_between()
        .px(px(10.))
        .rounded_md()
        .border_1()
        .border_color(rgb(theme::INPUT_BORDER))
        .text_size(px(14.))
        .text_color(rgb(theme::TEXT_SECONDARY))
        .child(value)
        .child(
            Icon::new(IconName::ChevronDown)
                .size(px(16.))
                .text_color(rgb(theme::TEXT_SECONDARY)),
        )
}
