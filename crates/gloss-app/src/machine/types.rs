//! 状态机的状态词汇表：稳定态、浮层视图、失败语义与在途探测的载荷类型。
//! 决策逻辑归 `machine` 本体与 `policy`，这里只有数据形状。

use tokio_util::sync::CancellationToken;

use gloss_core::guard::SensitiveKind;
use gloss_core::model::GlossError;
use gloss_core::task::{TaskInput, TaskKind, TaskOptions, TaskOutcome};

/// 失败卡的动作按钮（错误映射表）：状态机按错误变体给出该显式
/// 给用户的出口，渲染层照画、壳执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorAction {
    /// 可重试类（网络/限流）：原样重发失败的那个任务。
    Retry,
    /// 配置/鉴权类：打开设置页（密钥、模型绑定都在那里修）。
    OpenSettings,
}

/// 四个稳定态：探测不进本枚举（探测段在状态机之外）；提交从任意态直接进
/// `Translating` 并取消旧任务，收起回 `Idle`。它不是独立字段——状态由
/// [`TaskStateMachine::overlay_view`] 唯一决定，见
/// [`TaskStateMachine::state`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppState {
    #[default]
    Idle,
    Translating,
    Show,
    Error,
}

/// 浮层内容视图：状态机的可视化投影，由 `ui::popup` 按 TaskKind 分发
/// 渲染。
#[derive(Debug, Clone, PartialEq)]
pub enum OverlayView {
    /// 推理中：原文 + 已到达的原始回复。`raw` 是模型原始 JSON 流（`note` 的
    /// 渐进提取在渲染层）；`classified` 在 LLM 层分类前为 `None`，
    /// `accept_classified` 到达后精化（代码解释从分类帧起按代码排版）；
    /// `repairing` 置位时页脚切「重新注解」（无横幅无计数）。
    Streaming {
        source: String,
        /// 模型原始回复的累积（含 JSON 结构，提取归渲染层）。
        raw: String,
        /// LLM 层回传的任务类型；分类前为 `None`。
        classified: Option<TaskKind>,
        /// 修复轮进行中（`accept_repairing` 置位）。
        repairing: bool,
    },
    /// 产物卡：按 `TaskKind` 精排或展示 markdown 注文。`source` 是本次任务的
    /// 原文，供注疏排布的「经」位；代码语言由渲染层从 `structured.code_language`
    /// 现算（唯一来源是产物的 LLM 判定），`degraded` 时渲染降级卡。
    Outcome {
        source: String,
        outcome: TaskOutcome,
    },
    /// 失败信息与动作出口：`action` 为 `None` 表示没有按钮可给（重新划词即可）。
    Failed {
        cause: FailureCause,
        action: Option<ErrorAction>,
    },
}

/// 失败卡的错误来源。状态机只记来源不记文案——文案随界面语言变，属渲染层
/// （`crate::ui::i18n::Text::for_error`），不随任务冻结。
#[derive(Debug, Clone, PartialEq)]
pub enum FailureCause {
    Task(GlossError),
    /// 推理通道不可用（通道③发送失败）。
    TransportChannel,
}

/// `commit_probe` 的结果：下发 / 被内容闸门拦下 / 不采纳。三态而非 `Option`
/// ——「内容疑似敏感」与「陈旧丢弃」在壳侧要做不同的事（前者记 warn，后者
/// 只记 debug），合并成 `None` 就分不出来了。
#[derive(Debug, Clone, PartialEq)]
pub enum InputOutcome {
    /// 任务已组装，壳经通道③下发。
    Dispatch(RunRequest),
    /// 内容疑似敏感：探测丢弃，当前显示保留（它属于上一个会话）。
    Blocked(SensitiveKind),
    /// 陈旧探测编号或模态错配：不采纳，浮层与通道都不动。
    Ignored,
}

/// 失败处置的结果：弹失败卡 / 静默丢弃 / 不采纳。三态而非 bool
/// ——「静默丢弃」要留排查日志（划词探测失败的唯一痕迹），「陈旧丢弃」只记
/// debug，合并后就分不出来了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureOutcome {
    /// 已落 `Error` 态，浮层弹失败卡（壳记 warn）。
    Shown,
    /// 划词探测遇「无选区可读」：纯误滑——探测丢弃、不弹卡，状态机与当前
    /// 显示一律不动（壳只记一条带前台应用的 info）。
    SilentlyDropped,
    /// 探测编号不符、代数陈旧或已隐藏：不采纳，浮层与状态都不动（壳记 info）。
    Ignored,
}

/// `commit_probe` 采纳取材产物后的下发请求：壳把它经通道③发送。
#[derive(Debug, Clone, PartialEq)]
pub struct RunRequest {
    /// 请求代数，与触发同值。
    pub generation: u64,
    /// 取材输入，原样转发给 LLM 层。
    pub input: TaskInput,
    /// 触发时按配置快照冻结的任务选项（见 `PendingTask`），执行途中
    /// 不再回读配置。
    pub options: TaskOptions,
    /// 随任务下发的取消令牌（App 侧同时留存，新触发时取消）。
    pub cancel: CancellationToken,
}

/// 探测时定下的任务选项：一次配置快照解析出全部参数，产物到达（提交）时
/// 随请求下发——单次任务的配置从探测那一刻起就固定了，取材途中换配置不会
/// 让同一个任务用上两个版本的参数。任务类型不在其中：kind 由 LLM 层在执行
/// 前分类。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PendingOptions(pub(super) TaskOptions);

/// 在途探测的取材模态：决定提交时接受的产物形状，也决定失败时的静默名单。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProbeSource {
    Selection,
    PasteboardImage,
}

/// 在途的探测（划词或剪贴板图片）：同一时刻至多一个，新探测整体替换它；
/// 产物到达时经 [`TaskStateMachine::commit_probe`] 才接管状态。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ProbeTask {
    /// 与代数同一计数器分配，提交时提升为代数。
    pub(super) id: u64,
    pub(super) pending: PendingOptions,
    /// 决定提交时接受的产物形状与失败时的静默名单。
    pub(super) source: ProbeSource,
}
