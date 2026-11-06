use std::rc::Rc;

use gpui_kit::{Context, IntoElement, ParentElement, Styled, div};

use super::super::{
    components::{ChangeHandler, kit_select, row, section, select},
    font_download::{self, FontTarget},
    window::SettingsWindow,
};

pub(in crate::window::settings) fn render(
    this: &SettingsWindow,
    cx: &mut Context<SettingsWindow>,
) -> impl IntoElement {
    let entity = cx.entity();
    let font_size: ChangeHandler = Rc::new(move |value, app| {
        if let Ok(value) = value.parse::<u8>() {
            entity.update(app, |this, cx| {
                this.draft.terminal_font_size = value;
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
            this.draft.terminal_font_weight = weight;
            this.error = None;
            cx.notify();
        });
    });
    let entity = cx.entity();
    let line_height: ChangeHandler = Rc::new(move |value, app| {
        if let Ok(value) = value.parse::<u8>() {
            entity.update(app, |this, cx| {
                this.draft.terminal_line_height = value;
                this.error = None;
                cx.notify();
            });
        }
    });
    let entity = cx.entity();
    let cell_width: ChangeHandler = Rc::new(move |value, app| {
        if let Ok(value) = value.trim_end_matches('%').parse::<u8>() {
            entity.update(app, |this, cx| {
                this.draft.terminal_cell_width = value;
                this.error = None;
                cx.notify();
            });
        }
    });

    let font_weight_value = match this.draft.terminal_font_weight {
        500 => "Medium",
        600 => "Semibold",
        700 => "Bold",
        800 => "Extra Bold",
        _ => "Regular",
    };
    let font_size_value = this.draft.terminal_font_size.to_string();
    let line_height_value = format!("{}%", this.draft.terminal_line_height);
    let cell_width_value = format!("{}%", this.draft.terminal_cell_width);

    let mut typography = div().w_full().flex().flex_col().gap_2().child(row(
        "Font face",
        kit_select(
            "terminal-font-family",
            "Terminal font family",
            "Search fonts...",
            &this.selects.terminal_font,
            220.,
        ),
    ));
    if let Some(status) = this
        .font_install
        .as_ref()
        .filter(|status| status.target == FontTarget::Terminal)
    {
        typography = typography.child(font_download::render_status(status));
    }
    typography = typography
        .child(row(
            "Font size",
            select(
                "terminal-font-size",
                font_size_value,
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
        ))
        .child(row(
            "Font weight",
            select(
                "terminal-font-weight",
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
        ))
        .child(row(
            "Line height",
            select(
                "terminal-line-height",
                line_height_value.as_str(),
                vec![
                    ("100%", true),
                    ("110%", true),
                    ("120%", true),
                    ("130%", true),
                    ("140%", true),
                    ("150%", true),
                    ("160%", true),
                    ("175%", true),
                    ("200%", true),
                ],
                170.,
                true,
                Some(line_height),
            ),
        ))
        .child(row(
            "Cell width",
            select(
                "terminal-cell-width",
                cell_width_value.as_str(),
                vec![
                    ("100%", true),
                    ("110%", true),
                    ("120%", true),
                    ("130%", true),
                    ("140%", true),
                ],
                170.,
                true,
                Some(cell_width),
            ),
        ));

    div()
        .flex()
        .flex_col()
        .child(section("Typography", typography))
}
