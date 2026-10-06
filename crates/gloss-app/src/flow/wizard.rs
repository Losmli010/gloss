//! 启动向导会话：授权引导步骤与监听失效提示的生命周期。
//!
//! 步骤决策读启动事实（`Env::startup`），推进由用户动作驱动（每步等一次
//! 确认或收起）；步骤走完即发密钥预热命令——读密钥的系统授权框由此前置
//! 到启动期受控出现，读到的值进存储的进程内缓存，首次划词不再弹。运行中
//! 监听失效（`MouseListenerDegraded`）复用同一窗口呈现提示卡。
//!
//! 向导不进任务状态机：它是一次性会话（与设置编辑会话同类），不占请求
//! 代数，窗口关闭即结束。

use std::collections::VecDeque;

use gloss_core::log::{debug, info, thread, warn};

use gloss_platform::permissions;

use crate::app::GlossApp;
use crate::channel::{Command, Traced};
use crate::ui::i18n::Text;
use crate::ui::wizard::{WizardState, WizardStep, WizardView};

/// 授权引导动作（`Env::guide` 的出厂实现）：步骤 → 打开对应系统设置面板。
pub(crate) fn platform_guide(step: WizardStep) -> bool {
    match step {
        WizardStep::Accessibility => permissions::open_accessibility_pane(),
        WizardStep::InputMonitoring => permissions::open_input_monitoring_pane(),
    }
}

impl GlossApp {
    /// 启动向导入口（`resumed` 调用一次）：按启动事实排出缺失授权的引导
    /// 步骤并停在第一步；两项授权都已就绪则不建会话，直接发预热——密码
    /// 框在启动期照常受控出现。
    pub(crate) fn start_wizard(&mut self) {
        let facts = self.env.startup;
        let mut steps = VecDeque::new();
        if !facts.accessibility_granted {
            steps.push_back(WizardStep::Accessibility);
        }
        if !facts.mouse_listening {
            steps.push_back(WizardStep::InputMonitoring);
        }
        if steps.is_empty() {
            info!(thread = thread::UI, "event permissions granted at startup");
            self.send_secret_prewarm();
            return;
        }
        if let Some(first) = steps.pop_front() {
            self.wizard = Some(WizardState {
                view: WizardView::Step(first),
                steps,
                prewarm_sent: false,
            });
            self.show_wizard();
        }
    }

    /// 推进向导：展示下一个待引导步骤；没有步骤了就收起窗口、发预热并
    /// 结束会话。用户动作（打开设置/收起）与窗口关闭键都汇到这里。
    pub(crate) fn advance_wizard(&mut self) {
        let Some(mut state) = self.wizard.take() else {
            return;
        };
        match state.steps.pop_front() {
            Some(step) => {
                state.view = WizardView::Step(step);
                self.wizard = Some(state);
                self.show_wizard();
            }
            None => self.finish_wizard(state),
        }
    }

    /// 「打开系统设置」动作：开对应授权面板后推进向导（面板打开失败只降
    /// 级记日志，不拦推进——引导路径不挡路）。
    pub(crate) fn open_wizard_guide(&mut self, step: WizardStep) {
        let opened = (self.env.guide)(step);
        if opened {
            info!(
                thread = thread::UI,
                ?step,
                "permission pane opened from the wizard"
            );
        }
        self.advance_wizard();
    }

    /// 鼠标监听运行中失效（`MouseListenerDegraded`）：复用向导窗口呈现提
    /// 示卡。已停在输入监控步骤的向导不重复提示；其余情形把输入监控步骤
    /// 插到队首并展示（失效的恢复路径与缺失授权相同：重新授权 + 重启）。
    pub(crate) fn on_listener_degraded(&mut self) {
        if let Some(state) = &self.wizard
            && matches!(state.view, WizardView::Step(WizardStep::InputMonitoring))
        {
            return;
        }
        warn!(
            thread = thread::UI,
            "guiding input monitoring after listener degradation"
        );
        match &mut self.wizard {
            Some(state) => {
                // 当前正在展示的步骤排回队尾（失效提示插队，原步骤不丢）；
                // 队列里已有的输入监控步骤去重后再插到队首。
                if let WizardView::Step(current) = state.view {
                    state.steps.push_back(current);
                }
                state
                    .steps
                    .retain(|step| *step != WizardStep::InputMonitoring);
                state.steps.push_front(WizardStep::InputMonitoring);
                self.advance_wizard();
            }
            None => {
                self.wizard = Some(WizardState {
                    view: WizardView::Degraded,
                    steps: VecDeque::new(),
                    prewarm_sent: true,
                });
                self.show_wizard();
            }
        }
    }

