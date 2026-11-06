use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::{AnyElement, Context, FontWeight, IntoElement, ParentElement, Styled, div, px, rgb};

use super::{
    pages, theme,
    window::{Page, SettingsWindow},
};

pub(super) fn render(this: &SettingsWindow, cx: &mut Context<SettingsWindow>) -> impl IntoElement {
    let (title, description, content): (&str, &str, AnyElement) = match this.page {
        Page::General => (
            "General",
            "Basic settings for Dopad.",
            pages::general(this, cx).into_any_element(),
        ),
        Page::Editor => (
            "Editor",
            "Control text editing behavior.",
            pages::editor(this, cx).into_any_element(),
        ),
        Page::Terminal => (
            "Terminal",
            "Configure terminal typography and cell dimensions.",
            pages::terminal(this, cx).into_any_element(),
        ),
        Page::Appearance => (
            "Appearance",
            "Adjust how Dopad looks.",
            pages::appearance(this, cx).into_any_element(),
        ),
        Page::Files => (
            "Files",
            "Configure file handling.",
            pages::files(this, cx).into_any_element(),
        ),
        Page::Encoding => (
            "Encoding",
            "Configure text encoding and line endings.",
            pages::encoding(this, cx).into_any_element(),
        ),
        Page::Keyboard => (
            "Keyboard",
            "View keyboard shortcuts.",
            pages::keyboard(this, cx).into_any_element(),
        ),
        Page::Advanced => (
            "Advanced",
            "Advanced editor options.",
            pages::advanced(this, cx).into_any_element(),
        ),
    };

    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scrollbar()
        .px(px(32.))
        .pt(px(26.))
        .pb(px(24.))
        .bg(rgb(theme::BG_PRIMARY))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(26.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::TEXT_PRIMARY))
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(rgb(theme::TEXT_SECONDARY))
                        .child(description),
                ),
        )
        .child(div().mt(px(26.)).child(content))
}
