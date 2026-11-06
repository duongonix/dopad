use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, IntoElement, Modifiers, ParentElement, Render, Styled, TestAppContext,
    Window, div, point, px, rgb, size,
};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::{InteractiveElement, StatefulInteractiveElement, TestSupportExt};

use super::{
    ACTIVE_TAB_TOP_GAP, CONTROL_SLOT_HEIGHT, CONTROL_SLOT_WIDTH, IconName, ScrollHandle,
    TAB_HEIGHT, TAB_INDICATOR_WIDTH, TabIndicator, content_layout, fixed_control, hover_scroll,
    render_tab_list, reveal_active_tab, tab_hover_background, tab_indicator, tab_padding,
};

struct LayoutHarness {
    tab_count: usize,
    active: usize,
    new_document_clicks: usize,
    tabs_scroll: ScrollHandle,
}

impl Render for LayoutHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let hover_state = window.use_keyed_state("dopad-tabs-hover-scroll", cx, |_, _| {
            hover_scroll::HoverScrollState::default()
        });
        reveal_active_tab(window, cx, self.active, &self.tabs_scroll, &hover_state);
        let tab_view = cx.entity();
        let tabs = (0..self.tab_count)
            .map(|index| {
                let active = index == self.active;
                let select_view = tab_view.clone();
                let title = div()
                    .id(format!("dopad-test-title-{index}"))
                    .debug_selector(move || {
                        if active {
                            "dopad-test-active-title".into()
                        } else {
                            format!("dopad-test-title-{index}")
                        }
                    })
                    .test_support()
                    .min_w_0()
                    .max_w(px(106.))
                    .flex_shrink_0()
                    .text_ellipsis()
                    .child(format!("Document {index:02} with a long filename.md"));
                let tab_select_hover_state = hover_state.clone();
                div()
                    .id(format!("dopad-test-tab-{index}"))
                    .debug_selector(move || {
                        if active {
                            "dopad-test-active-tab".into()
                        } else {
                            format!("dopad-test-tab-{index}")
                        }
                    })
                    .test_support()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .h(px(TAB_HEIGHT))
                    .mt(px(ACTIVE_TAB_TOP_GAP))
                    .max_w(px(170.))
                    .pl(px(15.))
                    .pr(px(12.))
                    .rounded_tl(px(8.))
                    .rounded_tr(px(8.))
                    .when(active, |tab| tab.bg(rgb(0xFFF8E7)))
                    .when(!active, |tab| tab.hover(|style| style.bg(rgb(0xF7EBD9))))
                    .child(title)
                    .on_click(move |_, _, cx| {
                        hover_scroll::cancel(&tab_select_hover_state, cx);
                        select_view.update(cx, |state, cx| {
                            state.active = index;
                            cx.notify();
                        });
                    })
                    .child(
                        div()
                            .id(format!("dopad-test-tab-indicator-{index}"))
                            .debug_selector(move || {
                                if active {
                                    "dopad-test-active-tab-indicator".into()
                                } else {
                                    format!("dopad-test-tab-indicator-{index}")
                                }
                            })
                            .test_support()
                            .size(px(TAB_INDICATOR_WIDTH))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center(),
                    )
                    .into_any_element()
            })
            .collect::<Vec<AnyElement>>();
        let new_document = div()
            .id("dopad-test-new-document")
            .debug_selector(|| "dopad-test-new-document".into())
            .test_support()
            .child(
                Button::new("layout-test-new-document")
                    .icon(IconName::Plus)
                    .compact()
                    .ghost()
                    .on_click(cx.listener(|state, _, _, cx| {
                        state.new_document_clicks += 1;
                        state.tab_count += 1;
                        state.active = state.tab_count - 1;
                        cx.notify();
                    })),
            );
        let tabs = render_tab_list(tabs, hover_state, self.tabs_scroll.clone(), cx.entity());
        let actions = div()
            .id("dopad-test-topbar-actions")
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_0()
            .child(fixed_control("new-document", new_document, None))
            .child(fixed_control("menu", div().size_full(), None))
            .child(fixed_control("window-minimize", div().size_full(), None))
            .child(fixed_control("window-maximize", div().size_full(), None))
            .child(fixed_control("window-close", div().size_full(), None));
        let content = content_layout(div().w(px(88.)).h(px(44.)), tabs, actions, 0xF2E8CF);

        div().size_full().flex().flex_col().child(content)
    }
}

