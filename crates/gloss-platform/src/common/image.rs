//! 剪贴板图片归一化的平台中立共享形状：预算判定、降采样、便携式网络图形
//! （PNG）编码与一轮读取的“竞态→交付不出→解码”三路只写一份，两侧平台
//! 目录共用；平台侧只留格式映射与取数原语（计数读数、交付与映射闭包），
//! 系统独有的数据形态变换（如系统（Windows）的位图头合成）也留在平台侧。
//! 交付形状（交付风格）携带预算口径的字节长度、解码格式与载荷借用视图：
//! 载荷经借用直通解码，共享层不复制图像字节。本模块零目标条件编译、零
//! 外部函数接口（FFI）、零不安全块。
//!
//! 两条静默丢弃路径分开留痕，都返回空读取、调用方不弹卡：
//! - “竞态”：1 观察与 2 取材之间剪贴板被覆盖，计数读数前进——留调试痕，
//!   与划词空选区误滑同型处置；
//! - “交付不出”：计数未变、板上也声明了图像，但读不出数据——留警告痕供
//!   日志检视按级筛排查（各侧成因见两侧平台绑定模块头）。
//!
//! 硬失败按错误上抛，由调用方弹失败卡。观察比对先于内容读取：比对不过
//! 不调交付闭包。设界逐级前置：交付字节上限 → 仅读头取尺寸过像素面积
//! 上限 → 才整图解码 → 超长边产物按比例降采样 → 产物过字节上限。恶意
//! 粘贴内容在任何大分配发生之前被整体拒绝。日志只记字节数与像素尺寸。
//! 取材字节预算随本模块单源：选中文本的字节上限也在此登记（两侧选区
//! 读取器共用）。

use std::borrow::Cow;
use std::io::Cursor;
use std::sync::Arc;

use gloss_core::log::{debug, thread, warn};
use gloss_core::model::GlossError;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, ImageFormat, ImageReader};

use super::clipboard_watch::PasteboardObserver;

pub const MAX_FLAVOR_BYTES: usize = 64 * 1024 * 1024;

pub const MAX_PIXELS: u64 = 4096 * 4096;

pub const MAX_PNG_BYTES: usize = 20 * 1024 * 1024;

pub const MAX_PRODUCT_SIDE: u32 = 1536;

pub const MAX_SELECTION_BYTES: usize = 256 * 1024;

fn flavor_within_budget(len: usize) -> bool {
    len <= MAX_FLAVOR_BYTES
}

fn pixels_within_budget(width: u32, height: u32) -> bool {
    u64::from(width) * u64::from(height) <= MAX_PIXELS
}

fn png_within_budget(len: usize) -> bool {
    len <= MAX_PNG_BYTES
}

fn downscaled_dims(width: u32, height: u32) -> Option<(u32, u32)> {
    let longest = width.max(height);
    if longest <= MAX_PRODUCT_SIDE {
        return None;
    }
    let scale = f64::from(MAX_PRODUCT_SIDE) / f64::from(longest);
    let scaled = |side: u32| ((f64::from(side) * scale).round() as u32).max(1);
    Some((scaled(width), scaled(height)))
}

pub(crate) struct DeliveredFlavor<'a> {
    pub(crate) flavor_len: usize,
    pub(crate) format: ImageFormat,
    pub(crate) payload: Cow<'a, [u8]>,
}

pub(crate) fn header_unreadable(err: impl std::fmt::Display) -> GlossError {
    debug!(
        thread = thread::EVENT,
        error = %err,
        "clipboard image header unreadable"
    );
    GlossError::ImageUnavailable
}

fn decode_failed(err: impl std::fmt::Display) -> GlossError {
    debug!(
        thread = thread::EVENT,
        error = %err,
        "clipboard image decode failed"
    );
    GlossError::ImageUnavailable
}

fn header_dimensions(bytes: &[u8], format: ImageFormat) -> Result<(u32, u32), GlossError> {
    ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(header_unreadable)
}

fn decode_rgba(bytes: &[u8], format: ImageFormat) -> Result<image::RgbaImage, GlossError> {
    ImageReader::with_format(Cursor::new(bytes), format)
        .decode()
        .map(|decoded| decoded.into_rgba8())
        .map_err(decode_failed)
}

pub(crate) fn decode_to_png(delivered: &DeliveredFlavor<'_>) -> Result<Arc<[u8]>, GlossError> {
    if !flavor_within_budget(delivered.flavor_len) {
        debug!(
            thread = thread::EVENT,
            bytes = delivered.flavor_len,
            "clipboard image exceeds the flavor budget"
        );
        return Err(GlossError::ImageTooLarge);
    }
    let payload = delivered.payload.as_ref();
    let (width, height) = header_dimensions(payload, delivered.format)?;
    if !pixels_within_budget(width, height) {
        debug!(
            thread = thread::EVENT,
            width, height, "clipboard image exceeds the pixel budget"
        );
        return Err(GlossError::ImageTooLarge);
    }
    let rgba = decode_rgba(payload, delivered.format)?;
    let (width, height, rgba) = downscale_to_budget(rgba);
    encode_png(width, height, rgba.as_raw())
}

fn downscale_to_budget(rgba: image::RgbaImage) -> (u32, u32, image::RgbaImage) {
    match downscaled_dims(rgba.width(), rgba.height()) {
        Some((width, height)) => {
            debug!(
                thread = thread::EVENT,
                source_width = rgba.width(),
                source_height = rgba.height(),
                width,
                height,
                "clipboard image downscaled for the wire"
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
    }
}

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
            "clipboard image exceeds the byte budget"
        );
        return Err(GlossError::ImageTooLarge);
    }
    Ok(Arc::from(png))
}

