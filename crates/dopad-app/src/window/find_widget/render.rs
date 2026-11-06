use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Disableable, Icon, Sizable, Size,
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
};
use gpui_kit::{
    Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _,
    Window, div, px, rgb,
};

use super::super::search::SearchAction;
use super::FindWidget;
impl Render for FindWidget {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().size_0().into_any_element();
        }
        let dark = self
            .parent
            .upgrade()
            .is_some_and(|parent| parent.read(cx).dark_theme);
        let palette = crate::theme::palette(dark);
        let counter = self
            .error
            .clone()
            .or_else(|| self.result_message.clone())
            .unwrap_or_else(|| {
                if self.find.read(cx).value().is_empty() {
                    String::new()
                } else if self.matches.is_empty() {
                    "No results".into()
                } else if let Some(index) = self.current {
                    format!("{} of {}", index + 1, self.matches.len())
                } else {
                    format!("{} results", self.matches.len())
                }
            });
        let counter_color = if self.error.is_some() {
            0xA13D2D
        } else {
            palette.secondary
        };

        let expand = Button::new("find-toggle-replace")
            .icon(if self.replace_mode {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .xsmall()
            .compact()
            .ghost()
            .tooltip(if self.replace_mode {
                "Hide Replace"
            } else {
                "Show Replace"
            })
            .accessibility_label(if self.replace_mode {
                "Hide Replace"
            } else {
                "Show Replace"
            })
            .on_click(cx.listener(|this, _, window, cx| {
                this.replace_mode = !this.replace_mode;
                if this.replace_mode {
                    this.replace.update(cx, |state, cx| state.focus(window, cx));
                } else {
                    this.find.update(cx, |state, cx| state.focus(window, cx));
                }
                cx.notify();
            }));
        let find_row = div()
            .flex()
            .items_center()
            .gap_1()
            .child(expand)
            .child(input_field(&self.find, "find-query", palette))
            .child(toggle_button(
                "find-match-case",
                IconName::CaseSensitive,
                "Match Case (Alt+C)",
                self.match_case,
                palette,
                cx.listener(|this, _, _, cx| this.toggle_case(cx)),
            ))
            .child(toggle_button(
                "find-whole-word",
                IconName::WholeWord,
                "Match Whole Word (Alt+W)",
                self.whole_word,
                palette,
                cx.listener(|this, _, _, cx| this.toggle_whole_word(cx)),
            ))
            .child(toggle_button(
                "find-regex",
                IconName::Regex,
                "Use Regular Expression (Alt+R)",
                self.regex,
                palette,
                cx.listener(|this, _, _, cx| this.toggle_regex(cx)),
            ))
            .child(toggle_button(
                "find-in-selection",
                IconName::ScanSearch,
                "Find in Selection",
                self.find_in_selection,
                palette,
                cx.listener(|this, _, _, cx| {
                    let selected = this.active_selection(cx);
                    this.toggle_find_in_selection(selected, cx);
                }),
            ))
            .child(
                div()
                    .min_w(px(55.))
                    .text_size(px(11.))
                    .text_color(rgb(counter_color))
                    .child(counter),
            )
            .child(icon_button(
                "find-previous",
                IconName::ArrowUp,
                "Previous Match (Shift+Enter)",
                palette,
                self.matches.is_empty(),
                cx.listener(|this, _, _, cx| this.navigate_previous(cx)),
            ))
            .child(icon_button(
                "find-next",
                IconName::ArrowDown,
                "Next Match (Enter)",
                palette,
                self.matches.is_empty(),
                cx.listener(|this, _, _, cx| this.navigate_next(cx)),
            ))
            .child(icon_button(
                "find-close",
                IconName::X,
                "Close (Escape)",
                palette,
                false,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            ));

        let mut widget = div()
            .absolute()
            .top(px(12.))
            .right(px(18.))
            .w(px(540.))
            .max_w_full()
            .flex()
            .flex_col()
            .gap_1()
            .p(px(6.))
            .bg(rgb(palette.chrome))
            .border_1()
            .border_color(rgb(palette.border))
            .text_color(rgb(palette.text))
            .child(find_row);
        if self.replace_mode {
            let disabled = self.matches.is_empty() || self.error.is_some();
            let replace_actions = div()
                .flex()
                .items_center()
                .gap_1()
                .child(input_field(&self.replace, "replace-query", palette))
                .child(
                    Button::new("find-replace-one")
                        .label("Replace")
                        .xsmall()
                        .compact()
                        .ghost()
                        .disabled(disabled)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let replacement = this.replace.read(cx).value().to_string();
                            this.submit(SearchAction::ReplaceCurrent(replacement), cx);
                        })),
                )
                .child(
                    Button::new("find-replace-all")
                        .label("Replace All")
                        .xsmall()
                        .compact()
                        .ghost()
                        .disabled(disabled)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let replacement = this.replace.read(cx).value().to_string();
                            this.submit(SearchAction::ReplaceAll(replacement), cx);
                        })),
                );
            widget = widget.child(replace_actions);
        }
        widget.into_any_element()
    }
}

fn input_field(
    state: &Entity<InputState>,
    id: &'static str,
    palette: &crate::theme::Palette,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .h(px(30.))
        .border_1()
        .border_color(rgb(palette.border))
        .bg(rgb(palette.canvas))
        .child(Input::new(state))
}

fn toggle_button(
    id: &'static str,
    icon: IconName,
    tooltip: &'static str,
    active: bool,
    palette: &crate::theme::Palette,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> impl IntoElement {
    Button::new(id)
        .child(Icon::new(icon).with_size(Size::Size(px(15.))))
        .xsmall()
        .compact()
        .ghost()
        .text_color(rgb(if active {
            palette.accent
        } else {
            palette.secondary
        }))
        .tooltip(tooltip)
        .accessibility_label(tooltip)
        .on_click(on_click)
}

fn icon_button(
    id: &'static str,
    icon: IconName,
    tooltip: &'static str,
    palette: &crate::theme::Palette,
    disabled: bool,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> impl IntoElement {
    Button::new(id)
        .child(Icon::new(icon).with_size(Size::Size(px(15.))))
        .xsmall()
        .compact()
        .ghost()
        .text_color(rgb(palette.secondary))
        .tooltip(tooltip)
        .accessibility_label(tooltip)
        .disabled(disabled)
        .on_click(on_click)
}
