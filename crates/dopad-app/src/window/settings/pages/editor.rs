use std::rc::Rc;

use gpui_kit::{App, Context, IntoElement, ParentElement, Styled, div};

use super::super::{
    components::{
        ChangeHandler, check_row, check_row_with_description, kit_select, row, section, select,
    },
    font_download::{self, FontTarget},
    window::SettingsWindow,
};

pub(in crate::window::settings) fn render(
    this: &SettingsWindow,
    cx: &mut Context<SettingsWindow>,
) -> impl IntoElement {
    let entity = cx.entity();
    let wrap = Rc::new(move |value: bool, app: &mut App| {
        entity.update(app, |this, cx| {
            this.draft.word_wrap = value;
            this.error = None;
            this.preview_word_wrap(value, cx);
            cx.notify();
        });
    });
    let entity = cx.entity();
    let auto_close = Rc::new(move |value: bool, app: &mut App| {
        entity.update(app, |this, cx| {
            this.draft.auto_close_brackets = value;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let smart_indent = Rc::new(move |value: bool, app: &mut App| {
        entity.update(app, |this, cx| {
            this.draft.smart_indent = value;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let indent_guides = Rc::new(move |value: bool, app: &mut App| {
        entity.update(app, |this, cx| {
            this.draft.indent_guides = value;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let line_numbers = Rc::new(move |value: bool, app: &mut App| {
        entity.update(app, |this, cx| {
            this.draft.show_line_numbers = value;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let insert_spaces = Rc::new(move |value: bool, app: &mut App| {
        entity.update(app, |this, cx| {
            this.draft.insert_spaces = value;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let font_size: ChangeHandler = Rc::new(move |value, app| {
        if let Ok(value) = value.parse::<u8>() {
            entity.update(app, |this, cx| {
                this.draft.editor_font_size = value;
                this.error = None;
                cx.notify();
            });
        }
    });
    let entity = cx.entity();
    let font_weight: ChangeHandler = Rc::new(move |value, app| {
        let weight = match value {
            "Medium" => 500,
            "Semibold" => 600,
            "Bold" => 700,
            "Extra Bold" => 800,
            _ => 400,
        };
        entity.update(app, |this, cx| {
            this.draft.editor_font_weight = weight;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let line_height: ChangeHandler = Rc::new(move |value, app| {
        if let Ok(value) = value.parse::<f32>() {
            entity.update(app, |this, cx| {
                this.draft.editor_line_height = (value * 100.0).round() as u8;
                this.error = None;
                cx.notify();
            });
        }
    });
    let entity = cx.entity();
    let tab_size: ChangeHandler = Rc::new(move |value, app| {
        if let Ok(value) = value.parse::<u8>() {
            entity.update(app, |this, cx| {
                this.draft.tab_size = value;
                this.error = None;
                cx.notify();
            });
        }
    });
    let size = this.draft.editor_font_size.to_string();
    let font_weight_value = match this.draft.editor_font_weight {
        500 => "Medium",
        600 => "Semibold",
        700 => "Bold",
        800 => "Extra Bold",
        _ => "Regular",
    };
    let line_height_value = match this.draft.editor_line_height {
        125 => "1.25",
        175 => "1.75",
        200 => "2.0",
        _ => "1.5",
    };
    let tab_size_value = this.draft.tab_size.to_string();

    let font_family_row = row(
        "Font family",
        kit_select(
            "settings-font-family",
            "Editor font family",
            "Search fonts...",
            &this.selects.editor_font,
            220.,
        ),
    );
    let font_size_row = row(
        "Font size",
        select(
            "settings-font-size",
            size.clone(),
            vec![
                ("10", true),
                ("11", true),
                ("12", true),
                ("13", true),
                ("14", true),
                ("15", true),
                ("16", true),
                ("18", true),
                ("20", true),
                ("22", true),
                ("24", true),
            ],
            170.,
            true,
            Some(font_size),
        ),
    );
    let font_weight_row = row(
        "Font weight",
        select(
            "settings-font-weight",
            font_weight_value,
            vec![
                ("Regular", true),
                ("Medium", true),
                ("Semibold", true),
                ("Bold", true),
                ("Extra Bold", true),
            ],
            170.,
            true,
            Some(font_weight),
        ),
    );
    let line_height_row = row(
        "Line height",
        select(
            "settings-line-height",
            line_height_value,
            vec![("1.25", true), ("1.5", true), ("1.75", true), ("2.0", true)],
            170.,
            true,
            Some(line_height),
        ),
    );
    let tab_size_row = row(
        "Tab size",
        select(
            "settings-tab-size",
            tab_size_value,
            vec![
                ("2", true),
                ("3", true),
                ("4", true),
                ("5", true),
                ("6", true),
                ("8", true),
            ],
            170.,
            true,
            Some(tab_size),
        ),
    );

    let mut typography = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(font_family_row);
    if let Some(status) = this
        .font_install
        .as_ref()
        .filter(|status| status.target == FontTarget::Editor)
    {
        typography = typography.child(font_download::render_status(status));
    }
    typography = typography
        .child(font_size_row)
        .child(font_weight_row)
        .child(line_height_row)
        .child(tab_size_row);

    let editing = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(check_row(
            "settings-insert-spaces",
            "Insert spaces instead of tabs",
            this.draft.insert_spaces,
            true,
            Some(insert_spaces),
        ))
        .child(check_row_with_description(
            "settings-word-wrap",
            "Word Wrap",
            "Wrap long lines to fit within the editor width.",
            this.draft.word_wrap,
            true,
            Some(wrap),
        ))
        .child(check_row(
            "settings-line-numbers",
            "Show line numbers",
            this.draft.show_line_numbers,
            true,
            Some(line_numbers),
        ))
        .child(check_row(
            "settings-auto-indent",
            "Auto indent",
            this.draft.smart_indent,
            true,
            Some(smart_indent),
        ))
        .child(check_row(
            "settings-auto-brackets",
            "Auto close brackets",
            this.draft.auto_close_brackets,
            true,
            Some(auto_close),
        ))
        .child(check_row(
            "settings-indent-guides",
            "Show indentation guides",
            this.draft.indent_guides,
            true,
            Some(indent_guides),
        ));

    div()
        .flex()
        .flex_col()
        .child(section("Typography", typography))
        .child(section("Editing", editing))
}
