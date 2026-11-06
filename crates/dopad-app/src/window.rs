use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

use crate::error::AppResult;
use crate::session::SessionCoordinator;
use crate::settings::{AppSettings, SettingsService};
use dopad_core::{ByteOffset, ByteRange, Revision, TextPosition, detect_eol};
use gpui_kit::component::input::{EditorState, TabSize};
use gpui_kit::component::notification::NotificationList;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Context, Entity, ListAlignment, ListState, Window,
    WindowBounds, WindowOptions, open_window, px, size,
};

mod background;
mod bracket;
mod cli;
mod dialogs;
mod document_tab;
mod editing;
mod editor_ui;
mod file_drop;
mod file_search;
mod file_view;
mod find_widget;
mod fonts;
mod formatting;
mod image_preview;
mod language;
mod menu;
mod open;
mod paths;
mod preview;
mod preview_view;
mod save;
mod search;
mod session_state;
mod settings;
mod shortcuts;
mod sidebar;
mod sync;
#[path = "window/terminal_window.rs"]
mod terminal_window;
mod toast;
mod topbar;
mod unsupported_file;
mod update_window;
mod view;
use document_tab::DocumentTab;
use save::apply_document_defaults;
use sync::{apply_minimal_diff, local_change_range, update_eol_after_edit};

gpui_kit::actions!(
    dopad,
    [
        NewDocument,
        OpenDocument,
        OpenFolder,
        SaveDocument,
        SaveAsDocument,
        CloseDocument,
        Find,
        Replace,
        ToggleWrap,
        ZoomIn,
        ZoomOut,
        ResetZoom,
        TogglePreview,
        OpenSettings,
        GoToLine,
        DuplicateLine,
        ToggleComment,
        MoveLineUp,
        MoveLineDown,
        FormatDocument,
        FindNext,
        FindPrevious,
        ToggleFindCase,
        ToggleFindWholeWord,
        ToggleFindRegex,
        FindSelection,
        AddNextMatch,
        CloseFind,
        IgnoreShortcut,
    ]
);

const WINDOW_WIDTH: f32 = 1000.0;
const WINDOW_HEIGHT: f32 = 650.0;
const MIN_WINDOW_WIDTH: f32 = 560.0;
const MIN_WINDOW_HEIGHT: f32 = 320.0;

