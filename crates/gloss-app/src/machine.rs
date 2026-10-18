//! 任务状态机与浮层视图（functional core）：纯状态转移，不做 IO；通道②（主→事件线程）③（主→tokio）
//! 由壳发送，①（平台→主）④（回传→主）由壳消费，浮层显隐与日志亦在壳侧。两段式「探测—提交」：
//! 探测只领编号、冻结任务选项，不改状态也不取消在途任务；产物到达才提交——编号提升为代数、
//! 旧任务取消让位、视图换流式卡。`generation` 只在提交路径赋值（`commit_probe` 置位，
//! `begin_run` 随请求携带），回传按它过滤陈旧。敏感闸门两处：场景闸门在 `trigger_decision`（触发前），内容闸门在
//! `commit_probe` 的文本分支（下发前，命中保留当前显示，图像没有可扫描文本故不设闸门）。

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use gloss_core::config::Config;
use gloss_core::guard::{self, SceneFacts, SensitiveKind, TriggerBlock};
use gloss_core::model::GlossError;
use gloss_core::model::Locale;
use gloss_core::task::{TaskInput, TaskKind, TaskOptions, TaskOutcome};

use crate::channel::{AcquireCommand, Event, PlatformEvent};

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
struct PendingOptions(TaskOptions);

/// 在途探测的取材模态：决定提交时接受的产物形状，也决定失败时的静默名单。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeSource {
    Selection,
    PasteboardImage,
}

/// 在途的探测（划词或剪贴板图片）：同一时刻至多一个，新探测整体替换它；
/// 产物到达时经 [`TaskStateMachine::commit_probe`] 才接管状态。
#[derive(Debug, Clone, PartialEq)]
struct ProbeTask {
    /// 与代数同一计数器分配，提交时提升为代数。
    id: u64,
    pending: PendingOptions,
    /// 决定提交时接受的产物形状与失败时的静默名单。
    source: ProbeSource,
}

#[derive(Debug, Default)]
pub struct TaskStateMachine {
    /// 代数与探测编号共用的单调计数器：两路探测都从这里领号，永不复用。
    next_id: u64,
    /// 当前已提交会话的代数：`accept_chunk`/`accept_done`/`accept_failed`
    /// 的陈旧过滤基准。探测编号在提交时才提升为代数。
    generation: u64,
    /// 在途的探测（划词或剪贴板图片，同一时刻至多一个）。新探测与收起都会
    /// 替换或清掉它。
    probe: Option<ProbeTask>,
    /// 在途推理的取消令牌：提交与收起时取消（唯一取消机制）。只在
    /// `Translating` 存在——完成、失败与收起都清空，重试时换新令牌。
    current_cancel: Option<CancellationToken>,
    /// 当前任务的请求副本（input + options）：推理期间随行，可重试失败后
    /// 留在 Error 态供 [`TaskStateMachine::retry`] 原样重发；完成、隐藏与
    /// 不可重试失败即清。
    active_request: Option<(TaskInput, TaskOptions)>,
    /// 当前会话附带的图像（`TaskInput::Image` 的字节 Arc，提交时零拷贝
    /// 留存）：经注疏卡「经」位的渲染源，随新任务置定、收起清空。不进
    /// 视图变体——图像只在图像任务存在，视图形状不随它分叉。
    attached_image: Option<Arc<[u8]>>,
    /// 当前浮层的内容视图；`None` 时浮层显示渲染自检卡，即 `Idle`。
    /// 四个稳定态都由它唯一决定——机器不另存状态字段，「状态↔视图」
    /// 不存在两个可以分别赋值的真值源。
    overlay_view: Option<OverlayView>,
}

