use std::{
    env,
    error::Error,
    fs,
    hint::black_box,
    path::PathBuf,
    time::{Duration, Instant},
};

use dopad_core::ByteOffset;
use dopad_editor::{Editor, SearchOptions, find_matches};

const FIXTURES: [&str; 3] = ["text-1mb.txt", "text-10mb.txt", "text-100mb.txt"];
const SEARCH_MARKER: &str = "DOPAD_BENCH_END_MARKER";

fn main() -> Result<(), Box<dyn Error>> {
    let fixture_dir = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/bench-fixtures"));
    let mut rows = vec!["scenario,size_bytes,samples,median_ns,p95_ns".to_owned()];

    for fixture in FIXTURES {
        let path = fixture_dir.join(fixture);
        let text = fs::read_to_string(&path).map_err(|error| {
            format!(
                "could not read {}: {error}; run scripts/bench/windows.ps1 first",
                path.display()
            )
        })?;
        let size = text.len();
        let construction_samples = if size >= 100 * 1024 * 1024 { 3 } else { 5 };
        rows.push(measure_editor_construction(
            &text,
            size,
            construction_samples,
        ));
        rows.push(measure_edit_undo(&text, size, 25)?);
        if size <= 64 * 1024 * 1024 {
            rows.push(measure_search(&text, size, 5)?);
        }
    }

    let report = rows.join("\n") + "\n";
    print!("{report}");
    if let Some(path) = env::var_os("DOPAD_BENCH_OUTPUT") {
        fs::write(path, report)?;
    }
    Ok(())
}

fn measure_editor_construction(text: &str, size: usize, samples: usize) -> String {
    let mut timings = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        black_box(Editor::new(black_box(text)));
        timings.push(start.elapsed());
    }
    row("editor_construct", size, timings)
}

fn measure_edit_undo(text: &str, size: usize, samples: usize) -> Result<String, Box<dyn Error>> {
    let mut editor = Editor::new(text);
    let midpoint = size / 2;
    let mut timings = Vec::with_capacity(samples);
    for _ in 0..samples {
        editor.set_cursor(ByteOffset(midpoint))?;
        let start = Instant::now();
        editor.type_text("x")?;
        editor.undo()?;
        timings.push(start.elapsed());
        black_box(editor.snapshot());
    }
    Ok(row("edit_undo", size, timings))
}

fn measure_search(text: &str, size: usize, samples: usize) -> Result<String, Box<dyn Error>> {
    let editor = Editor::new(text);
    let options = SearchOptions {
        query: SEARCH_MARKER.to_owned(),
        case_sensitive: true,
        max_matches: 4,
        ..SearchOptions::literal(SEARCH_MARKER)
    };
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    let mut timings = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let result = find_matches(&editor.snapshot(), &options, &cancelled)?;
        timings.push(start.elapsed());
        black_box(result);
    }
    Ok(row("search_marker", size, timings))
}

fn row(scenario: &str, size: usize, timings: Vec<Duration>) -> String {
    let mut nanos = timings
        .into_iter()
        .map(|duration| duration.as_nanos())
        .collect::<Vec<_>>();
    nanos.sort_unstable();
    let median = nanos[(nanos.len() - 1) / 2];
    let p95 = nanos[((nanos.len() * 95).div_ceil(100) - 1).min(nanos.len() - 1)];
    format!("{scenario},{size},{},{median},{p95}", nanos.len())
}