#[gpui::test]
fn tab_overflow_keeps_fixed_regions_and_reveals_selection_after_resize(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, test_window) = cx.add_window_view(|_, _| LayoutHarness {
        tab_count: 1,
        active: 0,
        new_document_clicks: 0,
        tabs_scroll: ScrollHandle::new(),
    });
    let test_window = &mut *test_window;
    test_window.simulate_resize(size(px(560.), px(180.)));

    let mut fixed_bounds = None;
    for tab_count in [1, 5, 10, 20, 50, 100] {
        view.update(test_window, |state, cx| {
            state.tab_count = tab_count;
            state.active = tab_count - 1;
            cx.notify();
        });
        test_window.run_until_parked();
        for _ in 0..3 {
            test_window.update(|window, cx| {
                window.draw(cx).clear(cx);
                window.simulate_next_frame(cx);
            });
        }

        let (content, left, center, right, window_width) = (
            test_window
                .debug_bounds("dopad-topbar-content")
                .expect("topbar content is measured"),
            test_window
                .debug_bounds("dopad-topbar-left")
                .expect("fixed left region is measured"),
            test_window
                .debug_bounds("dopad-topbar-center")
                .expect("flexible center region is measured"),
            test_window
                .debug_bounds("dopad-topbar-right")
                .expect("fixed right region is measured"),
            test_window.update(|window, _| window.bounds().size.width),
        );
        assert_eq!(content.size.height, px(44.), "topbar must stay 44px tall");
        assert!(left.right() <= center.left());
        assert!(center.right() <= right.left());
        assert_eq!(right.size.width, px(CONTROL_SLOT_WIDTH * 5.));
        let slots = [
            "dopad-topbar-control-new-document",
            "dopad-topbar-control-menu",
            "dopad-topbar-control-window-minimize",
            "dopad-topbar-control-window-maximize",
            "dopad-topbar-control-window-close",
        ]
        .map(|selector| {
            test_window
                .debug_bounds(selector)
                .expect("each fixed topbar action slot is measured")
        });
        for slot in &slots {
            assert_eq!(slot.size.width, px(CONTROL_SLOT_WIDTH));
            assert_eq!(slot.size.height, px(CONTROL_SLOT_HEIGHT));
        }
        for adjacent in slots.windows(2) {
            assert_eq!(adjacent[0].right(), adjacent[1].left());
        }

        if let Some((expected_left, expected_right)) = fixed_bounds {
            assert_eq!(
                left.origin.x, expected_left,
                "left moved with {tab_count} tabs"
            );
            assert_eq!(
                right.origin.x, expected_right,
                "right moved with {tab_count} tabs: window={window_width:?}, content={content:?}, left={left:?}, center={center:?}, right={right:?}"
            );
        } else {
            fixed_bounds = Some((left.origin.x, right.origin.x));
        }

        let scroll = view.read_with(test_window, |state, _| state.tabs_scroll.clone());
        let (rendered_tab_count, rendered_active) =
            view.read_with(test_window, |state, _| (state.tab_count, state.active));
        let active_tab = scroll
            .bounds_for_item(rendered_active)
            .expect("active tab bounds are measured");
        let active_tab_layout = test_window
            .debug_bounds("dopad-test-active-tab")
            .expect("active tab layout is measured");
        let active_title = test_window
            .debug_bounds("dopad-test-active-title")
            .expect("active tab title is measured");
        let active_indicator = test_window
            .debug_bounds("dopad-test-active-tab-indicator")
            .expect("active tab indicator slot is measured");
        let tab_viewport = test_window
            .debug_bounds("dopad-tab-viewport")
            .expect("tab viewport is measured");
        assert_eq!(active_tab.size.height, px(TAB_HEIGHT));
        assert_eq!(
            active_tab.top(),
            tab_viewport.top() + px(ACTIVE_TAB_TOP_GAP)
        );
        assert_eq!(active_tab.bottom(), tab_viewport.bottom());
        assert_eq!(tab_viewport.bottom(), content.bottom());
        assert_eq!(active_title.left(), active_tab_layout.left() + px(15.));
        assert_eq!(active_indicator.size.width, px(TAB_INDICATOR_WIDTH));
        assert_eq!(
            active_indicator.right(),
            active_tab_layout.right() - px(12.)
        );
        if rendered_tab_count <= 2 {
            assert_eq!(scroll.max_offset().x, px(0.));
        } else {
            assert!(
                scroll.max_offset().x > px(0.),
                "requested tabs={tab_count}, rendered tabs={rendered_tab_count}, max offset={:?}, viewport={:?}",
                scroll.max_offset().x,
                scroll.bounds()
            );
            let viewport = scroll.bounds();
            let offset = scroll.offset().x;
            assert!(active_tab.left() + offset >= viewport.left());
            assert!(active_tab.right() + offset <= viewport.right());
        }

        if tab_count == 1 {
            let button = test_window
                .debug_bounds("dopad-test-new-document")
                .expect("new-document button is measured");
            test_window.simulate_click(
                point(
                    button.origin.x + button.size.width / 2.,
                    button.origin.y + button.size.height / 2.,
                ),
                Modifiers::default(),
            );
            assert_eq!(
                view.read_with(test_window, |state, _| state.new_document_clicks),
                1,
                "new-document button click must be dispatched"
            );
            for _ in 0..3 {
                test_window.update(|window, cx| {
                    window.draw(cx).clear(cx);
                    window.simulate_next_frame(cx);
                });
            }
        }
    }

    for width in [560., 720., 980., 640., 560.] {
        test_window.simulate_resize(size(px(width), px(180.)));
        test_window.run_until_parked();
        for _ in 0..3 {
            test_window.update(|window, cx| {
                window.draw(cx).clear(cx);
                window.simulate_next_frame(cx);
            });
        }

        let (left, center, right) = (
            test_window
                .debug_bounds("dopad-topbar-left")
                .expect("fixed left region remains measured"),
            test_window
                .debug_bounds("dopad-topbar-center")
                .expect("flexible center region remains measured"),
            test_window
                .debug_bounds("dopad-topbar-right")
                .expect("fixed right region remains measured"),
        );
        assert!(left.right() <= center.left());
        assert!(center.right() <= right.left());

        let scroll = view.read_with(test_window, |state, _| state.tabs_scroll.clone());
        let viewport = scroll.bounds();
        let active_tab = scroll
            .bounds_for_item(99)
            .expect("active tab remains measured after resize");
        let offset = scroll.offset().x;
        assert!(active_tab.left() + offset >= viewport.left());
        assert!(
            active_tab.right() + offset <= viewport.right(),
            "active tab outside after resize to {width}: viewport={viewport:?}, tab={active_tab:?}, offset={offset:?}, max={:?}",
            scroll.max_offset()
        );
    }

    for scale_factor in [1.25, 1.5, 1.0] {
        test_window.simulate_scale_factor_change(scale_factor);
        test_window.run_until_parked();
        for _ in 0..3 {
            test_window.update(|window, cx| {
                window.draw(cx).clear(cx);
                window.simulate_next_frame(cx);
            });
        }

        let (content, left, center, right) = (
            test_window
                .debug_bounds("dopad-topbar-content")
                .expect("topbar is measured after DPI change"),
            test_window
                .debug_bounds("dopad-topbar-left")
                .expect("fixed left region remains measured after DPI change"),
            test_window
                .debug_bounds("dopad-topbar-center")
                .expect("flexible center remains measured after DPI change"),
            test_window
                .debug_bounds("dopad-topbar-right")
                .expect("fixed right region remains measured after DPI change"),
        );
        assert_eq!(content.size.height, px(44.));
        assert!(left.right() <= center.left());
        assert!(center.right() <= right.left());

        let scroll = view.read_with(test_window, |state, _| state.tabs_scroll.clone());
        let viewport = scroll.bounds();
        let active_tab = scroll
            .bounds_for_item(99)
            .expect("active tab remains measured after DPI change");
        let offset = scroll.offset().x;
        assert!(
            active_tab.left() + offset >= viewport.left(),
            "active tab left edge outside at scale {scale_factor}: viewport={viewport:?}, tab={active_tab:?}, offset={offset:?}"
        );
        assert!(
            active_tab.right() + offset <= viewport.right(),
            "active tab right edge outside at scale {scale_factor}: viewport={viewport:?}, tab={active_tab:?}, offset={offset:?}"
        );
    }

    test_window.simulate_resize(size(px(980.), px(180.)));
    test_window.run_until_parked();
    for _ in 0..3 {
        test_window.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.simulate_next_frame(cx);
        });
    }
    let scroll = view.read_with(test_window, |state, _| state.tabs_scroll.clone());
    let viewport = scroll.bounds();
    let visible_tab = scroll
        .bounds_for_item(98)
        .expect("neighboring tab is measured after widening");
    let offset = scroll.offset().x;
    assert!(visible_tab.left() + offset >= viewport.left());
    assert!(visible_tab.right() + offset <= viewport.right());

    view.update(test_window, |state, cx| {
        state.active = 98;
        cx.notify();
    });
    test_window.run_until_parked();
    for _ in 0..3 {
        test_window.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.simulate_next_frame(cx);
        });
    }
    assert_eq!(
        scroll.offset().x,
        offset,
        "selecting a fully visible tab must not scroll the tab strip"
    );
}

