//! 剪贴板图片取材读取器：读 NSPasteboard 图像 flavor → 解码 → PNG（快速压缩档）。
//!
//! 两条静默丢弃路径分开留痕（都返回 `Ok(None)`，调用方不弹卡）：
//! - **竞态**：① 观察与 ② 取材之间剪贴板被覆盖（changeCount 前进）——
//!   debug 留痕，与划词空选区误滑同型处置；
//! - **交付不出**：changeCount 未变、板上也声明了图像类型，但读不出数据
//!   ——macOS 15+ 的「允许粘贴」授权被拒走这条（`dataForType:` 返回 nil），
//!   warn 留痕供 `just logs --level warn` 排查。
//!
//! 硬失败返回 `Err`，由调用方上抛 `TaskFailed` 弹失败卡。观察比对先于内容
//! 读取：比对不过不碰 flavor（内容读取才触发系统粘贴授权）。
//!
//! 设界逐级前置：flavor 字节上限 → 仅读头取尺寸过像素面积上限 → 才整图
//! 解码 → 超长边产物按比例降采样 → 产物过 PNG 字节上限。pastebomb 在任何
//! 大分配发生之前被整体拒绝。日志只记字节数与像素尺寸。

use std::io::Cursor;
use std::sync::Arc;

use gloss_core::log::{debug, thread, warn};
use gloss_core::model::GlossError;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, ImageFormat, ImageReader};
use objc2::rc::Retained;
use objc2_foundation::NSData;

use super::{ImageFlavor, PasteboardObserver, pasteboard_change_count, pasteboard_image_flavor};

/// flavor 字节上限：恰好 4096² 的未压缩 TIFF（RGB8）约 48MB，给满尺寸的
/// 合法图像留量；超限在头解析之前整体拒绝。
pub const MAX_FLAVOR_BYTES: usize = 64 * 1024 * 1024;

/// 像素面积上限：4096×4096 ≈ 16MP（4K 截图 8.3MP 在内）；面积在上限处的
/// RGBA 位图即 64MB，超出即视为 pastebomb。
pub const MAX_PIXELS: u64 = 4096 * 4096;

/// PNG 字节上限：取材产物进 `TaskInput`、缓存会话与出网请求体，超限整体
/// 报错。
pub const MAX_PNG_BYTES: usize = 20 * 1024 * 1024;

/// 产物最长边上限：取材产物经 base64 出网并入缓存会话，超过即按比例缩到
/// 上限内——视觉模型内部本就会把输入压到 ~1MP 一档，上限外的像素只有编码
/// 与传输成本；1536 长边覆盖图表与截图文字的判读质量，同时把解码后的
/// GPU 纹理与 base64 请求体的量级压到 ~9MB 档。上限以内的产物原样编码
/// （不放大、不重采样）。
pub const MAX_PRODUCT_SIDE: u32 = 1536;

/// flavor 字节是否在预算内（纯逻辑，单测覆盖边界）。
fn flavor_within_budget(len: usize) -> bool {
    len <= MAX_FLAVOR_BYTES
}

/// 像素面积是否在预算内（纯逻辑，单测覆盖边界）。
fn pixels_within_budget(width: u32, height: u32) -> bool {
    u64::from(width) * u64::from(height) <= MAX_PIXELS
}

/// PNG 字节是否在预算内（纯逻辑，单测覆盖边界）。
fn png_within_budget(len: usize) -> bool {
    len <= MAX_PNG_BYTES
}

/// 超限产物按比例缩到最长边上限内的目标尺寸（纯逻辑，单测覆盖）：上限
/// 以内返回 `None`（原样编码）；缩放按比例四舍五入、至少保留 1 像素。
fn downscaled_dims(width: u32, height: u32) -> Option<(u32, u32)> {
    let longest = width.max(height);
    if longest <= MAX_PRODUCT_SIDE {
        return None;
    }
    let scale = f64::from(MAX_PRODUCT_SIDE) / f64::from(longest);
    let scaled = |side: u32| ((f64::from(side) * scale).round() as u32).max(1);
    Some((scaled(width), scaled(height)))
}

