//! 浮层显隐：显示入口、收起出口与浮层动作执行，附露面与显形决策的
//! 纯函数。

use gloss_core::log::{debug, info, thread};
use gloss_core::model::ScreenPoint;
use winit::dpi::LogicalPosition;
use winit::event_loop::ActiveEventLoop;

use crate::channel::Event;
use crate::ui::popup::OverlayAction;
use crate::windows::{Placement, WindowManager};

use super::GlossApp;

impl GlossApp {
    /// 统一显示入口：显示并重置出现动画起点，让本次显示从淡入开始；
    /// 同时清上一轮的拖动状态，跨显示残留的拖动所有权不得带进本轮
    /// （隐藏期丢失的鼠标释放会让 egui 侧拖动所有权残留，按压点清掉后
    /// 热区保持惰性，见 `ui::popup::RenderState::reset_drag_state`）。
    /// 浮层常驻——收起只认 Esc、关闭按钮与新触发的内容替换。
    pub(super) fn show_overlay(&mut self, position: LogicalPosition<f64>) {
        let Some(windows) = &self.windows else {
            return;
        };
        if let Some(frame) = self.frame.as_ref() {
            crate::ui::popup::reset_appear_animation(&frame.egui_ctx);
            frame.reset_overlay_drag();
        }
        windows.show_at(position);
    }

    /// 收起浮层的统一出口（Esc / 关闭按钮 / 内容闸门拦下）：隐藏窗口、清
    /// 渲染截止时刻与拖动状态防空转防残留，状态机放弃在途任务回 `Idle`
    /// （迟到产物经代数或状态守卫丢弃——为一个不可见的浮层继续推理与
    /// 渲染纯属空转）。
    pub(super) fn dismiss_overlay(&mut self, reason: &'static str) {
        info!(
            thread = thread::UI,
            generation = self.machine.generation(),
            reason,
            "overlay dismissed"
        );
        self.overlay_repaint = None;
        if let Some(frame) = self.frame.as_ref() {
            frame.reset_overlay_drag();
        }
        if let Some(windows) = &self.windows {
            windows.hide();
        }
        self.machine.hide_overlay();
    }

    /// 执行浮层一帧上交的动作（错误映射的壳侧半边 + 头部动作区）。
    pub(super) fn handle_overlay_action(&mut self, action: OverlayAction) {
        match action {
            OverlayAction::Retry => match self.machine.retry() {
                Some(request) => {
                    info!(
                        thread = thread::UI,
                        generation = request.generation,
                        "error card retry, task re-dispatched to tokio"
                    );
                    self.send_run(request);
                }
                None => {
                    debug!(
                        thread = thread::UI,
                        generation = self.machine.generation(),
                        state = ?self.machine.state(),
                        "stale retry click dropped"
                    );
                }
            },
            // 失败卡的「打开设置」与头部齿轮、托盘走同一个入口。
            OverlayAction::OpenSettings => self.open_settings(),
            OverlayAction::Dismiss => self.dismiss_overlay("close button"),
        }
    }
}

/// 浮层显示位置与摆放意图的决策：当前代数携带划词释放坐标时在选区
/// 附近露面（右下偏移一点，避免浮层压在光标/选区上）并记为定点摆放
/// ——后续内容撑高窗口时原锚点重新钳制而非被居中覆盖；否则居中
/// （陈旧代数同样回落居中）。
pub(super) fn show_position(
    anchor: Option<(u64, ScreenPoint)>,
    generation: u64,
    centered: LogicalPosition<f64>,
) -> (LogicalPosition<f64>, Placement) {
    match anchor.filter(|(anchored_gen, _)| *anchored_gen == generation) {
        Some((_, pos)) => {
            let position = LogicalPosition::new(
                f64::from(pos.x) + SELECTION_OFFSET_X,
                f64::from(pos.y) + SELECTION_OFFSET_Y,
            );
            (position, Placement::At(position))
        }
        None => (centered, Placement::Centered),
    }
}

/// 浮层跟随划词位置时的偏移（逻辑点）：让卡片略偏右下，不压住光标。
const SELECTION_OFFSET_X: f64 = 16.0;
const SELECTION_OFFSET_Y: f64 = 20.0;

/// 浮层居中于显示器（逻辑坐标）：优先窗口当前所在的显示器，其次主显示器。
/// 定位与尺寸的算法在窗口管理器（`WindowManager::centered_position`），
/// 这里是给壳与自检 handler 的稳定入口。
pub fn centered_position(
    event_loop: &ActiveEventLoop,
    windows: &WindowManager,
) -> LogicalPosition<f64> {
    windows.centered_position(event_loop)
}

/// 回传事件的类别标签（`auto_show` 策略只关心类别，不关心代数与载荷）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventKind {
    InputReady,
    TaskClassified,
    TaskChunk,
    TaskDone,
    TaskFailed,
}

