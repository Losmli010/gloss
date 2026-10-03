//! 通道①③④的消费与下发：平台事件→取材命令（通道②经 machine 产出）、
//! 取材产物→推理任务（通道③）、回传事件→浮层展示决策（通道④）。

use gloss_core::log::{Span, debug, info, task_span, thread, warn};
use gloss_core::task::{TaskInput, TaskKind};
use winit::event_loop::ActiveEventLoop;

use crate::channel::{AcquireCommand, Command, Event, PlatformEvent, Traced};
use crate::machine::{FailureOutcome, InputOutcome, RunRequest, TriggerDecision, trigger_decision};

use super::GlossApp;
use super::overlay::{centered_position, event_kind, should_reveal, show_position};

impl GlossApp {
    /// 消费通道①：平台事件 → 取材命令。只有真实下发的命令才占用编号
    /// （未接线事件不作废在途回传）。划词手势走探测段
    /// （`begin_selection_probe`——状态机不动、不显形，产物到达才提交）。
    ///
    /// 触发前读一次场景事实（安全输入态、前台应用）：闸门拦下的触发与
    /// 未接线事件一样不进状态机，但记 warn——用户会想知道「为什么划了没
    /// 反应」，而这是他能自己修的（换一个应用，或取消密码框的聚焦）。
    /// 前台应用随划词探测留存（`probe_front_app`），探测失败的排查日志
    /// 带上它——那是「划了没反应」的唯一线索。
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
            // 设置入口：托盘与浮层失败卡共用同一条路；不占
            // 用代数（与未接线事件一样不进状态机）。
            if matches!(event, PlatformEvent::OpenSettingsRequested) {
                info!(thread = thread::UI, "settings open requested");
                self.open_settings();
                continue;
            }
            // 逐事件现读场景事实：安全输入态与前台应用都可能在两次触发
            // 之间变化，探针也就两次纯查询。
            let scene = self.scene.facts();
            let Some(command) = (match &event {
                PlatformEvent::SelectionGesture { .. } => {
                    self.machine
                        .begin_selection_probe(&event, &config, self.system_locale, &scene)
                }
                _ => None,
            }) else {
                // 两类拦下各有各的级别与措辞：被场景闸门拦下的是「这一次
                // 的场景不合适」（换一个应用，或取消密码框的聚焦）；自身
                // 前台是防误触的日常过滤（连拖 Gloss 自己的浮层），只留
                // debug；未接线的事件同样只留在默认级别看不见的 debug 里。
                match trigger_decision(&event, &scene) {
                    TriggerDecision::Blocked(block) => warn!(
                        thread = thread::UI,
                        reason = %block,
                        "trigger suppressed by the sensitive content guard"
                    ),
                    TriggerDecision::SelfSuppressed => debug!(
                        thread = thread::UI,
                        "selection gesture ignored: gloss itself is the frontmost app"
                    ),
                    _ => debug!(
                        thread = thread::UI,
                        event = ?event,
                        "platform event ignored: not wired yet"
                    ),
                }
                continue;
            };
            let AcquireCommand::AcquireText { generation, .. } = &command else {
                continue;
            };
            let generation = *generation;
            let span = task_span(generation);
            self.task_span = Some((generation, span.clone()));
            let entered_span = span.clone();
            let _entered = entered_span.enter();
            info!(
                thread = thread::UI,
                "selection probe dispatched as acquire command"
            );
            // 划词探测记录释放坐标与前台应用（随探测编号）：浮层显示时跟随
            // 选区；探测失败的排查日志带上应用标识。
            if let PlatformEvent::SelectionGesture { pos } = event {
                self.selection_anchor = Some((generation, pos));
                self.probe_front_app = scene.front_app;
            }
            if !self.send_acquire(command, span) {
                // 取材通道发送失败：作废探测即可（当前显示不动）。
                self.machine.drop_probe();
                self.probe_front_app = None;
            }
        }
    }

    /// 通道②发送；返回是否发出。接收端消失（事件线程死亡/退出）时由
    /// 调用方作废探测（当前显示不动）。
    fn send_acquire(&mut self, command: AcquireCommand, span: Span) -> bool {
        let AcquireCommand::AcquireText { generation, .. } = &command else {
            return false;
        };
        let generation = *generation;
        let Some(endpoints) = &self.endpoints else {
            return false;
        };
        let traced = Traced {
            payload: command,
            span,
        };
        if endpoints.acquire_commands.send(traced).is_err() {
            warn!(
                thread = thread::UI,
                generation, "acquire channel closed, command dropped"
            );
            return false;
        }
        true
    }

    /// 消费通道④：回传事件按代数/探测编号采纳——连续快速触发时旧代的
    /// 产物被丢弃，浮层只显示最后一次请求的结果。状态决策在 machine，壳
    /// 只做通道发送、浮层展示与日志。
    ///
    /// 浮层「什么时候露面」抽在 [`super::overlay::should_reveal`]（内含
    /// [`super::overlay::auto_show_after`] 的按批判定）：挂起显形请求由
    /// 划词提交（内容到达才显形）置位，批处理后在同一帧消费；失败即弹
    /// （权限卡等）走批次判定。
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
        let pending_reveal = std::mem::take(&mut self.pending_reveal);
        // 显形决策（守卫与「显形或失败即弹」的取舍）在 should_reveal：
        // 这里只递交挂起请求、机器当前视图与本批回传。
        if should_reveal(pending_reveal, self.machine.overlay_view().is_some(), batch)
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

    /// 采纳取材产物：划词探测命中走提交段（内容到达才显形），其余按
    /// 陈旧产物丢弃（收起后的取材、被顶掉的旧探测）。返回是否进入了
    /// 需要展示浮层的新任务。
    pub(super) fn accept_input(&mut self, generation: u64, input: TaskInput) -> bool {
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
                    kind = ?request.task.kind,
                    target_lang = ?request.task.options.target_lang,
                    prompt_locale = ?request.task.options.prompt_locale,
                    "selection committed, task dispatched to tokio"
                );
                self.probe_front_app = None;
                self.send_run(request);
                // 内容到达才显形：从 Idle 出窗、从已显示改锚点重定位，
                // 误滑（探测失败）永远走不到这里。
                self.pending_reveal = true;
                true
            }
            InputOutcome::Blocked(reason) => {
                warn!(
                    thread = thread::UI,
                    generation = generation,
                    reason = ?reason,
                    "probed selection suppressed by the sensitive content guard, task not dispatched"
                );
                self.probe_front_app = None;
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

    /// 采纳自动分类结果（头部任务标签的更新点）。返回是否需要重绘。
    fn accept_classified(&mut self, generation: u64, kind: TaskKind) -> bool {
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

    /// 采纳任务失败：划词探测命中走探测失败处置（误滑静默丢弃 / 权限卡
    /// 显式反馈），其余按推理失败处置（落 `Error` 态弹失败卡，文案与动作
    /// 出口由 machine 按错误类别给出，见 `machine::error_action`）。返回
    /// 是否需要展示浮层。
    pub(super) fn accept_failed(
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
                self.probe_front_app = None;
                false
            }
            FailureOutcome::Shown => {
                warn!(
                    thread = thread::UI,
                    generation = generation,
                    error = %error,
                    "selection probe failed"
                );
                self.probe_front_app = None;
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gloss_core::config::{Config, ModelBinding};
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
    use crate::stubs::ports::{MemoryConfigStore, StubSceneProbe};

    fn suspicious_text() -> String {
        format!("{{\"token\":\"sk-{}\"}}", "9f2b7c1d")
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
            .find(|line| line.contains(r#""message":"probe""#))
            .unwrap_or_default();
        assert!(probe.contains("\"generation\":1"), "{text}");
    }

    #[test]
    fn a_disabled_default_kind_does_not_stop_the_selection_gesture() {
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
            matches!(
                ac_rx.try_recv().unwrap().payload,
                AcquireCommand::AcquireText {
                    generation: 1,
                    kind: TaskKind::Auto
                }
            ),
            "the gesture carries no explicit intent, so the kind switches do not gate it"
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
            panic!("run task expected");
        };
        assert!(!token_a.is_cancelled());

        assert!(app.accept_chunk(1, "部分A".into()));
        assert!(streaming_body(&app).contains("部分A"));

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
        let Command::RunTask { task, .. } = cmd_rx.try_recv().unwrap().payload;
        assert_eq!(task.options.target_lang, Some(Lang::Zh));
        assert_eq!(
            task.kind,
            TaskKind::Auto,
            "the gesture dispatches the sentinel; the model is resolved at rebuild"
        );
        assert_eq!(task.options.model_override, None);

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
        assert_eq!(task.kind, TaskKind::Auto);
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
                is_self: false,
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
    fn probe_empty_selection_is_silently_dropped() {
        let (mut app, _config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert_eq!(
            app.machine.state(),
            AppState::Idle,
            "the probe never enters the state machine"
        );

        assert!(
            !app.accept_failed(1, &gloss_core::model::GlossError::SelectionUnavailable),
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
        let Command::RunTask { cancel: token, .. } = cmd_rx.try_recv().unwrap().payload;
        assert!(
            app.pending_reveal,
            "the commit flagged the reveal for drain_events"
        );
        app.pending_reveal = false;
        assert!(app.accept_chunk(1, "流式正文".into()));
        let view_before = app.machine.overlay_view().cloned();
        let state_before = app.machine.state();

        trigger_selection(&mut app, &pe_tx);
        assert!(
            !token.is_cancelled(),
            "the probe must not cancel the stream"
        );
        assert!(
            !app.accept_failed(2, &gloss_core::model::GlossError::SelectionUnavailable),
            "a mis-slide over a visible session shows nothing new"
        );
        assert_eq!(app.machine.state(), state_before);
        assert_eq!(app.machine.overlay_view(), view_before.as_ref());
        assert!(!app.pending_reveal, "nothing new to reveal");
        assert!(app.machine.probe_id().is_none());

        assert!(app.accept_chunk(1, "、继续".into()));
        assert!(streaming_body(&app).contains("继续"));
    }

    #[test]
    fn committing_the_probe_flags_the_reveal_for_the_same_frame() {
        let (mut app, _config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(!app.pending_reveal, "probing never reveals");

        assert!(app.accept_input(1, text_input("内容到了")));
        assert!(
            app.pending_reveal,
            "the commit flags the reveal; drain_events consumes it the same frame"
        );
        assert_eq!(app.machine.state(), AppState::Translating);
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
