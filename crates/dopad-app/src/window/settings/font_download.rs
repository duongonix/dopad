use std::{
    borrow::Cow,
    collections::BTreeMap,
    sync::mpsc::{self, TryRecvError},
    thread,
    time::Duration,
};

use gpui_kit::component::progress::Progress;
use gpui_kit::{Context, FontWeight, IntoElement, ParentElement, Styled, div, px, rgb};

use crate::window::fonts::{self, FontInstallEvent};

use super::{theme, window::SettingsWindow};

pub(super) fn font_options(
    installed: impl IntoIterator<Item = String>,
    current: &str,
) -> Vec<(String, bool)> {
    let mut families = BTreeMap::new();
    for family in installed
        .into_iter()
        .chain([
            "System Monospace".to_owned(),
            "JetBrains Mono".to_owned(),
            "Cascadia Code".to_owned(),
        ])
        .chain((!current.trim().is_empty()).then(|| current.to_owned()))
    {
        let family = family.trim().to_owned();
        if !family.is_empty() {
            families.entry(family.to_lowercase()).or_insert(family);
        }
    }
    families
        .into_values()
        .map(|family| (family, true))
        .collect()
}

pub(super) struct FontInstallStatus {
    pub(super) target: FontTarget,
    family: String,
    stage: FontInstallStage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FontTarget {
    Editor,
    Terminal,
}

enum FontInstallStage {
    Downloading { received: u64, total: Option<u64> },
    Installing,
    Ready,
    Failed(String),
}

impl FontInstallStatus {
    fn downloading(family: String, target: FontTarget) -> Self {
        Self {
            target,
            family,
            stage: FontInstallStage::Downloading {
                received: 0,
                total: None,
            },
        }
    }
}

impl SettingsWindow {
    pub(super) fn choose_font(&mut self, family: &str, target: FontTarget, cx: &mut Context<Self>) {
        self.error = None;
        self.font_install_generation = self.font_install_generation.saturating_add(1);
        let generation = self.font_install_generation;
        let installed = cx
            .text_system()
            .all_font_names()
            .iter()
            .any(|name| name.eq_ignore_ascii_case(family));

        if installed || !fonts::is_downloadable(family) {
            set_font_family(&mut self.draft, target, family);
            self.font_install = None;
            cx.notify();
            return;
        }

        let Some(data_dir) = crate::settings::app_data_dir() else {
            self.font_install = Some(FontInstallStatus {
                target,
                family: family.to_owned(),
                stage: FontInstallStage::Failed(
                    "Dopad data directory is unavailable on this system.".into(),
                ),
            });
            cx.notify();
            return;
        };

        let family = family.to_owned();
        self.font_install = Some(FontInstallStatus::downloading(family.clone(), target));
        let text_system = cx.text_system().clone();
        let (sender, receiver) = mpsc::channel();
        let worker_family = family.clone();
        let worker_data_dir = data_dir.clone();
        let worker_sender = sender.clone();
        let worker = thread::Builder::new()
            .name("dopad-font-download".into())
            .spawn(move || {
                let result =
                    fonts::install_or_load(&worker_family, &worker_data_dir, &worker_sender)
                        .map_err(|error| format!("{error:#}"));
                let _ = sender.send(FontInstallEvent::Finished(result));
            });
        if let Err(error) = worker {
            self.font_install = Some(FontInstallStatus {
                target,
                family,
                stage: FontInstallStage::Failed(format!("Could not start font download: {error}")),
            });
            cx.notify();
            return;
        }

        cx.spawn(async move |this, cx| loop {
            match receiver.try_recv() {
                Ok(FontInstallEvent::Progress { received, total }) => {
                    if this
                        .update(cx, |settings, cx| {
                            if settings.font_install_generation == generation {
                                settings.font_install = Some(FontInstallStatus {
                                    target,
                                    family: family.clone(),
                                    stage: FontInstallStage::Downloading { received, total },
                                });
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(FontInstallEvent::Installing) => {
                    if this
                        .update(cx, |settings, cx| {
                            if settings.font_install_generation == generation {
                                settings.font_install = Some(FontInstallStatus {
                                    target,
                                    family: family.clone(),
                                    stage: FontInstallStage::Installing,
                                });
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(FontInstallEvent::Finished(result)) => {
                    let result = match result {
                        Ok(bytes) => match text_system.add_fonts(vec![Cow::Owned(bytes)]) {
                            Ok(()) => Ok(()),
                            Err(error) => {
                                let family_to_remove = family.clone();
                                let data_dir_to_clean = data_dir.clone();
                                cx.background_executor()
                                    .spawn(async move {
                                        fonts::remove_cached_font(
                                            &family_to_remove,
                                            &data_dir_to_clean,
                                        );
                                    })
                                    .await;
                                Err(format!("GPUI could not register this font: {error}"))
                            }
                        },
                        Err(error) => Err(error),
                    };
                    let update = this.update(cx, |settings, cx| {
                        if settings.font_install_generation != generation {
                            return;
                        }
                        settings.font_install = Some(FontInstallStatus {
                            target,
                            family: family.clone(),
                            stage: match result {
                                Ok(()) => {
                                    set_font_family(&mut settings.draft, target, &family);
                                    FontInstallStage::Ready
                                }
                                Err(error) => FontInstallStage::Failed(error),
                            },
                        });
                        cx.notify();
                    });
                    if let Err(error) = update {
                        tracing::debug!(%error, "font settings window closed during installation");
                    }
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    let _ = this.update(cx, |settings, cx| {
                        if settings.font_install_generation == generation {
                            settings.font_install = Some(FontInstallStatus {
                                target,
                                family: family.clone(),
                                stage: FontInstallStage::Failed(
                                    "Font worker stopped before completing.".into(),
                                ),
                            });
                            cx.notify();
                        }
                    });
                    return;
                }
                Err(TryRecvError::Empty) => {
                    cx.background_executor()
                        .timer(Duration::from_millis(80))
                        .await;
                }
            }
        })
        .detach();
        cx.notify();
    }
}

fn set_font_family(settings: &mut crate::settings::AppSettings, target: FontTarget, family: &str) {
    match target {
        FontTarget::Editor => settings.editor_font_family = family.to_owned(),
        FontTarget::Terminal => settings.terminal_font_family = family.to_owned(),
    }
}

pub(super) fn render_status(status: &FontInstallStatus) -> impl IntoElement {
    let (message, color, progress) = match &status.stage {
        FontInstallStage::Downloading { received, total } => {
            let detail = total
                .map(|total| format!("{}%", percentage(*received, total).round() as u32))
                .unwrap_or_else(|| format!("{} KB", received / 1024));
            let progress = Progress::new("settings-font-download")
                .accessibility_label(format!("Downloading {}", status.family))
                .color(rgb(theme::ACCENT))
                .loading(total.is_none())
                .value(total.map_or(0., |total| percentage(*received, total)))
                .w_full();
            (
                format!(
                    "Downloading {} to Dopad data folder ({detail})",
                    status.family
                ),
                theme::TEXT_SECONDARY,
                Some(progress.into_any_element()),
            )
        }
        FontInstallStage::Installing => {
            let progress = Progress::new("settings-font-installing")
                .accessibility_label(format!("Installing {}", status.family))
                .color(rgb(theme::ACCENT))
                .loading(true)
                .w_full();
            (
                format!("Installing {}...", status.family),
                theme::TEXT_SECONDARY,
                Some(progress.into_any_element()),
            )
        }
        FontInstallStage::Ready => (
            format!("{} is ready to use.", status.family),
            theme::ACCENT,
            None,
        ),
        FontInstallStage::Failed(error) => (
            format!("Could not install {}: {error}", status.family),
            0xA13D2D,
            None,
        ),
    };

    let mut row = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .px(px(2.))
        .pb(px(8.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(color))
        .child(message);
    if let Some(progress) = progress {
        row = row.child(progress);
    }
    row
}

fn percentage(received: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.;
    }
    (received as f64 * 100. / total as f64).clamp(0., 100.) as f32
}

#[cfg(test)]
mod tests {
    use super::{font_options, percentage};

    #[test]
    fn font_options_include_system_and_downloadable_faces_without_duplicates() {
        let options = font_options(
            [
                "Hack Nerd Font Mono".to_owned(),
                "Consolas".to_owned(),
                "consolas".to_owned(),
            ],
            "Hack Nerd Font Mono",
        );
        let names = options
            .iter()
            .map(|(family, _)| family.as_str())
            .collect::<Vec<_>>();

        assert!(names.contains(&"Hack Nerd Font Mono"));
        assert!(names.contains(&"System Monospace"));
        assert!(names.contains(&"JetBrains Mono"));
        assert!(names.contains(&"Cascadia Code"));
        assert_eq!(
            names
                .iter()
                .filter(|name| name.eq_ignore_ascii_case("consolas"))
                .count(),
            1
        );
        assert!(names.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn download_progress_handles_unknown_zero_and_overrun_totals() {
        assert_eq!(percentage(0, 0), 0.);
        assert_eq!(percentage(25, 100), 25.);
        assert_eq!(percentage(125, 100), 100.);
    }
}
