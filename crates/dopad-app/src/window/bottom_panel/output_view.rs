use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Anchor, Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, div, px, rgb,
};

use super::{BottomPanelView, view_selector};
use crate::{output::OutputChannel, theme::Palette, window::DopadWindow};

const HEADER_HEIGHT: f32 = 38.;
const LOG_ROW_HEIGHT: f32 = 22.;

pub(super) fn render(
    window: &mut DopadWindow,
    palette: Palette,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    let entries = window.bottom_panel.output.entries.clone();
    let selected = window.bottom_panel.output.selected_entry;
    let scroll = window.bottom_panel.output.scroll.clone();
    let channel = window
        .bottom_panel
        .output
        .channel
        .unwrap_or(OutputChannel::General);
    let parent = cx.entity().downgrade();
    let channel_button = Button::new("output-channel-selector")
        .child(channel.label())
        .icon(IconName::ChevronDown)
        .xsmall()
        .compact()
        .ghost()
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |mut menu, _, _| {
            for channel in OutputChannel::ALL {
                let parent = parent.clone();
                menu = menu.item(
                    PopupMenuItem::new(channel.label()).on_click(move |_, _, cx| {
                        let _ = parent
                            .update(cx, |window, cx| window.select_output_channel(channel, cx));
                    }),
                );
            }
            menu
        });
    let copy = Button::new("output-copy-selected")
        .icon(IconName::Copy)
        .xsmall()
        .compact()
        .ghost()
        .tooltip("Copy selected log entry")
        .on_click(cx.listener(|this, _, _, cx| this.copy_selected_output(cx)));
    let clear = Button::new("output-clear-channel")
        .icon(IconName::Delete)
        .xsmall()
        .compact()
        .ghost()
        .tooltip("Clear current channel")
        .on_click(cx.listener(|this, _, _, cx| this.clear_output_channel(cx)));
    let scroll_end = Button::new("output-scroll-to-end")
        .icon(IconName::ArrowDown)
        .xsmall()
        .compact()
        .ghost()
        .tooltip("Scroll to end")
        .on_click(cx.listener(|this, _, _, cx| {
            let state = &mut this.bottom_panel.output;
            state.auto_follow = true;
            if !state.entries.is_empty() {
                state
                    .scroll
                    .scroll_to_item(state.entries.len() - 1, gpui_kit::ScrollStrategy::Bottom);
            }
            cx.notify();
        }));
    let selector = view_selector(
        cx.entity().downgrade(),
        BottomPanelView::Output,
        palette,
        "output-view-selector",
    );
    let close = Button::new("output-close-panel")
        .icon(IconName::X)
        .xsmall()
        .compact()
        .ghost()
        .tooltip("Close Bottom Panel")
        .on_click(cx.listener(|this, _, window, cx| this.hide_bottom_panel(window, cx)));

    let header = div()
        .w_full()
        .h(px(HEADER_HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_2()
        .px(px(10.))
        .bg(rgb(palette.chrome))
        .border_b_1()
        .border_color(rgb(palette.border))
        .child(selector)
        .child(channel_button)
        .child(div().flex_1().min_w_0())
        .child(copy)
        .child(clear)
        .child(scroll_end)
        .child(close);

    let list_entries = entries.clone();
    let list = gpui_kit::uniform_list(
        "dopad-output-log-lines",
        entries.len(),
        cx.processor(move |_this, range: std::ops::Range<usize>, _, cx| {
            range
                .filter_map(|index| list_entries.get(index))
                .map(|entry| {
                    let sequence = entry.sequence;
                    let line = entry.line();
                    let is_selected = selected == Some(sequence);
                    div()
                        .id(format!("output-line-{sequence}"))
                        .w_full()
                        .h(px(LOG_ROW_HEIGHT))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .text_size(px(11.))
                        .font_family("monospace")
                        .text_color(rgb(palette.text))
                        .when(is_selected, |row| row.bg(rgb(palette.selection)))
                        .when(!is_selected, |row| {
                            row.hover(|style| style.bg(rgb(palette.tab_hover)))
                        })
                        .child(div().w_full().min_w_0().text_ellipsis().child(line))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.bottom_panel.output.selected_entry = Some(sequence);
                            cx.notify();
                        }))
                        .into_any_element()
                })
                .collect::<Vec<_>>()
        }),
    )
    .h_full()
    .w_full()
    .track_scroll(&scroll)
    .on_scroll(cx.listener(|this, _, _, _| {
        this.bottom_panel.output.auto_follow = this
            .bottom_panel
            .output
            .scroll
            .is_scrolled_to_end()
            .unwrap_or(true);
    }));

    let body = if entries.is_empty() {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(palette.secondary))
            .child("No output yet")
            .into_any_element()
    } else {
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(list)
            .into_any_element()
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(rgb(palette.canvas))
        .children([header.into_any_element(), body])
        .into_any_element()
}

