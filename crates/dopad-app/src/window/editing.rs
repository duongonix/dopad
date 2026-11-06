use std::ops::Range;

use dopad_core::EolKind;
use dopad_syntax::CommentMarkers;

pub(super) struct LineEdit {
    pub range: Range<usize>,
    pub replacement: String,
}

#[derive(Clone, Copy)]
struct Line {
    start: usize,
    content_end: usize,
    end: usize,
}

pub(super) fn duplicate_lines(
    text: &str,
    selection: Range<usize>,
    eol: EolKind,
) -> Option<LineEdit> {
    let lines = lines(text);
    let (first, last) = selected_lines(&lines, selection, text.len())?;
    let range = lines[first].start..lines[last].end;
    let block = &text[range.clone()];
    let separator = if ends_in_line_ending(block) {
        ""
    } else {
        preferred_eol(text, eol)
    };
    Some(LineEdit {
        range,
        replacement: format!("{block}{separator}{block}"),
    })
}

pub(super) fn move_lines(text: &str, selection: Range<usize>, delta: isize) -> Option<LineEdit> {
    let lines = lines(text);
    let (first, last) = selected_lines(&lines, selection, text.len())?;
    let (start, end, order) = match delta {
        -1 if first > 0 => (first - 1, last, moved_up_order(first, last)),
        1 if last + 1 < lines.len() => (first, last + 1, moved_down_order(first, last)),
        _ => return None,
    };

    let range = lines[start].start..lines[end].end;
    let mut replacement = String::with_capacity(range.len());
    for (slot, line_index) in (start..=end).zip(order) {
        let line = lines[line_index];
        let slot_line = lines[slot];
        replacement.push_str(&text[line.start..line.content_end]);
        replacement.push_str(&text[slot_line.content_end..slot_line.end]);
    }

    Some(LineEdit { range, replacement })
}

pub(super) fn toggle_comments(
    text: &str,
    selection: Range<usize>,
    markers: CommentMarkers,
) -> Option<LineEdit> {
    let all_lines = lines(text);
    let (first, last) = selected_lines(&all_lines, selection, text.len())?;
    let chosen = &all_lines[first..=last];
    let has_content = |line: &Line| !text[line.start..line.content_end].trim().is_empty();
    if !chosen.iter().any(has_content) {
        return None;
    }
    let remove = chosen
        .iter()
        .filter(|line| has_content(line))
        .all(|line| is_commented(&text[line.start..line.content_end], markers));
    let mut replacement =
        String::with_capacity(chosen.iter().map(|line| line.end - line.start + 8).sum());
    for line in chosen {
        let content = &text[line.start..line.content_end];
        if content.trim().is_empty() {
            replacement.push_str(content);
        } else if remove {
            replacement.push_str(&uncomment_line(content, markers));
        } else {
            replacement.push_str(&comment_line(content, markers));
        }
        replacement.push_str(&text[line.content_end..line.end]);
    }
    Some(LineEdit {
        range: chosen[0].start..chosen.last()?.end,
        replacement,
    })
}

fn is_commented(line: &str, markers: CommentMarkers) -> bool {
    let trimmed = line.trim_start();
    markers
        .line
        .is_some_and(|prefix| trimmed.starts_with(prefix))
        || markers
            .block
            .is_some_and(|(start, end)| trimmed.starts_with(start) && trimmed.ends_with(end))
}

fn comment_line(line: &str, markers: CommentMarkers) -> String {
    let indent = line.len() - line.trim_start().len();
    let (leading, content) = line.split_at(indent);
    if let Some(prefix) = markers.line {
        format!("{leading}{prefix} {content}")
    } else if let Some((start, end)) = markers.block {
        format!("{leading}{start} {content} {end}")
    } else {
        line.to_owned()
    }
}

fn uncomment_line(line: &str, markers: CommentMarkers) -> String {
    let indent = line.len() - line.trim_start().len();
    let (leading, content) = line.split_at(indent);
    if let Some(prefix) = markers.line.filter(|prefix| content.starts_with(prefix)) {
        let content = content.strip_prefix(prefix).unwrap_or(content);
        format!("{leading}{}", content.strip_prefix(' ').unwrap_or(content))
    } else if let Some((start, end)) = markers
        .block
        .filter(|(start, end)| content.starts_with(start) && content.ends_with(end))
    {
        let content = content.strip_prefix(start).unwrap_or(content);
        let content = content.strip_suffix(end).unwrap_or(content);
        let content = content.strip_prefix(' ').unwrap_or(content);
        format!("{leading}{}", content.strip_suffix(' ').unwrap_or(content))
    } else {
        line.to_owned()
    }
}

fn moved_up_order(first: usize, last: usize) -> Vec<usize> {
    (first..=last).chain(std::iter::once(first - 1)).collect()
}

fn moved_down_order(first: usize, last: usize) -> Vec<usize> {
    std::iter::once(last + 1).chain(first..=last).collect()
}

