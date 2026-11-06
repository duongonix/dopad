use gpui_kit::component::notification::{Notification, NotificationList};
use gpui_kit::{Anchor, Context, Styled, Window, px, rgb};

use crate::theme::Palette;

use super::DopadWindow;

struct StatusToast;

pub(super) fn push_current_status(
    dopad: &mut DopadWindow,
    window: &mut Window,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) {
    let message = if !dopad.status.is_empty() {
        Some(dopad.status.clone())
    } else if let Some(tab) = dopad
        .active_tab()
        .filter(|tab| tab.external_change_detected)
    {
        Some(format!(
            "{} changed outside Dopad; saving is blocked to protect the disk copy.",
            tab.title
        ))
    } else {
        dopad.session_status.clone()
    };

    let changed =
        dopad.status_generation != dopad.toast_generation || message != dopad.last_toast_message;
    if !changed {
        return;
    }

    dopad.toast_generation = dopad.status_generation;
    dopad.last_toast_message = message.clone();
    let Some(message) = message else {
        return;
    };

    let notification = Notification::new()
        .id::<StatusToast>()
        .message(message)
        .placement(Anchor::BottomRight)
        .bg(rgb(palette.chrome))
        .text_color(rgb(palette.text))
        .border_color(rgb(palette.border))
        .rounded(px(0.));
    let notifications: gpui_kit::Entity<NotificationList> = dopad.notifications.clone();
    cx.spawn_in(window, async move |_, cx| {
        let _ = notifications.update_in(cx, |list, notification_window, notification_cx| {
            list.push(notification, notification_window, notification_cx);
        });
    })
    .detach();
}
