use std::collections::HashMap;
use std::hash::{BuildHasherDefault, DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use egui::{Color32, Rgba};
use manfred_motion::Route;
use manfred_recipe::RouteSpec;
use manfred_scene::Outline;
use pfx_gpu::{Gpu, wgpu};
use wgpu::util::DeviceExt;

use crate::theme::{ACCENT, BG_DEEP, TEXT_PRIMARY};

pub const CREASE_DEGREES: f32 = 1.0;
const PRECISION: f32 = 1e4;
const EDGE_BYTES: u64 = 32;
const PLACED_BYTES: usize = 96;
const VIEW_BYTES: u64 = 192;
const LINE_WIDTH: f32 = 1.0;
const SELECTED_WIDTH: f32 = 1.6;
const ROUTE_WIDTH: f32 = 2.0;
const LINE_ALPHA: f32 = 0.8;
const HIDDEN: f32 = 0.14;
const XRAY: f32 = 0.45;
const ROUTE_SAMPLES: f64 = 512.0;
const ROUTE_LEAST_STEP: f64 = 0.05;
const IDENTITY: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

pub type Matrix = [[f32; 4]; 4];
pub type Segment = [[f32; 3]; 2];

#[derive(Default)]
pub struct Fx(u64);

impl Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.mix(u64::from_le_bytes(word));
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.mix(value);
    }

    fn write_i64(&mut self, value: i64) {
        self.mix(value as u64);
    }

    fn write_usize(&mut self, value: usize) {
        self.mix(value as u64);
    }
}

impl Fx {
    fn mix(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

type Quick<K, V> = HashMap<K, V, BuildHasherDefault<Fx>>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub amount: f32,
    pub xray: bool,
    pub clay: f32,
}

impl Default for Look {
    fn default() -> Look {
        Look {
            amount: 0.0,
            xray: false,
            clay: 0.0,
        }
    }
}

impl Look {
    pub fn on(self) -> bool {
        self.amount > 0.0
    }
}

pub fn amount(text: &str) -> Result<f32, String> {
    match text.trim() {
        "on" => Ok(1.0),
        "off" => Ok(0.0),
        number => match number.parse::<f32>() {
            Ok(value) if (0.0..=1.0).contains(&value) => Ok(value),
            _ => Err(format!(
                "{number} is not a wireframe amount: on, off or a number from 0 to 1"
            )),
        },
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let u = sub(c, b);
    let v = sub(a, b);
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    n.map(|value| value / length)
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn edges(positions: &[[f32; 3]], indices: &[u32], crease_degrees: f32) -> Vec<Segment> {
    let crease = crease_degrees.to_radians().cos();
    let key = |p: [f32; 3]| p.map(|value| (value * PRECISION).round() as i64);
    let mut seen: Quick<([i64; 3], [i64; 3]), usize> = Quick::default();
    seen.reserve(indices.len());
    let mut open: Vec<Option<(Segment, [f32; 3])>> = Vec::with_capacity(indices.len() / 2);
    let mut out = Vec::new();
    for face in indices.chunks_exact(3) {
        let (Some(&a), Some(&b), Some(&c)) = (
            positions.get(face[0] as usize),
            positions.get(face[1] as usize),
            positions.get(face[2] as usize),
        ) else {
            continue;
        };
        let corners = [a, b, c];
        let hashes = [key(corners[0]), key(corners[1]), key(corners[2])];
        if hashes[0] == hashes[1] || hashes[1] == hashes[2] || hashes[2] == hashes[0] {
            continue;
        }
        let facing = normal(corners[0], corners[1], corners[2]);
        for j in 0..3 {
            let next = (j + 1) % 3;
            let (from, to) = (hashes[j], hashes[next]);
            match seen.get(&(to, from)).copied() {
                Some(slot) if open[slot].is_some() => {
                    if let Some((line, other)) = open[slot].take()
                        && dot(facing, other) <= crease
                    {
                        out.push(line);
                    }
                }
                _ => {
                    if let std::collections::hash_map::Entry::Vacant(entry) = seen.entry((from, to))
                    {
                        entry.insert(open.len());
                        open.push(Some(([corners[j], corners[next]], facing)));
                    }
                }
            }
        }
    }
    out.extend(open.into_iter().flatten().map(|(line, _)| line));
    out
}

pub fn route_lines(routes: &[RouteSpec]) -> Vec<Vec<[f32; 3]>> {
    routes
        .iter()
        .filter_map(|spec| {
            let route = Route::new(&spec.points, spec.closed).ok()?;
            let step = (route.length() / ROUTE_SAMPLES).max(ROUTE_LEAST_STEP);
            Some(
                route
                    .sample(step)
                    .into_iter()
                    .map(|(_, frame)| frame.position.as_vec3().to_array())
                    .collect(),
            )
        })
        .collect()
}

fn routes_key(routes: &[RouteSpec]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for spec in routes {
        spec.closed.hash(&mut hasher);
        for point in &spec.points {
            for value in point {
                value.to_bits().hash(&mut hasher);
            }
        }
        hasher.write_u8(0xff);
    }
    hasher.finish()
}

pub fn fade(from: f32, to: f32, frames: usize) -> Vec<f32> {
    if frames == 0 {
        return vec![to];
    }
    (1..=frames)
        .map(|frame| from + (to - from) * frame as f32 / frames as f32)
        .collect()
}

pub struct Scene<'a> {
    pub key: u64,
    pub outlines: &'a [Outline<'a>],
    pub routes: &'a [RouteSpec],
    pub selected: &'a [u32],
    pub pieces: &'a [String],
}

pub struct Target<'a> {
    pub output: &'a wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    pub depth: &'a wgpu::TextureView,
    pub size: [u32; 2],
    pub camera: (Matrix, Matrix),
    pub scale: f32,
    pub region: Option<[f32; 4]>,
    pub under: Option<&'a crate::backdrop::Drawn>,
}

