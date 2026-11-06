use std::sync::atomic::{AtomicBool, Ordering};

use dopad_core::{ByteOffset, ByteRange, Revision, TextSnapshot};
use regex::{Regex, RegexBuilder};

const MAX_QUERY_BYTES: usize = 4096;
const MAX_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_MATCHES: usize = 100_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchOptions {
    pub query: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub max_matches: usize,
}

impl SearchOptions {
    pub fn literal(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            regex: false,
            case_sensitive: false,
            whole_word: false,
            max_matches: MAX_MATCHES,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchMatch {
    pub range: ByteRange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResult {
    pub revision: Revision,
    pub matches: Vec<SearchMatch>,
}

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("search query exceeds the 4096-byte limit")]
    QueryTooLong,
    #[error("document exceeds the 64 MiB search limit")]
    DocumentTooLarge,
    #[error("search match limit must be between 1 and 100000")]
    InvalidMatchLimit,
    #[error("search result exceeds the configured match limit")]
    TooManyMatches,
    #[error("invalid regular expression: {0}")]
    InvalidRegex(#[from] regex::Error),
    #[error("search was cancelled")]
    Cancelled,
    #[error("search scope is outside the document or splits a UTF-8 character")]
    InvalidScope,
    #[error("replacement match is invalid or no longer matches the query")]
    InvalidReplacementMatch,
}

pub fn find_matches(
    snapshot: &TextSnapshot,
    options: &SearchOptions,
    cancelled: &AtomicBool,
) -> Result<SearchResult, SearchError> {
    find_matches_in_range(snapshot, options, None, cancelled)
}

/// Finds matches in the full document or inside a UTF-8 byte range.
/// Match ranges returned for a scoped search remain absolute document offsets.
pub fn find_matches_in_range(
    snapshot: &TextSnapshot,
    options: &SearchOptions,
    scope: Option<ByteRange>,
    cancelled: &AtomicBool,
) -> Result<SearchResult, SearchError> {
    if options.query.len() > MAX_QUERY_BYTES {
        return Err(SearchError::QueryTooLong);
    }
    if snapshot.len_bytes() > MAX_DOCUMENT_BYTES {
        return Err(SearchError::DocumentTooLarge);
    }
    if options.max_matches == 0 || options.max_matches > MAX_MATCHES {
        return Err(SearchError::InvalidMatchLimit);
    }
    if options.query.is_empty() {
        return Ok(SearchResult {
            revision: snapshot.revision(),
            matches: Vec::new(),
        });
    }
    let text = snapshot.to_string();
    let scope = scope.unwrap_or_else(|| ByteRange::new(0, text.len()));
    if scope.start.0 > scope.end.0
        || scope.end.0 > text.len()
        || !text.is_char_boundary(scope.start.0)
        || !text.is_char_boundary(scope.end.0)
    {
        return Err(SearchError::InvalidScope);
    }
    let matcher = compile_matcher(options)?;
    let searchable = &text[scope.start.0..scope.end.0];
    let mut matches = Vec::new();
    let mut search_from = 0;

    while search_from <= searchable.len() {
        if cancelled.load(Ordering::Relaxed) {
            return Err(SearchError::Cancelled);
        }
        let Some(found) = matcher.find_at(searchable, search_from) else {
            break;
        };
        let accepted = !options.whole_word || is_whole_word(searchable, found.start(), found.end());
        if accepted {
            if matches.len() == options.max_matches {
                return Err(SearchError::TooManyMatches);
            }
            matches.push(SearchMatch {
                range: ByteRange::new(scope.start.0 + found.start(), scope.start.0 + found.end()),
            });
        }

        if found.start() == found.end() {
            if found.end() == searchable.len() {
                break;
            }
            let Some(ch) = searchable[found.end()..].chars().next() else {
                break;
            };
            search_from = found.end() + ch.len_utf8();
        } else {
            search_from = found.end();
        }
    }

    Ok(SearchResult {
        revision: snapshot.revision(),
        matches,
    })
}

/// Expands one replacement using the same Rust regex capture syntax as Replace All.
pub fn expand_replacement(
    text: &str,
    matched: SearchMatch,
    options: &SearchOptions,
    replacement: &str,
) -> Result<String, SearchError> {
    validate_match_range(text, matched.range)?;
    if !options.regex {
        return Ok(replacement.to_owned());
    }
    let matcher = compile_matcher(options)?;
    expand_with_matcher(text, matched, replacement, &matcher)
}

/// Builds the resulting text for a set of non-overlapping matches.
/// Regex capture expansion is done once per match without recompiling the pattern.
pub fn replace_matches_text(
    text: &str,
    matches: &[SearchMatch],
    options: &SearchOptions,
    replacement: &str,
) -> Result<String, SearchError> {
    let matcher = options
        .regex
        .then(|| compile_matcher(options))
        .transpose()?;
    let mut capacity = text.len();
    for matched in matches {
        validate_match_range(text, matched.range)?;
        capacity = capacity
            .saturating_sub(matched.range.len())
            .saturating_add(replacement.len());
    }

    let mut output = String::with_capacity(capacity);
    let mut cursor = 0;
    for matched in matches {
        let start = matched.range.start.0;
        let end = matched.range.end.0;
        if start < cursor {
            return Err(SearchError::InvalidReplacementMatch);
        }
        output.push_str(&text[cursor..start]);
        if let Some(matcher) = matcher.as_ref() {
            output.push_str(&expand_with_matcher(text, *matched, replacement, matcher)?);
        } else {
            output.push_str(replacement);
        }
        cursor = end;
    }
    output.push_str(&text[cursor..]);
    Ok(output)
}

fn expand_with_matcher(
    text: &str,
    matched: SearchMatch,
    replacement: &str,
    matcher: &Regex,
) -> Result<String, SearchError> {
    let Some(captures) = matcher.captures_at(text, matched.range.start.0) else {
        return Err(SearchError::InvalidReplacementMatch);
    };
    let Some(full_match) = captures.get(0) else {
        return Err(SearchError::InvalidReplacementMatch);
    };
    if full_match.start() != matched.range.start.0 || full_match.end() != matched.range.end.0 {
        return Err(SearchError::InvalidReplacementMatch);
    }
    let mut expanded = String::new();
    captures.expand(replacement, &mut expanded);
    Ok(expanded)
}

fn validate_match_range(text: &str, range: ByteRange) -> Result<(), SearchError> {
    if range.start.0 > range.end.0
        || range.end.0 > text.len()
        || !text.is_char_boundary(range.start.0)
        || !text.is_char_boundary(range.end.0)
    {
        return Err(SearchError::InvalidReplacementMatch);
    }
    Ok(())
}

pub fn compile_matcher(options: &SearchOptions) -> Result<Regex, SearchError> {
    if options.query.len() > MAX_QUERY_BYTES {
        return Err(SearchError::QueryTooLong);
    }
    let pattern = if options.regex {
        options.query.clone()
    } else {
        regex::escape(&options.query)
    };
    Ok(RegexBuilder::new(&pattern)
        .case_insensitive(!options.case_sensitive)
        .size_limit(2 * 1024 * 1024)
        .dfa_size_limit(2 * 1024 * 1024)
        .build()?)
}

pub fn is_whole_word(text: &str, start: usize, end: usize) -> bool {
    let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(is_word) && !after.is_some_and(is_word)
}

impl super::Editor {
    /// Applies non-overlapping search matches as a single undoable transaction.
    pub fn replace_matches(
        &mut self,
        matches: &[SearchMatch],
        replacement: &str,
    ) -> dopad_core::CoreResult<super::EditResult> {
        self.replace_ranges(matches.iter().map(|item| item.range).collect(), replacement)
    }

    fn replace_ranges(
        &mut self,
        ranges: Vec<ByteRange>,
        replacement: &str,
    ) -> dopad_core::CoreResult<super::EditResult> {
        use crate::history::{EditKind, HistoryEntry, StateId};
        use crate::{CursorAffinity, Selection};
        use dopad_core::{CoreError, Edit};

        let snapshot = self.snapshot();
        let mut edits = Vec::with_capacity(ranges.len());
        let mut last_end = None;
        for range in ranges {
            snapshot.validate_grapheme_boundary(range.start)?;
            snapshot.validate_grapheme_boundary(range.end)?;
            if last_end.is_some_and(|end| end > range.start.0) {
                return Err(CoreError::OverlappingEdits);
            }
            last_end = Some(range.end.0);
            edits.push(Edit::new(range, replacement));
        }

        let before_selection = self.selection;
        let before_state = self.current_state;
        let after_state = StateId(self.next_state);
        let next_state = self
            .next_state
            .checked_add(1)
            .ok_or(CoreError::RevisionExhausted)?;
        let outcome = self.buffer.apply_edits(self.revision(), &edits)?;
        if !outcome.changed {
            return Ok(super::EditResult {
                revision: outcome.revision,
                changed: false,
                undoable: false,
            });
        }

        let after_selection = Selection::caret(ByteOffset(
            outcome.inverse.last().map_or(0, |edit| edit.range.end.0),
        ));
        let undoable = self.history.record(HistoryEntry::new(
            edits,
            outcome.inverse,
            before_state,
            after_state,
            before_selection,
            after_selection,
            EditKind::Other,
        ));
        self.current_state = after_state;
        self.next_state = next_state;
        self.selection = Selection {
            anchor: crate::Cursor {
                offset: after_selection.head.offset,
                affinity: CursorAffinity::Downstream,
                preferred_column: None,
            },
            head: crate::Cursor {
                offset: after_selection.head.offset,
                affinity: CursorAffinity::Downstream,
                preferred_column: None,
            },
        };
        Ok(super::EditResult {
            revision: outcome.revision,
            changed: true,
            undoable,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn finds_unicode_case_insensitive_whole_words_using_utf8_byte_ranges() {
        let editor = super::super::Editor::new("Æon æon aéon");
        let options = SearchOptions {
            whole_word: true,
            ..SearchOptions::literal("æON")
        };
        let result = find_matches(&editor.snapshot(), &options, &AtomicBool::new(false)).unwrap();
        assert_eq!(result.matches.len(), 2);
        assert_eq!(result.matches[0].range, ByteRange::new(0, "Æon".len()));
    }

    #[test]
    fn reports_invalid_regex_and_handles_zero_width_without_looping() {
        let editor = super::super::Editor::new("a中");
        let invalid = SearchOptions {
            regex: true,
            ..SearchOptions::literal("[")
        };
        assert!(matches!(
            find_matches(&editor.snapshot(), &invalid, &AtomicBool::new(false)),
            Err(SearchError::InvalidRegex(_))
        ));
        let zero_width = SearchOptions {
            regex: true,
            ..SearchOptions::literal("^|$")
        };
        let result =
            find_matches(&editor.snapshot(), &zero_width, &AtomicBool::new(false)).unwrap();
        assert_eq!(result.matches.len(), 2);
    }

    #[test]
    fn empty_and_cancelled_searches_are_explicit() {
        let editor = super::super::Editor::new("word");
        assert!(
            find_matches(
                &editor.snapshot(),
                &SearchOptions::literal(""),
                &AtomicBool::new(false)
            )
            .unwrap()
            .matches
            .is_empty()
        );
        assert!(matches!(
            find_matches(
                &editor.snapshot(),
                &SearchOptions::literal("word"),
                &AtomicBool::new(true)
            ),
            Err(SearchError::Cancelled)
        ));
    }

    #[test]
    fn replacement_batch_is_one_undo_step() {
        let mut editor = super::super::Editor::new("foo foo");
        let result = find_matches(
            &editor.snapshot(),
            &SearchOptions::literal("foo"),
            &AtomicBool::new(false),
        )
        .unwrap();
        editor.replace_matches(&result.matches, "bar").unwrap();
        assert_eq!(editor.snapshot().to_string(), "bar bar");
        assert!(editor.undo().unwrap());
        assert_eq!(editor.snapshot().to_string(), "foo foo");
    }

    #[test]
    fn scoped_search_returns_absolute_ranges_and_respects_utf8_boundaries() {
        let editor = super::super::Editor::new("前 foo foo 後");
        let start = "前 ".len();
        let end = start + "foo".len();
        let result = find_matches_in_range(
            &editor.snapshot(),
            &SearchOptions::literal("foo"),
            Some(ByteRange::new(start, end)),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].range, ByteRange::new(start, end));

        assert!(matches!(
            find_matches_in_range(
                &editor.snapshot(),
                &SearchOptions::literal("foo"),
                Some(ByteRange::new(1, end)),
                &AtomicBool::new(false),
            ),
            Err(SearchError::InvalidScope)
        ));
    }

    #[test]
    fn regex_replacement_expands_captures_and_escaped_dollars() {
        let source = "name=John, city=Taipei";
        let options = SearchOptions {
            regex: true,
            ..SearchOptions::literal(r"(\w+)=(\w+)")
        };
        let editor = super::super::Editor::new(source);
        let matches = find_matches(&editor.snapshot(), &options, &AtomicBool::new(false))
            .unwrap()
            .matches;
        assert_eq!(
            replace_matches_text(source, &matches, &options, "$2=$1").unwrap(),
            "John=name, Taipei=city"
        );

        let one = expand_replacement(source, matches[0], &options, "$$$2").unwrap();
        assert_eq!(one, "$John");
    }
}
