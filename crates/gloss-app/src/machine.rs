//! 任务状态机与浮层视图（functional core）：纯状态转移，不做任何 IO。
//!
//! 触发 → 取材 → 推理 → 展示/失败的完整转移在此收敛；通道发送、浮层
//! 窗口操作、日志由壳（app 的 winit handler 与组装点）执行——本模块只
//! 决策、不副作用，因此可被集成测试以公共 API 全时序驱动（分层测试
//! 的 L1 层，见 tests/pipeline.rs）。
//!
//! 代数（generation）的**唯一赋值点**是 [`TaskStateMachine::trigger`]：
//! 只有真实下发的触发才递增；回传事件按代数匹配，不匹配即陈旧丢弃。
//!
//! 两道敏感信息闸门的落点：场景闸门在 [`trigger_decision`]（触发前，
//! 拦下即不取材不占代数、不出浮层），内容闸门在
//! [`TaskStateMachine::accept_input`]（取材后、下发前，命中即丢弃这次取材
//! 回 `Idle`——不下发、不出浮层，壳只记一行 warn）。

use tokio_util::sync::CancellationToken;

use gloss_core::config::Config;
use gloss_core::guard::{self, SceneFacts, SensitiveKind, TriggerBlock};
use gloss_core::model::GlossError;
use gloss_core::model::Locale;
use gloss_core::task::{InputSource, Task, TaskInput, TaskKind, TaskOptions, TaskOutcome};

use crate::channel::{AcquireCommand, PlatformEvent};

/// 失败卡的动作按钮（错误映射表）：状态机按错误变体给出该显式
/// 给用户的出口，渲染层照画、壳执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorAction {
    /// 可重试类（网络/限流）：原样重发失败的那个任务。
    Retry,
    /// 配置/鉴权类：打开设置页（密钥、模型绑定都在那里修）。
    OpenSettings,
}

/// 应用状态机：触发 → 取材 → 推理 → 展示/失败。
///
/// 转移概要：任何可见态收到新触发（[`TaskStateMachine::trigger`]）都取
/// 消在途任务并回 `Fetching`；`Fetching` 采纳 `InputReady` 后携取消令牌
/// 下发通道③进 `Translating`——**除非内容闸门命中**，那时这次取材被丢弃、
/// 直接回 `Idle`（不下发、不出浮层）；`Translating` 收 `TaskChunk` 追加
/// 展示、收 `TaskDone` 定格 `Show`、收 `TaskFailed` 落 `Error`；收起
/// （Esc / 关闭按钮）回 `Idle`。
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
    /// 框选交互（占位，随框选遮罩落地；过渡期内无转移路径）。
    #[allow(dead_code)]
    RegionSelecting,
}

/// 浮层内容视图：状态机的可视化投影，由 `ui::popup` 按 TaskKind 分发
/// 渲染。
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
    /// 失败信息与动作出口：`action` 指出浮层该给用户的按钮（错误
    /// 映射），`None` 表示无可操作出口（重新划词即可）。
    Failed {
        /// 失败来源；面向用户的措辞由渲染层按界面语言映射。
        cause: FailureCause,
        /// 失败卡的动作按钮；随错误类别而定。
        action: Option<ErrorAction>,
    },
}

/// 失败卡的错误来源：任务链路的 [`GlossError`]，或壳层通道故障。
///
/// 状态机只记来源、不记文案——文案随界面语言变，属渲染层的产物（见
/// [`crate::i18n::ErrorText`]），不随任务冻结。
#[derive(Debug, Clone, PartialEq)]
pub enum FailureCause {
    /// 任务链路返回的错误。
    Task(GlossError),
    /// 取材通道不可用（通道②发送失败）。
    AcquireChannel,
    /// 推理通道不可用（通道③发送失败）。
    TransportChannel,
}