struct Pipes {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    lines: wgpu::RenderPipeline,
    ground: wgpu::RenderPipeline,
    view: wgpu::Buffer,
}

#[derive(Default)]
pub struct Wire {
    pipes: Option<Pipes>,
    cache: Quick<u64, Arc<[Segment]>>,
    set: Option<(u64, u64, usize)>,
    edges: Option<wgpu::Buffer>,
    count: u32,
    routes: usize,
    placed: Option<wgpu::Buffer>,
    written: Vec<u8>,
}

fn linear(colour: Color32, alpha: f32) -> [f32; 4] {
    let rgba = Rgba::from(colour);
    [rgba.r(), rgba.g(), rgba.b(), alpha]
}

fn push_floats(bytes: &mut Vec<u8>, values: impl IntoIterator<Item = f32>) {
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

fn push_matrix(bytes: &mut Vec<u8>, matrix: &Matrix) {
    push_floats(bytes, matrix.iter().flatten().copied());
}

fn depth_terms(projection: &Matrix) -> [f32; 4] {
    [
        projection[2][2],
        projection[3][2],
        projection[2][3],
        projection[3][3],
    ]
}

pub fn region_pixels(region: Option<[f32; 4]>, size: [u32; 2]) -> [f32; 4] {
    match region {
        Some([x0, y0, x1, y1]) => {
            let [w, h] = size.map(|side| side as f32);
            [x0 * w, y0 * h, x1 * w, y1 * h]
        }
        None => [0.0; 4],
    }
}

fn storage(device: &wgpu::Device, label: &str, bytes: &[u8]) -> wgpu::Buffer {
    let mut contents = bytes.to_vec();
    contents.resize(contents.len().max(EDGE_BYTES as usize), 0);
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: &contents,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    })
}

impl Wire {
    #[cfg(test)]
    fn edges(&self) -> u32 {
        self.count
    }

