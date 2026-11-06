use super::{WARM_PAPER, build_warm_paper_highlight_theme};
use crate::syntax::register_highlight_queries;

#[test]
fn warm_paper_tokens_match_the_approved_palette() {
    assert_eq!(WARM_PAPER.canvas, 0xFFF8E7);
    assert_eq!(WARM_PAPER.chrome, 0xF2E8CF);
    assert_eq!(WARM_PAPER.text, 0x38342F);
    assert_eq!(WARM_PAPER.secondary, 0x746F64);
    assert_eq!(WARM_PAPER.tab_active_text, 0x25221F);
    assert_eq!(WARM_PAPER.tab_dirty, 0x38342F);
    assert_eq!(WARM_PAPER.tab_hover, 0xF7EBD9);
    assert_eq!(WARM_PAPER.muted, 0xA9A08D);
    assert_eq!(WARM_PAPER.accent, 0xB65F32);
    assert_eq!(WARM_PAPER.border, 0xD8CEBA);
    assert_eq!(WARM_PAPER.selection, 0xE8D6B7);
    assert_eq!(WARM_PAPER.syntax_keyword, 0xA13D2D);
    assert_eq!(WARM_PAPER.syntax_function, 0x275F80);
    assert_eq!(WARM_PAPER.syntax_type, 0x654C91);
    assert_eq!(WARM_PAPER.syntax_variable, 0x38342F);
    assert_eq!(WARM_PAPER.syntax_parameter, 0x574D40);
    assert_eq!(WARM_PAPER.syntax_property, 0x72512E);
    assert_eq!(WARM_PAPER.syntax_constant, 0x87529A);
    assert_eq!(WARM_PAPER.syntax_string, 0x346A43);
    assert_eq!(WARM_PAPER.syntax_string_escape, 0x346A43);
    assert_eq!(WARM_PAPER.syntax_string_regex, 0x346A43);
    assert_eq!(WARM_PAPER.syntax_boolean, 0xA13D2D);
    assert_eq!(WARM_PAPER.syntax_attribute, 0x9A5427);
    assert_eq!(WARM_PAPER.syntax_tag, 0x654C91);
    assert_eq!(WARM_PAPER.syntax_html_attribute, 0x72512E);
    assert_eq!(WARM_PAPER.syntax_heading, 0xA13D2D);
    assert_eq!(WARM_PAPER.syntax_comment, 0x787064);
    assert_eq!(WARM_PAPER.syntax_number, 0x8651A0);
    assert_eq!(WARM_PAPER.syntax_link, 0x275F80);
    assert_eq!(WARM_PAPER.syntax_invalid, 0xA13D2D);
    assert_eq!(WARM_PAPER.syntax_operator, 0x514A42);
    assert_eq!(WARM_PAPER.syntax_punctuation, 0x655D51);
    assert_eq!(WARM_PAPER.syntax_lifetime, 0xA13D2D);
}

#[test]
fn warm_paper_highlight_theme_is_valid() {
    let theme = build_warm_paper_highlight_theme();
    assert!(theme.is_ok());
}

#[test]
fn warm_paper_syntax_text_colors_meet_wcag_aa_on_editor_background() {
    let text_colors = [
        WARM_PAPER.syntax_keyword,
        WARM_PAPER.syntax_function,
        WARM_PAPER.syntax_type,
        WARM_PAPER.syntax_variable,
        WARM_PAPER.syntax_parameter,
        WARM_PAPER.syntax_property,
        WARM_PAPER.syntax_constant,
        WARM_PAPER.syntax_string,
        WARM_PAPER.syntax_number,
        WARM_PAPER.syntax_boolean,
        WARM_PAPER.syntax_comment,
        WARM_PAPER.syntax_attribute,
        WARM_PAPER.syntax_operator,
        WARM_PAPER.syntax_lifetime,
        WARM_PAPER.syntax_punctuation,
        WARM_PAPER.syntax_invalid,
    ];
    for color in text_colors {
        assert!(
            contrast_ratio(color, WARM_PAPER.canvas) >= 4.5,
            "#{color:06X} is below WCAG AA on #{:06X}",
            WARM_PAPER.canvas
        );
    }
}

