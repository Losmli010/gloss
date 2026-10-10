//! 经位附件图像的解码：内存 PNG → egui 像素容器。
//!
//! 纯数据变换，不依赖 egui 绘制侧（`egui::ColorImage` 只是像素容器）；
//! 纹理装入随 egui 上下文，留在绘制侧的 `RenderState::attached_texture`。

use std::sync::Arc;

use gloss_core::log::{thread, warn};

/// 解码经位附件图像（剪贴板取材产物，平台侧已做过尺寸设界）；失败隔离
/// 降级——记一条只含字节长度的告警（图像内容不落日志），绘制侧落占位，
/// 不影响其余内容。参数化 PNG 来源，解码与降级分支可经 L1 测试直接驱动。
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
