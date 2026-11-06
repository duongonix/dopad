use std::{io::Read, time::Duration};

use semver::Version;
use serde::Deserialize;

const API_URL: &str = "https://api.github.com/repos/duongonix/dopad/releases/latest";
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_RELEASE_NOTES_BYTES: usize = 256 * 1024;
const MAX_ASSETS: usize = 64;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct ReleaseAsset {
    pub name: String,
    pub size: u64,
    #[serde(rename = "browser_download_url")]
    pub url: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ReleaseInfo {
    pub tag: String,
    pub version: Version,
    pub name: String,
    pub body: String,
    pub published_at: Option<String>,
    pub html_url: String,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    published_at: Option<String>,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}

pub(crate) fn fetch_latest() -> anyhow::Result<ReleaseInfo> {
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(concat!("Dopad/", env!("CARGO_PKG_VERSION")))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut response = agent
        .get(API_URL)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(|error| anyhow::anyhow!(format_api_error(error)))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_RESPONSE_BYTES,
        "GitHub release response exceeded the size limit"
    );
    let release: ApiRelease = serde_json::from_slice(&bytes)
        .map_err(|error| anyhow::anyhow!("GitHub returned invalid release metadata: {error}"))?;
    validate_release(release)
}

fn format_api_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(status @ (403 | 429)) => format!(
            "GitHub denied the release request (HTTP {status}); the public API rate limit may have been reached. Try again later."
        ),
        ureq::Error::StatusCode(status) => {
            format!("GitHub Releases returned HTTP {status}.")
        }
        ureq::Error::Timeout(_) => "The GitHub release check timed out.".into(),
        ureq::Error::HostNotFound => "Could not resolve the GitHub host.".into(),
        error => format!("GitHub release check failed: {error}"),
    }
}

fn validate_release(release: ApiRelease) -> anyhow::Result<ReleaseInfo> {
    anyhow::ensure!(
        !release.draft && !release.prerelease,
        "latest release is not stable"
    );
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name)
        .parse::<Version>()
        .map_err(|error| anyhow::anyhow!("release tag is not semantic versioning: {error}"))?;
    anyhow::ensure!(
        version.pre.is_empty(),
        "prereleases are not offered by auto-update"
    );
    anyhow::ensure!(
        release.body.len() <= MAX_RELEASE_NOTES_BYTES,
        "release notes exceed the size limit"
    );
    anyhow::ensure!(
        release.assets.len() <= MAX_ASSETS,
        "release has too many assets"
    );
    anyhow::ensure!(
        trusted_release_url(&release.html_url),
        "release URL is not an official GitHub URL"
    );
    for asset in &release.assets {
        anyhow::ensure!(!asset.name.is_empty(), "release contains an unnamed asset");
        anyhow::ensure!(
            trusted_asset_url(&asset.url),
            "release contains an unsafe asset URL"
        );
    }
    let name = if release.name.trim().is_empty() {
        release.tag_name.clone()
    } else {
        release.name
    };
    Ok(ReleaseInfo {
        tag: release.tag_name,
        version,
        name,
        body: release.body,
        published_at: release.published_at,
        html_url: release.html_url,
        assets: release.assets,
    })
}

fn trusted_release_url(url: &str) -> bool {
    url.starts_with("https://github.com/duongonix/dopad/releases/")
}

pub(super) fn trusted_asset_url(url: &str) -> bool {
    [
        "https://github.com/duongonix/dopad/releases/download/",
        "https://release-assets.githubusercontent.com/",
    ]
    .iter()
    .any(|prefix| url.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> ApiRelease {
        ApiRelease {
            tag_name: tag.into(),
            name: String::new(),
            body: "## Changes".into(),
            published_at: None,
            html_url: format!("https://github.com/duongonix/dopad/releases/tag/{tag}"),
            draft: false,
            prerelease: false,
            assets: Vec::new(),
        }
    }

    #[test]
    fn accepts_stable_semver_tag_with_optional_v_prefix() {
        assert_eq!(
            validate_release(release("v1.2.3")).unwrap().version,
            Version::new(1, 2, 3)
        );
        assert_eq!(
            validate_release(release("1.2.3")).unwrap().version,
            Version::new(1, 2, 3)
        );
    }

    #[test]
    fn rejects_prerelease_draft_and_invalid_semver() {
        assert!(validate_release(release("v1.2.3-beta.1")).is_err());
        let mut draft = release("v1.2.3");
        draft.draft = true;
        assert!(validate_release(draft).is_err());
        assert!(validate_release(release("latest")).is_err());
    }

    #[test]
    fn rejects_untrusted_urls_and_oversized_notes() {
        let mut unsafe_url = release("v1.2.3");
        unsafe_url.html_url = "https://example.com/release".into();
        assert!(validate_release(unsafe_url).is_err());

        let mut long_notes = release("v1.2.3");
        long_notes.body = "x".repeat(MAX_RELEASE_NOTES_BYTES + 1);
        assert!(validate_release(long_notes).is_err());
    }

    #[test]
    fn api_rate_limit_and_network_failures_are_actionable() {
        assert!(format_api_error(ureq::Error::StatusCode(403)).contains("rate limit"));
        assert!(format_api_error(ureq::Error::StatusCode(429)).contains("Try again later"));
        assert!(format_api_error(ureq::Error::HostNotFound).contains("resolve the GitHub host"));
    }
}
