//! 任务状态机与浮层视图（functional core）：纯状态转移，不做任何 IO。
//!
//! 触发 → 取材 → 推理 → 展示/失败的完整转移在此收敛；通道发送、浮层
//! 窗口操作、日志由壳（app 的 winit handler 与组装点）执行——本模块只
//! 决策、不副作用，因此可被集成测试以公共 API 全时序驱动（M3 分层测试
//! 的 L1 层，见 tests/pipeline.rs）。
//!
//! 代数（generation）的**唯一赋值点**是 [`TaskStateMachine::trigger`]：
//! 只有真实下发的触发才递增；回传事件按代数匹配，不匹配即陈旧丢弃。

use tokio_util::sync::CancellationToken;

use gloss_core::model::GlossError;
use gloss_core::task::{InputSource, Task, TaskInput, TaskKind, TaskOptions, TaskOutcome};

use crate::channel::{AcquireCommand, PlatformEvent};

/// 应用状态机（06 §6.1）：触发 → 取材 → 推理 → 展示/失败。
///
/// 转移概要：任何可见态收到新触发（[`TaskStateMachine::trigger`]）都取
/// 消在途任务并回 `Fetching`；`Fetching` 采纳 `InputReady` 后携取消令牌
/// 下发通道③进 `Translating`；`Translating` 收 `TaskChunk` 追加展示、
/// 收 `TaskDone` 定格 `Show`、收 `TaskFailed` 落 `Error`；失焦/超时隐藏
/// 回 `Idle`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppState {
    /// 浮层隐藏，无在途任务。
    #[default]
    Idle,
    /// 取材中：通道②命令已下发，等待 `InputReady`。
    Fetching,
    /// 推理中：`RunTask` 已下发 tokio，chunk 流式到达。
    Translating,
    /// 展示产物。
    Show,
    /// 失败态：显示失败信息，等待下一次触发重试。
    Error,
    /// 框选交互（占位，随 M5 框选遮罩落地；过渡期内无转移路径）。
    #[allow(dead_code)]
    RegionSelecting,
}

/// 浮层内容视图：状态机的可视化投影，由 `ui::popup` 按 TaskKind 分发
/// 渲染（M3-T9）。
#[derive(Debug, Clone, PartialEq)]
pub enum OverlayView {
    /// 取材/推理中：原文 + 已到达的流式正文（含结构化块的原始流，渲染
    /// 层按 [`gloss_core::prompt::STRUCTURED_FENCE`] 过滤）。
    Streaming {
        /// 触发时选中的原文。
        source: String,
        /// 已到达的流式正文累积（原始流）。
        body: String,
    },
    /// 产物卡：按 `TaskKind` 精排或展示 markdown 正文。
    Outcome(TaskOutcome),
    /// 失败信息（再次触发即重试）。
    Failed {
        /// 面向用户的失败说明。
        message: String,
    },
}

/// `accept_input` 采纳取材产物后的下发请求：壳把它经通道③发送。
#[derive(Debug, Clone, PartialEq)]
pub struct RunRequest {
    /// 请求代数，与触发同值。
    pub generation: u64,
    /// 组装好的任务（kind 来自触发时记录，选项缺省，M4 配置接入后填充）。
    pub task: Task,
    /// 随任务下发的取消令牌（App 侧同时留存，新触发时取消）。
    pub cancel: CancellationToken,
}

/// 任务状态机：纯状态 + 决策，无 IO，可全时序驱动。
#[derive(Debug, Default)]
pub struct TaskStateMachine {
    generation: u64,
    state: AppState,
    /// 触发时确定的任务类型，待 `InputReady` 到达后组装 `Task`。
    pending_kind: Option<TaskKind>,
    /// 在途推理的取消令牌：新触发时取消旧任务（唯一取消机制，08 §4.2）。
    current_cancel: Option<CancellationToken>,
    /// 当前浮层的内容视图；`None` 时浮层显示渲染自检卡。
    overlay_view: Option<OverlayView>,
}

impl TaskStateMachine {
    /// 初始态：Idle、零代数、无浮层内容。
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前请求代数。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 当前状态。
    pub fn state(&self) -> AppState {
        self.state
    }

