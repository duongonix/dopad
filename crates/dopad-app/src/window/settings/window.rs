use gpui_kit::component::TitleBar;
use gpui_kit::component::input::InputState;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Context, Entity, FontWeight, IntoElement, ParentElement,
    Render, Styled, Subscription, WeakEntity, Window, WindowBounds, WindowOptions, div, img,
    open_window, px, rgb, size,
};

use crate::{
    settings::{AppSettings, SettingsService, save_file},
    window::DopadWindow,
};

use super::{content, font_download, footer, selects, sidebar, theme};

const WINDOW_WIDTH: f32 = 1000.0;
const WINDOW_HEIGHT: f32 = 680.0;
const MIN_WINDOW_WIDTH: f32 = 760.0;
const MIN_WINDOW_HEIGHT: f32 = 520.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    General,
    Editor,
    Terminal,
    Appearance,
    Files,
    Encoding,
    Keyboard,
    Advanced,
}

pub(super) struct SettingsWindow {
    pub(super) page: Page,
    pub(super) draft: AppSettings,
    pub(super) saved: AppSettings,
    pub(super) saving: bool,
    pub(super) error: Option<String>,
    pub(super) keyboard_search: Entity<InputState>,
    pub(super) selects: selects::Selectors,
    pub(super) keyboard_notice: Option<String>,
    pub(super) font_install: Option<font_download::FontInstallStatus>,
    pub(super) font_install_generation: u64,
    service: SettingsService,
    parent: WeakEntity<DopadWindow>,
    handle: Option<AnyWindowHandle>,
    _select_subscriptions: Vec<Subscription>,
}

impl SettingsWindow {
    fn new(
        service: SettingsService,
        parent: WeakEntity<DopadWindow>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = service.current();
        let (selects, select_subscriptions) = selects::initialize(&settings, window, cx);
        Self {
            page: Page::General,
            draft: settings.clone(),
            saved: settings,
            saving: false,
            error: None,
            keyboard_search: cx
                .new(|cx| InputState::new(window, cx).placeholder("Search shortcuts")),
            selects,
            keyboard_notice: None,
            font_install: None,
            font_install_generation: 0,
            service,
            parent,
            handle: None,
            _select_subscriptions: select_subscriptions,
        }
    }

    pub(super) fn sync_keyboard_selects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for spec in super::super::shortcuts::specs() {
            let value = super::super::shortcuts::display_key(
                &super::super::shortcuts::effective_key(&self.draft, spec.id),
            );
            if let Some(select) = self.selects.keyboard.get(spec.id) {
                select.update(cx, |state, cx| {
                    state.set_selected_value(&value, window, cx);
                });
            }
        }
    }

    pub(super) fn preview_word_wrap(&self, enabled: bool, cx: &mut Context<Self>) {
        let _ = self.parent.update_in(cx, |main, window, cx| {
            main.apply_word_wrap_only(enabled, window, cx)
        });
    }

    pub(super) fn restore_word_wrap_preview(&mut self, cx: &mut Context<Self>) {
        let Some(saved_word_wrap) = restore_word_wrap_value(&mut self.draft, &self.saved) else {
            return;
        };
        self.preview_word_wrap(saved_word_wrap, cx);
    }

    pub(super) fn persist(
        &mut self,
        close_after: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.saving {
            return;
        }
        if self.draft == self.saved {
            if close_after {
                window.remove_window();
            }
            return;
        }
        let conflicts = super::super::shortcuts::conflicts(&self.draft);
        if !conflicts.is_empty() {
            self.keyboard_notice = Some(format!("Shortcut conflicts: {}", conflicts.join("; ")));
            self.error = Some("Resolve shortcut conflicts before saving settings.".into());
            cx.notify();
            return;
        }
        if let Err(error) = super::super::shortcuts::bindings(&self.draft, &*cx) {
            self.error = Some(format!("Invalid keyboard shortcut: {error}"));
            cx.notify();
            return;
        }
        let Some(path) = self.service.path() else {
            self.error = Some(
                "Settings cannot be saved because the configuration directory is unavailable."
                    .into(),
            );
            cx.notify();
            return;
        };

        let draft = self.draft.clone();
        let service = self.service.clone();
        let parent = self.parent.clone();
        let handle = self.handle;
        self.saving = true;
        self.error = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let save_draft = draft.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    save_file(&path, &save_draft).map_err(|error| format!("{error:#}"))
                })
                .await;
            let close_handle = this.update(cx, |settings, cx| {
                settings.saving = false;
                match result {
                    Ok(()) => {
                        let old_settings = settings.saved.clone();
                        service.replace(draft.clone());
                        settings.saved = draft.clone();
                        settings.error = None;
                        settings.keyboard_notice = None;
                        match (
                            super::super::shortcuts::masked_bindings(&old_settings, &*cx),
                            super::super::shortcuts::bindings(&draft, &*cx),
                        ) {
                            (Ok(masked), Ok(bindings)) => {
                                cx.bind_keys(masked);
                                cx.bind_keys(bindings);
                            }
                            (Err(error), _) | (_, Err(error)) => {
                                tracing::error!(%error, "saved shortcut bindings could not be applied");
                                settings.error = Some(format!(
                                    "Settings were saved, but keyboard shortcuts could not be applied: {error}"
                                ));
                            }
                        }
                        let _ = parent.update_in(cx, |main, window, cx| {
                            main.apply_settings(draft.clone(), window, cx)
                        });
                        cx.notify();
                        close_after.then_some(handle).flatten()
                    }
                    Err(error) => {
                        settings.error = Some(format!("Could not save settings: {error}"));
                        cx.notify();
                        None
                    }
                }
            });
            if let Ok(Some(handle)) = close_handle {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        })
        .detach();
    }
}

