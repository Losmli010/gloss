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

use gloss_core::config::Config;
use gloss_core::model::GlossError;
use gloss_core::task::{InputSource, Task, TaskInput, TaskKind, TaskOptions, TaskOutcome};

use crate::channel::{AcquireCommand, PlatformEvent};

/// 失败卡的动作按钮（06 §7 错误映射表）：状态机按错误变体给出该显式
/// 给用户的出口，渲染层照画、壳执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorAction {
    /// 可重试类（网络/限流）：原样重发失败的那个任务。
    Retry,
    /// 配置/鉴权类：打开设置页（密钥、模型绑定都在那里修）。
    OpenSettings,
}

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
    /// 失败态：显示失败信息与动作出口（重试按钮或设置页引导）。
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
    /// 失败信息与动作出口：`action` 指出浮层该给用户的按钮（06 §7 错误
    /// 映射），`None` 表示无可操作出口（重新划词即可）。
    Failed {
        /// 面向用户的失败说明。
        message: String,
        /// 失败卡的动作按钮；随错误类别而定。
        action: Option<ErrorAction>,
    },
}

/// `accept_input` 采纳取材产物后的下发请求：壳把它经通道③发送。
#[derive(Debug, Clone, PartialEq)]
pub struct RunRequest {
    /// 请求代数，与触发同值。
    pub generation: u64,
    /// 组装好的任务：`kind` 与 `options` 都取自触发时那份配置快照（见
    /// `PendingTask`），执行途中不再回读配置。
    pub task: Task,
    /// 随任务下发的取消令牌（App 侧同时留存，新触发时取消）。
    pub cancel: CancellationToken,
}

/// 触发时定下的任务：一次配置快照解析出类型与选项，`InputReady` 到达后
/// 直接组装——单次任务的配置从触发那一刻起就固定了（06 §6.3「单次任务内
/// 配置一致」），取材途中换配置不会让同一个任务用上两个版本的参数。
#[derive(Debug, Clone, PartialEq)]
struct PendingTask {
    /// 触发时确定的任务类型。
    kind: TaskKind,
    /// 由同一次快照解析出的任务选项。
    options: TaskOptions,
}

/// 任务状态机：纯状态 + 决策，无 IO，可全时序驱动。
#[derive(Debug, Default)]
pub struct TaskStateMachine {
    generation: u64,
    state: AppState,
    /// 触发时确定的任务类型与选项，待 `InputReady` 到达后组装 `Task`。
    pending: Option<PendingTask>,
    /// 在途推理的取消令牌：新触发时取消旧任务（唯一取消机制，08 §4.2）。
    current_cancel: Option<CancellationToken>,
    /// 当前任务的副本：推理期间随行，可重试失败后留在 Error 态供
    /// [`TaskStateMachine::retry`] 原样重发；完成、隐藏与不可重试失败即清。
    active_task: Option<Task>,
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
    /// 材命令。`config` 是壳在任务开始时取的配置快照：划词手势的任务类型与
    /// 后续选项都按它解析，此后本任务不再读配置。未接线的平台事件返回 None
    /// 且不产生任何状态副作用。
    pub fn trigger(&mut self, event: &PlatformEvent, config: &Config) -> Option<AcquireCommand> {
        let command = acquire_command_for(event, self.generation + 1, config)?;
        // 最新触发取代在途任务：旧推理立即取消（其迟到产物经代数过滤
        // 丢弃），令牌清空等待新任务。
        if let Some(cancel) = self.current_cancel.take() {
            cancel.cancel();
        }
        self.generation += 1;
        // 新触发取代一切旧任务：连可重试的失败任务副本一并作废（重发它
        // 没有意义，用户已经表达了新的意图）。
        self.active_task = None;
        // kind 从命令里取（两个变体都携带），选项按同一个 kind 从**同一份**
        // 快照解析——这里是「单次任务内配置一致」的实现点。
        //
        // `CaptureRegion` 是 M5-T3 的预留：今天 `trigger` 不会返回它
        // （`acquire_command_for` 对 Region 返回 None）。接线时必须同时让
        // `accept_input` 接纳 `TaskInput::Image`，否则任务会卡在 `Fetching`
        // 且不弹浮层（`accept_input` 只收文本）。
        let kind = match &command {
            AcquireCommand::AcquireText { kind, .. }
            | AcquireCommand::CaptureRegion { kind, .. } => *kind,
        };
        self.pending = Some(PendingTask {
            kind,
            options: task_options(kind, config),
        });
        self.state = AppState::Fetching;
        Some(command)
    }

