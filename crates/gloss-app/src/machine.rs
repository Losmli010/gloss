//! 任务状态机与浮层视图（functional core）：纯状态转移，不做任何 IO。
//!
//! 触发 → 取材 → 推理 → 展示/失败的完整转移在此收敛；通道发送、浮层
//! 窗口操作、日志由壳（app 的 winit handler 与组装点）执行——本模块只
//! 决策、不副作用，因此可被集成测试以公共 API 全时序驱动（分层测试
//! 的 L1 层，见 tests/pipeline.rs）。
//!
//! 代数（generation）的**唯一赋值点**是 [`TaskStateMachine::trigger`]：
//! 只有真实下发的触发才递增；回传事件按代数匹配，不匹配即陈旧丢弃。

use tokio_util::sync::CancellationToken;

use gloss_core::config::Config;
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
/// 下发通道③进 `Translating`；`Translating` 收 `TaskChunk` 追加展示、
/// 收 `TaskDone` 定格 `Show`、收 `TaskFailed` 落 `Error`；收起（Esc /
/// 关闭按钮）回 `Idle`。
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
    /// 里只有三态偏好，落定需要这一份环境事实）。未接线的平台事件返回 None
    /// 且不产生任何状态副作用。
    pub fn trigger(
        &mut self,
        event: &PlatformEvent,
        config: &Config,
        system_locale: Locale,
    ) -> Option<AcquireCommand> {
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
        // `CaptureRegion` 是框选取材的预留：今天 `trigger` 不会返回它
        // （`acquire_command_for` 对 Region 返回 None）。接线时必须同时让
        // `accept_input` 接纳 `TaskInput::Image`，否则任务会卡在 `Fetching`
        // 且不弹浮层（`accept_input` 只收文本）。
        let kind = match &command {
            AcquireCommand::AcquireText { kind, .. }
            | AcquireCommand::CaptureRegion { kind, .. } => *kind,
        };
        self.pending = Some(PendingTask {
            kind,
            options: task_options(kind, config, system_locale),
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

/// 任务原文（流式视图与重试用）：当前只有文本任务进入推理（图像取材接入
/// 像取材时随它扩展），其余模态留空。
fn source_text(task: &Task) -> String {
    match &task.input {
        TaskInput::Text { text, .. } => text.clone(),
        TaskInput::Image { .. } | TaskInput::Audio { .. } => String::new(),
    }
}

/// 一次平台事件的去向：进取材，或被任务开关拦下。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerRoute {
    /// 放行：该任务类型已启用，事件进入取材。
    Allowed(TaskKind),
    /// 拦下：该任务类型被设置页的任务开关停用。
    Disabled(TaskKind),
}

/// 事件若是一次真实触发，给出它的去向；未接线的事件（框选、设置、退出）
/// 返回 `None`。
///
/// 停用判定只在本函数里做一次：取材映射按 `Allowed` 放行，壳按 `Disabled`
/// 记一条 warn——被任务开关停用的触发是用户能自己修的配置问题，与「事件
/// 还没接线」不是一类，日志级别因此不同（见 [`AcquireCommand`] 的两个
/// `None` 出口）。
///
/// 划词手势的任务类型来自配置的 `default_text_kind`，并按取材源收口成文本
/// 类——配置写成图像 kind 时不送进引擎挨模态校验（见
/// `Config::selection_task_kind`）；热键绑定携带的 kind 是逐条显式意图，不
/// 做收口（只看开关）。
pub fn trigger_route(event: &PlatformEvent, config: &Config) -> Option<TriggerRoute> {
    let kind = match event {
        PlatformEvent::HotkeyTriggered { binding } => match binding.source {
            InputSource::Selection => binding.kind,
            // 图像取材待框选路径接入后消费。
            InputSource::Region => return None,
        },
        PlatformEvent::SelectionGesture { .. } => config.selection_task_kind(),
        PlatformEvent::RegionGesture { .. }
        | PlatformEvent::OpenSettingsRequested
        | PlatformEvent::QuitRequested => return None,
    };
    Some(if config.is_kind_enabled(kind) {
        TriggerRoute::Allowed(kind)
    } else {
        TriggerRoute::Disabled(kind)
    })
}

/// 平台事件 → 取材命令的映射：每次真实触发占用一个新代数；未接线的平台
/// 事件与停用的 kind 返回 None（判定见 [`trigger_route`]）。
fn acquire_command_for(
    event: &PlatformEvent,
    generation: u64,
    config: &Config,
) -> Option<AcquireCommand> {
    match trigger_route(event, config)? {
        TriggerRoute::Allowed(kind) => Some(AcquireCommand::AcquireText { generation, kind }),
        TriggerRoute::Disabled(_) => None,
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

    #[test]
    fn trigger_mapping_covers_wired_events_only() {
        let mut machine = TaskStateMachine::new();
        let command = machine
            .trigger(&selection_gesture(), &Config::default(), Locale::Zh)
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
                    &Config::default(),
                    Locale::Zh
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
    fn trigger_route_separates_disabled_triggers_from_unwired_events() {
        let config = Config {
            default_text_kind: TaskKind::ExplainCode,
            enabled_kinds: vec![TaskKind::TranslateWord],
            ..Default::default()
        };
        assert_eq!(
            trigger_route(&selection_gesture(), &config),
            Some(TriggerRoute::Disabled(TaskKind::ExplainCode)),
            "a selection whose default task is switched off is a trigger the user can fix"
        );
        assert_eq!(
            trigger_route(
                &PlatformEvent::HotkeyTriggered {
                    binding: gloss_core::task::HotkeyBinding {
                        trigger: "Cmd+Shift+E".into(),
                        kind: TaskKind::ExplainCode,
                        source: gloss_core::task::InputSource::Selection,
                    }
                },
                &config
            ),
            Some(TriggerRoute::Disabled(TaskKind::ExplainCode)),
            "a disabled hotkey binding is disabled, not unwired"
        );
        assert_eq!(
            trigger_route(
                &PlatformEvent::HotkeyTriggered {
                    binding: gloss_core::task::HotkeyBinding {
                        trigger: "Cmd+Shift+R".into(),
                        kind: TaskKind::ImageOcr,
                        source: gloss_core::task::InputSource::Region,
                    }
                },
                &config
            ),
            None,
            "region bindings stay outside the route table until the capture path lands"
        );
        assert_eq!(
            trigger_route(
                &PlatformEvent::HotkeyTriggered {
                    binding: gloss_core::task::HotkeyBinding {
                        trigger: "Cmd+Shift+O".into(),
                        kind: TaskKind::ImageOcr,
                        source: gloss_core::task::InputSource::Selection,
                    }
                },
                &Config::default()
            ),
            Some(TriggerRoute::Allowed(TaskKind::ImageOcr)),
            "a hotkey kind is an explicit per-binding choice: no text-kind fold applies to it"
        );
        assert_eq!(
            trigger_route(&PlatformEvent::OpenSettingsRequested, &config),
            None,
            "settings is an entry point, not a trigger"
        );
        assert_eq!(
            trigger_route(
                &PlatformEvent::RegionGesture {
                    rect: ScreenRect {
                        x: 0,
                        y: 0,
                        width: 10,
                        height: 10,
                    },
                },
                &config
            ),
            None,
            "the capture gesture has no acquisition path yet"
        );
        assert_eq!(
            trigger_route(&PlatformEvent::QuitRequested, &config),
            None,
            "quit is an exit, not a trigger"
        );
        assert_eq!(
            trigger_route(&selection_gesture(), &Config::default()),
            Some(TriggerRoute::Allowed(TaskKind::TranslateWord))
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

        let command = machine
            .trigger(&selection_gesture(), &config, Locale::Zh)
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

    #[test]
    fn image_default_kind_falls_back_to_a_text_kind() {
        let mut machine = TaskStateMachine::new();
        let config = Config {
            default_text_kind: TaskKind::ImageOcr,
            ..Default::default()
        };

        let command = machine
            .trigger(&selection_gesture(), &config, Locale::Zh)
            .expect("selection gesture must acquire");
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

        assert!(
            machine
                .trigger(&selection_gesture(), &config, Locale::Zh)
                .is_some()
        );
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
                    Locale::Zh
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
            machine
                .trigger(&selection_gesture(), &disabled_default, Locale::Zh)
                .is_none(),
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
            .trigger(&selection_gesture(), &chosen, Locale::Zh)
            .expect("trigger");
        let request = explicit
            .accept_input(1, text_input("hello"))
            .expect("input should be accepted");
        assert_eq!(
            request.task.options.prompt_locale,
            Some(Locale::En),
            "an explicit choice ignores the injected system language"
        );

        let mut following = TaskStateMachine::new();
        let factory = Config::default();
        assert_eq!(factory.language, Language::System);
        following
            .trigger(&selection_gesture(), &factory, Locale::En)
            .expect("trigger");
        let request = following
            .accept_input(1, text_input("hello"))
            .expect("input should be accepted");
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

        machine
            .trigger(&selection_gesture(), &before, Locale::Zh)
            .expect("trigger");
        let request = machine
            .accept_input(1, text_input("hello"))
            .expect("input should be accepted");
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
        machine
            .trigger(&selection_gesture(), &after, Locale::Zh)
            .expect("second trigger");
        let request = machine
            .accept_input(2, text_input("world"))
            .expect("input should be accepted");
        assert_eq!(request.task.options.target_lang, Some(Lang::Ko));
        assert_eq!(request.task.options.prompt_locale, Some(Locale::Zh));
    }

    #[test]
    fn accept_input_yields_run_request_and_guards_state() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&selection_gesture(), &Config::default(), Locale::Zh)
            .expect("trigger");

        let request = machine
            .accept_input(1, text_input("hello"))
            .expect("input should be accepted");
        assert_eq!(request.generation, 1);
        assert_eq!(request.task.kind, TaskKind::TranslateWord);
        assert_eq!(machine.state(), AppState::Translating);
        assert!(machine.current_cancel().is_some());

        assert!(machine.accept_input(1, text_input("again")).is_none());
    }

    #[test]
    fn image_input_for_text_kind_is_rejected() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&selection_gesture(), &Config::default(), Locale::Zh)
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

    #[test]
    fn hide_abandons_inflight_and_drops_late_events() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&selection_gesture(), &Config::default(), Locale::Zh)
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

    #[test]
    fn failed_guard_matches_fetching_and_translating_only() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&selection_gesture(), &Config::default(), Locale::Zh)
            .expect("trigger");
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
        machine
            .trigger(&selection_gesture(), &Config::default(), Locale::Zh)
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

    #[test]
    fn transport_failure_lands_in_error() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&selection_gesture(), &Config::default(), Locale::Zh)
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

    #[test]
    fn retryable_failure_keeps_task_and_retry_redispatches_it() {
        let mut machine = TaskStateMachine::new();
        machine
            .trigger(&selection_gesture(), &Config::default(), Locale::Zh)
            .expect("trigger");
        let original = machine
            .accept_input(1, text_input("hello"))
            .expect("accepted");
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
        machine
            .trigger(&selection_gesture(), &Config::default(), Locale::Zh)
            .expect("trigger");
        machine.accept_input(1, text_input("x")).expect("accepted");

        assert!(
            machine.accept_failed(1, &GlossError::EngineRateLimited),
            "rate limited is retryable"
        );
        assert!(machine.retry().is_some());

        machine
            .trigger(&selection_gesture(), &Config::default(), Locale::Zh)
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
            .trigger(&selection_gesture(), &Config::default(), Locale::Zh)
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

        machine
            .trigger(&selection_gesture(), &Config::default(), Locale::Zh)
            .expect("trigger");
        machine.accept_input(4, text_input("x")).expect("accepted");
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
        machine
            .trigger(&selection_gesture(), &Config::default(), Locale::Zh)
            .expect("trigger");
        machine.accept_input(1, text_input("x")).expect("accepted");
        assert!(machine.accept_failed(1, &GlossError::EngineNetwork));

        machine
            .trigger(&selection_gesture(), &Config::default(), Locale::Zh)
            .expect("trigger");
        assert!(machine.retry().is_none(), "new trigger supersedes retry");

        machine.accept_input(2, text_input("y")).expect("accepted");
        assert!(machine.accept_failed(2, &GlossError::EngineNetwork));
        machine.hide_overlay();
        assert_eq!(machine.state(), AppState::Idle);
        assert!(machine.retry().is_none(), "hide drops the retry task");
    }
}