/// `accept_input` 的结果：下发 / 被内容闸门拦下 / 不采纳。三态而非 `Option`
/// ——「内容疑似敏感」与「陈旧丢弃」在壳侧要做不同的事（前者要记一条 warn，
/// 后者只记 debug），合并成 `None` 就分不出来了。
#[derive(Debug, Clone, PartialEq)]
pub enum InputOutcome {
    /// 任务已组装，壳经通道③下发。
    Dispatch(RunRequest),
    /// 内容疑似敏感：这次取材作废——任务不组装、不下发、浮层不露面。
    Blocked(SensitiveKind),
    /// 陈旧代数、非取材态或模态错配：不采纳，浮层与通道都不动。
    Ignored,
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
/// 直接组装——单次任务的配置从触发那一刻起就固定了（「单次任务内
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
    /// 在途推理的取消令牌：新触发时取消旧任务（唯一取消机制）。
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
    /// 后续选项都按它解析，此后本任务不再读配置。`system_locale` 是壳在
    /// 启动期读到的系统语言，供配置里的 `Language::System` 落定（配置快照
    /// 里只有三态偏好，落定需要这一份环境事实）。`scene` 是壳在触发前读到
    /// 的场景事实，供敏感场景闸门判定（见 [`trigger_decision`]）。未接线的
    /// 平台事件与被拦下的触发都返回 None 且不产生任何状态副作用。
    pub fn trigger(
        &mut self,
        event: &PlatformEvent,
        config: &Config,
        system_locale: Locale,
        scene: &SceneFacts,
    ) -> Option<AcquireCommand> {
        let kind = match trigger_decision(event, config, scene) {
            TriggerDecision::Acquire(kind) => kind,
            TriggerDecision::Disabled(_)
            | TriggerDecision::Blocked(_)
            | TriggerDecision::Unwired => return None,
        };
        let command = AcquireCommand::AcquireText {
            generation: self.generation + 1,
            kind,
        };
        // 最新触发取代在途任务：旧推理立即取消（其迟到产物经代数过滤
        // 丢弃），令牌清空等待新任务。
        if let Some(cancel) = self.current_cancel.take() {
            cancel.cancel();
        }
        self.generation += 1;
        // 新触发取代一切旧任务：连可重试的失败任务副本、待确认的任务
        // 副本一并作废（重发它们没有意义，用户已经表达了新的意图）。
        self.active_task = None;
        // kind 从命令里取（两个变体都携带），选项按同一个 kind 从**同一份**
        // 快照解析——这里是「单次任务内配置一致」的实现点。
        //
        // `CaptureRegion` 是框选取材的预留：今天 `trigger` 不会返回它
        // （`trigger_decision` 对 Region 返回 `Unwired`）。接线时必须同时让
        // `accept_input` 接纳 `TaskInput::Image`，否则任务会卡在 `Fetching`
        // 且不弹浮层（`accept_input` 只收文本）。
        self.pending = Some(PendingTask {
            kind,
            options: task_options(kind, config, system_locale),
        });
        self.state = AppState::Fetching;
        Some(command)
    }

