mod body;
mod status;

use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div, px, rgb,
};

use super::{
    DopadWindow, ResetZoom, TogglePreview, ToggleWrap, ZoomIn, ZoomOut, session_state,
    terminal_window,
};
use crate::theme;

impl Render for DopadWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        session_state::start_bootstrap(self, cx);
        session_state::show_recovery_prompt(self, window, cx);
        self.sidebar.width = f32::from(self.settings.current().sidebar_width);
        if self.tabs.iter().any(|tab| tab.path.is_some()) {
            super::sidebar::ensure_file_icon_theme(self, cx);
        }
        let file_drop_listener = cx.listener(|this, event: &gpui_kit::FileDropEvent, _, cx| {
            this.handle_file_drop(event, cx);
        });
        self.ensure_active_ui(window, cx);
        self.ensure_active_lsp(cx);
        self.ensure_preview(cx);
        self.ensure_active_image(cx);

        let palette = theme::palette(self.dark_theme);
        super::toast::push_current_status(self, window, palette, cx);
        let body = body::render(self, window, palette, cx);
        let topbar = super::topbar::render(self, window, palette, cx).into_any_element();
        let status = status::render(self, body.state.preview_mode, window, palette, cx);
        let notifications = self.notifications.clone();
        let lsp_toast = super::lsp::render_install_toast(self, palette, cx);
        let sidebar_mode = self.sidebar.mode;
        let bounds = window.bounds();
        let show_status_bar = self.settings.current().show_status_bar;
        let max_terminal_height =
            terminal_window::max_panel_height(f32::from(bounds.size.height), show_status_bar);
        self.terminal_height = self.terminal_height.clamp(
            terminal_window::TERMINAL_PANEL_MIN_HEIGHT,
            max_terminal_height,
        );
        let left_width = (if sidebar_mode.is_visible() { 48. } else { 0. })
            + if sidebar_mode.has_panel() {
                self.sidebar.width
            } else {
                0.
            };
        let terminal_width = (f32::from(bounds.size.width) - left_width).max(1.);
        if self.terminal_visible
            && let Some(terminal) = self.terminal_panel.as_ref().cloned()
        {
            let terminal_height = self.terminal_height;
            terminal.update(cx, |terminal, _| {
                terminal.configure_for_panel(terminal_width, terminal_height, window)
            });
        }

        let mut editor_region = body.element;
        if self.terminal_visible
            && let Some(terminal) = self.terminal_panel.as_ref().cloned()
        {
            let terminal_height = self.terminal_height;
            let terminal_min = terminal_window::TERMINAL_PANEL_MIN_HEIGHT;
            let terminal_max = max_terminal_height;
            let view = cx.entity();
            editor_region = v_resizable("dopad-editor-terminal-panels")
                .child(resizable_panel().child(editor_region))
                .child(
                    resizable_panel()
                        .size(px(terminal_height))
                        .size_range(px(terminal_min)..px(terminal_max))
                        .flex_none()
                        .child(terminal),
                )
                .on_resize(move |state, _, app| {
                    let Some(size) = state.read(app).sizes().last().copied() else {
                        return;
                    };
                    let height = f32::from(size).round().clamp(terminal_min, terminal_max);
                    view.update(app, |this, cx| {
                        if (this.terminal_height - height).abs() > 0.5 {
                            this.terminal_height = height;
                            cx.notify();
                        }
                    });
                })
                .into_any_element();
        }

        let body_region = if !sidebar_mode.is_visible() {
            editor_region
        } else {
            let rail = super::sidebar::render_navigation_rail(self, palette, cx);
            let workspace = if sidebar_mode.has_panel() {
                let sidebar = super::sidebar::render(self, window, palette, cx);
                let settings = self.settings.clone();
                let view = cx.entity();
                h_resizable("dopad-workspace-panels")
                    .child(
                        resizable_panel()
                            .size(px(self.sidebar.width))
                            .size_range(px(220.)..px(420.))
                            .flex_none()
                            .child(sidebar),
                    )
                    .child(resizable_panel().child(editor_region))
                    .on_resize(move |state, _, app| {
                        let Some(size) = state.read(app).sizes().first().copied() else {
                            return;
                        };
                        let width = f32::from(size).round().clamp(220., 420.) as u16;
                        let mut current = settings.current();
                        if current.sidebar_width == width {
                            return;
                        }
                        current.sidebar_width = width;
                        settings.replace(current.clone());
                        if let Some(path) = settings.path() {
                            app.background_executor()
                                .spawn(async move {
                                    if let Err(error) = crate::settings::save_file(&path, &current) {
                                        tracing::warn!(error = %error, "sidebar width could not be persisted");
                                    }
                                })
                                .detach();
                        }
                        view.update(app, |_, cx| cx.notify());
                    })
                    .into_any_element()
            } else {
                editor_region
            };
            div()
                .size_full()
                .min_w_0()
                .flex()
                .child(rail)
                .child(div().flex_1().min_w_0().size_full().child(workspace))
                .into_any_element()
        };
        let mut content = vec![
            topbar,
            div()
                .flex_1()
                .min_h_0()
                .child(body_region)
                .into_any_element(),
        ];
        if self.settings.current().show_status_bar {
            content.push(status);
        }
        session_state::schedule_checkpoint_if_changed(self, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .bg(rgb(palette.canvas))
            .on_action(cx.listener(|this, _: &super::NewDocument, _, cx| this.add_blank(cx)))
            .on_action(cx.listener(|this, _: &super::OpenDocument, _, cx| this.open_dialog(cx)))
            .on_action(
                cx.listener(|this, _: &super::OpenFolder, _, cx| this.open_folder_dialog(cx)),
            )
            .on_action(
                cx.listener(|this, _: &super::SaveDocument, _, cx| {
                    this.save_active(false, None, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &super::SaveAsDocument, _, cx| {
                this.save_active(true, None, cx)
            }))
            .on_action(cx.listener(|this, _: &super::CloseDocument, window, cx| {
                this.request_close(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &super::Find, window, cx| {
                    this.open_search(false, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &super::Replace, window, cx| {
                this.open_search(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &super::FindNext, window, cx| {
                this.find_action(super::shortcuts::FindUiAction::Next, window, cx)
            }))
            .on_action(cx.listener(|this, _: &super::FindPrevious, window, cx| {
                this.find_action(super::shortcuts::FindUiAction::Previous, window, cx)
            }))
            .on_action(cx.listener(|this, _: &super::ToggleFindCase, window, cx| {
                this.find_action(super::shortcuts::FindUiAction::Case, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &super::ToggleFindWholeWord, window, cx| {
                    this.find_action(super::shortcuts::FindUiAction::WholeWord, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &super::ToggleFindRegex, window, cx| {
                this.find_action(super::shortcuts::FindUiAction::Regex, window, cx)
            }))
            .on_action(cx.listener(|this, _: &super::FindSelection, window, cx| {
                this.find_action(super::shortcuts::FindUiAction::Selection, window, cx)
            }))
            .on_action(cx.listener(|this, _: &super::AddNextMatch, _, cx| this.add_next_match(cx)))
            .on_action(cx.listener(|this, _: &super::CloseFind, window, cx| {
                this.close_search_widget(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleWrap, window, cx| this.toggle_wrap(window, cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.adjust_zoom(1.0, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.adjust_zoom(-1.0, cx)))
            .on_action(cx.listener(|this, _: &ResetZoom, _, cx| this.reset_zoom(cx)))
            .on_action(cx.listener(|this, _: &TogglePreview, _, cx| this.toggle_preview(cx)))
            .on_action(cx.listener(|this, _: &super::GoToLine, window, cx| {
                if this.active_tab().is_some_and(|tab| tab.editor.is_some()) {
                    super::dialogs::go_to_line(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &super::DuplicateLine, window, cx| {
                this.duplicate_line(window, cx)
            }))
            .on_action(cx.listener(|this, _: &super::ToggleComment, window, cx| {
                this.toggle_comment(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &super::MoveLineUp, window, cx| {
                    this.move_line(-1, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &super::MoveLineDown, window, cx| {
                this.move_line(1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &super::FormatDocument, window, cx| {
                this.format_document(window, cx)
            }))
            .on_action(cx.listener(|this, _: &super::OpenSettings, window, cx| {
                let parent = cx.entity().downgrade();
                if let Err(error) = super::settings::open(window, cx, parent, this.settings.clone())
                {
                    tracing::error!(error = %error, "failed to open Settings");
                    this.set_status(format!("Could not open Settings: {error:#}"));
                    cx.notify();
                }
            }))
            .children(content)
            .child(
                div()
                    .absolute()
                    .size_0()
                    .child(super::file_drop::Capture::new(move |event, window, app| {
                        file_drop_listener(event, window, app)
                    })),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .child(notifications),
            )
            .child(lsp_toast)
    }
}

impl DopadWindow {
    fn handle_file_drop(&mut self, event: &gpui_kit::FileDropEvent, cx: &mut Context<Self>) {
        match event {
            gpui_kit::FileDropEvent::Entered { paths, .. } => {
                self.pending_drop_paths = paths.paths().to_vec();
            }
            gpui_kit::FileDropEvent::Submit { .. } => {
                let paths = std::mem::take(&mut self.pending_drop_paths);
                self.open_paths(paths, cx);
            }
            gpui_kit::FileDropEvent::Ended => self.pending_drop_paths.clear(),
            gpui_kit::FileDropEvent::Pending { .. } | gpui_kit::FileDropEvent::Exited => {}
        }
    }
}
