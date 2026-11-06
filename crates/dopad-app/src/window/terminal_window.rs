use std::{path::PathBuf, sync::Arc};

use dopad_terminal::{
    PtySize, Shell, TerminalSession, TerminalSnapshot, default_shell, discover_shells,
};
use gpui_kit::component::TitleBar;
use gpui_kit::{
    AnyWindowHandle, AppContext, ClipboardItem, Context, Entity, FocusHandle, KeyDownEvent,
    ScrollHandle, ScrollStrategy, Subscription, UniformListScrollHandle, Window, WindowBounds,
    WindowOptions, font, open_window, px, size,
};

use super::DopadWindow;
use crate::settings::{AppSettings, SettingsService};

const WINDOW_WIDTH: f32 = 920.;
const WINDOW_HEIGHT: f32 = 560.;
const MIN_WINDOW_WIDTH: f32 = 560.;
const MIN_WINDOW_HEIGHT: f32 = 320.;
pub(super) const TERMINAL_TOPBAR_HEIGHT: f32 = 44.;
pub(super) const TERMINAL_FOOTER_HEIGHT: f32 = 30.;
pub(super) const TERMINAL_PADDING_X: f32 = 20.;
pub(super) const TERMINAL_PADDING_Y: f32 = 16.;
const TERMINAL_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/></svg>"#;

struct TerminalTab {
    id: u64,
    shell: Shell,
    cwd: PathBuf,
    session: Option<TerminalSession>,
    snapshot: Arc<TerminalSnapshot>,
    starting: bool,
}

pub(super) struct TerminalWindow {
    settings: SettingsService,
    cwd: PathBuf,
    shells: Vec<Shell>,
    tabs: Vec<TerminalTab>,
    active: usize,
    next_id: u64,
    discovering: bool,
    status: Option<String>,
    focus: FocusHandle,
    output_scroll: UniformListScrollHandle,
    tab_scroll: ScrollHandle,
    tab_layout: Option<(usize, usize, f32, f32)>,
    pty_size: PtySize,
    _resize_observer: Option<Subscription>,
    selection: Option<((usize, usize), (usize, usize))>,
    selecting: bool,
    auto_follow: bool,
}