    /// 采纳取材产物：组装 `Task` 并返回下发请求（壳经通道③发送），进入
    /// `Translating`。选项取触发时那份快照（不经参数再传配置）。
    ///
    /// 内容闸门命中时这次取材作废：任务不组装、不下发、浮层不露面，直接回
    /// `Idle`（壳据 [`InputOutcome::Blocked`] 记一行 warn 并收起浮层窗口）。
    /// **没有放行出口**——防护不交由用户控制，命中就是发送不成。
    pub fn accept_input(&mut self, generation: u64, input: TaskInput) -> InputOutcome {
        if generation != self.generation || self.state != AppState::Fetching {
            return InputOutcome::Ignored;
        }
        // 先校验模态再消费 pending：模态错配不吃掉待组装任务，同代数
        // 的后续合法 InputReady 仍可被采纳。
        let TaskInput::Text { text, hint } = input else {
            return InputOutcome::Ignored;
        };
        let Some(PendingTask { kind, options }) = self.pending.take() else {
            return InputOutcome::Ignored;
        };
        if let Some(reason) = guard::detect_sensitive(&text) {
            // 视图一并清掉：里面可能还留着上一次任务的产物卡，而它属于另
            // 一次取材（留着会被读成「这次划词的结果」）。
            self.overlay_view = None;
            self.state = AppState::Idle;
            return InputOutcome::Blocked(reason);
        }
        let task = Task {
            kind,
            input: TaskInput::Text {
                text: text.clone(),
                hint,
            },
            options,
        };
        self.overlay_view = Some(OverlayView::Streaming {
            source: text,
            body: String::new(),
        });
        self.state = AppState::Translating;
        InputOutcome::Dispatch(self.begin_run(task))
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

    /// 采纳任务失败：落 `Error` 态并展示失败信息与动作出口（错误
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
        // 故障的 fail_* 降级路径）一律清掉，retry() 自然无从发起。副本
        // 缺失时（未来取材路径若回传可重试错误）不给 Retry 出口——别摆
        // 一颗点了没反应的死按钮。
        let mut action = error_action(error);
        if action == Some(ErrorAction::Retry) {
            if self.active_task.is_none() {
                action = None;
            }
        } else {
            self.active_task = None;
        }
        self.state = AppState::Error;
        self.overlay_view = Some(OverlayView::Failed {
            cause: FailureCause::Task(error.clone()),
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
        self.overlay_view = Some(streaming_view(&task));
        self.state = AppState::Translating;
        Some(self.begin_run(task))
    }

    /// 下发一个任务的共用半边：换新取消令牌并记在途（`active_task` 留在
    /// 状态机里，失败可重试）。视图由调用方先定好——只有调用方知道这次
    /// 下发用哪个视图。
    fn begin_run(&mut self, task: Task) -> RunRequest {
        let cancel = CancellationToken::new();
        self.current_cancel = Some(cancel.clone());
        self.active_task = Some(task.clone());
        RunRequest {
            generation: self.generation,
            task,
            cancel,
        }
    }

    /// 浮层收起（Esc / 关闭按钮）即放弃在途任务：取消令牌（唯一取消机
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
            cause: FailureCause::AcquireChannel,
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
            cause: FailureCause::TransportChannel,
            action: None,
        });
    }
}

/// 错误 → 失败卡动作（映射表）：网络/限流可原样重试；鉴权、模态
/// 与配置错误都要进设置页才能修（模型绑定、密钥的修改入口在设置页）：
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

/// 一个任务下发的流式视图起点：原文照抄、正文空。重试与首次下发共用。
fn streaming_view(task: &Task) -> OverlayView {
    OverlayView::Streaming {
        source: source_text(task),
        body: String::new(),
    }
}

/// 任务原文（流式视图与重试用）：当前只有文本任务进入推理（图像取材接入
/// 像取材时随它扩展），其余模态留空。
fn source_text(task: &Task) -> String {
    match &task.input {
        TaskInput::Text { text, .. } => text.clone(),
        TaskInput::Image { .. } | TaskInput::Audio { .. } => String::new(),
    }
}

/// 一次平台事件的去向：取材，或被三类闸门之一拦下。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerDecision {
    /// 放行：该任务类型已启用且场景闸门放行，事件进入取材。
    Acquire(TaskKind),
    /// 拦下：该任务类型被设置页的任务开关停用。
    Disabled(TaskKind),
    /// 拦下：触发前场景闸门（安全输入态 / 敏感应用名单）。只拦真实触发，
    /// 设置与退出不在此列。
    Blocked(TriggerBlock),
    /// 未接线的事件（框选、设置、退出）。
    Unwired,
}