/// 图像 flavor 编码数据 → 解码 RGBA → 超长边降采样 → PNG（快速压缩档：
/// 编码在事件线程上执行，时长直接挂在触发到弹卡的链路上）。
///
/// 设界依次执行：flavor 字节上限在头解析之前，像素面积上限以仅读头的
/// 尺寸判定、先于整图解码，超 [`MAX_PRODUCT_SIDE`] 的产物按比例降采样，
/// PNG 字节上限判在编码产物上。超限整体报 `ImageTooLarge`（不截断）；
/// 头不可读、解码失败、缓冲与尺寸不符都按读取失败处理。
fn decode_to_png(flavor: ImageFlavor, bytes: &[u8]) -> Result<Arc<[u8]>, GlossError> {
    if !flavor_within_budget(bytes.len()) {
        debug!(
            thread = thread::EVENT,
            bytes = bytes.len(),
            "pasteboard image exceeds the flavor budget"
        );
        return Err(GlossError::ImageTooLarge);
    }
    let format = match flavor {
        ImageFlavor::Tiff => ImageFormat::Tiff,
        ImageFlavor::Png => ImageFormat::Png,
    };
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|err| {
            debug!(
                thread = thread::EVENT,
                error = %err,
                "pasteboard image header unreadable"
            );
            GlossError::ImageUnavailable
        })?;
    if !pixels_within_budget(width, height) {
        debug!(
            thread = thread::EVENT,
            width, height, "pasteboard image exceeds the pixel budget"
        );
        return Err(GlossError::ImageTooLarge);
    }
    let decoded = ImageReader::with_format(Cursor::new(bytes), format)
        .decode()
        .map_err(|err| {
            debug!(
                thread = thread::EVENT,
                error = %err,
                "pasteboard image decode failed"
            );
            GlossError::ImageUnavailable
        })?;
    let rgba = decoded.to_rgba8();
    let (width, height, rgba) = match downscaled_dims(rgba.width(), rgba.height()) {
        Some((width, height)) => {
            debug!(
                thread = thread::EVENT,
                source_width = rgba.width(),
                source_height = rgba.height(),
                width,
                height,
                "pasteboard image downscaled for the wire"
            );
            let resized = image::imageops::resize(
                &rgba,
                width,
                height,
                image::imageops::FilterType::Triangle,
            );
            (width, height, resized)
        }
        None => (rgba.width(), rgba.height(), rgba),
    };
    encode_png(width, height, rgba.as_raw())
}

/// RGBA 位图 → PNG。缓冲与声明尺寸不符按读取失败处理（校验在前，
/// `write_image` 的内部校验兜底）。
fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Arc<[u8]>, GlossError> {
    if rgba.len() != width as usize * height as usize * 4 {
        debug!(
            thread = thread::EVENT,
            bytes = rgba.len(),
            width,
            height,
            "RGBA buffer does not match the declared size"
        );
        return Err(GlossError::ImageUnavailable);
    }
    let mut png = Vec::new();
    if let Err(err) =
        PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::NoFilter)
            .write_image(rgba, width, height, ExtendedColorType::Rgba8)
    {
        debug!(thread = thread::EVENT, error = %err, "png encoding failed");
        return Err(GlossError::ImageUnavailable);
    }
    if !png_within_budget(png.len()) {
        debug!(
            thread = thread::EVENT,
            bytes = png.len(),
            "pasteboard image exceeds the byte budget"
        );
        return Err(GlossError::ImageTooLarge);
    }
    Ok(Arc::from(png))
}

/// 剪贴板图片读取器：通道②的取材实现（事件线程上顺序调用）。
pub struct ClipboardImageReader {
    /// ① 观察记录的句柄：取材时比对现值判定①②之间的覆盖竞态。
    observer: PasteboardObserver,
}

impl ClipboardImageReader {
    /// 创建读取器：句柄来自同一次装配的 [`ClipboardWatchSource::observer`]。
    pub fn new(observer: PasteboardObserver) -> Self {
        Self { observer }
    }

    /// 读剪贴板图片为 PNG 字节：`Ok(None)` = 静默丢弃（竞态或交付不出，
    /// 读取器内已按分支留痕）；`Err` = 取材失败（调用方上抛 `TaskFailed`）。
    pub fn read(&mut self) -> Result<Option<Arc<[u8]>>, GlossError> {
        self.read_with(pasteboard_change_count, pasteboard_image_flavor)
    }

