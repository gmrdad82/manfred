use std::collections::HashMap;
use std::path::{Path, PathBuf};

use manfred_recipe::Reference;
use pfx_gpu::{Gpu, wgpu};
use wgpu::util::DeviceExt;

use crate::wire::Matrix;

const QUAD_BYTES: u64 = 208;

#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub path: PathBuf,
    pub reference: Reference,
}

struct Image {
    view: wgpu::TextureView,
    size: [u32; 2],
}

struct Pipe {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

#[derive(Default)]
pub struct Backdrop {
    pipe: Option<Pipe>,
    images: HashMap<PathBuf, Option<Image>>,
    epoch: u64,
}

pub struct Drawn {
    groups: Vec<wgpu::BindGroup>,
    pipeline: wgpu::RenderPipeline,
}

impl Drawn {
    pub fn record(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        for group in &self.groups {
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..6, 0..1);
        }
    }
}

pub struct Target<'a> {
    pub output: &'a wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    pub depth: &'a wgpu::TextureView,
    pub size: [u32; 2],
    pub camera: (Matrix, Matrix),
}

fn load(gpu: &Gpu, path: &Path) -> Option<Image> {
    let (size, pixels) = manfred_scene::read_png(path).ok()?;
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("manfred reference"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &pixels,
    );
    Some(Image {
        view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
        size,
    })
}

fn push_floats(bytes: &mut Vec<u8>, values: impl IntoIterator<Item = f32>) {
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

impl Backdrop {
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        target: &Target<'_>,
        epoch: u64,
        shown: &[Shown],
        through: bool,
    ) -> Option<Drawn> {
        if shown.is_empty() {
            return None;
        }
        if epoch != self.epoch {
            self.images.clear();
            self.epoch = epoch;
        }
        let device = &gpu.device;
        if self
            .pipe
            .as_ref()
            .is_none_or(|pipe| pipe.format != target.format)
        {
            self.pipe = Some(pipe(device, target.format));
        }
        for item in shown {
            if !self.images.contains_key(&item.path) {
                let image = load(gpu, &item.path);
                self.images.insert(item.path.clone(), image);
            }
        }
        let pipe = self.pipe.as_ref()?;
        let (view, projection) = target.camera;
        let groups: Vec<wgpu::BindGroup> = shown
            .iter()
            .filter_map(|item| {
                let image = self.images.get(&item.path)?.as_ref()?;
                let mut bytes = Vec::with_capacity(QUAD_BYTES as usize);
                push_floats(&mut bytes, view.iter().flatten().copied());
                push_floats(&mut bytes, projection.iter().flatten().copied());
                for corner in item.reference.corners(image.size) {
                    push_floats(&mut bytes, corner.map(|c| c as f32));
                    push_floats(&mut bytes, [1.0]);
                }
                push_floats(
                    &mut bytes,
                    [
                        target.size[0] as f32,
                        target.size[1] as f32,
                        item.reference.opacity.clamp(0.0, 1.0) as f32,
                        if through { 1.0 } else { 0.0 },
                    ],
                );
                let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("manfred reference quad"),
                    contents: &bytes,
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("manfred reference"),
                    layout: &pipe.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: uniform.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&image.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&pipe.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(target.depth),
                        },
                    ],
                }))
            })
            .collect();
        (!groups.is_empty()).then(|| Drawn {
            groups,
            pipeline: pipe.pipeline.clone(),
        })
    }

    pub fn draw(gpu: &Gpu, target: &Target<'_>, drawn: &Drawn) {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("manfred reference"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("manfred reference"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.output,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            drawn.record(&mut pass);
        }
        gpu.queue.submit([encoder.finish()]);
    }
}

fn pipe(device: &wgpu::Device, format: wgpu::TextureFormat) -> Pipe {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("manfred reference"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("manfred reference"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(QUAD_BYTES),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("manfred reference"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("manfred reference"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("corner"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("paint"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("manfred reference"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..wgpu::SamplerDescriptor::default()
    });
    Pipe {
        format,
        layout,
        pipeline,
        sampler,
    }
}

const SHADER: &str = r#"
struct Quad {
    view: mat4x4f,
    projection: mat4x4f,
    corners: array<vec4f, 4>,
    size: vec2f,
    opacity: f32,
    through: f32,
}

@group(0) @binding(0) var<uniform> quad: Quad;
@group(0) @binding(1) var image: texture_2d<f32>;
@group(0) @binding(2) var soft: sampler;
@group(0) @binding(3) var depth: texture_depth_2d;

struct Corner {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn corner(@builtin(vertex_index) index: u32) -> Corner {
    var order = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    var uvs = array<vec2f, 4>(vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0), vec2f(1.0, 1.0));
    let k = order[index];
    let clip = quad.projection * quad.view * vec4f(quad.corners[k].xyz, 1.0);
    var out: Corner;
    out.clip = vec4f(clip.xy, 0.5 * clip.w, clip.w);
    out.uv = uvs[k];
    return out;
}

@fragment
fn paint(at: Corner) -> @location(0) vec4f {
    let colour = textureSampleLevel(image, soft, at.uv, 0.0);
    let limit = vec2i(quad.size) - vec2i(1, 1);
    let z = textureLoad(depth, clamp(vec2i(at.clip.xy), vec2i(0, 0), limit), 0);
    if z < 1.0 && quad.through < 0.5 {
        discard;
    }
    return vec4f(colour.rgb, colour.a * quad.opacity);
}
"#;
