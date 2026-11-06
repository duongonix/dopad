use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::{Context, IntoElement, ParentElement, Styled, div, px, rgb};

use super::super::{
    components::{BoolChangeHandler, ChangeHandler, check_row, row, section, select},
    theme,
    window::SettingsWindow,
};

pub(in crate::window::settings) fn render(
    this: &SettingsWindow,
    cx: &mut Context<SettingsWindow>,
) -> impl IntoElement {
    let entity = cx.entity();
    let large_file_mode: ChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.large_file_mode = value == "Automatic";
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let syntax_threshold: ChangeHandler = Rc::new(move |value, app| {
        let value = value.trim_end_matches(" MiB");
        if let Ok(value) = value.parse::<u16>() {
            entity.update(app, |this, cx| {
                this.draft.syntax_highlighting_threshold_mb = value;
                this.error = None;
                cx.notify();
            });
        }
    });
    let entity = cx.entity();
    let preview_cache: BoolChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.preview_cache_enabled = value;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let logging_level: ChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.logging_level = value.to_owned();
            this.error = None;
            cx.notify();
        });
    });
    let threshold_value = format!("{} MiB", this.draft.syntax_highlighting_threshold_mb);
    let logging_value = match this.draft.logging_level.as_str() {
        "Error" => "Error",
        "Info" => "Info",
        "Debug" => "Debug",
        "Trace" => "Trace",
        _ => "Warn",
    };
    let advanced = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(row(
            "Large file mode",
            select(
                "advanced-large-file",
                if this.draft.large_file_mode {
                    "Automatic"
                } else {
                    "Off"
                },
                vec![("Automatic", true), ("Off", true)],
                170.,
                true,
                Some(large_file_mode),
            ),
        ))
        .child(row(
            "Syntax highlighting threshold",
            select(
                "advanced-syntax-threshold",
                threshold_value,
                vec![
                    ("1 MiB", true),
                    ("5 MiB", true),
                    ("10 MiB", true),
                    ("25 MiB", true),
                    ("50 MiB", true),
                ],
                170.,
                true,
                Some(syntax_threshold),
            ),
        ))
        .child(check_row(
            "advanced-preview-cache",
            "Keep preview results cached",
            this.draft.preview_cache_enabled,
            true,
            Some(preview_cache),
        ))
        .child(row(
            "Logging level",
            select(
                "advanced-log-level",
                logging_value,
                vec![
                    ("Error", true),
                    ("Warn", true),
                    ("Info", true),
                    ("Debug", true),
                    ("Trace", true),
                ],
                170.,
                true,
                Some(logging_level),
            ),
        ));
    let entity = cx.entity();
    let reset = Button::new("reset-settings")
        .label("Reset settings")
        .outline()
        .h(px(36.))
        .on_click(move |_, _, app| {
            entity.update(app, |this, cx| {
                this.draft = Default::default();
                this.error = None;
                cx.notify();
            });
        });
    let reset_section = div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .text_size(px(14.))
        .text_color(rgb(theme::TEXT_PRIMARY))
        .child("Restore supported settings to their defaults")
        .child(reset);
    let _ = this;
    div()
        .flex()
        .flex_col()
        .child(section("Performance", advanced))
        .child(section("Reset", reset_section))
}
