//! The page pass: every page, and flat fills, as tinted quads.

use std::sync::{Arc, OnceLock};

use eframe::egui_wgpu::{self, CallbackResources, ScreenDescriptor, wgpu};
use egui::{Color32, Rect};
use wgpu::util::DeviceExt as _;

/// A page raster on the GPU, ready to bind.
#[derive(Debug)]
pub struct GpuPage {
    pub(super) bind_group: wgpu::BindGroup,
    _texture: wgpu::Texture,
}

/// One quad: a page, or a flat fill when `texture` is none.
#[derive(Clone, Debug)]
pub struct Quad {
    pub rect: Rect,
    /// What clips it on screen: its pane's scroll area.
    pub clip: Rect,
    pub tint: Color32,
    pub texture: Option<Arc<GpuPage>>,
}

impl Quad {
    pub fn fill(rect: Rect, colour: Color32) -> Self {
        Self {
            rect,
            clip: rect,
            tint: colour,
            texture: None,
        }
    }
}

/// Pipelines and shared bindings; one per renderer.
#[derive(Debug)]
pub struct PagePass {
    /// Draws to the screen, and to the glass strip at a fraction of it.
    pub(super) screen: wgpu::RenderPipeline,
    pub(super) strip: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// A white texel: flat fills are white tinted.
    white: wgpu::BindGroup,
    pub format: wgpu::TextureFormat,
}

/// Raster bytes per pixel; RGBA8 throughout.
const TEXEL: u32 = 4;
/// Per instance: a clip-space rect, then a tint.
const INSTANCE: u64 = 8 * 4;

impl PagePass {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let linear = format!("const LINEAR: bool = {};\n", format.is_srgb());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("page pass"),
            source: wgpu::ShaderSource::Wgsl((linear + include_str!("page.wgsl")).into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("page"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("page pass"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: INSTANCE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4],
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("page"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = upload(device, queue, (&layout, &sampler), [1, 1], &[255; 4]);
        Self {
            screen: pipeline("fs"),
            strip: pipeline("fs_strip"),
            layout,
            sampler,
            white: white.bind_group,
            format,
        }
    }

    /// Uploads an RGBA8 raster; none if the bytes don't match the size.
    pub fn texture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Option<GpuPage> {
        let row = width.checked_mul(TEXEL)?;
        if width == 0 || height == 0 || pixels.len() != (row as usize) * (height as usize) {
            return None;
        }
        let bind = (&self.layout, &self.sampler);
        Some(upload(device, queue, bind, [width, height], pixels))
    }

    /// Draws `quads`; `scissor` maps each clip into the target.
    pub(super) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        instances: &wgpu::Buffer,
        quads: &[Quad],
        scissor: impl Fn(Rect) -> Option<[u32; 4]>,
    ) {
        pass.set_vertex_buffer(0, instances.slice(..));
        for (i, quad) in (0u32..).zip(quads) {
            let Some([x, y, w, h]) = scissor(quad.clip) else {
                continue;
            };
            pass.set_scissor_rect(x, y, w, h);
            let group = quad.texture.as_ref().map_or(&self.white, |t| &t.bind_group);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..4, i..i + 1);
        }
    }
}

/// Uploads checked RGBA8 bytes of `[width, height]`.
fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    (layout, sampler): (&wgpu::BindGroupLayout, &wgpu::Sampler),
    [width, height]: [u32; 2],
    pixels: &[u8],
) -> GpuPage {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("page"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * TEXEL),
            rows_per_image: Some(height),
        },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("page"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    GpuPage {
        bind_group,
        _texture: texture,
    }
}

/// Instance bytes for `quads` drawn into a target spanning `area`.
pub(super) fn instances(device: &wgpu::Device, quads: &[Quad], area: Rect) -> wgpu::Buffer {
    let size = area.size().max(egui::vec2(1.0, 1.0));
    let x = |v: f32| (v - area.left()) / size.x * 2.0 - 1.0;
    let y = |v: f32| 1.0 - (v - area.top()) / size.y * 2.0;
    let mut bytes = Vec::with_capacity(quads.len() * INSTANCE as usize);
    for q in quads {
        let tint = q.tint.to_array().map(|c| f32::from(c) / 255.0);
        let r = q.rect;
        let floats = [x(r.left()), y(r.top()), x(r.right()), y(r.bottom())];
        for f in floats.into_iter().chain(tint) {
            bytes.extend_from_slice(&f.to_ne_bytes());
        }
    }
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("page instances"),
        contents: &bytes,
        usage: wgpu::BufferUsages::VERTEX,
    })
}

/// Scissor in a target of `px` pixels spanning `area`; none if empty.
pub(super) fn scissor(clip: Rect, area: Rect, px: [u32; 2]) -> Option<[u32; 4]> {
    let scale = egui::vec2(px[0] as f32, px[1] as f32) / area.size().max(egui::vec2(1.0, 1.0));
    let r = clip.intersect(area);
    if !r.is_positive() {
        return None;
    }
    let min = ((r.min - area.min) * scale).floor();
    let max = ((r.max - area.min) * scale)
        .ceil()
        .min(egui::vec2(px[0] as f32, px[1] as f32));
    let (x, y) = (min.x.max(0.0) as u32, min.y.max(0.0) as u32);
    let (w, h) = (
        (max.x as u32).saturating_sub(x),
        (max.y as u32).saturating_sub(y),
    );
    (w > 0 && h > 0).then_some([x, y, w, h])
}

/// One pane's pages, drawn into egui's pass.
pub struct PagesCallback {
    pass: Arc<PagePass>,
    quads: Vec<Quad>,
    instances: OnceLock<(wgpu::Buffer, Rect, [u32; 2])>,
}

impl PagesCallback {
    pub fn shape(pass: Arc<PagePass>, clip: Rect, quads: Vec<Quad>) -> egui::Shape {
        let callback = Self {
            pass,
            quads,
            instances: OnceLock::new(),
        };
        egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(clip, callback))
    }
}

impl egui_wgpu::CallbackTrait for PagesCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        _resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let px = screen.size_in_pixels;
        let ppp = screen.pixels_per_point;
        let area = Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(px[0] as f32, px[1] as f32) / ppp,
        );
        let buffer = instances(device, &self.quads, area);
        let _ = self.instances.set((buffer, area, px));
        Vec::new()
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        _resources: &CallbackResources,
    ) {
        let Some((buffer, area, px)) = self.instances.get() else {
            return;
        };
        // egui's clamped viewport squashes pages hanging off screen.
        pass.set_viewport(0.0, 0.0, px[0] as f32, px[1] as f32, 0.0, 1.0);
        pass.set_pipeline(&self.pass.screen);
        let clip = info.clip_rect;
        self.pass.draw(pass, buffer, &self.quads, |q| {
            scissor(q.intersect(clip), *area, *px)
        });
    }
}
