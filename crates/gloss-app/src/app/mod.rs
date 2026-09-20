//! winit 事件循环：主线程的窗口生命周期与渲染驱动。
//!
//! 壳层按职责拆内部子模块，[`GlossApp`] 结构体与其余壳级编排留在本模块：
//! - [`events`]——事件循环入口与跨线程唤醒句柄；
//! - [`render`]——浮层与设置窗口共用的帧渲染管线；
//! - [`handler`]——winit 事件分发（`ApplicationHandler` 实现）；
//! - [`channels`]——通道①③④的消费与下发；
//! - [`overlay`]——浮层显隐、自动隐藏与失败卡动作出口；
//! - [`settings_session`]——设置窗口的编辑会话生命周期；
//! - [`theme`]——主题偏好施加到两个 egui 上下文。

mod channels;
mod events;
mod handler;
mod overlay;
mod render;
mod settings_session;
mod theme;

pub use events::{UserEvent, Waker, run};
pub use overlay::centered_position;
pub use render::{Frame, build_window_stack, render_frame, render_frame_with};

use std::sync::Arc;
use std::time::Instant;

use gloss_core::config_handle::ConfigHandle;
use gloss_core::ports::{ConfigStore, HotkeyBinder};

use crate::channel::AppEndpoints;
use crate::machine::TaskStateMachine;
use crate::ui::settings::{self, SettingsAction, SettingsState};
use crate::windows::WindowManager;

struct GlossApp {
    windows: Option<WindowManager>,
    frame: Option<Frame>,
    /// 浮层 egui 要求的下一帧时间点；`None` 表示等到有事件再画。
    overlay_repaint: Option<Instant>,
    /// 设置窗口 egui 要求的下一帧时间点，与浮层的 [`Self::overlay_repaint`]
    /// 各自独立。
    settings_repaint: Option<Instant>,
    /// 浮层自动隐藏时刻；仅浮层可见时为 `Some`
    auto_hide: Option<Instant>,
    /// 组装点移交的通道端点（① 收、② 发、③ 发、④ 收）。
    endpoints: Option<AppEndpoints>,
    /// 任务状态机（functional core，见 machine.rs）：纯状态转移，壳只做
    /// 通道发送、浮层窗口操作与日志。
    machine: TaskStateMachine,
    /// 运行时配置句柄：每批平台事件取一份快照交给状态机，
    /// 配置保存后无需重启即对下一次触发生效。
    config: Arc<ConfigHandle>,
    /// 配置存储：设置页写 keychain 用——文档半边走句柄，密钥
    /// 半边不进快照也不进句柄，经这里直查。
    store: Arc<dyn ConfigStore>,
    /// 设置窗口的渲染帧；随窗口栈在 `resumed` 时建好，隐藏期保留。
    settings_frame: Option<Frame>,
    /// 设置窗口的编辑会话；窗口可见时有值，关闭/保存完成即清（草稿随
    /// 之丢弃）。
    settings: Option<SettingsState>,
    /// 热键重绑定端口：设置页保存后在主线程同步调用，不走通道。
    hotkeys: Arc<dyn HotkeyBinder>,
    /// 已施加到两个 egui 上下文的主题偏好；`None` 表示还没施加过。
    applied_theme: Option<egui::ThemePreference>,
}

impl GlossApp {
    /// 组装点移交的通道端点、配置句柄、配置存储与热键端口；窗口与帧状态
    /// 在 `resumed` 时建立。
    fn new(
        endpoints: AppEndpoints,
        config: Arc<ConfigHandle>,
        store: Arc<dyn ConfigStore>,
        hotkeys: Arc<dyn HotkeyBinder>,
    ) -> Self {
        Self {
            windows: None,
            frame: None,
            overlay_repaint: None,
            settings_repaint: None,
            auto_hide: None,
            endpoints: Some(endpoints),
            machine: TaskStateMachine::new(),
            config,
            store,
            settings_frame: None,
            settings: None,
            hotkeys,
            applied_theme: None,
        }
    }

    /// 画一帧：egui 出绘制数据 → wgpu 呈现，并把 egui 要求的下一帧记下
    /// 来；失败卡上的动作按钮（重试/打开设置）就地执行。
    fn draw(&mut self) {
        self.apply_theme();
        let Some(frame) = self.frame.as_mut() else {
            return;
        };
        let overlay_view = self.machine.overlay_view();
        let (repaint, action) = render_frame(frame, overlay_view);
        self.overlay_repaint = repaint;
        // 失败卡的动作出口：重试原样重发，鉴权/配置类打开设置。
        if let Some(action) = action {
            self.handle_error_action(action);
        }
    }

