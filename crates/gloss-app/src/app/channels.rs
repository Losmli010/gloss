//! 通道①③④的消费与下发：平台事件→取材命令（通道②经 machine 产出）、
//! 取材产物→推理任务（通道③）、回传事件→浮层展示决策（通道④）。

use std::time::Instant;

use gloss_core::log::{debug, info, thread, warn};
use gloss_core::task::TaskInput;
use winit::event_loop::ActiveEventLoop;

use crate::channel::{AcquireCommand, Command, Event, PlatformEvent};
use crate::machine::RunRequest;

use super::GlossApp;
use super::overlay::{
    AUTO_HIDE_AFTER, auto_show_after, centered_position, event_kind, show_position,
};

impl GlossApp {
    /// 消费通道①：平台事件 → 取材命令。只有真实下发的命令才占用新代数
    /// （未接线事件不作废在途回传）；触发→命令的日志链路同时承担热键端到
    /// 端的验收验证（CI 无法合成真实按键，只能真机按日志走查）。
    pub(super) fn drain_platform_events(&mut self) {
        // 配置快照在本批事件的起手处取一次（零锁读）：本批触发的任务都用
        // 同一份配置解析类型与选项——任务一旦触发，其配置就固定了。
        let config = self.config.snapshot();
        // 先收集再处理：endpoints 的借用与 &mut self 互斥，收进 Vec 后即
        // 归还，后续可用正常的方法调用。
        let events: Vec<PlatformEvent> = self
            .endpoints
            .as_ref()
            .map_or(Vec::new(), |e| e.platform_events.try_iter().collect());
        for event in events {
            // 设置入口：托盘/热键与浮层失败卡共用同一条路；不占
            // 用代数（与未接线事件一样不进状态机）。
            if matches!(event, PlatformEvent::OpenSettingsRequested) {
                info!(thread = thread::UI, "settings open requested");
                self.open_settings();
                continue;
            }
            let superseded = self.machine.current_cancel().is_some();
            if let Some(command) = self.machine.trigger(&event, &config) {
                info!(
                    thread = thread::UI,
                    generation = self.machine.generation(),
                    cancelled_inflight = superseded,
                    "platform event dispatched as acquire command"
                );
                // 划词触发记录释放坐标（随代数）：浮层显示时跟随选区；
                // 其它触发源（热键）不带坐标，显示决策回落居中。
                if let PlatformEvent::SelectionGesture { pos } = event {
                    self.selection_anchor = Some((self.machine.generation(), pos));
                }
                self.send_acquire(command);
            } else {
                debug!(
                    thread = thread::UI,
                    event = ?event,
                    "platform event ignored: not wired yet, or its task kind is disabled"
                );
            }
        }
    }

    /// 通道②发送；接收端消失（事件线程死亡/退出）时落 Error 态兜底，
    /// 避免滞留 Fetching。
    fn send_acquire(&mut self, command: AcquireCommand) {
        let Some(endpoints) = &self.endpoints else {
            return;
        };
        if endpoints.acquire_commands.send(command).is_err() {
            warn!(
                thread = thread::UI,
                generation = self.machine.generation(),
                "acquire channel closed, command dropped"
            );
            self.machine.fail_acquire(self.machine.generation());
        }
    }

    /// 消费通道④：取材产物按代数采纳——连续快速触发时旧代数的产物被
    /// 丢弃，浮层只显示最后一次请求的结果。状态决策在 machine，壳只做
    /// 通道发送、浮层展示与日志。
    ///
    /// 浮层「什么时候自动露面」由 `auto_show` 决定，策略本身抽在
    /// [`super::overlay::auto_show_for`] / [`super::overlay::auto_show_after`]
    /// 这两个纯函数里。它是壳侧的展示开关，不随任务下发、也不参与缓存
    /// key，因此不像任务选项那样在触发时冻结——按到达时的快照读即可。
    pub(super) fn drain_events(&mut self, event_loop: &ActiveEventLoop) {
        let events: Vec<Event> = self
            .endpoints
            .as_ref()
            .map_or(Vec::new(), |e| e.events.try_iter().collect());
        let auto_show = self.config.snapshot().auto_show;
        // 先按批推进状态机、收集每条的采纳结果，再一次性决定这一批要不要
        // 露面：逐条 `|=` 等价于按批取或，抽出来是为了这条语义可测。
        let mut batch = Vec::with_capacity(events.len());
        for event in events {
            // 策略只关心「哪一类回传」，事件本身在下一行被消费掉。
            let kind = event_kind(&event);
            let accepted = match event {
                Event::InputReady { generation, input } => self.accept_input(generation, input),
                Event::TaskChunk { generation, delta } => self.accept_chunk(generation, delta),
                Event::TaskDone {
                    generation,
                    outcome,
                } => {
                    let accepted = self.accept_done(generation, outcome);
                    // 结果卡可见时长从完成时刻重新起算。
                    if accepted && self.windows.is_some() {
                        self.auto_hide = Some(Instant::now() + AUTO_HIDE_AFTER);
                    }
                    accepted
                }
                Event::TaskFailed { generation, error } => self.accept_failed(generation, &error),
            };
            batch.push((kind, accepted));
        }
        if auto_show_after(batch, auto_show)
            && let Some(windows) = &self.windows
        {
            // 划词触发的浮层跟随选区（代数对得上时），否则居中；屏幕
            // 边缘钳制后显示。
            let centered = centered_position(event_loop, windows);
            let position =
                show_position(self.selection_anchor, self.machine.generation(), centered);
            let position = windows.clamp_position(position);
            self.show_overlay(position);
        }
        self.request_redraw();
    }

