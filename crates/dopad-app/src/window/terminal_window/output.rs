use std::sync::Arc;

use dopad_terminal::{TerminalColor, TerminalSnapshot};
use gpui_kit::{
    Context, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement, StatefulInteractiveElement, Styled, UniformListScrollHandle,
    div, px, rgb, uniform_list,
};

use super::{TERMINAL_PADDING_X, TERMINAL_PADDING_Y, TerminalWindow};
use crate::settings::AppSettings;
use crate::theme;

struct OutputRun {
    foreground: u32,
    background: Option<u32>,
    bold: bool,
    hyperlink: Option<String>,
    text: String,
}

struct LineRenderContext {
    snapshot: Arc<TerminalSnapshot>,
    font: String,
    font_size: f32,
    font_weight: u16,
    line_height: f32,
    default_color: u32,
    selection: Option<((usize, usize), (usize, usize))>,
    selection_color: u32,
    cell_width: f32,
    cell_width_scale: u8,
}

pub(super) fn render_output(
    snapshot: Option<Arc<TerminalSnapshot>>,
    scroll: UniformListScrollHandle,
    settings: &AppSettings,
    selection: Option<((usize, usize), (usize, usize))>,
    palette: &theme::Palette,
    cell_width: f32,
    cx: &mut Context<TerminalWindow>,
) -> impl IntoElement + use<> {
    let snapshot = snapshot.unwrap_or_else(|| Arc::new(TerminalSnapshot::default()));
    let count = snapshot.lines.len();
    let text_color = palette.tab_active_text;
    let selection_color = palette.selection;
    let line_context = LineRenderContext {
        snapshot: snapshot.clone(),
        font: settings.terminal_font_family.clone(),
        font_size: f32::from(settings.terminal_font_size),
        font_weight: settings.terminal_font_weight,
        line_height: super::terminal_line_height(
            settings.terminal_font_size,
            settings.terminal_line_height,
        ),
        default_color: text_color,
        selection,
        selection_color,
        cell_width,
        cell_width_scale: settings.terminal_cell_width,
    };
    uniform_list(
        "terminal-output",
        count,
        cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
            range
                .map(|index| render_line(&line_context, index, cx))
                .collect::<Vec<_>>()
        }),
    )
    .h_full()
    .w_full()
    .px(px(TERMINAL_PADDING_X))
    .py(px(TERMINAL_PADDING_Y))
    .track_scroll(&scroll)
    .on_scroll(cx.listener(|this, _, _, cx| {
        this.auto_follow = this.output_scroll.is_scrolled_to_end().unwrap_or(true);
        cx.notify();
    }))
    .into_any_element()
}

