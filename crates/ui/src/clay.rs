use egui::{Color32, Rgba};
use manfred_scene::Outline;
use pfx_gpu::{Gpu, wgpu};
use wgpu::util::DeviceExt;

use crate::theme::{ACCENT, BG_DEEP};
use crate::wire::{Look, Matrix};

const VERTEX_BYTES: u64 = 16;
const PLACED_BYTES: usize = 80;
const VIEW_BYTES: u64 = 192;
const OUTLINE_WIDTH: f32 = 2.0;
const ALBEDO: f32 = 0.6;
const SHADE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shading {
    Shaded,
    Clay,
    Wireframe,
}

impl Shading {
    pub fn parse(text: &str) -> Result<Shading, String> {
        match text.trim() {
            "shaded" => Ok(Shading::Shaded),
            "clay" => Ok(Shading::Clay),
            "wireframe" | "wire" => Ok(Shading::Wireframe),
            other => Err(format!(
                "{other} is not a shading mode: shaded, clay or wireframe"
            )),
        }
    }

    pub fn of(look: Look) -> Shading {
        if look.on() {
            Shading::Wireframe
        } else if look.clay > 0.0 {
            Shading::Clay
        } else {
            Shading::Shaded
        }
    }

    pub fn next(self) -> Shading {
        match self {
            Shading::Shaded => Shading::Clay,
            Shading::Clay => Shading::Wireframe,
            Shading::Wireframe => Shading::Shaded,
        }
    }

    pub fn amounts(self) -> (f32, f32) {
        match self {
            Shading::Shaded => (0.0, 0.0),
            Shading::Clay => (1.0, 0.0),
            Shading::Wireframe => (0.0, 1.0),
        }
    }
}

pub fn fade(clay: f32, wire: f32, to: Shading, frames: usize) -> Vec<(f32, f32)> {
    let (end_clay, end_wire) = to.amounts();
    if frames == 0 {
        return vec![(end_clay, end_wire)];
    }
    let covered = wire >= 1.0;
    let covering = end_wire >= 1.0;
    (1..=frames)
        .map(|frame| {
            let along = frame as f32 / frames as f32;
            let wire = wire + (end_wire - wire) * along;
            let clay = if covered || (covering && frame == frames) {
                end_clay
            } else if covering {
                clay
            } else {
                clay + (end_clay - clay) * along
            };
            (clay, wire)
        })
        .collect()
}

pub struct Scene<'a> {
    pub key: u64,
    pub outlines: &'a [Outline<'a>],
    pub selected: &'a [u32],
}

pub struct Target<'a> {
    pub output: &'a wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    pub size: [u32; 2],
    pub camera: (Matrix, Matrix),
    pub scale: f32,
}

struct Pipes {
    format: wgpu::TextureFormat,
    model_layout: wgpu::BindGroupLayout,
    show_layout: wgpu::BindGroupLayout,
    model: wgpu::RenderPipeline,
    show: wgpu::RenderPipeline,
    view: wgpu::Buffer,
}

struct Planes {
    size: [u32; 2],
    shade: wgpu::TextureView,
    depth: wgpu::TextureView,
}

#[derive(Default)]
pub struct Clay {
    pipes: Option<Pipes>,
    planes: Option<Planes>,
    set: Option<(u64, usize)>,
    vertices: Option<wgpu::Buffer>,
    indices: Option<wgpu::Buffer>,
    count: u32,
    placed: Option<wgpu::Buffer>,
    written: Vec<u8>,
}

fn linear(colour: Color32) -> [f32; 4] {
    let rgba = Rgba::from(colour);
    [rgba.r(), rgba.g(), rgba.b(), 1.0]
}

fn push_floats(bytes: &mut Vec<u8>, values: impl IntoIterator<Item = f32>) {
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

fn push_matrix(bytes: &mut Vec<u8>, matrix: &Matrix) {
    push_floats(bytes, matrix.iter().flatten().copied());
}

fn buffer(
    device: &wgpu::Device,
    label: &str,
    mut bytes: Vec<u8>,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    bytes.resize(bytes.len().max(VERTEX_BYTES as usize), 0);
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: &bytes,
        usage: usage | wgpu::BufferUsages::COPY_DST,
    })
}

