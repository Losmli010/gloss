//! 页头应用图标的解码与裁剪：进程内只解码一次（含失败），失败隔离降级。
//!
//! 纯数据变换，不依赖 egui 绘制侧（`egui::ColorImage` 只是像素容器）；
//! 纹理装入随 egui 上下文，留在绘制侧的 `RenderState::icon_texture`。

use std::sync::OnceLock;

use gloss_core::log::{thread, warn};

/// 应用图标 PNG：与 Dock 图标同一份设计资产（矢量源与画布说明见
/// assets/icons/gloss-app-icon.svg）。
const APP_ICON_PNG: &[u8] = include_bytes!("../../../../../assets/icons/gloss-dock-icon.png");
/// 应用图标画布的 Big Sur 规范比例：画布 1024、四周透明边距 100、图形本体
/// 824（SVG 源同值）；页头按此比例裁出图形本体，不显示透明边距。
const ICON_CANVAS: u32 = 1024;
const ICON_MARGIN: u32 = 100;
const ICON_CONTENT: u32 = 824;

/// 应用图标的解码结果：进程内只解码一次（含失败）。
pub(super) fn app_icon_image() -> Option<egui::ColorImage> {
    static DECODED: OnceLock<Option<egui::ColorImage>> = OnceLock::new();
    DECODED
        .get_or_init(|| decode_app_icon(APP_ICON_PNG))
        .clone()
}

/// 解码给定的 PNG 字节并按画布比例裁出图形本体；失败走隔离降级——记一条
/// 告警，页头退化为无图标的动作行，不影响其余内容。参数化 PNG 来源，
/// 裁剪数学与降级分支可经 L1 测试直接驱动。
fn decode_app_icon(png: &[u8]) -> Option<egui::ColorImage> {
    use image::GenericImageView;
    let decoded = match image::load_from_memory(png) {
        Ok(decoded) => decoded,
        Err(error) => {
            warn!(
                thread = thread::UI,
                error = %error,
                "app icon failed to decode, header renders without it"
            );
            return None;
        }
    };
    let (width, height) = decoded.dimensions();
    // 裁剪数学假设正方形画布与四边等边距（SVG 源即如此）；资产若改版失衡，
    // debug 构建里第一时间显形。
    debug_assert_eq!(width, height, "app icon canvas is expected to be square");
    let margin = width * ICON_MARGIN / ICON_CANVAS;
    let content = width * ICON_CONTENT / ICON_CANVAS;
    let rgba = decoded
        .crop_imm(margin, margin, content, content)
        .to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [content as usize, content as usize],
        rgba.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{APP_ICON_PNG, decode_app_icon};

    #[test]
    fn decode_app_icon_rejects_bad_bytes() {
        assert!(decode_app_icon(b"not a png").is_none());
    }

    #[test]
    fn decode_app_icon_crops_to_the_content_square() {
        let image = decode_app_icon(APP_ICON_PNG).expect("embedded icon must decode");
        assert_eq!(image.width(), 206, "256 * 824 / 1024");
        assert_eq!(image.height(), 206, "256 * 824 / 1024");
    }
}
