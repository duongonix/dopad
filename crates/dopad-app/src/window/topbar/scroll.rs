use gpui_kit::{Context, Entity, Pixels, ScrollHandle, Window, point, px};

use super::hover_scroll;

pub(super) fn reveal_active_tab<V: 'static>(
    window: &mut Window,
    cx: &mut Context<V>,
    active: usize,
    tabs_scroll: &ScrollHandle,
    hover_state: &Entity<hover_scroll::HoverScrollState>,
) {
    let layout_state =
        window.use_keyed_state("dopad-tabs-layout", cx, |_, _| None::<(usize, Pixels, f32)>);
    let layout = (active, window.bounds().size.width, window.scale_factor());
    let previous = *layout_state.read(cx);
    if previous.is_some_and(|previous| previous != layout) {
        hover_scroll::cancel(hover_state, cx);
    }
    let active_changed = previous.is_none_or(|(previous_active, _, _)| previous_active != active);
    if active_changed {
        // The scroll handle defers item reveal until the tab row has measured the
        // current children during this frame.
        tabs_scroll.scroll_to_item(active);
    }
    let viewport_changed = previous.is_some_and(|(_, previous_width, previous_scale)| {
        previous_width != layout.1 || previous_scale != layout.2
    });
    if active_changed || viewport_changed {
        let tabs_scroll = tabs_scroll.clone();
        let view = window.current_view();
        window.on_next_frame(move |_, cx| {
            reveal_measured_tab(&tabs_scroll, active);
            cx.notify(view);
        });
    }
    if previous != Some(layout) {
        layout_state.update(cx, |previous, _| *previous = Some(layout));
    }
}

pub(in crate::window) fn reveal_measured_tab(scroll: &ScrollHandle, index: usize) {
    let Some(tab) = scroll.bounds_for_item(index) else {
        scroll.scroll_to_item(index);
        return;
    };
    let viewport = scroll.bounds();
    let current = scroll.offset();
    let mut x = current.x;
    if tab.size.width > viewport.size.width || tab.left() + x < viewport.left() {
        x = viewport.left() - tab.left();
    } else if tab.right() + x > viewport.right() {
        x = viewport.right() - tab.right();
    }
    x = x.min(px(0.)).max(-scroll.max_offset().x);
    scroll.set_offset(point(x, current.y));
}
