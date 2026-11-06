use std::collections::{HashMap, HashSet};

use super::{FileEntryKind, IconId, IconRequest};
use super::{manifest, provider::IconThemeProvider};

pub(crate) struct MaterialIconTheme {
    file_names: HashMap<String, IconId>,
    file_extensions: HashMap<String, IconId>,
    folder_names: HashMap<String, IconId>,
    folder_names_expanded: HashMap<String, IconId>,
    file_default: Option<IconId>,
    folder_default: Option<IconId>,
    folder_open_default: Option<IconId>,
}

impl MaterialIconTheme {
    pub(super) fn from_manifest(
        bytes: &[u8],
        available_assets: HashSet<String>,
    ) -> anyhow::Result<Self> {
        let manifest = manifest::parse(bytes, &available_assets)?;
        if manifest.skipped_icon_definitions > 0 || manifest.skipped_associations > 0 {
            tracing::warn!(
                skipped_icons = manifest.skipped_icon_definitions,
                skipped_associations = manifest.skipped_associations,
                "Material icon metadata contains unavailable or unsafe assets"
            );
        }
        let mut theme = Self {
            file_names: manifest.file_names,
            file_extensions: manifest.file_extensions,
            folder_names: manifest.folder_names,
            folder_names_expanded: manifest.folder_names_expanded,
            file_default: manifest.file_default,
            folder_default: manifest.folder_default,
            folder_open_default: manifest.folder_open_default,
        };
        // The upstream pack has no Cargo.toml association or cargo.svg; use its Rust icon.
        if let Some(rust_icon) = theme.file_extensions.get("rs").cloned() {
            theme
                .file_names
                .entry("cargo.toml".into())
                .or_insert(rust_icon);
        }
        Ok(theme)
    }
}

impl IconThemeProvider for MaterialIconTheme {
    fn id(&self) -> &'static str {
        "material"
    }

    fn name(&self) -> &'static str {
        "Material Icon Theme"
    }

    fn resolve_icon(&self, request: &IconRequest<'_>) -> Option<IconId> {
        match request.kind {
            FileEntryKind::File => self.resolve_file(request.name),
            FileEntryKind::Directory => self.resolve_folder(request.name, request.expanded),
        }
    }
}

impl MaterialIconTheme {
    fn resolve_file(&self, name: &str) -> Option<IconId> {
        if let Some(icon) = lookup(&self.file_names, name) {
            return Some(icon.clone());
        }
        for (dot, _) in name.match_indices('.') {
            let extension = &name[dot + 1..];
            if !extension.is_empty()
                && let Some(icon) = lookup(&self.file_extensions, extension)
            {
                return Some(icon.clone());
            }
        }
        self.file_default.clone()
    }

    fn resolve_folder(&self, name: &str, expanded: bool) -> Option<IconId> {
        if expanded && let Some(icon) = lookup(&self.folder_names_expanded, name) {
            return Some(icon.clone());
        }
        if let Some(icon) = lookup(&self.folder_names, name) {
            return Some(icon.clone());
        }
        if expanded {
            self.folder_open_default
                .clone()
                .or_else(|| self.folder_default.clone())
        } else {
            self.folder_default.clone()
        }
    }
}