    /// 采纳取材产物：组装 `Task` 并返回下发请求（壳经通道③发送），进入
    /// `Translating`。选项取触发时那份快照（不经参数再传配置）。返回
    /// `Some` 表示进入了需要展示浮层的新任务。
    pub fn accept_input(&mut self, generation: u64, input: TaskInput) -> Option<RunRequest> {
        if generation != self.generation || self.state != AppState::Fetching {
            return None;
        }
        // 先校验模态再消费 pending：模态错配不吃掉待组装任务，同代数
        // 的后续合法 InputReady 仍可被采纳。
        let TaskInput::Text { text, hint } = input else {
            return None;
        };
        let PendingTask { kind, options } = self.pending.take()?;
        let cancel = CancellationToken::new();
        self.current_cancel = Some(cancel.clone());
        let task = Task {
            kind,
            input: TaskInput::Text {
                text: text.clone(),
                hint,
            },
            options,
        };
        self.active_task = Some(task.clone());
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
        self.active_task = None;
        self.overlay_view = Some(OverlayView::Outcome(outcome));
        self.state = AppState::Show;
        true
    }

    /// 采纳任务失败：落 `Error` 态并展示失败信息与动作出口（06 §7 错误
    /// 映射：可重试类带重试按钮并保留任务副本，配置/鉴权类引导去设置页）。
    /// 返回是否需要展示浮层。
    pub fn accept_failed(&mut self, generation: u64, error: &GlossError) -> bool {
        // Fetching 态收取材失败、Translating 态收推理失败；其余（含已
        // 隐藏）不采纳——失败卡不得把已收起的浮层弹回。
        if generation != self.generation
            || !matches!(self.state, AppState::Fetching | AppState::Translating)
        {
            return false;
        }
        self.current_cancel = None;
        // 只有「原样重发有意义」的失败才留任务副本；其余类别（含通道级
        // 故障的 fail_* 降级路径）一律清掉，retry() 自然无从发起。
        let action = error_action(error);
        if action != Some(ErrorAction::Retry) {
            self.active_task = None;
        }
        self.state = AppState::Error;
        self.overlay_view = Some(OverlayView::Failed {
            message: error_message(error),
            action,
        });
        true
    }

    /// 重试失败卡上的任务（Error 态）：原样重发失败的那个任务（同代数
    /// ——旧任务的流已随首个错误终结，不会有两路同代回传），浮层回到
    /// 流式视图。非 Error 态或无可重试任务时返回 `None`。
    pub fn retry(&mut self) -> Option<RunRequest> {
        if self.state != AppState::Error {
            return None;
        }
        let task = self.active_task.clone()?;
        let cancel = CancellationToken::new();
        self.current_cancel = Some(cancel.clone());
        self.overlay_view = Some(OverlayView::Streaming {
            source: source_text(&task),
            body: String::new(),
        });
        self.state = AppState::Translating;
        Some(RunRequest {
            generation: self.generation,
            task,
            cancel,
        })
    }

    /// 浮层收起（失焦/自动隐藏）即放弃在途任务：取消令牌（唯一取消机
    /// 制）、清空视图并回 `Idle`。放弃后的迟到产物经代数或状态守卫丢弃
    /// ——为一个不可见的浮层继续推理与渲染纯属空转；重新划词即重新开
    /// 始，取材自当前选区（旧产物本就可能已过期）。
    pub fn hide_overlay(&mut self) {
        if let Some(cancel) = self.current_cancel.take() {
            cancel.cancel();
        }
        self.active_task = None;
        self.state = AppState::Idle;
        self.overlay_view = None;
    }