    /// 采纳取材产物：组装 Task 携令牌下发通道③，进入 Translating。
    /// 返回是否进入了需要展示浮层的新任务。
    pub(super) fn accept_input(&mut self, generation: u64, input: TaskInput) -> bool {
        match self.machine.accept_input(generation, input) {
            Some(request) => {
                info!(
                    thread = thread::UI,
                    generation = request.generation,
                    "input ready, task dispatched to tokio"
                );
                self.send_run(request);
                true
            }
            None => {
                debug!(
                    thread = thread::UI,
                    generation = generation,
                    current = self.machine.generation(),
                    state = ?self.machine.state(),
                    "stale or unexpected input ready dropped"
                );
                false
            }
        }
    }

    /// 通道③发送；接收端不可用（tokio 桥死亡）时状态机降级落 Error。
    pub(super) fn send_run(&mut self, request: RunRequest) {
        let Some(endpoints) = &self.endpoints else {
            self.machine.fail_transport(request.generation);
            return;
        };
        if endpoints
            .commands
            .send(Command::RunTask {
                generation: request.generation,
                task: request.task,
                cancel: request.cancel,
            })
            .is_err()
        {
            warn!(
                thread = thread::UI,
                generation = request.generation,
                "command channel closed, task dropped"
            );
            self.machine.fail_transport(request.generation);
        }
    }

    /// 采纳流式增量。返回是否有新内容需要重绘。
    fn accept_chunk(&mut self, generation: u64, delta: String) -> bool {
        let accepted = self.machine.accept_chunk(generation, delta);
        if !accepted {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.machine.generation(),
                state = ?self.machine.state(),
                "superseded or hidden chunk dropped"
            );
        }
        accepted
    }

    /// 采纳任务产物：定格正文并进入 `Show`。返回是否需要重绘。
    pub(super) fn accept_done(
        &mut self,
        generation: u64,
        outcome: gloss_core::task::TaskOutcome,
    ) -> bool {
        let accepted = self.machine.accept_done(generation, outcome);
        if accepted {
            info!(
                thread = thread::UI,
                generation = generation,
                "task done, showing outcome"
            );
        } else {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.machine.generation(),
                state = ?self.machine.state(),
                "superseded or hidden task done dropped"
            );
        }
        accepted
    }

    /// 采纳任务失败：落 `Error` 态并展示失败卡（文案与动作出口由
    /// machine 按错误类别给出，见 `machine::error_action`）。
    /// 返回是否需要展示浮层。
    pub(super) fn accept_failed(
        &mut self,
        generation: u64,
        error: &gloss_core::model::GlossError,
    ) -> bool {
        let accepted = self.machine.accept_failed(generation, error);
        if accepted {
            warn!(
                thread = thread::UI,
                generation = generation,
                error = %error,
                "task failed"
            );
        } else {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.machine.generation(),
                "stale task failed dropped"
            );
        }
        accepted
    }
}

#[cfg(test)]
mod tests {
    use gloss_core::config::{Config, DEFAULT_TEXT_MODEL, ModelBinding};
    use gloss_core::model::Lang;
    use gloss_core::task::TaskKind;

    use crate::app::test_support::{
        driven_app, outcome_body, plain_outcome, streaming_body, text_input, trigger_selection,
    };
    use crate::channel::{AcquireCommand, Command};
    use crate::machine::AppState;

