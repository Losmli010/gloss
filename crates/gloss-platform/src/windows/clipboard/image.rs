//! 剪贴板图片取材读取器：读板上图像格式的数据字节 → 解码 → 便携式网络
//! 图形（PNG）产物，快速压缩档。
//!
//! 两条静默丢弃路径分开留痕，都返回空读取、调用方不弹卡：
//! - “竞态”：1 观察与 2 取材之间剪贴板被覆盖，序列号前进——留调试痕，
//!   与划词空选区误滑同型处置；
//! - “交付不出”：序列号未变、板上也声明了图像格式，但读不出数据——
//!   板被其它应用短时占有或数据查询失败走这条，留警告痕供日志检视按级筛
//!   排查。
//!
//! 硬失败按错误上抛，由调用方弹失败卡。观察比对先于内容读取：比对不过
//! 不取数据字节。
//!
//! 设界逐级前置：字节上限在全局内存锁页与拷贝之前（外部函数接口（FFI）
//! 读取的分配尺寸判定）→ 仅读头取尺寸过像素面积上限 → 才整图解码 → 超长
//! 边产物按比例降采样 → 产物过字节上限。恶意粘贴内容在任何大分配发生之前
//! 被整体拒绝。日志只记字节数与像素尺寸。
//!
//! 解码分工：便携式网络图形（PNG）格式按图像格式直解；位图家族（位图
//! （CF_DIB）与版本 5 位图（CF_DIBV5））是无文件头的设备无关位图（DIB）
//! 流——按信息头布局算出像素数据偏移，合成位图（BMP）文件头后走图像库的
//! 位图解码路径（信息头尺寸覆盖基线 40 字节头到版本 5；核心 12 字节头不
//! 在支持面，报读取失败）。

use std::io::Cursor;
use std::sync::Arc;

use gloss_core::log::{debug, thread, warn};
use gloss_core::model::GlossError;
use gloss_core::ports::ClipboardImageReader;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, ImageFormat, ImageReader};

use super::{
    FlavorRead, ImageFlavor, PasteboardObserver, clipboard_image_flavor, clipboard_sequence_number,
};

pub const MAX_FLAVOR_BYTES: usize = 64 * 1024 * 1024;

pub const MAX_PIXELS: u64 = 4096 * 4096;

pub const MAX_PNG_BYTES: usize = 20 * 1024 * 1024;

pub const MAX_PRODUCT_SIDE: u32 = 1536;

const BMP_FILE_HEADER_LEN: usize = 14;

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

fn decode_to_png(flavor: ImageFlavor, bytes: &[u8]) -> Result<Arc<[u8]>, GlossError> {
    if !flavor_within_budget(bytes.len()) {
        debug!(
            thread = thread::EVENT,
            bytes = bytes.len(),
            "clipboard image exceeds the flavor budget"
        );
        return Err(GlossError::ImageTooLarge);
    }
    let (width, height) = header_dimensions(flavor, bytes)?;
    if !pixels_within_budget(width, height) {
        debug!(
            thread = thread::EVENT,
            width, height, "clipboard image exceeds the pixel budget"
        );
        return Err(GlossError::ImageTooLarge);
    }
    let rgba = decode_rgba(flavor, bytes)?;
    let (width, height, rgba) = downscale_to_budget(rgba);
    encode_png(width, height, rgba.as_raw())
}

fn header_unreadable(err: impl std::fmt::Display) -> GlossError {
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

fn header_dimensions(flavor: ImageFlavor, bytes: &[u8]) -> Result<(u32, u32), GlossError> {
    match flavor {
        ImageFlavor::Png => ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png)
            .into_dimensions()
            .map_err(header_unreadable),
        ImageFlavor::Dib | ImageFlavor::DibV5 => dib_reader(bytes)?
            .into_dimensions()
            .map_err(header_unreadable),
    }
}

fn decode_rgba(flavor: ImageFlavor, bytes: &[u8]) -> Result<image::RgbaImage, GlossError> {
    match flavor {
        ImageFlavor::Png => ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png)
            .decode()
            .map(|decoded| decoded.into_rgba8())
            .map_err(decode_failed),
        ImageFlavor::Dib | ImageFlavor::DibV5 => dib_reader(bytes)?
            .decode()
            .map(|decoded| decoded.into_rgba8())
            .map_err(decode_failed),
    }
}

