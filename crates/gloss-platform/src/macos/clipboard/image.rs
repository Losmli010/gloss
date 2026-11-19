//! 剪贴板图片取材读取器的平台绑定：把共享归一化形状（平台中立共享层）
//! 绑定到系统（macOS）原语。
//!
//! 共享形状（预算判定、降采样、便携式网络图形（PNG）编码与“竞态→交付
//! 不出→解码”三路）住平台中立共享层（common）；本模块只提供计数读数、
//! 交付与映射两个原语——直读系统粘贴板的图像数据风格（标记图像文件格式
//! （TIFF）先、便携式网络图形（PNG）后），按类型取数据并映射为解码格式，
//! 数据经零拷贝借用视图直通解码。1 观察与 2 取材之间剪贴板可能被覆盖：
//! 读取器比对现值——不一致即竞态，共享层静默丢弃；“交付不出”（板上声明
//! 了图像却读不出数据）在系统（macOS）的成因是 15 及以上版本“允许粘贴”
//! 授权被拒走内容读取返回空——共享层留警告痕，本头留成因。
//!
//! 日志红线：图像只记字节数与像素尺寸，内容与字节永不落日志、不落盘、
//! 不进错误消息。

use crate::common::image::DeliveredFlavor;
pub use crate::common::image::SystemImageReader;

use gloss_core::model::GlossError;
use gloss_core::ports::ClipboardImageReader;
use objc2::rc::Retained;
use objc2_foundation::NSData;
use std::borrow::Cow;
use std::sync::Arc;

use super::{ImageFlavor, pasteboard_change_count, pasteboard_image_flavor};

fn map_pasteboard_flavor(
    (flavor, data): &(ImageFlavor, Retained<NSData>),
) -> Result<DeliveredFlavor<'_>, GlossError> {
    let format = match flavor {
        ImageFlavor::Tiff => image::ImageFormat::Tiff,
        ImageFlavor::Png => image::ImageFormat::Png,
    };
    // SAFETY: `data` (a `Retained<NSData>`) stays alive and is not modified
    // within this scope — the delivered borrow is tied to the fetched flavor
    // and consumed by the shared decode within the same read call, so the
    // slice never outlives `data`.
    let bytes = unsafe { data.as_bytes_unchecked() };
    Ok(DeliveredFlavor {
        flavor_len: bytes.len(),
        format,
        payload: Cow::Borrowed(bytes),
    })
}

impl ClipboardImageReader for SystemImageReader {
    fn read(&mut self) -> Result<Option<Arc<[u8]>>, GlossError> {
        self.read_with(
            pasteboard_change_count,
            || Ok(pasteboard_image_flavor()),
            map_pasteboard_flavor,
        )
    }
}

#[cfg(test)]
mod tests {
    use objc2_foundation::NSData;

    use super::*;
    use crate::common::image::decode_to_png;

    const RGBA_2X2: [u8; 16] = [
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 0,
    ];

    fn encoded_2x2(format: image::ImageFormat) -> Vec<u8> {
        let canvas = image::RgbaImage::from_raw(2, 2, RGBA_2X2.to_vec()).expect("canvas");
        let mut bytes = std::io::Cursor::new(Vec::new());
        canvas.write_to(&mut bytes, format).expect("encode");
        bytes.into_inner()
    }

    #[test]
    fn png_and_tiff_flavors_decode_to_png() {
        for (flavor, bytes) in [
            (ImageFlavor::Png, encoded_2x2(image::ImageFormat::Png)),
            (ImageFlavor::Tiff, encoded_2x2(image::ImageFormat::Tiff)),
        ] {
            let data = NSData::with_bytes(&bytes);
            let fetched = (flavor, data);
            let delivered = map_pasteboard_flavor(&fetched).expect("mapping must succeed");
            let png = decode_to_png(&delivered).expect("decode must succeed");
            let decoded = image::load_from_memory(&png).expect("valid png");
            assert_eq!((decoded.width(), decoded.height()), (2, 2), "{flavor:?}");
        }
    }
}
