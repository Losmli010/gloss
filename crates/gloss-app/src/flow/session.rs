//! 任务会话：一次划词触发到任务收敛期间的壳侧管线状态——坐标锚点、
//! 排查线索、显形挂起与任务日志段。与纯状态机分离：这里
//! 的东西是“管线上下文”，不是任务生命周期决策。

use gloss_core::guard::FrontApp;
use gloss_core::log::Span;
use gloss_core::model::ScreenPoint;

#[derive(Default)]
pub(crate) struct Session {
    pub(crate) selection_anchor: Option<(u64, ScreenPoint)>,
    pub(crate) probe_front_app: Option<FrontApp>,
    pub(crate) pending_reveal: bool,
    pub(crate) task_span: Option<(u64, Span)>,
}

impl Session {
    pub(crate) fn span_for(&self, generation: u64) -> Option<Span> {
        self.task_span
            .as_ref()
            .filter(|(current, _)| *current == generation)
            .map(|(_, span)| span.clone())
    }
}
