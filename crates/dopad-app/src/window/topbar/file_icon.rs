use crate::{
    icon_theme::{FileEntryKind, IconImageCache, IconRequest, IconThemeManager},
    theme::Palette,
};
use gpui_kit::component::{Icon, Sizable, Size};
use gpui_kit::{AnyElement, Entity, IntoElement, Styled, StyledImage, img, px, rgb};

pub(super) fn tab_file_icon(
    name: &str,
    icon_theme: Option<&IconThemeManager>,
    icon_cache: Option<&Entity<IconImageCache>>,
    palette: &Palette,
) -> AnyElement {
    let request = IconRequest {
        name,
        kind: FileEntryKind::File,
        expanded: false,
    };
    let Some(icon_id) = icon_theme.and_then(|theme| theme.resolve_icon(&request)) else {
        return fallback_icon(palette);
    };
    let fallback_color = palette.secondary;
    let mut image = img(icon_id.shared_resource_path())
        .size(px(14.))
        .with_fallback(move || {
            Icon::new(gpui_kit::assets::IconName::File)
                .with_size(Size::Size(px(14.)))
                .text_color(rgb(fallback_color))
                .into_any_element()
        });
    if let Some(cache) = icon_cache {
        image = image.image_cache(cache);
    }
    image.into_any_element()
}

fn fallback_icon(palette: &Palette) -> AnyElement {
    Icon::new(gpui_kit::assets::IconName::File)
        .with_size(Size::Size(px(14.)))
        .text_color(rgb(palette.secondary))
        .into_any_element()
}
