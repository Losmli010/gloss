//! 显示与触发策略（纯函数）：平台事件的去向判定、错误→动作映射、任务
//! 选项构造与浮层露面策略。状态迁移归 `machine` 本体，这里只回答「要不要
//! /去哪里」。

use std::sync::Arc;

use gloss_core::config::Config;
use gloss_core::guard::{self, SceneFacts, TriggerBlock};
use gloss_core::model::{GlossError, Locale};
use gloss_core::task::{TaskInput, TaskOptions};

use crate::channel::{Event, PlatformEvent};
use crate::machine::types::{ErrorAction, OverlayView};

/// 错误 → 失败卡动作（映射表）：网络/限流可原样重试；鉴权、模态、JSON 模式
/// 不支持与配置错误都要进设置页才能修（模型、密钥与模型服务的修改入口都在
/// 那里）；其余类别给不出有意义的按钮——权限类引导已写在文案里，协议异常重发
/// 同一个请求只会再错一次。
pub(super) fn error_action(error: &GlossError) -> Option<ErrorAction> {
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
pub(super) fn streaming_view(input: &TaskInput) -> OverlayView {
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
pub(super) fn task_options(config: &Config, system_locale: Locale) -> TaskOptions {
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
pub(super) fn auto_show_for(kind: EventKind, accepted: bool) -> bool {
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