/// 一次平台事件的去向判定：任务开关与场景闸门都在这里做一次（上游据此
/// 记不同级别的日志与不同的动作）。
///
/// 顺序是「开关 → 场景」：被停用的 kind 本来就无响应，先判它才不会让
/// 日志把「无响应」说成「被防护拦下」。
pub fn trigger_decision(
    event: &PlatformEvent,
    config: &Config,
    scene: &SceneFacts,
) -> TriggerDecision {
    let kind = match event {
        PlatformEvent::HotkeyTriggered { binding } => match binding.source {
            InputSource::Selection => binding.kind,
            // 图像取材待框选路径接入后消费。
            InputSource::Region => return TriggerDecision::Unwired,
        },
        PlatformEvent::SelectionGesture { .. } => config.selection_task_kind(),
        PlatformEvent::RegionGesture { .. }
        | PlatformEvent::OpenSettingsRequested
        | PlatformEvent::QuitRequested => return TriggerDecision::Unwired,
    };
    // 停用判定只在这条路径上做一次（划词手势的任务类型已按取材源收口成
    // 文本类，见 `Config::selection_task_kind`；热键绑定携带的 kind 是逐条
    // 显式意图，不做收口）。
    if !config.is_kind_enabled(kind) {
        return TriggerDecision::Disabled(kind);
    }
    match guard::trigger_block(scene) {
        Some(block) => TriggerDecision::Blocked(block),
        None => TriggerDecision::Acquire(kind),
    }
}

