#![forbid(unsafe_code)]

mod language;
pub use language::{CommentMarkers, Language, detect_language};

use std::sync::atomic::{AtomicBool, Ordering};

/// Finds the matching delimiter for a cursor-adjacent bracket.
///
/// This scanner is intentionally small and bounded by its caller's background
/// task. It skips quoted text and the common line/block comments for each
/// language so brackets in strings and comments are not treated as syntax.
pub fn matching_bracket(
    text: &str,
    cursor: usize,
    language: Language,
) -> Option<std::ops::Range<usize>> {
    matching_bracket_with_cancel(text, cursor, language, &AtomicBool::new(false))
}

pub fn matching_bracket_with_cancel(
    text: &str,
    cursor: usize,
    language: Language,
    cancelled: &AtomicBool,
) -> Option<std::ops::Range<usize>> {
    let target = bracket_at(text, cursor)?;
    let target_char = text[target..].chars().next()?;
    let (open, _) = bracket_pair(target_char)?;
    let target_is_open = target_char == open;
    let mut stack: Vec<(usize, char)> = Vec::new();
    let mut state = ScanState::Code;
    let mut index = 0;

    while index < text.len() {
        if index.is_multiple_of(4096) && cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let rest = &text[index..];
        match state {
            ScanState::LineComment => {
                let ch = rest.chars().next()?;
                index += ch.len_utf8();
                if ch == '\n' || ch == '\r' {
                    state = ScanState::Code;
                }
            }
            ScanState::BlockComment(end) => {
                if rest.starts_with(end) {
                    index += end.len();
                    state = ScanState::Code;
                } else {
                    index += rest.chars().next()?.len_utf8();
                }
            }
            ScanState::Quoted(quote) => {
                let ch = rest.chars().next()?;
                index += ch.len_utf8();
                if ch == '\\' {
                    if index < text.len() {
                        index += text[index..].chars().next()?.len_utf8();
                    }
                } else if ch == quote {
                    state = ScanState::Code;
                }
            }
            ScanState::Code => {
                if let Some(markers) = language.comment_markers() {
                    if let Some((start, end)) = markers.block
                        && rest.starts_with(start)
                    {
                        state = ScanState::BlockComment(end);
                        index += start.len();
                        continue;
                    }
                    if markers.line.is_some_and(|marker| rest.starts_with(marker)) {
                        state = ScanState::LineComment;
                        index += markers.line?.len();
                        continue;
                    }
                }
                let ch = rest.chars().next()?;
                if ch == '\'' || ch == '"' || ch == '`' {
                    state = ScanState::Quoted(ch);
                    index += ch.len_utf8();
                    continue;
                }
                if let Some((opening, _)) = bracket_pair(ch) {
                    if ch == opening {
                        stack.push((index, ch));
                    } else if let Some(&(start, stacked_opening)) = stack.last()
                        && stacked_opening == opening
                    {
                        stack.pop();
                        if target_is_open && start == target {
                            return Some(start..index + ch.len_utf8());
                        }
                        if !target_is_open && index == target {
                            return Some(start..index + ch.len_utf8());
                        }
                    }
                }
                index += ch.len_utf8();
            }
        }
    }
    None
}
#[derive(Clone, Copy)]
enum ScanState {
    Code,
    LineComment,
    BlockComment(&'static str),
    Quoted(char),
}

fn bracket_at(text: &str, cursor: usize) -> Option<usize> {
    if cursor > text.len() || !text.is_char_boundary(cursor) {
        return None;
    }
    if text[cursor..]
        .chars()
        .next()
        .is_some_and(|ch| bracket_pair(ch).is_some())
    {
        return Some(cursor);
    }
    let (offset, ch) = text[..cursor].char_indices().next_back()?;
    bracket_pair(ch).map(|_| offset)
}

fn bracket_pair(ch: char) -> Option<(char, char)> {
    match ch {
        '(' | ')' => Some(('(', ')')),
        '[' | ']' => Some(('[', ']')),
        '{' | '}' => Some(('{', '}')),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_brackets_ignores_strings_comments_and_nesting() {
        assert_eq!(
            matching_bracket("fn main() { let s = \")\"; }", 9, Language::Rust),
            Some(7..9)
        );
        assert_eq!(
            matching_bracket("fn main() { /* } */ let x = (1); }", 10, Language::Rust),
            Some(10..34)
        );
        assert_eq!(matching_bracket("print(\"(\")", 8, Language::Python), None);
        assert_eq!(
            matching_bracket("x = (1 + [2])", 5, Language::Python),
            Some(4..13)
        );
    }
}
