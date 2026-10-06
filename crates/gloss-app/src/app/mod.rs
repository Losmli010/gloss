//! winit 事件循环：主线程的窗口生命周期与渲染驱动。
//!
//! 壳层按职责拆内部子模块，[`GlossApp`] 组合根与其余壳级编排留在本模块：
//! - [`events`]——事件循环入口与跨线程唤醒句柄；
//! - [`handler`]——winit 事件分发（`ApplicationHandler` 实现）。
//!
//! 通道消费与浮层显隐、动作执行在 `flow`（probe / task / reveal / actions），
//! 渲染帧管线与呈现资源在 `present`，设置编辑会话在 `flow::settings_session`。

mod events;
mod handler;

pub use crate::flow::reveal::centered_position;
pub use events::{UserEvent, Waker, run};

use std::sync::Arc;

use gloss_core::config::Theme;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::log::Span;
use gloss_core::model::Locale;
use gloss_core::ports::{ConfigStore, SceneProbe};
use winit::dpi::LogicalSize;
use winit::event_loop::EventLoopProxy;

use crate::channel::AppEndpoints;
use crate::flow::session::Session;
use crate::machine::{AppState, TaskStateMachine};
use crate::present::render::{render_frame, render_frame_with};
use crate::present::workspace::Workspace;
use crate::ui::i18n::Text;
use crate::ui::settings::{self, SettingsAction, SettingsState};
use crate::update::UpdateWiring;

/// 外部服务句柄聚合：配置、存储、场景探针、启动期系统语言与更新接线。
/// 句柄本身进程内不变（配置内容经句柄热更新）。
pub(crate) struct Env {
    /// 运行时配置句柄：每批平台事件取一份快照交给状态机，
    /// 配置保存后无需重启即对下一次触发生效。
    pub(crate) config: Arc<ConfigHandle>,
    /// 配置存储：设置页写 keychain 用——文档半边走句柄，密钥
    /// 半边不进快照也不进句柄，经这里直查。
    pub(crate) store: Arc<dyn ConfigStore>,
    /// 触发前场景探针：安全输入态与前台应用由它现读，壳只把它转交状态机
    /// 作场景闸门判定（见 `drain_platform_events`）。
    pub(crate) scene: Arc<dyn SceneProbe>,
    /// 启动期读到的系统语言：配置里的 `Language::System` 靠它落定成具体的
    /// 界面语言与 prompt 模板语言（进程内不变，改系统语言要重启）。
    pub(crate) system_locale: Locale,
    /// 更新子系统的壳侧接线：设置页每帧读其 receiver 渲染，用户动作经
    /// 出口转投模块（与主流程四通道隔离）。
    pub(crate) update: UpdateWiring,
}

impl Env {
    /// 当前界面语言：配置里的三态偏好按启动期系统语言落定（与 prompt
    /// 选表同一处取值）。逐帧从快照取——设置页保存后下一帧即换文案表。
    pub(crate) fn locale(&self) -> Locale {
        self.config.snapshot().language.resolve(self.system_locale)
    }

    /// 配置快照里的主题偏好：建帧（上下文建立时装入）与逐帧施加共用这
    /// 一处取值。
    pub(crate) fn target_theme(&self) -> Theme {
        self.config.snapshot().theme
    }
}

