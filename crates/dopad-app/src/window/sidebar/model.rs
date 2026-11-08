use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use gpui_kit::Entity;
use gpui_kit::component::input::InputState;
use gpui_kit::{FocusHandle, UniformListScrollHandle};

use super::super::file_search::{FileMatches, SearchFilesResult};
use crate::icon_theme::{IconImageCache, IconThemeManager};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SidebarMode {
    #[default]
    Closed,
    Rail,
    Explorer,
    Search,
}

impl SidebarMode {
    pub(in crate::window) fn is_visible(self) -> bool {
        self != Self::Closed
    }

    pub(in crate::window) fn has_panel(self) -> bool {
        matches!(self, Self::Explorer | Self::Search)
    }

    pub(super) fn toggle_sidebar(self, last_panel: Self) -> Self {
        if self.is_visible() {
            Self::Closed
        } else {
            last_panel
        }
    }

    pub(super) fn toggle_panel(self, panel: Self) -> Self {
        if self == panel { Self::Rail } else { panel }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum SearchScope {
    #[default]
    CurrentFolder,
    OpenFiles,
    CurrentFile,
    CustomFolder,
}

#[derive(Clone, Debug)]
pub(super) struct ExplorerEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

#[derive(Clone, Debug)]
pub(super) enum DirectoryListing {
    Ready(Vec<ExplorerEntry>),
    Error(String),
    Truncated(Vec<ExplorerEntry>, usize),
}

pub(super) struct ExplorerState {
    pub(super) root: Option<PathBuf>,
    pub(super) expanded: HashSet<PathBuf>,
    pub(super) entries: HashMap<PathBuf, DirectoryListing>,
    pub(super) loading: HashSet<PathBuf>,
    pub(super) selected: Option<PathBuf>,
    pub(super) generation: u64,
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) focus_handle: Option<FocusHandle>,
    pub(super) scroll_handle: UniformListScrollHandle,
    pub(super) icon_theme: Option<Arc<IconThemeManager>>,
    pub(super) icon_cache: Option<Entity<IconImageCache>>,
    pub(super) icon_theme_loading: bool,
    pub(super) icon_theme_failed: bool,
    pub(super) icon_theme_generation: u64,
}

impl Default for ExplorerState {
    fn default() -> Self {
        Self {
            root: None,
            expanded: HashSet::new(),
            entries: HashMap::new(),
            loading: HashSet::new(),
            selected: None,
            generation: 0,
            cancelled: Arc::new(AtomicBool::new(false)),
            focus_handle: None,
            scroll_handle: UniformListScrollHandle::new(),
            icon_theme: None,
            icon_cache: None,
            icon_theme_loading: false,
            icon_theme_failed: false,
            icon_theme_generation: 0,
        }
    }
}

pub(super) struct SearchPanelState {
    pub(super) input: Option<Entity<InputState>>,
    pub(super) match_case: bool,
    pub(super) whole_word: bool,
    pub(super) regex: bool,
    pub(super) in_current_folder: bool,
    pub(super) scope: SearchScope,
    pub(super) custom_root: Option<PathBuf>,
    pub(super) filter: Option<Entity<InputState>>,
    pub(super) running: bool,
    pub(super) status: String,
    pub(super) results: Vec<FileMatches>,
    pub(super) collapsed: HashSet<PathBuf>,
    pub(super) generation: u64,
    pub(super) cancel: Option<Arc<AtomicBool>>,
    pub(super) summary: Option<SearchFilesResult>,
}

impl Default for SearchPanelState {
    fn default() -> Self {
        Self {
            input: None,
            match_case: false,
            whole_word: false,
            regex: false,
            in_current_folder: true,
            scope: SearchScope::CurrentFolder,
            custom_root: None,
            filter: None,
            running: false,
            status: String::new(),
            results: Vec::new(),
            collapsed: HashSet::new(),
            generation: 0,
            cancel: None,
            summary: None,
        }
    }
}

pub struct SidebarState {
    pub(in crate::window) mode: SidebarMode,
    pub(in crate::window) last_panel: SidebarMode,
    pub(in crate::window) width: f32,
    pub(super) explorer: ExplorerState,
    pub(super) search: SearchPanelState,
}

impl SidebarState {
    pub fn new(width: u16) -> Self {
        Self {
            mode: SidebarMode::Closed,
            last_panel: SidebarMode::Explorer,
            width: f32::from(width.clamp(220, 420)),
            explorer: ExplorerState::default(),
            search: SearchPanelState::default(),
        }
    }
}

#[cfg(test)]
mod explorer_icon_state_tests {
    use super::{ExplorerState, SidebarMode};

    #[test]
    fn icon_theme_and_decoded_image_cache_start_uninitialized() {
        let state = ExplorerState::default();
        assert!(state.icon_theme.is_none());
        assert!(state.icon_cache.is_none());
        assert!(!state.icon_theme_loading);
        assert!(!state.icon_theme_failed);
    }

    #[test]
    fn navigation_rail_and_panels_have_independent_visibility_states() {
        assert_eq!(
            SidebarMode::Closed.toggle_sidebar(SidebarMode::Explorer),
            SidebarMode::Explorer
        );
        assert_eq!(
            SidebarMode::Rail.toggle_panel(SidebarMode::Explorer),
            SidebarMode::Explorer
        );
        assert_eq!(
            SidebarMode::Explorer.toggle_panel(SidebarMode::Explorer),
            SidebarMode::Rail
        );
        assert_eq!(
            SidebarMode::Explorer.toggle_panel(SidebarMode::Search),
            SidebarMode::Search
        );
        assert_eq!(
            SidebarMode::Search.toggle_sidebar(SidebarMode::Search),
            SidebarMode::Closed
        );
    }
}