impl TaskStateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 在途探测的编号（划词或剪贴板图片）；无探测时为 `None`。壳据此把
    /// 通道④的回传分流到探测提交段（探测编号在提交时才提升为代数，不能
    /// 按代数匹配）。
    pub fn probe_id(&self) -> Option<u64> {
        self.probe.as_ref().map(|probe| probe.id)
    }

    /// 当前稳定态：视图的投影。机器只存 [`Self::overlay_view`]，
    /// `Translating ⟺ Streaming`、`Show ⟺ Outcome`、`Error ⟺ Failed`、
    /// `Idle ⟺ None` 因此是定义而非约定——不存在两个字段对不上的中间态。
    pub fn state(&self) -> AppState {
        match &self.overlay_view {
            None => AppState::Idle,
            Some(OverlayView::Streaming { .. }) => AppState::Translating,
            Some(OverlayView::Outcome { .. }) => AppState::Show,
            Some(OverlayView::Failed { .. }) => AppState::Error,
        }
    }

    pub fn overlay_view(&self) -> Option<&OverlayView> {
        self.overlay_view.as_ref()
    }

    /// 当前会话附带的图像（渲染「经」位用）；非图像任务或无会话时为
    /// `None`。图像字节不进视图变体，渲染层经这里取用——经位布局开关
    /// 就是它的有无，不依赖分类结果的到达时序。
    pub fn attached_image(&self) -> Option<&Arc<[u8]>> {
        self.attached_image.as_ref()
    }

    /// 在途任务的取消令牌（测试观察口）。
    #[cfg(test)]
    pub fn current_cancel(&self) -> Option<&CancellationToken> {
        self.current_cancel.as_ref()
    }

    /// 划词探测入口（探测段）：领探测编号、冻结任务选项并返回取材命令——
    /// **不改状态、不换视图、不取消在途任务**，已显示的内容与在途推理全程
    /// 无感，误滑因此零干扰。产物到达走 [`Self::commit_probe`]，失败走
    /// [`Self::commit_probe_failed`]；新探测替换旧探测（旧探测的迟到产物经
    /// 编号过滤丢弃）。`config` 是壳在事件起手处取的快照，`system_locale`
    /// 供 `Language::System` 落定，`scene` 供场景闸门判定。未接线或被闸门
    /// 拦下的手势返回 `None` 且无副作用。
    pub fn begin_selection_probe(
        &mut self,
        event: &PlatformEvent,
        config: &Config,
        system_locale: Locale,
        scene: &SceneFacts,
    ) -> Option<AcquireCommand> {
        if !matches!(event, PlatformEvent::SelectionGesture { .. }) {
            return None;
        }
        match trigger_decision(event, scene) {
            TriggerDecision::Acquire => {}
            TriggerDecision::Blocked(_)
            | TriggerDecision::SelfSuppressed
            | TriggerDecision::Unwired => return None,
        }
        let id = self.next_id + 1;
        self.next_id += 1;
        self.probe = Some(ProbeTask {
            id,
            pending: PendingOptions(task_options(config, system_locale)),
            source: ProbeSource::Selection,
        });
        Some(AcquireCommand::AcquireText { generation: id })
    }

    /// 剪贴板图片探测入口（探测段）：镜像 [`Self::begin_selection_probe`]，
    /// 只领编号、冻结选项并返回读图命令。图像内容不在①携带，取材命令②再到
    /// 事件线程读，①与②之间被覆盖的竞态由取材侧静默兜底。场景闸门照常生效，
    /// 但不判自身前台——剪贴板观察没有「连拖自己浮层」的误触面；本应用兜底
    /// 取材写下的剪贴板变化由平台侧按轮次计数归因吞掉
    /// （[`gloss_platform::clipboard::WatchRoundFlag`]）。
    pub fn begin_pasteboard_probe(
        &mut self,
        event: &PlatformEvent,
        config: &Config,
        system_locale: Locale,
        scene: &SceneFacts,
    ) -> Option<AcquireCommand> {
        if !matches!(event, PlatformEvent::PasteboardImageObserved) {
            return None;
        }
        match trigger_decision(event, scene) {
            TriggerDecision::Acquire => {}
            TriggerDecision::Blocked(_)
            | TriggerDecision::SelfSuppressed
            | TriggerDecision::Unwired => return None,
        }
        let id = self.next_id + 1;
        self.next_id += 1;
        self.probe = Some(ProbeTask {
            id,
            pending: PendingOptions(task_options(config, system_locale)),
            source: ProbeSource::PasteboardImage,
        });
        Some(AcquireCommand::AcquireClipboardImage { generation: id })
    }

    /// 提交探测（划词与剪贴板图片共用）：编号提升为代数，旧会话让位（取消
    /// 在途推理、作废请求副本，迟到的旧产物经代数过滤丢弃），视图整卡换流式
    /// 视图并进 `Translating`。选项取探测时冻结的那份，不再读配置。
    ///
    /// 产物与探测模态先配对再消费：编号不符（陈旧）或模态错配都不消费探测，
    /// 同编号的后续合法产物仍可提交。文本分支命中内容闸门时探测作废但**当前
    /// 显示保留**（它属于上一个会话，误划与敏感内容都不该把它顶掉），且没有
    /// 放行出口——防护不交由用户控制。图像分支没有可扫描文本，不设内容闸门；
    /// 字节以 Arc 零拷贝留存进 `attached_image`，流式视图原文为空（「经」是
    /// 图本身）。
    pub fn commit_probe(&mut self, probe_id: u64, input: TaskInput) -> InputOutcome {
        // 先校验模态再消费探测：模态错配不吃掉待下发任务，同编号的
        // 后续合法产物仍可提交。
        let source = match self.probe.as_ref() {
            Some(probe) if probe.id == probe_id => probe.source,
            _ => return InputOutcome::Ignored,
        };
        let attached = match (&input, source) {
            (TaskInput::Text { text }, ProbeSource::Selection) => {
                if let Some(reason) = guard::detect_sensitive(text) {
                    self.probe = None;
                    return InputOutcome::Blocked(reason);
                }
                None
            }
            (TaskInput::Image { png, .. }, ProbeSource::PasteboardImage) => Some(Arc::clone(png)),
            _ => return InputOutcome::Ignored,
        };
        let Some(ProbeTask {
            id,
            pending: PendingOptions(options),
            ..
        }) = self.probe.take()
        else {
            return InputOutcome::Ignored;
        };
        if let Some(cancel) = self.current_cancel.take() {
            cancel.cancel();
        }
        self.active_request = None;
        self.generation = id;
        self.attached_image = attached;
        InputOutcome::Dispatch(self.begin_run(input, options))
    }

    /// 探测失败的处置（划词与剪贴板图片共用）：划词的空选区/读不到算误滑——
    /// 静默丢弃，状态机与当前显示一律不动（壳只记一条带前台应用的排查日志）；
    /// 剪贴板图片没有误滑形态（changeCount 前进才触发，覆盖竞态在取材侧已被
    /// 吞掉），全部失败（超上限、读取失败等）都是真实故障，落失败卡。编号不符
    /// 按陈旧丢弃。
    pub fn commit_probe_failed(&mut self, probe_id: u64, error: &GlossError) -> FailureOutcome {
        let source = match self.probe.as_ref() {
            Some(probe) if probe.id == probe_id => probe.source,
            _ => return FailureOutcome::Ignored,
        };
        self.probe = None;
        let silent = match source {
            ProbeSource::Selection => matches!(
                error,
                GlossError::SelectionUnavailable | GlossError::SelectionEmpty
            ),
            ProbeSource::PasteboardImage => false,
        };
        if silent {
            return FailureOutcome::SilentlyDropped;
        }
        self.land_error(error);
        FailureOutcome::Shown
    }

    /// 探测作废（取材通道发送失败等）：清掉编号，防止滞留的探测吞掉
    /// 后续同形产物。当前显示与状态机不动。
    pub fn drop_probe(&mut self) {
        self.probe = None;
    }

    /// 落 `Error` 态的共用半边：取消在途、作废请求副本（重试只认
    /// `active_request`）、按错误映射给动作出口并落失败卡。
    fn land_error(&mut self, error: &GlossError) {
        if let Some(cancel) = self.current_cancel.take() {
            cancel.cancel();
        }
        // 只有「原样重发有意义」的失败才留请求副本；其余类别（含通道级
        // 故障的 fail_* 降级路径）一律清掉，retry() 自然无从发起。副本缺失
        // 时不给 Retry 出口——别摆一颗点了没反应的死按钮。
        let mut action = error_action(error);
        if action == Some(ErrorAction::Retry) {
            if self.active_request.is_none() {
                action = None;
            }
        } else {
            self.active_request = None;
        }
        self.overlay_view = Some(OverlayView::Failed {
            cause: FailureCause::Task(error.clone()),
            action,
        });
    }

    /// 采纳流式增量：追加到流式视图的原始回复（`note` 的渐进提取在渲染层）。
    /// 返回是否有新内容需要重绘。
    ///
    /// 这份累积只服务**流式显示**；完成态以 `TaskDone` 的 `outcome.note` 为
    /// 权威源，整卡覆盖（见 `crate::runtime::pipeline`）。
    pub fn accept_chunk(&mut self, generation: u64, delta: String) -> bool {
        if generation != self.generation {
            return false;
        }
        match &mut self.overlay_view {
            Some(OverlayView::Streaming { raw, .. }) => {
                raw.push_str(&delta);
                true
            }
            _ => false,
        }
    }

    /// 采纳任务产物：定格注文并进入 `Show`。返回是否需要重绘。
    ///
    /// `outcome.note` 直接覆盖流式累积（完成态以它为权威源）；原文从流式
    /// 视图随行进产物卡，供注疏的「经」位对照。**降级产物**（契约校验失败）
    /// 照常上卡，但**不清 `active_request`**——重试按钮据此原样重发，守卫见
    /// [`Self::retry`]。
    pub fn accept_done(&mut self, generation: u64, outcome: TaskOutcome) -> bool {
        if generation != self.generation {
            return false;
        }
        let source = match &mut self.overlay_view {
            Some(OverlayView::Streaming { source, .. }) => std::mem::take(source),
            _ => return false,
        };
        // 流已终结：令牌随推理期结束而清，保持「Some ⟺ Translating」。
        // 这里只丢弃句柄、不取消——任务已经正常完成。
        self.current_cancel = None;
        if !outcome.degraded {
            self.active_request = None;
        }
        self.overlay_view = Some(OverlayView::Outcome { source, outcome });
        true
    }

    /// 采纳自动分类结果：把判定的任务类型写进流式视图（含分类失败时的兜底
    /// kind，排版如实反映即将执行的任务）。陈旧代数或非流式视图不采纳——
    /// 迟到标签不得落在已定格的产物卡上。返回是否需要重绘。
    pub fn accept_classified(&mut self, generation: u64, kind: TaskKind) -> bool {
        if generation != self.generation {
            return false;
        }
        match &mut self.overlay_view {
            Some(OverlayView::Streaming { classified, .. }) => {
                *classified = Some(kind);
                true
            }
            _ => false,
        }
    }

    /// 采纳修复轮开始（`TaskRepairing`）：清掉上一轮的流式累积并把视图置
    /// 「修复中」（页脚切「重新注解」，无横幅无计数——轮次只在 core 日志）。
    /// 守卫同 [`Self::accept_chunk`]：新会话的流式卡不得被旧任务的修复事件
    /// 清稿。返回是否需要重绘。
    pub fn accept_repairing(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        match &mut self.overlay_view {
            Some(OverlayView::Streaming { raw, repairing, .. }) => {
                raw.clear();
                *repairing = true;
                true
            }
            _ => false,
        }
    }

    /// 采纳任务失败：落 `Error` 态并展示失败信息与动作出口（可重试类带重试
    /// 按钮并保留请求副本，配置/鉴权类引导去设置页）。这是推理路径的失败；
    /// 探测失败走 [`Self::commit_probe_failed`]。其余状态不采纳——失败卡不得
    /// 把已收起的浮层弹回。
    pub fn accept_failed(&mut self, generation: u64, error: &GlossError) -> FailureOutcome {
        if generation != self.generation || self.state() != AppState::Translating {
            return FailureOutcome::Ignored;
        }
        self.current_cancel = None;
        self.land_error(error);
        FailureOutcome::Shown
    }

    /// 重试失败卡或降级产物卡上的任务：原样重发失败/降级的那个请求（同
    /// 代数——旧任务的流已终结，不会有两路同代回传），浮层回到流式视图。
    /// 守卫：`Error` 态，或 `Show` 态且**当前产物是降级产物**（正常产物
    /// 没有重试出口）；无可重试请求时返回 `None`。
    pub fn retry(&mut self) -> Option<RunRequest> {
        let retryable = matches!(
            self.overlay_view,
            Some(OverlayView::Failed { .. })
                | Some(OverlayView::Outcome {
                    outcome: TaskOutcome { degraded: true, .. },
                    ..
                })
        );
        if !retryable {
            return None;
        }
        // 取走在途请求原件：`begin_run` 会把流式视图与请求副本一并重新
        // 记回，这里只需交出状态。
        let (input, options) = self.active_request.take()?;
        Some(self.begin_run(input, options))
    }

    /// 下发一个任务的共用半边：换新取消令牌、落定流式视图（首次下发与
    /// 重试同形——原文取自本次下发的输入）并把请求副本记在途（失败可
    /// 重试）。
    fn begin_run(&mut self, input: TaskInput, options: TaskOptions) -> RunRequest {
        let cancel = CancellationToken::new();
        // CancellationToken 是 Arc 共享句柄：跨线程取消与在途留存各持一枚，
        // clone 只递引用计数，零堆分配——勿改成深拷贝或字段拆解。
        self.current_cancel = Some(cancel.clone());
        self.overlay_view = Some(streaming_view(&input));
        self.active_request = Some((input.clone(), options.clone()));
        RunRequest {
            generation: self.generation,
            input,
            options,
            cancel,
        }
    }

    /// 浮层收起（Esc / 关闭按钮）即放弃在途任务：取消令牌（唯一取消机制）、
    /// 清空视图与随行图像并回 `Idle`；在途探测一并作废（收起后迟到的产物
    /// 不得把浮层弹回）。放弃后的迟到产物经代数或状态守卫丢弃——为一个不可
    /// 见的浮层继续推理与渲染纯属空转；重新触发即重新取材（旧产物本就可能
    /// 已过期）。
    pub fn hide_overlay(&mut self) {
        if let Some(cancel) = self.current_cancel.take() {
            cancel.cancel();
        }
        self.active_request = None;
        self.probe = None;
        self.attached_image = None;
        self.overlay_view = None;
    }

    /// 推理通道不可用（③发送失败）时的降级：直接落 `Error` 态。通道已死时
    /// 重发只会再死一次，失败卡不带重试按钮。
    pub fn fail_transport(&mut self, generation: u64) {
        if generation != self.generation || self.state() != AppState::Translating {
            return;
        }
        self.current_cancel = None;
        self.active_request = None;
        self.overlay_view = Some(OverlayView::Failed {
            cause: FailureCause::TransportChannel,
            action: None,
        });
    }
}