pub struct SystemImageReader {
    observer: PasteboardObserver,
}

impl SystemImageReader {
    pub fn new(observer: PasteboardObserver) -> Self {
        Self { observer }
    }

    pub(crate) fn read_with<T>(
        &mut self,
        count: impl FnOnce() -> Option<u64>,
        fetch: impl FnOnce() -> Result<Option<T>, GlossError>,
        map: impl FnOnce(&T) -> Result<DeliveredFlavor<'_>, GlossError>,
    ) -> Result<Option<Arc<[u8]>>, GlossError> {
        let Some(current) = count() else {
            debug!(
                thread = thread::EVENT,
                "clipboard unavailable, image read declined"
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
        let Some(fetched) = fetch()? else {
            warn!(
                thread = thread::EVENT,
                "the clipboard declares an image but delivers none, image dropped"
            );
            return Ok(None);
        };
        let delivered = map(&fetched)?;
        decode_to_png(&delivered).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    const RGBA_2X2: [u8; 16] = [
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 0,
    ];

    fn encoded_2x2() -> Vec<u8> {
        let canvas = image::RgbaImage::from_raw(2, 2, RGBA_2X2.to_vec()).expect("canvas");
        let mut bytes = Cursor::new(Vec::new());
        canvas
            .write_to(&mut bytes, ImageFormat::Png)
            .expect("encode");
        bytes.into_inner()
    }

    fn png_flavor(bytes: &[u8]) -> DeliveredFlavor<'_> {
        DeliveredFlavor {
            flavor_len: bytes.len(),
            format: ImageFormat::Png,
            payload: Cow::Borrowed(bytes),
        }
    }

    fn patch_png_dimensions(bytes: &mut [u8], width: u32, height: u32) {
        assert_eq!(&bytes[12..16], b"IHDR", "the header chunk must be IHDR");
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        let crc = crc32fast::hash(&bytes[12..29]);
        bytes[29..33].copy_from_slice(&crc.to_be_bytes());
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
        let err = decode_to_png(&png_flavor(&over)).expect_err("over-budget flavor");
        assert_eq!(
            err,
            GlossError::ImageTooLarge,
            "the flavor cap fires before any header parsing"
        );
    }

    #[test]
    fn header_dimensions_are_bounded_before_the_decode() {
        let mut bytes = encoded_2x2();
        patch_png_dimensions(&mut bytes, 4097, 4096);
        let err = decode_to_png(&png_flavor(&bytes)).expect_err("over-budget image");
        assert_eq!(
            err,
            GlossError::ImageTooLarge,
            "the header gate fires before the full decode"
        );
    }

    #[test]
    fn a_truncated_flavor_decodes_to_a_read_failure() {
        let bytes = encoded_2x2();
        let truncated = &bytes[..40];
        let err = decode_to_png(&png_flavor(truncated)).expect_err("truncated flavor");
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
        let mut reader = SystemImageReader::new(observer);
        let read = reader.read_with(
            || Some(8),
            || panic!("content must not be fetched when the observation no longer matches"),
            |_: &()| panic!("no mapping without a delivery"),
        );
        assert_eq!(read.expect("a race drops, not fails"), None);
    }

    #[test]
    fn an_undeliverable_flavor_is_dropped_quietly() {
        let observer = PasteboardObserver::default();
        observer.record(7);
        let mut reader = SystemImageReader::new(observer);
        let read = reader.read_with(
            || Some(7),
            || Ok(None),
            |_: &()| panic!("no mapping without a delivery"),
        );
        assert_eq!(read.expect("undelivered drops, not fails"), None);
    }

    #[test]
    fn an_over_limit_flavor_fails_with_the_size_error() {
        let observer = PasteboardObserver::default();
        observer.record(7);
        let mut reader = SystemImageReader::new(observer);
        let read = reader.read_with(
            || Some(7),
            || Err(GlossError::ImageTooLarge),
            |_: &()| panic!("no mapping without a delivery"),
        );
        assert_eq!(
            read.expect_err("an over-limit flavor is a hard failure"),
            GlossError::ImageTooLarge
        );
    }

    #[test]
    fn an_unobservable_board_is_a_read_failure() {
        let mut reader = SystemImageReader::new(PasteboardObserver::default());
        let err = reader
            .read_with(
                || None,
                || panic!("no fetch without a readable count"),
                |_: &()| panic!("no mapping without a delivery"),
            )
            .expect_err("unobservable board");
        assert_eq!(err, GlossError::ImageUnavailable);
    }

    #[test]
    fn a_matching_board_still_decodes_to_png() {
        let observer = PasteboardObserver::default();
        observer.record(7);
        let bytes = encoded_2x2();
        let mut reader = SystemImageReader::new(observer);
        let png = reader
            .read_with(
                || Some(7),
                || Ok(Some(bytes)),
                |delivered: &Vec<u8>| Ok(png_flavor(delivered)),
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
        let png = decode_to_png(&png_flavor(bytes.into_inner().as_slice()))
            .expect("an oversized product is downscaled, not rejected");
        let decoded = image::load_from_memory(&png).expect("valid png");
        assert_eq!(
            (decoded.width(), decoded.height()),
            (1536, 731),
            "the product lands within the side cap with the aspect preserved"
        );
    }
}
