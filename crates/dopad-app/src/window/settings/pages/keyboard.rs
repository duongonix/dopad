use gpui_kit::component::{button::Button, input::Input};
use gpui_kit::{Context, IntoElement, ParentElement, Styled, div, px, rgb};

use crate::window::shortcuts;

use super::super::{
    components::{kit_select, row, section},
    theme,
    window::SettingsWindow,
};

pub(in crate::window::settings) fn render(
    this: &SettingsWindow,
    cx: &mut Context<SettingsWindow>,
) -> impl IntoElement {
    let query = this
        .keyboard_search
        .read(cx)
        .value()
        .trim()
        .to_ascii_lowercase();
    let mut table = div().w_full().flex().flex_col();
    let mut visible_count = 0;

    for spec in shortcuts::specs() {
        let current_key = shortcuts::effective_key(&this.draft, spec.id);
        let current_label = shortcuts::display_key(&current_key);
        if !query.is_empty()
            && !spec.label.to_ascii_lowercase().contains(&query)
            && !current_label.to_ascii_lowercase().contains(&query)
        {
            continue;
        }

        let Some(select_state) = this.selects.keyboard.get(spec.id) else {
            continue;
        };
        visible_count += 1;
        table = table.child(row(
            spec.label,
            kit_select(
                spec.id,
                format!("{} shortcut", spec.label),
                "Search shortcuts...",
                select_state,
                220.,
            ),
        ));
    }

    let entity = cx.entity();
    let reset = Button::new("reset-shortcuts")
        .label("Reset to default")
        .outline()
        .h(px(34.))
        .on_click(move |_, window, app| {
            entity.update(app, |this, cx| {
                this.draft.shortcut_overrides.clear();
                this.sync_keyboard_selects(window, cx);
                this.keyboard_notice = None;
                this.error = None;
                cx.notify();
            });
        });

    let entity = cx.entity();
    let detect = Button::new("detect-shortcut-conflicts")
        .label("Detect conflicts")
        .outline()
        .h(px(34.))
        .on_click(move |_, _, app| {
            entity.update(app, |this, cx| {
                let conflicts = shortcuts::conflicts(&this.draft);
                this.keyboard_notice = Some(if conflicts.is_empty() {
                    "No shortcut conflicts detected.".into()
                } else {
                    format!(
                        "Conflicts: {}",
                        conflicts
                            .iter()
                            .map(|conflict| {
                                let (key, actions) =
                                    conflict.split_once(": ").unwrap_or((conflict, ""));
                                format!("{}: {actions}", shortcuts::display_key(key))
                            })
                            .collect::<Vec<_>>()
                            .join("; ")
                    )
                });
                this.error = None;
                cx.notify();
            });
        });

    let mut customization = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .child(Input::new(&this.keyboard_search))
        .child(
            div()
                .w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(reset)
                .child(detect),
        );
    if let Some(notice) = &this.keyboard_notice {
        customization = customization.child(
            div()
                .text_size(px(13.))
                .text_color(rgb(theme::ACCENT))
                .child(notice.clone()),
        );
    }

    let assignments = if visible_count == 0 {
        div()
            .w_full()
            .py(px(12.))
            .text_size(px(13.))
            .text_color(rgb(theme::TEXT_SECONDARY))
            .child("No shortcuts match this search.")
    } else {
        table
    };

    div()
        .flex()
        .flex_col()
        .child(section("Customization", customization))
        .child(section("Shortcuts", assignments))
}
