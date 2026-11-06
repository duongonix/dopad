use std::sync::{Arc, OnceLock};

use gpui_kit::{Image, ImageFormat};

const PNG: &[u8] = include_bytes!("../assets/dopad-icon.png");

pub(crate) fn image() -> Arc<Image> {
    static IMAGE: OnceLock<Arc<Image>> = OnceLock::new();
    IMAGE
        .get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, PNG.to_vec())))
        .clone()
}

pub(crate) fn window_icon() -> Option<Arc<image::RgbaImage>> {
    static ICON: OnceLock<Option<Arc<image::RgbaImage>>> = OnceLock::new();
    ICON.get_or_init(|| {
        image::load_from_memory(PNG)
            .ok()
            .map(|image| Arc::new(image.to_rgba8()))
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use super::PNG;

    #[test]
    fn dopad_icon_has_transparent_background_and_visible_mark() {
        let image = image::load_from_memory(PNG)
            .expect("embedded Dopad icon is a valid image")
            .to_rgba8();
        assert_eq!(image.get_pixel(0, 0)[3], 0);
        assert_eq!(image.get_pixel(256, 256)[3], 255);
    }
}