fn dib_reader(dib: &[u8]) -> Result<ImageReader<Cursor<Vec<u8>>>, GlossError> {
    let file = bmp_file_bytes(dib).ok_or_else(|| header_unreadable("malformed dib header"))?;
    Ok(ImageReader::with_format(
        Cursor::new(file),
        ImageFormat::Bmp,
    ))
}

fn dib_pixel_offset(dib: &[u8]) -> Option<u32> {
    if dib.len() < 40 {
        return None;
    }
    let header_size = u32::from_le_bytes(dib[0..4].try_into().ok()?);
    if !(40..=124).contains(&header_size) || dib.len() < header_size as usize {
        return None;
    }
    let bit_count = u16::from_le_bytes(dib[14..16].try_into().ok()?);
    let compression = u32::from_le_bytes(dib[16..20].try_into().ok()?);
    let colors_used = u32::from_le_bytes(dib[32..36].try_into().ok()?);
    let mut offset = header_size;
    if bit_count <= 8 {
        let entries = if colors_used == 0 {
            1u32 << bit_count
        } else {
            colors_used
        };
        offset = offset.checked_add(4u32.checked_mul(entries)?)?;
    } else if compression == 3 && header_size == 40 {
        offset = offset.checked_add(12)?;
    }
    Some(offset)
}

fn bmp_file_bytes(dib: &[u8]) -> Option<Vec<u8>> {
    let offset = dib_pixel_offset(dib)?;
    let file_size = u32::try_from(BMP_FILE_HEADER_LEN + dib.len()).ok()?;
    let mut file = Vec::with_capacity(BMP_FILE_HEADER_LEN + dib.len());
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&file_size.to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(
        &u32::try_from(BMP_FILE_HEADER_LEN)
            .ok()?
            .checked_add(offset)?
            .to_le_bytes(),
    );
    file.extend_from_slice(dib);
    Some(file)
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

    fn read_with(
        &mut self,
        count: impl FnOnce() -> Option<u64>,
        flavor: impl FnOnce() -> Result<Option<(ImageFlavor, Vec<u8>)>, GlossError>,
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
        match flavor()? {
            Some((flavor, bytes)) => decode_to_png(flavor, &bytes).map(Some),
            None => {
                warn!(
                    thread = thread::EVENT,
                    "the clipboard declares an image but delivers none, image dropped"
                );
                Ok(None)
            }
        }
    }
}