fn restore_word_wrap_value(draft: &mut AppSettings, saved: &AppSettings) -> Option<bool> {
    if draft.word_wrap == saved.word_wrap {
        return None;
    }
    draft.word_wrap = saved.word_wrap;
    Some(saved.word_wrap)
}

pub(in crate::window) fn open(
    window: &mut Window,
    cx: &mut App,
    parent: WeakEntity<DopadWindow>,
    service: SettingsService,
) -> anyhow::Result<()> {
    let window_bounds = WindowBounds::centered(size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
    let options = WindowOptions {
        icon: crate::icon::window_icon(),
        window_bounds: Some(window_bounds),
        window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
        ..TitleBar::window_options()
    };
    let (handle, entity) = open_window(options, cx, |window, cx| {
        window.set_window_title("Settings");
        let entity = cx.new(|cx| SettingsWindow::new(service, parent, window, cx));
        let weak_entity = entity.downgrade();
        window.on_window_should_close(cx, move |_, app| {
            let Some(entity) = weak_entity.upgrade() else {
                return true;
            };
            app.update_entity(&entity, |settings, cx| {
                settings.restore_word_wrap_preview(cx)
            });
            true
        });
        entity
    })?;
    entity.update(cx, |settings, _| settings.handle = Some(handle));
    let _ = window;
    Ok(())
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let topbar = TitleBar::new()
            .h(px(44.))
            .w_full()
            .bg(rgb(theme::BG_TOPBAR))
            .border_b_1()
            .border_color(rgb(theme::BORDER))
            .child(
                div()
                    .h(px(44.))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px(px(18.))
                    .child(img(crate::icon::image()).size(px(22.)))
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(theme::TEXT_PRIMARY))
                            .child("Settings"),
                    ),
            );
        let body = div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .bg(rgb(theme::BG_PRIMARY))
            .child(sidebar::render(self, cx))
            .child(content::render(self, cx));

        let _ = window;
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme::BG_PRIMARY))
            .children([
                topbar.into_any_element(),
                body.into_any_element(),
                footer::render(self, cx).into_any_element(),
            ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_restores_word_wrap_preview_to_last_saved_value() {
        let saved = AppSettings::default();
        let mut draft = AppSettings {
            word_wrap: true,
            ..saved.clone()
        };

        assert_eq!(restore_word_wrap_value(&mut draft, &saved), Some(false));
        assert!(!draft.word_wrap);
        assert_eq!(restore_word_wrap_value(&mut draft, &saved), None);
    }
}