fn plane(
    device: &wgpu::Device,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    label: &str,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

impl Clay {
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        scene: &Scene<'_>,
        target: &Target<'_>,
        look: Look,
    ) -> Result<(), String> {
        if look.clay <= 0.0 {
            return Ok(());
        }
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("manfred clay"),
            });
        self.encode(gpu, &mut encoder, scene, target, look.clay, None)?;
        gpu.queue.submit([encoder.finish()]);
        Ok(())
    }

    pub fn encode(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Scene<'_>,
        target: &Target<'_>,
        amount: f32,
        stamps: Option<&wgpu::QuerySet>,
    ) -> Result<(), String> {
        let device = &gpu.device;
        if self
            .pipes
            .as_ref()
            .is_none_or(|pipes| pipes.format != target.format)
        {
            self.pipes = Some(pipes(device, target.format));
        }
        if self
            .planes
            .as_ref()
            .is_none_or(|planes| planes.size != target.size)
        {
            self.planes = Some(Planes {
                size: target.size,
                shade: plane(device, target.size, SHADE_FORMAT, "manfred clay shade"),
                depth: plane(device, target.size, DEPTH_FORMAT, "manfred clay depth"),
            });
        }
        let set = (scene.key, scene.outlines.len());
        if self.set != Some(set) {
            self.rebuild(device, scene);
            self.set = Some(set);
        }
        self.place(gpu, scene);
        let (Some(pipes), Some(planes)) = (self.pipes.as_ref(), self.planes.as_ref()) else {
            return Err("clay has no pipelines".into());
        };
        let (Some(vertices), Some(indices), Some(placed)) =
            (&self.vertices, &self.indices, &self.placed)
        else {
            return Err("clay has no buffers".into());
        };
        let (view, projection) = target.camera;
        let mut uniform = Vec::with_capacity(VIEW_BYTES as usize);
        push_matrix(&mut uniform, &view);
        push_matrix(&mut uniform, &projection);
        push_floats(&mut uniform, linear(BG_DEEP));
        push_floats(&mut uniform, linear(ACCENT));
        push_floats(
            &mut uniform,
            [
                amount.clamp(0.0, 1.0),
                OUTLINE_WIDTH * target.scale.max(0.5),
                ALBEDO,
                0.0,
            ],
        );
        gpu.queue.write_buffer(&pipes.view, 0, &uniform);
        let model_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("manfred clay model"),
            layout: &pipes.model_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: pipes.view.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: placed.as_entire_binding(),
                },
            ],
        });
        let show_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("manfred clay show"),
            layout: &pipes.show_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: pipes.view.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&planes.shade),
                },
            ],
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("manfred clay model"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &planes.shade,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &planes.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: stamps.map(|query_set| wgpu::RenderPassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: None,
                }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if self.count > 0 {
                pass.set_pipeline(&pipes.model);
                pass.set_bind_group(0, &model_group, &[]);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..self.count, 0, 0..1);
            }
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("manfred clay show"),
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
            timestamp_writes: stamps.map(|query_set| wgpu::RenderPassTimestampWrites {
                query_set,
                beginning_of_pass_write_index: None,
                end_of_pass_write_index: Some(1),
            }),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipes.show);
        pass.set_bind_group(0, &show_group, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    }

    fn rebuild(&mut self, device: &wgpu::Device, scene: &Scene<'_>) {
        let most = device.limits().max_buffer_size / VERTEX_BYTES;
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut base = 0u64;
        for (owner, outline) in scene.outlines.iter().enumerate() {
            let len = outline.positions.len() as u64;
            if base + len > most || base + len > u64::from(u32::MAX) {
                break;
            }
            if outline.indices.iter().any(|&index| u64::from(index) >= len) {
                continue;
            }
            for position in outline.positions {
                push_floats(&mut vertices, *position);
                vertices.extend_from_slice(&(owner as u32).to_le_bytes());
            }
            let offset = base as u32;
            for face in outline.indices.chunks_exact(3) {
                for &index in face {
                    indices.extend_from_slice(&(index + offset).to_le_bytes());
                }
            }
            base += len;
        }
        self.count = (indices.len() / 4) as u32;
        self.vertices = Some(buffer(
            device,
            "manfred clay vertices",
            vertices,
            wgpu::BufferUsages::VERTEX,
        ));
        self.indices = Some(buffer(
            device,
            "manfred clay indices",
            indices,
            wgpu::BufferUsages::INDEX,
        ));
        self.written.clear();
        self.placed = None;
    }

    fn place(&mut self, gpu: &Gpu, scene: &Scene<'_>) {
        let mut bytes = Vec::with_capacity(scene.outlines.len() * PLACED_BYTES);
        for outline in scene.outlines {
            push_matrix(&mut bytes, &outline.model);
            let mark = if scene.selected.contains(&outline.id) {
                1.0
            } else {
                0.5
            };
            push_floats(&mut bytes, [mark, 0.0, 0.0, 0.0]);
        }
        if bytes == self.written && self.placed.is_some() {
            return;
        }
        match &self.placed {
            Some(buffer) if buffer.size() >= bytes.len() as u64 && !bytes.is_empty() => {
                gpu.queue.write_buffer(buffer, 0, &bytes);
            }
            _ => {
                self.placed = Some(buffer(
                    &gpu.device,
                    "manfred clay placed",
                    bytes.clone(),
                    wgpu::BufferUsages::STORAGE,
                ));
            }
        }
        self.written = bytes;
    }
}

