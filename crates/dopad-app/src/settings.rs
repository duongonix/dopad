use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use dopad_core::{Encoding, EolKind};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

const SETTINGS_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub(crate) struct AppSettings {
    pub confirm_before_close: bool,
    pub restore_session: bool,
    pub auto_check_updates: bool,
    pub word_wrap: bool,
    pub editor_font_family: String,
    pub editor_font_size: u8,
    pub editor_font_weight: u16,
    #[serde(default, skip_serializing)]
    pub editor_bold: bool,
    pub editor_line_height: u8,
    pub tab_size: u8,
    pub insert_spaces: bool,
    pub show_line_numbers: bool,
    pub show_status_bar: bool,
    pub sidebar_width: u16,
    pub ui_font_size: u8,
    pub compact_tabs: bool,
    pub file_icon_theme: String,
    pub auto_save: bool,
    pub save_interval_seconds: u16,
    pub detect_external_changes: bool,
    pub show_recent_files: bool,
    pub recent_file_limit: u16,
    pub recent_files: Vec<PathBuf>,
    pub default_file_type: String,
    pub default_file_extension: String,
    pub default_encoding: Encoding,
    pub default_line_ending: EolKind,
    pub detect_encoding: bool,
    pub preserve_encoding: bool,
    pub bom_behavior: String,
    pub large_file_mode: bool,
    pub syntax_highlighting_threshold_mb: u16,
    pub preview_cache_enabled: bool,
    pub logging_level: String,
    pub shortcut_overrides: BTreeMap<String, String>,
    pub auto_close_brackets: bool,
    pub smart_indent: bool,
    pub indent_guides: bool,
    pub terminal_font_family: String,
    pub terminal_font_size: u8,
    pub terminal_font_weight: u16,
    pub terminal_line_height: u8,
    pub terminal_cell_width: u8,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            confirm_before_close: true,
            restore_session: true,
            auto_check_updates: true,
            word_wrap: false,
            editor_font_family: "System Monospace".into(),
            editor_font_size: 14,
            editor_font_weight: 400,
            editor_bold: false,
            editor_line_height: 150,
            tab_size: 4,
            insert_spaces: true,
            show_line_numbers: true,
            show_status_bar: true,
            sidebar_width: 280,
            ui_font_size: 14,
            compact_tabs: true,
            file_icon_theme: "material".into(),
            auto_save: false,
            save_interval_seconds: 180,
            detect_external_changes: true,
            show_recent_files: false,
            recent_file_limit: 50,
            recent_files: Vec::new(),
            default_file_type: "Plain Text".into(),
            default_file_extension: String::new(),
            default_encoding: Encoding::Utf8,
            default_line_ending: EolKind::Lf,
            detect_encoding: true,
            preserve_encoding: true,
            bom_behavior: "Preserve if present".into(),
            large_file_mode: true,
            syntax_highlighting_threshold_mb: 10,
            preview_cache_enabled: true,
            logging_level: "Warn".into(),
            shortcut_overrides: BTreeMap::new(),
            auto_close_brackets: true,
            smart_indent: true,
            indent_guides: false,
            terminal_font_family: "System Monospace".into(),
            terminal_font_size: 13,
            terminal_font_weight: 400,
            terminal_line_height: 140,
            terminal_cell_width: 100,
        }
    }
}

#[derive(Deserialize, Serialize)]
struct SettingsFile {
    version: u32,
    settings: AppSettings,
}

#[derive(Clone)]
pub(crate) struct SettingsService {
    settings: Arc<RwLock<AppSettings>>,
    path: Option<PathBuf>,
}

impl SettingsService {
    pub(crate) fn load() -> Self {
        let path = settings_path();
        let settings = match path.as_deref().map(load_file) {
            Some(Ok(settings)) => settings,
            Some(Err(error)) => {
                tracing::warn!(error = %error, "settings could not be loaded; using defaults");
                AppSettings::default()
            }
            None => {
                tracing::warn!("settings directory is unavailable; settings will not persist");
                AppSettings::default()
            }
        };
        Self {
            settings: Arc::new(RwLock::new(settings)),
            path,
        }
    }

