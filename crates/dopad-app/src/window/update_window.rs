use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    link::Link,
    scroll::ScrollableElement,
};
use gpui_kit::{
    Context, Entity, FontWeight, InteractiveElement, IntoElement, ListAlignment, ListState,
    ParentElement, Render, Styled, Window, WindowControlArea, div, img, px, rgb,
};

use crate::{
    settings::{SettingsService, save_file},
    theme::{self, Palette},
    updater::{UpdateManager, UpdateState, current_version},
};
use gpui_kit::component::Disableable;

use super::DopadWindow;
mod lifecycle;
mod progress;

const WINDOW_WIDTH: f32 = 760.0;
const WINDOW_HEIGHT: f32 = 540.0;

struct UpdateWindow {
    updater: Entity<UpdateManager>,
    settings: SettingsService,
    notes_list: ListState,
    error: Option<String>,
}

impl UpdateWindow {
    fn new(
        updater: Entity<UpdateManager>,
        settings: SettingsService,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&updater, |_, _, cx| cx.notify()).detach();
        Self {
            updater,
            settings,
            notes_list: ListState::new(0, ListAlignment::Top, px(220.)),
            error: None,
        }
    }

    fn set_auto_check_updates(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let mut settings = self.settings.current();
        if settings.auto_check_updates == enabled {
            return;
        }
        let Some(path) = self.settings.path() else {
            self.error = Some("Update preference could not be saved on this system.".into());
            cx.notify();
            return;
        };
        settings.auto_check_updates = enabled;
        let draft = settings.clone();
        let service = self.settings.clone();
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(
                    async move { save_file(&path, &draft).map_err(|error| format!("{error:#}")) },
                )
                .await;
            entity.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        service.replace(settings);
                        this.error = None;
                    }
                    Err(error) => {
                        this.error = Some(format!("Could not save update preference: {error}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn start_check(&mut self, force: bool, cx: &mut Context<Self>) {
        self.updater
            .update(cx, |manager, cx| manager.check(cx, force));
    }

    fn start_download(&mut self, cx: &mut Context<Self>) {
        self.updater
            .update(cx, |manager, cx| manager.start_download(cx));
    }

    fn cancel_download(&mut self, cx: &mut Context<Self>) {
        self.updater
            .update(cx, |manager, cx| manager.cancel_download(cx));
    }

    fn launch_package(&mut self, cx: &mut Context<Self>) {
        self.updater
            .update(cx, |manager, cx| manager.launch_installer(cx));
    }
}

impl Render for UpdateWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(false);
        let state = self.updater.read(cx).state.clone();
        let settings = self.settings.current();
        let heading = match &state {
            UpdateState::NotChecked => "Check for updates".to_owned(),
            UpdateState::Checking => "Checking for updates".to_owned(),
            UpdateState::UpToDate => "Dopad is up to date".to_owned(),
            _ => state.details().map_or_else(
                || "Update unavailable".to_owned(),
                |details| {
                    format!(
                        "Update to {} · current {}",
                        details.release.tag, details.current_version
                    )
                },
            ),
        };
        let body = render_body(&state, palette, window, self.notes_list.clone());
        let status_error = self.error.as_ref().map(|error| {
            div()
                .w_full()
                .text_size(px(12.))
                .text_color(rgb(palette.accent))
                .child(error.clone())
        });
        let checkbox = Checkbox::new("auto-check-updates")
            .label("Automatically check for updates")
            .checked(settings.auto_check_updates)
            .on_change(cx.listener(|this, value, _, cx| this.set_auto_check_updates(*value, cx)));
        let button_row = render_footer_buttons(&state, cx);
        let download_line = progress::download_description(&state);

        let header = div()
            .h(px(44.))
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px(px(18.))
            .bg(rgb(palette.chrome))
            .border_b_1()
            .border_color(rgb(palette.border))
            .child(
                div()
                    .h(px(44.))
                    .flex()
                    .items_center()
                    .gap_3()
                    .window_control_area(WindowControlArea::Drag)
                    .child(img(crate::icon::image()).size(px(22.)))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(palette.text))
                            .child("Dopad"),
                    ),
            )
            .child(
                Button::new("update-window-close")
                    .icon(IconName::WindowClose)
                    .compact()
                    .ghost()
                    .text_color(rgb(palette.secondary))
                    .accessibility_label("Close Update window")
                    .on_click(|_, window, _| window.remove_window()),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(palette.canvas))
            .children([
                header.into_any_element(),
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .px(px(28.))
                    .pt(px(22.))
                    .pb(px(16.))
                    .child(
                        div()
                            .text_size(px(24.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(palette.text))
                            .child(heading),
                    )
                    .child(render_description(&state, palette))
                    .child(body)
                    .children(status_error)
                    .into_any_element(),
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .border_t_1()
                    .border_color(rgb(palette.border))
                    .bg(rgb(palette.chrome))
                    .px(px(22.))
                    .py(px(12.))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(checkbox)
                            .child(button_row),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .justify_end()
                            .text_size(px(12.))
                            .text_color(rgb(palette.secondary))
                            .child(download_line),
                    )
                    .into_any_element(),
            ])
    }
}

