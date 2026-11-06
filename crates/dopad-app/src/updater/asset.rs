#[cfg(target_os = "linux")]
use std::{path::Path, process::Command};

use semver::Version;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct SignedManifest {
    pub version: String,
    pub target: String,
    pub asset_name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PackageTarget {
    pub id: String,
    pub asset_name: String,
}

pub(crate) fn select_asset(
    version: &Version,
    assets: &[super::github::ReleaseAsset],
) -> Option<super::github::ReleaseAsset> {
    let target = current_target_for_download(version)?;
    assets
        .iter()
        .find(|asset| asset.name == target.asset_name)
        .cloned()
}

pub(crate) fn current_target_for_download(version: &Version) -> Option<PackageTarget> {
    let tag = format!("v{version}");
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    let (target_id, suffix) = ("windows-x86_64", "-setup.exe");
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    let (target_id, suffix) = ("windows-arm64", ".zip");
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    let (target_id, suffix) = ("macos-x64", ".zip");
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    let (target_id, suffix) = ("macos-arm64", ".zip");
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    let (target_id, suffix) = linux_target("linux-x86_64", current_linux_package()?);
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    let (target_id, suffix) = linux_target("linux-aarch64", current_linux_package()?);
    #[cfg(not(any(
        all(
            target_os = "windows",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ),
        all(
            target_os = "macos",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ),
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    )))]
    return None;

    let asset_name = format!("dopad-{tag}-{target_id}{suffix}");
    Some(PackageTarget {
        id: target_id.into(),
        asset_name,
    })
}

#[cfg(target_os = "linux")]
fn linux_target(platform: &'static str, package: &'static str) -> (&'static str, String) {
    let package = match package {
        "deb" => "deb",
        "rpm" => "rpm",
        "AppImage" => "AppImage",
        _ => "unsupported",
    };
    let id = match (platform, package) {
        ("linux-x86_64", "deb") => "linux-x86_64-deb",
        ("linux-x86_64", "rpm") => "linux-x86_64-rpm",
        ("linux-x86_64", "AppImage") => "linux-x86_64-AppImage",
        ("linux-aarch64", "deb") => "linux-aarch64-deb",
        ("linux-aarch64", "rpm") => "linux-aarch64-rpm",
        ("linux-aarch64", "AppImage") => "linux-aarch64-AppImage",
        _ => platform,
    };
    (id, format!(".{package}"))
}

#[cfg(target_os = "linux")]
fn current_linux_package() -> Option<&'static str> {
    if std::env::var_os("APPIMAGE").is_some() {
        return Some("AppImage");
    }

    let executable = std::env::current_exe().ok()?;
    if !executable.starts_with("/usr/lib/dopad") {
        return None;
    }
    if package_query("dpkg-query", &["-S"], &executable) {
        Some("deb")
    } else if package_query("rpm", &["-qf"], &executable) {
        Some("rpm")
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn package_query(command: &str, args: &[&str], executable: &Path) -> bool {
    Command::new(command)
        .args(args)
        .arg(executable)
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updater::github::ReleaseAsset;

    fn asset(name: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: name.into(),
            size: 1,
            url: "https://github.com/duongonix/dopad/releases/download/test/asset".into(),
        }
    }

    #[test]
    fn selects_only_the_exact_current_platform_asset() {
        let version = Version::parse("1.2.3").expect("valid test version");
        let assets = vec![
            asset("dopad-v1.2.3-windows-x86_64-setup.exe"),
            asset("dopad-v1.2.3-windows-arm64.zip"),
        ];
        let selected = select_asset(&version, &assets);

        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        assert_eq!(
            selected.map(|asset| asset.name),
            Some("dopad-v1.2.3-windows-x86_64-setup.exe".into())
        );
        #[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
        assert!(selected.is_none());
    }

    #[test]
    fn unsupported_architecture_does_not_fall_back_to_another_asset() {
        let version = Version::parse("1.2.3").expect("valid test version");
        assert!(select_asset(&version, &[]).is_none());
    }

    #[cfg(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
    ))]
    #[test]
    fn current_target_uses_the_release_asset_name_for_this_platform() {
        let version = Version::parse("1.2.3").expect("valid test version");
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        assert_eq!(
            current_target_for_download(&version).map(|target| target.asset_name),
            Some("dopad-v1.2.3-windows-x86_64-setup.exe".into())
        );
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        assert_eq!(
            current_target_for_download(&version).map(|target| target.asset_name),
            Some("dopad-v1.2.3-macos-x64.zip".into())
        );
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        assert_eq!(
            current_target_for_download(&version).map(|target| target.asset_name),
            Some("dopad-v1.2.3-macos-arm64.zip".into())
        );
        #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
        assert_eq!(
            current_target_for_download(&version).map(|target| target.asset_name),
            Some("dopad-v1.2.3-windows-arm64.zip".into())
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_package_targets_use_matching_architecture_names() {
        assert_eq!(
            linux_target("linux-x86_64", "deb"),
            ("linux-x86_64-deb", ".deb".into())
        );
        assert_eq!(
            linux_target("linux-x86_64", "rpm"),
            ("linux-x86_64-rpm", ".rpm".into())
        );
        assert_eq!(
            linux_target("linux-aarch64", "deb"),
            ("linux-aarch64-deb", ".deb".into())
        );
        assert_eq!(
            linux_target("linux-aarch64", "rpm"),
            ("linux-aarch64-rpm", ".rpm".into())
        );
    }
}
