use alacritty_terminal::{
    event::{Event, EventListener},
    grid::Dimensions,
    index::{Column, Line},
    term::{Config, Term, cell::Flags},
    vte::ansi::{Color, CursorShape, NamedColor, Processor, Rgb},
};
use std::sync::mpsc;

const SCROLLBACK_LINES: usize = 5_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalColor(pub u8, pub u8, pub u8);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalCell {
    pub character: char,
    pub foreground: TerminalColor,
    pub background: Option<TerminalColor>,
    pub bold: bool,
    pub hyperlink: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalLine(pub Vec<TerminalCell>);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalSnapshot {
    pub lines: Vec<TerminalLine>,
    pub cursor: Option<(usize, usize)>,
    pub exit_status: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Copy)]
pub struct TerminalSize {
    pub columns: usize,
    pub rows: usize,
}

pub(crate) struct Emulator {
    term: Term<ResponseListener>,
    parser: Processor,
}

struct ResponseListener(mpsc::Sender<Vec<u8>>);

impl EventListener for ResponseListener {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            let _ = self.0.send(text.into_bytes());
        }
    }
}

impl Emulator {
    pub(crate) fn new(size: TerminalSize, responses: mpsc::Sender<Vec<u8>>) -> Self {
        let dimensions = size.clamped();
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &dimensions, ResponseListener(responses)),
            parser: Processor::new(),
        }
    }

    pub(crate) fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    pub(crate) fn resize(&mut self, size: TerminalSize) {
        self.term.resize(size.clamped());
    }

    pub(crate) fn snapshot(&self) -> TerminalSnapshot {
        let grid = self.term.grid();
        let columns = grid.columns();
        let total_lines = grid.total_lines();
        let history = total_lines
            .saturating_sub(grid.screen_lines())
            .min(SCROLLBACK_LINES);
        let first_line = -(history as i32);
        let lines = (0..total_lines.min(SCROLLBACK_LINES + grid.screen_lines()))
            .map(|row| {
                let line = Line(first_line + row as i32);
                let cells = (0..columns)
                    .map(|column| {
                        let cell = &grid[line][Column(column)];
                        TerminalCell {
                            character: if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                                '\0'
                            } else {
                                cell.c
                            },
                            foreground: color(cell.fg, self.term.colors()),
                            background: match cell.bg {
                                Color::Named(NamedColor::Background) => None,
                                value => Some(color(value, self.term.colors())),
                            },
                            bold: cell.flags.contains(Flags::BOLD),
                            hyperlink: cell.hyperlink().map(|hyperlink| hyperlink.uri().to_owned()),
                        }
                    })
                    .collect();
                TerminalLine(cells)
            })
            .collect();
        let content = self.term.renderable_content();
        let cursor = if grid.display_offset() == 0 && content.cursor.shape != CursorShape::Hidden {
            let point = content.cursor.point;
            usize::try_from(point.line.0)
                .ok()
                .map(|row| (history.saturating_add(row), point.column.0))
        } else {
            None
        };
        TerminalSnapshot {
            lines,
            cursor,
            exit_status: None,
            error: None,
        }
    }
}

fn color(color: Color, colors: &alacritty_terminal::term::color::Colors) -> TerminalColor {
    match color {
        Color::Spec(Rgb { r, g, b }) => TerminalColor(r, g, b),
        Color::Named(named) => colors[named]
            .map(|rgb| TerminalColor(rgb.r, rgb.g, rgb.b))
            .unwrap_or_else(|| named_color(named)),
        Color::Indexed(index) => indexed_color(index),
    }
}

fn named_color(color: NamedColor) -> TerminalColor {
    use NamedColor::*;
    match color {
        Black => TerminalColor(56, 52, 47),
        Red | BrightRed => TerminalColor(161, 61, 45),
        Green | BrightGreen => TerminalColor(52, 106, 67),
        Yellow | BrightYellow => TerminalColor(154, 84, 39),
        Blue | BrightBlue => TerminalColor(39, 95, 128),
        Magenta | BrightMagenta => TerminalColor(101, 76, 145),
        Cyan | BrightCyan => TerminalColor(39, 95, 128),
        White | BrightWhite => TerminalColor(37, 34, 31),
        BrightBlack => TerminalColor(128, 121, 108),
        Foreground | BrightForeground => TerminalColor(37, 34, 31),
        Background => TerminalColor(255, 248, 231),
        Cursor => TerminalColor(56, 52, 47),
        DimBlack => TerminalColor(128, 121, 108),
        DimRed => TerminalColor(161, 61, 45),
        DimGreen => TerminalColor(52, 106, 67),
        DimYellow => TerminalColor(154, 84, 39),
        DimBlue => TerminalColor(39, 95, 128),
        DimMagenta => TerminalColor(101, 76, 145),
        DimCyan => TerminalColor(39, 95, 128),
        DimWhite | DimForeground => TerminalColor(116, 111, 100),
    }
}

