use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon, Sizable, Size,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::{
    Anchor, Context, Div, FontWeight, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window, WindowControlArea, div, img,
    px, rgb,
};

use super::output::render_output;
use super::{TERMINAL_ICON, TerminalWindow, default_shell};
use crate::theme;

const TERMINAL_CONTROL_SLOT_WIDTH: f32 = 34.;
const TERMINAL_CONTROL_SLOT_HEIGHT: f32 = 44.;

impl Render for TerminalWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(false);
        let bounds = window.bounds();
        let terminal_settings = self.settings.current();
        let (cell_width, line_height) = super::terminal_metrics(window, &terminal_settings);
        self.resize_active(
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
            cell_width,
            line_height,
        );
        let layout = (
            self.active,
            self.tabs.len(),
            f32::from(window.bounds().size.width),
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
        let active = self.tabs.get(self.active);
        let shell_name = active
            .map(|tab| tab.shell.name.clone())
            .unwrap_or_else(|| "Terminal".into());
        let cwd = active
            .map(|tab| tab.cwd.display().to_string())
            .unwrap_or_else(|| self.cwd.display().to_string());
        let footer = render_footer(shell_name, cwd, palette);
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
            .bg(rgb(palette.canvas))
            .children([topbar, body.into_any_element(), footer.into_any_element()])
    }
}

fn render_footer(
    shell_name: String,
    cwd: String,
    palette: &theme::Palette,
) -> impl IntoElement + use<> {
    div()
        .h(px(super::TERMINAL_FOOTER_HEIGHT))
        .w_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_end()
        .px(px(14.))
        .bg(rgb(palette.chrome))
        .border_t_1()
        .border_color(rgb(palette.border))
        .text_size(px(11.))
        .text_color(rgb(palette.secondary))
        .child(
            div()
                .flex()
                .items_center()
                .gap_4()
                .child(shell_name)
                .child(div().min_w_0().max_w(px(420.)).text_ellipsis().child(cwd)),
        )
}

fn render_topbar(
    terminal: &mut TerminalWindow,
    window: &Window,
    palette: &theme::Palette,
    cx: &mut Context<TerminalWindow>,
) -> impl IntoElement + use<> {
    let view = cx.entity();
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
                .h(px(39.))
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
    let tabs = div()
        .id("terminal-tabs")
        .flex_1()
        .min_w_0()
        .h(px(44.))
        .flex()
        .items_end()
        .gap_0()
        .overflow_x_scroll()
        .track_scroll(&terminal.tab_scroll)
        .children(tabs);
    let add = Button::new("terminal-new-tab")
        .icon(IconName::Plus)
        .compact()
        .ghost()
        .tooltip("New terminal")
        .accessibility_label("New terminal")
        .on_click(cx.listener(|this, _, _, cx| {
            if let Some(shell) = default_shell(&this.shells) {
                this.start_shell(shell, cx);
            }
        }));
    let shell_menu = Button::new("terminal-menu")
        .icon(IconName::ChevronDown)
        .compact()
        .ghost()
        .tooltip("Terminal actions")
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
    let maximize_icon = if window.is_maximized() {
        IconName::WindowRestore
    } else {
        IconName::WindowMaximize
    };
    let right = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_0()
        .child(fixed_control(add))
        .child(fixed_control(shell_menu))
        .child(window_button(
            "terminal-minimize",
            IconName::WindowMinimize,
            "Minimize",
            WindowControlArea::Min,
            0,
        ))
        .child(window_button(
            "terminal-maximize",
            maximize_icon,
            "Maximize or restore",
            WindowControlArea::Max,
            1,
        ))
        .child(window_button(
            "terminal-close-window",
            IconName::WindowClose,
            "Close",
            WindowControlArea::Close,
            2,
        ));
    div()
        .h(px(super::TERMINAL_TOPBAR_HEIGHT))
        .w_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_4()
        .px_3()
        .bg(rgb(palette.chrome))
        .child(
            div()
                .w(px(118.))
                .flex_shrink_0()
                .h_full()
                .window_control_area(WindowControlArea::Drag)
                .flex()
                .items_center()
                .gap_2()
                .pl(px(8.))
                .child(img(crate::icon::image()).size(px(20.)))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(palette.text))
                        .child("Terminal"),
                ),
        )
        .child(div().flex_1().min_w_0().overflow_x_hidden().child(tabs))
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

fn window_button(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    area: WindowControlArea,
    action: u8,
) -> impl IntoElement {
    let button = Button::new(id)
        .icon(icon)
        .compact()
        .ghost()
        .tooltip(label)
        .accessibility_label(label)
        .on_click(move |_, window, _| match action {
            0 => window.minimize_window(),
            1 => window.zoom_window(),
            _ => window.remove_window(),
        });
    let control = fixed_control(button);
    if cfg!(target_os = "windows") {
        control.window_control_area(area)
    } else {
        control
    }
}