/// 主线程应用的组合根：每个所有者各持一类职责——任务决策在 `machine`、
/// 管线会话状态在 `session`、呈现资源在 `workspace`、服务句柄在 `env`；
/// 本结构体自身只剩绘制调度与设置窗编辑草稿。
pub(crate) struct GlossApp {
    /// 呈现资源：窗口管理器、两窗口渲染帧、各自的下一帧时刻、已施加主题。
    pub(crate) workspace: Workspace,
    /// 组装点移交的通道端点（① 收、② 发、③ 发、④ 收）。
    pub(crate) endpoints: Option<AppEndpoints>,
    /// 任务状态机（functional core，见 machine.rs）：纯状态转移，壳只做
    /// 通道发送、浮层窗口操作与日志。
    pub(crate) machine: TaskStateMachine,
    /// 触发/任务的管线会话状态（锚点、排查线索、显形挂起、任务 span）。
    pub(crate) session: Session,
    /// 外部服务句柄（配置/存储/场景/系统语言/更新接线/启动权限事实）。
    pub(crate) env: Env,
    /// 设置窗口的编辑会话；窗口可见时有值，关闭/保存完成即清（草稿随
    /// 之丢弃）。
    pub(crate) settings: Option<SettingsState>,
    /// 自定义事件的投递端：后台线程（字体装载）向主线程发完成信号用。
    /// 测试驱动（不经 run()）为 None——那条路径不触发字体装载。
    pub(crate) proxy: Option<EventLoopProxy<UserEvent>>,
}

