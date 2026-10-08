//! 事件循环入口与跨线程唤醒句柄。

use std::error::Error;
use std::sync::Arc;

use gloss_core::config_handle::ConfigHandle;
use gloss_core::model::Locale;
use gloss_core::ports::{ConfigStore, SceneProbe};
use winit::event_loop::{EventLoop, EventLoopProxy};

use crate::channel::AppEndpoints;
use crate::update::UpdateWiring;

use super::GlossApp;

/// 投递给主线程的自定义事件。
#[derive(Clone, Copy, Debug)]
pub enum UserEvent {
    /// 有跨线程消息待处理
    Wake,
    /// 设置窗口内容自外部变化（更新子系统相位迁移）：窗口可见时请求重绘
    RedrawSettings,
    /// 系统字体装载完成：换完整字体表并按当前主题重施加，浮层请求重绘
    FontsReady,
}

/// 唤醒主线程的句柄：事件线程与 tokio 各持一份 clone。
#[derive(Clone, Debug)]
pub struct Waker(EventLoopProxy<UserEvent>);

impl Waker {
    /// 唤醒主线程；返回 `false` 表示事件循环已退出。
    pub fn wake(&self) -> bool {
        self.0.send_event(UserEvent::Wake).is_ok()
    }

    /// 唤醒主线程请求设置窗重绘（设置窗内容自外部变化时用）；返回 `false`
    /// 表示事件循环已退出。
    pub fn wake_settings(&self) -> bool {
        self.0.send_event(UserEvent::RedrawSettings).is_ok()
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
/// `theme` 施加到各 egui 上下文（见
/// `GlossApp::apply_theme`）。`system_locale` 是组装点读到的系统语言，
/// 供配置里的 `Language::System` 落定成 [`Locale`]（prompt 模板与界面文案共用）。
/// `scene` 是触发前场景探针（安全输入态、前台应用），供敏感信息防护的
/// 场景闸门判定——同样是平台适配器的事，壳只消费。`permissions_ready`
/// 是组装点的启动期权限预检结论（两项取材授权在启动时刻是否都已就绪），
/// 密钥预热据此门控（见 `GlossApp::send_secret_prewarm`）。
///
/// `update` 是更新子系统的壳侧接线（组装点经 `update::start_once()` 建立，
/// 见 [`UpdateWiring`]）：设置页每帧读 [`watch::Receiver`] 里的
/// [`UpdateState`] 渲染，用户动作经出口转投模块——与主流程四通道完全隔离。
#[allow(
    clippy::too_many_arguments,
    reason = "组装点的主入口：每项都是不同关注点的注入端点，收敛成结构体只会把清单变成字段袋"
)]
pub fn run(
    endpoints: AppEndpoints,
    config: Arc<ConfigHandle>,
    store: Arc<dyn ConfigStore>,
    scene: Arc<dyn SceneProbe>,
    system_locale: Locale,
    permissions_ready: bool,
    update: UpdateWiring,
    on_waker: impl FnOnce(Waker),
) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let waker = Waker(event_loop.create_proxy());
    on_waker(waker);
    let proxy = event_loop.create_proxy();
    let mut app = GlossApp::new(
        endpoints,
        config,
        store,
        scene,
        system_locale,
        permissions_ready,
        update,
        Some(proxy),
    );
    event_loop.run_app(&mut app)?;
    Ok(())
}
