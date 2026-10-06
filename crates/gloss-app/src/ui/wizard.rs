//! 启动向导窗口的 UI：授权引导步骤与监听失效提示卡的渲染与会话状态。
//!
//! 决策（显示哪一步、何时收起）在 `flow::wizard`，这里只把当前视图画出
//! 来并把用户动作上交（打开设置面板 / 收起）——与设置窗同一「渲染不落
//! 盘」的分工。

use std::collections::VecDeque;

use egui::Ui;

use crate::ui::i18n::Text;
use crate::ui::style::{font, space};

/// 向导引导的授权步骤：顺序即划词链路的依赖顺序（辅助功能管选区读取，
/// 输入监控管手势监听）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WizardStep {
    /// 辅助功能：AX 选区读取的前提。
    Accessibility,
    /// 输入监控：划词手势 tap 的建立前提。
    InputMonitoring,
}

/// 向导窗口的当前视图：引导步骤或运行中失效提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WizardView {
    /// 启动引导序列里的某一步。
    Step(WizardStep),
    /// 鼠标监听运行中失效（`MouseListenerDegraded` 事件的呈现）。
    Degraded,
}

/// 向导会话状态：存在即窗口可见；步骤走完（预热发出）或用户收起即清，
/// 生命周期归 `flow::wizard`。
pub(crate) struct WizardState {
    /// 当前展示的视图。
    pub(crate) view: WizardView,
    /// 待展示的后续步骤（当前视图处理后按序弹出）。
    pub(crate) steps: VecDeque<WizardStep>,
    /// 密钥预热是否已发出（步骤走完即发；运行中失效插入的会话不再重发）。
    pub(crate) prewarm_sent: bool,
}

/// 向导窗口上交的动作：窗口只渲染，开面板与推进都在流程层。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WizardAction {
    /// 普通帧，无需壳动作。
    Idle,
    /// 打开该步骤对应的系统设置面板（流程侧随后推进向导）。
    OpenGuide(WizardStep),
    /// 收起当前视图（跳过该步 / 关闭失效提示），不打开面板。
    Dismiss,
}

/// 画一帧向导窗口：正文按当前视图取文案，按钮区上交动作。
pub(crate) fn draw(ui: &mut Ui, state: &WizardState, text: &Text) -> WizardAction {
    let (body, dismiss_label, guide) = match state.view {
        WizardView::Step(WizardStep::Accessibility) => (
            &text.gloss_wizard_accessibility_body,
            &text.gloss_wizard_skip,
            WizardStep::Accessibility,
        ),
        WizardView::Step(WizardStep::InputMonitoring) => (
            &text.gloss_wizard_input_monitoring_body,
            &text.gloss_wizard_skip,
            WizardStep::InputMonitoring,
        ),
        WizardView::Degraded => (
            &text.gloss_wizard_degraded_body,
            &text.gloss_wizard_dismiss,
            WizardStep::InputMonitoring,
        ),
    };
    let mut action = WizardAction::Idle;
    egui::Frame::new()
        .fill(ui.visuals().window_fill)
        .inner_margin(egui::Margin::same(WINDOW_PADDING))
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.add_space(space::SECTION);
                ui.label(egui::RichText::new(body.as_str()).size(font::BODY));
                ui.add_space(space::SECTION);
                // 按钮行按剩余宽度分成主次两格（对话框惯例的大点击面）。
                ui.horizontal(|ui| {
                    let open_width = ui.available_width() * 0.6;
                    if ui
                        .add_sized(
                            [open_width, 0.0],
                            egui::Button::new(text.gloss_wizard_open_settings.as_str()),
                        )
                        .clicked()
                    {
                        action = WizardAction::OpenGuide(guide);
                    }
                    if ui
                        .add_sized(
                            [ui.available_width(), 0.0],
                            egui::Button::new(dismiss_label.as_str()),
                        )
                        .clicked()
                    {
                        action = WizardAction::Dismiss;
                    }
                });
            });
        });
    action
}

/// 向导窗口内边距（与设置窗同值）。
const WINDOW_PADDING: i8 = 16;

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use egui_kittest::kittest::Queryable;
    use gloss_core::model::Locale;

    use crate::ui::i18n::Text;

    use super::{WizardAction, WizardState, WizardStep, WizardView, draw};

    fn state(view: WizardView) -> WizardState {
        WizardState {
            view,
            steps: VecDeque::new(),
            prewarm_sent: false,
        }
    }

    fn interactive_harness(
        state: WizardState,
        sink: Rc<RefCell<WizardAction>>,
    ) -> egui_kittest::Harness<'static> {
        let text = Text::get(Locale::En);
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            let frame_action = draw(ui, &state, text);
            if frame_action != WizardAction::Idle {
                *sink.borrow_mut() = frame_action;
            }
        });
        harness.set_size(egui::vec2(420.0, 200.0));
        harness
    }

    fn snapshot_harness(state: WizardState) -> egui_kittest::Harness<'static> {
        let text = Text::get(Locale::En);
        let mut harness = egui_kittest::Harness::builder()
            .with_theme(egui::Theme::Light)
            .build_ui(move |ui| {
                let _ = draw(ui, &state, text);
            });
        harness.set_size(egui::vec2(420.0, 200.0));
        harness
    }

    #[test]
    fn open_settings_button_reports_the_step_guide() {
        let action = Rc::new(RefCell::new(WizardAction::Idle));
        let mut harness = interactive_harness(
            state(WizardView::Step(WizardStep::Accessibility)),
            Rc::clone(&action),
        );
        harness.run();
        harness.get_by_label("Open System Settings").click();
        harness.run();
        assert_eq!(
            *action.borrow(),
            WizardAction::OpenGuide(WizardStep::Accessibility)
        );
    }

    #[test]
    fn the_degraded_notice_dismisses_without_a_pane() {
        let action = Rc::new(RefCell::new(WizardAction::Idle));
        let mut harness = interactive_harness(state(WizardView::Degraded), Rc::clone(&action));
        harness.run();
        harness.get_by_label("Got it").click();
        harness.run();
        assert_eq!(*action.borrow(), WizardAction::Dismiss);
    }

    #[test]
    fn snapshots_match_baseline() {
        let mut results = egui_kittest::SnapshotResults::new();

        let mut harness = snapshot_harness(state(WizardView::Step(WizardStep::Accessibility)));
        harness.run();
        harness.get_by_label_contains("Accessibility");
        harness.snapshot("wizard_accessibility");
        results.extend_harness(&mut harness);

        let mut harness = snapshot_harness(state(WizardView::Step(WizardStep::InputMonitoring)));
        harness.run();
        harness.get_by_label_contains("Input Monitoring");
        harness.snapshot("wizard_input_monitoring");
        results.extend_harness(&mut harness);

        let mut harness = snapshot_harness(state(WizardView::Degraded));
        harness.run();
        harness.get_by_label_contains("stopped");
        harness.snapshot("wizard_degraded");
        results.extend_harness(&mut harness);

        results.unwrap();
    }
}
