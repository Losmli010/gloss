//! 任务状态机与浮层视图（functional core）：纯状态转移，不做任何 IO。
//!
//! 触发 → 取材 → 推理 → 展示/失败的完整转移在此收敛；通道发送、浮层
//! 窗口操作、日志由壳（app 的 winit handler 与组装点）执行——本模块只
//! 决策、不副作用，因此可被集成测试以公共 API 全时序驱动（分层测试
//! 的 L1 层，见 tests/pipeline.rs）。
//!
//! 触发只有**划词手势**一条路：两段式「探测—提交」，编号共用一个单调
//! 计数器（编号不复用，代数与探测编号永不碰撞）。[`TaskStateMachine::begin_selection_probe`]
//! 只领一个探测编号、在状态机之外取材——不改状态、不换视图、不取消
//! 在途任务，已显示的内容与在途推理全程无感；[`TaskStateMachine::commit_selection`]
//! 在产物到达时才提交：探测编号提升为代数、旧任务让位、视图换流式卡。
//! 探测失败（[`TaskStateMachine::commit_selection_failed`]）：空选区/
//! 读不到按误滑静默丢弃（显示原样保留），权限缺失落失败卡。
//!
//! **任务类型不在状态机**：划词手势不带显式意图，状态机冻结的只有配置
//! 选项（单次快照）；kind 由 LLM 层在执行前分类（提示直通/LLM 分类/兜底
//! 常量），经事件④的 `TaskClassified` 回传精化流式视图。失败卡的出口、
//! 重试的重发都按「input + options」原样进行，不回读配置。
//!
//! 两道敏感信息闸门的落点：场景闸门在 [`trigger_decision`]（触发前，
//! 拦下即不取材不占编号、不出浮层），内容闸门在 [`TaskStateMachine::commit_selection`]
//! （取材后、下发前，命中即丢弃探测且当前显示保留——不下发、不出浮层）。
//!
//! 浮层露面策略（[`should_reveal`]）同样是本模块的纯决策：挂起显形请求
//! 与「失败即弹」的按批判定；「怎么显示」在 `flow::reveal`。

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

/// 应用状态机：触发 → 取材 → 推理 → 展示/失败。
///
/// 转移概要：划词手势不走状态（探测段在状态机之外，产物经
/// [`TaskStateMachine::commit_selection`] 直接进 `Translating`，取消在途
/// 任务并换流式卡）；`Translating` 收 `TaskChunk` 追加展示、收 `TaskDone`
/// 定格 `Show`、收 `TaskFailed` 落 `Error`；收起（Esc / 关闭按钮）回
/// `Idle`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppState {
    /// 浮层隐藏，无在途任务。
    #[default]
    Idle,
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
    /// 推理中：原文 + 已到达的流式原始回复（模型的原始 JSON 流，渲染层
    /// 对 `note` 字段做渐进提取）。`classified` 是按代码排版的判定结果：
    /// LLM 层分类前为 `None`，`accept_classified` 到达后精化（代码解释
    /// 从分类帧起按代码排版）。
    Streaming {
        /// 触发时选中的原文。
        source: String,
        /// 已到达的流式原始回复累积（含 JSON 结构，提取归渲染层）。
        raw: String,
        /// 自动分类判明的任务类型（LLM 层回传后精化）。
        classified: Option<TaskKind>,
        /// 代码语言（角标与高亮规则集共用）：流式期恒 `None`——语言
        /// 只认产物回传的判定，流式期按通用启发集着色、无角标。
        code_lang: Option<String>,
    },
    /// 产物卡：按 `TaskKind` 精排或展示 markdown 注文。`source` 是本次
    /// 任务的原文（从流式视图随行而来），经注疏排布的「经」位用。
    /// `code_lang` 取产物自带的 LLM 判定（`outcome.code_language`），
    /// 采纳门槛见 `plausible_code_language`，缺失时不兜底、为 `None`。
    Outcome {
        /// 触发时选中的原文。
        source: String,
        /// 产物本体。
        outcome: TaskOutcome,
        /// 代码语言（由产物判定现算，不经流式视图中转）。
        code_lang: Option<String>,
    },
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
/// [`crate::ui::i18n::ErrorText`]），不随任务冻结。
#[derive(Debug, Clone, PartialEq)]
pub enum FailureCause {
    /// 任务链路返回的错误。
    Task(GlossError),
    /// 推理通道不可用（通道③发送失败）。
    TransportChannel,
}