    pub(crate) fn current(&self) -> AppSettings {
        self.settings
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn path(&self) -> Option<PathBuf> {
        self.path.clone()
    }

    pub(crate) fn replace(&self, settings: AppSettings) {
        *self
            .settings
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = settings;
    }
}

pub(crate) fn save_file(path: &Path, settings: &AppSettings) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("settings path has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(
        &mut temporary,
        &SettingsFile {
            version: SETTINGS_VERSION,
            settings: settings.clone(),
        },
    )?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    sync_parent(parent)?;
    Ok(())
}

fn load_file(path: &Path) -> anyhow::Result<AppSettings> {
    let file: SettingsFile = match File::open(path) {
        Ok(file) => serde_json::from_reader(file)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(AppSettings::default()),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        file.version == SETTINGS_VERSION,
        "unsupported settings version {}",
        file.version
    );
    validate(file.settings)
}

fn validate(mut settings: AppSettings) -> anyhow::Result<AppSettings> {
    if settings.editor_bold {
        if settings.editor_font_weight == 400 {
            settings.editor_font_weight = 800;
        }
        settings.editor_bold = false;
    }
    anyhow::ensure!(
        (10..=24).contains(&settings.editor_font_size),
        "editor font size must be between 10 and 24"
    );
    settings.editor_font_size = settings.editor_font_size.clamp(10, 24);
    anyhow::ensure!(
        (220..=420).contains(&settings.sidebar_width),
        "sidebar width must be between 220 and 420"
    );
    validate_font_family(&settings.editor_font_family, "editor")?;
    anyhow::ensure!(
        [400, 500, 600, 700, 800].contains(&settings.editor_font_weight),
        "unsupported editor font weight"
    );
    validate_font_family(&settings.terminal_font_family, "terminal")?;
    anyhow::ensure!(
        (10..=24).contains(&settings.terminal_font_size),
        "terminal font size must be between 10 and 24"
    );
    anyhow::ensure!(
        [400, 500, 600, 700, 800].contains(&settings.terminal_font_weight),
        "unsupported terminal font weight"
    );
    anyhow::ensure!(
        [100, 110, 120, 130, 140, 150, 160, 175, 200].contains(&settings.terminal_line_height),
        "unsupported terminal line height"
    );
    anyhow::ensure!(
        [100, 110, 120, 130, 140].contains(&settings.terminal_cell_width),
        "unsupported terminal cell width"
    );
    anyhow::ensure!(
        [125, 150, 175, 200].contains(&settings.editor_line_height),
        "editor line height must be 1.25, 1.5, 1.75 or 2.0"
    );
    anyhow::ensure!(
        (2..=8).contains(&settings.tab_size),
        "tab size must be between 2 and 8"
    );
    anyhow::ensure!(
        [12, 14, 16, 18].contains(&settings.ui_font_size),
        "UI font size must be 12, 14, 16 or 18"
    );
    anyhow::ensure!(
        settings.file_icon_theme == "material",
        "unsupported file icon theme"
    );
    anyhow::ensure!(
        [30, 60, 180, 300, 600].contains(&settings.save_interval_seconds),
        "auto-save interval must be 30, 60, 180, 300 or 600 seconds"
    );
    anyhow::ensure!(
        [10, 20, 50, 100].contains(&settings.recent_file_limit),
        "recent file limit must be 10, 20, 50 or 100"
    );
    anyhow::ensure!(
        ["Plain Text", "Markdown", "JSON", "XML"].contains(&settings.default_file_type.as_str()),
        "unsupported default document type"
    );
    anyhow::ensure!(
        ["", "txt", "md", "json", "xml"].contains(&settings.default_file_extension.as_str()),
        "unsupported default file extension"
    );
    anyhow::ensure!(
        matches!(
            settings.default_line_ending,
            EolKind::Lf | EolKind::CrLf | EolKind::Cr
        ),
        "default line ending must be LF, CRLF or CR"
    );
    anyhow::ensure!(
        [
            "Preserve if present",
            "Always include BOM",
            "Never include BOM"
        ]
        .contains(&settings.bom_behavior.as_str()),
        "unsupported BOM behavior"
    );
    anyhow::ensure!(
        [1, 5, 10, 25, 50].contains(&settings.syntax_highlighting_threshold_mb),
        "syntax highlighting threshold must be 1, 5, 10, 25 or 50 MiB"
    );
    anyhow::ensure!(
        ["Error", "Warn", "Info", "Debug", "Trace"].contains(&settings.logging_level.as_str()),
        "unsupported logging level"
    );
    const SHORTCUT_IDS: [&str; 26] = [
        "new_document",
        "open_document",
        "save",
        "save_as",
        "close_tab",
        "find",
        "replace",
        "find_next",
        "find_previous",
        "find_case",
        "find_whole_word",
        "find_regex",
        "find_selection",
        "add_next_match",
        "close_find",
        "toggle_wrap",
        "zoom_in",
        "zoom_in_alternate",
        "zoom_out",
        "reset_zoom",
        "preview",
        "go_to_line",
        "duplicate_line",
        "toggle_comment",
        "move_line_up",
        "move_line_down",
    ];
    anyhow::ensure!(settings.shortcut_overrides.len() <= SHORTCUT_IDS.len());
    for (action, key) in &settings.shortcut_overrides {
        anyhow::ensure!(
            SHORTCUT_IDS.contains(&action.as_str()),
            "unsupported shortcut action: {action}"
        );
        anyhow::ensure!(
            is_valid_shortcut_key(key),
            "invalid shortcut key for {action}"
        );
    }
    settings.recent_files.truncate(100);
    Ok(settings)
}

