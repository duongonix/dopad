use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon, Sizable, Size,
    button::{Button, ButtonVariants},
};
use gpui_kit::{AnyElement, Context, IntoElement, ParentElement, Styled, div, px, rgb};

use crate::{
    theme::Palette,
    window::{DopadWindow, file_view::TabContent},
};

pub(super) fn render_file_state(
    title: &str,
    content: &TabContent,
    loading_visible: bool,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) -> AnyElement {
    let mut body = div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .px(px(28.))
        .bg(rgb(palette.canvas))
        .text_color(rgb(palette.text));
    match content {
        TabContent::Loading => {
            if !loading_visible {
                return div().size_full().bg(rgb(palette.canvas)).into_any_element();
            }
            body = body
                .child(Icon::new(IconName::File).with_size(Size::Size(px(42.))))
                .child(div().text_size(px(17.)).child("Opening file…"))
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(palette.secondary))
                        .child(title.to_owned()),
                );
        }
        TabContent::Unsupported(info) => {
            body = body
                .child(
                    Icon::new(IconName::FileQuestionMark)
                        .with_size(Size::Size(px(44.)))
                        .text_color(rgb(palette.secondary)),
                )
                .child(
                    div()
                        .text_size(px(17.))
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child("Cannot preview this file"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(palette.secondary))
                        .child("Dopad doesn't support previewing this file type."),
                )
                .child(file_details(
                    title,
                    &info.file_type,
                    Some(info.file_size),
                    palette,
                ));
            if info.can_open_externally {
                body = body.child(
                    Button::new("open-file-externally")
                        .label("Open Externally")
                        .primary()
                        .on_click(
                            cx.listener(|this, _, _, cx| this.open_active_file_externally(cx)),
                        ),
                );
            } else {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(rgb(palette.muted))
                        .child("External opening is disabled for executable or script files."),
                );
            }
            if let Some(error) = &info.external_error {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(rgb(palette.secondary))
                        .child(error.clone()),
                );
            }
        }
        TabContent::Error(error) => {
            body = body
                .child(
                    Icon::new(IconName::FileQuestionMark)
                        .with_size(Size::Size(px(42.)))
                        .text_color(rgb(palette.secondary)),
                )
                .child(
                    div()
                        .text_size(px(17.))
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child("Unable to open file"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(palette.secondary))
                        .child(error.message.clone()),
                )
                .child(file_details(
                    title,
                    &error.file_type,
                    error.file_size,
                    palette,
                ))
                .child(
                    Button::new("retry-open-file")
                        .label("Retry")
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| this.retry_active_file(cx))),
                );
        }
        _ => return div().size_full().bg(rgb(palette.canvas)).into_any_element(),
    }
    body.into_any_element()
}

fn file_details(
    filename: &str,
    file_type: &str,
    size: Option<u64>,
    palette: &Palette,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap_1()
        .mt(px(4.))
        .text_xs()
        .text_color(rgb(palette.secondary))
        .child(format!("Filename: {filename}"))
        .child(format!("File type: {file_type}"))
        .child(format!(
            "Size: {}",
            size.map(super::file_view::format_file_size)
                .unwrap_or_else(|| "Unknown".to_owned())
        ))
        .into_any_element()
}