/// `commit_selection` 的结果：下发 / 被内容闸门拦下 / 不采纳。三态而非 `Option`
/// ——「内容疑似敏感」与「陈旧丢弃」在壳侧要做不同的事（前者要记一行 warn，
/// 后者只记 debug），合并成 `None` 就分不出来了。
#[derive(Debug, Clone, PartialEq)]
pub enum InputOutcome {
    /// 任务已组装，壳经通道③下发。
    Dispatch(RunRequest),
    /// 内容疑似敏感：探测丢弃（当前显示保留，它属于上一个会话）。
    Blocked(SensitiveKind),
    /// 陈旧探测编号或模态错配：不采纳，浮层与通道都不动。
    Ignored,
}

/// 失败处置的结果：弹失败卡 / 静默丢弃 / 不采纳。三态而非 bool
/// ——「静默丢弃」与「陈旧丢弃」在壳侧要做不同的事：前者要留排查日志
/// （划词探测失败的唯一痕迹），后者只记 debug。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureOutcome {
    /// 已落 `Error` 态，浮层弹失败卡（壳记 warn）。
    Shown,
    /// 划词探测遇「无选区可读」：纯误滑——探测丢弃、不弹卡，状态机与
    /// 当前显示一律不动（壳只记一条带前台应用的 info）。
    SilentlyDropped,
    /// 探测编号不符、代数陈旧或已隐藏：不采纳，浮层与状态都不动（壳记 info）。
    Ignored,
}

/// `commit_selection` 采纳取材产物后的下发请求：壳把它经通道③发送。
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

/// 探测时定下的任务选项：一次配置快照解析出全部参数，产物到达（提交）
/// 时随请求下发——单次任务的配置从探测那一刻起就固定了（「单次任务内
/// 配置一致」），取材途中换配置不会让同一个任务用上两个版本的参数。
/// 任务类型不在其中：kind 由 LLM 层在执行前分类。
#[derive(Debug, Clone, PartialEq)]
struct PendingOptions(TaskOptions);

/// 在途的划词探测：探测编号与触发时冻结的任务选项。探测段不进状态机
/// ——产物到达时经 [`TaskStateMachine::commit_selection`] 才接管状态。
#[derive(Debug, Clone, PartialEq)]
struct ProbeTask {
    /// 探测编号：与代数同一计数器分配，提交时提升为代数。
    id: u64,
    /// 探测时冻结的任务选项。
    pending: PendingOptions,
}

/// 任务状态机：纯状态 + 决策，无 IO，可全时序驱动。
#[derive(Debug, Default)]
pub struct TaskStateMachine {
    /// 代数与探测编号共用的单调计数器：`begin_selection_probe` 从这里
    /// 领号，永不复用。
    next_id: u64,
    /// 当前已提交会话的代数：`accept_chunk`/`accept_done`/`accept_failed`
    /// 的陈旧过滤基准。探测编号在提交时才提升为代数。
    generation: u64,
    state: AppState,
    /// 在途的划词探测：取材在状态机之外进行，产物到达时提交或丢弃。
    /// 新探测与收起都会替换或清掉它。
    probe: Option<ProbeTask>,
    /// 在途推理的取消令牌：提交时取消旧任务（唯一取消机制）。
    current_cancel: Option<CancellationToken>,
    /// 当前任务的请求副本（input + options）：推理期间随行，可重试失败后
    /// 留在 Error 态供 [`TaskStateMachine::retry`] 原样重发；完成、隐藏与
    /// 不可重试失败即清。
    active_request: Option<(TaskInput, TaskOptions)>,
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

