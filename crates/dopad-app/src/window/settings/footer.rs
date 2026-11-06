use gpui_kit::component::{
    Disableable,
    button::{Button, ButtonVariants},
};
use gpui_kit::{Context, IntoElement, ParentElement, Styled, div, px, rgb};

use super::{theme, window::SettingsWindow};

pub(super) fn render(this: &SettingsWindow, cx: &mut Context<SettingsWindow>) -> impl IntoElement {
    let dirty = this.draft != this.saved;
    let saving = this.saving;
    let apply = Button::new("settings-apply")
        .label("Apply")
        .primary()
        .text_color(rgb(0xFFFFFF))
        .disabled(!dirty || saving)
        .h(px(36.))
        .w(px(112.))
        .on_click(cx.listener(|this, _, window, cx| this.persist(false, window, cx)));
    let ok = Button::new("settings-ok")
        .label("OK")
        .outline()
        .bg(rgb(theme::BG_PRIMARY))
        .border_1()
        .border_color(rgb(theme::INPUT_BORDER))
        .disabled(saving)
        .h(px(36.))
        .w(px(112.))
        .on_click(cx.listener(|this, _, window, cx| this.persist(true, window, cx)));
    let cancel = Button::new("settings-cancel")
        .label("Cancel")
        .outline()
        .bg(rgb(theme::BG_PRIMARY))
        .border_1()
        .border_color(rgb(theme::INPUT_BORDER))
        .disabled(saving)
        .h(px(36.))
        .w(px(112.))
        .on_click(cx.listener(|this, _, window, cx| {
            this.restore_word_wrap_preview(cx);
            window.remove_window();
        }));

    let message = this.error.as_ref().map_or_else(
        || div().flex_1().into_any_element(),
        |error| {
            div()
                .flex_1()
                .text_size(px(12.))
                .text_color(rgb(theme::ACCENT))
                .child(error.clone())
                .into_any_element()
        },
    );
    div()
        .h(px(64.))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .px(px(24.))
        .border_t_1()
        .border_color(rgb(theme::BORDER))
        .bg(rgb(theme::BG_PRIMARY))
        .child(message)
        .child(apply)
        .child(ok)
        .child(cancel)
}
