//! 剪贴板图片取材读取器：arboard 读 RGBA → PNG（快速压缩档）。
//!
//! 竞态兜底：① 观察与 ② 取材之间剪贴板可能被覆盖——changeCount 再变，或
//! 内容已不是图片（`ContentNotAvailable`），都按竞态返回 `Ok(None)`，调用
//! 方静默丢弃不弹卡；硬失败返回 `Err`，由调用方上抛 `TaskFailed`。
//!
//! 边界与上限：像素面积与 PNG 字节双上限（超限整体报 `ImageTooLarge`，不
//! 截断），RGBA 缓冲与声明尺寸不符按读取失败处理——图片不完整的产物不发
//! 往下游。日志只记字节数与像素尺寸。

use std::sync::Arc;

use arboard::Clipboard;
use gloss_core::log::{debug, thread};
use gloss_core::model::GlossError;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};

use super::{PasteboardObserver, pasteboard_change_count};

/// 像素面积上限：4096×4096 ≈ 16MP（4K 截图 8.3MP 在内）；面积在上限处的
/// RGBA 位图即 64MB，超出即视为 pastebomb。
pub const MAX_PIXELS: u64 = 4096 * 4096;

/// PNG 字节上限：取材产物进 `TaskInput`、缓存会话与出网请求体，超限整体
/// 报错。
pub const MAX_PNG_BYTES: usize = 20 * 1024 * 1024;

/// 像素面积是否在预算内（纯逻辑，单测覆盖边界）。
fn pixels_within_budget(width: u32, height: u32) -> bool {
    u64::from(width) * u64::from(height) <= MAX_PIXELS
}

/// PNG 字节是否在预算内（纯逻辑，单测覆盖边界）。
fn png_within_budget(len: usize) -> bool {
    len <= MAX_PNG_BYTES
}

/// RGBA 位图 → PNG（快速压缩档：编码在事件线程上执行，时长直接挂在触发
/// 到弹卡的链路上）。像素面积超限报 `ImageTooLarge`；RGBA 缓冲与尺寸不符、
/// 编码失败都按读取失败处理。
fn encode_png(width: usize, height: usize, rgba: &[u8]) -> Result<Arc<[u8]>, GlossError> {
    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
        debug!(thread = thread::EVENT, "pasteboard image size out of range");
        return Err(GlossError::SelectionUnavailable);
    };
    if !pixels_within_budget(width, height) {
        debug!(
            thread = thread::EVENT,
            width, height, "pasteboard image exceeds the pixel budget"
        );
        return Err(GlossError::ImageTooLarge);
    }
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
        let mut clipboard = match Clipboard::new() {
            Ok(clipboard) => clipboard,
            Err(err) => {
                debug!(
                    thread = thread::EVENT,
                    error = %err,
                    "clipboard unavailable, image read declined"
                );
                return Err(GlossError::SelectionUnavailable);
            }
        };
        match clipboard.get_image() {
            Ok(image) => encode_png(image.width, image.height, &image.bytes).map(Some),
            Err(arboard::Error::ContentNotAvailable) => {
                debug!(
                    thread = thread::EVENT,
                    "clipboard no longer holds an image, dropped"
                );
                Ok(None)
            }
            Err(err) => {
                debug!(
                    thread = thread::EVENT,
                    error = %err,
                    "clipboard image read failed"
                );
                Err(GlossError::SelectionUnavailable)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_rgba_encodes_to_a_decodable_png() {
        let rgba = [
            255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 0,
        ];
        let png = encode_png(2, 2, &rgba).expect("encode must succeed");
        let decoded = image::load_from_memory(&png).expect("valid png");
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
    }

    #[test]
    fn pixel_budget_rejects_area_over_the_limit() {
        assert!(pixels_within_budget(4096, 4096), "the limit itself fits");
        assert!(
            !pixels_within_budget(4097, 4096),
            "one pixel over the limit is rejected"
        );
        let err = encode_png(4097, 4096, &[]).expect_err("over-budget image");
        assert_eq!(err, GlossError::ImageTooLarge);
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
    fn a_mismatched_rgba_buffer_is_a_read_failure() {
        let err = encode_png(2, 2, &[1, 2, 3]).expect_err("buffer mismatch");
        assert_eq!(err, GlossError::SelectionUnavailable);
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
