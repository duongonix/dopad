use super::{UpdateDetails, UpdateState, current_version};
use crate::updater::github::ReleaseInfo;

#[test]
fn build_version_comes_from_the_cargo_package_version() {
    let parsed = semver::Version::parse(current_version());
    assert!(parsed.is_ok());
    assert_eq!(current_version(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn update_indicator_is_hidden_until_an_update_is_known() {
    assert!(!UpdateState::NotChecked.has_update());
    assert!(!UpdateState::Checking.has_update());
    assert!(!UpdateState::UpToDate.has_update());
}

#[test]
fn known_update_keeps_the_footer_indicator_through_package_handoff() {
    let details = UpdateDetails {
        release: ReleaseInfo {
            tag: "v1.2.3".into(),
            version: semver::Version::new(1, 2, 3),
            name: "Stable release".into(),
            body: String::new(),
            published_at: None,
            html_url: "https://github.com/duongonix/dopad/releases/tag/v1.2.3".into(),
            assets: Vec::new(),
        },
        current_version: semver::Version::new(1, 2, 2),
        notes: None,
        package: None,
        signing_key_available: true,
    };

    assert!(UpdateState::Available(details.clone()).has_update());
    assert!(UpdateState::PackageOpened(details).has_update());
}
