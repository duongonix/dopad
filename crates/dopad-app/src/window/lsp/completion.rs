use std::rc::Rc;

use gpui_kit::base::input::RopeExt as _;
use gpui_kit::component::input::{CompletionProvider, Rope};
use gpui_kit::{App, Task, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    Position, Range, TextEdit,
};
use unicode_ident::is_xid_continue;

pub(in crate::window) fn local_provider(
    language: dopad_syntax::Language,
) -> Rc<dyn CompletionProvider> {
    Rc::new(LocalCompletionProvider {
        language_id: language_id(language).to_owned(),
    })
}

pub(super) fn prefix_at(text: &Rope, offset: usize) -> String {
    let byte_offset = offset.min(text.len());
    text.chars_at(byte_offset)
        .reversed()
        .take_while(|character| is_xid_continue(*character) || *character == '$')
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

pub(super) fn local_completions(language_id: &str, prefix: &str) -> Vec<CompletionItem> {
    if prefix.is_empty() {
        return Vec::new();
    }
    let words: &[&str] = match language_id {
        "rust" => &[
            "as", "async", "await", "break", "const", "continue", "else", "enum", "fn", "for",
            "if", "if let", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
            "ref", "return", "self", "static", "struct", "trait", "type", "use", "where", "while",
        ],
        "typescript" | "typescriptreact" | "javascript" => &[
            "async",
            "await",
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "else",
            "export",
            "extends",
            "for",
            "from",
            "function",
            "if",
            "import",
            "in",
            "interface",
            "let",
            "new",
            "return",
            "switch",
            "throw",
            "try",
            "type",
            "var",
            "while",
        ],
        "python" => &[
            "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "elif",
            "else", "except", "False", "finally", "for", "from", "if", "import", "in", "is",
            "lambda", "None", "not", "or", "pass", "raise", "return", "self", "True", "try",
            "while", "with", "yield",
        ],
        _ => return Vec::new(),
    };
    let mut ranked = words
        .iter()
        .enumerate()
        .filter_map(|(index, word)| match_rank(prefix, word).map(|rank| (rank, index, *word)))
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(rank, index, _)| (*rank, *index));
    ranked
        .into_iter()
        .take(40)
        .map(|(_, _, word)| CompletionItem {
            label: word.to_owned(),
            kind: Some(CompletionItemKind::KEYWORD),
            filter_text: Some(word.to_owned()),
            ..CompletionItem::default()
        })
        .collect()
}

pub(super) fn normalize_for_editor(
    mut response: CompletionResponse,
    text: &Rope,
    offset: usize,
    prefix: &str,
) -> CompletionResponse {
    let normalize = |items: &mut Vec<CompletionItem>| {
        items.retain_mut(|item| normalize_item(item, text, offset, prefix));
    };
    match &mut response {
        CompletionResponse::Array(items) => normalize(items),
        CompletionResponse::List(list) => normalize(&mut list.items),
    }
    response
}

fn normalize_item(item: &mut CompletionItem, text: &Rope, offset: usize, prefix: &str) -> bool {
    if let Some(edit) = item.text_edit.take() {
        item.text_edit = normalize_text_edit(edit, text);
        item.text_edit.is_some()
    } else {
        let end = offset.min(text.len());
        let start = end.saturating_sub(prefix.len());
        let replacement = item
            .insert_text
            .take()
            .unwrap_or_else(|| item.label.clone());
        item.text_edit = Some(CompletionTextEdit::Edit(TextEdit::new(
            Range::new(text.offset_to_position(start), text.offset_to_position(end)),
            replacement,
        )));
        true
    }
}

