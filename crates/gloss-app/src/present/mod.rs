//! 呈现资源：窗口管理器、GPU 上下文与帧管线、呈现资源所有者。
//!
//! 被编排层驱动、被视图层使用的资源所有者；窗口与其渲染帧（winit 句柄 +
//! egui + wgpu surface）与已施加主题经 [`workspace`] 统一持有。

pub mod gpu;
pub(crate) mod render;
pub mod windows;
pub(crate) mod workspace;

// 帧管线的对外入口只此三件（根包 L3 自检与生产 init 消费）；渲染细节留在
// crate 内，render_frame 的返回类型一并可见。
pub use render::{Frame, OverlayFrameOutput, build_window_stack, render_frame};
