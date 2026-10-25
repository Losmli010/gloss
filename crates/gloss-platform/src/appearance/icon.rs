//! 应用图标适配器（macOS），实现图标端口。

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
            // SAFETY: the main thread is already proven by the `MainThreadMarker`
            // above (`sharedApplication` takes it as an argument); `image` is a
            // `Retained<NSImage>` that stays alive within this closure and is not
            // released during the call, and AppKit only reads it.
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