    /// 画一帧设置窗口：草稿编辑 + 动作上交（保存/密钥变更/取消）。
    fn draw_settings(&mut self) {
        self.apply_theme();
        let (Some(frame), Some(state)) = (&mut self.settings_frame, &mut self.settings) else {
            return;
        };
        let (repaint, action) = render_frame_with(frame, |ui| settings::draw(ui, state));
        self.settings_repaint = repaint;
        let Some(action) = action else {
            return;
        };
        match action {
            SettingsAction::Idle => {}
            SettingsAction::Save { config, key } => self.save_settings(config, key),
            SettingsAction::Close => self.close_settings(),
        }
    }

    fn request_redraw(&self) {
        if let Some(windows) = &self.windows {
            windows.request_redraw();
        }
    }
}

#[cfg(test)]
pub(super) mod test_support {
    use std::sync::Arc;

    use gloss_core::config::Config;
    use gloss_core::config_handle::ConfigHandle;
    use gloss_core::ports::{ConfigStore, HotkeyBinder};
    use gloss_core::task::TaskInput;

    use crate::channel::{AcquireCommand, AppEndpoints, Command, Event, PlatformEvent};
    use crate::machine::OverlayView;
    use crate::stubs::ports::{MemoryConfigStore, RecordingHotkeyBinder};

    use super::GlossApp;

    pub(super) type DrivenApp = (
        GlossApp,
        Arc<ConfigHandle>,
        Arc<dyn ConfigStore>,
        crossbeam_channel::Sender<PlatformEvent>,
        crossbeam_channel::Receiver<AcquireCommand>,
        tokio::sync::mpsc::UnboundedReceiver<Command>,
        crossbeam_channel::Sender<Event>,
    );

    pub(super) fn driven_app() -> DrivenApp {
        driven_app_with(Arc::new(MemoryConfigStore::default()))
    }

    pub(super) fn driven_app_with(store: Arc<dyn ConfigStore>) -> DrivenApp {
        driven_app_using(store, Arc::new(RecordingHotkeyBinder::default()))
    }

    pub(super) fn driven_app_using(
        store: Arc<dyn ConfigStore>,
        hotkeys: Arc<dyn HotkeyBinder>,
    ) -> DrivenApp {
        let crate::channel::Channels {
            platform_events,
            acquire_commands,
            commands,
            events,
        } = crate::channel::Channels::new();
        let crate::channel::CrossbeamPair {
            tx: pe_tx,
            rx: pe_rx,
        } = platform_events;
        let crate::channel::CrossbeamPair {
            tx: ac_tx,
            rx: ac_rx,
        } = acquire_commands;
        let crate::channel::CrossbeamPair {
            tx: ev_tx,
            rx: ev_rx,
        } = events;
        let crate::channel::CommandChannel {
            tx: cmd_tx,
            rx: cmd_rx,
        } = commands;
        let config = Arc::new(ConfigHandle::with_config(
            Arc::clone(&store),
            Config::default(),
        ));
        let app = GlossApp::new(
            AppEndpoints {
                platform_events: pe_rx,
                acquire_commands: ac_tx,
                commands: cmd_tx,
                events: ev_rx,
            },
            Arc::clone(&config),
            Arc::clone(&store) as Arc<dyn ConfigStore>,
            hotkeys,
        );
        (app, config, store, pe_tx, ac_rx, cmd_rx, ev_tx)
    }

    pub(super) fn trigger_selection(
        app: &mut GlossApp,
        pe_tx: &crossbeam_channel::Sender<PlatformEvent>,
    ) {
        pe_tx.send(PlatformEvent::SelectionGesture).unwrap();
        app.drain_platform_events();
    }

    pub(super) fn text_input(text: &str) -> TaskInput {
        TaskInput::Text {
            text: text.into(),
            hint: None,
        }
    }

    pub(super) fn plain_outcome(body: &str) -> gloss_core::task::TaskOutcome {
        gloss_core::task::TaskOutcome {
            kind: gloss_core::task::TaskKind::TranslateWord,
            body: body.into(),
            structured: gloss_core::task::OutcomeStructured::Plain { title: None },
        }
    }

    pub(super) fn streaming_body(app: &GlossApp) -> &str {
        match app.machine.overlay_view() {
            Some(OverlayView::Streaming { body, .. }) => body,
            other => panic!("expected streaming view, got {other:?}"),
        }
    }

    pub(super) fn outcome_body(app: &GlossApp) -> &str {
        match app.machine.overlay_view() {
            Some(OverlayView::Outcome(outcome)) => &outcome.body,
            other => panic!("expected outcome view, got {other:?}"),
        }
    }
}