/// 取回传事件的类别标签。
pub(super) fn event_kind(event: &Event) -> EventKind {
    match event {
        Event::InputReady { .. } => EventKind::InputReady,
        Event::TaskClassified { .. } => EventKind::TaskClassified,
        Event::TaskChunk { .. } => EventKind::TaskChunk,
        Event::TaskDone { .. } => EventKind::TaskDone,
        Event::TaskFailed { .. } => EventKind::TaskFailed,
    }
}

/// 单个回传事件后浮层要不要自动露面。
///
/// `accepted` 是状态机是否采纳了该事件：陈旧事件不触发显示。
/// 露面的两个来源：挂起显形请求（经壳层 `pending_reveal`——划词提交
/// 即显流式卡，见 [`should_reveal`]——那时还没有回传事件或产物已在批内
/// 提交），与失败总弹（错误不该被吞掉）。
/// 取材成功不直接负责露面：划词路径的显形随提交置位；用户若在取材中
/// 收起浮层，机器回 Idle，陈旧的取材产物采纳不上，自然也不会把浮层弹回。
/// 分类结果、流式增量与完成态都只在已可见的浮层上更新——三者同样不负责
/// 露面。
fn auto_show_for(kind: EventKind, accepted: bool) -> bool {
    match kind {
        EventKind::InputReady => false,
        EventKind::TaskFailed => accepted,
        EventKind::TaskClassified | EventKind::TaskDone | EventKind::TaskChunk => false,
    }
}

/// 一批回传之后浮层要不要自动露面：**任一**事件判为要显示就显示。
pub(super) fn auto_show_after(batch: impl IntoIterator<Item = (EventKind, bool)>) -> bool {
    batch
        .into_iter()
        .any(|(kind, accepted)| auto_show_for(kind, accepted))
}

/// 一批回传处理后浮层要不要显形：挂起显形请求（`pending`）要求机器确有
/// 视图——视图为空时弹出的会是渲染自检卡（挂起置位与消费之间没有插入
/// 点，两段 drain 同帧连跑，守卫只为防御）；划词提交的置位在批内
/// `commit_probe`，同一帧消费。挂起显形不依赖回传批次，与批次内的
/// 「失败即弹」任一成立即显示。
pub(super) fn should_reveal(
    pending: bool,
    has_view: bool,
    batch: impl IntoIterator<Item = (EventKind, bool)>,
) -> bool {
    (pending && has_view) || auto_show_after(batch)
}

#[cfg(test)]
mod tests {
    use gloss_core::model::ScreenPoint;
    use gloss_core::task::TaskInput;

    use crate::app::test_support::{driven_app, plain_outcome, text_input, trigger_selection};
    use crate::channel::{Command, PlatformEvent};
    use crate::machine::{AppState, ErrorAction, OverlayView};
    use winit::dpi::LogicalPosition;

    use super::{EventKind, auto_show_after, auto_show_for, should_reveal, show_position};
    use crate::windows::Placement;

    #[test]
    fn show_position_follows_selection_only_for_the_current_generation() {
        let anchor = Some((1, ScreenPoint::new(100, 200)));
        let centered = LogicalPosition::new(500.0, 500.0);

        let (position, placement) = show_position(anchor, 1, centered);
        assert_eq!(
            position,
            LogicalPosition::new(116.0, 220.0),
            "当前代数的划词触发跟随选区（右下偏移）"
        );
        assert_eq!(
            placement,
            Placement::At(position),
            "跟随触发必须记为定点摆放"
        );

        let (position, placement) = show_position(anchor, 2, centered);
        assert_eq!(position, centered, "代数对不上（后续触发）回落居中");
        assert_eq!(placement, Placement::Centered);

        let (position, placement) = show_position(None, 1, centered);
        assert_eq!(position, centered, "无划词锚点回落居中");
        assert_eq!(placement, Placement::Centered);
    }

    #[test]
    fn selection_trigger_records_its_anchor_per_generation() {
        let (mut app, _config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();

        pe_tx
            .send(PlatformEvent::SelectionGesture {
                pos: ScreenPoint::new(123, 45),
            })
            .unwrap();
        app.drain_platform_events();
        assert_eq!(
            app.selection_anchor,
            Some((1, ScreenPoint::new(123, 45))),
            "划词触发记录释放坐标随代数"
        );

        pe_tx
            .send(PlatformEvent::SelectionGesture {
                pos: ScreenPoint::new(9, 9),
            })
            .unwrap();
        app.drain_platform_events();
        assert_eq!(
            app.selection_anchor,
            Some((2, ScreenPoint::new(9, 9))),
            "新触发推进代数并刷新锚点"
        );
    }

    #[test]
    fn retry_action_redispatches_the_failed_task() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        let Command::RunTask { cancel, .. } = cmd_rx.try_recv().unwrap().payload;
        assert!(app.accept_failed(1, &gloss_core::model::GlossError::EngineNetwork));
        assert!(matches!(
            app.machine.overlay_view(),
            Some(OverlayView::Failed {
                action: Some(ErrorAction::Retry),
                ..
            })
        ));

        app.handle_overlay_action(crate::ui::popup::OverlayAction::Retry);
        assert_eq!(app.machine.state(), AppState::Translating);
        let Command::RunTask {
            generation,
            task,
            cancel: retried,
        } = cmd_rx.try_recv().unwrap().payload;
        assert_eq!(generation, 1, "retry keeps the failed task's generation");
        assert!(matches!(
            task.input,
            TaskInput::Text { ref text, .. } if text == "A"
        ));
        assert!(!retried.is_cancelled());
        assert!(
            !cancel.is_cancelled(),
            "a failed task's token is dropped, not cancelled"
        );

        assert!(app.accept_done(1, plain_outcome("重试成功")));
        assert_eq!(app.machine.state(), AppState::Show);
    }