impl GlossApp {
    /// 组装点移交的通道端点与外部服务句柄；窗口与帧状态在 `resumed` 时
    /// 建立。`system_locale` 同样来自组装点（系统语言是平台适配器的事，
    /// 壳只消费）。
    #[allow(
        clippy::too_many_arguments,
        reason = "组装点的主入口：每项都是不同关注点的注入端点，收敛成结构体只会把清单变成字段袋"
    )]
    fn new(
        endpoints: AppEndpoints,
        config: Arc<ConfigHandle>,
        store: Arc<dyn ConfigStore>,
        scene: Arc<dyn SceneProbe>,
        system_locale: Locale,
        update: UpdateWiring,
        proxy: Option<EventLoopProxy<UserEvent>>,
    ) -> Self {
        Self {
            workspace: Workspace::new(),
            endpoints: Some(endpoints),
            machine: TaskStateMachine::new(),
            session: Session::default(),
            env: Env {
                config,
                store,
                scene,
                system_locale,
                update,
            },
            settings: None,
            proxy,
        }
    }

    /// 指定代数的任务 span（副本，供 `enter()` 借用）；代数不符或尚无任务时
    /// 为 `None`。
    pub(crate) fn span_for(&self, generation: u64) -> Option<Span> {
        self.session.span_for(generation)
    }

    /// 画一帧：egui 出绘制数据 → wgpu 呈现，并把 egui 要求的下一帧记下
    /// 来；浮层上交的动作（失败卡与头部动作区）就地执行；页头拖动按
    /// 指针累计位移换算窗口落点；浮层内容的期望尺寸就地应用（内容自适
    /// 应高度，窗口管理器按显示器钳制）。
    fn draw(&mut self) {
        self.workspace.apply_theme(self.env.target_theme());
        let locale = self.env.locale();
        let Some(frame) = self.workspace.overlay_frame.as_mut() else {
            return;
        };
        let overlay_view = self.machine.overlay_view();
        let (repaint, output) = render_frame(frame, overlay_view, locale);
        self.workspace.overlay_repaint = repaint;
        if let Some(action) = output.action {
            self.handle_overlay_action(action);
        }
        // 页头拖动：落点以窗口当前实际位置为基准（无增量记账，见
        // apply_overlay_drag），拖动中的每帧按需平移。
        if let Some(offset) = output.drag
            && let Some(windows) = &mut self.workspace.windows
        {
            windows.apply_overlay_drag((f64::from(offset.x), f64::from(offset.y)));
        }
        if let Some(sizing) = output.sizing
            && let Some(windows) = &mut self.workspace.windows
        {
            // 流式期间走防抖尺寸（锁宽 + 步进增高），其余状态按精确尺寸
            // 重排——TaskDone 的定型重排也走这一支。
            let streaming = self.machine.state() == AppState::Translating;
            windows.set_overlay_size(
                LogicalSize::new(sizing.width as f64, sizing.height as f64),
                streaming,
            );
        }
    }

    /// 画一帧设置窗口：草稿编辑 + 动作上交（保存/密钥变更/取消/更新动作）。
    fn draw_settings(&mut self) {
        self.workspace.apply_theme(self.env.target_theme());
        let text = Text::get(self.env.locale());
        let update_state = self.env.update.receiver.borrow().clone();
        let (Some(frame), Some(state)) = (
            self.workspace.settings_frame.as_mut(),
            self.settings.as_mut(),
        ) else {
            return;
        };
        let (repaint, action) =
            render_frame_with(frame, |ui| settings::draw(ui, state, &update_state, text));
        self.workspace.settings_repaint = repaint;
        let Some(action) = action else {
            return;
        };
        match action {
            SettingsAction::Idle => {}
            SettingsAction::Save { config, key } => self.save_settings(*config, key),
            SettingsAction::Close => self.close_settings(),
            SettingsAction::Update(msg) => (self.env.update.send)(msg),
        }
    }

    pub(crate) fn request_redraw(&self) {
        self.workspace.request_redraw();
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Arc;

    use gloss_core::config::Config;
    use gloss_core::config_handle::ConfigHandle;
    use gloss_core::model::Locale;
    use gloss_core::model::ScreenPoint;
    use gloss_core::ports::{ConfigStore, SceneProbe};
    use gloss_core::task::TaskInput;

    use crate::channel::{AcquireCommand, AppEndpoints, Command, Event, PlatformEvent, Traced};
    use crate::machine::OverlayView;
    use crate::stubs::ports::{MemoryConfigStore, StubSceneProbe};

    use super::GlossApp;

    pub(crate) fn update_wiring() -> crate::update::UpdateWiring {
        let (_tx, receiver) =
            tokio::sync::watch::channel(crate::update::state::UpdateState::default());
        crate::update::UpdateWiring {
            receiver,
            send: Arc::new(|_msg: crate::update::UpdateMsg| {}),
        }
    }

    pub(crate) type DrivenApp = (
        GlossApp,
        Arc<ConfigHandle>,
        Arc<dyn ConfigStore>,
        crossbeam_channel::Sender<PlatformEvent>,
        crossbeam_channel::Receiver<Traced<AcquireCommand>>,
        tokio::sync::mpsc::UnboundedReceiver<Traced<Command>>,
        crossbeam_channel::Sender<Event>,
    );

    pub(crate) fn driven_app() -> DrivenApp {
        driven_app_with(Arc::new(MemoryConfigStore::default()))
    }

    pub(crate) fn driven_app_with(store: Arc<dyn ConfigStore>) -> DrivenApp {
        driven_app_with_scene(store, Arc::new(StubSceneProbe::default()))
    }

    pub(crate) fn driven_app_with_scene(
        store: Arc<dyn ConfigStore>,
        scene: Arc<dyn SceneProbe>,
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
            scene,
            Locale::Zh,
            update_wiring(),
            None,
        );
        (app, config, store, pe_tx, ac_rx, cmd_rx, ev_tx)
    }

    pub(crate) fn trigger_selection(
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

    pub(crate) fn text_input(text: &str) -> TaskInput {
        TaskInput::Text { text: text.into() }
    }

    pub(crate) fn plain_outcome(note: &str) -> gloss_core::task::TaskOutcome {
        gloss_core::task::TaskOutcome {
            kind: gloss_core::task::TaskKind::TranslateWord,
            note: note.into(),
            code_language: None,
            structured: gloss_core::task::OutcomeStructured::Plain {
                examples: Vec::new(),
            },
        }
    }

    pub(crate) fn streaming_raw(app: &GlossApp) -> &str {
        match app.machine.overlay_view() {
            Some(OverlayView::Streaming { raw, .. }) => raw,
            other => panic!("expected streaming view, got {other:?}"),
        }
    }

    pub(crate) fn outcome_note(app: &GlossApp) -> &str {
        match app.machine.overlay_view() {
            Some(OverlayView::Outcome { outcome, .. }) => &outcome.note,
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
            app.env.locale(),
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
            app.env.locale(),
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
            app.env.locale(),
            Locale::Zh,
            "System resolves through the startup system locale"
        );
    }
}
