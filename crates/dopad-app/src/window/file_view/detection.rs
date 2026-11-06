use std::path::Path;

use super::SupportedImageFormat;

pub(super) fn image_format_for_extension(path: &Path) -> Option<SupportedImageFormat> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some(SupportedImageFormat::Png),
        "jpg" | "jpeg" | "jpe" => Some(SupportedImageFormat::Jpeg),
        "gif" => Some(SupportedImageFormat::Gif),
        "bmp" | "dib" => Some(SupportedImageFormat::Bmp),
        "webp" => Some(SupportedImageFormat::WebP),
        "ico" => Some(SupportedImageFormat::Ico),
        _ => None,
    }
}

pub(super) fn looks_binary(prefix: &[u8]) -> bool {
    if prefix.starts_with(&[0xEF, 0xBB, 0xBF])
        || prefix.starts_with(&[0xFF, 0xFE])
        || prefix.starts_with(&[0xFE, 0xFF])
        || prefix.is_empty()
        || looks_like_bomless_utf16(prefix)
    {
        return false;
    }
    let controls = prefix
        .iter()
        .filter(|byte| matches!(**byte, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F))
        .count();
    controls * 100 > prefix.len()
}

fn looks_like_bomless_utf16(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    let even_nuls = bytes.iter().step_by(2).filter(|byte| **byte == 0).count();
    let odd_nuls = bytes
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|byte| **byte == 0)
        .count();
    even_nuls * 3 > bytes.len() / 2 || odd_nuls * 3 > bytes.len() / 2
}

pub(super) fn has_binary_signature(prefix: &[u8]) -> bool {
    prefix.starts_with(b"MZ")
        || prefix.starts_with(b"\x7fELF")
        || prefix.starts_with(b"%PDF-")
        || prefix.starts_with(b"PK\x03\x04")
        || prefix.starts_with(b"PK\x05\x06")
        || prefix.starts_with(b"PK\x07\x08")
        || prefix.starts_with(b"Rar!\x1a\x07")
        || prefix.starts_with(b"7z\xbc\xaf\x27\x1c")
        || prefix.starts_with(b"\x1f\x8b")
        || prefix.starts_with(b"BZh")
        || prefix.starts_with(b"\xfd7zXZ\0")
        || prefix.starts_with(b"SQLite format 3\0")
        || prefix.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1")
        || prefix.starts_with(b"ID3")
        || prefix.starts_with(b"fLaC")
        || prefix.starts_with(b"\x1a\x45\xdf\xa3")
        || (prefix.len() >= 8 && &prefix[4..8] == b"ftyp")
        || (prefix.starts_with(b"RIFF") && prefix.get(8..12) == Some(b"WAVE"))
}

pub(in crate::window) fn can_open_externally(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return true;
    };
    !matches!(
        extension.to_ascii_lowercase().as_str(),
        "exe"
            | "dll"
            | "com"
            | "scr"
            | "msi"
            | "bat"
            | "cmd"
            | "ps1"
            | "psm1"
            | "psd1"
            | "vbs"
            | "js"
            | "jscript"
            | "sh"
            | "bash"
            | "zsh"
            | "command"
            | "desktop"
            | "reg"
            | "lnk"
            | "app"
            | "jar"
            | "apk"
            | "run"
    )
}

pub(super) fn has_executable_signature(prefix: &[u8]) -> bool {
    prefix.starts_with(b"MZ")
        || prefix.starts_with(b"\x7fELF")
        || prefix.starts_with(b"#!")
        || matches!(
            prefix.get(..4),
            Some(b"\xfe\xed\xfa\xce")
                | Some(b"\xfe\xed\xfa\xcf")
                | Some(b"\xce\xfa\xed\xfe")
                | Some(b"\xcf\xfa\xed\xfe")
                | Some(b"\xca\xfe\xba\xbe")
        )
}

pub(in crate::window) fn open_externally(path: &Path) -> Result<(), String> {
    open::that(path)
        .map_err(|error| format!("Could not open this file with the system default app: {error}"))
}

pub(in crate::window) fn file_type_label(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_uppercase())
        .filter(|extension| !extension.is_empty())
        .unwrap_or_else(|| "Unknown".into())
}

pub(in crate::window) fn format_file_size(size: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = size as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", size, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
