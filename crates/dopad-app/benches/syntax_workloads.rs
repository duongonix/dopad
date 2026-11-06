use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::{Duration, Instant};

use gpui_kit::component::highlighter::SyntaxHighlighter;
use gpui_kit::component::input::Rope;
use tree_sitter::{InputEdit, Point};

const MIB: usize = 1024 * 1024;
const VIEWPORT_BYTES: usize = 4096;
const RUST_UNIT: &str = "/// Increment a value without wrapping.\nfn increment(value: u32) -> u32 {\n    let count = value + 1;\n    count\n}\n";

fn main() -> Result<(), Box<dyn Error>> {
    let requested_size = std::env::var("DOPAD_SYNTAX_BENCH_SIZE_MIB")
        .ok()
        .map(|value| value.parse::<usize>())
        .transpose()?;
    if requested_size.is_some_and(|size| !matches!(size, 1 | 10 | 100)) {
        return Err("DOPAD_SYNTAX_BENCH_SIZE_MIB must be 1, 10, or 100".into());
    }
    let mut output = std::env::var_os("DOPAD_SYNTAX_BENCH_OUTPUT")
        .map(File::create)
        .transpose()?
        .map(BufWriter::new);
    if let Some(output) = output.as_mut() {
        writeln!(
            output,
            "size_bytes,workload,language,iterations,median_ms,p95_ms"
        )?;
    }

    for (mib, parse_iterations, edit_iterations) in [(1, 7, 20), (10, 3, 8), (100, 0, 0)] {
        if requested_size.is_some_and(|size| size != mib) {
            continue;
        }
        let source = fixture(mib * MIB);
        let rope = Rope::from_str(&source);
        if mib < 100 {
            let cold = cold_parse_samples(&rope, parse_iterations)?;
            record(&mut output, mib * MIB, "cold_parse", "rust", &cold)?;

            let edits = incremental_samples(&source, edit_iterations)?;
            record(&mut output, mib * MIB, "incremental_edit", "rust", &edits)?;

            let scrolls = viewport_samples(&source, &rope, 60)?;
            record(&mut output, mib * MIB, "viewport_styles", "rust", &scrolls)?;
        } else {
            // Dopad's default large-file policy uses plain text at 10 MiB and up.
            // Exercise that no-parser path at 100 MiB rather than forcing a costly full parse.
            let mut highlighter = SyntaxHighlighter::new("text");
            let started = Instant::now();
            if !highlighter.update(None, &rope, None) {
                return Err("plain-text large-file update unexpectedly failed".into());
            }
            let setup = vec![started.elapsed()];
            record(
                &mut output,
                mib * MIB,
                "large_file_fallback",
                "text",
                &setup,
            )?;
            let scrolls = viewport_samples_for_highlighter(&highlighter, source.len(), 60);
            record(
                &mut output,
                mib * MIB,
                "fallback_viewport_styles",
                "text",
                &scrolls,
            )?;
        }
    }
    if let Some(output) = output.as_mut() {
        output.flush()?;
    }
    Ok(())
}

fn fixture(size_bytes: usize) -> String {
    let mut source = String::with_capacity(size_bytes);
    while source.len() < size_bytes {
        let remaining = size_bytes - source.len();
        source.push_str(&RUST_UNIT[..remaining.min(RUST_UNIT.len())]);
    }
    source
}

fn cold_parse_samples(text: &Rope, iterations: usize) -> Result<Vec<Duration>, Box<dyn Error>> {
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let mut highlighter = SyntaxHighlighter::new("rust");
        let started = Instant::now();
        if !highlighter.update(None, text, None) {
            return Err("unbounded cold parse unexpectedly timed out".into());
        }
        samples.push(started.elapsed());
    }
    Ok(samples)
}