    /// 在途划词探测的编号；无探测时为 `None`。壳据此把通道④的回传分流
    /// 到探测提交段（探测编号在提交时才提升为代数，不能按代数匹配）。
    pub fn probe_id(&self) -> Option<u64> {
        self.probe.as_ref().map(|probe| probe.id)
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

    /// 划词探测的入口（探测段）：只领探测编号、冻结任务选项并返回取材
    /// 命令——**不改状态、不换视图、不取消在途任务**。取材在状态机之外
    /// 进行：产物到达走 [`Self::commit_selection`]，失败走
    /// [`Self::commit_selection_failed`]。已显示的内容与在途推理全程无感，
    /// 误滑（取不到内容）因此零干扰。`config` 是壳在事件起手处取的配置
    /// 快照，任务选项按它解析并随探测冻结；`system_locale` 是壳在启动期
    /// 读到的系统语言，供配置里的 `Language::System` 落定；`scene` 是壳在
    /// 触发前读到的场景事实，供敏感场景闸门判定（见 [`trigger_decision`]）。
    /// 未接线事件与被闸门拦下的手势返回 None 且无任何副作用。新探测替换
    /// 旧探测（旧探测的迟到产物经编号过滤丢弃）。
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
        });
        Some(AcquireCommand::AcquireText { generation: id })
    }

    /// 提交划词探测（提交段）：探测编号提升为代数，旧会话让位（在途推理
    /// 取消、请求副本作废，迟到的旧产物经代数过滤丢弃），视图整卡换成
    /// 流式视图并进入 `Translating`。配置取探测时冻结的那份（不经参数再
    /// 传配置）。
    ///
    /// 内容闸门命中时探测作废但**当前显示保留**：它属于上一个会话，误划
    /// 与敏感内容都不该把它顶掉（壳据 [`InputOutcome::Blocked`] 记一行
    /// warn）。**没有放行出口**——防护不交由用户控制，命中就是发送不成。
    /// 编号不符（陈旧探测）或模态错配不消费探测：同编号的后续合法产物
    /// 仍可提交。
    pub fn commit_selection(&mut self, probe_id: u64, input: TaskInput) -> InputOutcome {
        if self.probe.as_ref().is_none_or(|probe| probe.id != probe_id) {
            return InputOutcome::Ignored;
        }
        // 先校验模态再消费探测：模态错配不吃掉待下发任务，同编号的
        // 后续合法产物仍可提交。
        let TaskInput::Text { text } = input else {
            return InputOutcome::Ignored;
        };
        let Some(ProbeTask {
            id,
            pending: PendingOptions(options),
        }) = self.probe.take()
        else {
            return InputOutcome::Ignored;
        };
        if let Some(reason) = guard::detect_sensitive(&text) {
            return InputOutcome::Blocked(reason);
        }
        // 提交即接管：旧在途任务让位，不留滞留的死数据。
        if let Some(cancel) = self.current_cancel.take() {
            cancel.cancel();
        }
        self.active_request = None;
        self.generation = id;
        // 视图与任务各要一份原文；语言判定不在此做——只认产物回传的
        // `code_language`（流式期无角标、按通用启发集着色）。
        let input = TaskInput::Text { text: text.clone() };
        self.overlay_view = Some(OverlayView::Streaming {
            source: text,
            raw: String::new(),
            classified: None,
            code_lang: None,
        });
        self.state = AppState::Translating;
        InputOutcome::Dispatch(self.begin_run(input, options))
    }

    /// 探测失败的处置：空选区/读不到按误滑静默丢弃（探测清掉，状态机与
    /// 当前显示一律不动——壳只记一条带前台应用的排查日志）；其余失败
    /// （权限缺失等）按 [`Self::commit_selection`] 同样的接管语义落失败卡
    /// （显式反馈；真实故障不该被吞掉）。编号不符按陈旧丢弃。
    pub fn commit_selection_failed(&mut self, probe_id: u64, error: &GlossError) -> FailureOutcome {
        if self.probe.as_ref().is_none_or(|probe| probe.id != probe_id) {
            return FailureOutcome::Ignored;
        }
        self.probe = None;
        if matches!(
            error,
            GlossError::SelectionUnavailable | GlossError::SelectionEmpty
        ) {
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
        // 故障的 fail_* 降级路径）一律清掉，retry() 自然无从发起。副本
        // 缺失时（取材路径若回传可重试错误）不给 Retry 出口——别摆一颗
        // 点了没反应的死按钮。
        let mut action = error_action(error);
        if action == Some(ErrorAction::Retry) {
            if self.active_request.is_none() {
                action = None;
            }
        } else {
            self.active_request = None;
        }
        self.state = AppState::Error;
        self.overlay_view = Some(OverlayView::Failed {
            cause: FailureCause::Task(error.clone()),
            action,
        });
    }

    /// 采纳流式增量：追加到流式视图的原始回复（`note` 的渐进提取在
    /// 渲染层）。返回是否有新内容需要重绘。
    ///
    /// 这份累积只服务**流式显示**；与桥侧完成态的并存约定及权威源
    /// 见 `crate::runtime::pipeline` 的模块文档（done 以 outcome.note 整卡覆盖）。
    pub fn accept_chunk(&mut self, generation: u64, delta: String) -> bool {
        if generation != self.generation || self.state != AppState::Translating {
            return false;
        }
        if let Some(OverlayView::Streaming { raw, .. }) = &mut self.overlay_view {
            raw.push_str(&delta);
        }
        true
    }

    /// 采纳任务产物：定格注文并进入 `Show`。返回是否需要重绘。
    ///
    /// `outcome.note` 直接覆盖流式视图（权威源约定见 `crate::runtime::pipeline`）；
    /// 原文从流式视图随行进产物卡（经注疏的「经」位），完成态保有原文
    /// 对照；代码语言只认产物的 LLM 判定（唯一来源）。
    pub fn accept_done(&mut self, generation: u64, outcome: TaskOutcome) -> bool {
        if generation != self.generation || self.state != AppState::Translating {
            return false;
        }
        let source = match std::mem::take(&mut self.overlay_view) {
            Some(OverlayView::Streaming { source, .. }) => source,
            _ => String::new(),
        };
        // 代码语言来自 LLM 的判定（随产物到达）：修剪首尾空白后过词元
        // 门槛，再经归一化（别名/大小写）采用。判定缺失或是散文/示例占位
        // 串时不做任何本地兜底——产物卡无角标、按通用启发集着色，自由
        // 文本不上角标。
        let code_lang = outcome
            .code_language
            .as_deref()
            .map(str::trim)
            .filter(|verdict| plausible_code_language(verdict))
            .and_then(crate::ui::code_hl::normalize_language);
        self.active_request = None;
        self.overlay_view = Some(OverlayView::Outcome {
            source,
            outcome,
            code_lang,
        });
        self.state = AppState::Show;
        true
    }

    /// 采纳自动分类结果：把判定的任务类型写进流式视图（含分类失败时
    /// LLM 层给的兜底 kind——排版如实反映即将执行的任务）。返回是否需要
    /// 重绘。陈旧代数、非推理态或非流式视图不采纳（迟到标签不得落在已
    /// 定格的产物卡上）。
    pub fn accept_classified(&mut self, generation: u64, kind: TaskKind) -> bool {
        if generation != self.generation || self.state != AppState::Translating {
            return false;
        }
        if let Some(OverlayView::Streaming { classified, .. }) = &mut self.overlay_view {
            *classified = Some(kind);
            return true;
        }
        false
    }

    /// 采纳任务失败：落 `Error` 态并展示失败信息与动作出口（错误
    /// 映射：可重试类带重试按钮并保留请求副本，配置/鉴权类引导去设置页）。
    /// 这是推理路径（`Translating` 收推理失败）；划词探测的失败走
    /// [`Self::commit_selection_failed`]。其余状态不采纳——失败卡不得把
    /// 已收起的浮层弹回。返回处置结果，壳按 [`FailureOutcome`] 区分日志
    /// 与窗口动作。
    pub fn accept_failed(&mut self, generation: u64, error: &GlossError) -> FailureOutcome {
        if generation != self.generation || self.state != AppState::Translating {
            return FailureOutcome::Ignored;
        }
        self.current_cancel = None;
        self.land_error(error);
        FailureOutcome::Shown
    }

    /// 重试失败卡上的任务（Error 态）：原样重发失败的那个请求（同代数
    /// ——旧任务的流已随首个错误终结，不会有两路同代回传），浮层回到
    /// 流式视图。非 Error 态或无可重试请求时返回 `None`。
    pub fn retry(&mut self) -> Option<RunRequest> {
        if self.state != AppState::Error {
            return None;
        }
        // 取走在途请求原件：`begin_run` 会把请求副本重新记回在途，这里
        // 无需先复制一份。
        let (input, options) = self.active_request.take()?;
        self.overlay_view = Some(streaming_view(&input));
        self.state = AppState::Translating;
        Some(self.begin_run(input, options))
    }

    /// 下发一个任务的共用半边：换新取消令牌并把请求副本记在途（失败可
    /// 重试）。视图由调用方先定好——只有调用方知道这次下发用哪个视图。
    fn begin_run(&mut self, input: TaskInput, options: TaskOptions) -> RunRequest {
        let cancel = CancellationToken::new();
        self.current_cancel = Some(cancel.clone());
        self.active_request = Some((input.clone(), options.clone()));
        RunRequest {
            generation: self.generation,
            input,
            options,
            cancel,
        }
    }

    /// 浮层收起（Esc / 关闭按钮）即放弃在途任务：取消令牌（唯一取消机
    /// 制）、清空视图并回 `Idle`；在途划词探测一并作废（收起后迟到的
    /// 探测产物不得把浮层弹回）。放弃后的迟到产物经代数或状态守卫丢弃
    /// ——为一个不可见的浮层继续推理与渲染纯属空转；重新划词即重新开
    /// 始，取材自当前选区（旧产物本就可能已过期）。
    pub fn hide_overlay(&mut self) {
        if let Some(cancel) = self.current_cancel.take() {
            cancel.cancel();
        }
        self.active_request = None;
        self.probe = None;
        self.state = AppState::Idle;
        self.overlay_view = None;
    }

    /// 推理通道不可用（③发送失败）时的降级：直接落 `Error` 态。通道
    /// 已死时重发只会再死一次，失败卡不带重试按钮。
    pub fn fail_transport(&mut self, generation: u64) {
        if generation != self.generation || self.state != AppState::Translating {
            return;
        }
        self.current_cancel = None;
        self.active_request = None;
        self.state = AppState::Error;
        self.overlay_view = Some(OverlayView::Failed {
            cause: FailureCause::TransportChannel,
            action: None,
        });
    }
}