    /// 当前浮层内容视图。
    pub fn overlay_view(&self) -> Option<&OverlayView> {
        self.overlay_view.as_ref()
    }

    /// 在途任务的取消令牌（壳据此在退出/调试时观察取消状态）。
    pub fn current_cancel(&self) -> Option<&CancellationToken> {
        self.current_cancel.as_ref()
    }

    /// 触发的状态机入口：取消在途任务 → 推进代数（唯一赋值点）→ 组装取
    /// 材命令。未接线的平台事件返回 None 且不产生任何状态副作用。
    pub fn trigger(&mut self, event: &PlatformEvent) -> Option<AcquireCommand> {
        let command = acquire_command_for(event, self.generation + 1)?;
        // 最新触发取代在途任务：旧推理立即取消（其迟到产物经代数过滤
        // 丢弃），令牌清空等待新任务。
        if let Some(cancel) = self.current_cancel.take() {
            cancel.cancel();
        }
        self.generation += 1;
        if let AcquireCommand::AcquireText { kind, .. } = &command {
            self.pending_kind = Some(*kind);
        }
        self.state = AppState::Fetching;
        Some(command)
    }

    /// 采纳取材产物：组装 `Task` 并返回下发请求（壳经通道③发送），进入
    /// `Translating`。返回 `Some` 表示进入了需要展示浮层的新任务。
    pub fn accept_input(&mut self, generation: u64, input: TaskInput) -> Option<RunRequest> {
        if generation != self.generation || self.state != AppState::Fetching {
            return None;
        }
        let kind = self.pending_kind.take()?;
        let TaskInput::Text { text, hint } = input else {
            return None;
        };
        let cancel = CancellationToken::new();
        self.current_cancel = Some(cancel.clone());
        let task = Task {
            kind,
            input: TaskInput::Text {
                text: text.clone(),
                hint,
            },
            options: TaskOptions::default(),
        };
        self.overlay_view = Some(OverlayView::Streaming {
            source: text,
            body: String::new(),
        });
        self.state = AppState::Translating;
        Some(RunRequest {
            generation,
            task,
            cancel,
        })
    }

    /// 采纳流式增量：追加到流式视图的原始正文（围栏过滤在渲染层）。
    /// 返回是否有新内容需要重绘。
    pub fn accept_chunk(&mut self, generation: u64, delta: String) -> bool {
        if generation != self.generation || self.state != AppState::Translating {
            return false;
        }
        if let Some(OverlayView::Streaming { body, .. }) = &mut self.overlay_view {
            body.push_str(&delta);
        }
        true
    }

    /// 采纳任务产物：定格正文并进入 `Show`。返回是否需要重绘。
    pub fn accept_done(&mut self, generation: u64, outcome: TaskOutcome) -> bool {
        if generation != self.generation || self.state != AppState::Translating {
            return false;
        }
        self.overlay_view = Some(OverlayView::Outcome(outcome));
        self.state = AppState::Show;
        true
    }

    /// 采纳任务失败：落 `Error` 态并展示失败信息（下一次触发即重试）。
    /// 返回是否需要展示浮层。
    pub fn accept_failed(&mut self, generation: u64, error: &GlossError) -> bool {
        if generation != self.generation {
            return false;
        }
        self.current_cancel = None;
        self.state = AppState::Error;
        self.overlay_view = Some(OverlayView::Failed {
            message: format!("任务失败：{error}（再次触发可重试）"),
        });
        true
    }

    /// 浮层收起后的状态回落：展示/失败信息不再有意义，清空回到 `Idle`。
    /// 在途任务（若恰在 Translating 时被手动收起）不取消——产物到达时
    /// 浮层虽不在展示，状态机仍按代数走完转移。
    pub fn hide_overlay(&mut self) {
        if matches!(
            self.state,
            AppState::Show | AppState::Error | AppState::Translating
        ) {
            self.state = AppState::Idle;
        }
        self.overlay_view = None;
    }