    /// 一轮读取（系统读数注入，纯逻辑单测覆盖竞态与无数据的分岔——与
    /// 哨兵源的 `poll_at` 同型）：
    ///
    /// - 板不可观察：读取失败上抛；
    /// - changeCount 与①观察记录不一致：①②之间被覆盖，竞态静默丢弃，
    ///   不读内容（比对先于 flavor——内容读取才触发系统粘贴授权）；
    /// - 板仍声明图像却交付不出数据：粘贴授权被拒走这条，warn 留痕后
    ///   静默丢弃；
    /// - 可读：解码为 PNG。
    fn read_with(
        &mut self,
        count: impl FnOnce() -> Option<u64>,
        flavor: impl FnOnce() -> Option<(ImageFlavor, Retained<NSData>)>,
    ) -> Result<Option<Arc<[u8]>>, GlossError> {
        let Some(current) = count() else {
            debug!(
                thread = thread::EVENT,
                "pasteboard unavailable, image read declined"
            );
            return Err(GlossError::ImageUnavailable);
        };
        if !self.observer.matches(current) {
            debug!(
                thread = thread::EVENT,
                "clipboard changed since the observation, image dropped"
            );
            return Ok(None);
        }
        let Some((flavor, data)) = flavor() else {
            warn!(
                thread = thread::EVENT,
                "the pasteboard declares an image but delivers none (paste permission may be denied), image dropped"
            );
            return Ok(None);
        };
        // SAFETY: `data`（Retained<NSData>）在本作用域内存活且不被改写，切片
        // 的借用不逃逸出本函数——解码产物即刻转为持有字节。
        let bytes = unsafe { data.as_bytes_unchecked() };
        decode_to_png(flavor, bytes).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    const RGBA_2X2: [u8; 16] = [
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 0,
    ];

    fn encoded_2x2(format: ImageFormat) -> Vec<u8> {
        let canvas = image::RgbaImage::from_raw(2, 2, RGBA_2X2.to_vec()).expect("canvas");
        let mut bytes = Cursor::new(Vec::new());
        canvas.write_to(&mut bytes, format).expect("encode");
        bytes.into_inner()
    }

    fn patch_tiff_dimension_tag(bytes: &mut [u8], tag: u16, value: u32) {
        let mut cursor = 0;
        while cursor + 12 <= bytes.len() {
            if bytes[cursor..cursor + 2] == tag.to_le_bytes()
                && bytes[cursor + 2..cursor + 4] == 4u16.to_le_bytes()
                && bytes[cursor + 4..cursor + 8] == 1u32.to_le_bytes()
            {
                bytes[cursor + 8..cursor + 12].copy_from_slice(&value.to_le_bytes());
                return;
            }
            cursor += 1;
        }
        panic!("dimension tag must exist in the encoded tiff");
    }

    #[test]
    fn png_and_tiff_flavors_decode_to_png() {
        for (flavor, bytes) in [
            (ImageFlavor::Png, encoded_2x2(ImageFormat::Png)),
            (ImageFlavor::Tiff, encoded_2x2(ImageFormat::Tiff)),
        ] {
            let png = decode_to_png(flavor, &bytes).expect("decode must succeed");
            let decoded = image::load_from_memory(&png).expect("valid png");
            assert_eq!((decoded.width(), decoded.height()), (2, 2));
        }
    }

    #[test]
    fn flavor_budget_rejects_bytes_over_the_limit() {
        assert!(
            flavor_within_budget(MAX_FLAVOR_BYTES),
            "the limit itself fits"
        );
        assert!(
            !flavor_within_budget(MAX_FLAVOR_BYTES + 1),
            "one byte over the limit is rejected"
        );
        let over = vec![0u8; MAX_FLAVOR_BYTES + 1];
        let err = decode_to_png(ImageFlavor::Png, &over).expect_err("over-budget flavor");
        assert_eq!(
            err,
            GlossError::ImageTooLarge,
            "the flavor cap fires before any header parsing"
        );
    }

    #[test]
    fn header_dimensions_are_bounded_before_the_decode() {
        let mut bytes = encoded_2x2(ImageFormat::Tiff);
        patch_tiff_dimension_tag(&mut bytes, 256, 4097);
        patch_tiff_dimension_tag(&mut bytes, 257, 4096);
        let err = decode_to_png(ImageFlavor::Tiff, &bytes).expect_err("over-budget image");
        assert_eq!(
            err,
            GlossError::ImageTooLarge,
            "the header gate fires before the full decode"
        );
    }

    #[test]
    fn a_truncated_flavor_decodes_to_a_read_failure() {
        let bytes = encoded_2x2(ImageFormat::Png);
        let truncated = &bytes[..40];
        let err = decode_to_png(ImageFlavor::Png, truncated).expect_err("truncated flavor");
        assert_eq!(err, GlossError::ImageUnavailable);
    }

    #[test]
    fn pixel_budget_rejects_area_over_the_limit() {
        assert!(pixels_within_budget(4096, 4096), "the limit itself fits");
        assert!(
            !pixels_within_budget(4097, 4096),
            "one pixel over the limit is rejected"
        );
    }

    #[test]
    fn a_mismatched_rgba_buffer_is_a_read_failure() {
        let err = encode_png(2, 2, &[1, 2, 3]).expect_err("buffer mismatch");
        assert_eq!(err, GlossError::ImageUnavailable);
    }

    #[test]
    fn pixel_budget_is_checked_before_the_buffer() {
        let err = encode_png(4096, 4096, &[]).expect_err("buffer mismatch");
        assert_eq!(
            err,
            GlossError::ImageUnavailable,
            "within-budget dims fall through to the buffer check"
        );
    }

    #[test]
    fn a_covered_board_is_dropped_without_touching_the_content() {
        let observer = PasteboardObserver::default();
        observer.record(7);
        let mut reader = ClipboardImageReader::new(observer);
        let read = reader.read_with(
            || Some(8),
            || panic!("content must not be read when the observation no longer matches"),
        );
        assert_eq!(read.expect("a race drops, not fails"), None);
    }

    #[test]
    fn an_undeliverable_flavor_is_dropped_quietly() {
        let observer = PasteboardObserver::default();
        observer.record(7);
        let mut reader = ClipboardImageReader::new(observer);
        let read = reader.read_with(|| Some(7), || None);
        assert_eq!(read.expect("undelivered drops, not fails"), None);
    }

    #[test]
    fn an_unobservable_board_is_a_read_failure() {
        let mut reader = ClipboardImageReader::new(PasteboardObserver::default());
        let err = reader
            .read_with(
                || None,
                || panic!("no flavor query without a readable count"),
            )
            .expect_err("unobservable board");
        assert_eq!(err, GlossError::ImageUnavailable);
    }

    #[test]
    fn a_matching_board_still_decodes_to_png() {
        let observer = PasteboardObserver::default();
        observer.record(7);
        let mut reader = ClipboardImageReader::new(observer);
        let tiff = encoded_2x2(ImageFormat::Tiff);
        let png = reader
            .read_with(
                || Some(7),
                || Some((ImageFlavor::Tiff, NSData::with_bytes(&tiff))),
            )
            .expect("read must succeed")
            .expect("the observed image must decode");
        let decoded = image::load_from_memory(&png).expect("valid png");
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
    }

    #[test]
    fn byte_budget_rejects_length_over_the_limit() {
        assert!(png_within_budget(MAX_PNG_BYTES), "the limit itself fits");
        assert!(
            !png_within_budget(MAX_PNG_BYTES + 1),
            "one byte over the limit is rejected"
        );
    }

    #[test]
    fn downscaled_dims_preserve_aspect_and_the_cap() {
        assert_eq!(
            downscaled_dims(1536, 1536),
            None,
            "at the cap the product encodes as-is"
        );
        assert_eq!(
            downscaled_dims(1024, 768),
            None,
            "under the cap is untouched"
        );
        assert_eq!(downscaled_dims(4096, 4096), Some((1536, 1536)));
        assert_eq!(downscaled_dims(4096, 100), Some((1536, 38)));
        assert_eq!(
            downscaled_dims(4097, 1),
            Some((1536, 1)),
            "rounding never loses a dimension entirely"
        );
        assert_eq!(downscaled_dims(1, 4097), Some((1, 1536)));
        let (width, height) = downscaled_dims(3000, 2049).expect("oversized product");
        assert_eq!(width, 1536);
        assert_eq!(height, 1049, "the short side follows the same scale");
    }

    #[test]
    fn oversized_products_are_downscaled_to_the_side_cap() {
        let canvas = image::RgbaImage::from_fn(2100, 1000, |x, y| {
            [(x % 256) as u8, (y % 256) as u8, 0, 255].into()
        });
        let mut bytes = Cursor::new(Vec::new());
        canvas
            .write_to(&mut bytes, ImageFormat::Png)
            .expect("encode");
        let png = decode_to_png(ImageFlavor::Png, bytes.into_inner().as_slice())
            .expect("an oversized product is downscaled, not rejected");
        let decoded = image::load_from_memory(&png).expect("valid png");
        assert_eq!(
            (decoded.width(), decoded.height()),
            (1536, 731),
            "the product lands within the side cap with the aspect preserved"
        );
    }
}