#[test]
fn rust_capture_groups_resolve_to_distinct_warm_paper_colors() {
    register_highlight_queries();
    let source = r#"#[derive(Debug)]
// documented type
pub struct User { pub name: String }
impl User {
    pub fn greet<'a>(&'a self, name: String) -> Option<String> {
        let count = 42;
        let ready = true;
        let greeting = format!("hello {}", name);
        Some(greeting).map(|value| value)
    }
}"#;
    let text = gpui_kit::component::input::Rope::from_str(source);
    let mut highlighter = gpui_kit::component::highlighter::SyntaxHighlighter::new("rust");
    assert!(highlighter.update(None, &text, None));
    let theme = build_warm_paper_highlight_theme().expect("valid theme");
    let styles = highlighter.styles(&(0..source.len()), &theme);
    let color_at_offset = |offset: usize| {
        styles
            .iter()
            .find(|(range, _)| range.contains(&offset))
            .and_then(|(_, style)| style.color)
    };
    let color_at = |needle: &str| {
        let offset = source.find(needle).expect("sample token exists");
        color_at_offset(offset)
    };

    assert_eq!(
        color_at("User"),
        Some(gpui::rgb(WARM_PAPER.syntax_type).into())
    );
    assert_eq!(
        color_at("derive"),
        Some(gpui::rgb(WARM_PAPER.syntax_attribute).into())
    );
    assert_eq!(
        color_at("pub"),
        Some(gpui::rgb(WARM_PAPER.syntax_keyword).into())
    );
    assert_eq!(
        color_at("documented type"),
        Some(gpui::rgb(WARM_PAPER.syntax_comment).into())
    );
    assert_eq!(
        color_at("greet"),
        Some(gpui::rgb(WARM_PAPER.syntax_function).into())
    );
    assert_eq!(
        color_at("name: String"),
        Some(gpui::rgb(WARM_PAPER.syntax_property).into())
    );
    let parameter = source
        .rfind("name: String")
        .expect("function parameter exists");
    assert_eq!(
        color_at_offset(parameter),
        Some(gpui::rgb(WARM_PAPER.syntax_parameter).into())
    );
    assert_eq!(
        color_at("42"),
        Some(gpui::rgb(WARM_PAPER.syntax_number).into())
    );
    assert_eq!(
        color_at("true"),
        Some(gpui::rgb(WARM_PAPER.syntax_boolean).into())
    );
    assert_eq!(
        color_at("format!"),
        Some(gpui::rgb(WARM_PAPER.syntax_attribute).into())
    );
    assert_eq!(
        color_at("impl"),
        Some(gpui::rgb(WARM_PAPER.syntax_keyword).into())
    );
    assert_eq!(
        color_at("let"),
        Some(gpui::rgb(WARM_PAPER.syntax_keyword).into())
    );
    assert_eq!(
        color_at("Option"),
        Some(gpui::rgb(WARM_PAPER.syntax_type).into())
    );
    assert_eq!(
        color_at("String"),
        Some(gpui::rgb(WARM_PAPER.syntax_type).into())
    );
    assert_eq!(
        color_at("Some"),
        Some(gpui::rgb(WARM_PAPER.syntax_function).into())
    );
    assert_eq!(
        color_at("self"),
        Some(gpui::rgb(WARM_PAPER.syntax_parameter).into())
    );
    assert_eq!(
        color_at("map"),
        Some(gpui::rgb(WARM_PAPER.syntax_function).into())
    );
    let lifetime = source.find("'a").expect("lifetime exists") + 1;
    assert_eq!(
        color_at_offset(lifetime),
        Some(gpui::rgb(WARM_PAPER.syntax_lifetime).into())
    );
    assert_eq!(
        color_at("count"),
        Some(gpui::rgb(WARM_PAPER.syntax_variable).into())
    );
    assert_eq!(
        color_at("\"hello {}\""),
        Some(gpui::rgb(WARM_PAPER.syntax_string).into())
    );
}

