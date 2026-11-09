use gpui_kit::base::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::progress::Progress;
use gpui_kit::{
    AppContext as _, Context, IntoElement, ParentElement as _, Styled as _, div, px, rgb,
};

use super::manager::{InstallPrompt, PromptPhase};
use crate::theme::Palette;
use crate::window::DopadWindow;

pub(in crate::window) fn render_install_toast(
    window: &DopadWindow,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    let Some(prompt) = window.lsp.prompt() else {
        return div().into_any_element();
    };
    let view = cx.entity().downgrade();
    let installing = matches!(
        prompt.phase,
        PromptPhase::Installing { .. } | PromptPhase::Starting
    );
    let cancel_view = view.clone();
    let cancel = Button::new("lsp-install-dismiss")
        .label(if installing { "Cancel" } else { "Close" })
        .ghost()
        .disabled(installing)
        .on_click(move |_, _, cx| {
            if let Some(view) = cancel_view.upgrade() {
                cx.update_entity(&view, |window: &mut DopadWindow, cx| {
                    window.cancel_lsp_prompt(cx);
                });
            }
        });

    let buttons = match &prompt.phase {
        PromptPhase::Missing => {
            let install_view = view.clone();
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .child(cancel)
                .child(
                    Button::new("lsp-install-confirm")
                        .label("Install")
                        .primary()
                        .on_click(move |_, _, cx| {
                            if let Some(view) = install_view.upgrade() {
                                cx.update_entity(&view, |window: &mut DopadWindow, cx| {
                                    window.install_lsp_prompt(cx);
                                });
                            }
                        }),
                )
                .into_any_element()
        }
        PromptPhase::Installing { .. } | PromptPhase::Starting => div().into_any_element(),
        PromptPhase::Manual => {
            let guide_view = view.clone();
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .child(cancel)
                .child(
                    Button::new("lsp-open-install-guide")
                        .label("Open Install Guide")
                        .primary()
                        .on_click(move |_, _, cx| {
                            if let Some(view) = guide_view.upgrade() {
                                view.read(cx).open_lsp_install_guide(cx);
                            }
                        }),
                )
                .into_any_element()
        }
        PromptPhase::Error(_) => {
            let retry_view = view.clone();
            let logs_view = view.clone();
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .child(cancel)
                .child(
                    Button::new("lsp-install-view-logs")
                        .label("View Logs")
                        .ghost()
                        .on_click(move |_, _, cx| {
                            if let Some(view) = logs_view.upgrade() {
                                cx.update_entity(&view, |window: &mut DopadWindow, cx| {
                                    window.show_lsp_output(cx);
                                });
                            }
                        }),
                )
                .child(
                    Button::new("lsp-install-retry")
                        .label("Retry")
                        .primary()
                        .on_click(move |_, _, cx| {
                            if let Some(view) = retry_view.upgrade() {
                                cx.update_entity(&view, |window: &mut DopadWindow, cx| {
                                    window.retry_lsp_prompt(cx);
                                });
                            }
                        }),
                )
                .into_any_element()
        }
    };

    let details = phase_details(&prompt, palette.secondary, palette.accent);
    div()
        .absolute()
        .right(px(16.))
        .bottom(px(42.))
        .w(px(370.))
        .p(px(14.))
        .flex()
        .flex_col()
        .gap(px(9.))
        .bg(rgb(palette.canvas))
        .border_1()
        .border_color(rgb(palette.border))
        .rounded(px(2.))
        .text_color(rgb(palette.text))
        .child(div().text_size(px(14.)).child("Language Server Required"))
        .child(
            div()
                .text_size(px(12.))
                .text_color(rgb(palette.secondary))
                .child(format!(
                    "{} autocomplete requires {}.",
                    prompt.spec.language, prompt.spec.name
                )),
        )
        .children(details)
        .child(buttons)
        .into_any_element()
}

fn phase_details(prompt: &InstallPrompt, secondary: u32, accent: u32) -> Vec<gpui_kit::AnyElement> {
    match &prompt.phase {
        PromptPhase::Missing => {
            let method = match prompt.spec.method {
                super::registry::InstallMethod::Rustup => "This uses the installed Rust toolchain.",
                super::registry::InstallMethod::Npm => {
                    "This installs the server with npm at user scope."
                }
                super::registry::InstallMethod::PythonPip => {
                    "This installs the server with pip at user scope."
                }
                super::registry::InstallMethod::GoInstall => {
                    "This installs the server using the installed Go toolchain."
                }
                super::registry::InstallMethod::Manual => "Install this language server manually.",
            };
            vec![
                div()
                    .text_size(px(12.))
                    .child(format!("{method} Continue?"))
                    .into_any_element(),
            ]
        }
        PromptPhase::Installing { message, progress } => {
            let bar = match progress {
                Some(fraction) => Progress::new("lsp-install-progress")
                    .accessibility_label(format!("Installing {}", prompt.spec.name))
                    .color(rgb(accent))
                    .value(fraction * 100.)
                    .w_full(),
                None => Progress::new("lsp-install-progress")
                    .accessibility_label(format!("Installing {}", prompt.spec.name))
                    .color(rgb(accent))
                    .loading(true)
                    .w_full(),
            };
            vec![
                bar.into_any_element(),
                div()
                    .text_size(px(12.))
                    .text_color(rgb(secondary))
                    .child(message.clone())
                    .into_any_element(),
            ]
        }
        PromptPhase::Starting => vec![
            Progress::new("lsp-install-progress")
                .accessibility_label(format!("Starting {}", prompt.spec.name))
                .color(rgb(accent))
                .loading(true)
                .w_full()
                .into_any_element(),
            div()
                .text_size(px(12.))
                .text_color(rgb(secondary))
                .child(format!("Starting and verifying {}…", prompt.spec.name))
                .into_any_element(),
        ],
        PromptPhase::Manual => vec![
            div()
                .text_size(px(12.))
                .child("Dopad will not run an unverified install command for this server.")
                .into_any_element(),
            div()
                .text_size(px(12.))
                .child("Open the official installation guide, then reopen a matching file.")
                .into_any_element(),
        ],
        PromptPhase::Error(message) => vec![
            div()
                .text_size(px(12.))
                .child("The language server could not be verified or started.")
                .into_any_element(),
            div()
                .text_size(px(11.))
                .child(message.clone())
                .into_any_element(),
        ],
    }
}
