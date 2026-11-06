use std::rc::Rc;

use gpui_kit::{App, Context, IntoElement, ParentElement, Styled, div};

use super::super::{
    components::{ChangeHandler, check_row, row, section, select, unavailable},
    window::SettingsWindow,
};

pub(in crate::window::settings) fn render(
    this: &SettingsWindow,
    cx: &mut Context<SettingsWindow>,
) -> impl IntoElement {
    let entity = cx.entity();
    let show_status_bar = Rc::new(move |value: bool, app: &mut App| {
        entity.update(app, |this, cx| {
            this.draft.show_status_bar = value;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let ui_font_size: ChangeHandler = Rc::new(move |value, app| {
        if let Ok(value) = value.parse::<u8>() {
            entity.update(app, |this, cx| {
                this.draft.ui_font_size = value;
                this.error = None;
                cx.notify();
            });
        }
    });
    let entity = cx.entity();
    let compact_tabs: ChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.compact_tabs = value == "Compact";
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let file_icon_theme: ChangeHandler = Rc::new(move |value, app| {
        if value == "Material Icon Theme" {
            entity.update(app, |this, cx| {
                this.draft.file_icon_theme = "material".into();
                this.error = None;
                cx.notify();
            });
        }
    });
    let ui_size = this.draft.ui_font_size.to_string();
    let tab_appearance = if this.draft.compact_tabs {
        "Compact"
    } else {
        "Comfortable"
    };
    let file_icon_theme_value = match this.draft.file_icon_theme.as_str() {
        "material" => "Material Icon Theme",
        _ => "Material Icon Theme",
    };
    let theme = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(row("Theme", unavailable("Light")))
        .child(row(
            "File Icon Theme",
            select(
                "settings-file-icon-theme",
                file_icon_theme_value,
                vec![("Material Icon Theme", true)],
                220.,
                true,
                Some(file_icon_theme),
            ),
        ))
        .child(row(
            "UI font size",
            select(
                "settings-ui-font-size",
                ui_size,
                vec![("12", true), ("14", true), ("16", true), ("18", true)],
                170.,
                true,
                Some(ui_font_size),
            ),
        ));
    let layout = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(check_row(
            "settings-status-bar",
            "Show status bar",
            this.draft.show_status_bar,
            true,
            Some(show_status_bar),
        ))
        .child(row(
            "Tab appearance",
            select(
                "settings-tab-appearance",
                tab_appearance,
                vec![("Compact", true), ("Comfortable", true)],
                220.,
                true,
                Some(compact_tabs),
            ),
        ));
    div()
        .flex()
        .flex_col()
        .child(section("Theme", theme))
        .child(section("Layout", layout))
}
