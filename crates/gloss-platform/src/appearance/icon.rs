//! macOS 应用图标适配器（图标端口的实现）。

use gloss_core::ports::AppIcon;
use objc2::MainThreadMarker;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSApplication, NSImage};
use objc2_foundation::NSData;

#[derive(Debug, Default, Clone, Copy)]
pub struct MacAppIcon;

impl MacAppIcon {
    pub fn new() -> Self {
        Self
    }
}

impl AppIcon for MacAppIcon {
    fn install(&self, png: &[u8]) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };

        autoreleasepool(|_| {
            let data = NSData::with_bytes(png);
            let Some(image) = NSImage::initWithData(mtm.alloc::<NSImage>(), &data) else {
                return false;
            };

            let app = NSApplication::sharedApplication(mtm);
            // SAFETY: 主线程已由上面的 MainThreadMarker 证明（sharedApplication
            // 也以它为入参）；image 是本闭包内仍存活的 Retained<NSImage>，调用
            // 期间不会被释放，AppKit 只读它。
            unsafe { app.setApplicationIconImage(Some(&image)) };
            true
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_degrades_to_false_off_the_main_thread() {
        let applied = std::thread::spawn(|| MacAppIcon::new().install(b"not a png"))
            .join()
            .expect("test thread must not panic");
        assert!(!applied, "off-main-thread call must report failure");
    }
}