fn lookup<'a>(map: &'a HashMap<String, IconId>, key: &str) -> Option<&'a IconId> {
    map.get(key).or_else(|| {
        if key.is_ascii() {
            map.get(&key.to_ascii_lowercase())
        } else {
            map.get(&key.to_lowercase())
        }
    })
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    fn theme() -> MaterialIconTheme {
        let manifest = br#"{
            "iconDefinitions": {
                "file": { "iconPath": "./../icons/file.svg" },
                "folder": { "iconPath": "./../icons/folder.svg" },
                "folder-open": { "iconPath": "./../icons/folder-open.svg" },
                "rust": { "iconPath": "./../icons/rust.svg" },
                "json": { "iconPath": "./../icons/json.svg" },
                "archive": { "iconPath": "./../icons/archive.svg" },
                "docker": { "iconPath": "./../icons/docker.svg" },
                "readme": { "iconPath": "./../icons/readme.svg" },
                "git": { "iconPath": "./../icons/git.svg" },
                "src": { "iconPath": "./../icons/folder-src.svg" },
                "src-open": { "iconPath": "./../icons/folder-src-open.svg" },
                "readme-light": { "iconPath": "./../icons/readme_light.svg" }
            },
            "file": "file",
            "folder": "folder",
            "folderExpanded": "folder-open",
            "fileNames": { "Dockerfile": "docker", "README.md": "readme", ".gitignore": "git" },
            "fileExtensions": { "rs": "rust", "json": "json", "tar.gz": "archive" },
            "folderNames": { "src": "src" },
            "folderNamesExpanded": { "src": "src-open" },
            "light": { "fileNames": { "README.md": "readme-light" } }
        }"#;
        let assets = [
            "icons/file.svg",
            "icons/folder.svg",
            "icons/folder-open.svg",
            "icons/rust.svg",
            "icons/json.svg",
            "icons/archive.svg",
            "icons/docker.svg",
            "icons/readme.svg",
            "icons/git.svg",
            "icons/folder-src.svg",
            "icons/folder-src-open.svg",
            "icons/readme_light.svg",
        ]
        .into_iter()
        .map(str::to_owned);
        MaterialIconTheme::from_manifest(manifest, assets.collect())
            .expect("valid fixture manifest")
    }

    fn resolve(
        theme: &impl IconThemeProvider,
        name: &str,
        kind: FileEntryKind,
        expanded: bool,
    ) -> IconId {
        theme
            .resolve_icon(&IconRequest {
                name,
                kind,
                expanded,
            })
            .expect("fixture provides a fallback")
    }

    #[test]
    fn resolves_exact_names_before_compound_and_regular_extensions() {
        let theme = theme();
        assert!(
            resolve(&theme, "DOCKERFILE", FileEntryKind::File, false)
                .resource_path()
                .ends_with("docker.svg")
        );
        assert!(
            resolve(&theme, "backup.tar.gz", FileEntryKind::File, false)
                .resource_path()
                .ends_with("archive.svg")
        );
        assert!(
            resolve(&theme, "main.rs", FileEntryKind::File, false)
                .resource_path()
                .ends_with("rust.svg")
        );
        assert!(
            resolve(&theme, ".gitignore", FileEntryKind::File, false)
                .resource_path()
                .ends_with("git.svg")
        );
    }

    #[test]
    fn resolves_open_folders_and_uses_light_overrides() {
        let theme = theme();
        let closed = resolve(&theme, "src", FileEntryKind::Directory, false);
        let open = resolve(&theme, "src", FileEntryKind::Directory, true);
        assert!(closed.resource_path().ends_with("folder-src.svg"));
        assert!(open.resource_path().ends_with("folder-src-open.svg"));
        assert!(
            resolve(&theme, "README.md", FileEntryKind::File, false)
                .resource_path()
                .ends_with("readme_light.svg")
        );
    }

    #[test]
    fn default_icons_and_unicode_names_are_safe() {
        let theme = theme();
        assert!(
            resolve(&theme, "unknown.bin", FileEntryKind::File, false)
                .resource_path()
                .ends_with("file.svg")
        );
        assert!(
            resolve(&theme, "資料.rs", FileEntryKind::File, false)
                .resource_path()
                .ends_with("rust.svg")
        );
    }

    #[test]
    fn malformed_manifest_and_missing_svg_references_fail_safely() {
        assert!(MaterialIconTheme::from_manifest(b"{broken", HashSet::new()).is_err());
        let manifest = br#"{
            "iconDefinitions": {
                "escape": { "iconPath": "./../icons/../outside.svg" }
            },
            "file": "missing",
            "folder": "missing",
            "folderExpanded": "missing",
            "fileExtensions": { "rs": "escape" }
        }"#;
        let theme = MaterialIconTheme::from_manifest(
            manifest,
            HashSet::from(["icons/../outside.svg".to_owned()]),
        )
        .expect("malformed references are ignored");
        assert!(
            theme
                .resolve_icon(&IconRequest {
                    name: "main.rs",
                    kind: FileEntryKind::File,
                    expanded: false,
                })
                .is_none()
        );
    }

    #[test]
    fn repeated_file_associations_share_the_same_icon_id() {
        let theme = theme();
        let rust_a = resolve(&theme, "one.rs", FileEntryKind::File, false);
        let rust_b = resolve(&theme, "two.rs", FileEntryKind::File, false);
        assert_eq!(rust_a, rust_b);
    }

    #[test]
    fn loads_official_metadata_and_references_only_packaged_svg_assets() {
        let theme = super::super::manager::IconThemeManager::load("material")
            .expect("official Material pack is valid");
        assert_eq!(theme.theme_id(), "material");
        for (name, suffix) in [
            ("package.json", "nodejs.svg"),
            ("main.rs", "rust.svg"),
            ("Cargo.toml", "rust.svg"),
            (".gitignore", "git.svg"),
            ("README.md", "readme.svg"),
        ] {
            assert!(
                resolve(&theme, name, FileEntryKind::File, false)
                    .resource_path()
                    .ends_with(suffix)
            );
        }
        let manifest = super::super::assets::manifest().expect("manifest is packaged");
        let assets = super::super::assets::material_asset_names().collect::<HashSet<_>>();
        let parsed = manifest::parse(&manifest, &assets).expect("manifest parses");
        assert_eq!(parsed.skipped_icon_definitions, 0);
        assert_eq!(parsed.skipped_associations, 0);
    }

    #[test]
    #[ignore = "manual optimized resolver benchmark; run with --release --ignored"]
    fn icon_theme_resolution_benchmark() {
        let started = Instant::now();
        let theme = super::super::manager::IconThemeManager::load("material")
            .expect("official Material pack is valid");
        let parse_time = started.elapsed();
        println!("manifest_init={parse_time:?}");

        for count in [100, 1_000, 10_000] {
            let names = (0..count)
                .map(|index| match index % 4 {
                    0 => format!("module-{index}.rs"),
                    1 => format!("view-{index}.tsx"),
                    2 => format!("settings-{index}.json"),
                    _ => format!("README-{index}.md"),
                })
                .collect::<Vec<_>>();
            let started = Instant::now();
            let mut resolved = 0usize;
            for _ in 0..100 {
                for name in &names {
                    resolved += usize::from(
                        theme
                            .resolve_icon(&IconRequest {
                                name,
                                kind: FileEntryKind::File,
                                expanded: false,
                            })
                            .is_some(),
                    );
                }
            }
            let elapsed = started.elapsed();
            println!(
                "entries={count} passes=100 total={elapsed:?} avg_per_pass={:?} resolved={resolved}",
                elapsed / 100
            );
        }
    }
}