fn normalize_text_edit(edit: CompletionTextEdit, text: &Rope) -> Option<CompletionTextEdit> {
    let convert = |position: Position| lsp_position_to_editor(text, position);
    match edit {
        CompletionTextEdit::Edit(edit) => {
            let range = Range::new(convert(edit.range.start)?, convert(edit.range.end)?);
            is_valid_range(&range)
                .then(|| CompletionTextEdit::Edit(TextEdit::new(range, edit.new_text)))
        }
        CompletionTextEdit::InsertAndReplace(edit) => {
            let insert = Range::new(convert(edit.insert.start)?, convert(edit.insert.end)?);
            let replace = Range::new(convert(edit.replace.start)?, convert(edit.replace.end)?);
            (is_valid_range(&insert) && is_valid_range(&replace)).then_some({
                CompletionTextEdit::InsertAndReplace(lsp_types::InsertReplaceEdit {
                    new_text: edit.new_text,
                    insert,
                    replace,
                })
            })
        }
    }
}

fn is_valid_range(range: &Range) -> bool {
    range.start.line < range.end.line
        || (range.start.line == range.end.line && range.start.character <= range.end.character)
}

fn lsp_position_to_editor(text: &Rope, position: Position) -> Option<Position> {
    let row = position.line as usize;
    if row >= text.lines_len() {
        return None;
    }
    let line = text.slice_line(row);
    let utf16_column = (position.character as usize).min(line.len_utf16());
    let byte_offset = text.line_start_offset(row) + line.utf16_to_byte_idx(utf16_column);
    Some(text.offset_to_position(byte_offset))
}

pub(super) fn filter_response(
    mut response: CompletionResponse,
    prefix: &str,
    language_id: &str,
) -> CompletionResponse {
    match &mut response {
        CompletionResponse::Array(items) => {
            *items = rank_items(std::mem::take(items), prefix);
            merge_local(items, language_id, prefix);
        }
        CompletionResponse::List(list) => {
            list.items = rank_items(std::mem::take(&mut list.items), prefix);
            merge_local(&mut list.items, language_id, prefix);
        }
    }
    response
}

fn rank_items(items: Vec<CompletionItem>, prefix: &str) -> Vec<CompletionItem> {
    let mut ranked = items
        .into_iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let candidate = item.filter_text.as_deref().unwrap_or(&item.label);
            match_rank(prefix, candidate).map(|rank| (rank, index, item))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(
        |(left_rank, left_index, left), (right_rank, right_index, right)| {
            left_rank
                .cmp(right_rank)
                .then_with(|| match (&left.sort_text, &right.sort_text) {
                    (Some(left), Some(right)) => left.cmp(right),
                    _ => left_index.cmp(right_index),
                })
        },
    );

    let mut seen = std::collections::HashSet::new();
    ranked
        .into_iter()
        .filter_map(|(_, _, item)| {
            let insertion = item.insert_text.as_deref().unwrap_or(&item.label);
            let key = (item.label.to_lowercase(), insertion.to_owned());
            seen.insert(key).then_some(item)
        })
        .take(100)
        .collect()
}

fn merge_local(items: &mut Vec<CompletionItem>, language_id: &str, prefix: &str) {
    if items.is_empty() {
        *items = local_completions(language_id, prefix);
    }
}

fn match_rank(prefix: &str, candidate: &str) -> Option<u8> {
    if prefix.is_empty() {
        return Some(3);
    }
    if candidate == prefix {
        Some(0)
    } else if candidate.starts_with(prefix) {
        Some(1)
    } else if candidate.to_lowercase().starts_with(&prefix.to_lowercase()) {
        Some(2)
    } else if prefix.chars().count() >= 2 && is_subsequence(prefix, candidate) {
        Some(3)
    } else {
        None
    }
}

fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut expected = needle.chars().flat_map(char::to_lowercase);
    let Some(mut current) = expected.next() else {
        return true;
    };
    for character in haystack.chars().flat_map(char::to_lowercase) {
        if character == current {
            match expected.next() {
                Some(next) => current = next,
                None => return true,
            }
        }
    }
    false
}

