//! 剪贴板图片取材读取器：读 NSPasteboard 图像 flavor → 解码 → PNG（快速压缩档）。
//!
//! 竞态兜底：① 观察与 ② 取材之间剪贴板可能被覆盖——changeCount 再变，或
//! 内容已不是可读图像，都按竞态返回 `Ok(None)`，调用方静默丢弃不弹卡；硬
//! 失败返回 `Err`，由调用方上抛 `TaskFailed`。
//!
//! 设界逐级前置：flavor 字节上限 → 仅读头取尺寸过像素面积上限 → 才整图
//! 解码 → 产物过 PNG 字节上限。pastebomb 在任何大分配发生之前被整体拒绝。
//! 日志只记字节数与像素尺寸。

use std::io::Cursor;
use std::sync::Arc;

use gloss_core::log::{debug, thread};
use gloss_core::model::GlossError;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, ImageFormat, ImageReader};

use super::{ImageFlavor, PasteboardObserver, pasteboard_change_count, pasteboard_image_flavor};

/// flavor 字节上限：恰好 4096² 的未压缩 TIFF（RGB8）约 43MB，给满尺寸的
/// 合法图像留量；超限在头解析之前整体拒绝。
pub const MAX_FLAVOR_BYTES: usize = 64 * 1024 * 1024;

/// 像素面积上限：4096×4096 ≈ 16MP（4K 截图 8.3MP 在内）；面积在上限处的
/// RGBA 位图即 64MB，超出即视为 pastebomb。
pub const MAX_PIXELS: u64 = 4096 * 4096;

/// PNG 字节上限：取材产物进 `TaskInput`、缓存会话与出网请求体，超限整体
/// 报错。
pub const MAX_PNG_BYTES: usize = 20 * 1024 * 1024;

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

/// 图像 flavor 编码数据 → 解码 RGBA → PNG（快速压缩档：编码在事件线程上
/// 执行，时长直接挂在触发到弹卡的链路上）。
///
/// 设界依次执行：flavor 字节上限在头解析之前，像素面积上限以仅读头的
/// 尺寸判定、先于整图解码，PNG 字节上限判在编码产物上。超限整体报
/// `ImageTooLarge`（不截断）；头不可读、解码失败、缓冲与尺寸不符都按
/// 读取失败处理。
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
            GlossError::SelectionUnavailable
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
            GlossError::SelectionUnavailable
        })?;
    let rgba = decoded.to_rgba8();
    encode_png(rgba.width(), rgba.height(), rgba.as_raw())
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
        return Err(GlossError::SelectionUnavailable);
    }
    let mut png = Vec::new();
    if let Err(err) =
        PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::NoFilter)
            .write_image(rgba, width, height, ExtendedColorType::Rgba8)
    {
        debug!(thread = thread::EVENT, error = %err, "png encoding failed");
        return Err(GlossError::SelectionUnavailable);
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

    /// 读剪贴板图片为 PNG 字节：`Ok(None)` = ①②之间被覆盖（竞态，调用方
    /// 静默丢弃）；`Err` = 取材失败（调用方上抛 `TaskFailed`）。
    pub fn read(&mut self) -> Result<Option<Arc<[u8]>>, GlossError> {
        let Some(current) = pasteboard_change_count() else {
            debug!(
                thread = thread::EVENT,
                "pasteboard unavailable, image read declined"
            );
            return Err(GlossError::SelectionUnavailable);
        };
        if !self.observer.matches(current) {
            return Ok(None);
        }
        let Some((flavor, data)) = pasteboard_image_flavor() else {
            debug!(
                thread = thread::EVENT,
                "clipboard no longer holds a readable image flavor, dropped"
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
        assert_eq!(err, GlossError::SelectionUnavailable);
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
        assert_eq!(err, GlossError::SelectionUnavailable);
    }

    #[test]
    fn pixel_budget_is_checked_before_the_buffer() {
        let err = encode_png(4096, 4096, &[]).expect_err("buffer mismatch");
        assert_eq!(
            err,
            GlossError::SelectionUnavailable,
            "within-budget dims fall through to the buffer check"
        );
    }

    #[test]
    fn byte_budget_rejects_length_over_the_limit() {
        assert!(png_within_budget(MAX_PNG_BYTES), "the limit itself fits");
        assert!(
            !png_within_budget(MAX_PNG_BYTES + 1),
            "one byte over the limit is rejected"
        );
    }
}
