use gpui_kit::StatefulInteractiveElement;
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::{
    Context, FontWeight, InteractiveElement, IntoElement, ParentElement, Styled, div, px, rgb,
};

use super::{
    theme,
    window::{Page, SettingsWindow},
};

pub(super) fn render(this: &SettingsWindow, cx: &mut Context<SettingsWindow>) -> impl IntoElement {
    let pages = [
        (Page::General, "General", IconName::Settings),
        (Page::Editor, "Editor", IconName::Pencil),
        (Page::Terminal, "Terminal", IconName::Terminal),
        (Page::Appearance, "Appearance", IconName::Palette),
        (Page::Files, "Files", IconName::Folder),
        (Page::Encoding, "Encoding", IconName::Code),
        (Page::Keyboard, "Keyboard", IconName::Keyboard),
        (Page::Advanced, "Advanced", IconName::SlidersHorizontal),
    ];

    let mut items = div().flex().flex_col().gap_1();
    for (page, label, icon) in pages {
        let active = this.page == page;
        let entity = cx.entity();
        let mut item = div()
            .id(format!("settings-sidebar-{label}"))
            .h(px(45.))
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .px(px(18.))
            .rounded_md()
            .cursor_pointer()
            .on_click(move |_, _, app| {
                entity.update(app, |this, cx| {
                    this.page = page;
                    cx.notify();
                });
            });
        item = if active {
            item.bg(rgb(theme::BG_ACTIVE))
        } else {
            item.hover(|style| style.bg(rgb(theme::BG_HOVER)))
        };
        let color = if active {
            theme::TEXT_ACTIVE
        } else {
            theme::TEXT_PRIMARY
        };
        let weight = if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        };
        items = items.child(
            item.child(Icon::new(icon).size(px(20.)).text_color(rgb(color)))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(weight)
                        .text_color(rgb(color))
                        .child(label),
                ),
        );
    }

    div()
        .w(px(220.))
        .h_full()
        .flex_shrink_0()
        .border_r_1()
        .border_color(rgb(theme::BORDER))
        .bg(rgb(theme::BG_SIDEBAR))
        .p_3()
        .child(items)
}