fn validate_font_family(family: &str, target: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !family.is_empty()
            && family.len() <= 256
            && family.trim() == family
            && !family.chars().any(char::is_control),
        "invalid {target} font family"
    );
    Ok(())
}

fn is_valid_shortcut_key(key: &str) -> bool {
    if key.is_empty() || key.len() > 64 || !key.is_ascii() {
        return false;
    }
    if matches!(key, "escape" | "f3" | "shift-f3") {
        return true;
    }

    const NAMED_KEYS: [&str; 8] = [
        "up", "down", "left", "right", "enter", "tab", "space", "delete",
    ];
    let Some((prefix, base)) = NAMED_KEYS
        .iter()
        .find_map(|name| key.strip_suffix(name).map(|prefix| (prefix, *name)))
        .or_else(|| {
            let base = key.as_bytes().last().copied()?;
            matches!(base, b'a'..=b'z' | b'0'..=b'9' | b'/' | b'=' | b'-')
                .then(|| (&key[..key.len() - 1], &key[key.len() - 1..]))
        })
    else {
        return false;
    };
    let Some(modifiers) = prefix.strip_suffix('-') else {
        return false;
    };

    let mut control = false;
    let mut seen = std::collections::BTreeSet::new();
    for modifier in modifiers.split('-') {
        if !seen.insert(modifier) {
            return false;
        }
        match modifier {
            "ctrl" | "cmd" => control = true,
            "alt" | "shift" => {}
            _ => return false,
        }
    }
    control || (modifiers == "alt" && matches!(base, "c" | "w" | "r" | "l"))
}

fn settings_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        env::var_os("APPDATA")
            .or_else(|| env::var_os("LOCALAPPDATA"))
            .map(|root| PathBuf::from(root).join("Dopad").join("settings.json"))
    }

    #[cfg(target_os = "macos")]
    {
        env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("Dopad")
                .join("settings.json")
        })
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(root) = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
            if root.is_absolute() {
                return Some(root.join("dopad").join("settings.json"));
            }
        }
        env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join(".config")
                .join("dopad")
                .join("settings.json")
        })
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let _ = env::var_os("HOME");
        None
    }
}

pub(crate) fn app_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        env::var_os("LOCALAPPDATA")
            .or_else(|| env::var_os("APPDATA"))
            .map(|root| PathBuf::from(root).join("Dopad"))
    }

    #[cfg(target_os = "macos")]
    {
        env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("Dopad")
        })
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(root) = env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
            if root.is_absolute() {
                return Some(root.join("dopad"));
            }
        }
        env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("dopad")
        })
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let _ = env::var_os("HOME");
        None
    }
}

fn sync_parent(parent: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(parent)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
        Ok(())
    }
}

#[cfg(test)]
#[path = "settings/tests.rs"]
mod tests;