pub(super) fn open(
    _parent_window: &mut Window,
    cx: &mut Context<DopadWindow>,
    settings: SettingsService,
    cwd: PathBuf,
) -> anyhow::Result<(AnyWindowHandle, Entity<TerminalWindow>)> {
    let bounds = WindowBounds::centered(size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
    let options = WindowOptions {
        icon: crate::icon::window_icon(),
        window_bounds: Some(bounds),
        window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
        ..TitleBar::window_options()
    };
    let (handle, view) = open_window(options, cx, move |window, cx| {
        window.set_window_title("Terminal");
        let view = cx.new(|cx| TerminalWindow::new(settings, cwd, cx));
        view.update(cx, |this, cx| {
            let bounds = window.bounds();
            let settings = this.settings.current();
            let (cell_width, line_height) = terminal_metrics(window, &settings);
            this.resize_active(
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
                cell_width,
                line_height,
            );
            this.focus.focus(window, cx);
            let settings = this.settings.clone();
            this._resize_observer =
                Some(cx.observe_window_bounds(window, move |this, window, _| {
                    let bounds = window.bounds();
                    let settings = settings.current();
                    let (cell_width, line_height) = terminal_metrics(window, &settings);
                    this.resize_active(
                        f32::from(bounds.size.width),
                        f32::from(bounds.size.height),
                        cell_width,
                        line_height,
                    );
                }));
            this.discover_and_start(cx);
        });
        view
    })?;
    Ok((handle, view))
}

impl TerminalWindow {
    fn new(settings: SettingsService, cwd: PathBuf, cx: &mut Context<Self>) -> Self {
        Self {
            settings,
            cwd,
            shells: Vec::new(),
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            discovering: true,
            status: None,
            focus: cx.focus_handle().tab_stop(true),
            output_scroll: UniformListScrollHandle::new(),
            tab_scroll: ScrollHandle::new(),
            tab_layout: None,
            pty_size: PtySize::default(),
            _resize_observer: None,
            selection: None,
            selecting: false,
            auto_follow: true,
        }
    }

    fn discover_and_start(&mut self, cx: &mut Context<Self>) {
        if !self.discovering {
            return;
        }
        self.discovering = false;
        let cwd = self.cwd.clone();
        let pty_size = self.pty_size;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let shells = discover_shells();
                    let shell = default_shell(&shells);
                    let session = shell.as_ref().map(|shell| {
                        TerminalSession::spawn(shell, &cwd, pty_size)
                            .map_err(|error| error.to_string())
                    });
                    (shells, shell, session)
                })
                .await;
            let _ = this.update(cx, |window, cx| {
                window.shells = result.0;
                match (result.1, result.2) {
                    (Some(shell), Some(Ok(session))) => {
                        window.attach_session(None, shell, session, cx)
                    }
                    (Some(shell), Some(Err(error))) => {
                        window.status = Some(format!("Could not start {}: {error}", shell.name))
                    }
                    _ => {
                        window.status = Some("No supported shell was found on this system.".into())
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn start_shell(&mut self, shell: Shell, cx: &mut Context<Self>) {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.tabs.push(TerminalTab {
            id,
            shell: shell.clone(),
            cwd: self.cwd.clone(),
            session: None,
            snapshot: Arc::new(TerminalSnapshot::default()),
            starting: true,
        });
        self.active = self.tabs.len().saturating_sub(1);
        self.auto_follow = true;
        self.status = None;
        let cwd = self.cwd.clone();
        let pty_size = self.pty_size;
        let spawn_shell = shell.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    TerminalSession::spawn(&spawn_shell, &cwd, pty_size)
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |window, cx| match result {
                Ok(session) => window.attach_session(Some(id), shell, session, cx),
                Err(error) => {
                    if let Some(tab) = window.tabs.iter_mut().find(|tab| tab.id == id) {
                        tab.starting = false;
                    }
                    window.status = Some(format!("Could not start {}: {error}", shell.name));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn attach_session(
        &mut self,
        tab_id: Option<u64>,
        shell: Shell,
        session: TerminalSession,
        cx: &mut Context<Self>,
    ) {
        let id = tab_id.unwrap_or_else(|| {
            let id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);
            self.tabs.push(TerminalTab {
                id,
                shell: shell.clone(),
                cwd: self.cwd.clone(),
                session: None,
                snapshot: Arc::new(TerminalSnapshot::default()),
                starting: true,
            });
            self.active = self.tabs.len().saturating_sub(1);
            id
        });
        let updates = session.updates();
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.snapshot = session.snapshot();
            tab.session = Some(session);
            tab.starting = false;
        }
        let entity = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            while updates.recv().await.is_ok() {
                let result = entity.update(cx, |window, cx| {
                    let is_active = window
                        .tabs
                        .get(window.active)
                        .is_some_and(|tab| tab.id == id);
                    if let Some(tab) = window.tabs.iter_mut().find(|tab| tab.id == id) {
                        if let Some(session) = tab.session.as_ref() {
                            tab.snapshot = session.snapshot();
                        }
                        if is_active && window.auto_follow && !tab.snapshot.lines.is_empty() {
                            window.output_scroll.scroll_to_item(
                                tab.snapshot.lines.len().saturating_sub(1),
                                ScrollStrategy::Bottom,
                            );
                        }
                    }
                    cx.notify();
                });
                if result.is_err() {
                    break;
                }
            }
        })
        .detach();
        self.status = None;
        cx.notify();
    }

    fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            let was_active = self.active == index;
            self.tabs.remove(index);
            if was_active {
                self.active = index.min(self.tabs.len().saturating_sub(1));
            } else if index < self.active {
                self.active = self.active.saturating_sub(1);
            }
            if let Some(session) = self
                .tabs
                .get(self.active)
                .and_then(|tab| tab.session.as_ref())
            {
                let _ = session.resize(self.pty_size);
            }
            self.auto_follow = true;
            if let Some(last) = self
                .tabs
                .get(self.active)
                .map(|tab| tab.snapshot.lines.len().saturating_sub(1))
            {
                self.output_scroll
                    .scroll_to_item(last, ScrollStrategy::Bottom);
            }
            if self.tabs.is_empty()
                && let Some(shell) = self.shells.first().cloned()
            {
                self.start_shell(shell, cx);
            }
            cx.notify();
        }
    }

    fn activate_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.active = index;
        self.auto_follow = true;
        if let Some(tab) = self.tabs.get(index) {
            if let Some(session) = tab.session.as_ref() {
                let _ = session.resize(self.pty_size);
            }
            self.output_scroll.scroll_to_item(
                tab.snapshot.lines.len().saturating_sub(1),
                ScrollStrategy::Bottom,
            );
        }
        cx.notify();
    }

    fn send_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let Some(session) = tab.session.as_ref() else {
            return;
        };
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        if event.prefer_character_input {
            if let Some(text) = event.keystroke.key_char.as_ref() {
                let _ = session.input(text.as_bytes().to_vec());
            }
            return;
        }
        if modifiers.shift && key == "pageup" {
            self.output_scroll.scroll_to_item(0, ScrollStrategy::Top);
            self.auto_follow = false;
            return;
        }
        if modifiers.shift && key == "pagedown" {
            self.auto_follow = true;
            let last = self
                .tabs
                .get(self.active)
                .map_or(0, |tab| tab.snapshot.lines.len().saturating_sub(1));
            self.output_scroll
                .scroll_to_item(last, ScrollStrategy::Bottom);
            return;
        }
        if key == "end" && modifiers.control {
            self.auto_follow = true;
            let last = self
                .tabs
                .get(self.active)
                .map_or(0, |tab| tab.snapshot.lines.len().saturating_sub(1));
            self.output_scroll
                .scroll_to_item(last, ScrollStrategy::Bottom);
            return;
        }
        if modifiers.control || modifiers.platform {
            if modifiers.shift && matches!(key, "c" | "C") {
                if let Some(text) = output::selection_text(&tab.snapshot, self.selection) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                return;
            }
            if modifiers.shift && matches!(key, "v" | "V") {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let _ = session.input(text.into_bytes());
                }
                return;
            }
            if !modifiers.shift && matches!(key, "v" | "V") {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let _ = session.input(text.into_bytes());
                }
                return;
            }
            if let Some(byte) = control_byte(key) {
                if byte == 3
                    && let Some(text) = output::selection_text(&tab.snapshot, self.selection)
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    return;
                }
                let _ = session.input(vec![byte]);
                return;
            }
        }
        let sequence: Option<&[u8]> = match key {
            "enter" => Some(b"\r"),
            "backspace" => Some(b"\x7f"),
            "tab" => Some(b"\t"),
            "escape" => Some(b"\x1b"),
            "up" => Some(b"\x1b[A"),
            "down" => Some(b"\x1b[B"),
            "right" => Some(b"\x1b[C"),
            "left" => Some(b"\x1b[D"),
            "home" => Some(b"\x1b[H"),
            "end" => Some(b"\x1b[F"),
            "delete" => Some(b"\x1b[3~"),
            "pageup" => Some(b"\x1b[5~"),
            "pagedown" => Some(b"\x1b[6~"),
            _ => None,
        };
        if let Some(sequence) = sequence {
            let _ = session.input(sequence.to_vec());
        } else if !modifiers.alt
            && !modifiers.control
            && !modifiers.platform
            && let Some(text) = event.keystroke.key_char.as_ref()
        {
            let _ = session.input(text.as_bytes().to_vec());
        }
    }

    pub(super) fn resize_active(
        &mut self,
        width: f32,
        height: f32,
        cell_width: f32,
        line_height: f32,
    ) {
        let columns = ((width - TERMINAL_PADDING_X * 2.) / cell_width.max(1.))
            .floor()
            .clamp(2., 500.) as u16;
        let rows =
            ((height - TERMINAL_TOPBAR_HEIGHT - TERMINAL_FOOTER_HEIGHT - TERMINAL_PADDING_Y * 2.)
                / line_height.max(1.))
            .floor()
            .clamp(1., 300.) as u16;
        let size = PtySize {
            columns,
            rows,
            pixel_width: cell_width.round().clamp(1., f32::from(u16::MAX)) as u16,
            pixel_height: line_height.round().clamp(1., f32::from(u16::MAX)) as u16,
        };
        if self.pty_size == size {
            return;
        }
        self.pty_size = size;
        if let Some(session) = self
            .tabs
            .get(self.active)
            .and_then(|tab| tab.session.as_ref())
        {
            let _ = session.resize(size);
        }
    }

    pub(super) fn settings_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bounds = window.bounds();
        let settings = self.settings.current();
        let (cell_width, line_height) = terminal_metrics(window, &settings);
        self.resize_active(
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
            cell_width,
            line_height,
        );
        cx.notify();
    }
}

