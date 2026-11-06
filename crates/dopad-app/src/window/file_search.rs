use std::{
    collections::HashSet,
    fs::File,
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use dopad_core::{Revision, TextSnapshot};
use dopad_editor::{SearchOptions, compile_matcher, is_whole_word};
use regex::Regex;
use unicode_segmentation::UnicodeSegmentation;

#[cfg(test)]
#[path = "file_search/tests.rs"]
mod tests;

const MAX_RESULTS: usize = 10_000;
const MAX_LINE_BYTES: usize = 1024 * 1024;
const IO_CHUNK_BYTES: usize = 8 * 1024;

#[derive(Clone)]
pub(super) struct OpenBuffer {
    pub path: PathBuf,
    pub display_name: String,
    pub document_id: u64,
    pub revision: Revision,
    pub snapshot: TextSnapshot,
}

#[derive(Clone, Debug)]
pub(super) struct MatchLine {
    pub line: usize,
    /// Zero-based grapheme column, compatible with Dopad editor selections.
    pub column: usize,
    pub match_columns: usize,
    pub snippet: String,
    pub match_range: std::ops::Range<usize>,
}

#[derive(Clone, Debug)]
pub(super) struct FileMatches {
    pub path: PathBuf,
    pub display_name: String,
    pub document_id: Option<u64>,
    pub revision: Option<Revision>,
    pub disk_identity: Option<DiskFileIdentity>,
    pub lines: Vec<MatchLine>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DiskFileIdentity {
    pub length: u64,
    pub modified_unix_nanos: Option<i128>,
}

#[derive(Clone, Debug)]
pub(super) struct SearchFilesResult {
    pub files: Vec<FileMatches>,
    pub match_count: usize,
    pub truncated: bool,
    pub elapsed: std::time::Duration,
    pub skipped_files: usize,
    pub skipped_long_lines: usize,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum SearchFilesError {
    #[error("invalid search expression: {0}")]
    Query(#[from] dopad_editor::SearchError),
    #[error("search cancelled")]
    Cancelled,
    #[error("search root is not accessible: {0}")]
    Root(#[source] io::Error),
    #[error("could not read an open document snapshot: {0}")]
    Snapshot(#[from] dopad_core::CoreError),
}

struct SearchConfig<'a> {
    matcher: &'a Regex,
    options: &'a SearchOptions,
    cancelled: &'a AtomicBool,
}

pub(super) fn search_files(
    roots: Vec<PathBuf>,
    open_buffers: Vec<OpenBuffer>,
    options: SearchOptions,
    cancelled: Arc<AtomicBool>,
    progress: async_channel::Sender<usize>,
) -> Result<SearchFilesResult, SearchFilesError> {
    let started = Instant::now();
    if options.query.is_empty() {
        return Ok(SearchFilesResult {
            files: Vec::new(),
            match_count: 0,
            truncated: false,
            elapsed: started.elapsed(),
            skipped_files: 0,
            skipped_long_lines: 0,
        });
    }
    let matcher = compile_matcher(&options)?;
    let search = SearchConfig {
        matcher: &matcher,
        options: &options,
        cancelled: &cancelled,
    };
    let result_limit = options.max_matches.min(MAX_RESULTS);
    let mut files = Vec::new();
    let mut match_count = 0usize;
    let mut skipped_files = 0usize;
    let mut skipped_long_lines = 0usize;
    let mut truncated = false;
    let mut scanned_files = 0usize;
    let mut visited = HashSet::new();
    let open_paths = open_buffers
        .iter()
        .map(|buffer| normalized(&buffer.path))
        .collect::<HashSet<_>>();

    for buffer in open_buffers {
        check_cancelled(&cancelled)?;
        visited.insert(normalized(&buffer.path));
        scanned_files += 1;
        let _ = progress.try_send(scanned_files);
        let (result, count, cut, skipped_lines) = search_snapshot(
            &buffer.path,
            &buffer.snapshot,
            Some(buffer.document_id),
            Some(buffer.revision),
            &search,
            result_limit.saturating_sub(match_count),
        )?;
        skipped_long_lines += skipped_lines;
        if let Some(mut result) = result {
            result.display_name = buffer.display_name;
            files.push(result);
            match_count += count;
        }
        truncated |= cut;
        if match_count >= result_limit {
            truncated = true;
            break;
        }
    }

    for root in roots {
        check_cancelled(&cancelled)?;
        let metadata = std::fs::metadata(&root).map_err(SearchFilesError::Root)?;
        if metadata.is_file() {
            let key = normalized(&root);
            if !visited.contains(&key) && !open_paths.contains(&key) {
                visited.insert(key);
                scanned_files += 1;
                let _ = progress.try_send(scanned_files);
                match search_path(&root, &search, result_limit.saturating_sub(match_count)) {
                    Ok((Some(result), count, cut, skipped_lines)) => {
                        files.push(result);
                        match_count += count;
                        truncated |= cut;
                        skipped_long_lines += skipped_lines;
                    }
                    Ok((None, _, cut, skipped_lines)) => {
                        truncated |= cut;
                        skipped_long_lines += skipped_lines;
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                        return Err(SearchFilesError::Cancelled);
                    }
                    Err(_) => skipped_files += 1,
                }
            }
            continue;
        }

        let walker = ignore::WalkBuilder::new(&root)
            .hidden(true)
            .git_ignore(true)
            .git_exclude(true)
            .parents(true)
            .filter_entry(|entry| {
                let Some(name) = entry.file_name().to_str() else {
                    return true;
                };
                !matches!(name, ".git" | "target" | "node_modules")
            })
            .build();
        for entry in walker {
            check_cancelled(&cancelled)?;
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    skipped_files += 1;
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Some(file_type) => file_type,
                None => continue,
            };
            if !file_type.is_file() {
                continue;
            }
            let path = entry.into_path();
            let key = normalized(&path);
            if !visited.insert(key.clone()) || open_paths.contains(&key) {
                continue;
            }
            scanned_files += 1;
            let _ = progress.try_send(scanned_files);
            match search_path(&path, &search, result_limit.saturating_sub(match_count)) {
                Ok((Some(result), count, cut, skipped_lines)) => {
                    files.push(result);
                    match_count += count;
                    truncated |= cut;
                    skipped_long_lines += skipped_lines;
                }
                Ok((None, _, cut, skipped_lines)) => {
                    truncated |= cut;
                    skipped_long_lines += skipped_lines;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                    return Err(SearchFilesError::Cancelled);
                }
                Err(_) => skipped_files += 1,
            }
            if match_count >= result_limit {
                truncated = true;
                break;
            }
        }
        if match_count >= result_limit {
            break;
        }
    }

    Ok(SearchFilesResult {
        files,
        match_count,
        truncated,
        elapsed: started.elapsed(),
        skipped_files,
        skipped_long_lines,
    })
}

fn search_path(
    path: &Path,
    search: &SearchConfig<'_>,
    remaining: usize,
) -> io::Result<(Option<FileMatches>, usize, bool, usize)> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    let disk_identity = Some(DiskFileIdentity {
        length: metadata.len(),
        modified_unix_nanos: modified_unix_nanos(metadata.modified().ok()),
    });
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::with_capacity(IO_CHUNK_BYTES);
    let mut files = FileMatches {
        path: path.to_path_buf(),
        display_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Untitled")
            .to_owned(),
        document_id: None,
        revision: None,
        disk_identity,
        lines: Vec::new(),
    };
    let mut line_number = 0usize;
    let mut match_count = 0usize;
    let mut truncated = false;
    let mut skipped_long_lines = 0usize;
    loop {
        if search.cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        buffer.clear();
        let line_len = read_bounded_line(&mut reader, &mut buffer, search.cancelled)?;
        if line_len == 0 {
            break;
        }
        line_number += 1;
        if line_number == 1 && buffer.iter().take(8192).any(|byte| *byte == 0) {
            return Ok((None, 0, false, skipped_long_lines));
        }
        if buffer.len() < line_len {
            skipped_long_lines += 1;
            continue;
        }
        let line = trim_line_ending(&buffer);
        let Ok(text) = std::str::from_utf8(line) else {
            continue;
        };
        let (lines, count, cut) = search_line(
            text,
            line_number,
            search,
            remaining.saturating_sub(match_count),
        );
        if let Some(line) = lines {
            files.lines.push(line);
            match_count += count;
        }
        truncated |= cut;
        if truncated {
            break;
        }
    }
    if files.lines.is_empty() {
        Ok((None, 0, truncated, skipped_long_lines))
    } else {
        Ok((Some(files), match_count, truncated, skipped_long_lines))
    }
}

fn search_snapshot(
    path: &Path,
    snapshot: &TextSnapshot,
    document_id: Option<u64>,
    revision: Option<Revision>,
    search: &SearchConfig<'_>,
    remaining: usize,
) -> Result<(Option<FileMatches>, usize, bool, usize), SearchFilesError> {
    let mut result = FileMatches {
        path: path.to_path_buf(),
        display_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Untitled")
            .to_owned(),
        document_id,
        revision,
        disk_identity: None,
        lines: Vec::new(),
    };
    let mut count = 0usize;
    let mut truncated = false;
    let mut skipped_long_lines = 0usize;
    for line_index in 0..snapshot.line_count() {
        check_cancelled(search.cancelled)?;
        let range = snapshot.line_content_range(line_index)?;
        if range.end.0.saturating_sub(range.start.0) > MAX_LINE_BYTES {
            skipped_long_lines += 1;
            continue;
        }
        let line_text = snapshot.slice(range)?;
        let (found, line_matches, cut) = search_line(
            &line_text,
            line_index + 1,
            search,
            remaining.saturating_sub(count),
        );
        if let Some(found) = found {
            result.lines.push(found);
            count += line_matches;
        }
        truncated |= cut;
        if truncated {
            break;
        }
    }
    if result.lines.is_empty() {
        Ok((None, 0, truncated, skipped_long_lines))
    } else {
        Ok((Some(result), count, truncated, skipped_long_lines))
    }
}

fn search_line(
    text: &str,
    line: usize,
    search: &SearchConfig<'_>,
    remaining: usize,
) -> (Option<MatchLine>, usize, bool) {
    let mut first = None;
    let mut count = 0usize;
    let mut truncated = false;
    for found in search.matcher.find_iter(text) {
        if search.options.whole_word && !is_whole_word(text, found.start(), found.end()) {
            continue;
        }
        if count >= remaining {
            truncated = true;
            break;
        }
        if first.is_none() {
            let match_char = text[..found.start()].chars().count();
            let snippet_start_char = match_char.saturating_sub(100);
            let snippet_start = text
                .char_indices()
                .nth(snippet_start_char)
                .map_or(0, |(offset, _)| offset);
            let snippet_end_char = snippet_start_char.saturating_add(360);
            let snippet_end = text
                .char_indices()
                .nth(snippet_end_char)
                .map_or(text.len(), |(offset, _)| offset);
            let snippet = &text[snippet_start..snippet_end];
            let column = UnicodeSegmentation::graphemes(&text[..found.start()], true).count();
            let match_columns = UnicodeSegmentation::graphemes(found.as_str(), true).count();
            first = Some(MatchLine {
                line,
                column,
                match_columns,
                snippet: snippet.to_owned(),
                match_range: found.start().saturating_sub(snippet_start)
                    ..found.end().min(snippet_end).saturating_sub(snippet_start),
            });
        }
        count += 1;
    }
    (first, count, truncated)
}

fn read_bounded_line(
    reader: &mut impl BufRead,
    output: &mut Vec<u8>,
    cancelled: &AtomicBool,
) -> io::Result<usize> {
    let mut total = 0usize;
    let mut oversized = false;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        let consumed = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        total = total.saturating_add(consumed);
        if !oversized {
            let room = MAX_LINE_BYTES.saturating_sub(output.len());
            output.extend_from_slice(&available[..consumed.min(room)]);
            oversized = consumed > room;
        }
        let complete = available[..consumed].last() == Some(&b'\n');
        reader.consume(consumed);
        if complete {
            break;
        }
    }
    Ok(total)
}

fn trim_line_ending(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn normalized(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn modified_unix_nanos(modified: Option<SystemTime>) -> Option<i128> {
    let modified = modified?;
    match modified.duration_since(UNIX_EPOCH) {
        Ok(duration) => Some(i128::try_from(duration.as_nanos()).unwrap_or(i128::MAX)),
        Err(error) => Some(-i128::try_from(error.duration().as_nanos()).unwrap_or(i128::MAX)),
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), SearchFilesError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(SearchFilesError::Cancelled)
    } else {
        Ok(())
    }
}
