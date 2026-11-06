use std::{
    borrow::Cow,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::Sender,
    time::Duration,
};

use anyhow::{Context as _, bail};
use gpui_kit::Context;
use tempfile::NamedTempFile;
use ureq::Agent;

use super::DopadWindow;

const MAX_FONT_BYTES: usize = 8 * 1024 * 1024;
const MAX_LICENSE_BYTES: usize = 128 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy)]
struct FontAsset {
    family: &'static str,
    directory: &'static str,
    file_name: &'static str,
    font_url: &'static str,
    license_url: &'static str,
    notice: &'static str,
}

const FONT_ASSETS: [FontAsset; 2] = [
    FontAsset {
        family: "JetBrains Mono",
        directory: "jetbrains-mono",
        file_name: "JetBrainsMono.ttf",
        font_url: "https://raw.githubusercontent.com/JetBrains/JetBrainsMono/v2.304/fonts/variable/JetBrainsMono%5Bwght%5D.ttf",
        license_url: "https://raw.githubusercontent.com/JetBrains/JetBrainsMono/v2.304/OFL.txt",
        notice: "Copyright 2020 The JetBrains Mono Project Authors. Licensed under the SIL Open Font License, Version 1.1.",
    },
    FontAsset {
        family: "Cascadia Code",
        directory: "cascadia-code",
        file_name: "CascadiaCode.ttf",
        font_url: "https://raw.githubusercontent.com/google/fonts/3dd78844021e948ceb633d1dcee3f7885561b5d9/ofl/cascadiacode/CascadiaCode%5Bwght%5D.ttf",
        license_url: "https://raw.githubusercontent.com/google/fonts/3dd78844021e948ceb633d1dcee3f7885561b5d9/ofl/cascadiacode/OFL.txt",
        notice: "Cascadia Code is copyright 2019-present Microsoft Corporation. Licensed under the SIL Open Font License, Version 1.1.",
    },
];

pub(super) enum FontInstallEvent {
    Progress { received: u64, total: Option<u64> },
    Installing,
    Finished(Result<Vec<u8>, String>),
}

pub(super) fn is_downloadable(family: &str) -> bool {
    asset(family).is_some()
}

pub(super) fn load_cached_selected_font(
    family: String,
    data_dir: PathBuf,
    cx: &mut Context<DopadWindow>,
) {
    let Some(asset) = asset(&family) else {
        return;
    };
    let path = font_path(data_dir.as_path(), asset);
    let text_system = cx.text_system().clone();
    cx.spawn(async move |this, cx| {
        let bytes = cx
            .background_executor()
            .spawn(async move { read_cached_font(&path) })
            .await;
        let bytes = match bytes {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(font = family, %error, "cached editor font could not be read");
                return;
            }
        };
        if let Err(error) = text_system.add_fonts(vec![Cow::Owned(bytes)]) {
            tracing::warn!(font = family, %error, "cached editor font could not be registered");
            return;
        }
        let _ = this.update(cx, |window, cx| {
            window.refresh_terminal(cx);
            cx.notify();
        });
    })
    .detach();
}

pub(super) fn install_or_load(
    family: &str,
    data_dir: &Path,
    events: &Sender<FontInstallEvent>,
) -> anyhow::Result<Vec<u8>> {
    let asset = asset(family).context("font is not in the downloadable font catalog")?;
    let directory = data_dir.join("fonts").join(asset.directory);
    fs::create_dir_all(&directory).context("could not create Dopad's font data directory")?;
    let path = font_path(data_dir, asset);

    let cached = read_cached_font(&path).context("cached font could not be inspected")?;
    let had_cache = cached.is_some();
    let bytes = if let Some(bytes) = cached {
        bytes
    } else {
        let bytes = download_font(asset.font_url, events)?;
        validate_font(&bytes).context("download did not contain a valid TrueType/OpenType font")?;
        bytes
    };

    events
        .send(FontInstallEvent::Installing)
        .context("font progress receiver was closed")?;
    ensure_license(asset, &directory)?;
    if !had_cache {
        write_atomic(&path, &bytes).context("could not save font in Dopad's data directory")?;
    }
    Ok(bytes)
}

pub(super) fn remove_cached_font(family: &str, data_dir: &Path) {
    if let Some(asset) = asset(family) {
        let _ = fs::remove_file(font_path(data_dir, asset));
    }
}

fn asset(family: &str) -> Option<FontAsset> {
    FONT_ASSETS
        .iter()
        .copied()
        .find(|asset| asset.family == family)
}

fn font_path(data_dir: &Path, asset: FontAsset) -> PathBuf {
    data_dir
        .join("fonts")
        .join(asset.directory)
        .join(asset.file_name)
}

