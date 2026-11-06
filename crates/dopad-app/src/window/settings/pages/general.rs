use std::rc::Rc;

use gpui_kit::component::Disableable;
use gpui_kit::{App, Context, IntoElement, ParentElement, Styled, div, px, rgb};

use super::super::{
    components::{BoolChangeHandler, ChangeHandler, check_row, checkbox, row, section, select},
    theme,
    window::SettingsWindow,
};

pub(in crate::window::settings) fn render(
    this: &SettingsWindow,
    cx: &mut Context<SettingsWindow>,
) -> impl IntoElement {
    let entity = cx.entity();
    let confirm = Rc::new(move |value: bool, app: &mut App| {
        entity.update(app, |this, cx| {
            this.draft.confirm_before_close = value;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let restore = Rc::new(move |value: bool, app: &mut App| {
        entity.update(app, |this, cx| {
            this.draft.restore_session = value;
            if value {
                this.draft.show_recent_files = false;
            }
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let startup_change: ChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.restore_session = value == "Restore last session";
            this.draft.show_recent_files = value == "Open recent files";
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let default_type: ChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.default_file_type = value.to_owned();
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let show_recent: BoolChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.show_recent_files = value;
            if value {
                this.draft.restore_session = false;
            }
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let auto_save: BoolChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.auto_save = value;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let external_changes: BoolChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.detect_external_changes = value;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let auto_updates: BoolChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.auto_check_updates = value;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let save_interval: ChangeHandler = Rc::new(move |value, app| {
        let seconds = match value {
            "30 seconds" => 30,
            "1 minute" => 60,
            "3 minutes" => 180,
            "5 minutes" => 300,
            "10 minutes" => 600,
            _ => return,
        };
        entity.update(app, |this, cx| {
            this.draft.save_interval_seconds = seconds;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let recent_limit: ChangeHandler = Rc::new(move |value, app| {
        if let Ok(value) = value.parse::<u16>() {
            entity.update(app, |this, cx| {
                this.draft.recent_file_limit = value;
                this.draft.recent_files.truncate(usize::from(value));
                this.error = None;
                cx.notify();
            });
        }
    });
    let startup_value = if this.draft.restore_session {
        "Restore last session"
    } else if this.draft.show_recent_files {
        "Open recent files"
    } else {
        "Open blank document"
    };
    let default_type_value = match this.draft.default_file_type.as_str() {
        "Markdown" => "Markdown",
        "JSON" => "JSON",
        "XML" => "XML",
        _ => "Plain Text",
    };
    let interval_value = match this.draft.save_interval_seconds {
        30 => "30 seconds",
        60 => "1 minute",
        300 => "5 minutes",
        600 => "10 minutes",
        _ => "3 minutes",
    };
    let recent_limit_value = this.draft.recent_file_limit.to_string();
    let clear_recent = cx.entity();

    let startup = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(row(
            "On startup",
            select(
                "settings-startup-mode",
                startup_value,
                vec![
                    ("Restore last session", true),
                    ("Open blank document", true),
                    ("Open recent files", true),
                ],
                360.,
                true,
                Some(startup_change),
            ),
        ))
        .child(row(
            "New file default type",
            select(
                "settings-default-type",
                default_type_value,
                vec![
                    ("Plain Text", true),
                    ("Markdown", true),
                    ("JSON", true),
                    ("XML", true),
                ],
                360.,
                true,
                Some(default_type),
            ),
        ))
        .child(check_row(
            "settings-reopen-session",
            "Reopen files from the previous session",
            this.draft.restore_session,
            true,
            Some(restore),
        ))
        .child(check_row(
            "settings-show-recent",
            "Show recent files on startup",
            this.draft.show_recent_files,
            true,
            Some(show_recent),
        ));

    let behavior = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(check_row(
            "settings-confirm-close",
            "Confirm before closing tabs with unsaved changes",
            this.draft.confirm_before_close,
            true,
            Some(confirm),
        ))
        .child(check_row(
            "settings-detect-external",
            "Detect when a file is modified from outside",
            this.draft.detect_external_changes,
            true,
            Some(external_changes),
        ))
        .child(check_row(
            "settings-auto-check-updates",
            "Automatically check for updates",
            this.draft.auto_check_updates,
            true,
            Some(auto_updates),
        ))
        .child(
            div()
                .w_full()
                .h(px(40.))
                .flex()
                .items_center()
                .justify_between()
                .child(checkbox(
                    "settings-auto-save",
                    "Auto-save files",
                    this.draft.auto_save,
                    true,
                    Some(auto_save),
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .text_size(px(14.))
                        .text_color(rgb(theme::TEXT_PRIMARY))
                        .child("Every")
                        .child(select(
                            "settings-save-interval",
                            interval_value,
                            vec![
                                ("30 seconds", true),
                                ("1 minute", true),
                                ("3 minutes", true),
                                ("5 minutes", true),
                                ("10 minutes", true),
                            ],
                            170.,
                            true,
                            Some(save_interval),
                        )),
                ),
        );

    let recent_files = div()
        .w_full()
        .h(px(40.))
        .flex()
        .items_center()
        .gap_4()
        .child(
            div()
                .flex_1()
                .text_size(px(14.))
                .text_color(rgb(theme::TEXT_PRIMARY))
                .child("Maximum number of recent files"),
        )
        .child(select(
            "settings-recent-limit",
            recent_limit_value.clone(),
            vec![("10", true), ("20", true), ("50", true), ("100", true)],
            170.,
            true,
            Some(recent_limit),
        ))
        .child(
            gpui_kit::component::button::Button::new("clear-recent-files")
                .label("Clear recent files")
                .outline()
                .disabled(this.draft.recent_files.is_empty())
                .h(px(36.))
                .w(px(170.))
                .on_click(move |_, _, app| {
                    clear_recent.update(app, |this, cx| {
                        this.draft.recent_files.clear();
                        this.error = None;
                        cx.notify();
                    });
                }),
        );

    div()
        .flex()
        .flex_col()
        .gap_1()
        .text_color(rgb(theme::TEXT_PRIMARY))
        .child(section("Startup", startup))
        .child(section("Behavior", behavior))
        .child(section("Recent Files", recent_files))
}
