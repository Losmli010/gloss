//! 剪贴板图片取材读取器（系统（Windows）占位）：剪贴板图片取材端口的桩。
//!
//! 系统实现按剪贴板数据风格解码图像并重编码为位图（PNG）；系统
//! （Windows）的对应读取（位图（CF_DIB／CF_DIBV5）与便携式网络图形
//! （PNG））按移植方案后续阶段补齐，本桩一律报告未支持。

use std::sync::Arc;

use gloss_core::model::GlossError;
use gloss_core::ports::ClipboardImageReader;

use super::watch::PasteboardObserver;

pub struct SystemImageReader;

impl SystemImageReader {
    pub fn new(_observer: PasteboardObserver) -> Self {
        Self
    }
}

impl ClipboardImageReader for SystemImageReader {
    fn read(&mut self) -> Result<Option<Arc<[u8]>>, GlossError> {
        Err(GlossError::PlatformUnsupported)
    }
}