/// 错误 → 失败卡动作（映射表）：网络/限流可原样重试；鉴权、模态、JSON 模式
/// 不支持与配置错误都要进设置页才能修（模型、密钥与模型服务的修改入口都在
/// 那里）；其余类别给不出有意义的按钮——权限类引导已写在文案里，协议异常重发
/// 同一个请求只会再错一次。
fn error_action(error: &GlossError) -> Option<ErrorAction> {
    match error {
        GlossError::EngineNetwork | GlossError::EngineRateLimited => Some(ErrorAction::Retry),
        GlossError::EngineAuth
        | GlossError::UnsupportedModality
        | GlossError::JsonModeUnsupported
        | GlossError::Config(_) => Some(ErrorAction::OpenSettings),
        _ => None,
    }
}

/// 一次下发请求的流式视图起点：原文照抄（图像任务无经位原文——经是图本身，
/// 走 `attached_image`）、正文空、未分类（LLM 层的 `TaskClassified` 到达后
/// 精化）。流式视图不带语言：唯一来源是产物的 `structured.code_language`，
/// 渲染层现算，流式期按通用启发集着色、无角标。重试与首次下发共用。
fn streaming_view(input: &TaskInput) -> OverlayView {
    let source = match input {
        TaskInput::Text { text } => text.clone(),
        _ => String::new(),
    };
    OverlayView::Streaming {
        source,
        raw: String::new(),
        classified: None,
        repairing: false,
    }
}

/// 一次平台事件的去向：取材，或被拦下。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerDecision {
    /// 放行：划词不带显式意图，类型由 LLM 层分类；剪贴板图片的取材动作即
    /// 显式意图，LLM 层恒定映射。
    Acquire,
    /// 场景闸门拦下（安全输入态 / 敏感应用名单）。只拦真实触发，设置不在
    /// 此列。
    Blocked(TriggerBlock),
    /// 拦下：Gloss 自身是前台应用时的划词手势——HID 全局 tap 连拖自己的
    /// 浮层不该触发取材。
    SelfSuppressed,
    /// 不进状态机的直通事件（设置、授权落定——各自在 drain 处有自己的出口）。
    Unwired,
}

/// 一次平台事件的去向判定：划词手势先判自身前台（防误触，与敏感防护无关，
/// 日志级别也不同）再过场景闸门；剪贴板观察只过场景闸门（剪贴板不是 HID
/// 手势，无自身前台的误触面）。
pub fn trigger_decision(event: &PlatformEvent, scene: &SceneFacts) -> TriggerDecision {
    match event {
        PlatformEvent::SelectionGesture { .. } => {
            if scene.front_app.as_ref().is_some_and(|app| app.is_self) {
                return TriggerDecision::SelfSuppressed;
            }
            match guard::trigger_block(scene) {
                Some(block) => TriggerDecision::Blocked(block),
                None => TriggerDecision::Acquire,
            }
        }
        PlatformEvent::PasteboardImageObserved => match guard::trigger_block(scene) {
            Some(block) => TriggerDecision::Blocked(block),
            None => TriggerDecision::Acquire,
        },
        PlatformEvent::OpenSettingsRequested | PlatformEvent::AccessibilityGranted => {
            TriggerDecision::Unwired
        }
    }
}

/// 按配置快照解析任务选项：目标语言取配置默认，模型取配置的单一项（`Arc<str>`
/// 共享，复制只递引用计数），prompt 模板语言按 `Language` 落定（`System` 取
/// 系统语言）。三者随任务冻结，执行途中不再回读配置。字段全量显式构造，不走
/// `..Default::default()`——后者会先把缺省模型 id 分配出来再被覆盖，且选项新增
/// 字段时编译器会在此处强制补齐，不会静默漏填。
fn task_options(config: &Config, system_locale: Locale) -> TaskOptions {
    TaskOptions {
        target_lang: Some(config.target_lang.clone()),
        model: Arc::from(config.model.as_str()),
        prompt_locale: Some(config.language.resolve(system_locale)),
    }
}

