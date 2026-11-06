use std::rc::Rc;

use dopad_core::{Encoding, EolKind};
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
    let encoding_change: ChangeHandler = Rc::new(move |value, app| {
        let encoding = match value {
            "UTF-8" => Encoding::Utf8,
            "UTF-8 BOM" => Encoding::Utf8Bom,
            "UTF-16 LE" => Encoding::Utf16Le,
            "UTF-16 BE" => Encoding::Utf16Be,
            _ => return,
        };
        entity.update(app, |this, cx| {
            this.draft.default_encoding = encoding;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let bom_change: ChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.bom_behavior = value.to_owned();
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let eol_change: ChangeHandler = Rc::new(move |value, app| {
        let eol = match value {
            "LF" => EolKind::Lf,
            "CRLF" => EolKind::CrLf,
            "CR" => EolKind::Cr,
            _ => return,
        };
        entity.update(app, |this, cx| {
            this.draft.default_line_ending = eol;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let detect: BoolChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.detect_encoding = value;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let preserve: BoolChangeHandler = Rc::new(move |value, app| {
        entity.update(app, |this, cx| {
            this.draft.preserve_encoding = value;
            this.error = None;
            cx.notify();
        });
    });
    let encoding_value = match this.draft.default_encoding {
        Encoding::Utf8 => "UTF-8",
        Encoding::Utf8Bom => "UTF-8 BOM",
        Encoding::Utf16Le | Encoding::Utf16LeNoBom => "UTF-16 LE",
        Encoding::Utf16Be | Encoding::Utf16BeNoBom => "UTF-16 BE",
    };
    let eol_value = match this.draft.default_line_ending {
        EolKind::CrLf => "CRLF",
        EolKind::Cr => "CR",
        _ => "LF",
    };
    let encoding = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(row(
            "Default encoding",
            select(
                "encoding-default",
                encoding_value,
                vec![
                    ("UTF-8", true),
                    ("UTF-8 BOM", true),
                    ("UTF-16 LE", true),
                    ("UTF-16 BE", true),
                ],
                220.,
                true,
                Some(encoding_change),
            ),
        ))
        .child(row(
            "BOM behavior",
            select(
                "encoding-bom",
                this.draft.bom_behavior.as_str(),
                vec![
                    ("Preserve if present", true),
                    ("Always include BOM", true),
                    ("Never include BOM", true),
                ],
                220.,
                true,
                Some(bom_change),
            ),
        ))
        .child(row(
            "Default line ending",
            select(
                "encoding-default-eol",
                eol_value,
                vec![("LF", true), ("CRLF", true), ("CR", true)],
                170.,
                true,
                Some(eol_change),
            ),
        ));
    let detection = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(check_row(
            "encoding-detect",
            "Detect encoding automatically",
            this.draft.detect_encoding,
            true,
            Some(detect),
        ))
        .child(check_row(
            "encoding-preserve",
            "Preserve original encoding when saving",
            this.draft.preserve_encoding,
            true,
            Some(preserve),
        ));
    div()
        .flex()
        .flex_col()
        .child(section("Default", encoding))
        .child(section("Detection", detection))
}
