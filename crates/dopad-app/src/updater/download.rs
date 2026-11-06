use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use tempfile::Builder;

use super::{
    asset::{self, SignedManifest},
    github::{ReleaseAsset, ReleaseInfo},
    verify,
};

const MAX_PACKAGE_SIZE: u64 = 2 * 1024 * 1024 * 1024;
const CHUNK_SIZE: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DownloadProgress {
    pub received: u64,
    pub total: u64,
    pub elapsed: Duration,
}

pub(crate) fn download_verified(
    release: &ReleaseInfo,
    cancelled: Arc<AtomicBool>,
    mut report: impl FnMut(DownloadProgress),
) -> anyhow::Result<PathBuf> {
    let target = asset::current_target_for_download(&release.version)
        .ok_or_else(|| anyhow::anyhow!("no update package is defined for this installation"))?;
    let package = release
        .assets
        .iter()
        .find(|asset| asset.name == target.asset_name)
        .ok_or_else(|| {
            anyhow::anyhow!("no update package matches this OS, architecture, and install type")
        })?;
    anyhow::ensure!(
        package.size > 0 && package.size <= MAX_PACKAGE_SIZE,
        "update package size is invalid"
    );

    let manifest_name = format!("dopad-update-{}.json", target.id);
    let signature_name = format!("dopad-update-{}.sig", target.id);
    let manifest_asset = find_asset(&release.assets, &manifest_name)?;
    let signature_asset = find_asset(&release.assets, &signature_name)?;
    let manifest_bytes = download_small(manifest_asset, 64 * 1024)?;
    let signature = String::from_utf8(download_small(signature_asset, 4096)?)?;
    let key = verify::embedded_key()?;
    let manifest: SignedManifest = verify::verify_manifest(&key, &manifest_bytes, &signature)?;
    validate_manifest(&manifest, release, package, &target.id)?;
    if cancelled.load(Ordering::Relaxed) {
        anyhow::bail!("update download cancelled");
    }

    let cache = crate::settings::app_data_dir()
        .ok_or_else(|| anyhow::anyhow!("application data directory is unavailable"))?
        .join("updates");
    fs::create_dir_all(&cache)?;
    let package_path = PathBuf::from(&package.name);
    let extension = package_path
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| {
            !extension.is_empty() && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        .ok_or_else(|| anyhow::anyhow!("update package has an invalid file extension"))?;
    let mut temporary = Builder::new()
        .prefix("dopad-update-")
        .suffix(&format!(".{extension}"))
        .tempfile_in(cache)?;

    let agent = http_agent(Duration::from_secs(20 * 60));
    let mut response = agent
        .get(&package.url)
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(|error| anyhow::anyhow!("update download failed: {error}"))?;
    if let Some(content_length) = response.body().content_length() {
        anyhow::ensure!(
            content_length == manifest.size,
            "download size differs from the signed manifest"
        );
    }

    let mut reader = response.body_mut().as_reader();
    let mut buffer = [0_u8; CHUNK_SIZE];
    let mut digest = Sha256::new();
    let mut received = 0_u64;
    let mut last_reported = 0_u64;
    let mut last_report = Instant::now();
    let started = last_report;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            anyhow::bail!("update download cancelled");
        }
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        received = received
            .checked_add(count as u64)
            .ok_or_else(|| anyhow::anyhow!("update download size overflow"))?;
        anyhow::ensure!(
            received <= manifest.size,
            "download exceeded the signed package size"
        );
        temporary.write_all(&buffer[..count])?;
        digest.update(&buffer[..count]);
        if received.saturating_sub(last_reported) >= 512 * 1024
            || last_report.elapsed() >= Duration::from_millis(150)
        {
            report(DownloadProgress {
                received,
                total: manifest.size,
                elapsed: started.elapsed(),
            });
            last_reported = received;
            last_report = Instant::now();
        }
    }
    anyhow::ensure!(
        received == manifest.size,
        "download ended before the signed package was complete"
    );
    temporary.as_file().sync_all()?;
    let actual_digest = format!("{:x}", digest.finalize());
    anyhow::ensure!(
        actual_digest.eq_ignore_ascii_case(&manifest.sha256),
        "downloaded update did not match its signed SHA-256 digest"
    );
    if cancelled.load(Ordering::Relaxed) {
        anyhow::bail!("update download cancelled");
    }
    report(DownloadProgress {
        received,
        total: manifest.size,
        elapsed: started.elapsed(),
    });
    let (_, path) = temporary.keep()?;
    Ok(path)
}

fn find_asset<'a>(assets: &'a [ReleaseAsset], name: &str) -> anyhow::Result<&'a ReleaseAsset> {
    assets
        .iter()
        .find(|asset| asset.name == name)
        .ok_or_else(|| anyhow::anyhow!("release is missing the signed update manifest"))
}

fn download_small(asset: &ReleaseAsset, limit: u64) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        asset.size <= limit,
        "signed update metadata exceeds the size limit"
    );
    let agent = http_agent(Duration::from_secs(30));
    let mut response = agent
        .get(&asset.url)
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(|error| anyhow::anyhow!("could not download signed update metadata: {error}"))?;
    let mut bytes = Vec::with_capacity(asset.size as usize);
    response
        .body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= limit,
        "signed update metadata exceeds the size limit"
    );
    anyhow::ensure!(
        bytes.len() as u64 == asset.size,
        "signed update metadata size did not match GitHub"
    );
    Ok(bytes)
}

fn validate_manifest(
    manifest: &SignedManifest,
    release: &ReleaseInfo,
    asset: &ReleaseAsset,
    target_id: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        manifest.version == release.version.to_string(),
        "signed update manifest version does not match the release"
    );
    anyhow::ensure!(
        manifest.asset_name == asset.name && manifest.size == asset.size,
        "signed update manifest does not match the selected package"
    );
    anyhow::ensure!(
        manifest.target == target_id,
        "signed update manifest target does not match this device"
    );
    Ok(())
}

fn http_agent(timeout: Duration) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(timeout))
        .user_agent(concat!("Dopad/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::new_with_config(config)
}

#[cfg(test)]
mod tests {
    use semver::Version;

    use super::*;
    use crate::updater::github::ReleaseAsset;

    #[test]
    fn signed_manifest_must_match_release_and_selected_package() {
        let release = ReleaseInfo {
            tag: "v1.2.3".into(),
            version: Version::new(1, 2, 3),
            name: "1.2.3".into(),
            body: String::new(),
            published_at: None,
            html_url: "https://github.com/duongonix/dopad/releases/tag/v1.2.3".into(),
            assets: Vec::new(),
        };
        let asset = ReleaseAsset {
            name: "dopad-v1.2.3-windows-x86_64-setup.exe".into(),
            size: 42,
            url: "https://github.com/duongonix/dopad/releases/download/v1.2.3/package.exe".into(),
        };
        let manifest = SignedManifest {
            version: "1.2.3".into(),
            target: "windows-x86_64".into(),
            asset_name: asset.name.clone(),
            size: 42,
            sha256: "a".repeat(64),
        };

        assert!(validate_manifest(&manifest, &release, &asset, "windows-x86_64").is_ok());
        assert!(validate_manifest(&manifest, &release, &asset, "windows-arm64").is_err());
    }
}
