use std::rc::Rc;

use gpui_kit::App;
use gpui_kit::{Context, IntoElement, ParentElement, Styled, div};

use super::super::{
    components::{BoolChangeHandler, ChangeHandler, check_row, row, section, select},
    window::SettingsWindow,
};

pub(in crate::window::settings) fn render(
    this: &SettingsWindow,
    cx: &mut Context<SettingsWindow>,
) -> impl IntoElement {
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
    let auto_save: BoolChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.auto_save = value;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let detect_external: BoolChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.detect_external_changes = value;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let interval_change: ChangeHandler = Rc::new(move |value, app| {
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
    let extension_change: ChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.default_file_extension = match value {
                "None" => String::new(),
                extension => extension.to_owned(),
            };
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let recent_limit_change: ChangeHandler = Rc::new(move |value, app| {
        if let Ok(value) = value.parse::<u16>() {
            entity.update(app, |this, cx| {
                this.draft.recent_file_limit = value;
                this.draft.recent_files.truncate(usize::from(value));
                this.error = None;
                cx.notify();
            });
        }
    });
    let interval_value = match this.draft.save_interval_seconds {
        30 => "30 seconds",
        60 => "1 minute",
        300 => "5 minutes",
        600 => "10 minutes",
        _ => "3 minutes",
    };
    let extension_value = match this.draft.default_file_extension.as_str() {
        "txt" => "txt",
        "md" => "md",
        "json" => "json",
        "xml" => "xml",
        _ => "None",
    };
    let recent_limit = this.draft.recent_file_limit.to_string();
    let saving = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(check_row(
            "files-auto-save",
            "Auto-save files",
            this.draft.auto_save,
            true,
            Some(auto_save),
        ))
        .child(row(
            "Save interval",
            select(
                "files-save-interval",
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
                Some(interval_change),
            ),
        ))
        .child(check_row(
            "files-session-restore",
            "Restore previous session",
            this.draft.restore_session,
            true,
            Some(restore),
        ))
        .child(check_row(
            "files-change-detection",
            "Detect external file changes",
            this.draft.detect_external_changes,
            true,
            Some(detect_external),
        ));
    let defaults = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(row(
            "Default file extension",
            select(
                "files-default-extension",
                extension_value,
                vec![
                    ("None", true),
                    ("txt", true),
                    ("md", true),
                    ("json", true),
                    ("xml", true),
                ],
                170.,
                true,
                Some(extension_change),
            ),
        ))
        .child(row(
            "Recent files limit",
            select(
                "files-recent-limit",
                recent_limit,
                vec![("10", true), ("20", true), ("50", true), ("100", true)],
                170.,
                true,
                Some(recent_limit_change),
            ),
        ));
    div()
        .flex()
        .flex_col()
        .child(section("Saving & Recovery", saving))
        .child(section("Defaults", defaults))
}