/// 按配置快照解析任务选项：目标语言取配置默认；模型按 kind 从
/// `model_by_kind` 解析（缺项时 core 的出厂默认兜底）后随任务下发；prompt
/// 模板语言按 `Language` 落定（`System` 取系统语言）——三者都是引擎/模板
/// 侧的输入，随任务冻结，执行途中不再回读配置。
fn task_options(kind: TaskKind, config: &Config, system_locale: Locale) -> TaskOptions {
    TaskOptions {
        target_lang: Some(config.target_lang.clone()),
        model_override: config.resolved_model(kind).map(str::to_owned),
        prompt_locale: Some(config.language.resolve(system_locale)),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gloss_core::config::{Language, ModelBinding};
    use gloss_core::guard::{FrontApp, SceneFacts, SensitiveKind};
    use gloss_core::model::{GlossError, Lang, ScreenPoint, ScreenRect};
    use gloss_core::task::{InputHint, OutcomeStructured};

    use super::*;

    fn selection_gesture() -> PlatformEvent {
        PlatformEvent::SelectionGesture {
            pos: ScreenPoint::new(0, 0),
        }
    }

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

    fn suspicious_text() -> String {
        format!("key sk-{}", "aB3".repeat(8))
    }

    fn trigger(machine: &mut TaskStateMachine, config: &Config) -> Option<AcquireCommand> {
        machine.trigger(
            &selection_gesture(),
            config,
            Locale::Zh,
            &SceneFacts::default(),
        )
    }

    fn dispatched(outcome: InputOutcome) -> RunRequest {
        match outcome {
            InputOutcome::Dispatch(request) => request,
            other => panic!("expected a dispatch, got {other:?}"),
        }
    }

    fn blocked_by_app() -> SceneFacts {
        SceneFacts {
            secure_input: false,
            front_app: Some(FrontApp {
                bundle_id: Some("com.1password.1password".into()),
                name: None,
            }),
        }
    }

    #[test]
    fn trigger_mapping_covers_wired_events_only() {
        let mut machine = TaskStateMachine::new();
        let command =
            trigger(&mut machine, &Config::default()).expect("selection gesture must acquire");
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
                    &Config::default(),
                    Locale::Zh,
                    &SceneFacts::default()
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

    #[test]
    fn trigger_decision_separates_disabled_blocked_and_unwired_events() {
        let open = SceneFacts::default();
        let config = Config {
            default_text_kind: TaskKind::ExplainCode,
            enabled_kinds: vec![TaskKind::TranslateWord],
            ..Default::default()
        };
        assert_eq!(
            trigger_decision(&selection_gesture(), &config, &open),
            TriggerDecision::Disabled(TaskKind::ExplainCode),
            "a selection whose default task is switched off is a trigger the user can fix"
        );
        assert_eq!(
            trigger_decision(
                &PlatformEvent::HotkeyTriggered {
                    binding: gloss_core::task::HotkeyBinding {
                        trigger: "Cmd+Shift+E".into(),
                        kind: TaskKind::ExplainCode,
                        source: gloss_core::task::InputSource::Selection,
                    }
                },
                &config,
                &open
            ),
            TriggerDecision::Disabled(TaskKind::ExplainCode),
            "a disabled hotkey binding is disabled, not unwired"
        );
        assert_eq!(
            trigger_decision(
                &PlatformEvent::HotkeyTriggered {
                    binding: gloss_core::task::HotkeyBinding {
                        trigger: "Cmd+Shift+R".into(),
                        kind: TaskKind::ImageOcr,
                        source: gloss_core::task::InputSource::Region,
                    }
                },
                &config,
                &open
            ),
            TriggerDecision::Unwired,
            "region bindings stay outside the route table until the capture path lands"
        );
        assert_eq!(
            trigger_decision(
                &PlatformEvent::HotkeyTriggered {
                    binding: gloss_core::task::HotkeyBinding {
                        trigger: "Cmd+Shift+O".into(),
                        kind: TaskKind::ImageOcr,
                        source: gloss_core::task::InputSource::Selection,
                    }
                },
                &Config::default(),
                &open
            ),
            TriggerDecision::Acquire(TaskKind::ImageOcr),
            "a hotkey kind is an explicit per-binding choice: no text-kind fold applies to it"
        );
        assert_eq!(
            trigger_decision(
                &PlatformEvent::OpenSettingsRequested,
                &config,
                &blocked_by_app()
            ),
            TriggerDecision::Unwired,
            "settings is an entry point, never a gated trigger"
        );
        assert_eq!(
            trigger_decision(
                &PlatformEvent::RegionGesture {
                    rect: ScreenRect {
                        x: 0,
                        y: 0,
                        width: 10,
                        height: 10,
                    },
                },
                &config,
                &open
            ),
            TriggerDecision::Unwired,
            "the capture gesture has no acquisition path yet"
        );
        assert_eq!(
            trigger_decision(&PlatformEvent::QuitRequested, &config, &blocked_by_app()),
            TriggerDecision::Unwired,
            "quit is an exit, never a gated trigger"
        );
        assert_eq!(
            trigger_decision(&selection_gesture(), &Config::default(), &open),
            TriggerDecision::Acquire(TaskKind::TranslateWord)
        );
        assert_eq!(
            trigger_decision(&selection_gesture(), &Config::default(), &blocked_by_app()),
            TriggerDecision::Blocked(TriggerBlock::BlockedApp("com.1password.1password")),
            "an enabled kind in a sensitive app is blocked, not disabled"
        );
    }

    #[test]
    fn scene_gate_stops_the_trigger_before_acquisition() {
        let mut machine = TaskStateMachine::new();
        let secure = SceneFacts {
            secure_input: true,
            front_app: Some(FrontApp {
                bundle_id: Some("com.example.editor".into()),
                name: None,
            }),
        };
        assert!(
            machine
                .trigger(
                    &selection_gesture(),
                    &Config::default(),
                    Locale::Zh,
                    &secure
                )
                .is_none(),
            "a focused password field stops the trigger"
        );
        assert!(
            machine
                .trigger(
                    &selection_gesture(),
                    &Config::default(),
                    Locale::Zh,
                    &blocked_by_app()
                )
                .is_none(),
            "a listed frontmost app stops the trigger"
        );
        assert_eq!(
            machine.generation(),
            0,
            "a suppressed trigger consumes no generation"
        );
        assert_eq!(
            machine.state(),
            AppState::Idle,
            "no overlay, no failure card"
        );
    }

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

        let command = trigger(&mut machine, &config).expect("selection gesture must acquire");
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

        let request = dispatched(machine.accept_input(1, text_input("fn main() {}")));
        assert_eq!(request.task.kind, TaskKind::ExplainCode);
        assert_eq!(request.task.options.target_lang, Some(Lang::Ja));
        assert_eq!(
            request.task.options.model_override.as_deref(),
            Some("code-model"),
            "model must be resolved for the same kind, not the factory default"
        );
    }

    #[test]
    fn image_default_kind_falls_back_to_a_text_kind() {
        let mut machine = TaskStateMachine::new();
        let config = Config {
            default_text_kind: TaskKind::ImageOcr,
            ..Default::default()
        };

        let command = trigger(&mut machine, &config).expect("selection gesture must acquire");
        assert!(matches!(
            command,
            AcquireCommand::AcquireText {
                kind: TaskKind::TranslateWord,
                ..
            }
        ));
    }

    #[test]
    fn disabled_kinds_are_not_acquired_and_consume_no_generation() {
        let mut machine = TaskStateMachine::new();
        let config = Config {
            enabled_kinds: vec![
                gloss_core::config::ALL_KINDS[0],
                gloss_core::config::ALL_KINDS[1],
            ],
            ..Default::default()
        };

        assert!(trigger(&mut machine, &config).is_some());
        assert_eq!(machine.generation(), 1);

        let binding = gloss_core::task::HotkeyBinding {
            trigger: "Cmd+Shift+E".into(),
            kind: gloss_core::config::ALL_KINDS[2],
            source: gloss_core::task::InputSource::Selection,
        };
        assert!(
            machine
                .trigger(
                    &PlatformEvent::HotkeyTriggered { binding },
                    &config,
                    Locale::Zh,
                    &SceneFacts::default()
                )
                .is_none(),
            "disabled kind must not acquire via hotkey"
        );
        assert_eq!(
            machine.generation(),
            1,
            "disabled trigger must not consume a generation"
        );

        let disabled_default = Config {
            default_text_kind: gloss_core::config::ALL_KINDS[2],
            enabled_kinds: Vec::new(),
            ..Default::default()
        };
        assert!(
            trigger(&mut machine, &disabled_default).is_none(),
            "disabled selection kind must not acquire"
        );
    }

    #[test]
    fn prompt_locale_follows_config_language_and_the_system() {
        let mut explicit = TaskStateMachine::new();
        let chosen = Config {
            language: Language::En,
            ..Default::default()
        };
        explicit
            .trigger(
                &selection_gesture(),
                &chosen,
                Locale::Zh,
                &SceneFacts::default(),
            )
            .expect("trigger");
        let request = dispatched(explicit.accept_input(1, text_input("hello")));
        assert_eq!(
            request.task.options.prompt_locale,
            Some(Locale::En),
            "an explicit choice ignores the injected system language"
        );

        let mut following = TaskStateMachine::new();
        let factory = Config::default();
        assert_eq!(factory.language, Language::System);
        following
            .trigger(
                &selection_gesture(),
                &factory,
                Locale::En,
                &SceneFacts::default(),
            )
            .expect("trigger");
        let request = dispatched(following.accept_input(1, text_input("hello")));
        assert_eq!(
            request.task.options.prompt_locale,
            Some(Locale::En),
            "follow-the-system takes the injected system language"
        );
    }

    #[test]
    fn options_freeze_at_trigger_time() {
        let mut machine = TaskStateMachine::new();
        let before = Config {
            target_lang: Lang::Ja,
            language: Language::En,
            ..Default::default()
        };
        let after = Config {
            target_lang: Lang::Ko,
            language: Language::Zh,
            ..Default::default()
        };

        trigger(&mut machine, &before).expect("trigger");
        let request = dispatched(machine.accept_input(1, text_input("hello")));
        assert_eq!(
            request.task.options.target_lang,
            Some(Lang::Ja),
            "in-flight task must keep the snapshot taken at trigger"
        );
        assert_eq!(
            request.task.options.prompt_locale,
            Some(Locale::En),
            "the prompt locale is frozen with the rest of the options"
        );
        trigger(&mut machine, &after).expect("second trigger");
        let request = dispatched(machine.accept_input(2, text_input("world")));
        assert_eq!(request.task.options.target_lang, Some(Lang::Ko));
        assert_eq!(request.task.options.prompt_locale, Some(Locale::Zh));
    }

    #[test]
    fn accept_input_yields_run_request_and_guards_state() {
        let mut machine = TaskStateMachine::new();
        trigger(&mut machine, &Config::default()).expect("trigger");

        let request = dispatched(machine.accept_input(1, text_input("hello")));
        assert_eq!(request.generation, 1);
        assert_eq!(request.task.kind, TaskKind::TranslateWord);
        assert_eq!(machine.state(), AppState::Translating);
        assert!(machine.current_cancel().is_some());

        assert!(matches!(
            machine.accept_input(1, text_input("again")),
            InputOutcome::Ignored
        ));
    }

    #[test]
    fn image_input_for_text_kind_is_rejected() {
        let mut machine = TaskStateMachine::new();
        trigger(&mut machine, &Config::default()).expect("trigger");
        assert!(matches!(
            machine.accept_input(
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
            ),
            InputOutcome::Ignored
        ));
    }

    #[test]
    fn hide_abandons_inflight_and_drops_late_events() {
        let mut machine = TaskStateMachine::new();
        trigger(&mut machine, &Config::default()).expect("trigger");
        let request = dispatched(machine.accept_input(1, text_input("hello")));
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

    #[test]
    fn failed_guard_matches_fetching_and_translating_only() {
        let mut machine = TaskStateMachine::new();
        trigger(&mut machine, &Config::default()).expect("trigger");
        assert!(machine.accept_failed(1, &GlossError::SelectionUnavailable));
        assert_eq!(machine.state(), AppState::Error);

        machine.hide_overlay();
        assert!(
            !machine.accept_failed(1, &GlossError::EngineNetwork),
            "hidden error card must not be resurrected"
        );
    }

    #[test]
    fn modality_mismatch_preserves_pending_task() {
        let mut machine = TaskStateMachine::new();
        trigger(&mut machine, &Config::default()).expect("trigger");
        assert!(matches!(
            machine.accept_input(
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
            ),
            InputOutcome::Ignored
        ));
        assert!(
            matches!(
                machine.accept_input(1, text_input("第二次")),
                InputOutcome::Dispatch(_)
            ),
            "pending kind must survive a modality mismatch"
        );
    }

    #[test]
    fn transport_failure_lands_in_error() {
        let mut machine = TaskStateMachine::new();
        trigger(&mut machine, &Config::default()).expect("trigger");
        let request = dispatched(machine.accept_input(1, text_input("x")));
        machine.fail_transport(request.generation);
        assert_eq!(machine.state(), AppState::Error);
        assert!(machine.current_cancel().is_none());
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed { action: None, .. })
        ));
        assert!(machine.retry().is_none());
    }

    #[test]
    fn retryable_failure_keeps_task_and_retry_redispatches_it() {
        let mut machine = TaskStateMachine::new();
        trigger(&mut machine, &Config::default()).expect("trigger");
        let original = dispatched(machine.accept_input(1, text_input("hello")));
        assert!(machine.accept_failed(1, &GlossError::EngineNetwork));

        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed {
                cause: FailureCause::Task(GlossError::EngineNetwork),
                action: Some(ErrorAction::Retry),
            })
        ));

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

    #[test]
    fn error_actions_follow_the_mapping_table() {
        let mut machine = TaskStateMachine::new();
        trigger(&mut machine, &Config::default()).expect("trigger");
        dispatched(machine.accept_input(1, text_input("x")));

        assert!(
            machine.accept_failed(1, &GlossError::EngineRateLimited),
            "rate limited is retryable"
        );
        assert!(machine.retry().is_some());

        trigger(&mut machine, &Config::default()).expect("trigger");
        dispatched(machine.accept_input(2, text_input("x")));
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

        trigger(&mut machine, &Config::default()).expect("trigger");
        dispatched(machine.accept_input(3, text_input("x")));
        assert!(machine.accept_failed(3, &GlossError::UnsupportedModality));
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed {
                action: Some(ErrorAction::OpenSettings),
                ..
            })
        ));

        trigger(&mut machine, &Config::default()).expect("trigger");
        dispatched(machine.accept_input(4, text_input("x")));
        assert!(machine.accept_failed(
            4,
            &GlossError::Config("no model configured for this task kind".into())
        ));
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed {
                action: Some(ErrorAction::OpenSettings),
                ..
            })
        ));
    }

    #[test]
    fn new_trigger_and_hide_supersede_the_retry_task() {
        let mut machine = TaskStateMachine::new();
        trigger(&mut machine, &Config::default()).expect("trigger");
        dispatched(machine.accept_input(1, text_input("x")));
        assert!(machine.accept_failed(1, &GlossError::EngineNetwork));

        trigger(&mut machine, &Config::default()).expect("trigger");
        assert!(machine.retry().is_none(), "new trigger supersedes retry");

        dispatched(machine.accept_input(2, text_input("y")));
        assert!(machine.accept_failed(2, &GlossError::EngineNetwork));
        machine.hide_overlay();
        assert_eq!(machine.state(), AppState::Idle);
        assert!(machine.retry().is_none(), "hide drops the retry task");
    }

    #[test]
    fn suspicious_input_is_dropped_without_a_task_or_a_card() {
        let mut machine = TaskStateMachine::new();
        trigger(&mut machine, &Config::default()).expect("trigger");
        dispatched(machine.accept_input(1, text_input("上一次的普通文本")));
        assert!(machine.accept_done(1, plain_outcome("上一次的产物")));

        trigger(&mut machine, &Config::default()).expect("second trigger");
        let outcome = machine.accept_input(2, text_input(&suspicious_text()));
        assert!(
            matches!(outcome, InputOutcome::Blocked(SensitiveKind::Token)),
            "a selection that looks like a token must be refused, got {outcome:?}"
        );
        assert_eq!(
            machine.state(),
            AppState::Idle,
            "the refused fetch leaves nothing behind"
        );
        assert!(
            machine.overlay_view().is_none(),
            "nothing is shown for a refused fetch — no card, no question, and the previous card is cleared too"
        );
        assert!(
            machine.current_cancel().is_none(),
            "nothing was dispatched, so there is no token to cancel"
        );
    }

    #[test]
    fn blocked_input_is_not_redispatched_by_any_later_path() {
        let mut machine = TaskStateMachine::new();
        trigger(&mut machine, &Config::default()).expect("trigger");
        machine.accept_input(1, text_input("card 4111 1111 1111 1111"));

        assert!(
            machine.retry().is_none(),
            "a refused fetch is not a retryable failure"
        );
        assert!(matches!(
            machine.accept_input(1, text_input("second arrival")),
            InputOutcome::Ignored
        ));
        assert!(
            !machine.accept_chunk(1, "迟到的正文".into())
                && !machine.accept_done(1, plain_outcome("迟到的产物"))
                && !machine.accept_failed(1, &GlossError::EngineNetwork),
            "no product of a refused fetch may be adopted either"
        );

        trigger(&mut machine, &Config::default()).expect("trigger");
        assert_eq!(machine.generation(), 2, "the next trigger starts over");
        assert!(matches!(
            machine.accept_input(2, text_input(&suspicious_text())),
            InputOutcome::Blocked(_)
        ));
        assert_eq!(machine.state(), AppState::Idle);
    }

    #[test]
    fn ordinary_input_still_passes_the_content_gate() {
        let mut machine = TaskStateMachine::new();
        trigger(&mut machine, &Config::default()).expect("trigger");

        assert!(
            matches!(
                machine.accept_input(1, text_input("今天下午三点开会")),
                InputOutcome::Dispatch(_)
            ),
            "the gate only fires on the high-confidence patterns"
        );
        assert_eq!(machine.state(), AppState::Translating);
    }
}