    pub fn draw(
        &mut self,
        gpu: &Gpu,
        scene: &Scene<'_>,
        target: &Target<'_>,
        look: Look,
    ) -> Result<(), String> {
        if !look.on() {
            return Ok(());
        }
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("manfred wireframe"),
            });
        self.encode(gpu, &mut encoder, scene, target, look, None)?;
        gpu.queue.submit([encoder.finish()]);
        Ok(())
    }

    pub fn encode(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Scene<'_>,
        target: &Target<'_>,
        look: Look,
        stamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> Result<(), String> {
        let device = &gpu.device;
        if self
            .pipes
            .as_ref()
            .is_none_or(|pipes| pipes.format != target.format)
        {
            self.pipes = Some(pipes(device, target.format));
        }
        let routes = routes_key(scene.routes);
        let set = (scene.key, routes, scene.outlines.len());
        if self.set != Some(set) {
            self.rebuild(device, scene);
            self.set = Some(set);
        }
        self.place(gpu, scene, target.scale);
        let Some(pipes) = self.pipes.as_ref() else {
            return Err("the wireframe has no pipelines".into());
        };
        let (view, projection) = target.camera;
        let mut uniform = Vec::with_capacity(VIEW_BYTES as usize);
        push_matrix(&mut uniform, &view);
        push_matrix(&mut uniform, &projection);
        push_floats(&mut uniform, linear(BG_DEEP, 1.0));
        push_floats(
            &mut uniform,
            [
                target.size[0] as f32,
                target.size[1] as f32,
                look.amount.clamp(0.0, 1.0),
                if look.xray { XRAY } else { HIDDEN },
            ],
        );
        push_floats(&mut uniform, depth_terms(&projection));
        push_floats(&mut uniform, region_pixels(target.region, target.size));
        gpu.queue.write_buffer(&pipes.view, 0, &uniform);
        let (Some(edges), Some(placed)) = (&self.edges, &self.placed) else {
            return Err("the wireframe has no buffers".into());
        };
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("manfred wireframe"),
            layout: &pipes.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: pipes.view.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: edges.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: placed.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(target.depth),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("manfred wireframe"),
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
            timestamp_writes: stamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &group, &[]);
        pass.set_pipeline(&pipes.ground);
        pass.draw(0..3, 0..1);
        if let Some(under) = target.under {
            under.record(&mut pass);
            pass.set_bind_group(0, &group, &[]);
        }
        if self.count > 0 {
            pass.set_pipeline(&pipes.lines);
            pass.draw(0..self.count * 6, 0..1);
        }
        Ok(())
    }

    fn rebuild(&mut self, device: &wgpu::Device, scene: &Scene<'_>) {
        let most = device.limits().max_storage_buffer_binding_size / EDGE_BYTES;
        let mut kept: Quick<u64, Arc<[Segment]>> = Quick::default();
        let mut bytes = Vec::new();
        let mut count = 0u64;
        let mut push = |bytes: &mut Vec<u8>, line: &Segment, owner: u32| {
            if count >= most {
                return;
            }
            push_floats(bytes, line[0]);
            bytes.extend_from_slice(&owner.to_le_bytes());
            push_floats(bytes, line[1]);
            bytes.extend_from_slice(&0u32.to_le_bytes());
            count += 1;
        };
        for (owner, outline) in scene.outlines.iter().enumerate() {
            let lines: Arc<[Segment]> = match (outline.key, kept.get(&outline.key)) {
                (0, _) => edges(outline.positions, outline.indices, CREASE_DEGREES).into(),
                (_, Some(lines)) => lines.clone(),
                (key, None) => {
                    let lines = self.cache.remove(&key).unwrap_or_else(|| {
                        edges(outline.positions, outline.indices, CREASE_DEGREES).into()
                    });
                    kept.insert(key, lines.clone());
                    lines
                }
            };
            for line in lines.iter() {
                push(&mut bytes, line, owner as u32);
            }
        }
        let lines = route_lines(scene.routes);
        for (index, line) in lines.iter().enumerate() {
            let owner = (scene.outlines.len() + index) as u32;
            for pair in line.windows(2) {
                push(&mut bytes, &[pair[0], pair[1]], owner);
            }
        }
        self.cache = kept;
        self.routes = lines.len();
        self.count = count as u32;
        self.edges = Some(storage(device, "manfred wireframe edges", &bytes));
        self.written.clear();
        self.placed = None;
    }

    fn place(&mut self, gpu: &Gpu, scene: &Scene<'_>, scale: f32) {
        let scale = scale.max(0.5);
        let mut bytes = Vec::with_capacity((scene.outlines.len() + self.routes) * PLACED_BYTES);
        let put = |bytes: &mut Vec<u8>, model: &Matrix, colour: [f32; 4], width: f32, through| {
            push_matrix(bytes, model);
            push_floats(bytes, colour);
            push_floats(bytes, [width * scale, through, 0.0, 0.0]);
        };
        for outline in scene.outlines {
            if scene.selected.contains(&outline.id)
                || scene.pieces.iter().any(|piece| piece == outline.name)
            {
                put(
                    &mut bytes,
                    &outline.model,
                    linear(ACCENT, 1.0),
                    SELECTED_WIDTH,
                    0.0,
                );
            } else {
                put(
                    &mut bytes,
                    &outline.model,
                    linear(TEXT_PRIMARY, LINE_ALPHA),
                    LINE_WIDTH,
                    0.0,
                );
            }
        }
        for _ in 0..self.routes {
            put(&mut bytes, &IDENTITY, linear(ACCENT, 1.0), ROUTE_WIDTH, 1.0);
        }
        if bytes == self.written && self.placed.is_some() {
            return;
        }
        match &self.placed {
            Some(buffer) if buffer.size() >= bytes.len() as u64 && !bytes.is_empty() => {
                gpu.queue.write_buffer(buffer, 0, &bytes);
            }
            _ => {
                self.placed = Some(storage(&gpu.device, "manfred wireframe placed", &bytes));
            }
        }
        self.written = bytes;
    }
}

