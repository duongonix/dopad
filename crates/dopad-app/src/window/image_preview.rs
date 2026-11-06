use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
};
use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, RenderImage, Styled,
    StyledImage, Window, div, img, px, rgb,
};

use crate::{
    theme::Palette,
    window::{DopadWindow, file_view::TabContent},
};

pub(super) const MAX_DECODED_PIXELS: u64 = 16_777_216;
const MAX_CACHE_BYTES: usize = 128 * 1024 * 1024;
pub(super) const MAX_ZOOM: f32 = 8.0;
const FIT_BASE_HEIGHT: f32 = 72.0;
const TOPBAR_HEIGHT: f32 = 44.0;
const STATUSBAR_HEIGHT: f32 = 28.0;

struct CachedImage {
    image: Arc<RenderImage>,
    bytes: usize,
}

#[derive(Default)]
pub(super) struct DecodedImageCache {
    entries: HashMap<u64, CachedImage>,
    lru: VecDeque<u64>,
    bytes: usize,
}

impl DecodedImageCache {
    pub fn get(&mut self, id: u64) -> Option<Arc<RenderImage>> {
        let image = self.entries.get(&id)?.image.clone();
        self.touch(id);
        Some(image)
    }

    pub fn contains(&self, id: u64) -> bool {
        self.entries.contains_key(&id)
    }

    pub fn insert(&mut self, id: u64, image: Arc<RenderImage>, bytes: usize) {
        self.remove(id);
        if bytes > MAX_CACHE_BYTES {
            return;
        }
        while self.bytes.saturating_add(bytes) > MAX_CACHE_BYTES {
            let Some(oldest) = self.lru.pop_front() else {
                break;
            };
            if let Some(evicted) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(evicted.bytes);
            }
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.entries.insert(id, CachedImage { image, bytes });
        self.lru.push_back(id);
    }

    pub fn remove(&mut self, id: u64) {
        if let Some(removed) = self.entries.remove(&id) {
            self.bytes = self.bytes.saturating_sub(removed.bytes);
            self.lru.retain(|entry| *entry != id);
        }
    }

    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    fn touch(&mut self, id: u64) {
        self.lru.retain(|entry| *entry != id);
        self.lru.push_back(id);
    }
}

