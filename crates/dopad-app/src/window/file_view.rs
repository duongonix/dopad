use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use dopad_core::{Encoding, LoadedFile, read_open_file_with_prefix};
use dopad_editor::Editor;
use dopad_syntax::Language;
use gpui_kit::RenderImage;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

mod detection;
mod errors;
use super::image_preview::MAX_DECODED_PIXELS;
pub(super) use detection::{
    can_open_externally, file_type_label, format_file_size, open_externally,
};
use detection::{
    has_binary_signature, has_executable_signature, image_format_for_extension, looks_binary,
};
use errors::{file_error, image_error, invalid_image, io_error};

const SNIFF_BYTES: usize = 8 * 1024;
const MAX_COMPRESSED_IMAGE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_IMAGE_EDGE: u32 = 32_768;
const MAX_DECODER_ALLOCATION: u64 = 96 * 1024 * 1024;
static IMAGE_DECODE_GATE: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq)]
pub(super) enum TabContent {
    Text,
    Loading,
    Image(ImageTabState),
    Unsupported(UnsupportedFileInfo),
    Error(FileOpenError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SupportedImageFormat {
    Png,
    Jpeg,
    Gif,
    Bmp,
    WebP,
    Ico,
}

impl SupportedImageFormat {
    fn from_image(format: ImageFormat) -> Option<Self> {
        match format {
            ImageFormat::Png => Some(Self::Png),
            ImageFormat::Jpeg => Some(Self::Jpeg),
            ImageFormat::Gif => Some(Self::Gif),
            ImageFormat::Bmp => Some(Self::Bmp),
            ImageFormat::WebP => Some(Self::WebP),
            ImageFormat::Ico => Some(Self::Ico),
            _ => None,
        }
    }

    fn image_format(self) -> ImageFormat {
        match self {
            Self::Png => ImageFormat::Png,
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Gif => ImageFormat::Gif,
            Self::Bmp => ImageFormat::Bmp,
            Self::WebP => ImageFormat::WebP,
            Self::Ico => ImageFormat::Ico,
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
            Self::Bmp => "BMP",
            Self::WebP => "WebP",
            Self::Ico => "ICO",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ImageFileInfo {
    pub format: SupportedImageFormat,
    pub width: u32,
    pub height: u32,
    pub file_size: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ImageTabState {
    pub info: ImageFileInfo,
    /// `None` means Fit to Window; `Some(1.0)` is native-size 100% zoom.
    pub zoom: Option<f32>,
    pub fit_zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
    pub pan_drag: Option<PanDrag>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PanDrag {
    pub start_x: f32,
    pub start_y: f32,
    pub pan_x: f32,
    pub pan_y: f32,
}

impl ImageTabState {
    pub fn new(info: ImageFileInfo) -> Self {
        Self {
            info,
            zoom: None,
            fit_zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            pan_drag: None,
        }
    }

    pub fn zoom_by(&mut self, factor: f32, fit_scale: f32) {
        let current = self.zoom.unwrap_or(fit_scale.max(0.01));
        self.zoom = Some((current * factor).clamp(0.1, super::image_preview::MAX_ZOOM));
    }

    pub fn reset_zoom(&mut self) {
        self.zoom = Some(1.0);
        self.pan_x = 0.0;
        self.pan_y = 0.0;
    }

    pub fn fit_to_window(&mut self) {
        self.zoom = None;
        self.pan_x = 0.0;
        self.pan_y = 0.0;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct UnsupportedFileInfo {
    pub file_type: String,
    pub file_size: u64,
    pub external_error: Option<String>,
    pub can_open_externally: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FileOpenErrorKind {
    PermissionDenied,
    NotFound,
    InvalidImage,
    ImageTooLarge,
    ChangedDuringRead,
    UnsupportedEncoding,
    InvalidText,
    Io,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FileOpenError {
    pub kind: FileOpenErrorKind,
    pub file_type: String,
    pub file_size: Option<u64>,
    pub message: String,
    pub detail: String,
}

pub(super) struct LoadedImage {
    pub info: ImageFileInfo,
    pub render_image: Arc<RenderImage>,
}

pub(super) struct LoadedTextFile {
    pub loaded: LoadedFile,
    pub editor: Editor,
    pub language: Language,
}

pub(super) enum LoadedFileView {
    Text(Box<LoadedTextFile>),
    Image(LoadedImage),
    Unsupported(UnsupportedFileInfo),
}

pub(super) fn load_file(
    requested_path: PathBuf,
    detect_encoding: bool,
    default_encoding: Encoding,
) -> Result<(PathBuf, LoadedFileView), FileOpenError> {
    let path = requested_path
        .canonicalize()
        .unwrap_or_else(|_| requested_path.clone());
    let file_type = file_type_label(&path);
    let metadata = fs::metadata(&path)
        .map_err(|error| io_error(&path, &file_type, None, error.kind(), error.to_string()))?;
    if !metadata.is_file() {
        return Err(FileOpenError {
            kind: FileOpenErrorKind::Io,
            file_type,
            file_size: Some(metadata.len()),
            message: "This is not a regular file.".into(),
            detail: "The selected path does not refer to a regular file.".into(),
        });
    }

    let file_size = metadata.len();
    let mut file = File::open(&path).map_err(|error| {
        io_error(
            &path,
            &file_type,
            Some(file_size),
            error.kind(),
            error.to_string(),
        )
    })?;
    let prefix = read_prefix(&mut file, &path, &file_type, file_size)?;
    let extension_format = image_format_for_extension(&path);
    let signature_format = image::guess_format(&prefix)
        .ok()
        .and_then(SupportedImageFormat::from_image);

    if let Some(format) =
        signature_format.or_else(|| extension_format.filter(|_| looks_binary(&prefix)))
    {
        let (info, render_image) = decode_image(
            &path,
            format,
            &file_type,
            file_size,
            metadata.modified().ok(),
        )?;
        return Ok((
            path,
            LoadedFileView::Image(LoadedImage { info, render_image }),
        ));
    }

    if has_binary_signature(&prefix) || looks_binary(&prefix) {
        let can_open_externally = can_open_externally(&path) && !has_executable_signature(&prefix);
        return Ok((
            path,
            LoadedFileView::Unsupported(UnsupportedFileInfo {
                file_type,
                file_size,
                external_error: None,
                can_open_externally,
            }),
        ));
    }

    match read_open_file_with_prefix(&path, file, prefix, detect_encoding, default_encoding) {
        Ok(loaded) => {
            let editor = Editor::new(&loaded.text);
            let language = dopad_syntax::detect_language(Some(&loaded.path), &loaded.text);
            Ok((
                path,
                LoadedFileView::Text(Box::new(LoadedTextFile {
                    loaded,
                    editor,
                    language,
                })),
            ))
        }
        Err(error) => Err(file_error(path, file_type, file_size, error)),
    }
}

fn read_prefix(
    file: &mut File,
    path: &Path,
    file_type: &str,
    file_size: u64,
) -> Result<Vec<u8>, FileOpenError> {
    let mut prefix = vec![0; SNIFF_BYTES.min(file_size as usize)];
    file.read_exact(&mut prefix).map_err(|error| {
        io_error(
            path,
            file_type,
            Some(file_size),
            error.kind(),
            error.to_string(),
        )
    })?;
    Ok(prefix)
}

fn decode_image(
    path: &Path,
    format: SupportedImageFormat,
    file_type: &str,
    file_size: u64,
    modified_before: Option<std::time::SystemTime>,
) -> Result<(ImageFileInfo, Arc<RenderImage>), FileOpenError> {
    if file_size > MAX_COMPRESSED_IMAGE_BYTES {
        return Err(image_error(
            FileOpenErrorKind::ImageTooLarge,
            file_type,
            Some(file_size),
            "Image file is larger than Dopad's preview limit.",
            "The compressed image exceeds 128 MiB.".into(),
        ));
    }
    let _decode_guard = IMAGE_DECODE_GATE
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| {
            image_error(
                FileOpenErrorKind::Io,
                file_type,
                Some(file_size),
                "Dopad's image decoder is unavailable.",
                "The image decode gate was poisoned.".into(),
            )
        })?;

    let mut reader = ImageReader::open(path).map_err(|error| {
        io_error(
            path,
            file_type,
            Some(file_size),
            error.kind(),
            error.to_string(),
        )
    })?;
    reader.set_format(format.image_format());
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_EDGE);
    limits.max_image_height = Some(MAX_IMAGE_EDGE);
    limits.max_alloc = Some(MAX_DECODER_ALLOCATION);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|error| invalid_image(file_type, file_size, error.to_string()))?;
    let (width, height) = decoder.dimensions();
    validate_dimensions(width, height).map_err(|()| {
        image_error(
            FileOpenErrorKind::ImageTooLarge,
            file_type,
            Some(file_size),
            "Image dimensions exceed Dopad's preview limit.",
            format!("Decoded dimensions are {width} × {height} pixels."),
        )
    })?;
    decoder
        .set_limits({
            let mut limits = Limits::default();
            limits.max_image_width = Some(MAX_IMAGE_EDGE);
            limits.max_image_height = Some(MAX_IMAGE_EDGE);
            limits.max_alloc = Some(MAX_DECODER_ALLOCATION);
            limits
        })
        .map_err(|error| {
            image_error(
                FileOpenErrorKind::ImageTooLarge,
                file_type,
                Some(file_size),
                "Image dimensions exceed Dopad's preview limit.",
                error.to_string(),
            )
        })?;
    let orientation = decoder
        .orientation()
        .map_err(|error| invalid_image(file_type, file_size, error.to_string()))?;
    let mut decoded = DynamicImage::from_decoder(decoder)
        .map_err(|error| invalid_image(file_type, file_size, error.to_string()))?;
    decoded.apply_orientation(orientation);
    let mut pixels = decoded.into_rgba8();
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }

    let after = fs::metadata(path).map_err(|error| {
        io_error(
            path,
            file_type,
            Some(file_size),
            error.kind(),
            error.to_string(),
        )
    })?;
    if after.len() != file_size || after.modified().ok() != modified_before {
        return Err(image_error(
            FileOpenErrorKind::ChangedDuringRead,
            file_type,
            Some(file_size),
            "The file changed while Dopad was loading it.",
            "Image metadata changed during decoding.".into(),
        ));
    }

    let render_image = Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]));
    Ok((
        ImageFileInfo {
            format,
            width,
            height,
            file_size,
        },
        render_image,
    ))
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), ()> {
    let pixels = u64::from(width).checked_mul(u64::from(height)).ok_or(())?;
    if width == 0
        || height == 0
        || width > MAX_IMAGE_EDGE
        || height > MAX_IMAGE_EDGE
        || pixels > MAX_DECODED_PIXELS
    {
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
#[path = "file_view/tests.rs"]
mod tests;
