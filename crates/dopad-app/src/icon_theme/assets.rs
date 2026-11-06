use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString, assets::AllAssets};

const MATERIAL_RESOURCE_PREFIX: &str = "dopad/material/";

#[derive(rust_embed::RustEmbed)]
#[folder = "assets/icon-themes/material"]
struct MaterialAssets;

pub(crate) struct DopadAssets;

impl AssetSource for DopadAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(material_path) = path.strip_prefix(MATERIAL_RESOURCE_PREFIX) {
            return Ok(MaterialAssets::get(material_path).map(|asset| asset.data));
        }
        AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = AllAssets.list(path)?;
        if let Some(material_path) = path.strip_prefix(MATERIAL_RESOURCE_PREFIX) {
            assets.extend(
                MaterialAssets::iter()
                    .filter(|name| name.starts_with(material_path))
                    .map(|name| format!("{MATERIAL_RESOURCE_PREFIX}{name}").into()),
            );
        }
        Ok(assets)
    }
}

pub(super) fn manifest() -> Option<Cow<'static, [u8]>> {
    MaterialAssets::get("manifest.json").map(|asset| asset.data)
}

pub(super) fn material_asset_names() -> impl Iterator<Item = String> {
    MaterialAssets::iter().map(|name| name.into_owned())
}

pub(super) fn asset_path(icon_file: &str) -> String {
    format!("{MATERIAL_RESOURCE_PREFIX}icons/{icon_file}")
}

#[cfg(test)]
mod tests {
    use gpui_kit::AssetSource;

    use super::DopadAssets;

    #[test]
    fn asset_source_loads_requested_svg_and_handles_missing_files() {
        let assets = DopadAssets;
        let rust_icon = assets
            .load("dopad/material/icons/rust.svg")
            .expect("asset source is valid")
            .expect("Rust icon is packaged");
        assert!(rust_icon.starts_with(b"<svg"));
        assert!(
            assets
                .load("dopad/material/icons/does-not-exist.svg")
                .expect("missing assets are handled")
                .is_none()
        );
    }
}
