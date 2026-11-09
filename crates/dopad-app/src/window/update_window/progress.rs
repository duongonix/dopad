use std::time::Duration;

use gpui_kit::{IntoElement, ParentElement, Styled, div, px, relative, rgb};

use crate::theme::Palette;
use crate::updater::{DownloadProgress, UpdateState, current_version};

pub(super) fn render_progress(progress: DownloadProgress, palette: Palette) -> impl IntoElement {
    let ratio = if progress.total == 0 {
        0.0
    } else {
        (progress.received as f32 / progress.total as f32).clamp(0.0, 1.0)
    };
    let downloaded = if progress.total == 0 {
        format!(
            "Downloaded {} · total size pending",
            format_bytes(progress.received)
        )
    } else {
        format!(
            "Downloaded {} of {} · {:.0}%",
            format_bytes(progress.received),
            format_bytes(progress.total),
            ratio * 100.0
        )
    };
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .h(px(6.))
                .w_full()
                .bg(rgb(palette.border))
                .child(div().h_full().w(relative(ratio)).bg(rgb(palette.accent))),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(rgb(palette.secondary))
                .child(downloaded),
        )
}

pub(super) fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    if value >= KB * KB * KB {
        format!("{:.1} GB", value / (KB * KB * KB))
    } else if value >= KB * KB {
        format!("{:.1} MB", value / (KB * KB))
    } else if value >= KB {
        format!("{:.0} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

pub(super) fn download_description(state: &UpdateState) -> String {
    match state {
        UpdateState::Available(details)
        | UpdateState::Failed {
            details: Some(details),
            ..
        } => {
            if !details.signing_key_available {
                "Secure updates are disabled in this build: the trusted signing key is not configured.".into()
            } else if let Some(package) = &details.package {
                format!(
                    "Download size: {} · Restart required",
                    format_bytes(package.size)
                )
            } else {
                "No compatible package is published for this installation.".into()
            }
        }
        UpdateState::Downloading { progress, .. } => {
            if progress.total == 0 {
                return "Preparing download…".into();
            }
            let elapsed = progress.elapsed.as_secs_f64();
            let speed = if elapsed > 0.1 {
                progress.received as f64 / elapsed
            } else {
                0.0
            };
            let remaining = if speed > 0.0 {
                Duration::from_secs_f64(
                    progress.total.saturating_sub(progress.received) as f64 / speed,
                )
                .as_secs()
            } else {
                0
            };
            let eta = (speed > 0.0).then(|| format!(" · About {remaining}s remaining"));
            format!(
                "{} / {} · {}/s{}",
                format_bytes(progress.received),
                format_bytes(progress.total),
                format_bytes(speed as u64),
                eta.unwrap_or_default()
            )
        }
        UpdateState::ReadyToInstall { details, .. } => details.package.as_ref().map_or_else(
            || "Verified package · restart required".into(),
            |package| {
                format!(
                    "Verified · {} · restart required",
                    format_bytes(package.size)
                )
            },
        ),
        UpdateState::Installing { .. } => "Installing the verified update…".into(),
        UpdateState::Installed(_) => "Update installed · restarting Dopad".into(),
        UpdateState::PackageOpened(_) => "Apply the package, then restart Dopad.".into(),
        UpdateState::NotChecked | UpdateState::Checking | UpdateState::UpToDate => {
            format!("Current version: v{}", current_version())
        }
        UpdateState::Failed { details: None, .. } => "Update package size unavailable".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::format_bytes;

    #[test]
    fn download_size_uses_binary_units() {
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
    }
}

