//! 编排用例·探测段：消费通道①（平台事件）→ 场景闸门 → 产出取材命令
//! 发往通道②。状态机不动、不显形——产物到达才由 `task` 提交。

use gloss_core::log::{Span, debug, info, task_span, thread, warn};

use crate::channel::{AcquireCommand, PlatformEvent, Traced};
use crate::machine::{TriggerDecision, trigger_decision};

use crate::app::GlossApp;

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
    pub(crate) fn drain_platform_events(&mut self) {
        // 配置快照在本批事件的起手处取一次（零锁读）：本批触发的任务都用
        // 同一份配置解析类型与选项——任务一旦触发，其配置就固定了。
        let config = self.env.config.snapshot();
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
            let scene = self.env.scene.facts();
            let Some(command) = (match &event {
                PlatformEvent::SelectionGesture { .. } => self.machine.begin_selection_probe(
                    &event,
                    &config,
                    self.env.system_locale,
                    &scene,
                ),
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
            self.session.task_span = Some((generation, span.clone()));
            let entered_span = span.clone();
            let _entered = entered_span.enter();
            info!(
                thread = thread::UI,
                "selection probe dispatched as acquire command"
            );
            // 划词探测记录释放坐标与前台应用（随探测编号）：浮层显示时跟随
            // 选区；探测失败的排查日志带上应用标识。
            if let PlatformEvent::SelectionGesture { pos } = event {
                self.session.selection_anchor = Some((generation, pos));
                self.session.probe_front_app = scene.front_app;
            }
            if !self.send_acquire(command, span) {
                // 取材通道发送失败：作废探测即可（当前显示不动）。
                self.machine.drop_probe();
                self.session.probe_front_app = None;
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
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gloss_core::config::Config;
    use gloss_core::guard::{FrontApp, SceneFacts};
    use gloss_core::log::{capture_global, info, thread};
    use gloss_core::model::Lang;
    use gloss_core::ports::SceneProbe;

    use crate::app::test_support::{driven_app, driven_app_with_scene, trigger_selection};
    use crate::channel::AcquireCommand;
    use crate::machine::AppState;
    use crate::stubs::ports::{MemoryConfigStore, StubSceneProbe};

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
                target_lang: Lang::Ja,
                ..Default::default()
            })
            .expect("save should succeed");

        trigger_selection(&mut app, &pe_tx);
        assert!(
            matches!(
                ac_rx.try_recv().unwrap().payload,
                AcquireCommand::AcquireText { generation: 1 }
            ),
            "the gesture carries no explicit intent: acquisition is kind-free"
        );
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
}
