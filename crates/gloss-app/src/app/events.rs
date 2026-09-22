//! 事件循环入口与跨线程唤醒句柄。

use std::error::Error;
use std::sync::Arc;

use gloss_core::config_handle::ConfigHandle;
use gloss_core::model::Locale;
use gloss_core::ports::{ConfigStore, HotkeyBinder};
use winit::event_loop::{EventLoop, EventLoopProxy};

use crate::channel::AppEndpoints;

use super::GlossApp;

/// 投递给主线程的自定义事件。
#[derive(Clone, Copy, Debug)]
pub enum UserEvent {
    /// 有跨线程消息待处理
    Wake,
}

/// 唤醒主线程的句柄：事件线程与 tokio 各持一份 clone。
#[derive(Clone, Debug)]
pub struct Waker(EventLoopProxy<UserEvent>);

impl Waker {
    /// 唤醒主线程；返回 `false` 表示事件循环已退出。
    pub fn wake(&self) -> bool {
        self.0.send_event(UserEvent::Wake).is_ok()
    }
}

/// 启动事件循环，直到退出才返回。
///
/// `endpoints` 是 App 侧通道端点（① 收平台事件、② 发取材命令、③ 发推理
/// 任务、④ 收回传事件），由组装点拆出移交；`config` 是运行时配置句柄，
/// 在每一批平台事件的起手处取一份快照交给状态机（见
/// `GlossApp::drain_platform_events`），配置热更新因此无需重启；`store`
/// 是配置存储的文档+密钥组合体，设置页经它写 keychain（密钥不经快照，
/// 也不进句柄）。
///
/// `on_waker` 拿到唤醒句柄——`main.rs` 是唯一组装点，句柄要由它分发给
/// 平台事件线程与 tokio，库这边不替上层决定跨线程拓扑。
///
/// `hotkeys` 是热键重绑定端口：适配器在组装点创建（注册有主线程亲和），
/// 设置页保存后由 App 直接调用。`theme` 施加到两个 egui 上下文（见
/// `GlossApp::apply_theme`）。`system_locale` 是组装点读到的系统语言，
/// 供配置里的 `Language::System` 落定成 prompt 模板语言。
pub fn run(
    endpoints: AppEndpoints,
    config: Arc<ConfigHandle>,
    store: Arc<dyn ConfigStore>,
    hotkeys: Arc<dyn HotkeyBinder>,
    system_locale: Locale,
    on_waker: impl FnOnce(Waker),
) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let waker = Waker(event_loop.create_proxy());
    on_waker(waker);
    let mut app = GlossApp::new(endpoints, config, store, hotkeys, system_locale);
    event_loop.run_app(&mut app)?;
    Ok(())
}
