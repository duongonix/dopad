use std::{
    fs,
    io::{BufReader, Cursor},
    sync::{Arc, atomic::AtomicBool},
};

use dopad_core::TextBuffer;
use dopad_editor::SearchOptions;
use tempfile::tempdir;

use super::{MAX_LINE_BYTES, OpenBuffer, SearchFilesError, read_bounded_line, search_files};

fn options(query: &str) -> SearchOptions {
    SearchOptions {
        query: query.to_owned(),
        regex: false,
        case_sensitive: true,
        whole_word: false,
        max_matches: 100,
    }
}

fn search(
    roots: Vec<std::path::PathBuf>,
    open_buffers: Vec<OpenBuffer>,
    options: SearchOptions,
) -> Result<super::SearchFilesResult, SearchFilesError> {
    let (_progress, receiver) = async_channel::bounded(1);
    drop(receiver);
    search_files(
        roots,
        open_buffers,
        options,
        Arc::new(AtomicBool::new(false)),
        _progress,
    )
}

#[test]
fn open_snapshot_results_use_grapheme_columns_and_keep_buffer_content() {
    let workspace = tempdir().expect("temporary workspace");
    let path = workspace.path().join("unicode.txt");
    fs::write(&path, "stale disk content").expect("write disk file");
    let snapshot = TextBuffer::new("first\r\ne\u{301} foo").snapshot();
    let buffer = OpenBuffer {
        path: path.clone(),
        display_name: "unicode.txt".into(),
        document_id: 42,
        revision: snapshot.revision(),
        snapshot,
    };

    let result = search(
        vec![workspace.path().to_path_buf()],
        vec![buffer],
        options("foo"),
    )
    .expect("search should succeed");

    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].document_id, Some(42));
    let line = &result.files[0].lines[0];
    assert_eq!(line.line, 2);
    assert_eq!(line.column, 2);
    assert_eq!(line.match_columns, 3);
}

#[test]
fn folder_search_respects_hidden_gitignore_and_excluded_directories() {
    let workspace = tempdir().expect("temporary workspace");
    fs::write(
        workspace.path().join(".gitignore"),
        "ignored.txt\ntarget/\nnode_modules/\n",
    )
    .expect("write ignore rules");
    fs::write(workspace.path().join("visible.txt"), "needle\n").expect("visible file");
    fs::write(workspace.path().join("ignored.txt"), "needle\n").expect("ignored file");
    fs::write(workspace.path().join(".hidden.txt"), "needle\n").expect("hidden file");
    for directory in [".git", "target", "node_modules"] {
        let path = workspace.path().join(directory);
        fs::create_dir_all(&path).expect("create excluded directory");
        fs::write(path.join("nested.txt"), "needle\n").expect("write excluded file");
    }

    let result = search(
        vec![workspace.path().to_path_buf()],
        Vec::new(),
        options("needle"),
    )
    .expect("folder search should succeed");
    let names = result
        .files
        .iter()
        .map(|file| file.display_name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(names, ["visible.txt"]);
    assert_eq!(result.match_count, 1);
}

#[test]
fn invalid_regex_is_reported_without_scanning() {
    let mut regex_options = options("(");
    regex_options.regex = true;

    let result = search(Vec::new(), Vec::new(), regex_options);

    assert!(matches!(result, Err(SearchFilesError::Query(_))));
}

#[test]
fn oversized_lines_are_bounded_reported_and_do_not_hide_later_matches() {
    let workspace = tempdir().expect("temporary workspace");
    let path = workspace.path().join("long-line.txt");
    let mut text = vec![b'x'; MAX_LINE_BYTES + 1];
    text.extend_from_slice(b"\nneedle\n");
    fs::write(&path, text).expect("write bounded-line fixture");

    let result = search(vec![path], Vec::new(), options("needle"))
        .expect("search should skip oversized line and continue");

    assert_eq!(result.skipped_long_lines, 1);
    assert_eq!(result.match_count, 1);
    assert_eq!(result.files[0].lines[0].line, 2);
}

#[test]
fn bounded_line_reader_honors_cancellation_while_consuming_a_line() {
    let cancelled = AtomicBool::new(true);
    let mut reader = BufReader::new(Cursor::new(b"large pending line"));
    let mut output = Vec::new();

    let error = read_bounded_line(&mut reader, &mut output, &cancelled)
        .expect_err("cancellation should interrupt line reading");

    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
}