fn render_line(
    context: &LineRenderContext,
    index: usize,
    cx: &mut Context<TerminalWindow>,
) -> gpui_kit::AnyElement {
    let LineRenderContext {
        snapshot,
        font,
        font_size,
        font_weight,
        line_height,
        default_color,
        selection,
        selection_color,
        cell_width,
        cell_width_scale,
    } = context;
    let default_color = *default_color;
    let selection = *selection;
    let selection_color = *selection_color;
    let cell_width = *cell_width;
    let font_size = *font_size;
    let font_weight = *font_weight;
    let line_height = *line_height;
    let cell_width_scale = *cell_width_scale;
    let Some(line) = snapshot.lines.get(index) else {
        return div().h(px(line_height)).into_any_element();
    };
    if cell_width_scale != 100 {
        return render_custom_cells(context, line, index, cx);
    }
    let mut runs = Vec::<OutputRun>::new();
    for (column, cell) in line.0.iter().enumerate() {
        let cursor = snapshot.cursor == Some((index, column));
        let foreground = if cursor {
            0xFFF8E7
        } else {
            color_hex(cell.foreground)
        };
        let selected = selection.is_some_and(|range| selection_contains(range, index, column));
        let background = if selected {
            Some(selection_color)
        } else if cursor {
            Some(0x38342F)
        } else {
            cell.background.map(color_hex)
        };
        let hyperlink = cell.hyperlink.as_deref().and_then(safe_terminal_url);
        let character = (!cell.character.is_control()).then_some(cell.character);
        if let Some(last) = runs.last_mut()
            && last.foreground == foreground
            && last.background == background
            && last.bold == cell.bold
            && last.hyperlink == hyperlink
        {
            if let Some(character) = character {
                last.text.push(character);
            }
        } else {
            runs.push(OutputRun {
                foreground,
                background,
                bold: cell.bold,
                hyperlink,
                text: character.map_or_else(String::new, |character| character.to_string()),
            });
        }
    }
    let row = div()
        .h(px(line_height))
        .w_full()
        .flex()
        .items_center()
        .overflow_hidden()
        .children(runs.into_iter().enumerate().map(|(run_index, output_run)| {
            let mut run = div()
                .id(format!("terminal-run-{index}-{run_index}"))
                .flex_shrink_0()
                .text_size(px(font_size))
                .font_family(font.clone())
                .font_weight(render_font_weight(font_weight, output_run.bold))
                .text_color(rgb(if output_run.foreground == 0 {
                    default_color
                } else {
                    output_run.foreground
                }));
            if let Some(background) = output_run.background {
                run = run.bg(rgb(background));
            }
            if let Some(url) = output_run.hyperlink {
                run = run
                    .underline()
                    .on_click(move |_, _, app| app.open_url(&url));
            }
            run.child(output_run.text).into_any_element()
        }));
    row.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
            let column = ((f32::from(event.position.x - window.bounds().origin.x)
                - TERMINAL_PADDING_X)
                / cell_width.max(1.))
            .floor()
            .max(0.) as usize;
            this.selection = Some(((index, column), (index, column)));
            this.selecting = true;
            cx.notify();
        }),
    )
    .on_mouse_move(
        cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
            if this.selecting {
                let column = ((f32::from(event.position.x - window.bounds().origin.x)
                    - TERMINAL_PADDING_X)
                    / cell_width.max(1.))
                .floor()
                .max(0.) as usize;
                if let Some((start, _)) = this.selection {
                    this.selection = Some((start, (index, column)));
                    cx.notify();
                }
            }
        }),
    )
    .into_any_element()
}

fn render_custom_cells(
    context: &LineRenderContext,
    line: &dopad_terminal::TerminalLine,
    index: usize,
    cx: &mut Context<TerminalWindow>,
) -> gpui_kit::AnyElement {
    let snapshot = context.snapshot.as_ref();
    let font = context.font.as_str();
    let font_size = context.font_size;
    let font_weight = context.font_weight;
    let line_height = context.line_height;
    let default_color = context.default_color;
    let selection = context.selection;
    let selection_color = context.selection_color;
    let cell_width = context.cell_width;
    let mut cells = Vec::with_capacity(line.0.len());
    let mut column = 0;
    while let Some(cell) = line.0.get(column) {
        let wide = line
            .0
            .get(column + 1)
            .is_some_and(|next| next.character == '\0');
        if cell.character == '\0' {
            column += 1;
            continue;
        }

        let cursor = snapshot.cursor.is_some_and(|(row, col)| {
            row == index && (col == column || (wide && col == column + 1))
        });
        let selected = selection.is_some_and(|range| {
            selection_contains(range, index, column)
                || (wide && selection_contains(range, index, column + 1))
        });
        let foreground = if cursor {
            0xFFF8E7
        } else {
            color_hex(cell.foreground)
        };
        let background = if selected {
            Some(selection_color)
        } else if cursor {
            Some(0x38342F)
        } else {
            cell.background.map(color_hex)
        };
        let mut glyph = div()
            .id(format!("terminal-cell-{index}-{column}"))
            .w(px(cell_width * if wide { 2. } else { 1. }))
            .h(px(line_height))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(font_size))
            .font_family(font)
            .font_weight(render_font_weight(font_weight, cell.bold))
            .text_color(rgb(if foreground == 0 {
                default_color
            } else {
                foreground
            }));
        if let Some(background) = background {
            glyph = glyph.bg(rgb(background));
        }
        if !cell.character.is_control() {
            glyph = glyph.child(cell.character.to_string());
        }
        if let Some(url) = cell.hyperlink.as_deref().and_then(safe_terminal_url) {
            glyph = glyph
                .underline()
                .on_click(move |_, _, app| app.open_url(&url));
        }
        cells.push(glyph.into_any_element());
        column += if wide { 2 } else { 1 };
    }

    let row = div()
        .h(px(line_height))
        .w_full()
        .flex()
        .items_center()
        .overflow_hidden()
        .children(cells);
    row.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
            let column = ((f32::from(event.position.x - window.bounds().origin.x)
                - TERMINAL_PADDING_X)
                / cell_width.max(1.))
            .floor()
            .max(0.) as usize;
            this.selection = Some(((index, column), (index, column)));
            this.selecting = true;
            cx.notify();
        }),
    )
    .on_mouse_move(
        cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
            if this.selecting {
                let column = ((f32::from(event.position.x - window.bounds().origin.x)
                    - TERMINAL_PADDING_X)
                    / cell_width.max(1.))
                .floor()
                .max(0.) as usize;
                if let Some((start, _)) = this.selection {
                    this.selection = Some((start, (index, column)));
                    cx.notify();
                }
            }
        }),
    )
    .into_any_element()
}

