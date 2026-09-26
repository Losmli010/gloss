//! 通道①③④的消费与下发：平台事件→取材命令（通道②经 machine 产出）、
//! 取材产物→推理任务（通道③）、回传事件→浮层展示决策（通道④）。

use gloss_core::log::{Span, debug, info, task_span, thread, warn};
use gloss_core::task::TaskInput;
use winit::event_loop::ActiveEventLoop;

use crate::channel::{AcquireCommand, Command, Event, PlatformEvent, Traced};
use crate::machine::{InputOutcome, RunRequest, TriggerDecision, trigger_decision};

use super::GlossApp;
use super::overlay::{auto_show_after, centered_position, event_kind, show_position};

impl GlossApp {
    /// 消费通道①：平台事件 → 取材命令。只有真实下发的命令才占用新代数
    /// （未接线事件不作废在途回传）；触发→命令的日志链路同时承担热键端到
    /// 端的验收验证（CI 无法合成真实按键，只能真机按日志走查）。
    ///
    /// 触发前读一次场景事实（安全输入态、前台应用）：闸门拦下的触发与
    /// 未接线事件一样不进状态机，但记 warn——用户会想知道「为什么划了没
    /// 反应」，而这是他能自己修的（换一个应用，或取消密码框的聚焦）。
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
            // 逐事件现读场景事实：安全输入态与前台应用都可能在两条触发
            // 之间变化，探针也就两次纯查询。
            let scene = self.scene.facts();
            if let Some(command) = self
                .machine
                .trigger(&event, &config, self.system_locale, &scene)
            {
                let span = task_span(self.machine.generation());
                self.task_span = Some((self.machine.generation(), span.clone()));
                let entered_span = span.clone();
                let _entered = entered_span.enter();
                info!(
                    thread = thread::UI,
                    cancelled_inflight = superseded,
                    "platform event dispatched as acquire command"
                );
                // 划词触发记录释放坐标（随代数）：浮层显示时跟随选区；
                // 其它触发源（热键）不带坐标，显示决策回落居中。
                if let PlatformEvent::SelectionGesture { pos } = event {
                    self.selection_anchor = Some((self.machine.generation(), pos));
                }
                self.send_acquire(command, span);
            } else {
                // 三类拦下各有各的级别与措辞：被任务开关停用的触发是用户
                // 能自己修的配置问题；被场景闸门拦下的是「这一次的场景不
                // 合适」（换应用或取消聚焦密码框即可，也可能是防护开关）；
                // 未接线的事件只留在默认级别看不见的 debug 里。
                match trigger_decision(&event, &config, &scene) {
                    TriggerDecision::Disabled(kind) => warn!(
                        thread = thread::UI,
                        kind = ?kind,
                        "trigger ignored: the task kind is disabled in settings"
                    ),
                    TriggerDecision::Blocked(block) => warn!(
                        thread = thread::UI,
                        reason = %block,
                        "trigger suppressed by the sensitive content guard"
                    ),
                    _ => debug!(
                        thread = thread::UI,
                        event = ?event,
                        "platform event ignored: not wired yet"
                    ),
                }
            }
        }
    }

    /// 通道②发送；接收端消失（事件线程死亡/退出）时落 Error 态兜底，
    /// 避免滞留 Fetching。
    fn send_acquire(&mut self, command: AcquireCommand, span: Span) {
        let Some(endpoints) = &self.endpoints else {
            return;
        };
        let traced = Traced {
            payload: command,
            span,
        };
        if endpoints.acquire_commands.send(traced).is_err() {
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
    /// 浮层「什么时候自动露面」抽在 [`super::overlay::auto_show_for`] /
    /// [`super::overlay::auto_show_after`] 这两个纯函数里：取材成功与
    /// 失败即弹，其余类别不负责露面。
    pub(super) fn drain_events(&mut self, event_loop: &ActiveEventLoop) {
        let events: Vec<Event> = self
            .endpoints
            .as_ref()
            .map_or(Vec::new(), |e| e.events.try_iter().collect());
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
                } => self.accept_done(generation, outcome),
                Event::TaskFailed { generation, error } => self.accept_failed(generation, &error),
            };
            batch.push((kind, accepted));
        }
        if auto_show_after(batch)
            && let Some(windows) = &mut self.windows
        {
            // 划词触发的浮层跟随选区（代数对得上时），否则居中；屏幕
            // 边缘钳制后显示，并按摆放意图记录——后续内容撑高窗口时
            // 才能按同一意图重定位。
            let centered = centered_position(event_loop, windows);
            let (position, placement) =
                show_position(self.selection_anchor, self.machine.generation(), centered);
            windows.set_placement(placement);
            let position = windows.clamp_position(position);
            self.show_overlay(position);
        }
        self.request_redraw();
    }

    /// 采纳取材产物：组装 Task 携令牌下发通道③，进入 Translating；内容
    /// 闸门命中时这次取材作废——不下发、不出浮层，只记一行 warn 并收起浮层
    /// （上一次的结果已经与新选区无关）。返回是否进入了需要展示浮层的新任务。
    pub(super) fn accept_input(&mut self, generation: u64, input: TaskInput) -> bool {
        match self.machine.accept_input(generation, input) {
            InputOutcome::Dispatch(request) => {
                info!(
                    thread = thread::UI,
                    generation = request.generation,
                    kind = ?request.task.kind,
                    target_lang = ?request.task.options.target_lang,
                    prompt_locale = ?request.task.options.prompt_locale,
                    "input ready, task dispatched to tokio"
                );
                self.send_run(request);
                true
            }
            InputOutcome::Blocked(reason) => {
                warn!(
                    thread = thread::UI,
                    generation = generation,
                    reason = ?reason,
                    "input suppressed by the sensitive content guard, task not dispatched"
                );
                // 状态机那边已回 Idle 并清空视图，这里做的是窗口那半边：
                // 隐藏浮层、清渲染截止时刻。浮层里可能还挂着上一次的结果，
                // 它属于另一次取材，留着会被读成「这次划词的结果」。
                self.dismiss_overlay("sensitive content blocked");
                false
            }
            InputOutcome::Ignored => {
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
        let traced = Traced {
            payload: Command::RunTask {
                generation: request.generation,
                task: request.task,
                cancel: request.cancel,
            },
            span: self.span_for(request.generation).unwrap_or_else(Span::none),
        };
        if endpoints.commands.send(traced).is_err() {
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
    use std::sync::Arc;

    use gloss_core::config::{Config, DEFAULT_TEXT_MODEL, ModelBinding};
    use gloss_core::guard::{FrontApp, SceneFacts};
    use gloss_core::log::{capture_global, info, thread};
    use gloss_core::model::Lang;
    use gloss_core::ports::SceneProbe;
    use gloss_core::task::TaskKind;

    use crate::app::test_support::{
        driven_app, driven_app_with_scene, outcome_body, plain_outcome, streaming_body, text_input,
        trigger_selection,
    };
    use crate::channel::{AcquireCommand, Command};
    use crate::machine::AppState;
    use crate::stubs::ports::{MemoryConfigStore, RecordingHotkeyBinder, StubSceneProbe};

    fn suspicious_text() -> String {
        format!("key sk-{}", "9f2b7c1d")
    }

    #[test]
    fn dispatched_acquire_carries_the_task_span() {
        let logs = capture_global();
        let (mut app, _config, _store, pe_tx, ac_rx, _cmd_rx, _ev_tx) = driven_app();

        trigger_selection(&mut app, &pe_tx);
        let job = ac_rx.try_recv().expect("acquire command dispatched");
        let _entered = job.span.enter();
        info!(thread = thread::EVENT, "probe");

        let text = logs.text();
        let probe = text
            .lines()
            .find(|line| line.contains("probe"))
            .unwrap_or_default();
        assert!(probe.contains("\"generation\":1"), "{text}");
    }

    #[test]
    fn a_disabled_default_kind_makes_the_selection_gesture_a_no_op() {
        let (mut app, config, _store, pe_tx, ac_rx, _cmd_rx, _ev_tx) = driven_app();
        config
            .save(Config {
                default_text_kind: TaskKind::ExplainCode,
                enabled_kinds: vec![TaskKind::TranslateWord],
                ..Default::default()
            })
            .expect("save should succeed");

        trigger_selection(&mut app, &pe_tx);
        assert!(
            ac_rx.try_recv().is_err(),
            "the gate must stop the command before it reaches the event thread"
        );
        assert_eq!(
            app.machine.generation(),
            0,
            "a dropped trigger keeps no gen"
        );
        assert_eq!(
            app.machine.state(),
            AppState::Idle,
            "no overlay, no failure card: this is the state the settings page now blocks"
        );

        config.save(Config::default()).expect("save should succeed");
        trigger_selection(&mut app, &pe_tx);
        assert!(matches!(
            ac_rx.try_recv().unwrap().payload,
            AcquireCommand::AcquireText {
                generation: 1,
                kind: TaskKind::TranslateWord
            }
        ));
    }

    #[test]
    fn late_events_of_superseded_trigger_do_not_bleed() {
        let (mut app, _config, _store, pe_tx, ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        trigger_selection(&mut app, &pe_tx);
        assert_eq!(app.machine.state(), AppState::Fetching);
        assert_eq!(app.machine.generation(), 1);
        assert!(matches!(
            ac_rx.try_recv().unwrap().payload,
            AcquireCommand::AcquireText { generation: 1, .. }
        ));

        assert!(app.accept_input(1, text_input("A")));
        assert_eq!(app.machine.state(), AppState::Translating);
        let Command::RunTask {
            generation: 1,
            cancel: token_a,
            ..
        } = cmd_rx.try_recv().unwrap().payload
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
            cmd_rx.try_recv().unwrap().payload,
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
        let Command::RunTask { task, .. } = cmd_rx.try_recv().unwrap().payload;
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
        let Command::RunTask { task, .. } = cmd_rx.try_recv().unwrap().payload;
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

        let Command::RunTask { task, .. } = cmd_rx.try_recv().unwrap().payload;
        assert_eq!(
            task.options.target_lang,
            Some(Lang::Zh),
            "in-flight task must keep the snapshot taken at trigger"
        );

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(2, text_input("B")));
        let Command::RunTask { task, .. } = cmd_rx.try_recv().unwrap().payload;
        assert_eq!(task.options.target_lang, Some(Lang::Ja));
    }

    #[test]
    fn a_sensitive_scene_makes_the_selection_gesture_a_no_op() {
        let scene = Arc::new(StubSceneProbe::default());
        let (mut app, _config, _store, pe_tx, ac_rx, _cmd_rx, _ev_tx) = driven_app_with_scene(
            Arc::new(MemoryConfigStore::default()),
            Arc::new(RecordingHotkeyBinder::default()),
            Arc::clone(&scene) as Arc<dyn SceneProbe>,
        );

        scene.set_facts(SceneFacts {
            secure_input: true,
            front_app: None,
        });
        trigger_selection(&mut app, &pe_tx);
        assert!(
            ac_rx.try_recv().is_err(),
            "a focused password field must stop the command before the event thread"
        );

        scene.set_facts(SceneFacts {
            secure_input: false,
            front_app: Some(FrontApp {
                bundle_id: Some("com.1password.1password".into()),
                name: None,
            }),
        });
        trigger_selection(&mut app, &pe_tx);
        assert!(
            ac_rx.try_recv().is_err(),
            "a listed frontmost app must stop the command too"
        );
        assert_eq!(
            app.machine.generation(),
            0,
            "a suppressed trigger keeps no generation"
        );
        assert_eq!(app.machine.state(), AppState::Idle);

        scene.set_facts(SceneFacts::default());
        trigger_selection(&mut app, &pe_tx);
        assert!(matches!(
            ac_rx.try_recv().unwrap().payload,
            AcquireCommand::AcquireText { generation: 1, .. }
        ));
    }

    #[test]
    fn suspicious_input_is_suppressed_and_shows_nothing() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);

        assert!(
            !app.accept_input(1, text_input(&suspicious_text())),
            "a refused fetch must not ask for the overlay"
        );
        assert!(
            cmd_rx.try_recv().is_err(),
            "nothing may reach tokio — there is no confirmation path to wait for"
        );
        assert_eq!(app.machine.state(), AppState::Idle);
        assert!(
            app.machine.overlay_view().is_none(),
            "no card, no question: the guard is not a dialog"
        );

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(2, text_input("普通的文本")));
        assert!(matches!(
            cmd_rx.try_recv().unwrap().payload,
            Command::RunTask { generation: 2, .. }
        ));
    }
}