    /// 推理通道不可用（③发送失败）时的降级：直接落 `Error` 态。
    pub fn fail_transport(&mut self, generation: u64) {
        if generation != self.generation {
            return;
        }
        self.current_cancel = None;
        self.state = AppState::Error;
        self.overlay_view = Some(OverlayView::Failed {
            message: "任务失败：推理通道不可用".into(),
        });
    }
}

/// 平台事件 → 取材命令的映射：每次真实触发占用一个新代数；未接线的
/// 平台事件（框选、设置、退出）返回 None。
fn acquire_command_for(event: &PlatformEvent, generation: u64) -> Option<AcquireCommand> {
    match event {
        PlatformEvent::HotkeyTriggered { binding } => match binding.source {
            InputSource::Selection => Some(AcquireCommand::AcquireText {
                generation,
                kind: binding.kind,
            }),
            // 图像取材待框选路径接入后消费。
            InputSource::Region => None,
        },
        PlatformEvent::SelectionGesture => Some(AcquireCommand::AcquireText {
            generation,
            kind: TaskKind::TranslateWord,
        }),
        PlatformEvent::RegionGesture { .. }
        | PlatformEvent::OpenSettingsRequested
        | PlatformEvent::QuitRequested => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gloss_core::model::ScreenRect;
    use gloss_core::task::InputHint;

    use super::*;

    fn text_input(text: &str) -> TaskInput {
        TaskInput::Text {
            text: text.into(),
            hint: Some(InputHint::CodeLanguage("rust".into())),
        }
    }

    /// 映射表：划词 → TranslateWord；热键绑定携带 kind；未接线事件 None。
    #[test]
    fn trigger_mapping_covers_wired_events_only() {
        let mut machine = TaskStateMachine::new();
        let command = machine
            .trigger(&PlatformEvent::SelectionGesture)
            .expect("selection gesture must acquire");
        assert!(matches!(
            command,
            AcquireCommand::AcquireText {
                generation: 1,
                kind: TaskKind::TranslateWord
            }
        ));

        let region_binding = gloss_core::task::HotkeyBinding {
            trigger: "Cmd+Shift+R".into(),
            kind: TaskKind::ImageOcr,
            source: gloss_core::task::InputSource::Region,
        };
        assert!(
            machine
                .trigger(&PlatformEvent::HotkeyTriggered {
                    binding: region_binding
                })
                .is_none(),
            "region source has no acquisition path yet"
        );
        assert_eq!(
            machine.generation(),
            1,
            "unwired events must not consume a generation"
        );
    }

    /// 采纳输入返回下发请求；图像输入与非 Fetching 态被拒。
    #[test]
    fn accept_input_yields_run_request_and_guards_state() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&PlatformEvent::SelectionGesture)
            .expect("trigger");

        let request = machine
            .accept_input(1, text_input("hello"))
            .expect("input should be accepted");
        assert_eq!(request.generation, 1);
        assert_eq!(request.task.kind, TaskKind::TranslateWord);
        assert_eq!(machine.state(), AppState::Translating);
        assert!(machine.current_cancel().is_some());

        // Translating 态不接受第二个 InputReady。
        assert!(machine.accept_input(1, text_input("again")).is_none());
    }

    /// 图像输入在文本 kind 下被拒（模态错配不进入推理）。
    #[test]
    fn image_input_for_text_kind_is_rejected() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&PlatformEvent::SelectionGesture)
            .expect("trigger");
        assert!(
            machine
                .accept_input(
                    1,
                    TaskInput::Image {
                        png: Arc::from(&b"png"[..]),
                        region: ScreenRect {
                            x: 0,
                            y: 0,
                            width: 1,
                            height: 1
                        }
                    }
                )
                .is_none()
        );
    }

    /// 通道不可用降级：fail_transport 直接落 Error 并展示失败视图。
    #[test]
    fn transport_failure_lands_in_error() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&PlatformEvent::SelectionGesture)
            .expect("trigger");
        let request = machine.accept_input(1, text_input("x")).expect("accepted");
        machine.fail_transport(request.generation);
        assert_eq!(machine.state(), AppState::Error);
        assert!(machine.current_cancel().is_none());
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed { .. })
        ));
    }
}
