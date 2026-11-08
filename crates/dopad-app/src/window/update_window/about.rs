use gpui_kit::component::link::Link;
use gpui_kit::{FontWeight, IntoElement, ParentElement, Styled, div, img, px, rgb};

use crate::theme::Palette;
use crate::updater::current_version;

pub(super) fn render(palette: &Palette) -> gpui_kit::AnyElement {
    div()
        .w_full()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w_full()
                .max_w(px(420.))
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .child(img(crate::icon::image()).size(px(52.)))
                .child(
                    div()
                        .text_size(px(22.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(palette.text))
                        .child("Dopad"),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(rgb(palette.secondary))
                        .child(format!("Version v{}", current_version())),
                )
                .child(
                    div()
                        .w_full()
                        .pt(px(14.))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(info_row(
                            "Author:",
                            div()
                                .text_size(px(13.))
                                .text_color(rgb(palette.text))
                                .child("DuongOnix"),
                            palette,
                        ))
                        .child(info_row(
                            "Facebook:",
                            Link::new("update-app-facebook")
                                .href("https://fb.com/duongonix")
                                .text_color(rgb(palette.accent))
                                .child("https://fb.com/duongonix"),
                            palette,
                        ))
                        .child(info_row(
                            "Email:",
                            Link::new("update-app-email")
                                .href("mailto:duongonixx@gmail.com")
                                .text_color(rgb(palette.accent))
                                .child("duongonixx@gmail.com"),
                            palette,
                        )),
                ),
        )
        .into_any_element()
}

fn info_row(label: &'static str, value: impl IntoElement, palette: &Palette) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .w(px(82.))
                .flex_shrink_0()
                .text_size(px(13.))
                .text_color(rgb(palette.secondary))
                .child(label),
        )
        .child(div().min_w_0().child(value))
}