/// 错误 → 失败卡动作（映射表）：网络/限流可原样重试；鉴权、模态
/// 与配置错误都要进设置页才能修（模型、密钥的修改入口在设置页）；
/// 其余类别没有按钮意义上的出口——权限类引导已写在文案里，协议
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

/// 一次下发请求的流式视图起点：原文照抄、正文空、未分类（LLM 层的
/// `TaskClassified` 到达后精化）、无语言判定（语言只认产物回传的
/// `code_language`，流式期按通用启发集着色、无角标）。重试与首次下发
/// 共用。
fn streaming_view(input: &TaskInput) -> OverlayView {
    let TaskInput::Text { text } = input else {
        return OverlayView::Streaming {
            source: String::new(),
            raw: String::new(),
            classified: None,
            code_lang: None,
        };
    };
    OverlayView::Streaming {
        source: text.clone(),
        raw: String::new(),
        classified: None,
        code_lang: None,
    }
}

/// LLM 代码语言判定的采纳门槛：语言标识应是单个 ASCII 词元（"rust"、
/// "c++"、"objective-c" 这个形状）。内部含空白或非 ASCII 字符的判定是
/// 散文或示例占位串（模型照抄「…或 null」），不可上角标——交由调用方
/// 落回无语言（无角标、通用启发集着色）；首尾空白由调用方先行修剪。
fn plausible_code_language(verdict: &str) -> bool {
    !verdict.is_empty()
        && verdict
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '#' | '-' | '.'))
}

