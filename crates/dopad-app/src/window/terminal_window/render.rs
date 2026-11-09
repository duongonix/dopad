use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon, Sizable, Size,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::{
    Anchor, Context, Div, FontWeight, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use super::super::bottom_panel::{BottomPanelView, view_selector};
use super::super::topbar::hover_scroll;
use super::output::render_output;
use super::{TERMINAL_ICON, TerminalWindow};
use crate::theme;

const TERMINAL_CONTROL_SLOT_WIDTH: f32 = 34.;
const TERMINAL_CONTROL_SLOT_HEIGHT: f32 = 36.;

impl Render for TerminalWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(false);
        let terminal_settings = self.settings.current();
        let (cell_width, line_height) = super::terminal_metrics(window, &terminal_settings);
        self.resize_active(self.panel_width, self.panel_height, cell_width, line_height);
        let layout = (
            self.active,
            self.tabs.len(),
            self.panel_width,
            window.scale_factor(),
        );
        if self.tab_layout != Some(layout) {
            self.tab_layout = Some(layout);
            let scroll = self.tab_scroll.clone();
            let active = self.active;
            let view = window.current_view();
            window.on_next_frame(move |_, cx| {
                super::super::topbar::reveal_measured_tab(&scroll, active);
                cx.notify(view);
            });
        }
        let topbar = render_topbar(self, window, palette, cx).into_any_element();
        let output = self.tabs.get(self.active).map(|tab| tab.snapshot.clone());
        let output = render_output(
            output,
            self.output_scroll.clone(),
            &terminal_settings,
            self.selection,
            palette,
            cell_width,
            cx,
        );
        let status = self.status.clone();
        let focus = self.focus.clone();
        let mut body = div()
            .flex_1()
            .min_h_0()
            .w_full()
            .bg(rgb(palette.canvas))
            .track_focus(&focus)
            .child(output)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.focus.focus(window, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.selecting = false;
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.send_key(event, cx)));
        if let Some(message) = status {
            body = body.child(
                div()
                    .absolute()
                    .top(px(12.))
                    .left(px(20.))
                    .text_color(rgb(palette.accent))
                    .child(message),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .key_context("Terminal")
            .bg(rgb(palette.canvas))
            .border_color(rgb(palette.border))
            .children([topbar, body.into_any_element()])
    }
}

fn render_topbar(
    terminal: &mut TerminalWindow,
    window: &mut Window,
    palette: &theme::Palette,
    cx: &mut Context<TerminalWindow>,
) -> impl IntoElement + use<> {
    let view = cx.entity();
    let hover_state = window.use_keyed_state("dopad-terminal-tabs-hover-scroll", cx, |_, _| {
        hover_scroll::HoverScrollState::default()
    });
    let move_state = hover_state.clone();
    let move_scroll = terminal.tab_scroll.clone();
    let move_view = view.clone();
    let shell_choices = terminal.shells.clone();
    let tabs = terminal
        .tabs
        .iter()
        .enumerate()
        .map(|(index, tab)| {
            let active = index == terminal.active;
            let tab_id = tab.id;
            let shell_name = tab.shell.name.clone();
            let select = cx.listener(move |this, _, _, cx| this.activate_tab(index, cx));
            let close = cx.listener(move |this, _, _, cx| this.close_tab(index, cx));
            let tab_bg = if active {
                palette.canvas
            } else {
                palette.chrome
            };
            let mut tab = div()
                .id(format!("terminal-tab-{tab_id}"))
                .flex_shrink_0()
                .h(px(31.))
                .mt(px(5.))
                .px(px(12.))
                .flex()
                .items_center()
                .gap_2()
                .rounded_tl(px(8.))
                .rounded_tr(px(8.))
                .bg(rgb(tab_bg));
            if !active {
                tab = tab.hover(|style| style.bg(rgb(0xF7EBD9)));
            }
            tab.child(
                Icon::default()
                    .data(TERMINAL_ICON)
                    .with_size(Size::Size(px(14.)))
                    .text_color(rgb(palette.secondary)),
            )
            .child(
                div()
                    .max_w(px(150.))
                    .text_ellipsis()
                    .text_size(px(13.))
                    .font_weight(if active {
                        FontWeight::BOLD
                    } else {
                        FontWeight::SEMIBOLD
                    })
                    .text_color(rgb(palette.text))
                    .child(shell_name),
            )
            .child(
                Button::new(format!("terminal-close-{tab_id}"))
                    .icon(IconName::X)
                    .xsmall()
                    .compact()
                    .ghost()
                    .on_click(close),
            )
            .on_click(select)
        })
        .collect::<Vec<_>>();
    let tab_row = div()
        .id("terminal-tabs")
        .flex_1()
        .min_w_0()
        .h(px(super::TERMINAL_TOPBAR_HEIGHT))
        .flex()
        .items_end()
        .gap_0()
        .overflow_x_scroll()
        .track_scroll(&terminal.tab_scroll)
        .children(tabs);
    let tabs = div()
        .flex_1()
        .min_w_0()
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
        })
        .child(tab_row);
    let shell_menu = Button::new("terminal-menu")
        .icon(IconName::Plus)
        .compact()
        .ghost()
        .tooltip("New terminal")
        .accessibility_label("New terminal")
        .dropdown_menu_with_anchor(Anchor::BottomRight, {
            let entity = view.clone();
            move |mut menu, _, _| {
                menu = menu.label("Open a shell").separator();
                for shell in shell_choices.iter().cloned() {
                    let entity = entity.clone();
                    let label = shell.name.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, app| {
                        let shell = shell.clone();
                        entity.update(app, |terminal, cx| terminal.start_shell(shell, cx));
                    }));
                }
                menu
            }
        });
    let minimize = Button::new("terminal-panel-minimize")
        .icon(IconName::ChevronDown)
        .compact()
        .ghost()
        .tooltip("Hide Terminal")
        .accessibility_label("Hide Terminal")
        .on_click(cx.listener(|this, _, window, cx| this.request_hide(window, cx)));
    let close = Button::new("terminal-panel-close")
        .icon(IconName::X)
        .compact()
        .ghost()
        .tooltip("Close Terminal panel")
        .accessibility_label("Close Terminal panel")
        .on_click(cx.listener(|this, _, window, cx| this.request_hide(window, cx)));
    let right = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_0()
        .child(fixed_control(shell_menu))
        .child(fixed_control(minimize))
        .child(fixed_control(close));
    div()
        .h(px(super::TERMINAL_TOPBAR_HEIGHT))
        .w_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_4()
        .px_3()
        .bg(rgb(palette.chrome))
        .border_b_1()
        .border_color(rgb(palette.border))
        .child(
            div()
                .w(px(94.))
                .flex_shrink_0()
                .h_full()
                .flex()
                .items_center()
                .pl(px(4.))
                .child(view_selector(
                    terminal.parent.clone(),
                    BottomPanelView::Terminal,
                    *palette,
                    "terminal-view-selector",
                )),
        )
        .child(tabs)
        .child(right)
}

fn fixed_control(button: impl IntoElement) -> Div {
    div()
        .w(px(TERMINAL_CONTROL_SLOT_WIDTH))
        .h(px(TERMINAL_CONTROL_SLOT_HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(button)
}

