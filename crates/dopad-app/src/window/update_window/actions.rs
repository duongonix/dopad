use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Disableable,
    button::{Button, ButtonVariants},
};
use gpui_kit::{Context, IntoElement, ParentElement, Styled, div, rgb};

use crate::updater::UpdateState;

use super::UpdateWindow;

pub(super) fn render_footer_buttons(
    state: &UpdateState,
    cx: &mut Context<UpdateWindow>,
) -> impl IntoElement {
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
                .label("Update")
                .primary()
                .text_color(rgb(0xFFFFFF))
                .on_click(cx.listener(|this, _, _, cx| this.start_update(cx)))
        }
        UpdateState::Downloading { .. } => Button::new("update-cancel")
            .label("Cancel")
            .outline()
            .on_click(cx.listener(|this, _, _, cx| this.cancel_download(cx))),
        UpdateState::ReadyToInstall { .. } => Button::new("update-install")
            .icon(IconName::Download)
            .label("Install Update")
            .primary()
            .text_color(rgb(0xFFFFFF))
            .on_click(cx.listener(|this, _, _, cx| this.launch_package(cx))),
        UpdateState::Installing { .. } => Button::new("update-installing")
            .label("Installing…")
            .primary()
            .disabled(true),
        UpdateState::Installed(_) => Button::new("update-installed")
            .label("Installed")
            .primary()
            .disabled(true),
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
