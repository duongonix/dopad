use std::collections::{HashMap, HashSet};

use crate::settings::AppSettings;

#[derive(Clone, Copy)]
pub(crate) struct ShortcutSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub default_key: &'static str,
}

const SHORTCUT_SPECS: [ShortcutSpec; 27] = [
    ShortcutSpec {
        id: "new_document",
        label: "New document",
        default_key: "ctrl-n",
    },
    ShortcutSpec {
        id: "open_document",
        label: "Open document",
        default_key: "ctrl-o",
    },
    ShortcutSpec {
        id: "save",
        label: "Save",
        default_key: "ctrl-s",
    },
    ShortcutSpec {
        id: "save_as",
        label: "Save As",
        default_key: "ctrl-shift-s",
    },
    ShortcutSpec {
        id: "close_tab",
        label: "Close tab",
        default_key: "ctrl-w",
    },
    ShortcutSpec {
        id: "find",
        label: "Find",
        default_key: "ctrl-f",
    },
    ShortcutSpec {
        id: "replace",
        label: "Replace",
        default_key: "ctrl-h",
    },
    ShortcutSpec {
        id: "find_next",
        label: "Find Next",
        default_key: "f3",
    },
    ShortcutSpec {
        id: "find_previous",
        label: "Find Previous",
        default_key: "shift-f3",
    },
    ShortcutSpec {
        id: "find_case",
        label: "Toggle Find Match Case",
        default_key: "alt-c",
    },
    ShortcutSpec {
        id: "find_whole_word",
        label: "Toggle Find Whole Word",
        default_key: "alt-w",
    },
    ShortcutSpec {
        id: "find_regex",
        label: "Toggle Find Regex",
        default_key: "alt-r",
    },
    ShortcutSpec {
        id: "find_selection",
        label: "Toggle Find in Selection",
        default_key: "alt-l",
    },
    ShortcutSpec {
        id: "add_next_match",
        label: "Add Next Match to Selection",
        default_key: "ctrl-d",
    },
    ShortcutSpec {
        id: "close_find",
        label: "Close Find Widget",
        default_key: "escape",
    },
    ShortcutSpec {
        id: "toggle_wrap",
        label: "Word Wrap",
        default_key: "ctrl-alt-w",
    },
    ShortcutSpec {
        id: "zoom_in",
        label: "Zoom In",
        default_key: "ctrl-=",
    },
    ShortcutSpec {
        id: "zoom_in_alternate",
        label: "Zoom In (alternate)",
        default_key: "ctrl-shift-=",
    },
    ShortcutSpec {
        id: "zoom_out",
        label: "Zoom Out",
        default_key: "ctrl--",
    },
    ShortcutSpec {
        id: "reset_zoom",
        label: "Reset Zoom",
        default_key: "ctrl-0",
    },
    ShortcutSpec {
        id: "preview",
        label: "Preview",
        default_key: "ctrl-shift-p",
    },
    ShortcutSpec {
        id: "go_to_line",
        label: "Go to Line",
        default_key: "ctrl-g",
    },
    ShortcutSpec {
        id: "duplicate_line",
        label: "Duplicate Line",
        default_key: "ctrl-shift-d",
    },
    ShortcutSpec {
        id: "toggle_comment",
        label: "Toggle Comment",
        default_key: "ctrl-/",
    },
    ShortcutSpec {
        id: "move_line_up",
        label: "Move Line Up",
        default_key: "ctrl-alt-shift-up",
    },
    ShortcutSpec {
        id: "move_line_down",
        label: "Move Line Down",
        default_key: "ctrl-alt-shift-down",
    },
    ShortcutSpec {
        id: "format_document",
        label: "Format Document",
        default_key: "shift-alt-f",
    },
];

const EXTRA_SHORTCUTS: [&str; 21] = [
    "ctrl-shift-n",
    "ctrl-alt-n",
    "ctrl-alt-o",
    "ctrl-alt-s",
    "ctrl-shift-w",
    "ctrl-alt-f",
    "ctrl-alt-h",
    "ctrl-alt-p",
    "ctrl-alt-g",
    "ctrl-alt-d",
    "ctrl-alt-/",
    "ctrl-shift-/",
    "f3",
    "shift-f3",
    "alt-c",
    "alt-w",
    "alt-r",
    "alt-l",
    "ctrl-d",
    "escape",
    "shift-alt-f",
];

pub(crate) fn specs() -> &'static [ShortcutSpec; 27] {
    &SHORTCUT_SPECS
}