    #[test]
    fn open_settings_action_keeps_the_error_card() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        let Command::RunTask { .. } = cmd_rx.try_recv().unwrap().payload;
        assert!(app.accept_failed(1, &gloss_core::model::GlossError::EngineAuth));
        assert!(matches!(
            app.machine.overlay_view(),
            Some(OverlayView::Failed {
                action: Some(ErrorAction::OpenSettings),
                ..
            })
        ));

        app.handle_overlay_action(crate::ui::popup::OverlayAction::OpenSettings);
        assert_eq!(app.machine.state(), AppState::Error);
        assert!(cmd_rx.try_recv().is_err(), "no re-dispatch for settings");
        assert!(
            app.settings.is_some(),
            "the open-settings action must start the edit session"
        );
    }

    #[test]
    fn dismiss_abandons_the_inflight_task_and_returns_to_idle() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        let Command::RunTask { cancel, .. } = cmd_rx.try_recv().unwrap().payload;

        app.dismiss_overlay("test");
        assert_eq!(app.machine.state(), AppState::Idle);
        assert!(
            cancel.is_cancelled(),
            "dismiss must cancel the in-flight task"
        );
        assert!(app.machine.overlay_view().is_none());
        assert!(
            !app.accept_done(1, plain_outcome("迟到结果")),
            "a late outcome after dismissal must be dropped"
        );
    }

    #[test]
    fn auto_show_policy_decides_when_the_overlay_pops() {
        use EventKind::{InputReady, TaskChunk, TaskDone, TaskFailed};

        assert!(
            !auto_show_for(InputReady, true),
            "显形走挂起请求，取材成功不负责露面"
        );
        assert!(!auto_show_for(TaskDone, true), "完成时浮层早已可见");
        assert!(!auto_show_for(TaskChunk, true), "chunk 只追加不露面");
        assert!(auto_show_for(TaskFailed, true), "错误不该被吞掉");

        for kind in [InputReady, TaskChunk, TaskDone, TaskFailed] {
            assert!(
                !auto_show_for(kind, false),
                "未被采纳的 {kind:?} 不得触发显示"
            );
        }
    }

    #[test]
    fn auto_show_survives_a_mixed_batch() {
        use EventKind::{InputReady, TaskChunk, TaskDone, TaskFailed};

        assert!(
            auto_show_after([(TaskChunk, true), (TaskFailed, true)]),
            "任一事件要显示就显示"
        );
        assert!(
            auto_show_after([(TaskDone, false), (TaskFailed, true)]),
            "陈旧的成功不得抵消一条被采纳的失败"
        );
        assert!(
            !auto_show_after([(TaskDone, false), (InputReady, false)]),
            "整批都没被采纳（陈旧）→ 不显示：迟到的产物不得把浮层弹回来"
        );
        assert!(!auto_show_after([]), "空批不显示");
    }

    #[test]
    fn pending_reveal_shows_the_skeleton_only_over_a_live_view() {
        use EventKind::TaskFailed;

        assert!(
            should_reveal(true, true, []),
            "触发即显：骨架视图在场时挂起请求即显形"
        );
        assert!(
            !should_reveal(true, false, []),
            "视图为空时弹出的会是渲染自检卡，守卫必须拦下挂起显形"
        );
        assert!(
            !should_reveal(false, false, [(TaskFailed, false)]),
            "无挂起且批次里没有失败即弹时不显示"
        );
    }

    #[test]
    fn reveal_decision_combines_the_pending_request_with_the_batch() {
        use EventKind::{InputReady, TaskChunk, TaskDone, TaskFailed};

        assert!(
            should_reveal(true, true, [(TaskDone, false), (InputReady, false)]),
            "挂起显形不依赖回传批次：整批陈旧也拦不下它"
        );
        assert!(
            should_reveal(false, false, [(TaskFailed, true)]),
            "失败即弹独立成立：无挂起也照常显示"
        );
        assert!(
            !should_reveal(false, true, [(InputReady, true), (TaskChunk, true)]),
            "取材成功与流式增量不负责露面：批次再新鲜也不显形"
        );
    }
}