/// 一次平台事件的去向：取材，或被闸门拦下。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerDecision {
    /// 放行：场景闸门放行，事件进入取材（划词不带显式意图，类型由 LLM
    /// 层分类决定）。
    Acquire,
    /// 拦下：触发前场景闸门（安全输入态 / 敏感应用名单）。只拦真实触发，
    /// 设置与退出不在此列。
    Blocked(TriggerBlock),
    /// 拦下：Gloss 自身是前台应用时的划词手势（防误触——HID 全局 tap 连拖
    /// Gloss 自己的浮层不该触发取材）。
    SelfSuppressed,
    /// 不进状态机的直通事件（框选、设置、退出、授权落定——各自在 drain
    /// 处有自己的出口）。
    Unwired,
}

/// 一次平台事件的去向判定：场景闸门在这里做一次（上游据此记不同级别
/// 的日志与不同的动作）。手势分支先判自身前台（防误触，与敏感防护无关、
/// 级别也不同）再过场景闸门。
pub fn trigger_decision(event: &PlatformEvent, scene: &SceneFacts) -> TriggerDecision {
    match event {
        // 划词手势不带显式意图：类型由 LLM 层在执行前分类。场景闸门照常生效。
        PlatformEvent::SelectionGesture { .. } => {
            if scene.front_app.as_ref().is_some_and(|app| app.is_self) {
                return TriggerDecision::SelfSuppressed;
            }
            match guard::trigger_block(scene) {
                Some(block) => TriggerDecision::Blocked(block),
                None => TriggerDecision::Acquire,
            }
        }
        PlatformEvent::RegionGesture { .. }
        | PlatformEvent::OpenSettingsRequested
        | PlatformEvent::QuitRequested
        // 授权落定在 drain 处直通处理（解预热门控），不经状态机。
        | PlatformEvent::AccessibilityGranted => TriggerDecision::Unwired,
    }
}

