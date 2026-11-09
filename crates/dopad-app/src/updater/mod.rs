mod asset;
mod download;
mod github;
mod install;
mod verify;

pub(crate) use download::DownloadProgress;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use dopad_preview::{MarkdownDocument, PreviewContent};
use gpui_kit::{AnyWindowHandle, Context};
use semver::Version;

use self::github::ReleaseInfo;

#[derive(Clone, Debug)]
pub(crate) struct UpdateDetails {
    pub release: ReleaseInfo,
    pub current_version: Version,
    pub notes: Option<Arc<MarkdownDocument>>,
    pub package: Option<github::ReleaseAsset>,
    pub signing_key_available: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum UpdateState {
    NotChecked,
    Checking,
    UpToDate,
    Available(UpdateDetails),
    Downloading {
        details: UpdateDetails,
        progress: DownloadProgress,
    },
    ReadyToInstall {
        details: UpdateDetails,
        path: std::path::PathBuf,
    },
    Installing {
        details: UpdateDetails,
    },
    Installed(UpdateDetails),
    PackageOpened(UpdateDetails),
    Failed {
        details: Option<UpdateDetails>,
        message: String,
        retry_path: Option<std::path::PathBuf>,
    },
}

impl UpdateState {
    pub(crate) fn has_update(&self) -> bool {
        matches!(
            self,
            Self::Available(_)
                | Self::Downloading { .. }
                | Self::ReadyToInstall { .. }
                | Self::Installing { .. }
                | Self::Installed(_)
                | Self::PackageOpened(_)
                | Self::Failed {
                    details: Some(_),
                    ..
                }
        )
    }