    /// 密钥预热回执（`SecretPrewarmed`）：只留痕——预热失败不拦主流程，
    /// 首次任务会自然重读并按既有失败路径兜底。
    pub(crate) fn on_secret_prewarmed(
        &mut self,
        result: &Result<(), gloss_core::model::GlossError>,
    ) {
        if let Err(err) = result {
            debug!(thread = thread::UI, error = %err, "secret prewarm reported failure");
        }
    }

    /// 向导窗口的关闭键：跳过剩余引导，直接收尾（收起 + 按需预热 + 结束
    /// 会话）——关闭是对整个引导序列的「不看了」，与单步「跳过」分开。
    pub(crate) fn skip_wizard(&mut self) {
        let Some(state) = self.wizard.take() else {
            return;
        };
        info!(
            thread = thread::UI,
            remaining = state.steps.len(),
            "startup wizard skipped"
        );
        self.hide_wizard();
        if !state.prewarm_sent {
            self.send_secret_prewarm();
        }
    }

    /// 步骤走完：收起窗口，按需发预热，结束会话（`prewarm_sent` 挡住运行
    /// 中失效插入的会话重发）。
    fn finish_wizard(&mut self, state: WizardState) {
        info!(thread = thread::UI, "startup wizard finished");
        self.hide_wizard();
        if !state.prewarm_sent {
            self.send_secret_prewarm();
        }
    }

    /// 展示向导窗口（标题按当前界面语言；内容变化由重绘带上）。
    fn show_wizard(&mut self) {
        let title = Text::get(self.env.locale()).gloss_wizard_title.as_str();
        if let Some(windows) = &self.workspace.windows {
            windows.show_wizard(title);
            windows.request_redraw_wizard();
        }
    }

    /// 收起向导窗口（隐藏不销毁）。
    fn hide_wizard(&mut self) {
        self.workspace.wizard_repaint = None;
        if let Some(windows) = &self.workspace.windows {
            windows.hide_wizard();
        }
    }

