//! 应用图标适配器（macOS），实现图标端口。

use gloss_core::model::GlossError;
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
    fn install(&self, png: &[u8]) -> Result<(), GlossError> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Err(GlossError::Config(
                "dock icon install needs the main thread".into(),
            ));
        };

        autoreleasepool(|_| {
            let data = NSData::with_bytes(png);
            let Some(image) = NSImage::initWithData(mtm.alloc::<NSImage>(), &data) else {
                return Err(GlossError::Config(
                    "the dock icon png does not decode into an image".into(),
                ));
            };

            let app = NSApplication::sharedApplication(mtm);
            // SAFETY: the main thread is already proven by the `MainThreadMarker`
            // above (`sharedApplication` takes it as an argument); `image` is a
            // `Retained<NSImage>` that stays alive within this closure and is not
            // released during the call, and AppKit only reads it.
            unsafe { app.setApplicationIconImage(Some(&image)) };
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_reports_an_error_off_the_main_thread() {
        let applied = std::thread::spawn(|| MacAppIcon::new().install(b"not a png"))
            .join()
            .expect("test thread must not panic");
        assert!(
            applied.is_err(),
            "off-main-thread call must report an error, got {applied:?}"
        );
    }
}
