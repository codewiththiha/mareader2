//! Film grain: one triangle over the window, multiplied or screened.

use std::sync::{Arc, OnceLock};

use eframe::egui_wgpu::{self, CallbackResources, ScreenDescriptor, wgpu};
use egui::Rect;
use mareader_appearance::FilmGrain;
use wgpu::util::DeviceExt as _;

use super::page_pass::NOISE;

/// The grain's two pipelines: they differ only in how they blend.
#[derive(Debug)]
pub struct GrainPass {
    multiply: wgpu::RenderPipeline,
    screen: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

/// Bytes of the shader's `Grain` uniform.
const UNIFORM_BYTES: u64 = 16;

impl GrainPass {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let linear = format!("const LINEAR: bool = {};\n", format.is_srgb());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("film grain"),
            source: wgpu::ShaderSource::Wgsl((linear + NOISE + include_str!("grain.wgsl")).into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("film grain"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("film grain"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        // The destination's alpha is kept; only its colour is grained.
        let keep = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = |entry: &str, src_factor, dst_factor| {
            let color = wgpu::BlendComponent {
                src_factor,
                dst_factor,
                operation: wgpu::BlendOperation::Add,
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState { color, alpha: keep }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        use wgpu::BlendFactor::{Dst, One, OneMinusSrc, Zero};
        Self {
            multiply: pipeline("fs_multiply", Dst, Zero),
            screen: pipeline("fs_screen", One, OneMinusSrc),
            layout,
        }
    }
}

/// This frame's grain, drawn over everything painted before it.
pub struct GrainCallback {
    pass: Arc<GrainPass>,
    grain: FilmGrain,
    offset: [f32; 2],
    group: OnceLock<wgpu::BindGroup>,
}

impl GrainCallback {
    /// Grains `screen`; `offset` is the crawl's, in points.
    pub fn shape(
        pass: Arc<GrainPass>,
        screen: Rect,
        grain: FilmGrain,
        offset: [f32; 2],
    ) -> egui::Shape {
        let callback = Self {
            pass,
            grain,
            offset,
            group: OnceLock::new(),
        };
        egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(screen, callback))
    }
}

impl egui_wgpu::CallbackTrait for GrainCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        _resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let [x, y] = self.offset;
        let floats = [x, y, self.grain.opacity, screen.pixels_per_point];
        let bytes: Vec<u8> = floats.iter().flat_map(|f| f.to_ne_bytes()).collect();
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("film grain"),
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("film grain"),
            layout: &self.pass.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let _ = self.group.set(group);
        Vec::new()
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        _resources: &CallbackResources,
    ) {
        let Some(group) = self.group.get() else {
            return;
        };
        let [w, h] = info.screen_size_px;
        pass.set_viewport(0.0, 0.0, w as f32, h as f32, 0.0, 1.0);
        let pipeline = if self.grain.screen {
            &self.pass.screen
        } else {
            &self.pass.multiply
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
}
