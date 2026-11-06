use std::{path::PathBuf, time::Duration};

use dopad_core::{FileFingerprint, file_changed_since};
use gpui_kit::Context;

use super::DopadWindow;

impl DopadWindow {
    pub(super) fn start_auto_save_loop(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let Ok(ids) = this.update(cx, |view, _| view.auto_save_due()) else {
                    return;
                };
                for id in ids {
                    if this
                        .update(cx, |view, cx| view.save_document(id, false, None, cx))
                        .is_err()
                    {
                        return;
                    }
                }

                let Ok(candidates) = this.update(cx, |view, _| view.external_change_candidates())
                else {
                    return;
                };
                if candidates.is_empty() {
                    continue;
                }

                let changed = cx
                    .background_executor()
                    .spawn(async move {
                        candidates
                            .into_iter()
                            .filter_map(|(id, path, fingerprint)| {
                                match file_changed_since(&path, fingerprint) {
                                    Ok(true) => Some((id, path, fingerprint)),
                                    Ok(false) => None,
                                    Err(error) => {
                                        tracing::warn!(%error, "external file change check failed");
                                        None
                                    }
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .await;
                let _ = this.update(cx, |view, cx| {
                    view.mark_external_changes(&changed, cx);
                });
            }
        })
        .detach();
    }

    fn auto_save_due(&mut self) -> Vec<u64> {
        let settings = self.settings.current();
        if !settings.auto_save
            || self.last_auto_save.elapsed()
                < Duration::from_secs(u64::from(settings.save_interval_seconds))
        {
            return Vec::new();
        }
        self.last_auto_save = std::time::Instant::now();
        self.tabs
            .iter()
            .filter(|tab| tab.path.is_some() && tab.is_dirty())
            .map(|tab| tab.id)
            .collect()
    }

    fn external_change_candidates(&mut self) -> Vec<(u64, PathBuf, FileFingerprint)> {
        if !self.settings.current().detect_external_changes
            || self.last_external_check.elapsed() < Duration::from_secs(2)
        {
            return Vec::new();
        }
        self.last_external_check = std::time::Instant::now();
        self.tabs
            .iter()
            .filter(|tab| !tab.external_change_detected)
            .filter_map(|tab| Some((tab.id, tab.path.clone()?, tab.fingerprint?)))
            .collect()
    }

    fn mark_external_changes(
        &mut self,
        changes: &[(u64, PathBuf, FileFingerprint)],
        cx: &mut Context<Self>,
    ) {
        let mut active_changed = false;
        for (id, path, fingerprint) in changes {
            let is_active = self.active_tab().is_some_and(|active| active.id == *id);
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == *id) {
                let current_path = tab
                    .path
                    .as_ref()
                    .is_some_and(|current| super::paths::same_path(current, path));
                if current_path && tab.fingerprint == Some(*fingerprint) {
                    tab.external_change_detected = true;
                    active_changed |= is_active;
                }
            }
        }
        if active_changed {
            self.set_status(
                "The file changed outside Dopad; saving is blocked to protect the disk copy.",
            );
        }
        if !changes.is_empty() {
            cx.notify();
        }
    }
}
