//! 任务会话：一次划词触发到任务收敛期间的壳侧管线状态——坐标锚点、
//! 排查线索、显形挂起与任务 span。与纯状态机（`machine`）分离：这里
//! 的东西是「管线上下文」，不是任务生命周期决策。

use gloss_core::guard::FrontApp;
use gloss_core::log::Span;
use gloss_core::model::ScreenPoint;

/// 一次触发到任务收敛期间的会话状态；字段随各自的边界置位与清空。
#[derive(Default)]
pub(crate) struct Session {
    /// 最近一次划词触发的释放坐标（随触发记录代数）：浮层跟随划词位置用，
    /// 代数对不上（陈旧）时浮层回落居中。
    pub(crate) selection_anchor: Option<(u64, ScreenPoint)>,
    /// 在途划词探测触发时的前台应用标识：探测失败按误滑静默丢弃，这条
    /// 标识是「划了没反应」排查日志的唯一线索；探测提交/丢弃即清。
    pub(crate) probe_front_app: Option<FrontApp>,
    /// 触发即显挂起：占代数的触发置位，下一次 drain_events 消费
    /// （那里才有 ActiveEventLoop 可做定位与显示；显形判定在
    /// `machine::should_reveal`）。
    pub(crate) pending_reveal: bool,
    /// 当前任务的 span（触发点创建）与它所属的代数：随通道②③下发，让接收
    /// 线程的日志自动带上 `generation`。重试沿用同一个（代数不变）。
    pub(crate) task_span: Option<(u64, Span)>,
}

impl Session {
    /// 指定代数的任务 span（副本，供 `enter()` 借用）；代数不符或尚无任务时
    /// 为 `None`。
    pub(crate) fn span_for(&self, generation: u64) -> Option<Span> {
        self.task_span
            .as_ref()
            .filter(|(current, _)| *current == generation)
            .map(|(_, span)| span.clone())
    }
}
