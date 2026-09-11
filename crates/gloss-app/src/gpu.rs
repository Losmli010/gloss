//! wgpu 设备与渲染目标：设备全进程一份，每个窗口一份 surface。

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use egui_wgpu::wgpu;
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use gloss_core::log::{debug, error, info, thread, warn};
use winit::dpi::PhysicalSize;
use winit::window::Window;

/// egui 要把整个窗口画出来，纹理边长上限按 4K 屏取（与 egui-wgpu 自身的取值一致）
pub const MAX_TEXTURE_DIMENSION: u32 = 8192;

/// 浮层卡片之外的底色留空：窗口是透明的，卡片由 egui 自己画，圆角外露见桌面
const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

/// 设备级资源：instance / adapter / device / queue 全进程一份，所有窗口共用。
pub struct GpuContext {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl GpuContext {
    /// 同步等待 adapter 与 device 请求（pollster 就是为这一步引入的，06 §八）。
    pub fn new() -> Result<Self, GpuError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        // 不指定 compatible_surface：设备先于窗口建立。目标平台（Metal / DX12）
        // 上可呈现的适配器与这里选到的是同一个。
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))?;

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("gloss"),
                required_limits: wgpu::Limits {
                    max_texture_dimension_2d: MAX_TEXTURE_DIMENSION,
                    ..wgpu::Limits::default()
                },
                ..Default::default()
            }))?;

        let info = adapter.get_info();
        info!(thread = thread::UI, backend = ?info.backend, device = %info.name, "gpu ready");

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    pub const fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    pub const fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    pub const fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub const fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

/// 一帧的呈现结果，供上层决定要不要立刻重绘。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderStatus {
    /// 已提交并呈现
    Presented,
    /// 本帧跳过（取帧超时或窗口被遮挡），等下一次重绘
    Skipped,
    /// surface 配置已过期，本帧跳过并已重新配置
    Outdated,
}

/// 一个窗口的渲染目标：surface + 配置 + egui 渲染器。
///
/// egui 渲染器绑定 target format，而 format 由 surface 决定，所以渲染器随
/// surface 各持一份，不放在设备级；同一 adapter 下多个窗口选到的 format 相同，
/// 重复持有的只是几块缓冲区。
pub struct GpuSurface {
    context: Arc<GpuContext>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    /// 尺寸已变但还没重新配置，在下一帧取帧前补上
    needs_configure: bool,
}

impl GpuSurface {
    pub fn new(context: &Arc<GpuContext>, window: Arc<Window>) -> Result<Self, GpuError> {
        let surface = context.instance().create_surface(window.clone())?;
        let size = window.inner_size();
        let caps = surface.get_capabilities(context.adapter());

        let format = egui_wgpu::preferred_framebuffer_format(&caps.formats)
            .map_err(|err| GpuError::Surface(err.to_string()))?;
        // wgpu 自带的默认配置省去逐个字段填值，只覆盖我们关心的几项
        let mut config = surface
            .get_default_config(context.adapter(), size.width.max(1), size.height.max(1))
            .ok_or(GpuError::UnsupportedSurface)?;
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        config.format = format;
        config.alpha_mode = pick_alpha_mode(&caps.alpha_modes);
        config.view_formats = vec![format];

        let renderer = Renderer::new(context.device(), format, RendererOptions::default());
        surface.configure(context.device(), &config);

        Ok(Self {
            context: Arc::clone(context),
            surface,
            config,
            renderer,
            needs_configure: false,
        })
    }

    /// 窗口尺寸变化。宽高为 0（最小化）时 wgpu 拒绝配置，等有真实尺寸再配。
    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.configure();
    }

    /// 画一帧：应用 egui 的纹理增量 → 提交顶点与索引 → 渲染 → 呈现。
    pub fn render(
        &mut self,
        textures: egui::TexturesDelta,
        paint_jobs: &[egui::ClippedPrimitive],
        pixels_per_point: f32,
    ) -> RenderStatus {
        if self.needs_configure {
            self.configure();
        }

        // 窗口被遮挡或取帧超时时跳过本帧：隐藏的浮层正属于前者，
        // 这里不能报错，否则每次隐藏都会刷一条日志
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                self.needs_configure = true;
                frame
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.configure();
                return RenderStatus::Outdated;
            }
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                return RenderStatus::Skipped;
            }
            other => {
                error!(thread = thread::UI, status = ?other, "surface is unusable, skipping frame");
                return RenderStatus::Skipped;
            }
        };

        let device = self.context.device();
        let queue = self.context.queue();
        let screen = ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gloss frame"),
        });

        for (id, deltas) in &textures.set {
            for delta in deltas {
                self.renderer.update_texture(device, queue, *id, delta);
            }
        }
        let user_buffers =
            self.renderer
                .update_buffers(device, queue, &mut encoder, paint_jobs, &screen);

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR_COLOR),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // egui 的 renderer 要求 'static 生命周期的 pass；交出生命周期后
            // 本作用域内不再碰 encoder，运行时也不会有别名操作
            self.renderer
                .render(&mut pass.forget_lifetime(), paint_jobs, &screen);
        }

        let encoded = encoder.finish();
        queue.submit(user_buffers.into_iter().chain([encoded]));
        // 释放必须排在 submit 之后：这批命令可能还在用它们
        for id in &textures.free {
            self.renderer.free_texture(id);
        }
        queue.present(frame);

        if self.needs_configure {
            debug!(thread = thread::UI, "surface was suboptimal, reconfiguring");
            self.configure();
        }
        RenderStatus::Presented
    }

    fn configure(&mut self) {
        self.surface.configure(self.context.device(), &self.config);
        self.needs_configure = false;
    }
}

