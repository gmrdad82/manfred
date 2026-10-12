use crate::content::{self, Painter, Picker, Route, Slots, Source};
use crate::light::{Bake, Light};
use crate::scene::{Camera, Scene};
use crate::vector::{cross, dot, sub, unit};
use pfx_geom::mesh::Mesh as Geometry;
use pfx_gpu::{Gpu, OffscreenTarget, PassTiming, wgpu};
use pfx_live::frame::{
    self, Camera as FrameCamera, Instance, InstanceSurface, Matrix, MeshData, MeshHandle, Sun,
};
use pfx_live::pipeline_cache::CacheLoad;
use pfx_live::probes::ProbeLighting;
use pfx_live::renderer::{Effects, Finish, Picked, Renderer, Text};
use pfx_live::warm::{Progress, Warming};
use pfx_materials::{Content, Material};
use pfx_post::Chain;
use std::path::Path;

pub const TURN_FRAMES: u32 = 8;

pub const IDENTITY: Matrix = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

fn rotate(v: [f32; 3], axis: [f32; 3], angle: f32) -> [f32; 3] {
    let (s, c) = angle.sin_cos();
    let d = dot(axis, v);
    let x = cross(axis, v);
    [0, 1, 2].map(|k| v[k] * c + x[k] * s + axis[k] * d * (1.0 - c))
}

#[derive(Clone, Debug)]
pub struct Look {
    pub fov: f32,
    pub sun: f32,
    pub sky: f32,
    pub turn: f32,
    pub near: f32,
    pub far: f32,
    pub chain: Chain,
    pub finish: Finish,
}

