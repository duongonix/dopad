use std::{
    io::{Cursor, Write},
    path::{Path, PathBuf},
    time::Instant,
};

use dopad_core::{Encoding, read_file_with_options};
use dopad_editor::Editor;
use image::{DynamicImage, ImageFormat};

use super::*;

fn image_bytes(format: ImageFormat) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        2,
        1,
        image::Rgba([196, 94, 52, 180]),
    ))
    .write_to(&mut bytes, format)
    .expect("test encoder should encode supported fixture");
    bytes.into_inner()
}

fn ico_bytes() -> Vec<u8> {
    let png = image_bytes(ImageFormat::Png);
    let mut bytes = vec![0, 0, 1, 0, 1, 0, 2, 1, 0, 0, 1, 0, 32, 0];
    bytes.extend_from_slice(&(png.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&22_u32.to_le_bytes());
    bytes.extend_from_slice(&png);
    bytes
}

fn write_file(extension: &str, bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("create test directory");
    let path = directory.path().join(format!("fixture.{extension}"));
    fs::write(&path, bytes).expect("write fixture");
    (directory, path)
}

fn load(path: &Path) -> Result<(PathBuf, LoadedFileView), FileOpenError> {
    load_file(path.to_path_buf(), true, Encoding::Utf8)
}

#[test]
fn opens_png_jpeg_gif_bmp_and_webp_by_signature() {
    for (extension, format, expected) in [
        ("png", ImageFormat::Png, SupportedImageFormat::Png),
        ("jpg", ImageFormat::Jpeg, SupportedImageFormat::Jpeg),
        ("gif", ImageFormat::Gif, SupportedImageFormat::Gif),
        ("bmp", ImageFormat::Bmp, SupportedImageFormat::Bmp),
        ("webp", ImageFormat::WebP, SupportedImageFormat::WebP),
    ] {
        let (_directory, path) = write_file(extension, &image_bytes(format));
        let (_, opened) = load(&path).expect("valid image should open");
        let LoadedFileView::Image(image) = opened else {
            panic!("recognized image should use an image tab");
        };
        assert_eq!(image.info.format, expected);
        assert_eq!((image.info.width, image.info.height), (2, 1));
        assert_eq!(image.render_image.frame_count(), 1);
    }
}

#[test]
fn opens_ico_and_routes_signature_without_image_extension() {
    let (_directory, ico_path) = write_file("ico", &ico_bytes());
    let (_, opened) = load(&ico_path).expect("valid ICO should open");
    let LoadedFileView::Image(image) = opened else {
        panic!("ICO should use an image tab");
    };
    assert_eq!(image.info.format, SupportedImageFormat::Ico);

    let (_directory, signature_path) = write_file("data", &image_bytes(ImageFormat::Png));
    assert!(matches!(
        load(&signature_path),
        Ok((_, LoadedFileView::Image(_)))
    ));
}

#[test]
fn svg_keeps_existing_text_preview_path() {
    let (_directory, path) = write_file("svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>");
    assert!(matches!(load(&path), Ok((_, LoadedFileView::Text(_)))));
}

#[test]
fn text_extension_with_binary_payload_never_creates_text_document() {
    let (_directory, path) = write_file("txt", b"\0\xFF\x01binary");
    assert!(matches!(
        load(&path),
        Ok((_, LoadedFileView::Unsupported(_)))
    ));
}

#[test]
fn invalid_utf8_without_binary_controls_is_reported_as_text_decode_error() {
    let (_directory, path) = write_file("unknown", b"caf\xe9 plain text");
    let error = match load(&path) {
        Err(error) => error,
        Ok(_) => panic!("invalid UTF-8 should not create a text buffer"),
    };
    assert_eq!(error.kind, FileOpenErrorKind::InvalidText);
}

#[test]
fn unknown_extension_with_valid_utf8_opens_as_text() {
    let (_directory, path) = write_file("notes", "Xin chào Dopad\nsecond line".as_bytes());
    let (_, opened) = load(&path).expect("valid UTF-8 text should open");
    let LoadedFileView::Text(text) = opened else {
        panic!("unknown text extension should use an editor tab");
    };
    assert_eq!(text.loaded.text, "Xin chào Dopad\nsecond line");
}

#[test]
fn binary_signatures_are_unsupported_independent_of_extension() {
    for signature in [
        &b"PK\x03\x04plain-looking text"[..],
        &b"%PDF-1.7"[..],
        &b"SQLite format 3\0"[..],
        &b"MZ\x90\0"[..],
        &b"7z\xbc\xaf\x27\x1c"[..],
    ] {
        let (_directory, path) = write_file("dat", signature);
        assert!(matches!(
            load(&path),
            Ok((_, LoadedFileView::Unsupported(_)))
        ));
    }
}

#[test]
fn unknown_extensions_that_contain_text_are_editable_even_when_named_like_images() {
    let (_directory, path) = write_file("png", b"this is ordinary UTF-8 source text");
    let (_, opened) = load(&path).expect("text content must not be rejected by extension");
    let LoadedFileView::Text(text) = opened else {
        panic!("text content should use the editor regardless of extension");
    };
    assert_eq!(text.loaded.text, "this is ordinary UTF-8 source text");
}

#[test]
fn text_script_extensions_are_classified_by_content() {
    let (_directory, path) = write_file("ps1", b"Write-Output 'hello'");
    assert!(matches!(load(&path), Ok((_, LoadedFileView::Text(_)))));
}

#[test]
fn corrupt_image_signature_is_a_read_error_instead_of_a_text_buffer() {
    let (_directory, path) = write_file("png", b"\x89PNG\r\n\x1a\ncorrupt image data");
    let error = match load(&path) {
        Err(error) => error,
        Ok(_) => panic!("corrupt image must fail"),
    };
    assert_eq!(error.kind, FileOpenErrorKind::InvalidImage);
}

#[test]
fn image_pixel_limit_rejects_oversized_dimensions() {
    assert!(validate_dimensions(8192, 2048).is_ok());
    assert!(validate_dimensions(8192, 2049).is_err());
    assert!(validate_dimensions(u32::MAX, u32::MAX).is_err());
}

#[test]
fn compressed_image_limit_rejects_before_opening_or_decoding() {
    let error = super::decode_image(
        Path::new("missing-large-image.png"),
        SupportedImageFormat::Png,
        "PNG",
        super::MAX_COMPRESSED_IMAGE_BYTES + 1,
        None,
    )
    .expect_err("compressed image limit should reject before file access");
    assert_eq!(error.kind, FileOpenErrorKind::ImageTooLarge);
}

#[test]
fn permission_denied_maps_to_a_distinct_user_error() {
    let error = super::io_error(
        Path::new("private.txt"),
        "TXT",
        Some(4),
        std::io::ErrorKind::PermissionDenied,
        "denied".into(),
    );
    assert_eq!(error.kind, FileOpenErrorKind::PermissionDenied);
    assert!(error.message.contains("Permission denied"));
}

#[test]
fn image_zoom_state_preserves_fit_native_zoom_and_pan_reset() {
    let mut state = ImageTabState::new(ImageFileInfo {
        format: SupportedImageFormat::Png,
        width: 1200,
        height: 800,
        file_size: 12,
    });
    state.fit_zoom = 0.5;
    state.zoom_by(1.2, state.fit_zoom);
    assert!((state.zoom.unwrap_or_default() - 0.6).abs() < f32::EPSILON);
    state.pan_x = 24.0;
    state.pan_y = -18.0;
    state.reset_zoom();
    assert_eq!(state.zoom, Some(1.0));
    assert_eq!((state.pan_x, state.pan_y), (0.0, 0.0));
    state.fit_to_window();
    assert_eq!(state.zoom, None);
}

#[test]
fn missing_path_reports_a_distinct_retryable_error() {
    let path = std::env::temp_dir().join("dopad-intentionally-missing-image.png");
    let error = match load(&path) {
        Err(error) => error,
        Ok(_) => panic!("missing file must report an error"),
    };
    assert_eq!(error.kind, FileOpenErrorKind::NotFound);
    assert!(error.message.contains("no longer exists"));
}

#[test]
fn script_and_executable_files_are_not_opened_externally() {
    assert!(!can_open_externally(Path::new("program.exe")));
    assert!(!can_open_externally(Path::new("script.ps1")));
    assert!(!can_open_externally(Path::new("run.command")));
    assert!(can_open_externally(Path::new("archive.zip")));
    assert!(has_executable_signature(b"MZ\x90\0"));
    assert!(has_executable_signature(b"\x7fELF\x02"));
    assert!(!has_executable_signature(b"\x7fPNG\r\n"));
}

#[test]
#[ignore = "manual warm-cache open-stage benchmark; run with --release --ignored --nocapture"]
fn benchmark_text_read_decode_and_document_model_at_requested_sizes() {
    const SIZES: [usize; 5] = [
        1_024,
        100 * 1_024,
        1_024 * 1_024,
        10 * 1_024 * 1_024,
        100 * 1_024 * 1_024,
    ];
    const LINE: &[u8] = b"let benchmark_value = 123; // generated fixture\n";
    let directory = tempfile::tempdir().expect("create benchmark directory");
    let mut chunk = Vec::with_capacity(64 * 1_024);
    while chunk.len() + LINE.len() <= 64 * 1_024 {
        chunk.extend_from_slice(LINE);
    }

    for size in SIZES {
        let path = directory.path().join(format!("fixture-{size}.txt"));
        let mut output = fs::File::create(&path).expect("create benchmark fixture");
        let mut remaining = size;
        while remaining > 0 {
            let count = remaining.min(chunk.len());
            output
                .write_all(&chunk[..count])
                .expect("write benchmark fixture");
            remaining -= count;
        }
        drop(output);

        // The fixture was just written, so these results represent warm OS page cache.
        let read_started = Instant::now();
        let loaded = read_file_with_options(&path, true, Encoding::Utf8)
            .expect("read and decode generated UTF-8 fixture");
        let read_decode = read_started.elapsed();

        let model_started = Instant::now();
        let editor = Editor::new(&loaded.text);
        let model = model_started.elapsed();
        assert_eq!(editor.snapshot().len_bytes(), loaded.text.len());
        println!(
            "bytes={size} warm_read_decode_ms={} core_model_ms={}",
            read_decode.as_secs_f64() * 1_000.0,
            model.as_secs_f64() * 1_000.0,
        );
        drop((editor, loaded));
    }
}