pub(super) fn render(
    window: &mut DopadWindow,
    ui_window: &Window,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) -> AnyElement {
    let Some(tab) = window.active_tab() else {
        return div().size_full().bg(rgb(palette.canvas)).into_any_element();
    };
    let id = tab.id;
    let title = tab.title.clone();
    let content = tab.content.clone();
    let TabContent::Image(state) = content else {
        return super::unsupported_file::render_file_state(&title, &content, false, palette, cx);
    };

    let viewport = image_viewport(window, ui_window);
    let fit = fit_scale(state.info.width, state.info.height, viewport.0, viewport.1);
    if let Some(tab) = window.tabs.iter_mut().find(|tab| tab.id == id)
        && let TabContent::Image(state) = &mut tab.content
    {
        state.fit_zoom = fit;
    }
    let zoom = state.zoom.unwrap_or(fit);
    let width = state.info.width as f32 * zoom;
    let height = state.info.height as f32 * zoom;
    let render_image = window.image_cache.get(id);
    let Some(render_image) = render_image else {
        return centered_message(
            "Loading image…",
            "Dopad is preparing this image preview.",
            palette,
        );
    };

    let image = img(render_image)
        .size(px(width))
        .h(px(height))
        .object_fit(gpui_kit::ObjectFit::Contain);
    let image_left = ((viewport.0 - width) * 0.5 + state.pan_x).max(-width + 32.0);
    let image_top = ((viewport.1 - height) * 0.5 + state.pan_y).max(-height + 32.0);
    let image = div()
        .absolute()
        .left(px(image_left))
        .top(px(image_top))
        .child(image);

    let controls = div()
        .absolute()
        .right(px(14.))
        .bottom(px(12.))
        .flex()
        .items_center()
        .gap_1()
        .child(image_control(
            "image-zoom-out",
            IconName::ZoomOut,
            "Zoom out",
            cx.listener(move |this, _, _, cx| this.zoom_image(id, 1.0 / 1.2, fit, cx)),
            palette,
        ))
        .child(image_control(
            "image-zoom-in",
            IconName::ZoomIn,
            "Zoom in",
            cx.listener(move |this, _, _, cx| this.zoom_image(id, 1.2, fit, cx)),
            palette,
        ))
        .child(image_control(
            "image-reset-zoom",
            IconName::RotateCcw,
            "Reset zoom to 100%",
            cx.listener(move |this, _, _, cx| this.reset_image_zoom(id, cx)),
            palette,
        ))
        .child(image_control(
            "image-fit-window",
            IconName::Expand,
            "Fit to window",
            cx.listener(move |this, _, _, cx| this.fit_image_to_window(id, cx)),
            palette,
        ));

    let move_id = id;
    let down_id = id;
    let up_id = id;
    let wheel_id = id;
    div()
        .id("image-preview-viewport")
        .size_full()
        .min_w_0()
        .min_h_0()
        .relative()
        .overflow_hidden()
        .bg(rgb(palette.canvas))
        .child(image)
        .child(controls)
        .on_mouse_down(
            gpui_kit::MouseButton::Left,
            cx.listener(move |this, event: &gpui_kit::MouseDownEvent, _, cx| {
                this.begin_image_pan(
                    down_id,
                    event.position.x.as_f32(),
                    event.position.y.as_f32(),
                    cx,
                );
            }),
        )
        .on_mouse_move(
            cx.listener(move |this, event: &gpui_kit::MouseMoveEvent, _, cx| {
                this.move_image_pan(
                    move_id,
                    event.position.x.as_f32(),
                    event.position.y.as_f32(),
                    cx,
                );
            }),
        )
        .on_mouse_up(
            gpui_kit::MouseButton::Left,
            cx.listener(move |this, _, _, cx| this.end_image_pan(up_id, cx)),
        )
        .on_scroll(
            cx.listener(move |this, event: &gpui_kit::ScrollEvent, _, cx| {
                if event.modifiers.control {
                    let delta = match event.delta {
                        gpui_kit::ScrollDelta::Pixels(delta) => delta.y.as_f32(),
                        gpui_kit::ScrollDelta::Lines(delta) => delta.y,
                    };
                    let factor = if delta < 0.0 { 1.1 } else { 1.0 / 1.1 };
                    this.zoom_image(wheel_id, factor, fit, cx);
                }
            }),
        )
        .into_any_element()
}

fn centered_message(title: &str, description: &str, palette: &Palette) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .bg(rgb(palette.canvas))
        .text_color(rgb(palette.secondary))
        .child(title.to_owned())
        .child(div().text_xs().child(description.to_owned()))
        .into_any_element()
}

fn image_control(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
    palette: &Palette,
) -> AnyElement {
    Button::new(id)
        .icon(icon)
        .xsmall()
        .compact()
        .ghost()
        .text_color(rgb(palette.secondary))
        .tooltip(label)
        .accessibility_label(label)
        .on_click(on_click)
        .into_any_element()
}

fn image_viewport(window: &DopadWindow, ui_window: &Window) -> (f32, f32) {
    let width = ui_window.bounds().size.width.as_f32();
    let height = ui_window.bounds().size.height.as_f32();
    let sidebar = if window.sidebar.mode == super::sidebar::SidebarMode::Closed {
        0.0
    } else {
        window.sidebar.width
    };
    let footer_height = if window.settings.current().show_status_bar {
        STATUSBAR_HEIGHT
    } else {
        0.0
    };
    (
        (width - sidebar).max(1.0),
        (height - TOPBAR_HEIGHT - footer_height).max(1.0),
    )
}

pub(super) fn fit_scale(width: u32, height: u32, viewport_width: f32, viewport_height: f32) -> f32 {
    (viewport_width / width.max(1) as f32)
        .min((viewport_height - FIT_BASE_HEIGHT).max(1.0) / height.max(1) as f32)
        .min(1.0)
}