fn pipes(device: &wgpu::Device, format: wgpu::TextureFormat) -> Pipes {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("manfred clay"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let uniform = wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(VIEW_BYTES),
        },
        count: None,
    };
    let model_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("manfred clay model"),
        entries: &[
            uniform,
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let show_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("manfred clay show"),
        entries: &[
            uniform,
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    });
    let layout = |group: &wgpu::BindGroupLayout| {
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("manfred clay"),
            bind_group_layouts: &[Some(group)],
            immediate_size: 0,
        })
    };
    let model = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("manfred clay model"),
        layout: Some(&layout(&model_layout)),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("solid"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: VERTEX_BYTES,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Uint32],
            })],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("matte"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: SHADE_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let show = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("manfred clay show"),
        layout: Some(&layout(&show_layout)),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("cover"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("composite"),
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
    Pipes {
        format,
        model_layout,
        show_layout,
        model,
        show,
        view: device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("manfred clay view"),
            size: VIEW_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
    }
}

const SHADER: &str = r#"
struct View {
    view: mat4x4f,
    projection: mat4x4f,
    ground: vec4f,
    accent: vec4f,
    amount: f32,
    width: f32,
    albedo: f32,
    spare: f32,
}

struct Placed {
    model: mat4x4f,
    mark: vec4f,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> placed: array<Placed>;

struct Solid {
    @builtin(position) clip: vec4f,
    @location(0) seen: vec3f,
    @location(1) @interpolate(flat) mark: f32,
}

@vertex
fn solid(@location(0) position: vec3f, @location(1) owner: u32) -> Solid {
    let item = placed[owner];
    let seen = view.view * item.model * vec4f(position, 1.0);
    var out: Solid;
    out.clip = view.projection * seen;
    out.seen = seen.xyz;
    out.mark = item.mark.x;
    return out;
}

@fragment
fn matte(solid: Solid) -> @location(0) vec4f {
    var normal = cross(dpdx(solid.seen), dpdy(solid.seen));
    let length = sqrt(dot(normal, normal));
    if length > 1e-20 {
        normal = normal / length;
    } else {
        normal = vec3f(0.0, 0.0, 1.0);
    }
    if dot(normal, solid.seen) > 0.0 {
        normal = -normal;
    }
    let turn = mat3x3f(view.view[0].xyz, view.view[1].xyz, view.view[2].xyz);
    let world = normal * turn;
    let key = normalize(vec3f(-0.35, 0.55, 0.75));
    let lambert = max(dot(normal, key), 0.0);
    let sky = 0.5 + 0.5 * world.y;
    let ambient = mix(0.16, 0.42, sky);
    let shade = view.albedo * (0.6 * lambert + ambient);
    return vec4f(vec3f(shade), solid.mark);
}

@vertex
fn cover(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let x = f32(index & 1u) * 4.0 - 1.0;
    let y = f32(index >> 1u) * 4.0 - 1.0;
    return vec4f(x, y, 0.0, 1.0);
}

@group(0) @binding(2) var shade: texture_2d<f32>;

fn mark_at(pixel: vec2i, limit: vec2i) -> f32 {
    return textureLoad(shade, clamp(pixel, vec2i(0, 0), limit), 0).a;
}

@fragment
fn composite(@builtin(position) at: vec4f) -> @location(0) vec4f {
    let limit = vec2i(textureDimensions(shade)) - vec2i(1, 1);
    let pixel = vec2i(at.xy);
    let here = textureLoad(shade, clamp(pixel, vec2i(0, 0), limit), 0);
    var colour = view.ground.rgb;
    if here.a > 0.25 {
        colour = here.rgb / (vec3f(1.0) + here.rgb * 0.15);
    }
    if here.a < 0.75 {
        let reach = view.width;
        var near = 0.0;
        for (var turn = 0u; turn < 12u; turn = turn + 1u) {
            let angle = f32(turn) * 0.5235988;
            let step = vec2f(cos(angle), sin(angle));
            for (var ring = 1u; ring <= 2u; ring = ring + 1u) {
                let along = reach * f32(ring) * 0.5;
                let probe = pixel + vec2i(round(step * along));
                if mark_at(probe, limit) > 0.75 {
                    near = max(near, select(0.6, 1.0, ring == 1u));
                }
            }
        }
        colour = mix(colour, view.accent.rgb, near);
    }
    return vec4f(colour, view.amount);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shading_cycles_and_fades_keep_the_clay_under_a_covering_wireframe() {
        assert_eq!(Shading::parse("clay"), Ok(Shading::Clay));
        assert_eq!(Shading::parse("wireframe"), Ok(Shading::Wireframe));
        assert!(Shading::parse("solid").is_err());
        assert_eq!(Shading::Shaded.next(), Shading::Clay);
        assert_eq!(Shading::Clay.next(), Shading::Wireframe);
        assert_eq!(Shading::Wireframe.next(), Shading::Shaded);
        let clay = Look {
            clay: 1.0,
            ..Look::default()
        };
        assert_eq!(Shading::of(clay), Shading::Clay);
        assert_eq!(
            Shading::of(Look {
                amount: 1.0,
                ..clay
            }),
            Shading::Wireframe
        );
        assert_eq!(
            fade(0.0, 0.0, Shading::Clay, 4),
            vec![(0.25, 0.0), (0.5, 0.0), (0.75, 0.0), (1.0, 0.0)]
        );
        assert_eq!(
            fade(1.0, 0.0, Shading::Wireframe, 2),
            vec![(1.0, 0.5), (0.0, 1.0)]
        );
        assert_eq!(
            fade(0.0, 1.0, Shading::Clay, 2),
            vec![(1.0, 0.5), (1.0, 0.0)]
        );
        assert_eq!(fade(1.0, 0.0, Shading::Shaded, 0), vec![(0.0, 0.0)]);
    }

    #[test]
    #[ignore = "GPU: manfred build showcase/chase.toml --out tmp/clay/chase, then pgpu run --class bench --as manfred -- cargo test -p manfred-ui clay -- --ignored --nocapture"]
    fn the_chase_street_draws_in_clay_in_about_a_millisecond_and_a_half() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp/clay/chase/street.scene.toml");
        let scene = pfx_load::scene::Scene::open(&path).unwrap_or_else(|error| {
            panic!("build showcase/chase.toml into tmp/clay/chase first: {error}")
        });
        let draws = scene.draws();
        let outlines: Vec<Outline<'_>> = draws
            .items
            .iter()
            .filter(|draw| draw.shadow.drawn())
            .map(|draw| Outline {
                key: 0,
                positions: &draw.geometry.positions,
                indices: &draw.geometry.indices,
                model: draw.model,
                id: draw.id,
                name: "",
            })
            .collect();
        let triangles: usize = outlines.iter().map(|o| o.indices.len() / 3).sum();
        let selected: Vec<u32> = outlines
            .iter()
            .map(|o| o.id)
            .nth(outlines.len() / 2)
            .into_iter()
            .collect();
        let size = [1586u32, 992];
        let eye = manfred_scene::Eye::from_camera(&crate::watched::shot(&scene), 1586.0 / 992.0);
        let gpu = manfred_scene::block_on(Gpu::headless()).unwrap();
        assert!(
            gpu.device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY),
            "the bench needs timestamp queries"
        );
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let output = plane(&gpu.device, size, format, "bench output");
        let target = Target {
            output: &output,
            format,
            size,
            camera: (eye.view(), eye.projection(size, 0.05, 1000.0)),
            scale: 1.0,
        };
        let shown = Scene {
            key: 1,
            outlines: &outlines,
            selected: &selected,
        };
        let set = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: None,
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let resolved = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let period = f64::from(gpu.queue.get_timestamp_period());
        let mut clay = Clay::default();
        let mut times = Vec::new();
        for frame in 0..14 {
            let since = std::time::Instant::now();
            let mut encoder = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            clay.encode(&gpu, &mut encoder, &shown, &target, 1.0, Some(&set))
                .unwrap();
            encoder.resolve_query_set(&set, 0..2, &resolved, 0);
            encoder.copy_buffer_to_buffer(&resolved, 0, &read, 0, 16);
            gpu.queue.submit([encoder.finish()]);
            read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            gpu.device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            let ticks: Vec<u64> = read
                .slice(..)
                .get_mapped_range()
                .unwrap()
                .chunks_exact(8)
                .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
                .collect();
            read.unmap();
            if frame >= 4 {
                times.push(ticks[1].saturating_sub(ticks[0]) as f64 * period / 1e6);
            }
            crate::gpu_turn(since);
        }
        times.sort_by(f64::total_cmp);
        let (best, median) = (times[0], times[times.len() / 2]);
        println!(
            "clay of the chase street: {} draws, {triangles} triangles, 1586x992: best {best:.3} ms, median {median:.3} ms",
            outlines.len(),
        );
        assert!(median < 3.0, "clay took {median:.3} ms");
    }
}
