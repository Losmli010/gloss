//! 编排用例·任务段：消费通道④（取材产物与任务回传）→ 状态机转移 →
//! 通道③下发 → 批后露面决策与重绘。显形「要不要」的策略在
//! `machine::should_reveal`，「怎么显示」在 `reveal`。

use gloss_core::log::{Span, debug, info, thread, warn};
use gloss_core::model::GlossError;
use gloss_core::task::TaskInput;
use winit::event_loop::ActiveEventLoop;

use crate::channel::{Command, Event, Traced};
use crate::machine::{FailureOutcome, InputOutcome, RunRequest, event_kind, should_reveal};

use super::reveal::{centered_position, show_position};
use crate::app::GlossApp;

impl GlossApp {
    /// 消费通道④：回传事件按代数/探测编号采纳——连续快速触发时旧代的
    /// 产物被丢弃，浮层只显示最后一次请求的结果。状态决策在 machine，壳
    /// 只做通道发送、浮层展示与日志。
    ///
    /// 浮层「什么时候露面」抽在 [`crate::machine::should_reveal`]（内含
    /// [`crate::machine::auto_show_after`] 的按批判定）：挂起显形请求由
    /// 划词提交（内容到达才显形）置位，批处理后在同一帧消费；失败即弹
    /// （权限卡等）走批次判定。
    pub(crate) fn drain_events(&mut self, event_loop: &ActiveEventLoop) {
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
                // 预热回执与任务代数无关（不占代数，与浮层无关）：旁路
                // 处理后不进露面批次。
                Event::SecretPrewarmed { result } => {
                    self.on_secret_prewarmed(&result);
                    continue;
                }
                Event::InputReady { generation, input } => self.accept_input(generation, input),
                Event::TaskClassified { generation, kind } => {
                    self.accept_classified(generation, kind)
                }
                Event::TaskChunk { generation, delta } => self.accept_chunk(generation, delta),
                Event::TaskDone {
                    generation,
                    outcome,
                } => self.accept_done(generation, outcome),
                Event::TaskFailed { generation, error } => self.accept_failed(generation, &error),
            };
            batch.push((kind, accepted));
        }
        // 挂起显形在批处理**之后**消费：划词提交的置位点就在本批的
        // commit_probe 里——取前会把它拖到下一帧，取后同帧即显。
        let pending_reveal = std::mem::take(&mut self.session.pending_reveal);
        // 显形决策（守卫与「显形或失败即弹」的取舍）在 should_reveal：
        // 这里只递交挂起请求、机器当前视图与本批回传。
        if should_reveal(pending_reveal, self.machine.overlay_view().is_some(), batch)
            && let Some(windows) = &mut self.workspace.windows
        {
            // 划词触发的浮层跟随选区（代数对得上时），否则居中；屏幕
            // 边缘钳制后显示，并按摆放意图记录——后续内容撑高窗口时
            // 才能按同一意图重定位。
            let centered = centered_position(event_loop, windows);
            let (position, placement) = show_position(
                self.session.selection_anchor,
                self.machine.generation(),
                centered,
            );
            windows.set_placement(placement);
            let position = windows.clamp_position(position);
            self.show_overlay(position);
        }
        self.request_redraw();
    }

    /// 采纳取材产物：划词探测命中走提交段（内容到达才显形），其余按
    /// 陈旧产物丢弃（收起后的取材、被顶掉的旧探测）。返回是否进入了
    /// 需要展示浮层的新任务。
    pub(crate) fn accept_input(&mut self, generation: u64, input: TaskInput) -> bool {
        // 探测编号在提交时才提升为代数，陈旧过滤按编号而不是代数。
        if self.machine.probe_id() == Some(generation) {
            return self.commit_probe(generation, input);
        }
        debug!(
            thread = thread::UI,
            generation = generation,
            current = self.machine.generation(),
            "stale or unexpected input ready dropped"
        );
        false
    }

    /// 提交划词探测（取材产物到达）：状态机接管（旧会话让位、代数提升、
    /// 视图整卡换流式卡），置挂起显形——出窗与重定位由 drain_events 在
    /// 同帧统一执行（那里才有 ActiveEventLoop）。内容闸门命中时探测作废
    /// 但**当前显示保留**（它属于上一个会话），只记一行 warn。
    fn commit_probe(&mut self, generation: u64, input: TaskInput) -> bool {
        match self.machine.commit_selection(generation, input) {
            InputOutcome::Dispatch(request) => {
                info!(
                    thread = thread::UI,
                    generation = request.generation,
                    model = %request.options.model,
                    target_lang = ?request.options.target_lang,
                    prompt_locale = ?request.options.prompt_locale,
                    "selection committed, task dispatched to tokio"
                );
                self.session.probe_front_app = None;
                self.send_run(request);
                // 内容到达才显形：从 Idle 出窗、从已显示改锚点重定位，
                // 误滑（探测失败）永远走不到这里。
                self.session.pending_reveal = true;
                true
            }
            InputOutcome::Blocked(reason) => {
                warn!(
                    thread = thread::UI,
                    generation = generation,
                    reason = ?reason,
                    "probed selection suppressed by the sensitive content guard, task not dispatched"
                );
                self.session.probe_front_app = None;
                false
            }
            InputOutcome::Ignored => {
                debug!(
                    thread = thread::UI,
                    generation = generation,
                    current = self.machine.generation(),
                    "stale selection probe result dropped"
                );
                false
            }
        }
    }

    /// 通道③发送；接收端不可用（tokio 桥死亡）时状态机降级落 Error。
    pub(crate) fn send_run(&mut self, request: RunRequest) {
        let Some(endpoints) = &self.endpoints else {
            self.machine.fail_transport(request.generation);
            return;
        };
        let traced = Traced {
            payload: Command::RunTask {
                generation: request.generation,
                input: request.input,
                options: request.options,
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

    /// 采纳自动分类结果（头部任务标签的更新点）。返回是否需要重绘。
    fn accept_classified(&mut self, generation: u64, kind: gloss_core::task::TaskKind) -> bool {
        let accepted = self.machine.accept_classified(generation, kind);
        if !accepted {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.machine.generation(),
                state = ?self.machine.state(),
                "stale classification dropped"
            );
        }
        accepted
    }

    /// 采纳任务产物：定格正文并进入 `Show`。返回是否需要重绘。
    pub(crate) fn accept_done(
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

    /// 采纳任务失败：划词探测命中走探测失败处置（误滑静默丢弃 / 权限卡
    /// 显式反馈），其余按推理失败处置（落 `Error` 态弹失败卡，文案与动作
    /// 出口由 machine 按错误类别给出，见 `machine::error_action`）。返回
    /// 是否需要展示浮层。
    pub(crate) fn accept_failed(
        &mut self,
        generation: u64,
        error: &gloss_core::model::GlossError,
    ) -> bool {
        if self.machine.probe_id() == Some(generation) {
            return self.fail_probe(generation, error);
        }
        match self.machine.accept_failed(generation, error) {
            FailureOutcome::Shown => {
                warn!(
                    thread = thread::UI,
                    generation = generation,
                    error = %error,
                    "task failed"
                );
                true
            }
            FailureOutcome::SilentlyDropped => {
                // 不可达：静默丢弃只发生在划词探测段（上方已分流）。
                // 防御性兜底——照陈旧语义丢弃，不碰窗口。
                debug!(
                    thread = thread::UI,
                    generation = generation,
                    error = %error,
                    "silent failure dropped outside the probe path"
                );
                false
            }
            FailureOutcome::Ignored => {
                // 陈旧失败的丢弃是竞速语义（新划词覆盖旧划词），但这条线正是
                // 「划了没弹窗」的排查盲区——升为 info 保证默认日志可见。
                info!(
                    thread = thread::UI,
                    generation = generation,
                    current = self.machine.generation(),
                    "stale task failed dropped, superseded by a newer gesture"
                );
                false
            }
        }
    }

    /// 划词探测失败：空选区/读不到按误滑静默丢弃——不留窗口动作、不留
    /// 弹窗（若浮层正在显示，当前内容原样保留），只留一条带前台应用的
    /// 排查痕迹，那是「划了没反应」的唯一日志线索；其余失败（权限缺失
    /// 等）落失败卡，经「失败即弹」显形（从 Idle 出窗或顶替已显示内容
    /// ——真实故障不该被吞掉）。
    fn fail_probe(&mut self, generation: u64, error: &gloss_core::model::GlossError) -> bool {
        match self.machine.commit_selection_failed(generation, error) {
            FailureOutcome::SilentlyDropped => {
                let front_app = self
                    .session
                    .probe_front_app
                    .as_ref()
                    .map(|app| {
                        app.bundle_id
                            .as_deref()
                            .or(app.name.as_deref())
                            .unwrap_or("unknown")
                    })
                    .unwrap_or("unknown");
                info!(
                    thread = thread::UI,
                    generation = generation,
                    error = %error,
                    front_app,
                    "selection probe found nothing, treated as a mis-slide"
                );
                self.session.probe_front_app = None;
                false
            }
            FailureOutcome::Shown => {
                warn!(
                    thread = thread::UI,
                    generation = generation,
                    error = %error,
                    "selection probe failed"
                );
                self.session.probe_front_app = None;
                true
            }
            FailureOutcome::Ignored => {
                debug!(
                    thread = thread::UI,
                    generation = generation,
                    current = self.machine.generation(),
                    "stale selection probe failure dropped"
                );
                false
            }
        }
    }
}

impl GlossApp {
    /// 发密钥预热命令（通道③，不带任务 span——预热不属于任何任务）：
    /// 启动授权引导（系统级弹窗）之后即发，macOS 的 keychain 授权框由此
    /// 前置到启动期受控出现，读到的值进存储的进程内缓存，首次划词不再弹。
    /// 发送失败（通道已关，应用正在退出）只留痕。
    pub(crate) fn send_secret_prewarm(&mut self) {
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

    /// 密钥预热回执（`SecretPrewarmed`）：只留痕——预热失败不拦主流程，
    /// 首次任务会自然重读并按既有失败路径兜底。
    pub(crate) fn on_secret_prewarmed(&mut self, result: &Result<(), GlossError>) {
        if let Err(err) = result {
            debug!(thread = thread::UI, error = %err, "secret prewarm reported failure");
        }
    }
}

#[cfg(test)]
mod tests {
    use gloss_core::config::{Config, DEFAULT_TEXT_MODEL};
    use gloss_core::model::{GlossError, Lang};

    use crate::app::test_support::{
        driven_app, outcome_note, plain_outcome, streaming_raw, text_input, trigger_selection,
    };
    use crate::channel::{AcquireCommand, Command};
    use crate::machine::AppState;

    #[test]
    fn prewarm_dispatches_the_configured_keychain_entry_exactly_once() {
        let (mut app, _config, _store, _pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        app.send_secret_prewarm();
        app.send_secret_prewarm();

        let first = cmd_rx
            .try_recv()
            .expect("the first prewarm lands on channel 3");
        assert!(
            matches!(
                first.payload,
                Command::PrewarmSecret { ref keychain_id }
                    if *keychain_id == app.env.config.snapshot().resolved_provider().keychain_id
            ),
            "the entry id must come from the current snapshot, got {:?}",
            first.payload
        );
        let second = cmd_rx.try_recv().expect("each call sends its own command");
        assert!(matches!(second.payload, Command::PrewarmSecret { .. }));
        assert!(
            cmd_rx.try_recv().is_err(),
            "two calls send exactly two commands"
        );
    }

    #[test]
    fn prewarm_without_endpoints_is_a_silent_no_op() {
        let (mut app, _config, _store, _pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        app.endpoints = None;

        app.send_secret_prewarm();
        assert!(
            cmd_rx.try_recv().is_err(),
            "no endpoints means nothing is sent and nothing panics"
        );
    }

    #[test]
    fn late_events_of_superseded_trigger_do_not_bleed() {
        let (mut app, _config, _store, pe_tx, ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        trigger_selection(&mut app, &pe_tx);
        assert_eq!(
            app.machine.state(),
            AppState::Idle,
            "the probe leaves the state machine alone until content arrives"
        );
        assert_eq!(app.machine.generation(), 0);
        assert!(matches!(
            ac_rx.try_recv().unwrap().payload,
            AcquireCommand::AcquireText { generation: 1, .. }
        ));

        assert!(app.accept_input(1, text_input("A")));
        assert_eq!(app.machine.generation(), 1, "commit promotes the probe id");
        assert_eq!(app.machine.state(), AppState::Translating);
        let Command::RunTask {
            generation: 1,
            cancel: token_a,
            ..
        } = cmd_rx.try_recv().unwrap().payload
        else {
            panic!("expected a RunTask command, got another variant")
        };
        assert!(!token_a.is_cancelled());

        assert!(app.accept_chunk(1, "部分A".into()));
        assert!(streaming_raw(&app).contains("部分A"));

        trigger_selection(&mut app, &pe_tx);
        assert_eq!(
            app.machine.generation(),
            1,
            "the second gesture probes without superseding anything"
        );
        assert_eq!(
            app.machine.state(),
            AppState::Translating,
            "the visible session keeps running while the probe is out"
        );
        assert!(!token_a.is_cancelled(), "a probe must not cancel task A");
        assert!(
            app.accept_chunk(1, "续A".into()),
            "A's stream keeps flowing during the probe"
        );

        assert!(app.accept_input(2, text_input("B")));
        assert!(token_a.is_cancelled(), "the commit supersedes task A");
        assert!(matches!(
            cmd_rx.try_recv().unwrap().payload,
            Command::RunTask { generation: 2, .. }
        ));
        assert!(!app.accept_chunk(1, "迟到A".into()));
        assert!(!app.accept_done(1, plain_outcome("迟到结果A")));
        assert!(app.accept_done(2, plain_outcome("结果B")));
        assert_eq!(app.machine.state(), AppState::Show);
        assert_eq!(outcome_note(&app), "结果B");
    }

    #[test]
    fn failed_task_lands_in_error_and_retry_works() {
        let (mut app, _config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));

        assert!(!app.accept_failed(0, &GlossError::EngineNetwork));
        assert_eq!(app.machine.state(), AppState::Translating);

        assert!(app.accept_failed(1, &GlossError::EngineNetwork));
        assert_eq!(app.machine.state(), AppState::Error);
        assert!(app.machine.current_cancel().is_none());

        trigger_selection(&mut app, &pe_tx);
        assert_eq!(
            app.machine.state(),
            AppState::Error,
            "a probe never disturbs the visible failure card"
        );
        assert_eq!(app.machine.probe_id(), Some(2));
    }

    #[test]
    fn stale_input_ready_is_dropped_entirely() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(!app.accept_input(42, text_input("来自未来")));
        assert_eq!(
            app.machine.state(),
            AppState::Idle,
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
        let Command::RunTask { options, .. } = cmd_rx.try_recv().unwrap().payload else {
            panic!("expected a RunTask command, got another variant")
        };
        assert_eq!(options.target_lang, Some(Lang::Zh));
        assert_eq!(
            options.model, DEFAULT_TEXT_MODEL,
            "the factory model freezes into the first task"
        );

        config
            .save(Config {
                target_lang: Lang::Ja,
                model: "deepseek-reasoner".into(),
                ..Default::default()
            })
            .expect("save should succeed");

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(2, text_input("B")));
        let Command::RunTask { options, .. } = cmd_rx.try_recv().unwrap().payload else {
            panic!("expected a RunTask command, got another variant")
        };
        assert_eq!(options.target_lang, Some(Lang::Ja));
        assert_eq!(
            options.model, "deepseek-reasoner",
            "the saved model freezes into the next task"
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

        let Command::RunTask { options, .. } = cmd_rx.try_recv().unwrap().payload else {
            panic!("expected a RunTask command, got another variant")
        };
        assert_eq!(
            options.target_lang,
            Some(Lang::Zh),
            "in-flight task must keep the snapshot taken at trigger"
        );

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(2, text_input("B")));
        let Command::RunTask { options, .. } = cmd_rx.try_recv().unwrap().payload else {
            panic!("expected a RunTask command, got another variant")
        };
        assert_eq!(options.target_lang, Some(Lang::Ja));
    }

    #[test]
    fn probe_empty_selection_is_silently_dropped() {
        let (mut app, _config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert_eq!(
            app.machine.state(),
            AppState::Idle,
            "the probe never enters the state machine"
        );

        assert!(
            !app.accept_failed(1, &GlossError::SelectionUnavailable),
            "a pure mis-drag must not pop the overlay for a failure card"
        );
        assert_eq!(app.machine.state(), AppState::Idle);
        assert!(
            app.machine.overlay_view().is_none(),
            "nothing was ever shown, nothing needs withdrawing"
        );
        assert!(app.machine.probe_id().is_none(), "the probe is consumed");
    }

    #[test]
    fn a_mis_slide_over_a_visible_session_preserves_it_entirely() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("正常选区")));
        let Command::RunTask { cancel: token, .. } = cmd_rx.try_recv().unwrap().payload else {
            panic!("expected a RunTask command, got another variant")
        };
        assert!(
            app.session.pending_reveal,
            "the commit flagged the reveal for drain_events"
        );
        app.session.pending_reveal = false;
        assert!(app.accept_chunk(1, "流式正文".into()));
        let view_before = app.machine.overlay_view().cloned();
        let state_before = app.machine.state();

        trigger_selection(&mut app, &pe_tx);
        assert!(
            !token.is_cancelled(),
            "the probe must not cancel the stream"
        );
        assert!(
            !app.accept_failed(2, &GlossError::SelectionUnavailable),
            "a mis-slide over a visible session shows nothing new"
        );
        assert_eq!(app.machine.state(), state_before);
        assert_eq!(app.machine.overlay_view(), view_before.as_ref());
        assert!(!app.session.pending_reveal, "nothing new to reveal");
        assert!(app.machine.probe_id().is_none());

        assert!(app.accept_chunk(1, "、继续".into()));
        assert!(streaming_raw(&app).contains("继续"));
    }

    #[test]
    fn committing_the_probe_flags_the_reveal_for_the_same_frame() {
        let (mut app, _config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(!app.session.pending_reveal, "probing never reveals");

        assert!(app.accept_input(1, text_input("内容到了")));
        assert!(
            app.session.pending_reveal,
            "the commit flags the reveal; drain_events consumes it the same frame"
        );
        assert_eq!(app.machine.state(), AppState::Translating);
    }

    #[test]
    fn suspicious_input_is_suppressed_and_shows_nothing() {
        let suspicious_text = format!("{{\"token\":\"sk-{}\"}}", "9f2b7c1d");
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);

        assert!(
            !app.accept_input(1, text_input(&suspicious_text)),
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
