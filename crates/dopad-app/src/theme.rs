use std::sync::Arc;

use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::{Anchor, App, px, rgb};

#[derive(Clone, Copy)]
pub struct Palette {
    pub canvas: u32,
    pub chrome: u32,
    pub text: u32,
    pub secondary: u32,
    pub tab_active_text: u32,
    pub tab_dirty: u32,
    pub tab_hover: u32,
    pub muted: u32,
    pub gutter: u32,
    pub accent: u32,
    pub border: u32,
    pub selection: u32,
    pub search_current: u32,
    pub active_line: u32,
    pub syntax_function: u32,
    pub syntax_type: u32,
    pub syntax_variable: u32,
    pub syntax_parameter: u32,
    pub syntax_property: u32,
    pub syntax_constant: u32,
    pub syntax_keyword: u32,
    pub syntax_string: u32,
    pub syntax_string_escape: u32,
    pub syntax_string_regex: u32,
    pub syntax_boolean: u32,
    pub syntax_comment: u32,
    pub syntax_number: u32,
    pub syntax_attribute: u32,
    pub syntax_tag: u32,
    pub syntax_html_attribute: u32,
    pub syntax_heading: u32,
    pub syntax_link: u32,
    pub syntax_invalid: u32,
    pub syntax_operator: u32,
    pub syntax_punctuation: u32,
    pub syntax_lifetime: u32,
}

const TOKYO_NIGHT: Palette = Palette {
    canvas: 0x1A1B26,
    chrome: 0x24283B,
    text: 0xC0CAF5,
    secondary: 0x9AA5CE,
    tab_active_text: 0xC0CAF5,
    tab_dirty: 0xC0CAF5,
    tab_hover: 0x2C3046,
    muted: 0x9AA5CE,
    gutter: 0x565F89,
    accent: 0x7AA2F7,
    border: 0x414868,
    selection: 0x364A73,
    search_current: 0x66533F,
    active_line: 0x202231,
    syntax_function: 0xE0AF68,
    syntax_type: 0x7AA2F7,
    syntax_variable: 0xC0CAF5,
    syntax_parameter: 0xE0AF68,
    syntax_property: 0x73DACA,
    syntax_constant: 0xBB9AF7,
    syntax_keyword: 0xBB9AF7,
    syntax_string: 0x9ECE6A,
    syntax_string_escape: 0xE0AF68,
    syntax_string_regex: 0xB4F9F8,
    syntax_boolean: 0xBB9AF7,
    syntax_comment: 0x565F89,
    syntax_number: 0xFF9E64,
    syntax_attribute: 0xE0AF68,
    syntax_tag: 0xF7768E,
    syntax_html_attribute: 0x73DACA,
    syntax_heading: 0x7AA2F7,
    syntax_link: 0x7AA2F7,
    syntax_invalid: 0xF7768E,
    syntax_operator: 0x9AA5CE,
    syntax_punctuation: 0x9AA5CE,
    syntax_lifetime: 0xBB9AF7,
};

const WARM_PAPER: Palette = Palette {
    canvas: 0xFFF8E7,
    chrome: 0xF2E8CF,
    text: 0x38342F,
    secondary: 0x746F64,
    tab_active_text: 0x25221F,
    tab_dirty: 0x38342F,
    tab_hover: 0xF7EBD9,
    muted: 0xA9A08D,
    gutter: 0xA9A08D,
    accent: 0xB65F32,
    border: 0xD8CEBA,
    selection: 0xE8D6B7,
    search_current: 0xE5B98F,
    active_line: 0xFFF8E7,
    syntax_function: 0x275F80,
    syntax_type: 0x654C91,
    syntax_variable: 0x38342F,
    syntax_parameter: 0x574D40,
    syntax_property: 0x72512E,
    syntax_constant: 0x87529A,
    syntax_keyword: 0xA13D2D,
    syntax_string: 0x346A43,
    syntax_string_escape: 0x346A43,
    syntax_string_regex: 0x346A43,
    syntax_boolean: 0xA13D2D,
    syntax_comment: 0x787064,
    syntax_number: 0x8651A0,
    syntax_attribute: 0x9A5427,
    syntax_tag: 0x654C91,
    syntax_html_attribute: 0x72512E,
    syntax_heading: 0xA13D2D,
    syntax_link: 0x275F80,
    syntax_invalid: 0xA13D2D,
    syntax_operator: 0x514A42,
    syntax_punctuation: 0x655D51,
    syntax_lifetime: 0xA13D2D,
};

pub fn palette(dark: bool) -> &'static Palette {
    if dark { &TOKYO_NIGHT } else { &WARM_PAPER }
}