/// 透明窗口要选一个带 alpha 的合成模式，否则圆角外会露出黑边。
///
/// 预乘优先：egui 输出的是预乘 alpha 的颜色。
fn pick_alpha_mode(modes: &[wgpu::CompositeAlphaMode]) -> wgpu::CompositeAlphaMode {
    let has = |mode| modes.contains(&mode);
    if has(wgpu::CompositeAlphaMode::PreMultiplied) {
        wgpu::CompositeAlphaMode::PreMultiplied
    } else if has(wgpu::CompositeAlphaMode::PostMultiplied) {
        wgpu::CompositeAlphaMode::PostMultiplied
    } else {
        warn!(
            thread = thread::UI,
            "surface has no alpha-capable composite mode, popup corners will not be transparent"
        );
        wgpu::CompositeAlphaMode::Auto
    }
}

/// GPU 初始化与 surface 失败。
#[derive(Debug)]
pub enum GpuError {
    /// 没有可用的图形适配器
    NoAdapter(String),
    /// 请求逻辑设备失败
    Device(String),
    /// 创建 surface 或选择其格式失败
    Surface(String),
    /// 该适配器不支持这个窗口的 surface
    UnsupportedSurface,
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter(detail) => write!(f, "no suitable gpu adapter: {detail}"),
            Self::Device(detail) => write!(f, "failed to request gpu device: {detail}"),
            Self::Surface(detail) => write!(f, "failed to set up surface: {detail}"),
            Self::UnsupportedSurface => write!(f, "surface is not supported by the adapter"),
        }
    }
}

impl Error for GpuError {}

impl From<wgpu::RequestAdapterError> for GpuError {
    fn from(err: wgpu::RequestAdapterError) -> Self {
        Self::NoAdapter(err.to_string())
    }
}

impl From<wgpu::RequestDeviceError> for GpuError {
    fn from(err: wgpu::RequestDeviceError) -> Self {
        Self::Device(err.to_string())
    }
}

impl From<wgpu::CreateSurfaceError> for GpuError {
    fn from(err: wgpu::CreateSurfaceError) -> Self {
        Self::Surface(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_mode_prefers_premultiplied() {
        use wgpu::CompositeAlphaMode as Mode;
        assert_eq!(
            pick_alpha_mode(&[Mode::Auto, Mode::PreMultiplied, Mode::PostMultiplied]),
            Mode::PreMultiplied
        );
    }

    #[test]
    fn alpha_mode_falls_back_to_postmultiplied() {
        use wgpu::CompositeAlphaMode as Mode;
        assert_eq!(
            pick_alpha_mode(&[Mode::Auto, Mode::PostMultiplied]),
            Mode::PostMultiplied
        );
    }

    #[test]
    fn alpha_mode_degrades_to_auto_without_transparency() {
        use wgpu::CompositeAlphaMode as Mode;
        assert_eq!(pick_alpha_mode(&[Mode::Opaque, Mode::Auto]), Mode::Auto);
        assert_eq!(pick_alpha_mode(&[]), Mode::Auto);
    }

    #[test]
    fn errors_describe_their_cause() {
        assert!(GpuError::UnsupportedSurface.to_string().contains("surface"));
        assert!(
            GpuError::NoAdapter("none".to_owned())
                .to_string()
                .contains("none")
        );
        assert!(
            GpuError::Device("lost".to_owned())
                .to_string()
                .contains("lost")
        );
        assert!(
            GpuError::Surface("format".to_owned())
                .to_string()
                .contains("format")
        );
    }
}