pub fn open_main(
    cx: &mut App,
    session: SessionCoordinator,
    settings: SettingsService,
    updater: Entity<crate::updater::UpdateManager>,
    launch_paths: Vec<PathBuf>,
) -> AppResult<MainWindow> {
    let current_settings = settings.current();
    cx.bind_keys(shortcuts::bindings(&current_settings, cx)?);
    let window_bounds = WindowBounds::centered(size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
    let options = WindowOptions {
        icon: crate::icon::window_icon(),
        window_bounds: Some(window_bounds),
        window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
        ..gpui_kit::component::TitleBar::window_options()
    };

    let (handle, view) = open_window(options, cx, move |window, cx| {
        window.set_window_title("Dopad");
        window.set_rem_size(px(f32::from(settings.current().ui_font_size)));
        let notifications = cx.new(|cx| NotificationList::new(window, cx));
        let view = cx.new(|cx| {
            DopadWindow::new(
                session.clone(),
                settings.clone(),
                updater.clone(),
                launch_paths.clone(),
                notifications,
                cx,
            )
        });
        session_state::install_close_guard(window, cx, &view);
        view.update(cx, |state, cx| state.start_auto_save_loop(cx));
        if let Some(data_dir) = crate::settings::app_data_dir() {
            view.update(cx, |_, cx| {
                fonts::load_cached_selected_font(
                    settings.current().editor_font_family,
                    data_dir.clone(),
                    cx,
                );
                let terminal_family = settings.current().terminal_font_family;
                if terminal_family != settings.current().editor_font_family {
                    fonts::load_cached_selected_font(terminal_family, data_dir, cx);
                }
            });
        }
        view
    })?;
    Ok(MainWindow { handle, view })
}

pub(crate) struct MainWindow {
    pub handle: AnyWindowHandle,
    pub view: Entity<DopadWindow>,
}

pub(crate) struct DopadWindow {
    pub tabs: Vec<DocumentTab>,
    pub active: usize,
    next_id: u64,
    pub dark_theme: bool,
    pub status: String,
    pub status_generation: u64,
    pub toast_generation: u64,
    pub last_toast_message: Option<String>,
    pub notifications: Entity<NotificationList>,
    pub(crate) updater: Entity<crate::updater::UpdateManager>,
    pub preview_cache: Option<dopad_preview::PreviewResult>,
    pub preview_list: ListState,
    pub preview_error: Option<(u64, Revision, String)>,
    pub preview_pending: Option<(u64, Revision, u64)>,
    pub preview_generation: u64,
    pub preview_cancel: Option<Arc<AtomicBool>>,
    image_cache: image_preview::DecodedImageCache,
    pub pending_file_loads: HashSet<u64>,
    pending_formats: HashSet<u64>,
    pub pending_loading_indicators: HashMap<u64, Arc<AtomicBool>>,
    pending_highlighters: HashSet<(u64, String)>,
    cli_open_states: HashMap<u64, cli::CliOpenState>,
    pending_cli_requests: Vec<crate::cli_ipc::UiMessage>,
    session: SessionCoordinator,
    settings: SettingsService,
    session_bootstrap_started: bool,
    session_bootstrap_complete: bool,
    session_generation: u64,
    session_signature: Option<session_state::SessionSignature>,
    session_status: Option<String>,
    window_close_pending: bool,
    pending_window_close: Option<session_state::PendingWindowClose>,
    recovery_prompt: Option<crate::session::SessionBootstrap>,
    recovery_prompt_open: bool,
    pending_drop_paths: Vec<PathBuf>,
    pending_startup_paths: Vec<PathBuf>,
    search_task: search::SearchTaskState,
    find_widget: Option<Entity<find_widget::FindWidget>>,
    sidebar: sidebar::SidebarState,
    pending_search_navigation: Option<(
        PathBuf,
        TextPosition,
        usize,
        Option<file_search::DiskFileIdentity>,
    )>,
    bracket_task: bracket::BracketTaskState,
    suppress_ui_sync: Option<u64>,
    last_auto_save: Instant,
    last_external_check: Instant,
    terminal_window: Option<(AnyWindowHandle, Entity<terminal_window::TerminalWindow>)>,
}

impl DopadWindow {
    pub(super) fn set_status(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.status_generation = self.status_generation.wrapping_add(1);
    }

    pub(super) fn clear_status(&mut self) {
        self.status.clear();
        self.status_generation = self.status_generation.wrapping_add(1);
    }

    fn new(
        session: SessionCoordinator,
        settings: SettingsService,
        updater: Entity<crate::updater::UpdateManager>,
        launch_paths: Vec<PathBuf>,
        notifications: Entity<NotificationList>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut first_tab = DocumentTab::blank(1);
        apply_document_defaults(&mut first_tab, &settings.current());
        let sidebar_width = settings.current().sidebar_width;
        cx.observe(&updater, |_, _, cx| cx.notify()).detach();
        Self {
            tabs: vec![first_tab],
            active: 0,
            next_id: 2,
            dark_theme: false,
            status: String::new(),
            status_generation: 0,
            toast_generation: 0,
            last_toast_message: None,
            notifications,
            updater,
            preview_cache: None,
            preview_list: ListState::new(0, ListAlignment::Top, px(240.)),
            preview_error: None,
            preview_pending: None,
            preview_generation: 0,
            preview_cancel: None,
            image_cache: image_preview::DecodedImageCache::default(),
            pending_file_loads: HashSet::new(),
            pending_formats: HashSet::new(),
            pending_loading_indicators: HashMap::new(),
            pending_highlighters: HashSet::new(),
            cli_open_states: HashMap::new(),
            pending_cli_requests: Vec::new(),
            session,
            settings,
            session_bootstrap_started: false,
            session_bootstrap_complete: false,
            session_generation: 0,
            session_signature: None,
            session_status: None,
            window_close_pending: false,
            pending_window_close: None,
            recovery_prompt: None,
            recovery_prompt_open: false,
            pending_drop_paths: Vec::new(),
            pending_startup_paths: launch_paths,
            search_task: search::SearchTaskState::default(),
            find_widget: None,
            sidebar: sidebar::SidebarState::new(sidebar_width),
            pending_search_navigation: None,
            bracket_task: bracket::BracketTaskState::default(),
            suppress_ui_sync: None,
            last_auto_save: Instant::now(),
            last_external_check: Instant::now(),
            terminal_window: None,
        }
    }

    pub(super) fn open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((handle, _)) = self.terminal_window.as_ref() {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.terminal_window = None;
        }

        let cwd = sidebar::root_folder(&self.sidebar)
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
        match terminal_window::open(window, cx, self.settings.clone(), cwd) {
            Ok(terminal) => self.terminal_window = Some(terminal),
            Err(error) => {
                self.set_status(format!("Could not open Terminal: {error:#}"));
                cx.notify();
            }
        }
    }

    pub(super) fn active_tab(&self) -> Option<&DocumentTab> {
        self.tabs.get(self.active)
    }

    pub(super) fn apply_settings(
        &mut self,
        settings: AppSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let wrap = settings.word_wrap;
        window.set_rem_size(px(f32::from(settings.ui_font_size)));
        self.last_auto_save = Instant::now();
        if !settings.preview_cache_enabled {
            self.preview_cache = None;
        }
        if let Err(error) = crate::logging::set_level(&settings.logging_level) {
            tracing::warn!(error = %error, "could not update logging level");
        }
        let font_size = f32::from(settings.editor_font_size);
        session_state::apply_word_wrap_setting(&mut self.tabs, wrap);
        let tab_size = TabSize {
            tab_size: usize::from(settings.tab_size),
            hard_tabs: !settings.insert_spaces,
        };
        for tab in &mut self.tabs {
            tab.font_size = font_size;
            if let Some(ui) = tab.ui.as_ref() {
                ui.update(cx, |state, cx| {
                    state.set_soft_wrap(wrap, window, cx);
                    state.set_line_number(settings.show_line_numbers, window, cx);
                    state.set_tab_size(tab_size, cx);
                    state.set_auto_close(settings.auto_close_brackets, window, cx);
                    state.set_smart_indent(settings.smart_indent, window, cx);
                    state.set_indent_guides(settings.indent_guides, window, cx);
                });
            }
        }
        self.refresh_terminal(cx);
        cx.notify();
    }

    pub(super) fn apply_word_wrap_only(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        session_state::apply_word_wrap_setting(&mut self.tabs, enabled);
        for tab in &self.tabs {
            if let Some(ui) = tab.ui.as_ref() {
                ui.update(cx, |state, cx| state.set_soft_wrap(enabled, window, cx));
            }
        }
        cx.notify();
    }

    pub(super) fn refresh_terminal(&self, cx: &mut Context<Self>) {
        if let Some((handle, terminal_view)) = self.terminal_window.as_ref() {
            let terminal_view = terminal_view.clone();
            let _ = handle.update(cx, |_, terminal_window, app| {
                terminal_view.update(app, |terminal, cx| {
                    terminal.settings_changed(terminal_window, cx)
                });
            });
        }
    }

    fn remember_selection(&mut self, id: u64, state: &Entity<EditorState>, cx: &mut Context<Self>) {
        let (range, head) = {
            let state = state.read(cx);
            (state.selected_range(), state.cursor())
        };
        let Some((anchor, head)) = sync::oriented_selection(&range, head) else {
            return;
        };
        let mut selection_changed = false;
        let mut state_changed = false;
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
            && let Some(editor) = tab.editor.as_mut()
        {
            selection_changed = tab.last_selection != range;
            let current_range = editor.selection().range();
            let current_range_matches =
                current_range.start.0 == range.start && current_range.end.0 == range.end;
            let current_head = editor.cursor().offset().0;
            state_changed = selection_changed || !current_range_matches || current_head != head;
            if !state_changed {
                return;
            }

            tab.last_selection = range.clone();
            if !current_range_matches || current_head != head {
                let _ = editor.set_selection(ByteOffset(anchor), ByteOffset(head));
            }
        }
        if !state_changed {
            return;
        }
        if selection_changed {
            bracket::request(self, id, state, range, cx);
        }
        cx.notify();
    }

    fn sync_ui_change(&mut self, id: u64, state: &Entity<EditorState>, cx: &mut Context<Self>) {
        let visible_find = self.find_widget.as_ref().cloned().filter(|widget| {
            widget.read(cx).is_visible() && self.active_tab().is_some_and(|tab| tab.id == id)
        });
        if let Some(widget) = visible_find.as_ref() {
            widget.update(cx, |widget, cx| widget.document_changed(cx));
        }
        let search_request = visible_find
            .as_ref()
            .map(|widget| widget.read(cx).search_request(cx));
        let (rope, cursor, selected) = {
            let state = state.read(cx);
            (state.text().clone(), state.cursor(), state.selected_range())
        };
        if self.suppress_ui_sync == Some(id) {
            self.suppress_ui_sync = None;
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
                && let Some(editor) = tab.editor.as_mut()
            {
                tab.last_selection = selected.clone();
                let _ = editor.set_selection(ByteOffset(selected.start), ByteOffset(selected.end));
            }
            cx.notify();
            return;
        }
        if self
            .search_task
            .pending
            .is_some_and(|(pending_id, _, _)| pending_id == id)
        {
            search::cancel(&mut self.search_task);
        }
        bracket::cancel(&mut self.bracket_task);
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let Some(editor) = tab.editor.as_mut() else {
            return;
        };

        let old_selection = tab.last_selection.clone();
        let old_len = editor.snapshot().len_bytes();
        let new_len = rope.len();
        let candidate = local_change_range(old_len, new_len, &old_selection, cursor, &selected);
        let applied = candidate.and_then(|(range, replacement)| {
            let inserted = rope.slice(replacement.start..replacement.end).to_string();
            let range = ByteRange::new(range.start, range.end);
            if editor.undo_matches(range, &inserted) && editor.undo().ok() == Some(true) {
                return Some(inserted);
            }
            if editor.redo_matches(range, &inserted) && editor.redo().ok() == Some(true) {
                return Some(inserted);
            }
            let has_newline = inserted.contains('\r') || inserted.contains('\n');
            let edit = if range.is_empty() && !has_newline {
                editor.type_text(&inserted)
            } else {
                editor.replace_range(range, &inserted)
            };
            edit.ok().map(|_| inserted)
        });

        if let Some(inserted) = applied {
            update_eol_after_edit(&mut tab.eol, &inserted);
        } else {
            let new_text = rope.to_string();
            if let Err(error) = apply_minimal_diff(editor, &new_text) {
                tracing::error!(document_id = id, error = %error, "failed to synchronize editor state");
                self.status = format!("Could not synchronize edit: {error}");
            }
            tab.eol = detect_eol(&new_text);
        }
        tab.last_selection = selected;
        if let Some(decorations) = tab.search_decorations.as_ref() {
            decorations.clear(cx);
        }
        if let Some(decorations) = tab.current_search_decorations.as_ref() {
            decorations.clear(cx);
        }
        if self
            .active_tab()
            .is_some_and(|tab| tab.id == id && tab.preview_mode)
        {
            self.invalidate_preview_for(id, cx);
        }
        if let Some((options, scope)) = search_request {
            search::start(self, options, scope, search::SearchAction::Update, cx);
        }
        cx.notify();
    }

    fn add_blank(&mut self, cx: &mut Context<Self>) {
        let mut tab = DocumentTab::blank(self.next_id);
        apply_document_defaults(&mut tab, &self.settings.current());
        self.tabs.push(tab);
        self.next_id = self.next_id.saturating_add(1);
        let index = self.tabs.len() - 1;
        preview::activate(self, index);
        self.status.clear();
        cx.notify();
    }

    fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else { return };
        let id = tab.id;
        if !tab.is_dirty() || !self.settings.current().confirm_before_close {
            self.remove_tab(id);
            cx.notify();
            return;
        }
        dialogs::confirm_close_tab(window, cx, id, tab.title.clone());
    }

    fn remove_tab(&mut self, id: u64) {
        if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
            self.complete_cli_tab_close(id, self.tabs[index].is_dirty());
            if self
                .search_task
                .pending
                .is_some_and(|(pending_id, _, _)| pending_id == id)
            {
                search::cancel(&mut self.search_task);
            }
            if self.active_tab().is_some_and(|tab| tab.id == id) {
                bracket::cancel(&mut self.bracket_task);
            }
            self.pending_file_loads.remove(&id);
            self.pending_formats.remove(&id);
            self.cancel_file_loading_indicator(id);
            self.image_cache.remove(id);
            self.tabs.remove(index);
            if self.tabs.is_empty() {
                let mut tab = DocumentTab::blank(self.next_id);
                apply_document_defaults(&mut tab, &self.settings.current());
                self.tabs.push(tab);
                self.next_id = self.next_id.saturating_add(1);
            }
            let active = self.active.min(self.tabs.len().saturating_sub(1));
            preview::activate(self, active);
        }
    }
}
