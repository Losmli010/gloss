//! macOS 应用图标适配器（[`AppIcon`] 端口的实现）。

use gloss_core::ports::AppIcon;
use objc2::MainThreadMarker;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSApplication, NSImage};
use objc2_foundation::NSData;

/// macOS 应用图标：调 `NSApplication.setApplicationIconImage:` 装上品牌图标。
///
/// 图标来源分两条路：打包后的 .app 由 cargo-bundle 把 `assets/icons/Gloss.icns`
/// 拷进 Resources、经 Info.plist 的 `CFBundleIconFile` 加载；而 `just run`
/// （cargo 直接跑可执行文件）没有 bundle 可读，Dock 里是系统给的默认图标——
/// 这个适配器补的正是这一段。bundle 运行时它同样生效，只是用同一设计的
/// 256px PNG 覆盖 icns 默认值，视觉无差。
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

        // 调用点在 EventLoop::build 之后、run_app 之前，主线程此刻没有活跃的
        // autorelease pool（winit 的 pool 只包它自己的内部段），自建一个把
        // AppKit 解码沿途的临时对象收掉。
        autoreleasepool(|_| {
            // NSImage 直接吃 PNG 字节（系统自带解码），因此不需要引入图像解码库。
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

    /// 契约：非主线程调用不 panic、如实回报失败（降级由调用方处理）。
    /// libtest（本仓库锁定的 1.96.1）在 macOS 上无条件为每个测试 spawn 子线程，
    /// 但为不让契约依赖这条跑法细节，这里显式再开子线程断言。坏数据能否解出
    /// 图的分支要主线程 + NSApplication 才能走到，自动化覆盖不了，由 `just run`
    /// 人工走查（Dock 里能看到图标）。
    #[test]
    fn install_degrades_to_false_off_the_main_thread() {
        let applied = std::thread::spawn(|| MacAppIcon::new().install(b"not a png"))
            .join()
            .expect("test thread must not panic");
        assert!(!applied, "off-main-thread call must report failure");
    }
}