fn render_description(state: &UpdateState, palette: &Palette) -> impl IntoElement {
    let text = match state {
        UpdateState::NotChecked => "Check GitHub Releases for a newer stable Dopad version.".to_owned(),
        UpdateState::Checking => "Dopad is checking the latest stable GitHub release.".to_owned(),
        UpdateState::UpToDate => "You already have the latest stable version.".to_owned(),
        UpdateState::Available(details) | UpdateState::Downloading { details, .. } => format!(
            "Version {} is available. Review the release notes before updating.",
            details.release.tag
        ),
        UpdateState::ReadyToInstall { .. } => {
            "The download finished and its signature and SHA-256 digest were verified.".to_owned()
        }
        UpdateState::PackageOpened(_) => {
            "The verified package was opened. Complete any installation or extraction, then restart Dopad.".to_owned()
        }
        UpdateState::Failed { message, .. } => message.clone(),
    };
    div()
        .w_full()
        .text_size(px(13.))
        .text_color(rgb(palette.secondary))
        .child(text)
}

fn render_body(
    state: &UpdateState,
    palette: &Palette,
    window: &Window,
    notes_list: ListState,
) -> gpui_kit::AnyElement {
    if let Some(details) = state.details() {
        let notes = if let Some(document) = details.notes.as_ref() {
            let preview = dopad_preview::PreviewContent::Markdown(document.clone());
            super::preview_view::render_preview(&preview, palette, window, 14., notes_list)
        } else {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(13.))
                .text_color(rgb(palette.secondary))
                .child("This release has no notes.")
                .into_any_element()
        };
        let progress = match state {
            UpdateState::Downloading { progress, .. } => {
                Some(progress::render_progress(*progress, *palette).into_any_element())
            }
            _ => None,
        };
        return div()
            .w_full()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(palette.text))
                    .child(format!("What's new in {}", details.release.name)),
            )
            .children(details.release.published_at.as_ref().map(|published| {
                div()
                    .text_size(px(11.))
                    .text_color(rgb(palette.secondary))
                    .child(format!("Published {published}"))
            }))
            .children(progress)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .border_1()
                    .border_color(rgb(palette.border))
                    .bg(rgb(palette.canvas))
                    .child(notes),
            )
            .child(release_link(&details.release.html_url, palette))
            .into_any_element();
    }

    let message = match state {
        UpdateState::NotChecked => "Installed version".to_owned(),
        UpdateState::Checking => "Retrieving release metadata from GitHub…".to_owned(),
        UpdateState::UpToDate => format!("Installed version: v{}", current_version()),
        UpdateState::Failed { message, .. } => message.clone(),
        UpdateState::Available(_)
        | UpdateState::Downloading { .. }
        | UpdateState::ReadyToInstall { .. }
        | UpdateState::PackageOpened(_) => String::new(),
    };
    div()
        .w_full()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(14.))
        .text_color(rgb(palette.secondary))
        .child(message)
        .into_any_element()
}

