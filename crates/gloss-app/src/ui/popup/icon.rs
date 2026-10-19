//! 页头应用图标的矢量栅格化：按目标物理像素边长渲染（含失败），失败隔离
//! 降级。
//!
//! 纯数据变换，不依赖 egui 绘制侧（`egui::ColorImage` 只是像素容器）；
//! 纹理装入随 egui 上下文，留在绘制侧的 `RenderState::icon_texture`。

use gloss_core::log::{thread, warn};

/// 应用图标矢量源：与 Dock 图标同一份设计资产（Dock 位图的渲染命令见
/// scripts/dev/build-app-icon.sh）。
const APP_ICON_SVG: &[u8] = include_bytes!("../../../../../assets/icons/gloss-app-icon.svg");
/// 应用图标画布的 Big Sur 规范比例：画布 1024、四周透明边距 100、图形本体
/// 824（SVG 源同值）；页头按此比例只栅格化图形本体，不显示透明边距。
const ICON_MARGIN: u32 = 100;
const ICON_CONTENT: u32 = 824;

/// 应用图标按给定边长（物理像素）的栅格化结果。
pub(super) fn app_icon_image(side_px: u32) -> Option<egui::ColorImage> {
    render_app_icon(APP_ICON_SVG, side_px)
}

/// 栅格化给定的 SVG 并按画布比例裁出图形本体；失败走隔离降级——记一条
/// 告警，页头退化为无图标的动作行，不影响其余内容。参数化 SVG 来源与
/// 目标边长，渲染数学与降级分支可经 L1 测试直接驱动。
fn render_app_icon(svg: &[u8], side_px: u32) -> Option<egui::ColorImage> {
    use resvg::tiny_skia::Pixmap;
    use resvg::usvg::{Options, Transform, Tree};

    let tree = match Tree::from_data(svg, &Options::default()) {
        Ok(tree) => tree,
        Err(error) => {
            warn!(
                thread = thread::UI,
                error = %error,
                "app icon failed to parse, header renders without it"
            );
            return None;
        }
    };
    // 裁剪数学假设正方形画布与四边等边距（SVG 源即如此）；资产若改版失衡，
    // debug 构建里第一时间显形。
    debug_assert_eq!(
        tree.size().width(),
        tree.size().height(),
        "app icon canvas is expected to be square"
    );
    let Some(mut pixmap) = Pixmap::new(side_px, side_px) else {
        warn!(
            thread = thread::UI,
            side_px = side_px,
            "app icon rasterization buffer rejected, header renders without it"
        );
        return None;
    };
    // 画布坐标映射：本体左上 (100, 100) 平移到原点，本体边长 824 缩放到
    // side_px——直接栅出本体，无需再裁像素缓冲。
    let scale = side_px as f32 / ICON_CONTENT as f32;
    let shift = -(ICON_MARGIN as f32) * scale;
    resvg::render(
        &tree,
        Transform::from_row(scale, 0.0, 0.0, scale, shift, shift),
        &mut pixmap.as_mut(),
    );
    Some(egui::ColorImage::from_rgba_premultiplied(
        [side_px as usize, side_px as usize],
        pixmap.data(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{APP_ICON_SVG, render_app_icon};

    #[test]
    fn render_app_icon_rejects_bad_bytes() {
        assert!(render_app_icon(b"not an svg", 40).is_none());
    }

    #[test]
    fn render_app_icon_rasterizes_the_content_square() {
        let image = render_app_icon(APP_ICON_SVG, 40).expect("embedded icon must render");
        assert_eq!(image.width(), 40);
        assert_eq!(image.height(), 40);
        let center = image.pixels[image.height() / 2 * image.width() + image.width() / 2];
        assert!(center.is_opaque(), "rasterized body must cover the center");
    }
}
