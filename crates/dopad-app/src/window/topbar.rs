use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon, Sizable, Size,
    button::{Button, ButtonCustomVariant, ButtonVariants},
};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, FontWeight, InteractiveElement, IntoElement,
    ParentElement, ScrollHandle, StatefulInteractiveElement, Styled, TestSupportExt, Window,
    WindowControlArea, div, img, px, rgb,
};

use super::DopadWindow;
use crate::theme::Palette;

#[path = "topbar/file_icon.rs"]
mod file_icon;
#[path = "topbar/hover_scroll.rs"]
pub(in crate::window) mod hover_scroll;
#[path = "topbar/scroll.rs"]
mod scroll;
#[path = "topbar/tab_context.rs"]
mod tab_context;
#[path = "topbar/tab_drag.rs"]
mod tab_drag;
use file_icon::tab_file_icon;
#[path = "topbar/tab_visual.rs"]
mod tab_visual;
use scroll::reveal_active_tab;
pub(in crate::window) use scroll::reveal_measured_tab;
use tab_visual::{TabIndicator, tab_hover_background, tab_indicator, tab_padding};

const TAB_INDICATOR_WIDTH: f32 = 20.;
const ACTIVE_TAB_TOP_GAP: f32 = 5.;
const TAB_HEIGHT: f32 = 39.;
const TAB_CONTENT_VERTICAL_OFFSET: f32 = -1.;
const CONTROL_SLOT_WIDTH: f32 = 34.;
const CONTROL_SLOT_HEIGHT: f32 = 44.;
const CLOSE_ICON_HOVER: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none"><path d="M18 6 6 18M6 6l12 12" stroke="currentColor" stroke-width="2.9" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;