fn render_footer_buttons(state: &UpdateState, cx: &mut Context<UpdateWindow>) -> impl IntoElement {
    let retry_entity = cx.entity();
    let later = Button::new("update-remind-later")
        .label("Remind Me Later")
        .outline()
        .on_click(cx.listener(|_, _, window, _| window.remove_window()));
    let action = match state {
        UpdateState::Available(details)
            if details.package.is_some() && details.signing_key_available =>
        {
            Button::new("update-now")
                .icon(IconName::Download)
                .label("Update Now")
                .primary()
                .text_color(rgb(0xFFFFFF))
                .on_click(cx.listener(|this, _, _, cx| this.start_download(cx)))
        }
        UpdateState::Downloading { .. } => Button::new("update-cancel")
            .label("Cancel")
            .outline()
            .on_click(cx.listener(|this, _, _, cx| this.cancel_download(cx))),
        UpdateState::ReadyToInstall { .. } => Button::new("update-install")
            .icon(IconName::Download)
            .label("Open Verified Package")
            .primary()
            .text_color(rgb(0xFFFFFF))
            .on_click(cx.listener(|this, _, _, cx| this.launch_package(cx))),
        UpdateState::NotChecked | UpdateState::Failed { details: None, .. } => {
            Button::new("update-retry-check")
                .label("Check for Updates")
                .outline()
                .on_click(move |_, _, cx| {
                    retry_entity.update(cx, |this, cx| this.start_check(true, cx));
                })
        }
        UpdateState::Failed {
            retry_path: Some(_),
            ..
        } => Button::new("update-retry-package-open")
            .label("Open Verified Package Again")
            .outline()
            .on_click(move |_, _, cx| {
                retry_entity.update(cx, |this, cx| {
                    this.updater
                        .update(cx, |manager, cx| manager.retry_package_open(cx));
                });
            }),
        UpdateState::Failed {
            details: Some(details),
            ..
        } if details.package.is_some() && details.signing_key_available => {
            let details = details.clone();
            Button::new("update-retry-download")
                .label("Retry Download")
                .outline()
                .on_click(move |_, _, cx| {
                    retry_entity.update(cx, |this, cx| {
                        this.updater.update(cx, |manager, cx| {
                            manager.state = UpdateState::Available(details.clone());
                            manager.start_download(cx);
                        });
                    });
                })
        }
        UpdateState::UpToDate | UpdateState::Checking | UpdateState::PackageOpened(_) => {
            Button::new("update-check-again")
                .label(if matches!(state, UpdateState::Checking) {
                    "Checking…"
                } else {
                    "Check Again"
                })
                .outline()
                .disabled(matches!(state, UpdateState::Checking))
                .on_click(move |_, _, cx| {
                    retry_entity.update(cx, |this, cx| this.start_check(true, cx));
                })
        }
        UpdateState::Available(_) | UpdateState::Failed { .. } => Button::new("update-unavailable")
            .label("View Release")
            .outline()
            .on_click(cx.listener(|this, _, _, cx| this.open_release_page(cx))),
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(later)
        .child(action)
}

fn release_link(url: &str, palette: &Palette) -> impl IntoElement {
    Link::new("update-release-link")
        .href(url.to_owned())
        .text_color(rgb(palette.accent))
        .child("View Release on GitHub")
}

impl UpdateWindow {
    fn open_release_page(&self, cx: &Context<Self>) {
        if let Some(details) = self.updater.read(cx).state.details().cloned()
            && let Err(error) = open::that(details.release.html_url)
        {
            tracing::warn!(%error, "could not open the Dopad release page");
        }
    }
}

impl DopadWindow {
    pub(super) fn open_update(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let current_window = self.updater.read(cx).window;
        if let Some(handle) = current_window
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }

        match lifecycle::open(self.updater.clone(), self.settings.clone(), cx) {
            Ok(handle) => self
                .updater
                .update(cx, |manager, _| manager.window = Some(handle)),
            Err(error) => {
                self.set_status(format!("Could not open Update: {error:#}"));
                cx.notify();
                return;
            }
        }
        let state = self.updater.read(cx).state.clone();
        if matches!(
            state,
            UpdateState::NotChecked | UpdateState::Failed { details: None, .. }
        ) {
            self.updater
                .update(cx, |manager, cx| manager.check(cx, true));
        }
    }
}
