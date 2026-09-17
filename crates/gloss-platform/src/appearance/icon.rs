//! macOS 应用图标适配器（[`AppIcon`] 端口的实现）。

use gloss_core::ports::AppIcon;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSImage};
use objc2_foundation::NSData;

/// macOS 应用图标：调 `NSApplication.setApplicationIconImage:` 装上品牌图标。
///
/// 只在**非 bundle 运行**时起作用：打包后的 .app 由 Info.plist 指向的
/// `assets/icons/Gloss.icns` 决定 Dock 与访达里的图标，而 `just run`
/// （cargo 直接跑可执行文件）没有 bundle 可读，Dock 里是系统给的默认图标——
/// 这个适配器补的正是这一段。
///
/// 与其余适配器不同，它不持有资源：PNG 字节由组装点（入口）注入，
/// 端口只负责「把给定的图交给平台外壳」。
#[derive(Debug, Default, Clone, Copy)]
pub struct MacAppIcon;

impl MacAppIcon {
    /// 构造适配器（无状态，可自由复制）。
    pub fn new() -> Self {
        Self
    }
}

impl AppIcon for MacAppIcon {
    fn install(&self, png: &[u8]) -> bool {
        // 主线程标记拿不到就放弃：NSApplication 只允许主线程访问，而图标
        // 是一次性动作，没有「稍后在正确线程补做」的机会，等了也没用。
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };

        // NSImage 直接吃 PNG 字节（系统自带解码），因此不需要引入图像解码库。
        let data = NSData::with_bytes(png);
        let Some(image) = NSImage::initWithData(mtm.alloc::<NSImage>(), &data) else {
            return false;
        };

        let app = NSApplication::sharedApplication(mtm);
        // SAFETY: 主线程已由上面的 MainThreadMarker 证明（sharedApplication
        // 也以它为入参）；image 是本函数内仍存活的 Retained<NSImage>，调用
        // 期间不会被释放，AppKit 只读它。
        unsafe { app.setApplicationIconImage(Some(&image)) };
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：字节不是合法图片时不 panic、如实回报失败（降级由调用方处理）。
    /// 真机上安装路径要主线程 + NSApplication，这里只锁「坏数据不炸」这条
    /// 早退分支；成功路径由 `just run` 人工走查（Dock 里能看到图标）。
    #[test]
    fn install_reports_failure_for_non_image_bytes() {
        let applied = MacAppIcon::new().install(b"not a png");
        assert!(!applied, "garbage bytes must not report success");
    }
}
