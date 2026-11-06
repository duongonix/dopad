use gpui_kit::SharedString;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileEntryKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct IconRequest<'a> {
    pub name: &'a str,
    pub kind: FileEntryKind,
    pub expanded: bool,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct IconId(SharedString);

impl IconId {
    pub(super) fn new(path: String) -> Self {
        Self(path.into())
    }

    #[cfg(test)]
    pub(crate) fn resource_path(&self) -> &str {
        self.0.as_ref()
    }

    pub(crate) fn shared_resource_path(&self) -> SharedString {
        self.0.clone()
    }
}

pub(crate) trait IconThemeProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn resolve_icon(&self, request: &IconRequest<'_>) -> Option<IconId>;
}