/// 回传事件的类别标签（露面策略只关心类别，不关心代数与载荷）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventKind {
    InputReady,
    TaskClassified,
    TaskChunk,
    /// 与 chunk 同类：只更新已可见的流式卡，不负责露面。
    TaskRepairing,
    TaskDone,
    TaskFailed,
    /// 与任务代数无关，露面策略恒不显示（壳层旁路留痕）。
    SecretPrewarmed,
}

pub(crate) fn event_kind(event: &Event) -> EventKind {
    match event {
        Event::InputReady { .. } => EventKind::InputReady,
        Event::TaskClassified { .. } => EventKind::TaskClassified,
        Event::TaskChunk { .. } => EventKind::TaskChunk,
        Event::TaskRepairing { .. } => EventKind::TaskRepairing,
        Event::TaskDone { .. } => EventKind::TaskDone,
        Event::TaskFailed { .. } => EventKind::TaskFailed,
        Event::SecretPrewarmed { .. } => EventKind::SecretPrewarmed,
    }
}

/// 单个回传事件后浮层要不要自动露面。
///
/// `accepted` 是状态机是否采纳了该事件：陈旧事件不触发显示。露面的来源只有
/// 「失败即弹」；挂起显形请求在 [`should_reveal`] 里与批次合并。取材成功、
/// 分类结果、流式增量、修复轮与完成态都只在已可见的浮层上更新，不负责露面。
/// 取材成功不直接露面还因为：用户若在取材中收起浮层，机器回 Idle，陈旧的取材
/// 产物采纳不上，自然也不会把浮层弹回。
fn auto_show_for(kind: EventKind, accepted: bool) -> bool {
    match kind {
        EventKind::InputReady => false,
        EventKind::TaskFailed => accepted,
        EventKind::TaskClassified
        | EventKind::TaskRepairing
        | EventKind::TaskDone
        | EventKind::TaskChunk => false,
        EventKind::SecretPrewarmed => false,
    }
}

/// 一批回传之后浮层要不要自动露面：**任一**事件判为要显示就显示。
pub(crate) fn auto_show_after(batch: impl IntoIterator<Item = (EventKind, bool)>) -> bool {
    batch
        .into_iter()
        .any(|(kind, accepted)| auto_show_for(kind, accepted))
}