fn render_font_weight(weight: u16, bold: bool) -> FontWeight {
    if bold && weight < 700 {
        return FontWeight::BOLD;
    }
    match weight {
        500 => FontWeight::MEDIUM,
        600 => FontWeight::SEMIBOLD,
        700 => FontWeight::BOLD,
        800 => FontWeight::EXTRA_BOLD,
        _ => FontWeight::NORMAL,
    }
}

fn selection_contains(range: ((usize, usize), (usize, usize)), line: usize, column: usize) -> bool {
    let (start, end) = if range.0 <= range.1 {
        range
    } else {
        (range.1, range.0)
    };
    (line, column) >= start && (line, column) <= end
}

pub(super) fn selection_text(
    snapshot: &TerminalSnapshot,
    range: Option<((usize, usize), (usize, usize))>,
) -> Option<String> {
    let ((start_line, start_column), (end_line, end_column)) = range?;
    let (start, end) = if (start_line, start_column) <= (end_line, end_column) {
        ((start_line, start_column), (end_line, end_column))
    } else {
        ((end_line, end_column), (start_line, start_column))
    };
    let mut lines = Vec::new();
    for line_index in start.0..=end.0 {
        let line = snapshot.lines.get(line_index)?;
        let from = if line_index == start.0 { start.1 } else { 0 };
        let to = if line_index == end.0 {
            end.1.saturating_add(1)
        } else {
            line.0.len()
        };
        lines.push(
            line.0
                .get(from..to.min(line.0.len()))?
                .iter()
                .filter(|cell| !cell.character.is_control())
                .map(|cell| cell.character)
                .collect::<String>()
                .trim_end()
                .to_owned(),
        );
    }
    Some(lines.join("\n"))
}

fn color_hex(color: TerminalColor) -> u32 {
    (u32::from(color.0) << 16) | (u32::from(color.1) << 8) | u32::from(color.2)
}

fn safe_terminal_url(uri: &str) -> Option<String> {
    if uri.is_empty() || uri.trim() != uri || uri.chars().any(char::is_control) {
        return None;
    }
    let (scheme, target) = uri.split_once(':')?;
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => {
            let authority = target.strip_prefix("//")?.split(['/', '?', '#']).next()?;
            if authority.is_empty()
                || authority.contains('@')
                || authority.chars().any(char::is_whitespace)
            {
                return None;
            }
        }
        "mailto" => {
            if target.is_empty()
                || target.starts_with("//")
                || target.chars().any(char::is_whitespace)
            {
                return None;
            }
        }
        _ => return None,
    }
    Some(uri.to_owned())
}

#[cfg(test)]
mod tests {
    use super::safe_terminal_url;

    #[test]
    fn terminal_links_allow_only_safe_external_schemes() {
        assert_eq!(
            safe_terminal_url("https://example.test/path?q=1"),
            Some("https://example.test/path?q=1".into())
        );
        assert_eq!(
            safe_terminal_url("mailto:user@example.test"),
            Some("mailto:user@example.test".into())
        );
        for unsafe_uri in [
            "file:///C:/Users/user/secret.txt",
            "javascript:alert(1)",
            "https://user:pass@example.test",
            "https://example.test/with\nnewline",
        ] {
            assert_eq!(safe_terminal_url(unsafe_uri), None, "{unsafe_uri:?}");
        }
    }
}