#[test]
fn tab_hover_and_padding_tokens_match_the_warm_paper_spec() {
    let palette = crate::theme::palette(false);
    assert_eq!(tab_padding(false), (15., 12.));
    assert_eq!(tab_padding(true), (9., 6.));
    assert_eq!(tab_hover_background(false, palette), Some(0xF7EBD9));
    assert_eq!(tab_hover_background(true, palette), None);
}

#[test]
fn tab_indicator_shows_close_for_active_or_hovered_tabs_and_preserves_dirty_state() {
    assert_eq!(tab_indicator(true, false, false), TabIndicator::Close);
    assert_eq!(tab_indicator(true, false, true), TabIndicator::Close);
    assert_eq!(tab_indicator(true, true, false), TabIndicator::Dirty);
    assert_eq!(tab_indicator(true, true, true), TabIndicator::Close);
    assert_eq!(tab_indicator(false, false, false), TabIndicator::Empty);
    assert_eq!(tab_indicator(false, false, true), TabIndicator::Close);
    assert_eq!(tab_indicator(false, true, false), TabIndicator::Dirty);
    assert_eq!(tab_indicator(false, true, true), TabIndicator::Close);
}

#[gpui::test]
fn hover_edge_scroll_moves_tabs_and_stops_when_pointer_returns_to_center(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, test_window) = cx.add_window_view(|_, _| LayoutHarness {
        tab_count: 100,
        active: 0,
        new_document_clicks: 0,
        tabs_scroll: ScrollHandle::new(),
    });
    test_window.simulate_resize(size(px(560.), px(180.)));
    test_window.run_until_parked();
    for _ in 0..3 {
        test_window.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.simulate_next_frame(cx);
        });
    }

    let scroll = view.read_with(test_window, |state, _| state.tabs_scroll.clone());
    let viewport = scroll.bounds();
    let y = viewport.top() + viewport.size.height / 2.;
    test_window.update(|window, cx| {
        window.simulate_mouse_move(point(viewport.right() - px(2.), y), cx);
    });
    for _ in 0..8 {
        test_window.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.simulate_next_frame(cx);
        });
    }
    test_window.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let right_edge_offset = scroll.offset().x;
    assert!(
        right_edge_offset < px(0.),
        "right edge must reveal later tabs"
    );
    assert_eq!(
        view.read_with(test_window, |state, _| state.active),
        0,
        "hovering over tabs must not change the active tab"
    );

    let pointer = point(viewport.right() - px(2.), y);
    let clicked_index = (0..100)
        .find(|index| {
            scroll.bounds_for_item(*index).is_some_and(|tab| {
                tab.left() + right_edge_offset <= pointer.x
                    && tab.right() + right_edge_offset >= pointer.x
            })
        })
        .expect("a tab is under the pointer at the right edge");
    test_window.simulate_click(pointer, Modifiers::default());
    assert_eq!(
        view.read_with(test_window, |state, _| state.active),
        clicked_index,
        "clicking while edge scrolling selects the tab under the pointer"
    );
    for _ in 0..3 {
        test_window.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.simulate_next_frame(cx);
        });
    }
    let after_click_offset = scroll.offset().x;
    test_window.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    assert_eq!(
        scroll.offset().x,
        after_click_offset,
        "clicking a tab takes priority over queued hover scrolling"
    );

    test_window.update(|window, cx| {
        window.simulate_mouse_move(point(viewport.left() + viewport.size.width / 2., y), cx);
        window.simulate_next_frame(cx);
    });
    assert_eq!(
        scroll.offset().x,
        after_click_offset,
        "moving into the center stops auto-scroll"
    );

    test_window.update(|window, cx| {
        window.simulate_mouse_move(point(viewport.left() + px(2.), y), cx);
    });
    test_window.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    assert!(
        scroll.offset().x > after_click_offset,
        "changing to the left edge reverses direction smoothly"
    );

    test_window.update(|window, cx| {
        window.simulate_mouse_move(point(viewport.right() + px(80.), y), cx);
        window.simulate_next_frame(cx);
    });
    let stopped_offset = scroll.offset().x;
    test_window.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    assert_eq!(
        scroll.offset().x,
        stopped_offset,
        "leaving the tab viewport stops queued animation frames"
    );
}