/// 按配置快照解析任务选项：目标语言取配置默认；模型取配置的单一项；
/// prompt 模板语言按 `Language` 落定（`System` 取系统语言）——三者都是
/// LLM 层的输入，随任务冻结，执行途中不再回读配置。
fn task_options(config: &Config, system_locale: Locale) -> TaskOptions {
    TaskOptions {
        target_lang: Some(config.target_lang.clone()),
        model: config.model.clone(),
        prompt_locale: Some(config.language.resolve(system_locale)),
        ..Default::default()
    }
}

/// 回传事件的类别标签（露面策略只关心类别，不关心代数与载荷）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventKind {
    InputReady,
    TaskClassified,
    TaskChunk,
    TaskDone,
    TaskFailed,
    /// 密钥预热回执：与任务代数无关，露面策略恒不显示（壳层旁路留痕）。
    SecretPrewarmed,
}

/// 取回传事件的类别标签。
pub(crate) fn event_kind(event: &Event) -> EventKind {
    match event {
        Event::InputReady { .. } => EventKind::InputReady,
        Event::TaskClassified { .. } => EventKind::TaskClassified,
        Event::TaskChunk { .. } => EventKind::TaskChunk,
        Event::TaskDone { .. } => EventKind::TaskDone,
        Event::TaskFailed { .. } => EventKind::TaskFailed,
        Event::SecretPrewarmed { .. } => EventKind::SecretPrewarmed,
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
        // 预热回执由壳层旁路留痕，与任务浮层无关。
        EventKind::SecretPrewarmed => false,
    }
}

/// 一批回传之后浮层要不要自动露面：**任一**事件判为要显示就显示。
pub(crate) fn auto_show_after(batch: impl IntoIterator<Item = (EventKind, bool)>) -> bool {
    batch
        .into_iter()
        .any(|(kind, accepted)| auto_show_for(kind, accepted))
}

