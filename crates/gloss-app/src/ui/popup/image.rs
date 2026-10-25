//! 经位附件图像的解码：内存图像（PNG）→ 界面库（egui）像素容器。
//!
//! 纯数据变换，不依赖界面库绘制侧（像素容器只是数据载体）；
//! 纹理装入随界面库上下文，留在绘制侧的附件纹理装入点。

use std::sync::Arc;

use gloss_core::log::{thread, warn};

pub(super) fn decode_attached(png: &Arc<[u8]>) -> Option<egui::ColorImage> {
    use image::GenericImageView;
    let decoded = match image::load_from_memory(png) {
        Ok(decoded) => decoded,
        Err(error) => {
            warn!(
                thread = thread::UI,
                len = png.len(),
                error = %error,
                "attached image failed to decode, the card renders a placeholder"
            );
            return None;
        }
    };
    let (width, height) = decoded.dimensions();
    let rgba = decoded.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [width as usize, height as usize],
        rgba.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::decode_attached;

    #[test]
    fn decode_attached_rejects_bad_bytes() {
        assert!(decode_attached(&Arc::from(&b"not a png"[..])).is_none());
    }

    #[test]
    fn decode_attached_keeps_the_pixel_dimensions() {
        let png = fixture_png(4, 2);
        let image = decode_attached(&png).expect("fixture must decode");
        assert_eq!(image.width(), 4);
        assert_eq!(image.height(), 2);
    }

    fn fixture_png(width: u32, height: u32) -> Arc<[u8]> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba([191, 97, 106, 255]));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("fixture encodes");
        bytes.into()
    }
}