/// 一批回传处理后浮层要不要显形：挂起显形请求（`pending`）要求机器确有视图
/// ——视图为空时弹出的会是渲染自检卡（挂起置位与消费之间没有插入点，两段
/// drain 同帧连跑，守卫只为防御）；划词提交的置位在批内 `commit_probe`，同一
/// 帧消费。挂起显形不依赖回传批次，与批次内的「失败即弹」任一成立即显示。
pub(crate) fn should_reveal(
    pending: bool,
    has_view: bool,
    batch: impl IntoIterator<Item = (EventKind, bool)>,
) -> bool {
    (pending && has_view) || auto_show_after(batch)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gloss_core::config::Language;
    use gloss_core::guard::{FrontApp, SceneFacts, SensitiveKind};
    use gloss_core::model::{GlossError, Lang, ScreenPoint};
    use gloss_core::task::OutcomeStructured;

    use super::*;

    fn selection_gesture() -> PlatformEvent {
        PlatformEvent::SelectionGesture {
            pos: ScreenPoint::new(0, 0),
        }
    }

    fn plain_outcome(note: &str) -> TaskOutcome {
        TaskOutcome {
            kind: TaskKind::TranslateWord,
            note: note.into(),
            structured: OutcomeStructured::Plain {
                examples: Vec::new(),
            },
            degraded: false,
        }
    }

    fn degraded_outcome() -> TaskOutcome {
        TaskOutcome {
            kind: TaskKind::ExplainCode,
            note: String::new(),
            structured: OutcomeStructured::Code {
                examples: Vec::new(),
                code_language: None,
            },
            degraded: true,
        }
    }

    fn text_input(text: &str) -> TaskInput {
        TaskInput::Text { text: text.into() }
    }

    fn suspicious_text() -> String {
        format!("key sk-{}", "aB3".repeat(8))
    }

    fn probe(machine: &mut TaskStateMachine, config: &Config) -> u64 {
        probe_using(machine, config, Locale::Zh)
    }

    fn probe_using(machine: &mut TaskStateMachine, config: &Config, system_locale: Locale) -> u64 {
        let command = machine
            .begin_selection_probe(
                &selection_gesture(),
                config,
                system_locale,
                &SceneFacts::default(),
            )
            .expect("selection gesture must probe");
        let AcquireCommand::AcquireText { generation } = command else {
            panic!("acquire text expected");
        };
        generation
    }

    fn pasteboard_probe(machine: &mut TaskStateMachine, config: &Config) -> u64 {
        let command = machine
            .begin_pasteboard_probe(
                &PlatformEvent::PasteboardImageObserved,
                config,
                Locale::Zh,
                &SceneFacts::default(),
            )
            .expect("pasteboard observation must probe");
        let AcquireCommand::AcquireClipboardImage { generation } = command else {
            panic!("acquire clipboard image expected");
        };
        generation
    }

    fn image_input(png: &[u8]) -> TaskInput {
        TaskInput::Image {
            png: Arc::from(png),
        }
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
                is_self: false,
            }),
        }
    }

    fn frontmost_is_self() -> SceneFacts {
        SceneFacts {
            secure_input: false,
            front_app: Some(FrontApp {
                bundle_id: Some("com.example.gloss".into()),
                name: None,
                is_self: true,
            }),
        }
    }

    #[test]
    fn probe_mapping_covers_wired_events_only() {
        let mut machine = TaskStateMachine::new();
        let probe_id = probe(&mut machine, &Config::default());
        assert_eq!(
            probe_id, 1,
            "the selection probe takes the first id from the counter"
        );
        assert_eq!(
            machine.generation(),
            0,
            "a probe takes a probe id, not a generation: the visible session is untouched"
        );

        for event in [
            PlatformEvent::PasteboardImageObserved,
            PlatformEvent::OpenSettingsRequested,
        ] {
            assert!(
                machine
                    .begin_selection_probe(
                        &event,
                        &Config::default(),
                        Locale::Zh,
                        &SceneFacts::default()
                    )
                    .is_none(),
                "unwired events must not acquire"
            );
        }
        assert_eq!(
            machine.generation(),
            0,
            "unwired events must not consume a generation"
        );
        assert_eq!(
            machine.probe_id(),
            Some(1),
            "unwired events must not replace the outstanding probe"
        );
    }

    #[test]
    fn trigger_decision_separates_blocked_and_unwired_events() {
        let open = SceneFacts::default();
        assert_eq!(
            trigger_decision(&selection_gesture(), &open),
            TriggerDecision::Acquire,
            "a selection carries no explicit intent: the kind is the LLM layer's call"
        );
        assert_eq!(
            trigger_decision(&PlatformEvent::PasteboardImageObserved, &open),
            TriggerDecision::Acquire,
            "a pasteboard image is an explicit image-explain intent"
        );
        assert_eq!(
            trigger_decision(&PlatformEvent::PasteboardImageObserved, &blocked_by_app()),
            TriggerDecision::Blocked(TriggerBlock::BlockedApp("com.1password.1password")),
            "a pasteboard observation in a sensitive app is blocked by the scene gate alone"
        );
        assert_eq!(
            trigger_decision(
                &PlatformEvent::PasteboardImageObserved,
                &frontmost_is_self()
            ),
            TriggerDecision::Acquire,
            "the clipboard watch has no self-frontmost mistouch surface"
        );
        assert_eq!(
            trigger_decision(&PlatformEvent::OpenSettingsRequested, &blocked_by_app()),
            TriggerDecision::Unwired,
            "settings is an entry point, never a gated trigger"
        );
        assert_eq!(
            trigger_decision(&PlatformEvent::AccessibilityGranted, &open),
            TriggerDecision::Unwired,
            "the grant event is handled in the drain, never a gated trigger"
        );
        assert_eq!(
            trigger_decision(&selection_gesture(), &open),
            TriggerDecision::Acquire
        );
        assert_eq!(
            trigger_decision(&selection_gesture(), &blocked_by_app()),
            TriggerDecision::Blocked(TriggerBlock::BlockedApp("com.1password.1password")),
            "a selection in a sensitive app is blocked"
        );
    }

    #[test]
    fn scene_gate_stops_the_probe_before_acquisition() {
        let mut machine = TaskStateMachine::new();
        let secure = SceneFacts {
            secure_input: true,
            front_app: Some(FrontApp {
                bundle_id: Some("com.example.editor".into()),
                name: None,
                is_self: false,
            }),
        };
        assert!(
            machine
                .begin_selection_probe(
                    &selection_gesture(),
                    &Config::default(),
                    Locale::Zh,
                    &secure
                )
                .is_none(),
            "a focused password field stops the probe"
        );
        assert!(
            machine
                .begin_selection_probe(
                    &selection_gesture(),
                    &Config::default(),
                    Locale::Zh,
                    &blocked_by_app()
                )
                .is_none(),
            "a listed frontmost app stops the probe"
        );
        assert_eq!(
            machine.probe_id(),
            None,
            "a suppressed probe takes no probe id"
        );
        assert_eq!(
            machine.state(),
            AppState::Idle,
            "no overlay, no failure card"
        );

        assert_eq!(
            probe(&mut machine, &Config::default()),
            1,
            "the same gesture probes once the scene clears"
        );
        assert_eq!(
            machine.generation(),
            0,
            "the probe itself still leaves the generation alone"
        );
    }

    #[test]
    fn probe_in_the_self_frontmost_scene_is_suppressed_without_an_id() {
        assert_eq!(
            trigger_decision(&selection_gesture(), &frontmost_is_self()),
            TriggerDecision::SelfSuppressed,
            "a drag over gloss's own window is anti-mistouch territory, not a trigger"
        );

        let mut machine = TaskStateMachine::new();
        assert!(
            machine
                .begin_selection_probe(
                    &selection_gesture(),
                    &Config::default(),
                    Locale::Zh,
                    &frontmost_is_self()
                )
                .is_none(),
            "the suppressed gesture acquires nothing"
        );
        assert_eq!(machine.probe_id(), None);
        assert_eq!(machine.state(), AppState::Idle);
        assert!(machine.overlay_view().is_none());
    }

    #[test]
    fn selection_options_pair_with_one_snapshot_including_the_model() {
        let mut machine = TaskStateMachine::new();
        let config = Config {
            target_lang: Lang::Ja,
            model: "frozen-model".into(),
            ..Default::default()
        };

        let probe_id = probe(&mut machine, &config);
        assert_eq!(
            probe_id, 1,
            "the gesture dispatches no kind; the LLM layer classifies"
        );

        let request = dispatched(machine.commit_probe(1, text_input("fn main() {}")));
        assert_eq!(
            request.options.model.as_ref(),
            "frozen-model",
            "the model freezes from the probe-time snapshot"
        );
        assert_eq!(
            request.options.target_lang,
            Some(Lang::Ja),
            "target language still freezes from the probe-time snapshot"
        );
    }

    #[test]
    fn prompt_locale_follows_config_language_and_the_system() {
        let mut explicit = TaskStateMachine::new();
        let chosen = Config {
            language: Language::En,
            ..Default::default()
        };
        assert_eq!(probe(&mut explicit, &chosen), 1);
        let request = dispatched(explicit.commit_probe(1, text_input("hello")));
        assert_eq!(
            request.options.prompt_locale,
            Some(Locale::En),
            "an explicit choice ignores the injected system language"
        );

        let mut following = TaskStateMachine::new();
        let factory = Config::default();
        assert_eq!(factory.language, Language::System);
        assert_eq!(probe_using(&mut following, &factory, Locale::En), 1);
        let request = dispatched(following.commit_probe(1, text_input("hello")));
        assert_eq!(
            request.options.prompt_locale,
            Some(Locale::En),
            "follow-the-system takes the injected system language"
        );
    }

    #[test]
    fn options_freeze_at_probe_time() {
        let mut machine = TaskStateMachine::new();
        let before = Config {
            target_lang: Lang::Ja,
            model: "before-model".into(),
            language: Language::En,
            ..Default::default()
        };
        let after = Config {
            target_lang: Lang::Ko,
            model: "after-model".into(),
            language: Language::Zh,
            ..Default::default()
        };

        assert_eq!(probe(&mut machine, &before), 1);
        let request = dispatched(machine.commit_probe(1, text_input("hello")));
        assert_eq!(
            request.options.target_lang,
            Some(Lang::Ja),
            "in-flight task must keep the snapshot taken at probe time"
        );
        assert_eq!(
            request.options.model.as_ref(),
            "before-model",
            "the model is frozen with the rest of the options"
        );
        assert_eq!(
            request.options.prompt_locale,
            Some(Locale::En),
            "the prompt locale is frozen with the rest of the options"
        );
        assert_eq!(probe(&mut machine, &after), 2);
        let request = dispatched(machine.commit_probe(2, text_input("world")));
        assert_eq!(request.options.target_lang, Some(Lang::Ko));
        assert_eq!(request.options.model.as_ref(), "after-model");
        assert_eq!(request.options.prompt_locale, Some(Locale::Zh));
    }

    #[test]
    fn commit_probe_yields_run_request_and_supersedes_the_previous_session() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);

        let request = dispatched(machine.commit_probe(1, text_input("hello")));
        assert_eq!(request.generation, 1);
        assert_eq!(machine.generation(), 1, "the probe id is promoted");
        assert_eq!(machine.state(), AppState::Translating);
        assert!(machine.current_cancel().is_some());
        assert!(machine.probe_id().is_none(), "the probe is consumed");

        let old_cancel = request.cancel;
        assert_eq!(probe(&mut machine, &Config::default()), 2);
        assert_eq!(machine.state(), AppState::Translating, "探测不动当前会话");
        dispatched(machine.commit_probe(2, text_input("world")));
        assert_eq!(machine.generation(), 2);
        assert!(
            old_cancel.is_cancelled(),
            "commit must cancel the superseded in-flight task"
        );
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Streaming { source, .. }) if source == "world"
        ));

        assert!(
            matches!(
                machine.commit_probe(2, text_input("again")),
                InputOutcome::Ignored
            ),
            "a consumed probe accepts no duplicate commit"
        );
    }

    #[test]
    fn a_probe_leaves_a_visible_session_completely_untouched() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let request = dispatched(machine.commit_probe(1, text_input("hello")));
        machine.accept_chunk(1, "已到达的正文".into());
        let view_before = machine.overlay_view().cloned();
        let state_before = machine.state();
        let cancel_before = request.cancel.clone();

        assert_eq!(probe(&mut machine, &Config::default()), 2);
        assert_eq!(machine.state(), state_before);
        assert_eq!(machine.overlay_view(), view_before.as_ref());
        assert!(machine.current_cancel().is_some());
        assert!(
            !cancel_before.is_cancelled(),
            "probing must not cancel the visible session's task"
        );

        let outcome = machine.commit_probe_failed(2, &GlossError::SelectionEmpty);
        assert_eq!(outcome, FailureOutcome::SilentlyDropped);
        assert_eq!(machine.state(), state_before, "误滑连状态都不碰");
        assert_eq!(machine.overlay_view(), view_before.as_ref());
        assert!(!cancel_before.is_cancelled());
    }

    #[test]
    fn image_input_for_text_kind_is_rejected() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        assert!(matches!(
            machine.commit_probe(
                1,
                TaskInput::Image {
                    png: Arc::from(&b"png"[..])
                }
            ),
            InputOutcome::Ignored
        ));
    }

    #[test]
    fn classified_kind_updates_the_streaming_chip_only_once_current() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_probe(1, text_input("hello")));

        assert!(
            !machine.accept_classified(0, TaskKind::TranslateWord),
            "a stale classification must be dropped"
        );
        assert!(machine.accept_classified(1, TaskKind::TranslateWord));
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Streaming {
                classified: Some(TaskKind::TranslateWord),
                ..
            })
        ));

        machine.accept_done(1, plain_outcome("产物"));
        assert!(
            !machine.accept_classified(1, TaskKind::ExplainCode),
            "a late classification must not touch a settled outcome card"
        );
    }

    #[test]
    fn the_streaming_view_waits_unclassified_until_the_llm_layer_reports() {
        let mut machine = TaskStateMachine::new();
        let probe_id = probe(&mut machine, &Config::default());
        dispatched(machine.commit_probe(
            probe_id,
            TaskInput::Text {
                text: "hello".into(),
            },
        ));
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Streaming {
                    classified: None,
                    ..
                })
            ),
            "the streaming view stays unclassified until the LLM layer reports"
        );
    }

    #[test]
    fn the_repair_marker_clears_the_stream_and_refuses_stale_rounds() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_probe(1, text_input("hello")));
        machine.accept_chunk(1, "上一轮的违约正文".into());

        assert!(
            !machine.accept_repairing(0),
            "a stale repair marker must be dropped"
        );
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Streaming { raw, repairing: false, .. }) if raw == "上一轮的违约正文"
        ));

        assert!(machine.accept_repairing(1));
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Streaming { raw, repairing: true, .. }) if raw.is_empty()
            ),
            "the violated accumulation is cleared when the repair round starts"
        );

        machine.accept_chunk(1, "新一轮的正文".into());
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Streaming {
                    repairing: true,
                    ..
                })
            ),
            "chunks of the repair round keep the repairing footer on"
        );

        machine.accept_done(1, plain_outcome("产物"));
        assert!(
            !machine.accept_repairing(1),
            "a settled outcome card must not be cleared by a late repair marker"
        );

        assert_eq!(probe(&mut machine, &Config::default()), 2);
        dispatched(machine.commit_probe(2, text_input("新任务")));
        assert!(
            machine.accept_repairing(2),
            "a fresh session accepts its own repair markers"
        );
    }

    #[test]
    fn a_degraded_done_keeps_the_request_and_retry_redispatches_it() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let original = dispatched(machine.commit_probe(1, text_input("hello")));
        machine.accept_chunk(1, "{\"note\":".into());
        assert!(machine.accept_done(1, degraded_outcome()));
        assert_eq!(machine.state(), AppState::Show);
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Outcome { outcome, .. }) if outcome.degraded
            ),
            "the degraded outcome settles on the outcome card"
        );

        let retried = machine
            .retry()
            .expect("a degraded card keeps the request copy for the retry");
        assert_eq!(retried.generation, original.generation, "same generation");
        assert_eq!(retried.input, original.input, "same input is re-dispatched");
        assert_eq!(
            retried.options, original.options,
            "same options are re-dispatched (no config re-read)"
        );
        assert_eq!(machine.state(), AppState::Translating);
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Streaming { source, .. }) if source == "hello"
            ),
            "the retry returns to the streaming view"
        );

        assert_eq!(probe(&mut machine, &Config::default()), 2);
        dispatched(machine.commit_probe(2, text_input("world")));
        assert!(machine.accept_done(2, plain_outcome("正常产物")));
        assert_eq!(machine.state(), AppState::Show);
        assert!(
            machine.retry().is_none(),
            "a normal outcome has no retry exit"
        );
    }

    #[test]
    fn hide_abandons_inflight_and_drops_late_events() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let request = dispatched(machine.commit_probe(1, text_input("hello")));
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
        assert_eq!(
            machine.accept_failed(1, &GlossError::EngineNetwork),
            FailureOutcome::Ignored,
            "hidden task's late failure must be dropped"
        );
        assert_eq!(machine.state(), AppState::Idle);
    }

    #[test]
    fn hide_overlay_drops_the_outstanding_probe() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        machine.hide_overlay();
        assert!(
            matches!(
                machine.commit_probe(1, text_input("迟到的产物")),
                InputOutcome::Ignored
            ),
            "a probe dropped by hide must not resurrect the overlay"
        );
        assert_eq!(
            machine.commit_probe_failed(1, &GlossError::SelectionEmpty),
            FailureOutcome::Ignored
        );

        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 2);
        machine.hide_overlay();
        assert!(
            matches!(
                machine.commit_probe(2, image_input(b"late product")),
                InputOutcome::Ignored
            ),
            "a pasteboard probe dropped by hide must not resurrect the overlay either"
        );
    }

    #[test]
    fn pasteboard_probe_takes_an_id_without_touching_the_session() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 1);
        assert_eq!(
            machine.probe_id(),
            Some(1),
            "the observation parks an outstanding probe"
        );
        assert_eq!(
            machine.generation(),
            0,
            "a probe takes a probe id, not a generation"
        );
        assert_eq!(machine.state(), AppState::Idle);
        assert!(machine.overlay_view().is_none());
        assert!(machine.attached_image().is_none());
        machine.drop_probe();

        assert_eq!(probe(&mut machine, &Config::default()), 2);
        let request = dispatched(machine.commit_probe(2, text_input("hello")));
        machine.accept_chunk(2, "已到达的正文".into());
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 3);
        assert_eq!(machine.state(), AppState::Translating);
        assert_eq!(
            machine.generation(),
            2,
            "the probe leaves the visible session alone"
        );
        assert!(
            !request.cancel.is_cancelled(),
            "probing must not cancel the visible session's task"
        );
    }

    #[test]
    fn the_scene_gate_stops_the_pasteboard_probe_too() {
        let mut machine = TaskStateMachine::new();
        assert!(
            machine
                .begin_pasteboard_probe(
                    &PlatformEvent::PasteboardImageObserved,
                    &Config::default(),
                    Locale::Zh,
                    &blocked_by_app()
                )
                .is_none(),
            "a listed frontmost app stops the observation"
        );
        let secure = SceneFacts {
            secure_input: true,
            front_app: None,
        };
        assert!(
            machine
                .begin_pasteboard_probe(
                    &PlatformEvent::PasteboardImageObserved,
                    &Config::default(),
                    Locale::Zh,
                    &secure
                )
                .is_none(),
            "a focused password field stops the observation"
        );
        assert_eq!(
            machine.probe_id(),
            None,
            "a suppressed observation takes no probe id"
        );
        assert_eq!(machine.state(), AppState::Idle);

        assert_eq!(
            pasteboard_probe(&mut machine, &Config::default()),
            1,
            "the observation probes once the scene clears"
        );
        assert_eq!(
            machine.generation(),
            0,
            "the probe itself still leaves the generation alone"
        );
    }

    #[test]
    fn pasteboard_commit_retains_the_image_and_dispatches() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 1);
        let png: Arc<[u8]> = Arc::from(&b"png-bytes"[..]);
        let request = dispatched(machine.commit_probe(
            1,
            TaskInput::Image {
                png: Arc::clone(&png),
            },
        ));
        assert_eq!(request.generation, 1);
        assert_eq!(
            request.input,
            TaskInput::Image {
                png: Arc::clone(&png)
            },
            "the dispatch carries the image input"
        );
        assert_eq!(machine.generation(), 1, "the probe id is promoted");
        assert_eq!(machine.state(), AppState::Translating);
        assert!(machine.probe_id().is_none(), "the probe is consumed");
        assert!(
            machine
                .attached_image()
                .is_some_and(|attached| Arc::ptr_eq(attached, &png)),
            "the image is retained by Arc without copying bytes"
        );
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Streaming { source, .. }) if source.is_empty()
            ),
            "the streaming card carries no source text: the image itself is the 经"
        );
    }

    #[test]
    fn probes_are_mutually_exclusive_across_triggers() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        assert_eq!(
            pasteboard_probe(&mut machine, &Config::default()),
            2,
            "a pasteboard probe replaces the outstanding selection probe"
        );
        assert!(matches!(
            machine.commit_probe(1, text_input("旧探测的产物")),
            InputOutcome::Ignored
        ));
        assert_eq!(
            machine.commit_probe_failed(1, &GlossError::ImageTooLarge),
            FailureOutcome::Ignored,
            "the superseded probe's late failure is stale too"
        );
        assert_eq!(
            dispatched(machine.commit_probe(2, image_input(b"png"))).generation,
            2
        );

        assert_eq!(
            probe(&mut machine, &Config::default()),
            3,
            "a selection probe replaces the outstanding pasteboard probe"
        );
        assert!(matches!(
            machine.commit_probe(2, image_input(b"png")),
            InputOutcome::Ignored
        ));
        assert_eq!(
            dispatched(machine.commit_probe(3, text_input("新选区"))).generation,
            3
        );
    }

    #[test]
    fn text_on_a_pasteboard_probe_is_ignored_without_consuming() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 1);
        assert!(
            matches!(
                machine.commit_probe(1, text_input("剪贴板探测不收文本")),
                InputOutcome::Ignored
            ),
            "a pasteboard probe only accepts image products"
        );
        assert_eq!(
            dispatched(machine.commit_probe(1, image_input(b"png"))).generation,
            1,
            "the probe survives the mismatch and still commits"
        );
    }

    #[test]
    fn pasteboard_commit_supersedes_and_a_new_task_clears_the_image() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let old = dispatched(machine.commit_probe(1, text_input("旧任务")));
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 2);
        let png: Arc<[u8]> = Arc::from(&b"png-bytes"[..]);
        dispatched(machine.commit_probe(
            2,
            TaskInput::Image {
                png: Arc::clone(&png),
            },
        ));
        assert!(
            old.cancel.is_cancelled(),
            "the pasteboard commit supersedes the in-flight task"
        );
        assert!(machine.attached_image().is_some());

        assert_eq!(probe(&mut machine, &Config::default()), 3);
        dispatched(machine.commit_probe(3, text_input("回到文本")));
        assert!(
            machine.attached_image().is_none(),
            "a text task clears the attached image"
        );
    }

    #[test]
    fn hide_overlay_clears_the_attached_image() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_probe(1, image_input(b"png")));
        assert!(machine.attached_image().is_some());
        machine.hide_overlay();
        assert!(
            machine.attached_image().is_none(),
            "hide clears the attached image with the rest of the session"
        );
    }

    #[test]
    fn image_too_large_failure_lands_in_the_failure_card() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 1);
        assert_eq!(
            machine.commit_probe_failed(1, &GlossError::ImageTooLarge),
            FailureOutcome::Shown,
            "an oversized image is a real failure, not a mis-slide"
        );
        assert_eq!(machine.state(), AppState::Error);
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed { action: None, .. })
        ));
        assert!(
            machine.retry().is_none(),
            "an oversized image has no button-shaped exit"
        );
        assert!(machine.probe_id().is_none(), "the probe is consumed");
        assert!(
            machine.attached_image().is_none(),
            "nothing was ever committed"
        );
    }

    #[test]
    fn retrying_a_failed_image_task_keeps_the_attached_image() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 1);
        let png: Arc<[u8]> = Arc::from(&b"png-bytes"[..]);
        dispatched(machine.commit_probe(
            1,
            TaskInput::Image {
                png: Arc::clone(&png),
            },
        ));
        machine.accept_chunk(1, "{\"note\":\"描述".into());
        assert_eq!(
            machine.accept_failed(1, &GlossError::EngineNetwork),
            FailureOutcome::Shown
        );

        let retried = machine.retry().expect("retry must be available");
        assert_eq!(
            retried.input,
            TaskInput::Image {
                png: Arc::clone(&png)
            },
            "the retry resends the same image input"
        );
        assert!(
            machine
                .attached_image()
                .is_some_and(|attached| Arc::ptr_eq(attached, &png)),
            "the image survives the retry for the streaming card"
        );
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Streaming { source, .. }) if source.is_empty()
        ));
    }

    #[test]
    fn failed_guard_matches_translating_only() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_probe(1, text_input("hello")));
        assert_eq!(
            machine.accept_failed(1, &GlossError::EngineNetwork),
            FailureOutcome::Shown,
            "an engine failure is not the no-selection family, the card still shows"
        );
        assert_eq!(machine.state(), AppState::Error);

        machine.hide_overlay();
        assert_eq!(
            machine.accept_failed(1, &GlossError::EngineNetwork),
            FailureOutcome::Ignored,
            "hidden error card must not be resurrected"
        );
    }

    #[test]
    fn probe_no_selection_failures_are_silently_dropped() {
        for error in [GlossError::SelectionUnavailable, GlossError::SelectionEmpty] {
            let mut machine = TaskStateMachine::new();
            assert_eq!(probe(&mut machine, &Config::default()), 1);
            assert_eq!(
                machine.commit_probe_failed(1, &error),
                FailureOutcome::SilentlyDropped,
                "{error:?} on a probe is a pure mis-drag: no card, no state change"
            );
            assert_eq!(machine.state(), AppState::Idle);
            assert!(machine.overlay_view().is_none());
            assert!(machine.probe_id().is_none(), "the probe is consumed");
        }
    }

    #[test]
    fn probe_permission_failures_still_raise_the_card() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        assert_eq!(
            machine.commit_probe_failed(1, &GlossError::AccessibilityDenied),
            FailureOutcome::Shown,
            "a permission failure is actionable feedback, not a mis-drag"
        );
        assert_eq!(machine.state(), AppState::Error);
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed { .. })
        ));
    }

    #[test]
    fn selection_failures_outside_the_probe_still_raise_the_card() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_probe(1, text_input("hello")));
        assert_eq!(
            machine.accept_failed(1, &GlossError::SelectionUnavailable),
            FailureOutcome::Shown,
            "the silent drop covers the probe leg only; an inference failure is a card"
        );
        assert_eq!(machine.state(), AppState::Error);
    }

    #[test]
    fn stale_probe_results_are_dropped() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        assert_eq!(
            probe(&mut machine, &Config::default()),
            2,
            "a newer probe replaces the older one"
        );
        assert!(
            matches!(
                machine.commit_probe(1, text_input("旧探测的产物")),
                InputOutcome::Ignored
            ),
            "a superseded probe must not commit"
        );
        assert_eq!(
            machine.commit_probe_failed(1, &GlossError::SelectionUnavailable),
            FailureOutcome::Ignored
        );
    }

    #[test]
    fn modality_mismatch_preserves_pending_options() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        assert!(matches!(
            machine.commit_probe(
                1,
                TaskInput::Image {
                    png: Arc::from(&b"png"[..])
                }
            ),
            InputOutcome::Ignored
        ));
        assert!(
            matches!(
                machine.commit_probe(1, text_input("第二次")),
                InputOutcome::Dispatch(_)
            ),
            "pending options must survive a modality mismatch"
        );
    }

    #[test]
    fn transport_failure_lands_in_error() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let request = dispatched(machine.commit_probe(1, text_input("x")));
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
    fn retryable_failure_keeps_request_and_retry_redispatches_it() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let original = dispatched(machine.commit_probe(1, text_input("hello")));
        assert_eq!(
            machine.accept_failed(1, &GlossError::EngineNetwork),
            FailureOutcome::Shown
        );

        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed {
                cause: FailureCause::Task(GlossError::EngineNetwork),
                action: Some(ErrorAction::Retry),
            })
        ));

        let retried = machine.retry().expect("retry must be available");
        assert_eq!(retried.generation, original.generation, "same generation");
        assert_eq!(retried.input, original.input, "same input is re-dispatched");
        assert_eq!(
            retried.options, original.options,
            "same options are re-dispatched (no config re-read)"
        );
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
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_probe(1, text_input("x")));

        assert_eq!(
            machine.accept_failed(1, &GlossError::EngineRateLimited),
            FailureOutcome::Shown,
            "rate limited is retryable"
        );
        assert!(machine.retry().is_some());

        assert_eq!(probe(&mut machine, &Config::default()), 2);
        dispatched(machine.commit_probe(2, text_input("x")));
        assert_eq!(
            machine.accept_failed(2, &GlossError::EngineAuth),
            FailureOutcome::Shown
        );
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

        assert_eq!(probe(&mut machine, &Config::default()), 3);
        dispatched(machine.commit_probe(3, text_input("x")));
        assert_eq!(
            machine.accept_failed(3, &GlossError::UnsupportedModality),
            FailureOutcome::Shown
        );
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed {
                action: Some(ErrorAction::OpenSettings),
                ..
            })
        ));

        assert_eq!(probe(&mut machine, &Config::default()), 4);
        dispatched(machine.commit_probe(4, text_input("x")));
        assert_eq!(
            machine.accept_failed(4, &GlossError::Config("empty model id".into())),
            FailureOutcome::Shown
        );
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed {
                action: Some(ErrorAction::OpenSettings),
                ..
            })
        ));

        assert_eq!(probe(&mut machine, &Config::default()), 5);
        dispatched(machine.commit_probe(5, text_input("x")));
        assert_eq!(
            machine.accept_failed(5, &GlossError::JsonModeUnsupported),
            FailureOutcome::Shown
        );
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed {
                cause: FailureCause::Task(GlossError::JsonModeUnsupported),
                action: Some(ErrorAction::OpenSettings),
            })
        ));
        assert!(
            machine.retry().is_none(),
            "an unsupported endpoint cannot be fixed by resending"
        );
    }

    #[test]
    fn new_commit_and_hide_supersede_the_retry_request() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_probe(1, text_input("x")));
        assert_eq!(
            machine.accept_failed(1, &GlossError::EngineNetwork),
            FailureOutcome::Shown
        );

        assert_eq!(
            probe(&mut machine, &Config::default()),
            2,
            "probing alone leaves the retry offer alone"
        );
        assert!(
            machine.retry().is_some(),
            "a mis-slide must not kill the retry"
        );
        dispatched(machine.commit_probe(2, text_input("y")));
        assert!(
            machine.retry().is_none(),
            "the committed selection supersedes the retry offer"
        );

        assert_eq!(
            machine.accept_failed(2, &GlossError::EngineNetwork),
            FailureOutcome::Shown
        );
        machine.hide_overlay();
        assert_eq!(machine.state(), AppState::Idle);
        assert!(machine.retry().is_none(), "hide drops the retry request");
    }

    #[test]
    fn suspicious_input_is_dropped_while_the_visible_session_survives() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let settled = dispatched(machine.commit_probe(1, text_input("上一次的普通文本")));
        assert!(machine.accept_done(1, plain_outcome("上一次的产物")));
        let view_before = machine.overlay_view().cloned();

        assert_eq!(probe(&mut machine, &Config::default()), 2);
        let outcome = machine.commit_probe(2, text_input(&suspicious_text()));
        assert!(
            matches!(outcome, InputOutcome::Blocked(SensitiveKind::Token)),
            "a selection that looks like a token must be refused, got {outcome:?}"
        );
        assert_eq!(
            machine.state(),
            AppState::Show,
            "the refused probe leaves the visible session alone"
        );
        assert_eq!(
            machine.overlay_view(),
            view_before.as_ref(),
            "the previous card is preserved — a refusal is not a dismissal"
        );
        assert!(machine.probe_id().is_none(), "the probe is consumed");
        assert!(
            machine.active_request.is_none(),
            "no request copy is left behind for a later retry to pick up"
        );
        assert!(
            !settled.cancel.is_cancelled(),
            "nothing new was dispatched, so the finished session's token is untouched"
        );
    }

    #[test]
    fn blocked_input_is_not_redispatched_by_any_later_path() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        machine.commit_probe(1, text_input("card 4111 1111 1111 1111"));

        assert!(
            machine.retry().is_none(),
            "a refused probe is not a retryable failure"
        );
        assert!(matches!(
            machine.commit_probe(1, text_input("second arrival")),
            InputOutcome::Ignored
        ));
        assert!(
            !machine.accept_chunk(1, "迟到的正文".into())
                && !machine.accept_done(1, plain_outcome("迟到的产物"))
                && machine.accept_failed(1, &GlossError::EngineNetwork) == FailureOutcome::Ignored,
            "no product of a refused probe may be adopted either"
        );
        machine.fail_transport(1);
        assert_eq!(
            machine.generation(),
            0,
            "the refusal never touches the machine: no session to disturb"
        );

        assert_eq!(
            probe(&mut machine, &Config::default()),
            2,
            "the next probe starts over"
        );
        assert!(matches!(
            machine.commit_probe(2, text_input(&suspicious_text())),
            InputOutcome::Blocked(_)
        ));
        assert!(machine.probe_id().is_none());
    }

    #[test]
    fn ordinary_input_still_passes_the_content_gate() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);

        assert!(
            matches!(
                machine.commit_probe(1, text_input("今天下午三点开会")),
                InputOutcome::Dispatch(_)
            ),
            "the gate only fires on the high-confidence patterns"
        );
        assert_eq!(machine.state(), AppState::Translating);
    }
}

#[cfg(test)]
mod reveal_policy_tests {
    use super::{EventKind, auto_show_after, auto_show_for, should_reveal};

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
    fn pending_reveal_shows_only_over_a_live_view() {
        use EventKind::TaskFailed;

        assert!(
            should_reveal(true, true, []),
            "划词提交：挂起请求只在活视图在场时显形"
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
