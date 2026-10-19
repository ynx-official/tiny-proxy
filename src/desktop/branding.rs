//! The in-window brand uses the same Relay Gate artwork as the executable.

use gpui_kit::{Image, ImageFormat};
use std::sync::{Arc, OnceLock};

pub(super) fn image() -> Arc<Image> {
    static IMAGE: OnceLock<Arc<Image>> = OnceLock::new();
    IMAGE
        .get_or_init(|| {
            Arc::new(Image::from_bytes(
                ImageFormat::Png,
                include_bytes!("../../assets/icons/png/tiny-proxy-128.png").to_vec(),
            ))
        })
        .clone()
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_brand_embeds_our_png_without_an_installed_sidecar_path() {
        let image = super::image();
        assert_eq!(
            image.bytes.as_slice(),
            include_bytes!("../../assets/icons/png/tiny-proxy-128.png")
        );
        assert_eq!(&image.bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(
            u32::from_be_bytes(image.bytes[16..20].try_into().unwrap()),
            128
        );
        assert_eq!(
            u32::from_be_bytes(image.bytes[20..24].try_into().unwrap()),
            128
        );
    }
}
