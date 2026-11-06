use gpui_kit::component::{
    WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    input::{Input, InputState},
};
use gpui_kit::{AppContext as _, Context, ParentElement as _, Window, div};

use super::DopadWindow;

pub(super) fn confirm_close_tab(
    window: &mut Window,
    cx: &mut Context<DopadWindow>,
    document_id: u64,
    title: String,
) {
    let view = cx.entity().downgrade();
    window.open_dialog(cx, move |dialog, _, _| {
        let save_view = view.clone();
        let discard_view = view.clone();

        let cancel = Button::new("cancel-close-tab")
            .label("Cancel")
            .ghost()
            .on_click(|_, window, cx| window.close_dialog(cx));
        let discard = Button::new("discard-close-tab")
            .label("Don't Save")
            .danger()
            .on_click(move |_, window, cx| {
                window.close_dialog(cx);
                let Some(discard_view) = discard_view.upgrade() else {
                    return;
                };
                cx.update_entity(&discard_view, |view: &mut DopadWindow, cx| {
                    view.remove_tab(document_id);
                    cx.notify();
                });
            });
        let save = Button::new("save-close-tab")
            .label("Save")
            .primary()
            .on_click(move |_, window, cx| {
                window.close_dialog(cx);
                let Some(save_view) = save_view.upgrade() else {
                    return;
                };
                cx.update_entity(&save_view, |view: &mut DopadWindow, cx| {
                    view.save_document(document_id, false, Some(document_id), cx);
                });
            });

        dialog
            .title("Unsaved changes")
            .child(div().child(format!("Save changes to {title} before closing?")))
            .footer(DialogFooter::new().child(cancel).child(discard).child(save))
    });
}

pub(super) fn go_to_line(window: &mut Window, cx: &mut Context<DopadWindow>) {
    if cx.entity().read(cx).active_tab().is_none() {
        return;
    }
    let input = cx.new(|cx| InputState::new(window, cx).placeholder("Line or Line:Column"));
    let view = cx.entity().downgrade();
    window.open_dialog(cx, move |dialog, window, cx| {
        input.update(cx, |state, cx| state.focus(window, cx));
        let line_input = input.clone();
        let go_view = view.clone();
        let go = Button::new("go-to-line-confirm")
            .label("Go")
            .primary()
            .on_click(move |_, window, cx| {
                let value = line_input.read(cx).value().to_string();
                let Some((line, column)) = parse_line_column(&value) else {
                    return;
                };
                window.close_dialog(cx);
                let Some(view) = go_view.upgrade() else {
                    return;
                };
                cx.update_entity(&view, |view: &mut DopadWindow, cx| {
                    view.go_to_line_column(line, column, cx)
                });
            });
        let cancel = Button::new("go-to-line-cancel")
            .label("Cancel")
            .ghost()
            .on_click(|_, window, cx| window.close_dialog(cx));
        dialog
            .title("Go to Line or Column")
            .child(div().child(Input::new(&input)))
            .footer(DialogFooter::new().child(cancel).child(go))
    });
}

fn parse_line_column(value: &str) -> Option<(usize, usize)> {
    let mut pieces = value.trim().split(':');
    let line = pieces.next()?.trim().parse::<usize>().ok()?;
    let column = match pieces.next() {
        Some(value) => value.trim().parse::<usize>().ok()?,
        None => 1,
    };
    if pieces.next().is_some() || line == 0 || column == 0 {
        return None;
    }
    Some((line, column))
}

#[cfg(test)]
mod tests {
    use super::parse_line_column;

    #[test]
    fn parses_line_and_grapheme_column() {
        assert_eq!(parse_line_column("12"), Some((12, 1)));
        assert_eq!(parse_line_column(" 12:8 "), Some((12, 8)));
        assert_eq!(parse_line_column("12:0"), None);
        assert_eq!(parse_line_column("x:2"), None);
        assert_eq!(parse_line_column("1:2:3"), None);
    }
}