pub(super) fn render(
    window: &DopadWindow,
    ui_window: &mut Window,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) -> impl IntoElement + use<> {
    let compact_tabs = window.settings.current().compact_tabs;
    let tabs_scroll = ui_window
        .use_keyed_state("dopad-tabs-scroll", cx, |_, _| ScrollHandle::new())
        .read(cx)
        .clone();
    let hover_state = ui_window.use_keyed_state("dopad-tabs-hover-scroll", cx, |_, _| {
        hover_scroll::HoverScrollState::default()
    });
    let view = cx.entity();
    let resume_scheduled =
        ui_window.use_keyed_state("dopad-close-batch-resume-scheduled", cx, |_, _| false);
    if window.close_batch_resume && !*resume_scheduled.read(cx) {
        resume_scheduled.update(cx, |scheduled, cx| {
            *scheduled = true;
            cx.notify();
        });
        let resume_scheduled = resume_scheduled.clone();
        let close_view = view.clone();
        ui_window.on_next_frame(move |window, cx| {
            resume_scheduled.update(cx, |scheduled, _| *scheduled = false);
            close_view.update(cx, |state, cx| {
                state.close_batch_resume = false;
                state.continue_close_batch(window, cx);
            });
        });
    }
    reveal_active_tab(ui_window, cx, window.active, &tabs_scroll, &hover_state);

    let new_hover_state = hover_state.clone();
    let new_document = Button::new("new-document")
        .icon(IconName::Plus)
        .compact()
        .ghost()
        .tooltip("New document")
        .accessibility_label("New document")
        .on_click(cx.listener(move |this, _, _, cx| {
            hover_scroll::cancel(&new_hover_state, cx);
            this.add_blank(cx);
        }));
    let (icon_theme, icon_cache) = super::sidebar::file_icon_resources(window);
    let tab_ids = std::sync::Arc::new(window.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>());
    let tabs = window
        .tabs
        .iter()
        .enumerate()
        .map(|(index, document)| {
            let active = index == window.active;
            let dirty = document.is_dirty();
            let tab_hover_state = ui_window.use_keyed_state(
                format!("dopad-tab-hover-{}", document.id),
                cx,
                |_, _| false,
            );
            let tab_hovered = *tab_hover_state.read(cx);
            let indicator = tab_indicator(active, dirty, tab_hovered);
            let close_hover_state = ui_window.use_keyed_state(
                format!("dopad-tab-close-hover-{}", document.id),
                cx,
                |_, _| false,
            );
            let close_hovered = *close_hover_state.read(cx);
            let tab_icon = tab_file_icon(
                &document.title,
                icon_theme.as_deref(),
                icon_cache.as_ref(),
                palette,
            );
            let title = div()
                .id(format!("tab-title-{}", document.id))
                .relative()
                .top(px(TAB_CONTENT_VERTICAL_OFFSET))
                .min_w_0()
                .max_w(px(106.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_1()
                .text_size(px(13.))
                .font_weight(if active {
                    FontWeight::BOLD
                } else {
                    FontWeight::SEMIBOLD
                })
                .text_color(rgb(if active {
                    palette.tab_active_text
                } else {
                    palette.text
                }))
                .child(tab_icon)
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_ellipsis()
                        .child(document.title.clone()),
                );
            let close_select_hover_state = hover_state.clone();
            let indicator_content: AnyElement = match indicator {
                TabIndicator::Close => {
                    let close_button_hover_state = close_hover_state.clone();
                    let close_icon: AnyElement = if close_hovered {
                        Icon::default()
                            .data(CLOSE_ICON_HOVER)
                            .with_size(Size::Size(px(18.)))
                            .text_color(rgb(palette.tab_active_text))
                            .into_any_element()
                    } else {
                        Icon::new(IconName::X)
                            .with_size(Size::Size(px(18.)))
                            .text_color(rgb(palette.secondary))
                            .into_any_element()
                    };
                    Button::new(format!("close-tab-{}", document.id))
                        .child(close_icon)
                        .compact()
                        .custom(
                            ButtonCustomVariant::new(cx)
                                .color(rgb(palette.canvas).opacity(0.).into())
                                .foreground(rgb(palette.secondary).into())
                                .hover(rgb(palette.canvas).opacity(0.).into())
                                .active(rgb(palette.canvas).opacity(0.).into()),
                        )
                        .size(px(TAB_INDICATOR_WIDTH))
                        .p_0()
                        .accessibility_label(format!("Close {}", document.title))
                        .on_hover(move |hovered, _, cx| {
                            close_button_hover_state.update(cx, |state, cx| {
                                if *state != *hovered {
                                    *state = *hovered;
                                    cx.notify();
                                }
                            });
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            hover_scroll::cancel(&close_select_hover_state, cx);
                            this.activate_tab(index, cx);
                            this.request_close(window, cx);
                        }))
                        .into_any_element()
                }
                TabIndicator::Dirty => div()
                    .id(format!("tab-dirty-indicator-{}", document.id))
                    .size(px(8.))
                    .rounded_full()
                    .bg(rgb(palette.tab_dirty))
                    .into_any_element(),
                TabIndicator::Empty => div().size(px(TAB_INDICATOR_WIDTH)).into_any_element(),
            };
            let indicator_slot = div()
                .id(format!("tab-indicator-slot-{}", document.id))
                .relative()
                .top(px(TAB_CONTENT_VERTICAL_OFFSET))
                .size(px(TAB_INDICATOR_WIDTH))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(indicator_content);

            let tab_select_hover_state = hover_state.clone();
            let (padding_left, padding_right) = tab_padding(compact_tabs);
            let mut tab = div()
                .id(format!("dopad-tab-{}", document.id))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_1()
                .h(px(TAB_HEIGHT))
                .mt(px(ACTIVE_TAB_TOP_GAP))
                .max_w(px(170.))
                .pl(px(padding_left))
                .pr(px(padding_right))
                .rounded_tl(px(8.))
                .rounded_tr(px(8.))
                .child(title)
                .child(indicator_slot)
                .on_click(cx.listener(move |this, _, _, cx| {
                    hover_scroll::cancel(&tab_select_hover_state, cx);
                    this.activate_tab(index, cx);
                }));
            if active {
                tab = tab.bg(rgb(palette.canvas));
            } else if let Some(hover_background) = tab_hover_background(active, palette) {
                tab = tab.hover(|style| style.bg(rgb(hover_background)));
            }
            let tab_hover_state = tab_hover_state.clone();
            tab = tab.on_hover(move |hovered, _, cx| {
                tab_hover_state.update(cx, |state, cx| {
                    if *state != *hovered {
                        *state = *hovered;
                        cx.notify();
                    }
                });
            });
            let drag_payload = tab_drag::DraggedTab::new(
                document.id,
                document.title.clone(),
                palette.canvas,
                palette.text,
                palette.border,
            );
            tab = tab.on_drag(drag_payload, |payload, position, _, cx| {
                cx.new(|_| payload.preview(position))
            });
            let reorder_view = view.clone();
            let target_id = document.id;
            tab = tab.on_drop(move |payload: &tab_drag::DraggedTab, _, cx| {
                reorder_view.update(cx, |window, cx| {
                    if tab_drag::reorder_tabs(
                        &mut window.tabs,
                        &mut window.active,
                        payload.document_id,
                        target_id,
                        |document| document.id,
                    ) {
                        cx.notify();
                    }
                });
            });
            tab_context::attach(tab, view.clone(), document.id, index, tab_ids.clone())
                .into_any_element()
        })
        .collect::<Vec<_>>();

    let tab_list = render_tab_list(tabs, hover_state, tabs_scroll, view);

    let brand = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_2()
        .child(img(crate::icon::image()).size(px(20.)));

    let menu = super::menu::render(window, cx);
    let maximize_icon = if ui_window.is_maximized() {
        IconName::WindowRestore
    } else {
        IconName::WindowMaximize
    };
    let right_controls = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_0()
        .child(fixed_control("new-document", new_document, None))
        .child(fixed_control("menu", menu, None))
        .child(window_control(
            "window-minimize",
            IconName::WindowMinimize,
            "Minimize window",
            WindowControlArea::Min,
            WindowAction::Minimize,
        ))
        .child(window_control(
            "window-maximize",
            maximize_icon,
            "Maximize or restore window",
            WindowControlArea::Max,
            WindowAction::ToggleMaximize,
        ))
        .child(window_control(
            "window-close",
            IconName::WindowClose,
            "Close window",
            WindowControlArea::Close,
            WindowAction::Close,
        ));

    content_layout(brand, tab_list, right_controls, palette.chrome)
}