fn incremental_samples(source: &str, iterations: usize) -> Result<Vec<Duration>, Box<dyn Error>> {
    let mut highlighter = SyntaxHighlighter::new("rust");
    let mut current_source = source.to_owned();
    let mut current_text = Rope::from_str(&current_source);
    highlighter.update(None, &current_text, None);
    let unit_offset = source.len() / 2 / RUST_UNIT.len() * RUST_UNIT.len();
    let edit_offset = unit_offset
        + RUST_UNIT.find("value + 1").ok_or("fixture token missing")?
        + "value + ".len();
    let mut samples = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let replacement = if current_source.as_bytes()[edit_offset] == b'2' {
            b'3'
        } else {
            b'2'
        };
        let mut next_source = current_source.clone();
        next_source.replace_range(
            edit_offset..edit_offset + 1,
            &(replacement as char).to_string(),
        );
        let next_text = Rope::from_str(&next_source);
        let position = point_at_byte(&current_source, edit_offset);
        let end_position = point_at_byte(&current_source, edit_offset + 1);
        let edit = InputEdit {
            start_byte: edit_offset,
            old_end_byte: edit_offset + 1,
            new_end_byte: edit_offset + 1,
            start_position: position,
            old_end_position: end_position,
            new_end_position: end_position,
        };
        let started = Instant::now();
        if !highlighter.update(Some(edit), &next_text, None) {
            return Err("incremental update unexpectedly timed out".into());
        }
        samples.push(started.elapsed());
        current_source = next_source;
        current_text = next_text;
    }
    std::hint::black_box(current_text);
    Ok(samples)
}

fn point_at_byte(source: &str, byte: usize) -> Point {
    let prefix = &source[..byte];
    let row = prefix.bytes().filter(|value| *value == b'\n').count();
    let column = prefix
        .rfind('\n')
        .map_or(prefix.len(), |newline| prefix.len() - newline - 1);
    Point::new(row, column)
}

fn viewport_samples(
    source: &str,
    rope: &Rope,
    iterations: usize,
) -> Result<Vec<Duration>, Box<dyn Error>> {
    let mut highlighter = SyntaxHighlighter::new("rust");
    if !highlighter.update(None, rope, None) {
        return Err("viewport fixture parse unexpectedly timed out".into());
    }
    Ok(viewport_samples_for_highlighter(
        &highlighter,
        source.len(),
        iterations,
    ))
}

fn viewport_samples_for_highlighter(
    highlighter: &SyntaxHighlighter,
    source_len: usize,
    iterations: usize,
) -> Vec<Duration> {
    let theme = gpui_kit::component::highlighter::HighlightTheme::default_light();
    let max_start = source_len.saturating_sub(VIEWPORT_BYTES);
    let mut samples = Vec::with_capacity(iterations);
    for index in 0..iterations {
        let start = max_start.saturating_mul(index) / iterations.max(1);
        let range = start..(start + VIEWPORT_BYTES).min(source_len);
        let started = Instant::now();
        std::hint::black_box(highlighter.styles(&range, &*theme));
        samples.push(started.elapsed());
    }
    samples
}

fn record(
    output: &mut Option<BufWriter<File>>,
    size_bytes: usize,
    workload: &str,
    language: &str,
    samples: &[Duration],
) -> Result<(), Box<dyn Error>> {
    let mut milliseconds = samples
        .iter()
        .map(Duration::as_secs_f64)
        .map(|seconds| seconds * 1000.0)
        .collect::<Vec<_>>();
    milliseconds.sort_by(f64::total_cmp);
    let median = percentile(&milliseconds, 0.50);
    let p95 = percentile(&milliseconds, 0.95);
    println!(
        "{size_bytes},{workload},{language},{},{median:.3},{p95:.3}",
        samples.len()
    );
    if let Some(output) = output.as_mut() {
        writeln!(
            output,
            "{size_bytes},{workload},{language},{},{median:.3},{p95:.3}",
            samples.len()
        )?;
    }
    Ok(())
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    let index = ((sorted.len() as f64 * fraction).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len().saturating_sub(1));
    sorted.get(index).copied().unwrap_or_default()
}
