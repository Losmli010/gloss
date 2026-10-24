//! 呈现资源：窗口管理器、图形硬件（GPU）上下文与帧管线、呈现资源所有者。
//!
//! 被编排层驱动、被视图层使用的资源所有者；窗口与其渲染帧——含窗口库（winit）
//! 句柄、界面库（egui）与图形后端（wgpu）的绘制表面——与已施加主题经工作区资源模块统一持有。

pub mod gpu;
pub(crate) mod render;
pub mod windows;
pub(crate) mod workspace;

pub use render::{Frame, OverlayFrameOutput, build_window_stack, render_frame};