impl Look {
    pub fn of(scene: &Scene, near: f32, far: f32, chain: Chain, finish: Finish) -> Look {
        Look {
            fov: scene.camera.fov_y,
            sun: scene.sun.irradiance,
            sky: scene.sky.intensity,
            turn: scene.sky.rotation(),
            near,
            far,
            chain,
            finish,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct View {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub dolly: f32,
    pub zoom: f32,
    pub look: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lens {
    pub eye: [f32; 3],
    pub forward: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub tan_y: f32,
    pub zoom: f32,
    pub shift: [f32; 2],
    pub pivot: [f32; 3],
    pub near: f32,
    pub far: f32,
}

impl Lens {
    pub fn rest(camera: &Camera, look: &Look) -> Lens {
        let forward = unit(sub(camera.target, camera.position));
        let right = unit(cross(forward, camera.up));
        let up = cross(right, forward);
        Lens {
            eye: camera.position,
            forward,
            right,
            up,
            tan_y: (look.fov.to_radians() * 0.5).tan(),
            zoom: 1.0,
            shift: camera.shift,
            pivot: camera.target,
            near: look.near,
            far: look.far,
        }
    }

    pub fn viewed(&self, view: &View) -> Lens {
        let (o, fw, r, u) = (self.eye, self.forward, self.right, self.up);
        let arm = sub(o, self.pivot);
        let (arm, fw, r) = (
            rotate(arm, u, view.yaw),
            rotate(fw, u, view.yaw),
            rotate(r, u, view.yaw),
        );
        let (arm, fw, u) = (
            rotate(arm, r, view.pitch),
            rotate(fw, r, view.pitch),
            rotate(u, r, view.pitch),
        );
        let (r, u) = (rotate(r, fw, view.roll), rotate(u, fw, view.roll));
        let o = [0, 1, 2].map(|k| self.pivot[k] + arm[k] + fw[k] * view.dolly);
        let (fw, r) = (rotate(fw, u, view.look[0]), rotate(r, u, view.look[0]));
        let (fw, u) = (rotate(fw, r, view.look[1]), rotate(u, r, view.look[1]));
        Lens {
            eye: o,
            forward: fw,
            right: r,
            up: u,
            zoom: view.zoom.max(0.25),
            ..*self
        }
    }

    pub fn tangents(&self, width: u32, height: u32) -> [f32; 2] {
        let tan_y = self.tan_y / self.zoom;
        [tan_y * width as f32 / height.max(1) as f32, tan_y]
    }

    pub fn view(&self) -> Matrix {
        let (r, u, f, e) = (self.right, self.up, self.forward, self.eye);
        [
            [r[0], u[0], -f[0], 0.0],
            [r[1], u[1], -f[1], 0.0],
            [r[2], u[2], -f[2], 0.0],
            [-dot(r, e), -dot(u, e), dot(f, e), 1.0],
        ]
    }

    pub fn projection(&self, width: u32, height: u32) -> Matrix {
        let [tan_x, tan_y] = self.tangents(width, height);
        let (near, far) = (self.near, self.far);
        [
            [1.0 / tan_x, 0.0, 0.0, 0.0],
            [0.0, 1.0 / tan_y, 0.0, 0.0],
            [self.shift[0], self.shift[1], far / (near - far), -1.0],
            [0.0, 0.0, far * near / (near - far), 0.0],
        ]
    }
}

pub fn materials<F>(scene: &Scene, mut lookup: F) -> Vec<Material>
where
    F: FnMut(&str) -> Material,
{
    scene.materials.iter().map(|name| lookup(name)).collect()
}

pub fn groups(scene: &Scene) -> Vec<Option<usize>> {
    scene
        .pieces
        .iter()
        .map(|piece| {
            scene
                .group_of(piece.object)
                .and_then(|name| scene.movers.iter().position(|m| m.name == name))
        })
        .collect()
}

pub fn keeps_reflection(scene: &Scene, piece: usize, keep: &[&str]) -> bool {
    let Some(piece) = scene.pieces.get(piece) else {
        return false;
    };
    let names = [
        scene.objects.get(piece.object as usize),
        scene.materials.get(piece.material as usize),
    ];
    names
        .into_iter()
        .flatten()
        .any(|name| keep.contains(&name.as_str()))
}

pub fn surfaces(scene: &Scene, groups: &[Option<usize>], keep: &[&str]) -> Vec<InstanceSurface> {
    groups
        .iter()
        .enumerate()
        .map(|(piece, group)| {
            let surface = match group {
                Some(g) => InstanceSurface {
                    clip: [scene.movers[*g].clip, scene.movers[*g].clip2],
                    ..Default::default()
                },
                None => InstanceSurface::default(),
            };
            if keeps_reflection(scene, piece, keep) {
                InstanceSurface {
                    reflection_occlusion: 0.0,
                    ..surface
                }
            } else {
                surface
            }
        })
        .collect()
}

pub fn geometry(scene: &Scene) -> Vec<Geometry> {
    scene
        .pieces
        .iter()
        .map(|piece| {
            Geometry::new(
                piece.positions.clone(),
                piece.normals.clone(),
                Vec::new(),
                piece.uvs.clone(),
                piece.indices.clone(),
            )
        })
        .collect()
}

pub struct Plan {
    pub meshes: Vec<Geometry>,
    pub instances: Vec<Instance>,
    pub materials: Vec<Material>,
    pub groups: Vec<Option<usize>>,
    pub surfaces: Vec<InstanceSurface>,
    pub contents: Vec<Content>,
    pub parts: Vec<u32>,
    pub movers: usize,
    pub camera: Camera,
    pub sun: Sun,
}

impl Plan {
    pub fn new<F>(scene: &Scene, look: &Look, lookup: F, keep: &[&str]) -> Plan
    where
        F: FnMut(&str) -> Material,
    {
        let materials = materials(scene, lookup);
        let groups = groups(scene);
        let instances: Vec<Instance> = scene
            .pieces
            .iter()
            .enumerate()
            .map(|(k, piece)| {
                Instance::new(
                    MeshHandle(k as u32),
                    IDENTITY,
                    piece.material,
                    content::id_of(piece.object),
                )
            })
            .collect();
        Plan {
            meshes: geometry(scene),
            contents: instances
                .iter()
                .map(|i| {
                    materials
                        .get(i.material as usize)
                        .map_or(Content::None, |m| m.content)
                })
                .collect(),
            instances,
            surfaces: surfaces(scene, &groups, keep),
            groups,
            materials,
            parts: scene.pieces.iter().map(|piece| piece.part).collect(),
            movers: scene.movers.len(),
            camera: scene.camera.clone(),
            sun: Sun {
                direction: unit(scene.sun.toward),
                colour: scene.sun.color,
                intensity: look.sun,
            },
        }
    }
}

pub struct Raster<P: Painter> {
    renderer: Renderer,
    output: Option<OffscreenTarget>,
    instances: Vec<Instance>,
    groups: Vec<Option<usize>>,
    surfaces: Vec<InstanceSurface>,
    contents: Vec<Content>,
    slots: Slots,
    painter: P,
    aimed: Option<(u32, u32)>,
    picked: Option<Picked>,
    tracked: Option<(u32, u32)>,
    posed: Vec<Matrix>,
    materials: Vec<Material>,
    rest: Lens,
    lens: Lens,
    sun: Sun,
    seed: u32,
    previous: Option<Matrix>,
    drawn: u32,
    light: Light,
    parts: Vec<u32>,
    passes: Vec<PassTiming>,
    warmed: bool,
    warming: Option<Warming>,
    saved: bool,
    finish: Finish,
    turn: fn(),
}

fn output(gpu: &Gpu, width: u32, height: u32) -> OffscreenTarget {
    let format = wgpu::TextureFormat::Rgba16Float;
    let texture = scratch(
        gpu,
        width,
        height,
        format,
        "runtime live output",
        wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::STORAGE_BINDING,
    );
    let view = texture.create_view(&Default::default());
    OffscreenTarget {
        texture,
        view,
        format,
        width,
        height,
    }
}

fn scratch(
    gpu: &Gpu,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    label: &str,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | usage,
        view_formats: &[],
    })
}

pub struct Parts<P: Painter> {
    pub plan: Plan,
    pub light: Light,
    pub look: Look,
    pub painter: P,
    pub turn: fn(),
}

impl<P: Painter> Raster<P> {
    pub fn window(
        gpu: &Gpu,
        parts: Parts<P>,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Result<Raster<P>, String> {
        let renderer = Renderer::new_with_output_format(gpu.clone(), size.0, size.1, format)?;
        Raster::build(renderer, None, parts)
    }

    pub fn headless(gpu: &Gpu, parts: Parts<P>, size: (u32, u32)) -> Result<Raster<P>, String> {
        Raster::headless_cached(gpu, parts, size, None).map(|(raster, _)| raster)
    }

    pub fn headless_cached(
        gpu: &Gpu,
        parts: Parts<P>,
        size: (u32, u32),
        cache: Option<&Path>,
    ) -> Result<(Raster<P>, Option<CacheLoad>), String> {
        let renderer = Renderer::new(gpu.clone(), size.0, size.1)?;
        let target = output(gpu, size.0, size.1);
        let mut raster = Raster::build(renderer, Some(target), parts)?;
        let load = cache.map(|path| raster.use_pipeline_cache(path));
        raster.warm_and_wait()?;
        if load.is_some() {
            raster.warmed_save().transpose()?;
        }
        Ok((raster, load))
    }

    fn build(
        mut renderer: Renderer,
        output: Option<OffscreenTarget>,
        parts: Parts<P>,
    ) -> Result<Raster<P>, String> {
        let Parts {
            plan,
            mut light,
            look,
            painter,
            turn,
        } = parts;
        let mut instances = plan.instances;
        for (instance, mesh) in instances.iter_mut().zip(&plan.meshes) {
            instance.mesh = renderer.upload_mesh(MeshData {
                positions: &mesh.positions,
                normals: &mesh.normals,
                tangents: &mesh.tangents,
                uvs: &mesh.uvs,
                uvs1: None,
                alpha: None,
                indices: &mesh.indices,
            })?;
        }
        let rest = Lens::rest(&plan.camera, &look);
        settle(&renderer)?;
        turn();
        light.install(&mut renderer, &look, &rest)?;
        settle(&renderer)?;
        turn();
        Ok(Raster {
            renderer,
            output,
            contents: plan.contents,
            instances,
            surfaces: plan.surfaces,
            slots: Slots::default(),
            painter,
            aimed: None,
            picked: None,
            tracked: None,
            groups: plan.groups,
            posed: vec![IDENTITY; plan.movers],
            materials: plan.materials,
            rest,
            lens: rest,
            sun: plan.sun,
            seed: 0,
            previous: None,
            drawn: 0,
            light,
            parts: plan.parts,
            passes: Vec::new(),
            warmed: false,
            warming: None,
            saved: false,
            finish: look.finish,
            turn,
        })
    }

    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    pub fn instances(&self) -> &[Instance] {
        &self.instances
    }

    pub fn materials(&self) -> &[Material] {
        &self.materials
    }

    pub fn painter(&self) -> &P {
        &self.painter
    }

    pub fn painter_mut(&mut self) -> &mut P {
        &mut self.painter
    }

    pub fn plain(&mut self, pieces: &[usize]) -> Vec<u32> {
        let mut previous = Vec::with_capacity(pieces.len());
        for &piece in pieces {
            let Some(instance) = self.instances.get_mut(piece) else {
                continue;
            };
            let Some(mut material) = self.materials.get(instance.material as usize).copied() else {
                continue;
            };
            previous.push(instance.material);
            material.layers = Default::default();
            instance.material = self.materials.len() as u32;
            self.materials.push(material);
        }
        previous
    }

    pub fn assign(&mut self, piece: usize, material: u32) -> Option<u32> {
        if material as usize >= self.materials.len() {
            return None;
        }
        let instance = self.instances.get_mut(piece)?;
        Some(std::mem::replace(&mut instance.material, material))
    }

    pub fn restore(&mut self, pieces: &[usize], previous: &[u32]) {
        for (&piece, &material) in pieces.iter().zip(previous) {
            if let Some(instance) = self.instances.get_mut(piece) {
                self.materials
                    .truncate(self.materials.len().min(instance.material as usize));
                instance.material = material;
            }
        }
    }

    pub fn groups(&self) -> &[Option<usize>] {
        &self.groups
    }

    pub fn surfaces(&self) -> &[InstanceSurface] {
        &self.surfaces
    }

    pub fn filled(&self) -> (&[bool], u64) {
        (self.slots.filled(), self.slots.uploads)
    }

    pub fn feed(&mut self, sources: &[Source<'_>]) -> Result<(), String> {
        self.slots.feed(&self.renderer, &mut self.painter, sources)
    }

    pub fn aim(&mut self, cursor: (f32, f32)) {
        self.aimed = content::pixel(cursor, self.renderer.size());
    }

    pub fn picked(&self) -> Option<Picked> {
        self.picked
    }

    pub fn fresh(&mut self) -> bool {
        let at = self.picked.map(|p| (p.x, p.y));
        let fresh = at.is_some() && at != self.tracked;
        self.tracked = at;
        fresh
    }

    pub fn pick<K: Picker>(
        &self,
        cursor: (f32, f32),
        picker: &K,
        ray: ([f32; 3], [f32; 3]),
    ) -> (Option<K::Hit>, Route) {
        let at = content::pixel(cursor, self.renderer.size());
        content::resolve(self.picked, at, picker, ray.0, ray.1)
    }

    pub fn light(&self) -> &Light {
        &self.light
    }

    pub fn probes(&self) -> &ProbeLighting {
        self.renderer.frame().probes()
    }

    pub fn set_probes(&mut self, mut probes: ProbeLighting) -> Result<(), String> {
        let threshold = self.light.lighting.reflection_occlusion;
        probes.set_reflection_occlusion(&self.renderer.gpu().queue, threshold)?;
        self.renderer.set_probes(probes)
    }

    pub fn set_reflection_occlusion(&mut self, threshold: f32) -> Result<(), String> {
        self.renderer.set_reflection_occlusion(threshold)
    }

    pub fn passes(&self) -> &[PassTiming] {
        &self.passes
    }

    pub fn set_hour(&mut self, hour: f32) -> Result<(), String> {
        self.light.set_hour(&mut self.renderer, hour)
    }

    pub fn size(&self) -> (u32, u32) {
        self.renderer.size()
    }

    pub fn bake(&mut self, bake: Bake) -> Result<usize, String> {
        self.warmed = false;
        self.light.bake(
            &mut self.renderer,
            bake,
            &self.parts,
            &mut self.materials,
            &mut self.instances,
        )
    }

    pub fn sky_settling(&self) -> bool {
        self.renderer.sky_settling()
    }

    pub fn pose(&mut self, poses: &[Matrix]) {
        for (posed, pose) in self.posed.iter_mut().zip(poses) {
            *posed = *pose;
        }
    }

    pub fn jitter(&self) -> [f32; 2] {
        pfx_live::taa::jitter_pixels(self.drawn)
    }

    pub fn lens(&self) -> &Lens {
        &self.lens
    }

    pub fn set_view(&mut self, view: &View) {
        self.lens = self.rest.viewed(view);
    }

    pub fn reseed(&mut self, seed: u64) {
        self.seed = (seed ^ (seed >> 32)) as u32;
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        self.renderer.resize(width, height)?;
        self.light.resized(&mut self.renderer, &self.rest);
        if self.output.is_some() {
            self.output = Some(output(self.renderer.gpu(), width, height));
        }
        self.previous = None;
        Ok(())
    }

    fn still_camera(&self) -> FrameCamera {
        let (width, height) = self.renderer.size();
        let view = self.lens.view();
        let projection = self.lens.projection(width, height);
        FrameCamera {
            view,
            projection,
            previous_view_projection: frame::multiply(projection, view),
            position: self.lens.eye,
        }
    }

    fn camera(&mut self) -> FrameCamera {
        let mut camera = self.still_camera();
        let current = camera.previous_view_projection;
        camera.previous_view_projection = self.previous.replace(current).unwrap_or(current);
        camera
    }

    fn scene<'a>(
        &self,
        camera: FrameCamera,
        time: f32,
        instances: &'a [Instance],
        materials: &'a [Material],
    ) -> frame::Scene<'a> {
        frame::Scene {
            camera,
            time,
            seed: self.seed,
            sun: self.sun,
            instances,
            materials,
            deformers: &[],
            wind: self.light.wind(time),
        }
    }

    fn place(&mut self, fresh: bool) {
        for (instance, group) in self.instances.iter_mut().zip(&self.groups) {
            instance.previous_model = instance.model;
            if let Some(g) = group {
                instance.model = self.posed[*g];
            }
            if fresh {
                instance.previous_model = instance.model;
            }
        }
    }

    pub fn draw(&mut self, time: f32, target: &wgpu::TextureView) -> Result<(), String> {
        if !self.warmed {
            self.warm()?;
        }
        self.place(self.previous.is_none());
        let surfaces = self
            .slots
            .surfaces(&self.painter, &self.contents, &self.surfaces);
        let materials = self.slots.materials(&self.painter, &self.materials);
        if let Some((x, y)) = self.aimed {
            self.renderer.pick(x, y)?;
        }
        let instances = std::mem::take(&mut self.instances);
        let drawn = self.submit(time, target, &instances, &materials, &surfaces);
        self.instances = instances;
        drawn?;
        if let Some(picked) = self.renderer.picked() {
            self.picked = Some(picked);
        }
        Ok(())
    }

    fn submit(
        &mut self,
        time: f32,
        target: &wgpu::TextureView,
        instances: &[Instance],
        materials: &[Material],
        surfaces: &[InstanceSurface],
    ) -> Result<(), String> {
        self.renderer.frame().set_surfaces(surfaces)?;
        let camera = self.camera();
        let time = time.max(0.0);
        self.drawn = (time * 60.0).round() as u32;
        let scene = self.scene(camera, time, instances, materials);
        let particles = self.light.particles(time);
        let creatures = self.light.birds(time);
        let effects = Effects {
            volume: self.light.volume(),
            particles: &particles,
            creatures: &creatures,
            creature_mesh: self.light.bird_mesh(),
            ..Default::default()
        };
        self.passes =
            self.renderer
                .render(&scene, &Text::default(), &effects, self.finish, target)?;
        Ok(())
    }

    pub fn dresses(&self) -> Vec<Vec<Material>> {
        let current = self.slots.materials(&self.painter, &self.materials);
        content::dresses(&self.painter, current, &self.materials)
    }

    pub fn use_pipeline_cache(&mut self, path: &Path) -> CacheLoad {
        self.renderer.use_pipeline_cache(path)
    }

    pub fn warm(&mut self) -> Result<(), String> {
        self.warmed = true;
        self.saved = false;
        let surfaces = self
            .slots
            .surfaces(&self.painter, &self.contents, &self.surfaces);
        self.renderer.frame().set_surfaces(&surfaces)?;
        let camera = self.still_camera();
        let dresses = self.dresses();
        let instances = &self.instances;
        let scenes: Vec<frame::Scene> = dresses
            .iter()
            .map(|materials| self.scene(camera, 0.0, instances, materials))
            .collect();
        self.warming = Some(self.renderer.warm(&scenes)?);
        Ok(())
    }

    pub fn warming(&self) -> Option<&Warming> {
        self.warming.as_ref()
    }

    pub fn ready(&self) -> bool {
        self.warming.as_ref().is_none_or(Warming::first_ready)
    }

    pub fn warmed_save(&mut self) -> Option<Result<Option<usize>, String>> {
        if self.saved || !self.warming.as_ref().is_some_and(Warming::done) {
            return None;
        }
        self.saved = true;
        Some(self.renderer.save_pipeline_cache())
    }

    pub fn made_on_render_thread(&self) -> usize {
        self.renderer.frame().opaque_made_on_render_thread()
    }

    pub fn warm_and_wait(&mut self) -> Result<Progress, String> {
        let view = match &self.output {
            Some(target) => target.view.clone(),
            None => {
                let (width, height) = self.renderer.size();
                let format = self.renderer.output_format();
                scratch(
                    self.renderer.gpu(),
                    width,
                    height,
                    format,
                    "runtime live warm",
                    wgpu::TextureUsages::empty(),
                )
                .create_view(&Default::default())
            }
        };
        self.warm()?;
        let progress = self
            .warming
            .as_ref()
            .map(Warming::wait)
            .ok_or("the warm did not start")?;
        (self.turn)();
        self.draw(0.0, &view)?;
        self.previous = None;
        self.settle()?;
        (self.turn)();
        Ok(progress)
    }

    pub fn shoot(&mut self, time: f32) -> Result<(), String> {
        if self.sky_settling() {
            self.renderer.wait_sky()?;
            self.settle()?;
            (self.turn)();
        }
        let view = self
            .output
            .as_ref()
            .ok_or("only a headless live renderer shoots offscreen")?
            .view
            .clone();
        self.draw(time, &view)
    }

    pub fn still(&mut self, time: f32, frames: u32) -> Result<(), String> {
        for k in 0..frames {
            self.shoot(time + k as f32 / 60.0)?;
            self.turned(k)?;
        }
        self.settle()
    }

    pub fn follow(
        &mut self,
        time: f32,
        frames: u32,
        track: &[Vec<Matrix>],
        steps: usize,
    ) -> Result<(), String> {
        let last = track.len().saturating_sub(1);
        let start = (time - frames.saturating_sub(1) as f32 / 60.0).max(0.0);
        for k in 0..frames {
            let back = (frames - 1 - k) as usize * steps;
            if let Some(poses) = track.get(last.saturating_sub(back)) {
                self.pose(poses);
            }
            self.shoot(start + k as f32 / 60.0)?;
            self.turned(k)?;
        }
        self.settle()
    }

    fn turned(&self, frame: u32) -> Result<(), String> {
        if frame % TURN_FRAMES == TURN_FRAMES - 1 {
            self.settle()?;
            (self.turn)();
        }
        Ok(())
    }

    pub fn settle(&self) -> Result<(), String> {
        settle(&self.renderer)
    }

    pub fn capture(&self) -> Result<Vec<u8>, String> {
        let target = self
            .output
            .as_ref()
            .ok_or("only an offscreen live frame can be captured")?;
        let halves = self.renderer.gpu().readback_rgba16(target)?;
        let mut rgb = Vec::with_capacity(halves.len() / 4 * 3);
        for pixel in halves.chunks_exact(4) {
            rgb.extend(pixel[..3].iter().map(|&v| {
                (half::f16::from_bits(v).to_f32().clamp(0.0, 1.0) * 255.0).round() as u8
            }));
        }
        Ok(rgb)
    }

    pub fn velocity(&self) -> Result<Vec<[u16; 2]>, String> {
        self.renderer.frame().readback_velocity()
    }

    pub fn ids(&self) -> Result<Vec<u32>, String> {
        let gpu = self.renderer.gpu();
        let targets = &self.renderer.frame().targets;
        let (width, height) = (targets.width, targets.height);
        let row = (width * 4).div_ceil(256) * 256;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("runtime live ids"),
            size: u64::from(row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            targets.ids.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            targets.ids.size(),
        );
        gpu.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.settle()?;
        let bytes = buffer.slice(..).get_mapped_range().expect("a mapped range");
        let mut ids = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            let start = (y * row) as usize;
            ids.extend(
                bytes[start..start + (width * 4) as usize]
                    .chunks_exact(4)
                    .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            );
        }
        Ok(ids)
    }
}

fn settle(renderer: &Renderer) -> Result<(), String> {
    renderer
        .gpu()
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn moving(before: &[Matrix], after: &[Matrix]) -> Vec<bool> {
    before.iter().zip(after).map(|(a, b)| a != b).collect()
}

pub fn still(velocity: [u16; 2]) -> bool {
    velocity == [0, 0]
}
