mod output_view;
mod problems;

use problems::ProblemRow;

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    input::{InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::{
    Anchor, AppContext, Context, Entity, IntoElement, ParentElement, Styled, WeakEntity, Window,
    rgb,
};

use super::{DopadWindow, diagnostics, terminal_window};
use crate::{output, theme::Palette};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BottomPanelView {
    Terminal,
    Problems,
    Output,
}

impl BottomPanelView {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Problems => "Problems",
            Self::Output => "Output",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SeverityFilter {
    #[default]
    All,
    Errors,
    Warnings,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProblemsRequestKey {
    revision: u64,
    workspace: Option<PathBuf>,
    filter: String,
    severity: SeverityFilter,
    collapsed: HashSet<PathBuf>,
}

#[derive(Default)]
struct ProblemsState {
    filter: Option<Entity<InputState>>,
    filter_text: String,
    severity: SeverityFilter,
    collapsed: HashSet<PathBuf>,
    needs_refresh: bool,
    cached_key: Option<ProblemsRequestKey>,
    pending_key: Option<ProblemsRequestKey>,
    request_generation: Arc<AtomicU64>,
    cached_rows: Arc<[ProblemRow]>,
}

struct OutputState {
    channel: Option<output::OutputChannel>,
    revision: u64,
    entries: Arc<[output::OutputEntry]>,
    scroll: gpui_kit::UniformListScrollHandle,
    selected_entry: Option<u64>,
    auto_follow: bool,
    poll_generation: u64,
}

impl Default for OutputState {
    fn default() -> Self {
        Self {
            channel: None,
            revision: 0,
            entries: Arc::default(),
            scroll: gpui_kit::UniformListScrollHandle::new(),
            selected_entry: None,
            auto_follow: true,
            poll_generation: 0,
        }
    }
}

pub(super) struct BottomPanelState {
    pub(super) active: Option<BottomPanelView>,
    pub(super) height: f32,
    problems: ProblemsState,
    output: OutputState,
}

impl Default for BottomPanelState {
    fn default() -> Self {
        Self {
            active: None,
            height: terminal_window::TERMINAL_PANEL_DEFAULT_HEIGHT,
            problems: ProblemsState::default(),
            output: OutputState::default(),
        }
    }
}

impl BottomPanelState {
    pub(super) fn is_active(&self, view: BottomPanelView) -> bool {
        self.active == Some(view)
    }

    pub(super) fn invalidate_problems(&mut self) {
        self.problems.needs_refresh = true;
        self.problems.pending_key = None;
        self.problems
            .request_generation
            .fetch_add(1, Ordering::Relaxed);
    }
}

pub(super) fn view_selector(
    parent: WeakEntity<DopadWindow>,
    active: BottomPanelView,
    palette: Palette,
    id: &'static str,
) -> impl IntoElement + use<> {
    Button::new(id)
        .child(active.label())
        .icon(IconName::ChevronDown)
        .xsmall()
        .compact()
        .ghost()
        .text_color(rgb(palette.secondary))
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |mut menu, _, _| {
            for view in [
                BottomPanelView::Terminal,
                BottomPanelView::Problems,
                BottomPanelView::Output,
            ] {
                let parent = parent.clone();
                let item = PopupMenuItem::new(view.label()).on_click(move |_, _, cx| {
                    let _ = parent.update(cx, |window, cx| window.set_bottom_panel_view(view, cx));
                });
                menu = menu.item(item);
            }
            menu
        })
}

pub(super) fn render_problems(
    window: &mut DopadWindow,
    palette: Palette,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    problems::render(window, palette, cx)
}

pub(super) fn render_output(
    window: &mut DopadWindow,
    palette: Palette,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    output_view::render(window, palette, cx)
}

impl DopadWindow {
    pub(super) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_bottom_panel(BottomPanelView::Terminal, window, cx);
    }

    pub(super) fn toggle_problems(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_bottom_panel(BottomPanelView::Problems, window, cx);
    }

    pub(super) fn set_bottom_panel_view(&mut self, view: BottomPanelView, cx: &mut Context<Self>) {
        if self.bottom_panel.active == Some(view) {
            return;
        }
        self.open_bottom_panel(view, None, cx);
    }

    fn toggle_bottom_panel(
        &mut self,
        view: BottomPanelView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.bottom_panel.active == Some(view) {
            self.hide_bottom_panel(window, cx);
        } else {
            self.open_bottom_panel(view, Some(window), cx);
        }
    }

    fn open_bottom_panel(
        &mut self,
        view: BottomPanelView,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        self.bottom_panel.active = Some(view);
        if self.bottom_panel.height == 0. {
            self.bottom_panel.height = terminal_window::TERMINAL_PANEL_DEFAULT_HEIGHT;
        }
        match view {
            BottomPanelView::Terminal => self.ensure_terminal(window, cx),
            BottomPanelView::Problems => self.ensure_problem_filter(window, cx),
            BottomPanelView::Output => self.open_output(),
        }
        cx.notify();
        if view == BottomPanelView::Output {
            self.start_output_poll(cx);
        }
    }

    pub(super) fn ensure_terminal(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let Some(window) = window else {
            return;
        };
        let bounds = window.bounds();
        let settings = self.settings.current();
        self.bottom_panel.height = terminal_window::clamp_panel_height(
            self.bottom_panel.height,
            f32::from(bounds.size.height),
            settings.show_status_bar,
        );
        let sidebar_width = (if self.sidebar.mode.is_visible() {
            48.
        } else {
            0.
        }) + if self.sidebar.mode.has_panel() {
            self.sidebar.width
        } else {
            0.
        };
        let panel_width = (f32::from(bounds.size.width) - sidebar_width).max(1.);
        let panel_height = self.bottom_panel.height;
        let is_new = self.terminal_panel.is_none();
        if is_new {
            let cwd = super::sidebar::root_folder(&self.sidebar)
                .or_else(|| {
                    self.active_tab()
                        .and_then(|tab| tab.path.as_deref()?.parent())
                        .map(PathBuf::from)
                })
                .filter(|path| path.is_dir())
                .or_else(|| {
                    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                        .map(PathBuf::from)
                })
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let settings = self.settings.clone();
            let parent = cx.entity().downgrade();
            self.terminal_panel =
                Some(cx.new(|cx| terminal_window::TerminalWindow::new(settings, cwd, parent, cx)));
        }
        if let Some(terminal) = self.terminal_panel.as_ref().cloned() {
            terminal.update(cx, |terminal, cx| {
                terminal.configure_for_panel(panel_width, panel_height, window);
                if is_new {
                    terminal.discover_and_start(cx);
                }
            });
            window.on_next_frame(move |window, cx| {
                terminal.update(cx, |terminal, cx| terminal.focus(window, cx));
            });
        }
    }

    pub(super) fn ensure_problem_filter(
        &mut self,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if self.bottom_panel.problems.filter.is_some() {
            return;
        }
        let Some(window) = window else {
            return;
        };
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter problems..."));
        cx.subscribe(&filter, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.bottom_panel.problems.filter_text = this
                    .bottom_panel
                    .problems
                    .filter
                    .as_ref()
                    .map(|filter| filter.read(cx).value().to_string())
                    .unwrap_or_default();
                this.bottom_panel.invalidate_problems();
                cx.notify();
            }
        })
        .detach();
        self.bottom_panel.problems.filter = Some(filter);
        self.bottom_panel.invalidate_problems();
    }

    fn open_output(&mut self) {
        let channel = self
            .bottom_panel
            .output
            .channel
            .unwrap_or(output::OutputChannel::General);
        self.refresh_output_snapshot(channel);
        self.bottom_panel.output.poll_generation =
            self.bottom_panel.output.poll_generation.wrapping_add(1);
    }

    fn refresh_output_snapshot(&mut self, channel: output::OutputChannel) {
        let channel_changed = self
            .bottom_panel
            .output
            .channel
            .is_some_and(|current| current != channel);
        let was_at_end = self
            .bottom_panel
            .output
            .scroll
            .is_scrolled_to_end()
            .unwrap_or(true);
        let (revision, entries) = output::snapshot(channel);
        let state = &mut self.bottom_panel.output;
        state.channel = Some(channel);
        state.revision = revision;
        state.entries = entries.into();
        if channel_changed {
            state.selected_entry = None;
        }
        state.auto_follow = was_at_end;
        if state.auto_follow && !state.entries.is_empty() {
            state
                .scroll
                .scroll_to_item(state.entries.len() - 1, gpui_kit::ScrollStrategy::Bottom);
        }
    }

    fn start_output_poll(&mut self, cx: &mut Context<Self>) {
        let generation = self.bottom_panel.output.poll_generation;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let continue_polling = this
                    .update(cx, |window, cx| {
                        if !window.bottom_panel.is_active(BottomPanelView::Output)
                            || window.bottom_panel.output.poll_generation != generation
                        {
                            return false;
                        }
                        let channel = window
                            .bottom_panel
                            .output
                            .channel
                            .unwrap_or(output::OutputChannel::General);
                        if output::revision(channel) != window.bottom_panel.output.revision {
                            window.refresh_output_snapshot(channel);
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !continue_polling {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn hide_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.bottom_panel.active == Some(BottomPanelView::Terminal) {
            self.hide_bottom_panel(window, cx);
        }
    }

    pub(super) fn hide_bottom_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.bottom_panel.active.take().is_none() {
            return;
        }
        self.bottom_panel.output.poll_generation =
            self.bottom_panel.output.poll_generation.wrapping_add(1);
        if let Some(editor) = self.active_tab().and_then(|tab| tab.ui.as_ref()).cloned() {
            editor.update(cx, |editor, cx| editor.focus(window, cx));
        }
        cx.notify();
    }

    pub(super) fn current_diagnostic_workspace(&self) -> Option<PathBuf> {
        self.active_tab()
            .and_then(|tab| self.lsp.active_workspace(tab.id))
            .map(PathBuf::from)
    }

    pub(super) fn current_diagnostic_counts(&self) -> diagnostics::DiagnosticCounts {
        self.diagnostics
            .counts(self.current_diagnostic_workspace().as_ref())
    }

    fn select_output_channel(&mut self, channel: output::OutputChannel, cx: &mut Context<Self>) {
        if self.bottom_panel.output.channel == Some(channel) {
            return;
        }
        self.refresh_output_snapshot(channel);
        cx.notify();
    }

    fn clear_output_channel(&mut self, cx: &mut Context<Self>) {
        let channel = self
            .bottom_panel
            .output
            .channel
            .unwrap_or(output::OutputChannel::General);
        output::clear(channel);
        self.refresh_output_snapshot(channel);
        cx.notify();
    }

    fn copy_selected_output(&self, cx: &mut Context<Self>) {
        let selected = self.bottom_panel.output.selected_entry;
        let Some(line) = self
            .bottom_panel
            .output
            .entries
            .iter()
            .find(|entry| Some(entry.sequence) == selected)
            .map(output::OutputEntry::line)
        else {
            return;
        };
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(line));
    }
}