pub(crate) fn choice_keys() -> Vec<String> {
    let mut keys = SHORTCUT_SPECS
        .iter()
        .map(|spec| effective_default_key(spec.id))
        .chain(EXTRA_SHORTCUTS.iter().map(|key| platform_key(key)))
        .collect::<Vec<_>>();
    let mut seen = HashSet::with_capacity(keys.len());
    keys.retain(|key| seen.insert(key.clone()));
    keys
}

fn platform_key(key: &str) -> String {
    if !cfg!(target_os = "macos") {
        return key.to_owned();
    }
    match key {
        "alt-c" => "cmd-alt-c".into(),
        "alt-w" => "cmd-alt-w".into(),
        "alt-r" => "cmd-alt-r".into(),
        "alt-l" => "cmd-alt-l".into(),
        _ => key.replacen("ctrl-", "cmd-", 1),
    }
}

pub(crate) fn display_key(key: &str) -> String {
    let (key, minus) = key
        .strip_suffix("--")
        .map_or((key, false), |prefix| (prefix, true));
    let label = key
        .split('-')
        .map(|part| match part {
            "ctrl" => "Ctrl".to_owned(),
            "cmd" => "Cmd".to_owned(),
            "alt" if cfg!(target_os = "macos") => "Option".to_owned(),
            "alt" => "Alt".to_owned(),
            "shift" => "Shift".to_owned(),
            "up" => "Up".to_owned(),
            "down" => "Down".to_owned(),
            "escape" => "Escape".to_owned(),
            other => other.to_ascii_uppercase(),
        })
        .collect::<Vec<_>>()
        .join("+");
    if minus { format!("{label}+-") } else { label }
}

pub(crate) fn key_from_display(label: &str) -> Option<String> {
    let parts = label
        .split('+')
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    let (last, modifiers) = parts.split_last()?;
    let last = last.trim();
    if last.is_empty() {
        return None;
    }
    if modifiers
        .iter()
        .any(|part| !matches!(part.as_str(), "ctrl" | "cmd" | "alt" | "option" | "shift"))
    {
        return None;
    }
    if last != "-" && (last.contains('+') || last.contains('-')) {
        return None;
    }
    let modifier_prefix = modifiers
        .iter()
        .map(|modifier| {
            if modifier == "option" {
                "alt"
            } else {
                modifier
            }
        })
        .collect::<Vec<_>>()
        .join("-");
    Some(if last == "-" {
        format!("{modifier_prefix}--")
    } else if modifier_prefix.is_empty() {
        last.to_owned()
    } else {
        format!("{modifier_prefix}-{last}")
    })
}

pub(crate) fn effective_default_key(id: &str) -> String {
    let default = SHORTCUT_SPECS
        .iter()
        .find(|spec| spec.id == id)
        .map_or("ctrl-", |spec| spec.default_key);
    if !cfg!(target_os = "macos") {
        return default.to_owned();
    }
    match id {
        "replace" => "cmd-alt-f".into(),
        "find_case" => "cmd-alt-c".into(),
        "find_whole_word" => "cmd-alt-w".into(),
        "find_regex" => "cmd-alt-r".into(),
        "find_selection" => "cmd-alt-l".into(),
        "add_next_match" => "cmd-d".into(),
        _ => default.replacen("ctrl-", "cmd-", 1),
    }
}

pub(crate) fn effective_key(settings: &AppSettings, id: &str) -> String {
    settings
        .shortcut_overrides
        .get(id)
        .cloned()
        .unwrap_or_else(|| effective_default_key(id))
}

pub(crate) fn conflicts(settings: &AppSettings) -> Vec<String> {
    let mut used = HashMap::<String, Vec<&'static str>>::new();
    for spec in specs() {
        used.entry(effective_key(settings, spec.id))
            .or_default()
            .push(spec.label);
    }
    used.into_iter()
        .filter_map(|(key, labels)| {
            (labels.len() > 1).then(|| format!("{key}: {}", labels.join(" / ")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_document_has_a_remappable_platform_correct_default() {
        assert_eq!(effective_default_key("format_document"), "shift-alt-f");
        assert!(specs().iter().any(|spec| spec.id == "format_document"));
        assert_eq!(
            key_from_display("Shift+Option+F").as_deref(),
            Some("shift-alt-f")
        );
        assert_eq!(
            display_key("shift-alt-f"),
            if cfg!(target_os = "macos") {
                "Shift+Option+F"
            } else {
                "Shift+Alt+F"
            }
        );
    }

    #[test]
    fn format_document_shortcut_uses_user_override() {
        let settings = AppSettings {
            shortcut_overrides: std::collections::BTreeMap::from([(
                "format_document".into(),
                "ctrl-alt-k".into(),
            )]),
            ..AppSettings::default()
        };
        assert_eq!(effective_key(&settings, "format_document"), "ctrl-alt-k");
    }
}