/// 一批回传处理后浮层要不要显形：挂起显形请求（`pending`）要求机器确有
/// 视图——视图为空时弹出的会是渲染自检卡（挂起置位与消费之间没有插入
/// 点，两段 drain 同帧连跑，守卫只为防御）；划词提交的置位在批内
/// `commit_probe`，同一帧消费。挂起显形不依赖回传批次，与批次内的
/// 「失败即弹」任一成立即显示。
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
    use gloss_core::model::{GlossError, Lang, ScreenPoint, ScreenRect};
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
            code_language: None,
            structured: OutcomeStructured::Plain {
                examples: Vec::new(),
            },
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
            PlatformEvent::RegionGesture {
                rect: ScreenRect {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 10,
                },
            },
            PlatformEvent::OpenSettingsRequested,
            PlatformEvent::QuitRequested,
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
            trigger_decision(
                &PlatformEvent::RegionGesture {
                    rect: ScreenRect {
                        x: 0,
                        y: 0,
                        width: 10,
                        height: 10,
                    },
                },
                &open
            ),
            TriggerDecision::Unwired,
            "the capture gesture has no acquisition path yet"
        );
        assert_eq!(
            trigger_decision(&PlatformEvent::QuitRequested, &blocked_by_app()),
            TriggerDecision::Unwired,
            "quit is an exit, never a gated trigger"
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

        let request = dispatched(machine.commit_selection(1, text_input("fn main() {}")));
        assert_eq!(
            request.options.model, "frozen-model",
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
        let request = dispatched(explicit.commit_selection(1, text_input("hello")));
        assert_eq!(
            request.options.prompt_locale,
            Some(Locale::En),
            "an explicit choice ignores the injected system language"
        );

        let mut following = TaskStateMachine::new();
        let factory = Config::default();
        assert_eq!(factory.language, Language::System);
        assert_eq!(probe_using(&mut following, &factory, Locale::En), 1);
        let request = dispatched(following.commit_selection(1, text_input("hello")));
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
        let request = dispatched(machine.commit_selection(1, text_input("hello")));
        assert_eq!(
            request.options.target_lang,
            Some(Lang::Ja),
            "in-flight task must keep the snapshot taken at probe time"
        );
        assert_eq!(
            request.options.model, "before-model",
            "the model is frozen with the rest of the options"
        );
        assert_eq!(
            request.options.prompt_locale,
            Some(Locale::En),
            "the prompt locale is frozen with the rest of the options"
        );
        assert_eq!(probe(&mut machine, &after), 2);
        let request = dispatched(machine.commit_selection(2, text_input("world")));
        assert_eq!(request.options.target_lang, Some(Lang::Ko));
        assert_eq!(request.options.model, "after-model");
        assert_eq!(request.options.prompt_locale, Some(Locale::Zh));
    }

    #[test]
    fn commit_selection_yields_run_request_and_supersedes_the_previous_session() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);

        let request = dispatched(machine.commit_selection(1, text_input("hello")));
        assert_eq!(request.generation, 1);
        assert_eq!(machine.generation(), 1, "the probe id is promoted");
        assert_eq!(machine.state(), AppState::Translating);
        assert!(machine.current_cancel().is_some());
        assert!(machine.probe_id().is_none(), "the probe is consumed");

        let old_cancel = request.cancel;
        assert_eq!(probe(&mut machine, &Config::default()), 2);
        assert_eq!(machine.state(), AppState::Translating, "探测不动当前会话");
        dispatched(machine.commit_selection(2, text_input("world")));
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
                machine.commit_selection(2, text_input("again")),
                InputOutcome::Ignored
            ),
            "a consumed probe accepts no duplicate commit"
        );
    }

    #[test]
    fn a_probe_leaves_a_visible_session_completely_untouched() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let request = dispatched(machine.commit_selection(1, text_input("hello")));
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

        let outcome = machine.commit_selection_failed(2, &GlossError::SelectionEmpty);
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
            machine.commit_selection(
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
    fn classified_kind_updates_the_streaming_chip_only_once_current() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_selection(1, text_input("hello")));

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

        machine.accept_done(
            1,
            TaskOutcome {
                kind: TaskKind::TranslateWord,
                note: "产物".into(),
                code_language: None,
                structured: OutcomeStructured::Plain {
                    examples: Vec::new(),
                },
            },
        );
        assert!(
            !machine.accept_classified(1, TaskKind::ExplainCode),
            "a late classification must not touch a settled outcome card"
        );
    }

    #[test]
    fn the_streaming_view_waits_unclassified_until_the_llm_layer_reports() {
        let mut machine = TaskStateMachine::new();
        let probe_id = probe(&mut machine, &Config::default());
        dispatched(machine.commit_selection(
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
                    code_lang: None,
                    ..
                })
            ),
            "the streaming view stays unclassified until the LLM layer reports"
        );
    }

    #[test]
    fn code_language_comes_only_from_the_llm_verdict() {
        let mut machine = TaskStateMachine::new();
        let probe_id = probe(&mut machine, &Config::default());
        dispatched(machine.commit_selection(
            probe_id,
            TaskInput::Text {
                text: "fn main() {}".into(),
            },
        ));
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Streaming {
                    code_lang: None,
                    ..
                })
            ),
            "no content probing: the streaming view carries no language"
        );

        // LLM 判定随产物到达：唯一来源。
        machine.accept_done(
            1,
            TaskOutcome {
                kind: TaskKind::ExplainCode,
                note: "产物".into(),
                code_language: Some("python".into()),
                structured: OutcomeStructured::Plain {
                    examples: Vec::new(),
                },
            },
        );
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Outcome {
                    code_lang: Some(lang),
                    ..
                }) if lang == "python"
            ),
            "the settled outcome card carries the LLM's verdict"
        );

        // 判定缺失：不做本地兜底，产物卡无语言。
        let mut machine = TaskStateMachine::new();
        let probe_id = probe(&mut machine, &Config::default());
        dispatched(machine.commit_selection(
            probe_id,
            TaskInput::Text {
                text: "fn main() {}".into(),
            },
        ));
        machine.accept_done(
            1,
            TaskOutcome {
                kind: TaskKind::ExplainCode,
                note: "产物".into(),
                code_language: None,
                structured: OutcomeStructured::Plain {
                    examples: Vec::new(),
                },
            },
        );
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Outcome {
                    code_lang: None,
                    ..
                })
            ),
            "a missing verdict leaves the outcome card without a language"
        );

        // 占位串判定（散文/示例占位）同样不上角标。
        let mut machine = TaskStateMachine::new();
        let probe_id = probe(&mut machine, &Config::default());
        dispatched(machine.commit_selection(
            probe_id,
            TaskInput::Text {
                text: "fn main() {}".into(),
            },
        ));
        machine.accept_done(
            1,
            TaskOutcome {
                kind: TaskKind::ExplainCode,
                note: "产物".into(),
                code_language: Some("…或 null".into()),
                structured: OutcomeStructured::Plain {
                    examples: Vec::new(),
                },
            },
        );
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Outcome {
                    code_lang: None,
                    ..
                })
            ),
            "a placeholder verdict is not adopted as a language"
        );

        // 首尾空白先修剪再过门槛（不因空白误判成占位串）。
        let mut machine = TaskStateMachine::new();
        let probe_id = probe(&mut machine, &Config::default());
        dispatched(machine.commit_selection(
            probe_id,
            TaskInput::Text {
                text: "fn main() {}".into(),
            },
        ));
        machine.accept_done(
            1,
            TaskOutcome {
                kind: TaskKind::ExplainCode,
                note: "产物".into(),
                code_language: Some(" python\n".into()),
                structured: OutcomeStructured::Plain {
                    examples: Vec::new(),
                },
            },
        );
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Outcome {
                    code_lang: Some(lang),
                    ..
                }) if lang == "python"
            ),
            "a whitespace-padded verdict is trimmed before the plausibility gate"
        );
    }

    #[test]
    fn hide_abandons_inflight_and_drops_late_events() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let request = dispatched(machine.commit_selection(1, text_input("hello")));
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
                machine.commit_selection(1, text_input("迟到的产物")),
                InputOutcome::Ignored
            ),
            "a probe dropped by hide must not resurrect the overlay"
        );
        assert_eq!(
            machine.commit_selection_failed(1, &GlossError::SelectionEmpty),
            FailureOutcome::Ignored
        );
    }

    #[test]
    fn failed_guard_matches_translating_only() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_selection(1, text_input("hello")));
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
                machine.commit_selection_failed(1, &error),
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
            machine.commit_selection_failed(1, &GlossError::AccessibilityDenied),
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
        dispatched(machine.commit_selection(1, text_input("hello")));
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
                machine.commit_selection(1, text_input("旧探测的产物")),
                InputOutcome::Ignored
            ),
            "a superseded probe must not commit"
        );
        assert_eq!(
            machine.commit_selection_failed(1, &GlossError::SelectionUnavailable),
            FailureOutcome::Ignored
        );
    }

    #[test]
    fn modality_mismatch_preserves_pending_options() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        assert!(matches!(
            machine.commit_selection(
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
                machine.commit_selection(1, text_input("第二次")),
                InputOutcome::Dispatch(_)
            ),
            "pending options must survive a modality mismatch"
        );
    }

    #[test]
    fn transport_failure_lands_in_error() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let request = dispatched(machine.commit_selection(1, text_input("x")));
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
        let original = dispatched(machine.commit_selection(1, text_input("hello")));
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
        dispatched(machine.commit_selection(1, text_input("x")));

        assert_eq!(
            machine.accept_failed(1, &GlossError::EngineRateLimited),
            FailureOutcome::Shown,
            "rate limited is retryable"
        );
        assert!(machine.retry().is_some());

        assert_eq!(probe(&mut machine, &Config::default()), 2);
        dispatched(machine.commit_selection(2, text_input("x")));
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
        dispatched(machine.commit_selection(3, text_input("x")));
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
        dispatched(machine.commit_selection(4, text_input("x")));
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
    }

    #[test]
    fn new_commit_and_hide_supersede_the_retry_request() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_selection(1, text_input("x")));
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
        dispatched(machine.commit_selection(2, text_input("y")));
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
        let settled = dispatched(machine.commit_selection(1, text_input("上一次的普通文本")));
        assert!(machine.accept_done(1, plain_outcome("上一次的产物")));
        let view_before = machine.overlay_view().cloned();

        assert_eq!(probe(&mut machine, &Config::default()), 2);
        let outcome = machine.commit_selection(2, text_input(&suspicious_text()));
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
        machine.commit_selection(1, text_input("card 4111 1111 1111 1111"));

        assert!(
            machine.retry().is_none(),
            "a refused probe is not a retryable failure"
        );
        assert!(matches!(
            machine.commit_selection(1, text_input("second arrival")),
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
            machine.commit_selection(2, text_input(&suspicious_text())),
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
                machine.commit_selection(1, text_input("今天下午三点开会")),
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
