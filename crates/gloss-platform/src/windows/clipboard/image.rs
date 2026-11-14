//! 剪贴板图片取材读取器的平台绑定：把共享归一化形状（平台中立共享层）
//! 绑定到系统（Windows）原语，并承载位图家族独有的设备无关位图（DIB）
//! 头合成与格式映射。
//!
//! 共享形状（预算判定、降采样、便携式网络图形（PNG）编码与“竞态→交付
//! 不出→解码”三路）住平台中立共享层（common）；本模块提供序列号读数与
//! 交付原语——便携式网络图形（PNG）注册格式、版本 5 位图（CF_DIBV5）与
//! 位图（CF_DIB）按优先序取首格式的数据字节（字节上限在全局内存锁页与
//! 拷贝之前判定，超限按尺寸错误上抛）——并把交付风格映射为解码格式：
//! 位图家族（位图（CF_DIB）与版本 5 位图（CF_DIBV5））是无文件头的设备
//! 无关位图（DIB）流，按信息头布局算出像素数据偏移、合成位图（BMP）文
//! 件头后走图像库的位图解码路径（信息头尺寸覆盖基线 40 字节头到版本 5；
//! 核心 12 字节头不在支持面，报读取失败）。位图文件字节在读取路径只合成
//! 一次，尺寸探测与解码共用同一份（热路径不做整图重复拷贝）；超限判定
//! 在交付原语前置（锁页与拷贝之前），共享管线的交付字节预算是第二道闸。
//!
//! 日志红线：图像只记字节数与像素尺寸，内容与字节永不落日志、不落盘、
//! 不进错误消息。

pub use crate::common::image::SystemImageReader;
use crate::common::image::{DeliveredFlavor, MAX_FLAVOR_BYTES, header_unreadable};

use gloss_core::model::GlossError;
use gloss_core::ports::ClipboardImageReader;
use std::borrow::Cow;
use std::sync::Arc;

use super::{FlavorRead, ImageFlavor, clipboard_image_flavor, clipboard_sequence_number};

const BMP_FILE_HEADER_LEN: usize = 14;

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

fn map_clipboard_flavor(
    (flavor, bytes): &(ImageFlavor, Vec<u8>),
) -> Result<DeliveredFlavor<'_>, GlossError> {
    match flavor {
        ImageFlavor::Png => Ok(DeliveredFlavor {
            flavor_len: bytes.len(),
            format: image::ImageFormat::Png,
            payload: Cow::Borrowed(bytes),
        }),
        ImageFlavor::Dib | ImageFlavor::DibV5 => bmp_file_bytes(bytes)
            .map(|file| DeliveredFlavor {
                flavor_len: bytes.len(),
                format: image::ImageFormat::Bmp,
                payload: Cow::Owned(file),
            })
            .ok_or_else(|| header_unreadable("malformed dib header")),
    }
}

impl ClipboardImageReader for SystemImageReader {
    fn read(&mut self) -> Result<Option<Arc<[u8]>>, GlossError> {
        self.read_with(
            clipboard_sequence_number,
            || match clipboard_image_flavor(MAX_FLAVOR_BYTES) {
                FlavorRead::Data(flavor, bytes) => Ok(Some((flavor, bytes))),
                FlavorRead::OverLimit => Err(GlossError::ImageTooLarge),
                FlavorRead::Absent => Ok(None),
            },
            map_clipboard_flavor,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::common::image::decode_to_png;

    const RGBA_2X2: [u8; 16] = [
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 0,
    ];

    fn encoded_2x2(format: image::ImageFormat) -> Vec<u8> {
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
            (ImageFlavor::Png, encoded_2x2(image::ImageFormat::Png)),
        ] {
            let delivered = map_clipboard_flavor(&(flavor, bytes)).expect("mapping must succeed");
            let png = decode_to_png(&delivered).expect("decode must succeed");
            let decoded = image::load_from_memory(&png).expect("valid png");
            assert_eq!((decoded.width(), decoded.height()), (2, 2), "{flavor:?}");
        }
    }

    #[test]
    fn dib_decode_keeps_row_order_and_channels() {
        let delivered =
            map_clipboard_flavor(&(ImageFlavor::DibV5, dib_2x2())).expect("mapping must succeed");
        let png = decode_to_png(&delivered).expect("decode must succeed");
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
        let mut v4 = encoded_2x2(image::ImageFormat::Bmp);
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
    fn dib_a_malformed_flavor_maps_to_a_read_failure() {
        let dib = &dib_2x2()[..20];
        let err =
            map_clipboard_flavor(&(ImageFlavor::Dib, dib.to_vec())).expect_err("malformed dib");
        assert_eq!(err, GlossError::ImageUnavailable);
    }
}