fn selected_lines(
    lines: &[Line],
    selection: Range<usize>,
    text_len: usize,
) -> Option<(usize, usize)> {
    if lines.is_empty() || selection.start > text_len || selection.end > text_len {
        return None;
    }
    let first = line_at(lines, selection.start);
    let mut last = line_at(lines, selection.end);
    if selection.start != selection.end && last > first && lines[last].start == selection.end {
        last -= 1;
    }
    Some((first, last))
}

fn line_at(lines: &[Line], offset: usize) -> usize {
    lines
        .partition_point(|line| line.start <= offset)
        .saturating_sub(1)
        .min(lines.len().saturating_sub(1))
}

fn lines(text: &str) -> Vec<Line> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\n' || bytes[index] == b'\r' {
            let content_end = index;
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
            index += 1;
            lines.push(Line {
                start,
                content_end,
                end: index,
            });
            start = index;
        } else {
            index += 1;
        }
    }
    if start <= text.len() {
        lines.push(Line {
            start,
            content_end: text.len(),
            end: text.len(),
        });
    }
    lines
}

fn ends_in_line_ending(text: &str) -> bool {
    text.ends_with('\n') || text.ends_with('\r')
}

fn preferred_eol(text: &str, eol: EolKind) -> &'static str {
    match eol {
        EolKind::Lf => "\n",
        EolKind::CrLf => "\r\n",
        EolKind::Cr => "\r",
        EolKind::Mixed | EolKind::None => {
            let bytes = text.as_bytes();
            let mut index = 0;
            while index < bytes.len() {
                match bytes[index] {
                    b'\r' if bytes.get(index + 1) == Some(&b'\n') => return "\r\n",
                    b'\r' => return "\r",
                    b'\n' => return "\n",
                    _ => index += 1,
                }
            }
            "\n"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, edit: LineEdit) -> String {
        let mut result = text.to_owned();
        result.replace_range(edit.range, &edit.replacement);
        result
    }

    #[test]
    fn duplicates_current_or_selected_lines_and_preserves_eol() {
        let edit = duplicate_lines("one\r\ntwo", 6..6, EolKind::CrLf).expect("line edit");
        assert_eq!(apply("one\r\ntwo", edit), "one\r\ntwo\r\ntwo");

        let edit = duplicate_lines("a\nb\nc", 0..4, EolKind::Lf).expect("line edit");
        assert_eq!(apply("a\nb\nc", edit), "a\nb\na\nb\nc");

        let edit = duplicate_lines("雪\n語\n", 0..0, EolKind::Lf).expect("line edit");
        assert_eq!(apply("雪\n語\n", edit), "雪\n雪\n語\n");

        let edit = duplicate_lines("first\r\nlast", 7..7, EolKind::Mixed).expect("line edit");
        assert_eq!(apply("first\r\nlast", edit), "first\r\nlast\r\nlast");
    }

    #[test]
    fn moving_lines_preserves_existing_mixed_endings() {
        let text = "one\r\ntwo\nthree";
        let edit = move_lines(text, 5..8, -1).expect("move up");
        assert_eq!(apply(text, edit), "two\r\none\nthree");

        let text = "one\r\ntwo\nthree";
        let edit = move_lines(text, 0..3, 1).expect("move down");
        assert_eq!(apply(text, edit), "two\r\none\nthree");

        let text = "one\ntwo\nthree\nfour";
        let edit = move_lines(text, 4..14, 1).expect("move selected lines down");
        assert_eq!(apply(text, edit), "one\nfour\ntwo\nthree");
    }

    #[test]
    fn line_move_boundaries_are_noops() {
        assert!(move_lines("one\ntwo", 0..0, -1).is_none());
        assert!(move_lines("one\ntwo", 4..4, 1).is_none());
    }

    #[test]
    fn toggles_line_and_block_comments_preserving_indentation_and_endings() {
        let text = "  let a = 1;\r\n  let b = 2;";
        let rust_markers = CommentMarkers {
            line: Some("//"),
            block: Some(("/*", "*/")),
        };
        let edit =
            toggle_comments(text, 0..text.len(), rust_markers).expect("comment selected lines");
        assert_eq!(apply(text, edit), "  // let a = 1;\r\n  // let b = 2;");
        let commented = "  // let a = 1;\r\n  // let b = 2;";
        let edit = toggle_comments(commented, 0..commented.len(), rust_markers)
            .expect("uncomment selected lines");
        assert_eq!(apply(commented, edit), text);

        let text = "<div>\n  text\n</div>";
        let html_markers = CommentMarkers {
            line: None,
            block: Some(("<!--", "-->")),
        };
        let edit = toggle_comments(text, 0..text.len(), html_markers)
            .expect("block comment selected lines");
        assert_eq!(
            apply(text, edit),
            "<!-- <div> -->\n  <!-- text -->\n<!-- </div> -->"
        );

        let existing_rust_block = "    /* let x = 1; */\n";
        let edit = toggle_comments(
            existing_rust_block,
            0..existing_rust_block.len(),
            rust_markers,
        )
        .expect("remove existing block comment");
        assert_eq!(edit.replacement, "    let x = 1;\n");
    }
}