pub(super) fn terminal_metrics(window: &Window, settings: &AppSettings) -> (f32, f32) {
    let text_system = window.text_system();
    let font_id = text_system.resolve_font(&font(&settings.terminal_font_family));
    let font_size = f32::from(settings.terminal_font_size);
    let natural_width = f32::from(text_system.layout_width(font_id, px(font_size), 'm'));
    (
        scaled_cell_width(natural_width, settings.terminal_cell_width),
        terminal_line_height(settings.terminal_font_size, settings.terminal_line_height),
    )
}

fn scaled_cell_width(natural_width: f32, scale: u8) -> f32 {
    (natural_width * f32::from(scale) / 100.).max(1.)
}

pub(super) fn terminal_line_height(font_size: u8, scale: u8) -> f32 {
    (f32::from(font_size) * f32::from(scale) / 100.).max(f32::from(font_size))
}

fn control_byte(key: &str) -> Option<u8> {
    let character = key.chars().next()?.to_ascii_lowercase();
    character
        .is_ascii_lowercase()
        .then_some(character as u8 - b'a' + 1)
}

#[path = "terminal_window/output.rs"]
mod output;
#[path = "terminal_window/render.rs"]
mod render;
#[cfg(test)]
#[path = "terminal_window/tests.rs"]
mod tests;