    /// 发密钥预热命令（通道③，不带任务 span——预热不属于任何任务）；发
    /// 送失败（通道已关，应用正在退出）只留痕。
    fn send_secret_prewarm(&mut self) {
        let keychain_id = self
            .env
            .config
            .snapshot()
            .resolved_provider()
            .keychain_id
            .clone();
        let Some(endpoints) = &self.endpoints else {
            return;
        };
        let command = Traced::untraced(Command::PrewarmSecret { keychain_id });
        if let Err(err) = endpoints.commands.send(command) {
            debug!(
                thread = thread::UI,
                error = %err,
                "command channel closed, secret prewarm dropped"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::app::StartupFacts;
    use crate::app::test_support::driven_app_with_facts;
    use crate::channel::{Command, PlatformEvent, Traced};
    use crate::stubs::ports::{MemoryConfigStore, StubSceneProbe};
    use crate::ui::wizard::{WizardStep, WizardView};

    use super::GlossApp;

    fn stub_guide(_step: WizardStep) -> bool {
        true
    }

    fn wizard_app(
        accessibility_granted: bool,
        mouse_listening: bool,
    ) -> (
        GlossApp,
        crossbeam_channel::Sender<PlatformEvent>,
        tokio::sync::mpsc::UnboundedReceiver<Traced<Command>>,
    ) {
        let (mut app, _config, _store, pe_tx, _ac_rx, cmd_rx, _ev_tx) = driven_app_with_facts(
            Arc::new(MemoryConfigStore::default()),
            Arc::new(StubSceneProbe::default()),
            StartupFacts {
                accessibility_granted,
                mouse_listening,
            },
        );
        app.env.guide = stub_guide;
        (app, pe_tx, cmd_rx)
    }

    #[test]
    fn missing_permissions_open_the_wizard_in_dependency_order() {
        let (mut app, _pe_tx, mut cmd_rx) = wizard_app(false, false);
        app.start_wizard();

        let state = app.wizard.as_ref().expect("wizard session expected");
        assert_eq!(state.view, WizardView::Step(WizardStep::Accessibility));
        assert_eq!(state.steps.len(), 1, "the other missing step is queued");

        app.open_wizard_guide(WizardStep::Accessibility);
        let state = app
            .wizard
            .as_ref()
            .expect("the second step keeps the session");
        assert_eq!(state.view, WizardView::Step(WizardStep::InputMonitoring));
        assert!(
            cmd_rx.try_recv().is_err(),
            "no prewarm before the steps are done"
        );

        app.open_wizard_guide(WizardStep::InputMonitoring);
        assert!(
            app.wizard.is_none(),
            "the last confirm finishes the sequence"
        );
        assert!(matches!(
            cmd_rx.try_recv().unwrap().payload,
            Command::PrewarmSecret { .. }
        ));
    }

    #[test]
    fn granted_permissions_skip_the_dialog_and_warm_up_directly() {
        let (mut app, _pe_tx, mut cmd_rx) = wizard_app(true, true);
        app.start_wizard();

        assert!(
            app.wizard.is_none(),
            "no dialog when both grants are present"
        );
        assert!(matches!(
            cmd_rx.try_recv().unwrap().payload,
            Command::PrewarmSecret { .. }
        ));
        assert!(cmd_rx.try_recv().is_err(), "the prewarm fires exactly once");
    }

    #[test]
    fn only_the_missing_permission_gets_a_step() {
        let (mut app, _pe_tx, mut cmd_rx) = wizard_app(true, false);
        app.start_wizard();

        let state = app
            .wizard
            .as_ref()
            .expect("wizard expected for the IM step only");
        assert_eq!(state.view, WizardView::Step(WizardStep::InputMonitoring));
        app.open_wizard_guide(WizardStep::InputMonitoring);
        assert!(app.wizard.is_none());
        assert!(matches!(
            cmd_rx.try_recv().unwrap().payload,
            Command::PrewarmSecret { .. }
        ));
    }

    #[test]
    fn acknowledge_advances_without_opening_a_pane() {
        let (mut app, _pe_tx, mut cmd_rx) = wizard_app(false, false);
        app.start_wizard();

        app.advance_wizard();
        let state = app
            .wizard
            .as_ref()
            .expect("the next step shows after a skip");
        assert_eq!(state.view, WizardView::Step(WizardStep::InputMonitoring));

        app.advance_wizard();
        assert!(app.wizard.is_none());
        assert!(matches!(
            cmd_rx.try_recv().unwrap().payload,
            Command::PrewarmSecret { .. }
        ));
    }

    #[test]
    fn closing_the_window_skips_the_whole_sequence_but_still_warms_up() {
        let (mut app, _pe_tx, mut cmd_rx) = wizard_app(false, false);
        app.start_wizard();

        app.skip_wizard();
        assert!(app.wizard.is_none());
        assert!(matches!(
            cmd_rx.try_recv().unwrap().payload,
            Command::PrewarmSecret { .. }
        ));
    }

    #[test]
    fn degradation_shows_the_notice_without_rewarming() {
        let (mut app, pe_tx, mut cmd_rx) = wizard_app(true, true);
        app.start_wizard();
        assert!(cmd_rx.try_recv().is_ok(), "startup prewarm");

        pe_tx
            .send(PlatformEvent::MouseListenerDegraded)
            .expect("platform channel alive");
        app.drain_platform_events();

        let state = app.wizard.as_ref().expect("degradation shows the notice");
        assert_eq!(state.view, WizardView::Degraded);
        assert!(
            cmd_rx.try_recv().is_err(),
            "the degraded notice must not re-fire the prewarm"
        );

        app.advance_wizard();
        assert!(
            app.wizard.is_none(),
            "dismissing the notice closes the session"
        );
    }

    #[test]
    fn degradation_during_guidance_jumps_the_input_monitoring_step_ahead() {
        let (mut app, pe_tx, mut cmd_rx) = wizard_app(false, false);
        app.start_wizard();

        pe_tx
            .send(PlatformEvent::MouseListenerDegraded)
            .expect("platform channel alive");
        app.drain_platform_events();

        let state = app.wizard.as_ref().expect("still guiding");
        assert_eq!(
            state.view,
            WizardView::Step(WizardStep::InputMonitoring),
            "the IM step jumps ahead of the pending accessibility step"
        );
        assert_eq!(state.steps.len(), 1, "the accessibility step stays queued");

        app.open_wizard_guide(WizardStep::InputMonitoring);
        let state = app.wizard.as_ref().expect("the queued step shows next");
        assert_eq!(state.view, WizardView::Step(WizardStep::Accessibility));
        assert!(cmd_rx.try_recv().is_err());

        app.open_wizard_guide(WizardStep::Accessibility);
        assert!(matches!(
            cmd_rx.try_recv().unwrap().payload,
            Command::PrewarmSecret { .. }
        ));
    }

    #[test]
    fn degradation_while_already_guiding_input_monitoring_is_a_no_op() {
        let (mut app, pe_tx, mut cmd_rx) = wizard_app(false, false);
        app.start_wizard();
        app.open_wizard_guide(WizardStep::Accessibility);

        pe_tx
            .send(PlatformEvent::MouseListenerDegraded)
            .expect("platform channel alive");
        app.drain_platform_events();

        let state = app.wizard.as_ref().expect("the IM step keeps the session");
        assert_eq!(state.view, WizardView::Step(WizardStep::InputMonitoring));
        assert_eq!(state.steps.len(), 0, "no duplicate step is queued");
        assert!(cmd_rx.try_recv().is_err());
    }
}