pub fn apply(dark: bool, cx: &mut App) {
    Theme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );
    let colors = palette(dark);
    let highlight_theme = if dark {
        None
    } else {
        match build_warm_paper_highlight_theme() {
            Ok(theme) => Some(Arc::new(theme)),
            Err(error) => {
                tracing::error!(error = %error, "failed to construct the Warm Paper syntax theme");
                None
            }
        }
    };
    Theme::update(cx, |theme| {
        theme.background = rgb(colors.canvas).into();
        theme.foreground = rgb(colors.text).into();
        theme.muted_foreground = rgb(colors.muted).into();
        theme.border = rgb(colors.border).into();
        theme.input = rgb(colors.border).into();
        theme.scrollbar = rgb(colors.canvas).into();
        theme.scrollbar_thumb = rgb(colors.border).into();
        theme.scrollbar_thumb_hover = rgb(colors.muted).into();
        let scrollbar_thumb: gpui_kit::Hsla = rgb(colors.border).into();
        let scrollbar_thumb_hover: gpui_kit::Hsla = rgb(colors.muted).into();
        theme.tokens.scrollbar_thumb = scrollbar_thumb.into();
        theme.tokens.scrollbar_thumb_hover = scrollbar_thumb_hover.into();
        theme.accent = rgb(colors.accent).into();
        theme.accent_foreground = rgb(colors.canvas).into();
        theme.primary = rgb(colors.accent).into();
        theme.primary_active = rgb(colors.accent).into();
        theme.primary_hover = rgb(colors.accent).into();
        theme.primary_foreground = rgb(colors.canvas).into();
        theme.selection = rgb(colors.selection).into();
        theme.caret = rgb(colors.accent).into();
        theme.title_bar = rgb(colors.chrome).into();
        theme.title_bar_border = rgb(colors.border).into();
        theme.status_bar = rgb(colors.chrome).into();
        theme.status_bar_border = rgb(colors.border).into();
        theme.notification.placement = Anchor::BottomRight;
        theme.notification.margins.bottom = px(44.);
        theme.notification.margins.right = px(16.);
        theme.notification.max_items = 1;
        theme.notification.width = px(360.);
        theme.tab_bar = rgb(colors.chrome).into();
        theme.tab = rgb(colors.chrome).into();
        theme.tab_active = rgb(colors.canvas).into();
        theme.tab_foreground = rgb(colors.secondary).into();
        theme.tab_active_foreground = rgb(colors.text).into();
        if let Some(highlight_theme) = highlight_theme {
            theme.highlight_theme = highlight_theme;
        }
        theme.radius = px(0.);
        theme.radius_lg = px(0.);
        theme.shadow = false;
    });
}

fn build_warm_paper_highlight_theme()
-> Result<gpui_kit::component::highlighter::HighlightTheme, serde_json::Error> {
    let colors = &WARM_PAPER;
    let color = |value: u32| format!("#{value:06X}");
    let syntax = [
        ("primary", colors.text),
        ("title", colors.syntax_heading),
        ("keyword", colors.syntax_keyword),
        ("boolean", colors.syntax_boolean),
        ("constant.builtin", colors.syntax_boolean),
        ("constant", colors.syntax_constant),
        ("constructor", colors.syntax_function),
        ("enum", colors.syntax_type),
        ("function", colors.syntax_function),
        ("property", colors.syntax_property),
        ("type", colors.syntax_type),
        ("variable", colors.syntax_variable),
        ("variable.special", colors.syntax_parameter),
        ("attribute", colors.syntax_attribute),
        ("tag", colors.syntax_tag),
        ("variant", colors.syntax_html_attribute),
        ("label", colors.syntax_lifetime),
        ("string", colors.syntax_string),
        ("string.escape", colors.syntax_string_escape),
        ("string.regex", colors.syntax_string_regex),
        ("string.special", colors.syntax_string_escape),
        ("string.special.symbol", colors.syntax_constant),
        ("comment", colors.syntax_comment),
        ("comment.doc", colors.syntax_comment),
        ("number", colors.syntax_number),
        ("link_text", colors.syntax_link),
        ("link_uri", colors.syntax_link),
        ("operator", colors.syntax_operator),
        ("punctuation", colors.syntax_punctuation),
        ("punctuation.bracket", colors.syntax_punctuation),
        ("punctuation.delimiter", colors.syntax_punctuation),
        ("punctuation.list_marker", colors.syntax_punctuation),
        ("punctuation.special", colors.syntax_punctuation),
        ("text.code.span", colors.text),
        ("text.literal", colors.text),
        ("emphasis", colors.text),
        ("emphasis.strong", colors.text),
        ("tag.doctype", colors.syntax_tag),
        ("hint", colors.syntax_invalid),
    ]
    .into_iter()
    .map(|(name, value)| {
        (
            name.to_owned(),
            serde_json::json!({ "color": color(value) }),
        )
    })
    .collect::<serde_json::Map<_, _>>();
    serde_json::from_value(serde_json::json!({
        "name": "Dopad Warm Paper",
        "appearance": "light",
        "style": {
            "editor.background": color(colors.canvas),
            "editor.foreground": color(colors.text),
            "editor.active_line.background": color(colors.active_line),
            "editor.line_number": color(colors.gutter),
            "editor.active_line_number": color(colors.gutter),
            "editor.gutter.background": color(colors.canvas),
            "syntax": syntax
        }
    }))
}

#[cfg(test)]
#[path = "theme/tests.rs"]
mod tests;
