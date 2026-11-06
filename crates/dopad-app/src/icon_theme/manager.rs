use std::collections::HashSet;
use std::sync::Arc;

use super::{IconId, IconRequest};
use super::{assets, provider::IconThemeProvider, resolver::MaterialIconTheme};

pub(crate) struct IconThemeManager {
    provider: Arc<dyn IconThemeProvider>,
}

impl IconThemeManager {
    pub(crate) fn load(theme_id: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            theme_id == "material",
            "unsupported file icon theme: {theme_id}"
        );
        let manifest =
            assets::manifest().ok_or_else(|| anyhow::anyhow!("icon manifest is missing"))?;
        let available_assets: HashSet<String> = assets::material_asset_names().collect();
        let provider = MaterialIconTheme::from_manifest(&manifest, available_assets)?;
        tracing::debug!(
            theme_id = provider.id(),
            theme_name = provider.name(),
            "loaded file icon theme"
        );
        Ok(Self {
            provider: Arc::new(provider),
        })
    }

    pub(crate) fn resolve_icon(&self, request: &IconRequest<'_>) -> Option<IconId> {
        self.provider.resolve_icon(request)
    }

    #[cfg(test)]
    pub(crate) fn theme_id(&self) -> &'static str {
        self.provider.id()
    }
}

impl IconThemeProvider for IconThemeManager {
    fn id(&self) -> &'static str {
        self.provider.id()
    }

    fn name(&self) -> &'static str {
        self.provider.name()
    }

    fn resolve_icon(&self, request: &IconRequest<'_>) -> Option<IconId> {
        self.provider.resolve_icon(request)
    }
}
