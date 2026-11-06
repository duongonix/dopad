#[cfg(target_os = "linux")]
use gpui_kit::WindowDecorations;
use gpui_kit::{
    AnyWindowHandle, AppContext, Context, Entity, TitlebarOptions, WindowBounds, WindowOptions,
    open_window, px, size,
};

use crate::{settings::SettingsService, updater::UpdateManager};

use super::{DopadWindow, UpdateWindow, WINDOW_HEIGHT, WINDOW_WIDTH};

pub(super) fn open(
    updater: Entity<UpdateManager>,
    settings: SettingsService,
    cx: &mut Context<DopadWindow>,
) -> anyhow::Result<AnyWindowHandle> {
    let bounds = WindowBounds::centered(size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
    let options = WindowOptions {
        icon: crate::icon::window_icon(),
        window_bounds: Some(bounds),
        window_min_size: Some(size(px(600.), px(400.))),
        titlebar: Some(TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: None,
        }),
        app_owns_titlebar_drag: true,
        #[cfg(target_os = "linux")]
        window_decorations: Some(WindowDecorations::Client),
        ..Default::default()
    };
    let (handle, _) = open_window(options, cx, move |window, cx| {
        window.set_window_title("Dopad Update");
        cx.new(|cx| UpdateWindow::new(updater, settings, cx))
    })?;
    Ok(handle)
}
