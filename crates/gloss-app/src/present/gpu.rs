//! wgpu 设备与渲染目标：设备全进程一份，每个窗口一份 surface。

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use egui_wgpu::wgpu;
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use gloss_core::log::{debug, error, info, thread, warn};
use winit::dpi::PhysicalSize;
use winit::window::Window;

const MAX_TEXTURE_DIMENSION: u32 = 8192;

pub const FONT_ATLAS_MAX_SIDE: usize = 2048;

const DESIRED_MAX_FRAME_LATENCY: u32 = 1;

const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

pub struct GpuContext {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl GpuContext {
    pub fn new() -> Result<Self, GpuError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

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

pub struct GpuSurface {
    context: Arc<GpuContext>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    needs_configure: bool,
}

impl GpuSurface {
    pub fn new(context: &Arc<GpuContext>, window: Arc<Window>) -> Result<Self, GpuError> {
        let surface = context.instance().create_surface(window.clone())?;
        let size = window.inner_size();
        let caps = surface.get_capabilities(context.adapter());

        let format = egui_wgpu::preferred_framebuffer_format(&caps.formats)
            .map_err(|err| GpuError::Surface(err.to_string()))?;
        let mut config = surface
            .get_default_config(context.adapter(), size.width.max(1), size.height.max(1))
            .ok_or(GpuError::UnsupportedSurface)?;
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        config.format = format;
        config.alpha_mode = pick_alpha_mode(&caps.alpha_modes);
        config.view_formats = vec![format];
        config.desired_maximum_frame_latency = DESIRED_MAX_FRAME_LATENCY;

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

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.configure();
    }

    pub fn adapter_name(&self) -> String {
        self.context.adapter().get_info().name
    }

    pub fn render(
        &mut self,
        mut textures: egui::TexturesDelta,
        paint_jobs: &[egui::ClippedPrimitive],
        pixels_per_point: f32,
    ) {
        if self.needs_configure {
            self.configure();
        }

        self.upload_texture_deltas(&textures);

        let Some(frame) = self.take_frame() else {
            self.discard_textures(textures);
            return;
        };

        self.draw_and_present(frame, &mut textures, paint_jobs, pixels_per_point);

        if self.needs_configure {
            debug!(thread = thread::UI, "surface was suboptimal, reconfiguring");
            self.configure();
        }
    }

    fn upload_texture_deltas(&mut self, textures: &egui::TexturesDelta) {
        let device = self.context.device();
        let queue = self.context.queue();
        for (id, deltas) in &textures.set {
            for delta in deltas {
                self.renderer.update_texture(device, queue, *id, delta);
            }
        }
    }

    fn take_frame(&mut self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => Some(frame),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                self.needs_configure = true;
                Some(frame)
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.configure();
                None
            }
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => None,
            other => {
                error!(thread = thread::UI, status = ?other, "surface is unusable, skipping frame");
                None
            }
        }
    }

    fn draw_and_present(
        &mut self,
        frame: wgpu::SurfaceTexture,
        textures: &mut egui::TexturesDelta,
        paint_jobs: &[egui::ClippedPrimitive],
        pixels_per_point: f32,
    ) {
        let screen = ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
        };
        let device = self.context.device();
        let queue = self.context.queue();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gloss frame"),
        });
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
            self.renderer
                .render(&mut pass.forget_lifetime(), paint_jobs, &screen);
        }

        let encoded = encoder.finish();
        queue.submit(user_buffers.into_iter().chain([encoded]));
        for id in &textures.free {
            self.renderer.free_texture(id);
        }
        textures.clear();
        queue.present(frame);
    }

    fn discard_textures(&mut self, mut textures: egui::TexturesDelta) {
        for id in &textures.free {
            self.renderer.free_texture(id);
        }
        textures.clear();
    }

    fn configure(&mut self) {
        self.surface.configure(self.context.device(), &self.config);
        self.needs_configure = false;
    }
}

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

#[derive(Debug)]
pub enum GpuError {
    NoAdapter(String),
    Device(String),
    Surface(String),
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