fn read_cached_font(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("could not open cached font"),
    };
    let size = file.metadata()?.len();
    if size == 0 || size > MAX_FONT_BYTES as u64 {
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take((MAX_FONT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if validate_font(&bytes).is_err() {
        return Ok(None);
    }
    Ok(Some(bytes))
}

fn download_font(url: &str, events: &Sender<FontInstallEvent>) -> anyhow::Result<Vec<u8>> {
    download_bytes(url, MAX_FONT_BYTES, |received, total| {
        let _ = events.send(FontInstallEvent::Progress { received, total });
    })
}

fn download_bytes(
    url: &str,
    limit: usize,
    mut progress: impl FnMut(u64, Option<u64>),
) -> anyhow::Result<Vec<u8>> {
    let config = Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .build();
    let agent = Agent::new_with_config(config);
    let mut response = agent
        .get(url)
        .header("User-Agent", "Dopad")
        .call()
        .with_context(|| format!("could not download {url}"))?;
    let total = response.body().content_length();
    if total.is_some_and(|length| length > limit as u64) {
        bail!("download exceeds the {limit}-byte safety limit");
    }
    let mut output = Vec::with_capacity(total.unwrap_or(0).min(limit as u64) as usize);
    let mut buffer = [0; 32 * 1024];
    let mut reader = response.body_mut().as_reader();
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        if output.len().saturating_add(count) > limit {
            bail!("download exceeds the {limit}-byte safety limit");
        }
        output.extend_from_slice(&buffer[..count]);
        progress(output.len() as u64, total);
    }
    if output.is_empty() {
        bail!("download returned an empty response");
    }
    Ok(output)
}

fn ensure_license(asset: FontAsset, directory: &Path) -> anyhow::Result<()> {
    let license_path = directory.join("OFL.txt");
    let license = match fs::read(&license_path) {
        Ok(bytes) if valid_ofl(&bytes) => bytes,
        _ => {
            let bytes = download_bytes(asset.license_url, MAX_LICENSE_BYTES, |_, _| {})
                .context("could not download the font license")?;
            anyhow::ensure!(valid_ofl(&bytes), "font license is not SIL OFL 1.1");
            write_atomic(&license_path, &bytes).context("could not save the font license")?;
            bytes
        }
    };
    anyhow::ensure!(valid_ofl(&license), "cached font license is invalid");
    write_atomic(&directory.join("NOTICE.txt"), asset.notice.as_bytes())
        .context("could not save the font copyright notice")?;
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .context("font cache path has no parent directory")?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn validate_font(bytes: &[u8]) -> anyhow::Result<()> {
    const VALID_SIGNATURES: [[u8; 4]; 4] = [*b"\x00\x01\x00\x00", *b"OTTO", *b"true", *b"typ1"];
    anyhow::ensure!(
        bytes.len() >= 12 && bytes.len() <= MAX_FONT_BYTES,
        "font data length is invalid"
    );
    anyhow::ensure!(
        VALID_SIGNATURES.contains(&bytes[..4].try_into().unwrap_or_default()),
        "font header is invalid"
    );
    Ok(())
}

fn valid_ofl(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes)
        .is_ok_and(|text| text.contains("SIL OPEN FONT LICENSE") && text.contains("Version 1.1"))
}

#[cfg(test)]
mod tests {
    use super::{
        FontInstallEvent, asset, install_or_load, is_downloadable, valid_ofl, validate_font,
    };

    #[test]
    fn download_catalog_only_contains_curated_monospace_families() {
        assert!(is_downloadable("JetBrains Mono"));
        assert!(is_downloadable("Cascadia Code"));
        assert!(!is_downloadable("System Monospace"));
        assert!(!is_downloadable("https://example.com/font.ttf"));
        assert!(asset("JetBrains Mono").is_some());
    }

    #[test]
    fn font_validation_rejects_html_and_oversized_payloads() {
        assert!(validate_font(b"<html>bad</html>").is_err());
        let mut valid = b"\x00\x01\x00\x00".to_vec();
        valid.resize(12, 0);
        assert!(validate_font(&valid).is_ok());
    }

    #[test]
    fn license_validation_requires_the_sil_open_font_license() {
        assert!(valid_ofl(b"SIL OPEN FONT LICENSE Version 1.1"));
        assert!(!valid_ofl(b"not a font license"));
    }

    #[test]
    fn cached_font_is_reused_and_keeps_its_license_and_notice() {
        let temp = tempfile::tempdir().expect("temporary app data directory");
        let asset = asset("JetBrains Mono").expect("curated font exists");
        let font_dir = temp.path().join("fonts").join(asset.directory);
        std::fs::create_dir_all(&font_dir).expect("font cache directory");
        let mut font = b"\x00\x01\x00\x00mock".to_vec();
        font.resize(12, 0);
        std::fs::write(font_dir.join(asset.file_name), &font).expect("cached font");
        std::fs::write(
            font_dir.join("OFL.txt"),
            "SIL OPEN FONT LICENSE Version 1.1",
        )
        .expect("cached license");
        let (sender, receiver) = std::sync::mpsc::channel();

        let loaded = install_or_load("JetBrains Mono", temp.path(), &sender)
            .expect("cached font should not need a network request");

        assert_eq!(loaded, font);
        assert!(matches!(
            receiver.try_recv(),
            Ok(FontInstallEvent::Installing)
        ));
        assert_eq!(
            std::fs::read_to_string(font_dir.join("NOTICE.txt")).expect("font notice"),
            asset.notice
        );
    }
}
