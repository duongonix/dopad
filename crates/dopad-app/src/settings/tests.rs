use super::{
    AppSettings, SETTINGS_VERSION, SettingsFile, is_valid_shortcut_key, load_file, save_file,
};
use dopad_core::{Encoding, EolKind};
use std::fs;

#[test]
fn word_wrap_is_disabled_by_default() {
    assert!(!AppSettings::default().word_wrap);
}

#[test]
fn word_wrap_setting_persists_across_restarts() {
    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("settings.json");
    let settings = AppSettings {
        word_wrap: true,
        ..AppSettings::default()
    };

    save_file(&path, &settings).expect("save Word Wrap setting");

    assert!(load_file(&path).expect("reload settings").word_wrap);
}

#[test]
fn material_icon_theme_is_the_default_and_only_supported_pack() {
    assert_eq!(AppSettings::default().file_icon_theme, "material");
    assert!(
        super::validate(AppSettings {
            file_icon_theme: "unknown".into(),
            ..AppSettings::default()
        })
        .is_err()
    );
}

#[test]
fn settings_round_trip_atomically() {
    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("settings.json");
    let settings = AppSettings {
        confirm_before_close: false,
        restore_session: false,
        auto_check_updates: false,
        word_wrap: false,
        editor_font_family: "Consolas".into(),
        editor_font_size: 18,
        editor_font_weight: 800,
        editor_bold: false,
        editor_line_height: 175,
        tab_size: 2,
        insert_spaces: false,
        show_line_numbers: false,
        show_status_bar: false,
        sidebar_width: 320,
        ui_font_size: 18,
        compact_tabs: false,
        file_icon_theme: "material".into(),
        auto_save: true,
        save_interval_seconds: 60,
        detect_external_changes: false,
        show_recent_files: true,
        recent_file_limit: 20,
        recent_files: vec![std::path::PathBuf::from("notes.md")],
        default_file_type: "Markdown".into(),
        default_file_extension: "md".into(),
        default_encoding: Encoding::Utf16LeNoBom,
        default_line_ending: EolKind::CrLf,
        detect_encoding: false,
        preserve_encoding: false,
        bom_behavior: "Never include BOM".into(),
        large_file_mode: false,
        syntax_highlighting_threshold_mb: 25,
        preview_cache_enabled: false,
        logging_level: "Trace".into(),
        shortcut_overrides: std::collections::BTreeMap::from([(
            "save".into(),
            "ctrl-alt-s".into(),
        )]),
        auto_close_brackets: false,
        smart_indent: false,
        indent_guides: true,
        terminal_font_family: "Consolas".into(),
        terminal_font_size: 16,
        terminal_font_weight: 600,
        terminal_line_height: 150,
        terminal_cell_width: 120,
    };

    save_file(&path, &settings).expect("save settings");
    assert_eq!(load_file(&path).expect("load settings"), settings);
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("read saved settings"))
            .expect("parse saved settings");
    assert!(saved["settings"].get("editor_bold").is_none());
    assert_eq!(saved["settings"]["file_icon_theme"], "material");

    let updated = AppSettings {
        editor_font_family: "JetBrains Mono".into(),
        editor_font_size: 12,
        ..AppSettings::default()
    };
    save_file(&path, &updated).expect("replace settings");
    assert_eq!(load_file(&path).expect("reload settings"), updated);
}

#[test]
fn missing_settings_file_uses_defaults() {
    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("missing.json");

    assert_eq!(
        load_file(&path).expect("load defaults"),
        AppSettings::default()
    );
}

#[test]
fn legacy_settings_default_new_font_weight() {
    let mut value = serde_json::to_value(SettingsFile {
        version: SETTINGS_VERSION,
        settings: AppSettings::default(),
    })
    .expect("serialize settings");
    value["settings"]
        .as_object_mut()
        .expect("settings object")
        .remove("editor_font_weight");

    let legacy: SettingsFile = serde_json::from_value(value).expect("deserialize legacy settings");
    assert!(!legacy.settings.editor_bold);
    assert_eq!(legacy.settings.editor_font_weight, 400);
}