fn language_id(language: dopad_syntax::Language) -> &'static str {
    match language {
        dopad_syntax::Language::Rust => "rust",
        dopad_syntax::Language::JavaScript => "javascript",
        dopad_syntax::Language::TypeScript => "typescript",
        dopad_syntax::Language::Tsx => "typescriptreact",
        dopad_syntax::Language::Python => "python",
        _ => "",
    }
}

struct LocalCompletionProvider {
    language_id: String,
}

impl CompletionProvider for LocalCompletionProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _: CompletionContext,
        _: &mut Window,
        _: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        let items = local_completions(&self.language_id, &prefix_at(text, offset));
        Task::ready(Ok(CompletionResponse::Array(items)))
    }

    fn is_completion_trigger(&self, _: usize, new_text: &str, _: &mut App) -> bool {
        new_text.chars().any(|character| {
            is_xid_continue(character) || matches!(character, '$' | '.' | ':' | '>')
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_unicode_prefix_without_scanning_the_document() {
        let text = Rope::from("a long preceding line\nλ变量;");
        assert_eq!(prefix_at(&text, text.len() - 1), "λ变量");
    }

    #[test]
    fn local_rust_keywords_are_filtered_by_the_typed_prefix() {
        let labels = local_completions("rust", "if")
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>();
        assert_eq!(labels, ["if", "if let"]);
    }

    #[test]
    fn lsp_items_are_filtered_ranked_and_deduplicated() {
        let items = ["pub", "if let", "if", "if", "impl"]
            .into_iter()
            .map(|label| CompletionItem {
                label: label.to_owned(),
                ..CompletionItem::default()
            })
            .collect();
        let response = filter_response(CompletionResponse::Array(items), "if", "rust");
        let CompletionResponse::Array(items) = response else {
            panic!("expected an item array");
        };
        assert_eq!(
            items.into_iter().map(|item| item.label).collect::<Vec<_>>(),
            ["if", "if let"]
        );
    }

    #[test]
    fn insertion_text_replaces_prefix_instead_of_appending_after_it() {
        let text = Rope::from("🙂 fo");
        let response = normalize_for_editor(
            CompletionResponse::Array(vec![CompletionItem {
                label: "formatted".to_owned(),
                insert_text: Some("format!".to_owned()),
                ..CompletionItem::default()
            }]),
            &text,
            text.len(),
            "fo",
        );
        let CompletionResponse::Array(items) = response else {
            panic!("expected an item array");
        };
        let Some(CompletionTextEdit::Edit(edit)) = items[0].text_edit.as_ref() else {
            panic!("expected a replacement text edit");
        };
        assert_eq!(
            edit.range,
            Range::new(Position::new(0, 2), Position::new(0, 4))
        );
        assert_eq!(edit.new_text, "format!");
    }

    #[test]
    fn lsp_utf16_text_edits_are_converted_to_editor_character_columns() {
        let text = Rope::from("🙂 foo");
        let item = CompletionItem {
            label: "food".to_owned(),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                Range::new(Position::new(0, 3), Position::new(0, 6)),
                "food".to_owned(),
            ))),
            ..CompletionItem::default()
        };
        let Some(CompletionTextEdit::Edit(edit)) =
            normalize_text_edit(item.text_edit.expect("test edit"), &text)
        else {
            panic!("expected a normalized text edit");
        };
        assert_eq!(
            edit.range,
            Range::new(Position::new(0, 2), Position::new(0, 5))
        );
    }

    #[test]
    fn case_insensitive_prefix_and_fuzzy_matches_are_kept_after_exact_matches() {
        let items = ["IfStatement", "if", "importantFunction"]
            .into_iter()
            .map(|label| CompletionItem {
                label: label.to_owned(),
                ..CompletionItem::default()
            })
            .collect();
        let response = filter_response(CompletionResponse::Array(items), "if", "javascript");
        let CompletionResponse::Array(items) = response else {
            panic!("expected an item array");
        };
        assert_eq!(items[0].label, "if");
        assert_eq!(items[1].label, "IfStatement");
        assert!(items.iter().any(|item| item.label == "importantFunction"));
    }
}