    #[test]
    fn late_events_of_superseded_trigger_do_not_bleed() {
        let (mut app, _config, _store, pe_tx, ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        trigger_selection(&mut app, &pe_tx);
        assert_eq!(app.machine.state(), AppState::Fetching);
        assert_eq!(app.machine.generation(), 1);
        assert!(matches!(
            ac_rx.try_recv().unwrap(),
            AcquireCommand::AcquireText { generation: 1, .. }
        ));

        assert!(app.accept_input(1, text_input("A")));
        assert_eq!(app.machine.state(), AppState::Translating);
        let Command::RunTask {
            generation: 1,
            cancel: token_a,
            ..
        } = cmd_rx.try_recv().unwrap()
        else {
            panic!("run task expected");
        };
        assert!(!token_a.is_cancelled());

        assert!(app.accept_chunk(1, "部分A".into()));
        assert!(streaming_body(&app).contains("部分A"));

        trigger_selection(&mut app, &pe_tx);
        assert_eq!(app.machine.generation(), 2);
        assert_eq!(app.machine.state(), AppState::Fetching);
        assert!(token_a.is_cancelled(), "new trigger must cancel task A");

        assert!(!app.accept_chunk(1, "迟到A".into()));
        assert!(
            !streaming_body(&app).contains("迟到A"),
            "late chunk of A must not bleed into the overlay"
        );

        assert!(app.accept_input(2, text_input("B")));
        assert!(matches!(
            cmd_rx.try_recv().unwrap(),
            Command::RunTask { generation: 2, .. }
        ));
        assert!(!app.accept_done(1, plain_outcome("迟到结果A")));
        assert!(app.accept_done(2, plain_outcome("结果B")));
        assert_eq!(app.machine.state(), AppState::Show);
        assert_eq!(outcome_body(&app), "结果B");
    }

    #[test]
    fn failed_task_lands_in_error_and_retry_works() {
        let (mut app, _config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));

        assert!(!app.accept_failed(0, &gloss_core::model::GlossError::EngineNetwork));
        assert_eq!(app.machine.state(), AppState::Translating);

        assert!(app.accept_failed(1, &gloss_core::model::GlossError::EngineNetwork));
        assert_eq!(app.machine.state(), AppState::Error);
        assert!(app.machine.current_cancel().is_none());

        trigger_selection(&mut app, &pe_tx);
        assert_eq!(app.machine.state(), AppState::Fetching);
    }

    #[test]
    fn stale_input_ready_is_dropped_entirely() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(!app.accept_input(42, text_input("来自未来")));
        assert_eq!(
            app.machine.state(),
            AppState::Fetching,
            "stale input must not move state"
        );
        assert!(
            cmd_rx.try_recv().is_err(),
            "stale input must not reach tokio"
        );
    }

    #[test]
    fn saved_config_applies_to_the_next_trigger() {
        let (mut app, config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        let Command::RunTask { task, .. } = cmd_rx.try_recv().unwrap();
        assert_eq!(task.options.target_lang, Some(Lang::Zh));
        assert_eq!(
            task.options.model_override.as_deref(),
            Some(DEFAULT_TEXT_MODEL)
        );

        config
            .save(Config {
                target_lang: Lang::Ja,
                model_by_kind: vec![ModelBinding {
                    kind: TaskKind::TranslateWord,
                    model: "deepseek-reasoner".into(),
                }],
                ..Default::default()
            })
            .expect("save should succeed");

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(2, text_input("B")));
        let Command::RunTask { task, .. } = cmd_rx.try_recv().unwrap();
        assert_eq!(task.options.target_lang, Some(Lang::Ja));
        assert_eq!(
            task.options.model_override.as_deref(),
            Some("deepseek-reasoner")
        );
    }

    #[test]
    fn saved_config_does_not_leak_into_the_inflight_task() {
        let (mut app, config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        trigger_selection(&mut app, &pe_tx);
        config
            .save(Config {
                target_lang: Lang::Ja,
                ..Default::default()
            })
            .expect("save should succeed");
        assert!(app.accept_input(1, text_input("A")));

        let Command::RunTask { task, .. } = cmd_rx.try_recv().unwrap();
        assert_eq!(
            task.options.target_lang,
            Some(Lang::Zh),
            "in-flight task must keep the snapshot taken at trigger"
        );

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(2, text_input("B")));
        let Command::RunTask { task, .. } = cmd_rx.try_recv().unwrap();
        assert_eq!(task.options.target_lang, Some(Lang::Ja));
    }
}