fn indexed_color(index: u8) -> TerminalColor {
    const ANSI: [TerminalColor; 16] = [
        TerminalColor(56, 52, 47),
        TerminalColor(161, 61, 45),
        TerminalColor(52, 106, 67),
        TerminalColor(154, 84, 39),
        TerminalColor(39, 95, 128),
        TerminalColor(101, 76, 145),
        TerminalColor(39, 95, 128),
        TerminalColor(232, 224, 205),
        TerminalColor(128, 121, 108),
        TerminalColor(161, 61, 45),
        TerminalColor(52, 106, 67),
        TerminalColor(154, 84, 39),
        TerminalColor(39, 95, 128),
        TerminalColor(101, 76, 145),
        TerminalColor(39, 95, 128),
        TerminalColor(37, 34, 31),
    ];
    match index {
        0..=15 => ANSI[usize::from(index)],
        16..=231 => {
            let n = usize::from(index - 16);
            let channel = |value: usize| {
                if value == 0 {
                    0
                } else {
                    (55 + value * 40) as u8
                }
            };
            TerminalColor(channel(n / 36), channel((n / 6) % 6), channel(n % 6))
        }
        232..=255 => {
            let value = 8 + (index - 232) * 10;
            TerminalColor(value, value, value)
        }
    }
}

impl TerminalSize {
    fn clamped(self) -> Sized {
        Sized {
            columns: self.columns.clamp(2, 500),
            rows: self.rows.clamp(1, 300),
        }
    }
}

struct Sized {
    columns: usize,
    rows: usize,
}
impl Dimensions for Sized {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_ansi_colors_into_cells() {
        let (responses, _receiver) = mpsc::channel();
        let mut terminal = Emulator::new(
            TerminalSize {
                columns: 24,
                rows: 2,
            },
            responses,
        );
        terminal.advance(b"plain \x1b[31mred\x1b[0m");
        let snapshot = terminal.snapshot();
        let text: String = snapshot.lines[0]
            .0
            .iter()
            .map(|cell| cell.character)
            .collect();
        assert!(text.starts_with("plain red"));
        assert_eq!(
            snapshot.lines[0].0[6].foreground,
            TerminalColor(161, 61, 45)
        );
    }

    #[test]
    fn preserves_indexed_and_truecolor_ansi_values() {
        let (responses, _receiver) = mpsc::channel();
        let mut terminal = Emulator::new(
            TerminalSize {
                columns: 24,
                rows: 2,
            },
            responses,
        );
        terminal.advance(b"\x1b[38;5;196mI\x1b[38;2;17;34;51mT");
        let cells = &terminal.snapshot().lines[0].0;
        assert_eq!(cells[0].foreground, TerminalColor(255, 0, 0));
        assert_eq!(cells[1].foreground, TerminalColor(17, 34, 51));
    }

    #[test]
    fn replies_to_device_status_queries_for_interactive_shells() {
        let (responses, receiver) = mpsc::channel();
        let mut terminal = Emulator::new(
            TerminalSize {
                columns: 24,
                rows: 2,
            },
            responses,
        );
        terminal.advance(b"\x1b[6n");
        assert_eq!(receiver.try_recv().ok(), Some(b"\x1b[1;1R".to_vec()));
    }

    #[test]
    fn retains_osc8_hyperlinks_in_the_terminal_grid() {
        let (responses, _receiver) = mpsc::channel();
        let mut terminal = Emulator::new(
            TerminalSize {
                columns: 24,
                rows: 2,
            },
            responses,
        );
        terminal.advance(b"\x1b]8;;https://example.test/path\x1b\\link\x1b]8;;\x1b\\");
        let cells = &terminal.snapshot().lines[0].0;
        assert_eq!(
            cells[0].hyperlink.as_deref(),
            Some("https://example.test/path")
        );
        assert_eq!(
            cells[3].hyperlink.as_deref(),
            Some("https://example.test/path")
        );
        assert_eq!(cells[4].hyperlink, None);
    }

    #[test]
    fn scrollback_is_bounded() {
        let (responses, _receiver) = mpsc::channel();
        let mut terminal = Emulator::new(
            TerminalSize {
                columns: 20,
                rows: 2,
            },
            responses,
        );
        for _ in 0..5_100 {
            terminal.advance(b"row\r\n");
        }
        assert!(terminal.snapshot().lines.len() <= SCROLLBACK_LINES + 2);
    }
}