impl ClipboardImageReader for SystemImageReader {
    fn read(&mut self) -> Result<Option<Arc<[u8]>>, GlossError> {
        self.read_with(clipboard_sequence_number, || {
            match clipboard_image_flavor(MAX_FLAVOR_BYTES) {
                FlavorRead::Data(flavor, bytes) => Ok(Some((flavor, bytes))),
                FlavorRead::OverLimit => Err(GlossError::ImageTooLarge),
                FlavorRead::Absent => Ok(None),
            }
        })
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

    fn dib_2x2() -> Vec<u8> {
        let mut dib = vec![0u8; 124 + 16];
        dib[0..4].copy_from_slice(&124u32.to_le_bytes());
        dib[4..8].copy_from_slice(&2u32.to_le_bytes());
        dib[8..12].copy_from_slice(&2i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&32u16.to_le_bytes());
        dib[16..20].copy_from_slice(&3u32.to_le_bytes());
        dib[20..24].copy_from_slice(&16u32.to_le_bytes());
        dib[40..44].copy_from_slice(&0x00ff_0000u32.to_le_bytes());
        dib[44..48].copy_from_slice(&0x0000_ff00u32.to_le_bytes());
        dib[48..52].copy_from_slice(&0x0000_00ffu32.to_le_bytes());
        dib[52..56].copy_from_slice(&0xff00_0000u32.to_le_bytes());
        dib[124..].copy_from_slice(&[
            0, 255, 0, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ]);
        dib
    }

    #[test]
    fn dib_dibv5_and_png_flavors_decode_to_png() {
        for (flavor, bytes) in [
            (ImageFlavor::Dib, dib_2x2()),
            (ImageFlavor::DibV5, dib_2x2()),
            (ImageFlavor::Png, encoded_2x2(ImageFormat::Png)),
        ] {
            let png = decode_to_png(flavor, &bytes).expect("decode must succeed");
            let decoded = image::load_from_memory(&png).expect("valid png");
            assert_eq!((decoded.width(), decoded.height()), (2, 2), "{flavor:?}");
        }
    }

    #[test]
    fn dib_decode_keeps_row_order_and_channels() {
        let png = decode_to_png(ImageFlavor::DibV5, &dib_2x2()).expect("decode must succeed");
        let decoded = image::load_from_memory(&png)
            .expect("valid png")
            .into_rgba8();
        assert_eq!(
            *decoded.get_pixel(0, 0),
            image::Rgba([0, 0, 255, 255]),
            "top row"
        );
        assert_eq!(
            *decoded.get_pixel(0, 1),
            image::Rgba([0, 255, 0, 255]),
            "bottom row"
        );
    }

    #[test]
    fn dib_pixel_offset_covers_header_variants() {
        assert_eq!(
            dib_pixel_offset(&dib_2x2()),
            Some(124),
            "v5 masks in header"
        );
        let mut v4 = encoded_2x2(ImageFormat::Bmp);
        assert_eq!(&v4[..2], b"BM", "the bmp file header must be present");
        v4.drain(..14);
        assert_eq!(
            dib_pixel_offset(&v4),
            Some(108),
            "v4 masks live inside the header too"
        );
        let mut info = vec![0u8; 40 + 16];
        info[0..4].copy_from_slice(&40u32.to_le_bytes());
        info[4..8].copy_from_slice(&2u32.to_le_bytes());
        info[8..12].copy_from_slice(&2i32.to_le_bytes());
        info[12..14].copy_from_slice(&1u16.to_le_bytes());
        info[14..16].copy_from_slice(&24u16.to_le_bytes());
        assert_eq!(dib_pixel_offset(&info), Some(40), "no palette, no masks");

        let mut bitfields = info.clone();
        bitfields[14..16].copy_from_slice(&16u16.to_le_bytes());
        bitfields[16..20].copy_from_slice(&3u32.to_le_bytes());
        assert_eq!(
            dib_pixel_offset(&bitfields),
            Some(52),
            "a baseline header with bitfields carries the mask block"
        );

        let mut palette = vec![0u8; 40];
        palette[0..4].copy_from_slice(&40u32.to_le_bytes());
        palette[14..16].copy_from_slice(&8u16.to_le_bytes());
        assert_eq!(
            dib_pixel_offset(&palette),
            Some(40 + 4 * 256),
            "a zero color count takes the full bit-depth palette"
        );
        palette[32..36].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(
            dib_pixel_offset(&palette),
            Some(40 + 4 * 2),
            "a declared color count wins over the bit-depth default"
        );
    }

    #[test]
    fn dib_pixel_offset_rejects_malformed_headers() {
        let mut short = dib_2x2();
        short.truncate(20);
        assert_eq!(
            dib_pixel_offset(&short),
            None,
            "a truncated header is illegal"
        );
        let mut core = vec![0u8; 40];
        core[0..4].copy_from_slice(&12u32.to_le_bytes());
        assert_eq!(
            dib_pixel_offset(&core),
            None,
            "the core header is unsupported"
        );
        let mut huge_palette = vec![0u8; 40];
        huge_palette[0..4].copy_from_slice(&40u32.to_le_bytes());
        huge_palette[14..16].copy_from_slice(&8u16.to_le_bytes());
        huge_palette[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            dib_pixel_offset(&huge_palette),
            None,
            "a palette that overflows the offset arithmetic is illegal"
        );
    }

    #[test]
    fn dib_flavor_budget_rejects_bytes_over_the_limit() {
        assert!(
            flavor_within_budget(MAX_FLAVOR_BYTES),
            "the limit itself fits"
        );
        assert!(
            !flavor_within_budget(MAX_FLAVOR_BYTES + 1),
            "one byte over the limit is rejected"
        );
        let over = vec![0u8; MAX_FLAVOR_BYTES + 1];
        let err = decode_to_png(ImageFlavor::Dib, &over).expect_err("over-budget flavor");
        assert_eq!(
            err,
            GlossError::ImageTooLarge,
            "the flavor cap fires before any header parsing"
        );
    }

    #[test]
    fn dib_header_dimensions_are_bounded_before_the_decode() {
        let mut dib = dib_2x2();
        dib[4..8].copy_from_slice(&4097u32.to_le_bytes());
        dib[8..12].copy_from_slice(&4096i32.to_le_bytes());
        let err = decode_to_png(ImageFlavor::Dib, &dib).expect_err("over-budget image");
        assert_eq!(
            err,
            GlossError::ImageTooLarge,
            "the header gate fires before the full decode"
        );
    }

    #[test]
    fn dib_a_truncated_flavor_decodes_to_a_read_failure() {
        let bytes = encoded_2x2(ImageFormat::Png);
        let truncated = &bytes[..40];
        let err = decode_to_png(ImageFlavor::Png, truncated).expect_err("truncated flavor");
        assert_eq!(err, GlossError::ImageUnavailable);
        let dib = &dib_2x2()[..20];
        let err = decode_to_png(ImageFlavor::Dib, dib).expect_err("truncated dib");
        assert_eq!(err, GlossError::ImageUnavailable);
    }

    #[test]
    fn dib_pixel_budget_rejects_area_over_the_limit() {
        assert!(pixels_within_budget(4096, 4096), "the limit itself fits");
        assert!(
            !pixels_within_budget(4097, 4096),
            "one pixel over the limit is rejected"
        );
    }

    #[test]
    fn dib_a_mismatched_rgba_buffer_is_a_read_failure() {
        let err = encode_png(2, 2, &[1, 2, 3]).expect_err("buffer mismatch");
        assert_eq!(err, GlossError::ImageUnavailable);
    }

    #[test]
    fn dib_pixel_budget_is_checked_before_the_buffer() {
        let err = encode_png(4096, 4096, &[]).expect_err("buffer mismatch");
        assert_eq!(
            err,
            GlossError::ImageUnavailable,
            "within-budget dims fall through to the buffer check"
        );
    }

    #[test]
    fn dib_a_covered_board_is_dropped_without_touching_the_content() {
        let observer = PasteboardObserver::default();
        observer.record(7);
        let mut reader = SystemImageReader::new(observer);
        let read = reader.read_with(
            || Some(8),
            || panic!("content must not be read when the observation no longer matches"),
        );
        assert_eq!(read.expect("a race drops, not fails"), None);
    }

    #[test]
    fn dib_an_undeliverable_flavor_is_dropped_quietly() {
        let observer = PasteboardObserver::default();
        observer.record(7);
        let mut reader = SystemImageReader::new(observer);
        let read = reader.read_with(|| Some(7), || Ok(None));
        assert_eq!(read.expect("undelivered drops, not fails"), None);
    }

    #[test]
    fn dib_an_over_limit_flavor_fails_with_the_size_error() {
        let observer = PasteboardObserver::default();
        observer.record(7);
        let mut reader = SystemImageReader::new(observer);
        let read = reader.read_with(|| Some(7), || Err(GlossError::ImageTooLarge));
        assert_eq!(
            read.expect_err("an over-limit flavor is a hard failure"),
            GlossError::ImageTooLarge
        );
    }

    #[test]
    fn dib_an_unobservable_board_is_a_read_failure() {
        let mut reader = SystemImageReader::new(PasteboardObserver::default());
        let err = reader
            .read_with(
                || None,
                || panic!("no flavor query without a readable count"),
            )
            .expect_err("unobservable board");
        assert_eq!(err, GlossError::ImageUnavailable);
    }

    #[test]
    fn dib_a_matching_board_still_decodes_to_png() {
        let observer = PasteboardObserver::default();
        observer.record(7);
        let mut reader = SystemImageReader::new(observer);
        let dib = dib_2x2();
        let png = reader
            .read_with(|| Some(7), || Ok(Some((ImageFlavor::Dib, dib))))
            .expect("read must succeed")
            .expect("the observed image must decode");
        let decoded = image::load_from_memory(&png).expect("valid png");
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
    }

    #[test]
    fn dib_byte_budget_rejects_length_over_the_limit() {
        assert!(png_within_budget(MAX_PNG_BYTES), "the limit itself fits");
        assert!(
            !png_within_budget(MAX_PNG_BYTES + 1),
            "one byte over the limit is rejected"
        );
    }

    #[test]
    fn dib_downscaled_dims_preserve_aspect_and_the_cap() {
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
    fn dib_oversized_products_are_downscaled_to_the_side_cap() {
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