pub(super) fn zoom_percent(
    state: &super::file_view::ImageTabState,
    ui_window: &Window,
    sidebar_width: Option<f32>,
    show_status_bar: bool,
) -> u32 {
    let bounds = ui_window.bounds().size;
    let viewport_width = (bounds.width.as_f32() - sidebar_width.unwrap_or_default()).max(1.0);
    let footer_height = if show_status_bar {
        STATUSBAR_HEIGHT
    } else {
        0.0
    };
    let viewport_height = (bounds.height.as_f32() - TOPBAR_HEIGHT - footer_height).max(1.0);
    let fit = fit_scale(
        state.info.width,
        state.info.height,
        viewport_width,
        viewport_height,
    );
    (state.zoom.unwrap_or(fit) * 100.0).round() as u32
}

impl DopadWindow {
    pub(super) fn ensure_active_image(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else { return };
        if !matches!(tab.content, TabContent::Image(_)) || self.image_cache.contains(tab.id) {
            return;
        }
        let Some(path) = tab.path.clone() else { return };
        self.start_file_load(tab.id, path, cx);
    }

    pub(super) fn zoom_image(&mut self, id: u64, factor: f32, fit: f32, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if let TabContent::Image(state) = &mut tab.content {
            state.zoom_by(factor, fit);
            cx.notify();
        }
    }

    pub(super) fn reset_image_zoom(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if let TabContent::Image(state) = &mut tab.content {
            state.reset_zoom();
            cx.notify();
        }
    }

    pub(super) fn fit_image_to_window(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if let TabContent::Image(state) = &mut tab.content {
            state.fit_to_window();
            cx.notify();
        }
    }

    pub(super) fn begin_image_pan(&mut self, id: u64, x: f32, y: f32, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if let TabContent::Image(state) = &mut tab.content {
            if state.zoom.is_some() {
                state.pan_drag = Some(super::file_view::PanDrag {
                    start_x: x,
                    start_y: y,
                    pan_x: state.pan_x,
                    pan_y: state.pan_y,
                });
            }
            cx.notify();
        }
    }

    pub(super) fn move_image_pan(&mut self, id: u64, x: f32, y: f32, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if let TabContent::Image(state) = &mut tab.content {
            let Some(drag) = state.pan_drag else { return };
            let max_x = (state.info.width as f32 * state.zoom.unwrap_or(1.0)) * 0.5;
            let max_y = (state.info.height as f32 * state.zoom.unwrap_or(1.0)) * 0.5;
            state.pan_x = (drag.pan_x + x - drag.start_x).clamp(-max_x, max_x);
            state.pan_y = (drag.pan_y + y - drag.start_y).clamp(-max_y, max_y);
            cx.notify();
        }
    }

    pub(super) fn end_image_pan(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if let TabContent::Image(state) = &mut tab.content {
            state.pan_drag = None;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::DecodedImageCache;
    use gpui_kit::RenderImage;
    use std::sync::Arc;

    #[test]
    fn cache_is_lru_bounded_and_releases_closed_image_entries() {
        let mut cache = DecodedImageCache::default();
        let image = || Arc::new(RenderImage::new(Vec::new()));
        cache.insert(1, image(), 64 * 1024 * 1024);
        cache.insert(2, image(), 64 * 1024 * 1024);
        assert!(cache.get(1).is_some());
        cache.insert(3, image(), 64 * 1024 * 1024);
        assert!(cache.contains(1));
        assert!(!cache.contains(2));
        assert!(cache.contains(3));
        assert_eq!(cache.bytes(), 128 * 1024 * 1024);
        cache.remove(1);
        assert_eq!(cache.bytes(), 64 * 1024 * 1024);
    }

    #[test]
    fn fit_scale_never_upscales_small_images_and_keeps_aspect() {
        assert_eq!(super::fit_scale(200, 100, 1000.0, 800.0), 1.0);
        let scale = super::fit_scale(2000, 1000, 1000.0, 800.0);
        assert!(scale <= 0.5);
        assert!((2000.0 * scale / (1000.0 * scale) - 2.0).abs() < f32::EPSILON);
    }
}
