use gpui_kit::component::{
    WindowExt as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::DialogFooter,
    input::{Input, InputState},
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    WeakEntity, Window, div, px,
};
use std::cell::Cell;

use super::super::{DopadWindow, search};
use crate::window::search::SearchAction;

pub(super) fn open(window: &mut Window, cx: &mut Context<DopadWindow>, replace: bool) {
    let parent = cx.entity().downgrade();
    let panel = cx.new(|cx| FindReplaceDialog::new(window, cx, parent, replace));
    let initial_focus = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            panel.update(cx, |panel, cx| panel.focus_find(window, cx));
        }
        let close = Button::new("find-close")
            .label("Close")
            .ghost()
            .on_click(|_, window, cx| window.close_dialog(cx));
        dialog
            .title(if replace { "Find and Replace" } else { "Find" })
            .child(panel.clone())
            .footer(DialogFooter::new().child(close))
    });
}

struct FindReplaceDialog {
    parent: WeakEntity<DopadWindow>,
    find: Entity<InputState>,
    replacement: Entity<InputState>,
    replace_mode: bool,
    match_case: bool,
    whole_word: bool,
    regex: bool,
}

impl FindReplaceDialog {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        parent: WeakEntity<DopadWindow>,
        replace_mode: bool,
    ) -> Self {
        Self {
            parent,
            find: cx.new(|cx| InputState::new(window, cx).placeholder("Find")),
            replacement: cx.new(|cx| InputState::new(window, cx).placeholder("Replace with")),
            replace_mode,
            match_case: false,
            whole_word: false,
            regex: false,
        }
    }

    fn focus_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find.update(cx, |input, cx| input.focus(window, cx));
    }
}

impl Render for FindReplaceDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let match_case = Checkbox::new("find-match-case")
            .label("Match case")
            .checked(self.match_case)
            .on_change(cx.listener(|this, value, _, cx| {
                this.match_case = *value;
                cx.notify();
            }));
        let whole_word = Checkbox::new("find-whole-word")
            .label("Whole word")
            .checked(self.whole_word)
            .on_change(cx.listener(|this, value, _, cx| {
                this.whole_word = *value;
                cx.notify();
            }));
        let regex = Checkbox::new("find-regex")
            .label("Regular expression")
            .checked(self.regex)
            .on_change(cx.listener(|this, value, _, cx| {
                this.regex = *value;
                cx.notify();
            }));

        let mut footer = div().w_full().flex().flex_wrap().items_center().gap_2();
        if self.replace_mode {
            let replace_parent = self.parent.clone();
            let replace_find = self.find.clone();
            let replace_value = self.replacement.clone();
            let replace_options = (self.match_case, self.whole_word, self.regex);
            footer = footer.child(
                Button::new("find-replace-current")
                    .label("Replace")
                    .outline()
                    .on_click(move |_, window, cx| {
                        submit_entities(
                            &replace_parent,
                            &replace_find,
                            replace_options,
                            SearchAction::ReplaceCurrent(
                                replace_value.read(cx).value().to_string(),
                            ),
                            window,
                            cx,
                        );
                    }),
            );
            let replace_all_parent = self.parent.clone();
            let replace_all_find = self.find.clone();
            let replace_all_value = self.replacement.clone();
            let replace_all_options = (self.match_case, self.whole_word, self.regex);
            footer = footer.child(
                Button::new("find-replace-all")
                    .label("Replace All")
                    .primary()
                    .on_click(move |_, window, cx| {
                        let replacement = replace_all_value.read(cx).value().to_string();
                        submit_entities(
                            &replace_all_parent,
                            &replace_all_find,
                            replace_all_options,
                            SearchAction::ReplaceAll(replacement),
                            window,
                            cx,
                        );
                    }),
            );
        }

        let previous_parent = self.parent.clone();
        let previous_find = self.find.clone();
        let previous_options = (self.match_case, self.whole_word, self.regex);
        footer = footer.child(
            Button::new("find-previous")
                .label("Previous")
                .ghost()
                .on_click(move |_, window, cx| {
                    submit_entities(
                        &previous_parent,
                        &previous_find,
                        previous_options,
                        SearchAction::Previous,
                        window,
                        cx,
                    );
                }),
        );
        let next_parent = self.parent.clone();
        let next_find = self.find.clone();
        let next_options = (self.match_case, self.whole_word, self.regex);
        footer = footer.child(
            Button::new("find-next")
                .label("Find Next")
                .primary()
                .on_click(move |_, window, cx| {
                    submit_entities(
                        &next_parent,
                        &next_find,
                        next_options,
                        SearchAction::Next,
                        window,
                        cx,
                    );
                }),
        );

        let mut content = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(Input::new(&self.find));
        if self.replace_mode {
            content = content.child(Input::new(&self.replacement));
        }
        content
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_4()
                    .child(match_case)
                    .child(whole_word)
                    .child(regex),
            )
            .child(footer.mt(px(4.)))
    }
}

fn submit_entities(
    parent: &WeakEntity<DopadWindow>,
    find: &Entity<InputState>,
    flags: (bool, bool, bool),
    action: SearchAction,
    window: &mut Window,
    cx: &mut App,
) {
    let mut options = dopad_editor::SearchOptions::literal(find.read(cx).value().to_string());
    options.case_sensitive = flags.0;
    options.whole_word = flags.1;
    options.regex = flags.2;
    if let Some(parent) = parent.upgrade() {
        parent.update(cx, |view, cx| {
            search::start(view, options, action, window, cx);
        });
    }
}