fn pipes(device: &wgpu::Device, format: wgpu::TextureFormat) -> Pipes {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("manfred wireframe"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let read = |binding: u32| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("manfred wireframe"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(VIEW_BYTES),
                },
                count: None,
            },
            read(1),
            read(2),
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
        label: Some("manfred wireframe"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = |vertex: &str, fragment: &str| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("manfred wireframe"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some(vertex),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(fragment),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    Pipes {
        format,
        lines: pipeline("line", "ink"),
        ground: pipeline("cover", "flat"),
        layout,
        view: device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("manfred wireframe view"),
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
    size: vec2f,
    amount: f32,
    hidden: f32,
    depth: vec4f,
    region: vec4f,
}

fn kept(pixel: vec2f) -> bool {
    let r = view.region;
    return r.x < r.z && pixel.x >= r.x && pixel.x < r.z && pixel.y >= r.y && pixel.y < r.w;
}

struct Edge {
    a: vec3f,
    owner: u32,
    b: vec3f,
    spare: u32,
}

struct Placed {
    model: mat4x4f,
    colour: vec4f,
    width: f32,
    through: f32,
    spare: vec2f,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> edges: array<Edge>;
@group(0) @binding(2) var<storage, read> placed: array<Placed>;
@group(0) @binding(3) var depth: texture_depth_2d;

struct Line {
    @builtin(position) clip: vec4f,
    @location(0) colour: vec4f,
    @location(1) across: f32,
    @location(2) far: f32,
    @location(3) @interpolate(flat) half: f32,
    @location(4) @interpolate(flat) through: f32,
}

fn trim(start: vec4f, end: vec4f) -> vec4f {
    let a = view.projection[2][2];
    let b = view.projection[3][2];
    let near = -0.5 * b / a;
    let alpha = (near - start.z) / (end.z - start.z);
    return vec4f(mix(start.xyz, end.xyz, alpha), end.w);
}

@vertex
fn line(@builtin(vertex_index) index: u32) -> Line {
    var corners = array<vec2f, 6>(
        vec2f(0.0, -1.0), vec2f(1.0, -1.0), vec2f(1.0, 1.0),
        vec2f(0.0, -1.0), vec2f(1.0, 1.0), vec2f(0.0, 1.0),
    );
    let edge = edges[index / 6u];
    let corner = corners[index % 6u];
    let owner = placed[edge.owner];
    let model_view = view.view * owner.model;
    var start = model_view * vec4f(edge.a, 1.0);
    var end = model_view * vec4f(edge.b, 1.0);
    var out: Line;
    out.colour = owner.colour;
    out.half = owner.width * 0.5;
    out.through = owner.through;
    out.across = 0.0;
    out.far = 0.0;
    if view.projection[2][3] == -1.0 {
        if start.z >= 0.0 && end.z >= 0.0 {
            out.clip = vec4f(0.0, 0.0, -1.0, 1.0);
            return out;
        }
        if start.z < 0.0 && end.z >= 0.0 {
            end = trim(start, end);
        } else if end.z < 0.0 && start.z >= 0.0 {
            start = trim(end, start);
        }
    }
    let clip_start = view.projection * start;
    let clip_end = view.projection * end;
    let ndc_start = clip_start.xy / clip_start.w;
    let ndc_end = clip_end.xy / clip_end.w;
    let aspect = view.size.x / view.size.y;
    var dir = ndc_end - ndc_start;
    dir.x *= aspect;
    if dot(dir, dir) > 1e-14 {
        dir = normalize(dir);
    } else {
        dir = vec2f(1.0, 0.0);
    }
    var offset = vec2f(dir.y, -dir.x);
    dir.x /= aspect;
    offset.x /= aspect;
    let reach = out.half + 1.0;
    let at_end = corner.x > 0.5;
    var shift = offset * corner.y + dir * select(-0.5, 0.5, at_end);
    shift *= 2.0 * reach / view.size.y;
    var clip = select(clip_start, clip_end, at_end);
    clip = vec4f(clip.xy + shift * clip.w, clip.zw);
    out.clip = clip;
    out.across = corner.y * reach;
    out.far = -select(start.z, end.z, at_end);
    return out;
}

fn distance_at(pixel: vec2i) -> f32 {
    let limit = vec2i(view.size) - vec2i(1, 1);
    let z = textureLoad(depth, clamp(pixel, vec2i(0, 0), limit), 0);
    if z >= 1.0 {
        return 1e30;
    }
    let terms = view.depth;
    let toward = (terms.y - z * terms.w) / (z * terms.z - terms.x);
    return -toward;
}

@fragment
fn ink(line: Line) -> @location(0) vec4f {
    let cover = clamp(line.half + 0.5 - abs(line.across), 0.0, 1.0);
    if cover <= 0.0 || kept(line.clip.xy) {
        discard;
    }
    var shown = 1.0;
    if line.through < 0.5 {
        let pixel = vec2i(line.clip.xy);
        let scene = max(
            max(distance_at(pixel), distance_at(pixel + vec2i(1, 0))),
            max(
                distance_at(pixel - vec2i(1, 0)),
                max(distance_at(pixel + vec2i(0, 1)), distance_at(pixel - vec2i(0, 1))),
            ),
        );
        if line.far > scene * 1.002 + 0.02 {
            shown = view.hidden;
        }
    }
    return vec4f(line.colour.rgb, line.colour.a * cover * shown * view.amount);
}

@vertex
fn cover(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let x = f32(index & 1u) * 4.0 - 1.0;
    let y = f32(index >> 1u) * 4.0 - 1.0;
    return vec4f(x, y, 0.0, 1.0);
}

@fragment
fn flat(@builtin(position) at: vec4f) -> @location(0) vec4f {
    if kept(at.xy) {
        discard;
    }
    return vec4f(view.ground.rgb, view.amount);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quad_keeps_its_four_sides_and_drops_the_flat_diagonal_and_a_fold_keeps_its_crease() {
        let quad = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ];
        assert_eq!(edges(&quad, &[0, 1, 2, 0, 2, 3], CREASE_DEGREES).len(), 4);
        let split = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ];
        assert_eq!(edges(&split, &[0, 1, 2, 3, 4, 5], CREASE_DEGREES).len(), 4);
        let fold = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        assert_eq!(edges(&fold, &[0, 1, 2, 0, 2, 3], CREASE_DEGREES).len(), 5);
        assert!(edges(&quad, &[0, 0, 1, 7, 8, 9], CREASE_DEGREES).is_empty());
    }

    fn street(columns: usize, rows: usize) -> (Vec<[f32; 3]>, Vec<u32>) {
        let mut positions = Vec::with_capacity((columns + 1) * (rows + 1));
        for row in 0..=rows {
            for column in 0..=columns {
                let x = column as f32 * 40.0 / columns as f32 - 20.0;
                let z = row as f32 * 600.0 / rows as f32;
                let y = 0.3 * (x * 1.3).sin() * (z * 0.7).cos() + 0.02 * (row % 3) as f32;
                positions.push([x, y, z]);
            }
        }
        let mut indices = Vec::with_capacity(columns * rows * 6);
        let at = |column: usize, row: usize| (row * (columns + 1) + column) as u32;
        for row in 0..rows {
            for column in 0..columns {
                let (a, b) = (at(column, row), at(column + 1, row));
                let (c, d) = (at(column + 1, row + 1), at(column, row + 1));
                indices.extend_from_slice(&[a, c, b, a, d, c]);
            }
        }
        (positions, indices)
    }

    fn timed(gpu: &Gpu, outlines: &[Outline<'_>], eye: &manfred_scene::Eye) -> (u32, f64, f64) {
        let size = [1586u32, 992];
        let texture = |format: wgpu::TextureFormat, label: &str| {
            gpu.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let output = texture(format, "bench output");
        let depth = texture(wgpu::TextureFormat::Depth32Float, "bench depth");
        let mut clear = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        clear.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        gpu.queue.submit([clear.finish()]);
        let scene = Scene {
            key: 1,
            outlines,
            routes: &[],
            selected: &[],
            pieces: &[],
        };
        let target = Target {
            output: &output,
            format,
            depth: &depth,
            size,
            camera: (eye.view(), eye.projection(size, 0.05, 1000.0)),
            scale: 1.0,
            region: None,
            under: None,
        };
        assert!(
            gpu.device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY),
            "the bench needs timestamp queries"
        );
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
        let mut wire = Wire::default();
        let mut times = Vec::new();
        for frame in 0..14 {
            let since = std::time::Instant::now();
            let mut encoder = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            wire.encode(
                gpu,
                &mut encoder,
                &scene,
                &target,
                Look {
                    amount: 1.0,
                    ..Look::default()
                },
                Some(wgpu::RenderPassTimestampWrites {
                    query_set: &set,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
            )
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
        (wire.edges(), times[0], times[times.len() / 2])
    }

    #[test]
    #[ignore = "GPU: pgpu run --class bench --as manfred -- cargo test -p manfred-ui wire -- --ignored --nocapture"]
    fn a_street_of_three_hundred_thousand_triangles_draws_its_wireframe_in_about_two_milliseconds()
    {
        let gpu = manfred_scene::block_on(Gpu::headless()).unwrap();
        let (positions, indices) = street(40, 3750);
        let made = std::time::Instant::now();
        let lines = edges(&positions, &indices, CREASE_DEGREES);
        let extracted = made.elapsed().as_secs_f64() * 1000.0;
        let eye = manfred_scene::Eye {
            position: [0.0, 1.6, -4.0],
            target: [0.0, 1.0, 40.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 50.0,
            shift: [0.0, 0.0],
            ortho_height: None,
        };
        let outlines = [Outline {
            key: 1,
            positions: &positions,
            indices: &indices,
            model: IDENTITY,
            id: 1,
            name: "",
        }];
        let (count, best, median) = timed(&gpu, &outlines, &eye);
        println!(
            "wireframe: {} triangles, {count} edges ({} extracted in {extracted:.1} ms), 1586x992: best {best:.3} ms, median {median:.3} ms",
            indices.len() / 3,
            lines.len(),
        );
        assert!(median < 4.0, "the wireframe took {median:.3} ms");
    }

    #[test]
    #[ignore = "GPU: manfred build showcase/chase.toml --out tmp/wire/chase, then pgpu run --class bench --as manfred -- cargo test -p manfred-ui wire -- --ignored --nocapture"]
    fn the_chase_street_draws_its_wireframe_in_about_two_milliseconds() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp/wire/chase/street.scene.toml");
        let scene = pfx_load::scene::Scene::open(&path).unwrap_or_else(|error| {
            panic!("build showcase/chase.toml into tmp/wire/chase first: {error}")
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
        let made = std::time::Instant::now();
        let lines: usize = outlines
            .iter()
            .map(|o| edges(o.positions, o.indices, CREASE_DEGREES).len())
            .sum();
        let extracted = made.elapsed().as_secs_f64() * 1000.0;
        let eye = manfred_scene::Eye::from_camera(&crate::watched::shot(&scene), 1586.0 / 992.0);
        let gpu = manfred_scene::block_on(Gpu::headless()).unwrap();
        let (count, best, median) = timed(&gpu, &outlines, &eye);
        println!(
            "wireframe of the chase street: {} draws, {triangles} triangles, {count} edges ({lines} extracted in {extracted:.1} ms), 1586x992: best {best:.3} ms, median {median:.3} ms",
            outlines.len(),
        );
        assert!(median < 4.0, "the wireframe took {median:.3} ms");
    }

    #[test]
    fn a_fade_steps_evenly_to_its_amount_and_an_amount_is_on_off_or_a_unit_number() {
        assert_eq!(fade(0.0, 1.0, 4), vec![0.25, 0.5, 0.75, 1.0]);
        assert_eq!(fade(1.0, 0.5, 2), vec![0.75, 0.5]);
        assert_eq!(fade(0.3, 1.0, 0), vec![1.0]);
        assert_eq!(amount("on"), Ok(1.0));
        assert_eq!(amount("off"), Ok(0.0));
        assert_eq!(amount("0.5"), Ok(0.5));
        assert!(amount("1.5").is_err());
        assert!(amount("half").is_err());
    }
}