    /// 取材通道不可用（②发送失败）时的降级：直接落 `Error` 态，避免
    /// 滞留 Fetching 等一个永远不会到达的 `InputReady`。
    pub fn fail_acquire(&mut self, generation: u64) {
        if generation != self.generation {
            return;
        }
        self.state = AppState::Error;
        self.overlay_view = Some(OverlayView::Failed {
            message: "任务失败：取材通道不可用".into(),
            action: None,
        });
    }

    /// 推理通道不可用（③发送失败）时的降级：直接落 `Error` 态。通道
    /// 已死时重发只会再死一次，失败卡不带重试按钮。
    pub fn fail_transport(&mut self, generation: u64) {
        if generation != self.generation {
            return;
        }
        self.current_cancel = None;
        self.active_task = None;
        self.state = AppState::Error;
        self.overlay_view = Some(OverlayView::Failed {
            message: "任务失败：推理通道不可用".into(),
            action: None,
        });
    }
}

/// 失败卡的展示文案（按错误变体，06 §7 的用户可见措辞）。
fn error_message(error: &GlossError) -> String {
    match error {
        GlossError::SelectionUnavailable => "未能读取选中文本，请重新选中后触发".into(),
        GlossError::AccessibilityDenied => {
            "辅助功能权限未授权：系统设置 → 隐私与安全性 → 辅助功能".into()
        }
        GlossError::ScreenCaptureDenied => {
            "屏幕录制权限未授权：系统设置 → 隐私与安全性 → 屏幕录制".into()
        }
        GlossError::RegionTooLarge => "框选区域超出屏幕，请重新框选".into(),
        GlossError::UnsupportedModality => {
            "当前模型不支持该任务，请在设置中为它配置匹配能力的模型".into()
        }
        GlossError::EngineNetwork => "网络错误，请检查网络后重试".into(),
        GlossError::EngineAuth => "API Key 无效或未配置，请到设置中检查".into(),
        GlossError::EngineRateLimited => "触发限流，请稍后重试".into(),
        GlossError::EngineResponse(detail) => format!("服务返回异常：{detail}"),
        GlossError::Config(detail) => format!("配置有误：{detail}"),
    }
}

/// 错误 → 失败卡动作（06 §7 映射表）：网络/限流可原样重试；鉴权、模态
/// 与配置错误都要进设置页才能修（模型绑定、密钥的修改入口在 M4-T6 落
/// 地）；其余类别没有按钮意义上的出口——权限类引导已写在文案里，协议
/// 异常重发同一个请求只会再错一次。
fn error_action(error: &GlossError) -> Option<ErrorAction> {
    match error {
        GlossError::EngineNetwork | GlossError::EngineRateLimited => Some(ErrorAction::Retry),
        GlossError::EngineAuth | GlossError::UnsupportedModality | GlossError::Config(_) => {
            Some(ErrorAction::OpenSettings)
        }
        _ => None,
    }
}

/// 任务原文（流式视图与重试用）：当前只有文本任务进入推理（M5 接入图
/// 像取材时随它扩展），其余模态留空。
fn source_text(task: &Task) -> String {
    match &task.input {
        TaskInput::Text { text, .. } => text.clone(),
        TaskInput::Image { .. } | TaskInput::Audio { .. } => String::new(),
    }
}

/// 平台事件 → 取材命令的映射：每次真实触发占用一个新代数；未接线的
/// 平台事件（框选、设置、退出）返回 None。划词手势的任务类型来自配置的
/// `default_text_kind`，并按取材源收口成文本类——配置写成图像 kind 时不
/// 送进引擎挨模态校验（见 `Config::selection_task_kind`）；热键绑定携带的
/// kind 是逐条显式意图，不做收口（M4-T7 配置化时校验）。
fn acquire_command_for(
    event: &PlatformEvent,
    generation: u64,
    config: &Config,
) -> Option<AcquireCommand> {
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
            kind: config.selection_task_kind(),
        }),
        PlatformEvent::RegionGesture { .. }
        | PlatformEvent::OpenSettingsRequested
        | PlatformEvent::QuitRequested => None,
    }
}

