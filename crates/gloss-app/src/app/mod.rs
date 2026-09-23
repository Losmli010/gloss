//! winit 事件循环：主线程的窗口生命周期与渲染驱动。
//!
//! 壳层按职责拆内部子模块，[`GlossApp`] 结构体与其余壳级编排留在本模块：
//! - [`events`]——事件循环入口与跨线程唤醒句柄；
//! - [`render`]——浮层与设置窗口共用的帧渲染管线；
//! - [`handler`]——winit 事件分发（`ApplicationHandler` 实现）；
//! - [`channels`]——通道①③④的消费与下发；
//! - [`overlay`]——浮层显隐、收起出口与浮层动作执行；
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

use gloss_core::config::Theme;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::log::Span;
use gloss_core::model::Locale;
use gloss_core::model::ScreenPoint;
use gloss_core::ports::{ConfigStore, HotkeyBinder};
use winit::dpi::LogicalSize;

use crate::channel::AppEndpoints;
use crate::i18n::Text;
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
    /// 启动期读到的系统语言：配置里的 `Language::System` 靠它落定成具体的
    /// 界面语言与 prompt 模板语言（进程内不变，改系统语言要重启）。
    system_locale: Locale,
    /// 已施加到两个 egui 上下文的主题；`None` 表示还没施加过（窗口未起时
    /// 会有这个状态）。
    applied_theme: Option<Theme>,
    /// 最近一次划词触发的释放坐标（随触发记录代数）：浮层跟随划词位置用，
    /// 代数对不上（热键触发、陈旧）时浮层回落居中。
    selection_anchor: Option<(u64, ScreenPoint)>,
    /// 当前任务的 span（触发点创建）：随通道②③下发，让接收线程的日志自动
    /// 带上 `generation`。重试沿用同一个（代数不变）。
    task_span: Option<Span>,
}

impl GlossApp {
    /// 组装点移交的通道端点、配置句柄、配置存储与热键端口；窗口与帧状态
    /// 在 `resumed` 时建立。`system_locale` 同样来自组装点（系统语言是
    /// 平台适配器的事，壳只消费）。
    fn new(
        endpoints: AppEndpoints,
        config: Arc<ConfigHandle>,
        store: Arc<dyn ConfigStore>,
        hotkeys: Arc<dyn HotkeyBinder>,
        system_locale: Locale,
    ) -> Self {
        Self {
            windows: None,
            frame: None,
            overlay_repaint: None,
            settings_repaint: None,
            endpoints: Some(endpoints),
            machine: TaskStateMachine::new(),
            config,
            store,
            settings_frame: None,
            settings: None,
            hotkeys,
            system_locale,
            applied_theme: None,
            selection_anchor: None,
            task_span: None,
        }
    }

    /// 当前任务 span 的副本：`enter()` 的守卫借用这个局部值，与 `&mut self`
    /// 不冲突；无任务时为 `None`（空操作）。
    fn current_task_span(&self) -> Option<Span> {
        self.task_span.clone()
    }

    /// 当前界面语言：配置里的三态偏好按启动期系统语言落定（与 prompt
    /// 选表同一处取值）。逐帧从快照取——设置页保存后下一帧即换文案表。
    fn locale(&self) -> Locale {
        self.config.snapshot().language.resolve(self.system_locale)
    }

    /// 画一帧：egui 出绘制数据 → wgpu 呈现，并把 egui 要求的下一帧记下
    /// 来；浮层上交的动作（失败卡与头部动作区）就地执行；浮层内容的期望
    /// 尺寸就地应用（内容自适应高度，窗口管理器按显示器钳制）。
    fn draw(&mut self) {
        self.apply_theme();
        let locale = self.locale();
        let Some(frame) = self.frame.as_mut() else {
            return;
        };
        let overlay_view = self.machine.overlay_view();
        let (repaint, action, sizing) = render_frame(frame, overlay_view, locale);
        self.overlay_repaint = repaint;
        if let Some(action) = action {
            self.handle_overlay_action(action);
        }
        if let Some(sizing) = sizing
            && let Some(windows) = &mut self.windows
        {
            windows.set_overlay_size(LogicalSize::new(sizing.width as f64, sizing.height as f64));
        }
    }

    /// 画一帧设置窗口：草稿编辑 + 动作上交（保存/密钥变更/取消）。
    fn draw_settings(&mut self) {
        self.apply_theme();
        let text = Text::get(self.locale());
        let (Some(frame), Some(state)) = (&mut self.settings_frame, &mut self.settings) else {
            return;
        };
        let (repaint, action) = render_frame_with(frame, |ui| settings::draw(ui, state, text));
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
mod test_support {
    use std::sync::Arc;

    use gloss_core::config::Config;
    use gloss_core::config_handle::ConfigHandle;
    use gloss_core::model::Locale;
    use gloss_core::model::ScreenPoint;
    use gloss_core::ports::{ConfigStore, HotkeyBinder};
    use gloss_core::task::TaskInput;

    use crate::channel::{AcquireCommand, AppEndpoints, Command, Event, PlatformEvent, Traced};
    use crate::machine::OverlayView;
    use crate::stubs::ports::{MemoryConfigStore, RecordingHotkeyBinder};

    use super::GlossApp;

    pub(super) type DrivenApp = (
        GlossApp,
        Arc<ConfigHandle>,
        Arc<dyn ConfigStore>,
        crossbeam_channel::Sender<PlatformEvent>,
        crossbeam_channel::Receiver<Traced<AcquireCommand>>,
        tokio::sync::mpsc::UnboundedReceiver<Traced<Command>>,
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
            Locale::Zh,
        );
        (app, config, store, pe_tx, ac_rx, cmd_rx, ev_tx)
    }

    pub(super) fn trigger_selection(
        app: &mut GlossApp,
        pe_tx: &crossbeam_channel::Sender<PlatformEvent>,
    ) {
        pe_tx
            .send(PlatformEvent::SelectionGesture {
                pos: ScreenPoint::new(0, 0),
            })
            .unwrap();
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

#[cfg(test)]
mod tests {
    use gloss_core::config::{Config, Language};
    use gloss_core::model::Locale;

    use super::test_support::driven_app;

    #[test]
    fn ui_locale_follows_the_saved_language_without_a_restart() {
        let (app, config, _store, _pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        assert_eq!(
            app.locale(),
            Locale::Zh,
            "the factory default follows the system locale"
        );

        config
            .save(Config {
                language: Language::En,
                ..Default::default()
            })
            .expect("save should succeed");
        assert_eq!(
            app.locale(),
            Locale::En,
            "a saved language must drive the next frame's table, no restart needed"
        );

        config
            .save(Config {
                language: Language::System,
                ..Default::default()
            })
            .expect("save should succeed");
        assert_eq!(
            app.locale(),
            Locale::Zh,
            "System resolves through the startup system locale"
        );
    }
}
