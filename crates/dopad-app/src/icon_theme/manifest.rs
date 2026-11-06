use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Deserialize;

use super::{assets, provider::IconId};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawManifest {
    #[serde(default)]
    icon_definitions: BTreeMap<String, IconDefinition>,
    #[serde(default)]
    file_extensions: BTreeMap<String, String>,
    #[serde(default)]
    file_names: BTreeMap<String, String>,
    #[serde(default)]
    folder_names: BTreeMap<String, String>,
    #[serde(default)]
    folder_names_expanded: BTreeMap<String, String>,
    #[serde(default)]
    file: String,
    #[serde(default)]
    folder: String,
    #[serde(default)]
    folder_expanded: String,
    #[serde(default)]
    light: Option<LightOverrides>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IconDefinition {
    icon_path: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LightOverrides {
    #[serde(default)]
    file_extensions: BTreeMap<String, String>,
    #[serde(default)]
    file_names: BTreeMap<String, String>,
    #[serde(default)]
    folder_names: BTreeMap<String, String>,
    #[serde(default)]
    folder_names_expanded: BTreeMap<String, String>,
}

pub(super) struct ParsedManifest {
    pub file_names: HashMap<String, IconId>,
    pub file_extensions: HashMap<String, IconId>,
    pub folder_names: HashMap<String, IconId>,
    pub folder_names_expanded: HashMap<String, IconId>,
    pub file_default: Option<IconId>,
    pub folder_default: Option<IconId>,
    pub folder_open_default: Option<IconId>,
    pub skipped_icon_definitions: usize,
    pub skipped_associations: usize,
}

pub(super) fn parse(
    bytes: &[u8],
    available_assets: &HashSet<String>,
) -> anyhow::Result<ParsedManifest> {
    let mut raw: RawManifest = serde_json::from_slice(bytes)?;
    if let Some(light) = raw.light.take() {
        raw.file_extensions.extend(light.file_extensions);
        raw.file_names.extend(light.file_names);
        raw.folder_names.extend(light.folder_names);
        raw.folder_names_expanded
            .extend(light.folder_names_expanded);
    }

    let mut skipped_icon_definitions = 0;
    let mut icons = HashMap::with_capacity(raw.icon_definitions.len());
    for (id, definition) in raw.icon_definitions {
        let Some(file_name) = safe_icon_file_name(&definition.icon_path) else {
            skipped_icon_definitions += 1;
            continue;
        };
        if !available_assets.contains(&format!("icons/{file_name}")) {
            skipped_icon_definitions += 1;
            continue;
        }
        icons.insert(id, IconId::new(assets::asset_path(file_name)));
    }

    let mut skipped_associations = 0;
    let file_names = resolve_map(raw.file_names, &icons, false, &mut skipped_associations);
    let file_extensions = resolve_map(raw.file_extensions, &icons, true, &mut skipped_associations);
    let folder_names = resolve_map(raw.folder_names, &icons, false, &mut skipped_associations);
    let folder_names_expanded = resolve_map(
        raw.folder_names_expanded,
        &icons,
        false,
        &mut skipped_associations,
    );

    Ok(ParsedManifest {
        file_default: icons.get(&raw.file).cloned(),
        folder_default: icons.get(&raw.folder).cloned(),
        folder_open_default: icons.get(&raw.folder_expanded).cloned(),
        file_names,
        file_extensions,
        folder_names,
        folder_names_expanded,
        skipped_icon_definitions,
        skipped_associations,
    })
}

fn resolve_map(
    entries: BTreeMap<String, String>,
    icons: &HashMap<String, IconId>,
    extension_keys: bool,
    skipped: &mut usize,
) -> HashMap<String, IconId> {
    let mut resolved = HashMap::with_capacity(entries.len());
    for (key, icon_id) in entries {
        let key = if extension_keys {
            key.trim_start_matches('.')
        } else {
            &key
        };
        let Some(icon) = icons.get(&icon_id) else {
            *skipped += 1;
            continue;
        };
        resolved.insert(key.to_lowercase(), icon.clone());
    }
    resolved
}

fn safe_icon_file_name(path: &str) -> Option<&str> {
    let file_name = path.strip_prefix("./../icons/")?;
    if file_name.is_empty()
        || !file_name.ends_with(".svg")
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name == ".svg"
    {
        return None;
    }
    Some(file_name)
}