    pub(crate) fn details(&self) -> Option<&UpdateDetails> {
        match self {
            Self::Available(details)
            | Self::Downloading { details, .. }
            | Self::ReadyToInstall { details, .. }
            | Self::Installing { details, .. }
            | Self::Installed(details)
            | Self::PackageOpened(details) => Some(details),
            Self::Failed { details, .. } => details.as_ref(),
            Self::NotChecked | Self::Checking | Self::UpToDate => None,
        }
    }
}

pub(crate) struct UpdateManager {
    pub(crate) state: UpdateState,
    pub(crate) window: Option<AnyWindowHandle>,
    cancel: Option<Arc<AtomicBool>>,
    download_after_check: bool,
}

impl Default for UpdateManager {
    fn default() -> Self {
        Self {
            state: UpdateState::NotChecked,
            window: None,
            cancel: None,
            download_after_check: false,
        }
    }
}

impl UpdateManager {
    pub(crate) fn check(&mut self, cx: &mut Context<Self>, force: bool) {
        if matches!(
            self.state,
            UpdateState::Checking | UpdateState::Downloading { .. }
        ) {
            return;
        }
        if !force && !matches!(self.state, UpdateState::NotChecked) {
            return;
        }
        self.state = UpdateState::Checking;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { check_release() })
                .await;
            let _ = this.update(cx, |manager, cx| {
                let download_after_check = std::mem::take(&mut manager.download_after_check);
                match result {
                    Ok(Some(details)) => {
                        manager.state = UpdateState::Available(details);
                        if download_after_check {
                            manager.start_download(cx);
                        } else {
                            cx.notify();
                        }
                    }
                    Ok(None) => {
                        manager.state = UpdateState::UpToDate;
                        cx.notify();
                    }
                    Err(error) => {
                        manager.state = UpdateState::Failed {
                            details: None,
                            message: error,
                            retry_path: None,
                        };
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    pub(crate) fn start_update(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.state,
            UpdateState::Checking
                | UpdateState::Downloading { .. }
                | UpdateState::Installing { .. }
        ) {
            return;
        }
        self.download_after_check = true;
        self.check(cx, true);
    }

    pub(crate) fn start_download(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Available(details) = self.state.clone() else {
            return;
        };
        if details.package.is_none() {
            self.state = UpdateState::Failed {
                details: Some(details),
                message: "No update package matches this operating system, architecture, and installation type.".into(),
                retry_path: None,
            };
            cx.notify();
            return;
        }
        if !details.signing_key_available {
            self.state = UpdateState::Failed {
                details: Some(details),
                message: "This build has no trusted update signing key. Update packages will not be downloaded or run.".into(),
                retry_path: None,
            };
            cx.notify();
            return;
        }

        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        self.state = UpdateState::Downloading {
            details: details.clone(),
            progress: DownloadProgress {
                received: 0,
                total: 0,
                elapsed: std::time::Duration::ZERO,
            },
        };
        cx.notify();

        let (sender, receiver) = async_channel::bounded(1);
        let progress_entity = cx.entity();
        cx.spawn(async move |_, cx| {
            while let Ok(progress) = receiver.recv().await {
                progress_entity.update(cx, |manager, cx| {
                    if let UpdateState::Downloading {
                        progress: current, ..
                    } = &mut manager.state
                    {
                        *current = progress;
                        cx.notify();
                    }
                });
            }
        })
        .detach();

        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let release = details.release.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    download::download_verified(&release, cancel.clone(), |progress| {
                        let _ = sender.try_send(progress);
                    })
                })
                .await;
            entity.update(cx, |manager, cx| {
                manager.cancel = None;
                match result {
                    Ok(path) => {
                        let auto_install = install::supports_automatic_install(&path);
                        manager.state = UpdateState::ReadyToInstall {
                            details: details.clone(),
                            path,
                        };
                        if auto_install {
                            manager.launch_installer(cx);
                        } else {
                            cx.notify();
                        }
                    }
                    Err(error) if error.to_string().contains("cancelled") => {
                        manager.state = UpdateState::Available(details);
                        cx.notify();
                    }
                    Err(error) => {
                        manager.state = UpdateState::Failed {
                            details: Some(details),
                            message: format!("Update download or verification failed: {error:#}"),
                            retry_path: None,
                        };
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    pub(crate) fn cancel_download(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        if let UpdateState::Downloading { details, .. } = self.state.clone() {
            self.state = UpdateState::Available(details);
            cx.notify();
        }
    }

    pub(crate) fn launch_installer(&mut self, cx: &mut Context<Self>) {
        let UpdateState::ReadyToInstall { details, path } = self.state.clone() else {
            return;
        };
        match install::launch_verified_package(&path) {
            #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
            Ok(install::PackageLaunch::Installer(mut child)) => {
                self.state = UpdateState::Installing {
                    details: details.clone(),
                };
                cx.notify();
                let entity = cx.entity();
                cx.spawn(async move |_, cx| {
                    loop {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(250))
                            .await;
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                entity.update(cx, |manager, cx| {
                                    manager.state = if status.success() {
                                        UpdateState::Installed(details)
                                    } else {
                                        UpdateState::Failed {
                                            details: Some(details),
                                            message: format!(
                                                "The Windows installer exited with status {status}."
                                            ),
                                            retry_path: Some(path),
                                        }
                                    };
                                    cx.notify();
                                });
                                break;
                            }
                            Ok(None) => {}
                            Err(error) => {
                                entity.update(cx, |manager, cx| {
                                    manager.state = UpdateState::Failed {
                                        details: Some(details),
                                        message: format!(
                                            "Could not monitor the Windows installer: {error}"
                                        ),
                                        retry_path: Some(path),
                                    };
                                    cx.notify();
                                });
                                break;
                            }
                        }
                    }
                })
                .detach();
                return;
            }
            Ok(install::PackageLaunch::External) => {
                self.state = UpdateState::PackageOpened(details)
            }
            Err(error) => {
                self.state = UpdateState::Failed {
                    details: Some(details),
                    message: format!("Could not open the verified update package: {error:#}"),
                    retry_path: Some(path),
                };
            }
        }
        cx.notify();
    }

    pub(crate) fn retry_package_open(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Failed {
            details: Some(details),
            retry_path: Some(path),
            ..
        } = self.state.clone()
        else {
            return;
        };
        self.state = UpdateState::ReadyToInstall { details, path };
        self.launch_installer(cx);
    }
}

fn check_release() -> Result<Option<UpdateDetails>, String> {
    let current_version = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|error| format!("Dopad was built with an invalid Cargo version: {error}"))?;
    let release = github::fetch_latest().map_err(|error| format!("{error:#}"))?;
    if release.version <= current_version {
        return Ok(None);
    }
    let notes = (!release.body.trim().is_empty())
        .then(|| {
            let cancelled = AtomicBool::new(false);
            dopad_preview::render_markdown(0, dopad_core::Revision(0), &release.body, &cancelled)
                .ok()
        })
        .flatten()
        .and_then(|preview| match preview.content {
            PreviewContent::Markdown(document) => Some(document),
            PreviewContent::Svg(_) => None,
        });
    let package = asset::select_asset(&release.version, &release.assets);
    let signing_key_available = verify::embedded_key().is_ok();
    Ok(Some(UpdateDetails {
        release,
        current_version,
        notes,
        package,
        signing_key_available,
    }))
}

pub(crate) fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests;