fn render_tab_list<V: 'static>(
    tabs: Vec<AnyElement>,
    hover_state: Entity<hover_scroll::HoverScrollState>,
    tabs_scroll: ScrollHandle,
    view: Entity<V>,
) -> impl IntoElement {
    let move_state = hover_state.clone();
    let move_scroll = tabs_scroll.clone();
    let move_view = view.clone();
    let tab_viewport = div()
        .id("dopad-tab-viewport")
        .debug_selector(|| "dopad-tab-viewport".into())
        .test_support()
        .flex_1()
        .min_w_0()
        .h(px(44.))
        .relative()
        .flex()
        .items_center()
        .overflow_x_hidden()
        .on_mouse_move(move |event, window, cx| {
            if let Some(generation) = move_state.update(cx, |state, _| {
                state.pointer_moved(event.position, &move_scroll)
            }) {
                hover_scroll::schedule_frame(
                    window,
                    move_state.clone(),
                    move_scroll.clone(),
                    move_view.clone(),
                    generation,
                );
            }
        })
        .on_mouse_exit({
            let state = hover_state.clone();
            move |_, _, cx| hover_scroll::cancel(&state, cx)
        });
    let tab_row = div()
        .id("dopad-tabs-scroll")
        .h(px(44.))
        .flex()
        .flex_1()
        .min_w_0()
        .items_end()
        .gap_0()
        .overflow_x_scroll()
        .track_scroll(&tabs_scroll)
        .children(tabs);
    let tab_viewport = tab_viewport.child(tab_row);

    div()
        .id("dopad-tab-list")
        .test_support()
        .flex_1()
        .min_w_0()
        .h(px(44.))
        .relative()
        .flex()
        .items_center()
        .overflow_x_hidden()
        .child(tab_viewport)
}

fn content_layout(
    brand: impl IntoElement,
    tab_list: impl IntoElement,
    actions: impl IntoElement,
    chrome: u32,
) -> impl IntoElement {
    div()
        .id("dopad-topbar-content")
        .debug_selector(|| "dopad-topbar-content".into())
        .test_support()
        .h(px(44.))
        .flex_shrink_0()
        .min_w_0()
        .flex()
        .items_center()
        .gap_4()
        .px_3()
        .bg(rgb(chrome))
        .child(
            div()
                .id("dopad-topbar-left")
                .debug_selector(|| "dopad-topbar-left".into())
                .test_support()
                .flex_shrink_0()
                .window_control_area(WindowControlArea::Drag)
                .child(brand),
        )
        .child(
            div()
                .id("dopad-topbar-center")
                .debug_selector(|| "dopad-topbar-center".into())
                .test_support()
                .flex_1()
                .min_w_0()
                .overflow_x_hidden()
                .child(tab_list),
        )
        .child(
            div()
                .id("dopad-topbar-right")
                .debug_selector(|| "dopad-topbar-right".into())
                .test_support()
                .flex_shrink_0()
                .child(actions),
        )
}

fn fixed_control(
    id: &'static str,
    button: impl IntoElement,
    area: Option<WindowControlArea>,
) -> impl IntoElement {
    let selector = format!("dopad-topbar-control-{id}");
    let control = div()
        .id(selector.clone())
        .debug_selector(move || selector.clone())
        .test_support()
        .w(px(CONTROL_SLOT_WIDTH))
        .h(px(CONTROL_SLOT_HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(button);
    if cfg!(target_os = "windows") {
        if let Some(area) = area {
            control.window_control_area(area)
        } else {
            control
        }
    } else {
        control
    }
}

#[derive(Clone, Copy)]
enum WindowAction {
    Minimize,
    ToggleMaximize,
    Close,
}

fn window_control(
    id: &'static str,
    icon: IconName,
    tooltip: &'static str,
    area: WindowControlArea,
    action: WindowAction,
) -> impl IntoElement {
    let mut button = Button::new(id)
        .icon(icon)
        .compact()
        .ghost()
        .tooltip(tooltip)
        .accessibility_label(tooltip);
    button = button.on_click(move |_, window, _| match action {
        WindowAction::Minimize => window.minimize_window(),
        WindowAction::ToggleMaximize => window.zoom_window(),
        WindowAction::Close => window.remove_window(),
    });
    fixed_control(id, button, Some(area))
}

#[cfg(test)]
#[path = "topbar/tests.rs"]
mod tests;