#[test]
fn legacy_bold_setting_migrates_to_extra_bold_weight() {
    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("settings.json");
    let mut value = serde_json::to_value(SettingsFile {
        version: SETTINGS_VERSION,
        settings: AppSettings::default(),
    })
    .expect("serialize settings");
    let settings = value["settings"].as_object_mut().expect("settings object");
    settings.insert("editor_bold".into(), serde_json::Value::Bool(true));
    settings.remove("editor_font_weight");
    fs::write(
        &path,
        serde_json::to_vec(&value).expect("serialize legacy settings"),
    )
    .expect("write legacy settings");

    let migrated = load_file(&path).expect("migrate legacy setting");
    assert_eq!(migrated.editor_font_weight, 800);
    assert!(!migrated.editor_bold);
}

#[test]
fn editor_font_weight_accepts_only_supported_values() {
    for weight in [400, 500, 600, 700, 800] {
        let settings = AppSettings {
            editor_font_weight: weight,
            ..AppSettings::default()
        };
        assert!(super::validate(settings).is_ok(), "weight {weight}");
    }
    let invalid = AppSettings {
        editor_font_weight: 450,
        ..AppSettings::default()
    };
    assert!(super::validate(invalid).is_err());
}

#[test]
fn terminal_typography_settings_validate_and_round_trip() {
    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("settings.json");
    let settings = AppSettings {
        terminal_font_family: "JetBrains Mono".into(),
        terminal_font_size: 18,
        terminal_font_weight: 700,
        terminal_line_height: 175,
        terminal_cell_width: 130,
        ..AppSettings::default()
    };

    save_file(&path, &settings).expect("save terminal typography settings");
    assert_eq!(load_file(&path).expect("reload settings"), settings);

    for invalid in [
        AppSettings {
            terminal_font_size: 9,
            ..settings.clone()
        },
        AppSettings {
            terminal_font_weight: 450,
            ..settings.clone()
        },
        AppSettings {
            terminal_line_height: 145,
            ..settings.clone()
        },
        AppSettings {
            terminal_cell_width: 90,
            ..settings.clone()
        },
        AppSettings {
            terminal_font_family: "".into(),
            ..settings
        },
    ] {
        assert!(super::validate(invalid).is_err());
    }
}

#[test]
fn installed_font_families_are_not_limited_to_the_bundled_catalog() {
    let settings = AppSettings {
        editor_font_family: "Hack Nerd Font Mono".into(),
        terminal_font_family: "Noto Sans Mono".into(),
        ..AppSettings::default()
    };
    assert!(super::validate(settings).is_ok());

    let long_family = "x".repeat(257);
    for family in ["", " Consolas", "Consolas\n", &long_family] {
        let settings = AppSettings {
            editor_font_family: family.into(),
            ..AppSettings::default()
        };
        assert!(super::validate(settings).is_err(), "family {family:?}");
    }
}

#[test]
fn shortcut_validation_requires_control_and_known_modifiers() {
    for key in [
        "ctrl-n",
        "ctrl-shift-=",
        "ctrl--",
        "ctrl-alt-shift-up",
        "cmd-alt-/",
        "alt-c",
        "alt-w",
        "alt-r",
        "alt-l",
        "f3",
        "shift-f3",
        "escape",
    ] {
        assert!(is_valid_shortcut_key(key), "{key}");
    }
    for key in ["", "n", "ctrl-", "ctrl-ctrl-s", "ctrl-unknown-key", "alt-x"] {
        assert!(!is_valid_shortcut_key(key), "{key}");
    }
}

#[test]
fn find_and_replace_shortcut_overrides_are_supported() {
    let settings = AppSettings {
        shortcut_overrides: std::collections::BTreeMap::from([
            ("find_next".into(), "f3".into()),
            ("close_find".into(), "escape".into()),
            ("find_case".into(), "alt-c".into()),
        ]),
        ..AppSettings::default()
    };
    assert!(super::validate(settings).is_ok());
}

#[test]
fn invalid_and_unsupported_settings_are_rejected() {
    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("settings.json");
    fs::write(
        &path,
        serde_json::to_vec(&SettingsFile {
            version: SETTINGS_VERSION,
            settings: AppSettings {
                editor_font_size: 30,
                ..AppSettings::default()
            },
        })
        .expect("serialize"),
    )
    .expect("write settings");

    assert!(load_file(&path).is_err());

    fs::write(&path, b"not json").expect("write corrupt settings");
    assert!(load_file(&path).is_err());
}