/// 按配置快照解析任务选项：目标语言取配置默认；模型按 kind 从
/// `model_by_kind` 解析（缺项时 core 的出厂默认兜底）后随任务下发——引擎
/// 只见到任务自身携带的模型，执行途中不再回读配置。
fn task_options(kind: TaskKind, config: &Config) -> TaskOptions {
    TaskOptions {
        target_lang: Some(config.target_lang.clone()),
        model_override: config.resolved_model(kind).map(str::to_owned),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gloss_core::config::ModelBinding;
    use gloss_core::model::{Lang, ScreenRect};
    use gloss_core::task::{InputHint, OutcomeStructured};

    use super::*;

    fn plain_outcome(body: &str) -> TaskOutcome {
        TaskOutcome {
            kind: TaskKind::TranslateWord,
            body: body.into(),
            structured: OutcomeStructured::Plain { title: None },
        }
    }

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
            .trigger(&PlatformEvent::SelectionGesture, &Config::default())
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
                .trigger(
                    &PlatformEvent::HotkeyTriggered {
                        binding: region_binding
                    },
                    &Config::default()
                )
                .is_none(),
            "region source has no acquisition path yet"
        );
        assert_eq!(
            machine.generation(),
            1,
            "unwired events must not consume a generation"
        );
    }

    /// 配置生效（M4-T3）：划词手势的任务类型取自快照的 `default_text_kind`，
    /// **且**模型按同一个 kind 解析——两条断言并排，锁住「命令的 kind」与
    /// 「选项里的模型」出自同一份快照、同一个 kind（只测其中一边的话，把
    /// `task_options` 硬编码成 TranslateWord 也能全绿）。
    #[test]
    fn selection_kind_and_options_pair_with_one_snapshot() {
        let mut machine = TaskStateMachine::new();
        let config = Config {
            default_text_kind: TaskKind::ExplainCode,
            target_lang: Lang::Ja,
            model_by_kind: vec![
                ModelBinding {
                    kind: TaskKind::TranslateWord,
                    model: "word-model".into(),
                },
                ModelBinding {
                    kind: TaskKind::ExplainCode,
                    model: "code-model".into(),
                },
            ],
            ..Default::default()
        };

        let command = machine
            .trigger(&PlatformEvent::SelectionGesture, &config)
            .expect("selection gesture must acquire");
        assert!(
            matches!(
                command,
                AcquireCommand::AcquireText {
                    generation: 1,
                    kind: TaskKind::ExplainCode
                }
            ),
            "gesture kind must come from the snapshot"
        );

        let request = machine
            .accept_input(1, text_input("fn main() {}"))
            .expect("input should be accepted");
        assert_eq!(request.task.kind, TaskKind::ExplainCode);
        assert_eq!(request.task.options.target_lang, Some(Lang::Ja));
        assert_eq!(
            request.task.options.model_override.as_deref(),
            Some("code-model"),
            "model must be resolved for the same kind, not the factory default"
        );
    }

    /// 取材源收口：配置把 `default_text_kind` 误配成图像 kind 时，划词路径
    /// 回退文本 kind，而不是把必被模态校验拒的任务送进引擎。
    #[test]
    fn image_default_kind_falls_back_to_a_text_kind() {
        let mut machine = TaskStateMachine::new();
        let config = Config {
            default_text_kind: TaskKind::ImageOcr,
            ..Default::default()
        };

        let command = machine
            .trigger(&PlatformEvent::SelectionGesture, &config)
            .expect("selection gesture must acquire");
        assert!(matches!(
            command,
            AcquireCommand::AcquireText {
                kind: TaskKind::TranslateWord,
                ..
            }
        ));
    }

    /// 快照在触发时定下：取材途中换配置（这里模拟为换一份 config 再喂
    /// InputReady）不影响已触发的任务——选项在 `trigger` 那一刻就固定了。
    #[test]
    fn options_freeze_at_trigger_time() {
        let mut machine = TaskStateMachine::new();
        let before = Config {
            target_lang: Lang::Ja,
            ..Default::default()
        };
        let after = Config {
            target_lang: Lang::Ko,
            ..Default::default()
        };

        machine
            .trigger(&PlatformEvent::SelectionGesture, &before)
            .expect("trigger");
        // 触发后配置变了，但本任务的选项不跟着变。
        let request = machine
            .accept_input(1, text_input("hello"))
            .expect("input should be accepted");
        assert_eq!(
            request.task.options.target_lang,
            Some(Lang::Ja),
            "in-flight task must keep the snapshot taken at trigger"
        );
        // 下一次触发才用上新配置。
        machine
            .trigger(&PlatformEvent::SelectionGesture, &after)
            .expect("second trigger");
        let request = machine
            .accept_input(2, text_input("world"))
            .expect("input should be accepted");
        assert_eq!(request.task.options.target_lang, Some(Lang::Ko));
    }

    /// 采纳输入返回下发请求；图像输入与非 Fetching 态被拒。
    #[test]
    fn accept_input_yields_run_request_and_guards_state() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&PlatformEvent::SelectionGesture, &Config::default())
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
            .trigger(&PlatformEvent::SelectionGesture, &Config::default())
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

    /// 隐藏即放弃：Translating 态隐藏取消在途任务、清空视图回 Idle；
    /// 迟到的同代数 TaskDone/TaskFailed 一律被状态守卫丢弃。
    #[test]
    fn hide_abandons_inflight_and_drops_late_events() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&PlatformEvent::SelectionGesture, &Config::default())
            .expect("trigger");
        let request = machine
            .accept_input(1, text_input("hello"))
            .expect("accepted");
        let token = request.cancel;

        machine.hide_overlay();
        assert_eq!(machine.state(), AppState::Idle);
        assert!(machine.overlay_view().is_none());
        assert!(token.is_cancelled(), "hide must cancel the in-flight task");
        assert!(machine.current_cancel().is_none());

        assert!(
            !machine.accept_done(1, plain_outcome("迟到产物")),
            "hidden task's late outcome must be dropped"
        );
        assert!(
            !machine.accept_failed(1, &GlossError::EngineNetwork),
            "hidden task's late failure must be dropped"
        );
        assert_eq!(machine.state(), AppState::Idle);
    }

    /// 失败守卫收紧后：Fetching 态收取材失败、Error 态迟到失败被拒。
    #[test]
    fn failed_guard_matches_fetching_and_translating_only() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&PlatformEvent::SelectionGesture, &Config::default())
            .expect("trigger");
        // Fetching 态：取材失败可采纳。
        assert!(machine.accept_failed(1, &GlossError::SelectionUnavailable));
        assert_eq!(machine.state(), AppState::Error);

        // Error 态隐藏后：同代数迟到失败被状态守卫拒绝。
        machine.hide_overlay();
        assert!(
            !machine.accept_failed(1, &GlossError::EngineNetwork),
            "hidden error card must not be resurrected"
        );
    }

    /// 模态错配不吃掉 pending：同代数的合法 InputReady 仍可采纳。
    #[test]
    fn modality_mismatch_preserves_pending_task() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&PlatformEvent::SelectionGesture, &Config::default())
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
        assert!(
            machine.accept_input(1, text_input("第二次")).is_some(),
            "pending kind must survive a modality mismatch"
        );
    }

    /// 通道不可用降级：fail_transport 直接落 Error 并展示失败视图（无
    /// 动作按钮——通道已死时重发只会再死一次）。
    #[test]
    fn transport_failure_lands_in_error() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&PlatformEvent::SelectionGesture, &Config::default())
            .expect("trigger");
        let request = machine.accept_input(1, text_input("x")).expect("accepted");
        machine.fail_transport(request.generation);
        assert_eq!(machine.state(), AppState::Error);
        assert!(machine.current_cancel().is_none());
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed { action: None, .. })
        ));
        assert!(machine.retry().is_none());
    }

    /// 可重试失败（06 §7）：失败卡带重试出口，retry() 原样重发同一个
    /// 任务（同代数、同输入、新令牌），浮层回到流式视图。
    #[test]
    fn retryable_failure_keeps_task_and_retry_redispatches_it() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&PlatformEvent::SelectionGesture, &Config::default())
            .expect("trigger");
        let original = machine
            .accept_input(1, text_input("hello"))
            .expect("accepted");
        assert!(machine.accept_failed(1, &GlossError::EngineNetwork));

        match machine.overlay_view() {
            Some(OverlayView::Failed {
                message,
                action: Some(ErrorAction::Retry),
            }) => assert!(message.contains("网络"), "message must name the class"),
            other => panic!("retryable failure expected, got {other:?}"),
        }

        let retried = machine.retry().expect("retry must be available");
        assert_eq!(retried.generation, original.generation, "same generation");
        assert_eq!(retried.task, original.task, "same task is re-dispatched");
        assert!(retried.cancel != original.cancel, "fresh cancel token");
        assert_eq!(machine.state(), AppState::Translating);
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Streaming { source, .. }) if source == "hello"
        ));
    }

    /// 限流与网络同类可重试；配置/鉴权/模态类引导去设置页且不可按钮重试。
    #[test]
    fn error_actions_follow_the_mapping_table() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&PlatformEvent::SelectionGesture, &Config::default())
            .expect("trigger");
        machine.accept_input(1, text_input("x")).expect("accepted");

        assert!(
            machine.accept_failed(1, &GlossError::EngineRateLimited),
            "rate limited is retryable"
        );
        assert!(machine.retry().is_some());

        // 限流失败把任务副本留在 Error 态；这里直接再次 accept_failed 不
        // 合法（已是 Error 态），所以重新走一遍触发→输入→失败。
        machine
            .trigger(&PlatformEvent::SelectionGesture, &Config::default())
            .expect("trigger");
        machine.accept_input(2, text_input("x")).expect("accepted");
        assert!(machine.accept_failed(2, &GlossError::EngineAuth));
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed {
                action: Some(ErrorAction::OpenSettings),
                ..
            })
        ));
        assert!(
            machine.retry().is_none(),
            "auth failure must not offer a retry button"
        );

        machine
            .trigger(&PlatformEvent::SelectionGesture, &Config::default())
            .expect("trigger");
        machine.accept_input(3, text_input("x")).expect("accepted");
        assert!(machine.accept_failed(3, &GlossError::UnsupportedModality));
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed {
                action: Some(ErrorAction::OpenSettings),
                ..
            })
        ));
    }

    /// 新触发与隐藏都作废重试：任务副本清空，retry() 返回 None。
    #[test]
    fn new_trigger_and_hide_supersede_the_retry_task() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&PlatformEvent::SelectionGesture, &Config::default())
            .expect("trigger");
        machine.accept_input(1, text_input("x")).expect("accepted");
        assert!(machine.accept_failed(1, &GlossError::EngineNetwork));

        // 新触发取代失败卡：可重试副本作废。
        machine
            .trigger(&PlatformEvent::SelectionGesture, &Config::default())
            .expect("trigger");
        assert!(machine.retry().is_none(), "new trigger supersedes retry");

        // 失败 → 隐藏：重试随浮层一起放弃。
        machine.accept_input(2, text_input("y")).expect("accepted");
        assert!(machine.accept_failed(2, &GlossError::EngineNetwork));
        machine.hide_overlay();
        assert_eq!(machine.state(), AppState::Idle);
        assert!(machine.retry().is_none(), "hide drops the retry task");
    }

    /// 展示文案按类别给出可执行的指引（权限类写清去哪里授权）。
    #[test]
    fn error_messages_name_the_fix() {
        assert_eq!(
            error_message(&GlossError::AccessibilityDenied),
            "辅助功能权限未授权：系统设置 → 隐私与安全性 → 辅助功能"
        );
        assert_eq!(
            error_message(&GlossError::EngineAuth),
            "API Key 无效或未配置，请到设置中检查"
        );
        assert!(
            error_message(&GlossError::EngineResponse("bad json".into())).contains("bad json"),
            "protocol errors keep the diagnostic text"
        );
    }
}