#[test]
fn scss_highlighter_colors_variables_mixins_and_properties() {
    use gpui_kit::component::input::Rope;

    register_highlight_queries();
    let source = "$gap: 1rem;\n@mixin surface($tone) { color: $tone; }";
    let text = Rope::from_str(source);
    let mut highlighter = gpui_kit::component::highlighter::SyntaxHighlighter::new("scss");
    assert!(highlighter.update(None, &text, None));

    let theme = build_warm_paper_highlight_theme().expect("valid theme");
    let styles = highlighter.styles(&(0..source.len()), &theme);
    let color_at = |needle: &str| {
        let offset = source.find(needle).expect("sample token exists");
        styles
            .iter()
            .find(|(range, _)| range.contains(&offset))
            .and_then(|(_, style)| style.color)
    };

    assert_eq!(
        color_at("$gap"),
        Some(gpui::rgb(WARM_PAPER.syntax_variable).into())
    );
    assert_eq!(
        color_at("surface"),
        Some(gpui::rgb(WARM_PAPER.syntax_function).into())
    );
    assert_eq!(
        color_at("color"),
        Some(gpui::rgb(WARM_PAPER.syntax_property).into())
    );
}

#[test]
fn javascript_builtin_literals_use_the_boolean_and_null_color() {
    use gpui_kit::component::input::Rope;

    register_highlight_queries();
    let source = "const enabled = true;\nconst missing = null;";
    let text = Rope::from_str(source);
    let mut highlighter = gpui_kit::component::highlighter::SyntaxHighlighter::new("javascript");
    assert!(highlighter.update(None, &text, None));

    let theme = build_warm_paper_highlight_theme().expect("valid theme");
    let styles = highlighter.styles(&(0..source.len()), &theme);
    for token in ["true", "null"] {
        let offset = source.find(token).expect("sample token exists");
        let color = styles
            .iter()
            .find(|(range, _)| range.contains(&offset))
            .and_then(|(_, style)| style.color);
        assert_eq!(color, Some(gpui::rgb(WARM_PAPER.syntax_boolean).into()));
    }
}

#[test]
fn rust_highlighter_keeps_incremental_edits_and_viewport_styles_in_sync() {
    use gpui_kit::component::input::Rope;
    use tree_sitter::{InputEdit, Point};

    register_highlight_queries();
    let original = "fn main() { let count = 42; }";
    let number = original.find("42").expect("number token exists");
    let mut highlighter = gpui_kit::component::highlighter::SyntaxHighlighter::new("rust");
    let original_rope = Rope::from_str(original);
    assert!(highlighter.update(None, &original_rope, None));

    let updated = original.replacen("42", "43", 1);
    let updated_rope = Rope::from_str(&updated);
    let edit = InputEdit {
        start_byte: number,
        old_end_byte: number + 2,
        new_end_byte: number + 2,
        start_position: Point::new(0, number),
        old_end_position: Point::new(0, number + 2),
        new_end_position: Point::new(0, number + 2),
    };
    assert!(highlighter.update(Some(edit), &updated_rope, None));

    let theme = build_warm_paper_highlight_theme().expect("valid theme");
    let styles = highlighter.styles(&(number..number + 2), &theme);
    assert!(styles.iter().any(|(range, style)| {
        range.contains(&number) && style.color == Some(gpui::rgb(WARM_PAPER.syntax_number).into())
    }));
}

#[test]
fn rust_highlight_ranges_respect_unicode_and_crlf_byte_boundaries() {
    use gpui_kit::component::input::Rope;

    register_highlight_queries();
    let source = "fn café() {\r\n    let crab = \"🦀\";\r\n    let count = 42;\r\n}";
    let text = Rope::from_str(source);
    let mut highlighter = gpui_kit::component::highlighter::SyntaxHighlighter::new("rust");
    assert!(highlighter.update(None, &text, None));

    let theme = build_warm_paper_highlight_theme().expect("valid theme");
    let styles = highlighter.styles(&(0..source.len()), &theme);
    assert!(!styles.is_empty());
    assert!(styles.iter().all(|(range, _)| {
        range.start <= range.end
            && source.is_char_boundary(range.start)
            && source.is_char_boundary(range.end)
    }));
}

fn contrast_ratio(foreground: u32, background: u32) -> f64 {
    fn luminance(color: u32) -> f64 {
        let channel = |shift: u32| {
            let srgb = f64::from((color >> shift) & 0xFF) / 255.0;
            if srgb <= 0.04045 {
                srgb / 12.92
            } else {
                ((srgb + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    let foreground = luminance(foreground);
    let background = luminance(background);
    (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05)
}
