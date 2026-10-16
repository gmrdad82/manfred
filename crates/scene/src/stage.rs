use crate::cards::{self, Atlas, Dress, Shown};
use crate::light::{Hours, Lighting};
use crate::maps;
use crate::memory::Memory;
use crate::parts::{self, Palette, Piece};
use crate::text::{self, Drawn, Laid};
use crate::{Error, Eye, IDENTITY};
use manfred_build::{Built, Part};
use manfred_recipe::{Camera as Shot, Daylight, Recipe, Shadow, Sky, Sun as Lamp};
use pfx::adapter::{TurnState, turn_now, turn_soon};
use pfx_gpu::pace::{Pacer, Stats};
use pfx_gpu::{Gpu, wgpu};
use pfx_live::frame::{
    Camera, Frame, Instance, Matrix, MeshData, MeshHandle, MeshStats, Scene, SceneWind, Sun,
    multiply,
};
use pfx_live::glass;
use pfx_live::maps::MapImages;
use pfx_live::renderer::{Effects, Exposure, Finish, Renderer, RendererError, Text};
use pfx_live::shadow::{Quality, ReceiverBox};
use pfx_materials::Material;
use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

const OUTPUT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const PACED_TEXELS: u64 = 8192 * 8192;
const EMPTY: [[f64; 3]; 2] = [[-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]];
const CASTER_MARGIN: f32 = 0.3;
const IN_FLIGHT: u64 = 2;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Key {
    name: String,
    ordinal: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Mark {
    key: Key,
    hash: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Keep(usize),
    Replace(usize),
    Add,
}

#[derive(Debug, PartialEq, Eq)]
struct Plan {
    actions: Vec<Action>,
    released: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pick {
    pub pixel: [u32; 2],
    pub object: Option<usize>,
    pub depth: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Change {
    pub kept: usize,
    pub replaced: usize,
    pub added: usize,
    pub released: usize,
    pub sky: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct SkyKey {
    sun: Option<Lamp>,
    daylight: Option<Daylight>,
    sky: Option<Sky>,
    base: Option<PathBuf>,
}

impl SkyKey {
    fn of(recipe: &Recipe) -> SkyKey {
        SkyKey {
            sun: recipe.sun.clone(),
            daylight: recipe.daylight.clone(),
            sky: recipe.sky.clone(),
            base: recipe.base.clone(),
        }
    }
}

pub struct Prepared {
    pieces: Vec<Piece>,
    marks: Vec<Mark>,
    slots: Vec<u32>,
    materials: Vec<Material>,
    names: Vec<String>,
    lighting: Lighting,
    sky: SkyKey,
    bounds: [[f64; 3]; 2],
    receiver: [[f64; 3]; 2],
    seed: u32,
    parents: Vec<Option<usize>>,
    text: Laid,
    glass: Vec<bool>,
    clips: Vec<[[f32; 4]; 2]>,
    dress: Vec<Dress>,
    atlas: Option<Atlas>,
    maps: Option<maps::Set>,
}

impl Prepared {
    pub fn of(recipe: &Recipe, parts: &[Part]) -> Result<Prepared, Error> {
        Prepared::kept(recipe, parts, &Memory::default())
    }

    pub fn kept(recipe: &Recipe, parts: &[Part], memory: &Memory) -> Result<Prepared, Error> {
        let dressed = cards::dress(recipe, parts::pieces(parts), memory)?;
        let pieces = dressed.pieces;
        let Palette {
            names,
            mut materials,
            slots,
        } = parts::palette(
            recipe,
            &pieces,
            dressed.dress.iter().map(|worn| worn.card.is_some()),
        )?;
        let maps = maps::Set::of(&pieces);
        if let Some(set) = &maps {
            set.dress(&names, &mut materials);
        }
        let glass: Vec<bool> = slots
            .iter()
            .map(|&slot| parts::glassy(&materials[slot as usize]))
            .collect();
        let clips = pieces
            .iter()
            .map(|piece| cards::clips(recipe, piece.object))
            .collect();
        let lighting = Lighting::kept(recipe, memory)?;
        let text = Laid::of(recipe, parts)?;
        let bounds = match parts::bounds(&pieces, false) {
            Some(bounds) => text::widen(bounds, text.bounds()),
            None => text.bounds().unwrap_or(EMPTY),
        };
        let receiver = parts::bounds(&pieces, true).unwrap_or(bounds);
        Ok(Prepared {
            marks: marks(&pieces),
            pieces,
            slots,
            materials,
            names,
            lighting,
            sky: SkyKey::of(recipe),
            bounds,
            receiver,
            seed: crate::seed(recipe),
            parents: parents(recipe),
            text,
            glass,
            clips,
            dress: dressed.dress,
            atlas: dressed.atlas,
            maps,
        })
    }

    pub fn meshes(&self) -> usize {
        self.pieces.len()
    }

    #[cfg(test)]
    pub fn glass_parts(&self) -> Vec<usize> {
        let mut objects: Vec<usize> = shown_glass(&self.glass)
            .map(|at| self.pieces[at].object)
            .collect();
        objects.dedup();
        objects
    }
}

struct Uploaded {
    mark: Mark,
    handle: MeshHandle,
}

struct Glassed {
    mark: Mark,
    handle: glass::MeshHandle,
}

pub struct Stage {
    renderer: Renderer,
    size: [u32; 2],
    uploaded: Vec<Uploaded>,
    instances: Vec<Instance>,
    materials: Vec<Material>,
    names: Vec<String>,
    lighting: Arc<Lighting>,
    hours: Hours,
    waiting: Vec<(u64, Option<f64>)>,
    sky: Option<SkyKey>,
    hour: Option<f64>,
    shown: Option<f64>,
    sun: Sun,
    bounds: [[f64; 3]; 2],
    receiver: [[f64; 3]; 2],
    seed: u32,
    previous: Option<Matrix>,
    parents: Vec<Option<usize>>,
    text: Drawn,
    glassed: Vec<Glassed>,
    glass: Vec<glass::Surface>,
    dress: Vec<Dress>,
    atlas: Option<Shown>,
    maps: Option<u64>,
    pacer: Pacer,
    faced: Vec<Instance>,
    through: Option<Shot>,
    posed: BTreeMap<String, Matrix>,
    pieces: Vec<Piece>,
    opaque: Vec<usize>,
    glazed: Vec<usize>,
    generation: u64,
    looked: Option<(Matrix, Matrix)>,
}

pub struct Outline<'a> {
    pub key: u64,
    pub positions: &'a [[f32; 3]],
    pub indices: &'a [u32],
    pub model: Matrix,
    pub id: u32,
    pub name: &'a str,
}

impl Stage {
    pub fn new(gpu: Gpu, recipe: &Recipe, built: &Built, size: [u32; 2]) -> Result<Stage, Error> {
        Stage::kept(gpu, recipe, built, size, &Memory::default())
    }

    pub fn with_parts(
        gpu: Gpu,
        recipe: &Recipe,
        parts: &[Part],
        size: [u32; 2],
    ) -> Result<Stage, Error> {
        Stage::made(gpu, Prepared::of(recipe, parts)?, size)
    }

    pub fn kept(
        gpu: Gpu,
        recipe: &Recipe,
        built: &Built,
        size: [u32; 2],
        memory: &Memory,
    ) -> Result<Stage, Error> {
        Stage::made(gpu, Stage::prepare_kept(recipe, built, memory)?, size)
    }

    pub fn prepare(recipe: &Recipe, built: &Built) -> Result<Prepared, Error> {
        Stage::prepare_kept(recipe, built, &Memory::default())
    }

    pub fn prepare_kept(
        recipe: &Recipe,
        built: &Built,
        memory: &Memory,
    ) -> Result<Prepared, Error> {
        Prepared::kept(&built.recipe(recipe), &built.parts, memory)
    }

    pub fn made(gpu: Gpu, prepared: Prepared, size: [u32; 2]) -> Result<Stage, Error> {
        let mut renderer = Renderer::new_with_output_format(gpu, size[0], size[1], OUTPUT)
            .map_err(|e| Error(refused(&e)))?;
        renderer.set_exposure(Exposure::Fixed(1.0)).map_err(Error)?;
        let lighting = Arc::new(prepared.lighting.clone());
        let mut stage = Stage {
            renderer,
            size,
            uploaded: Vec::new(),
            instances: Vec::new(),
            materials: Vec::new(),
            names: Vec::new(),
            hours: Hours::new(lighting.clone()),
            sun: lighting.shine(None).sun,
            lighting,
            waiting: Vec::new(),
            sky: None,
            hour: None,
            shown: None,
            bounds: EMPTY,
            receiver: EMPTY,
            seed: 0,
            previous: None,
            parents: Vec::new(),
            text: Drawn::default(),
            glassed: Vec::new(),
            glass: Vec::new(),
            dress: Vec::new(),
            atlas: None,
            maps: None,
            pacer: Pacer::default(),
            faced: Vec::new(),
            through: None,
            posed: BTreeMap::new(),
            pieces: Vec::new(),
            opaque: Vec::new(),
            glazed: Vec::new(),
            generation: 0,
            looked: None,
        };
        stage.apply(prepared)?;
        Ok(stage)
    }

    pub fn update(&mut self, recipe: &Recipe, built: &Built) -> Result<Change, Error> {
        let prepared = Stage::prepare(recipe, built)?;
        self.apply(prepared)
    }

    pub fn sky_changes(&self, prepared: &Prepared) -> bool {
        self.sky.as_ref() != Some(&prepared.sky)
    }

    pub fn apply(&mut self, prepared: Prepared) -> Result<Change, Error> {
        let Prepared {
            pieces,
            marks,
            slots,
            materials,
            names,
            lighting,
            sky,
            bounds,
            receiver,
            seed,
            parents,
            text,
            glass,
            clips,
            dress,
            atlas,
            maps,
        } = prepared;
        let opaque = opaque_at(&glass);
        let opaque_marks: Vec<Mark> = opaque.iter().map(|&at| marks[at].clone()).collect();
        let plan = plan(self.uploaded.iter().map(|u| &u.mark), &opaque_marks);
        let mut change = Change {
            sky: self.sky.as_ref() != Some(&sky),
            ..Change::default()
        };
        let glassed = glaze(
            &mut self.renderer,
            &pieces,
            &marks,
            &glass,
            std::mem::take(&mut self.glassed),
        )?;
        self.glass = glass_surfaces(&pieces, &slots, &materials, &glass, &clips, &glassed);
        for (surface, glassed) in self.glass.iter_mut().zip(&glassed) {
            surface.model = self
                .posed
                .get(&glassed.mark.key.name)
                .copied()
                .unwrap_or(IDENTITY);
        }
        self.glassed = glassed;
        let mut uploaded = Vec::with_capacity(opaque.len());
        for ((&index, mark), action) in opaque.iter().zip(opaque_marks).zip(&plan.actions) {
            let piece = &pieces[index];
            let handle = match *action {
                Action::Keep(at) => {
                    change.kept += 1;
                    self.uploaded[at].handle
                }
                Action::Replace(at) => {
                    let handle = self.uploaded[at].handle;
                    self.renderer
                        .replace_mesh(handle, data(piece))
                        .map_err(|e| Error(format!("object {}: {e}", piece.name)))?;
                    change.replaced += 1;
                    handle
                }
                Action::Add => {
                    change.added += 1;
                    upload(&mut self.renderer, piece)?
                }
            };
            uploaded.push(Uploaded { mark, handle });
        }
        for &at in &plan.released {
            self.renderer
                .release_mesh(self.uploaded[at].handle)
                .map_err(|e| Error(format!("object {}: {e}", self.uploaded[at].mark.key.name)))?;
            change.released += 1;
        }
        let dress: Vec<Dress> = opaque.iter().map(|&index| dress[index]).collect();
        self.instances = instances(&pieces, &slots, &dress, &uploaded, &opaque);
        self.restore_models(&uploaded);
        self.atlas = show(&self.renderer, &mut self.pacer, self.atlas, atlas)?;
        self.maps = show_maps(&mut self.renderer, self.maps, maps)?;
        surfaces(&self.renderer, &dress)?;
        self.dress = dress;
        self.faced.clear();
        self.uploaded = uploaded;
        self.materials = if materials.is_empty() {
            vec![Material::default()]
        } else {
            materials
        };
        self.names = names;
        self.renderer.set_finish(lighting.finish.clone());
        self.renderer.set_lights(&lighting.lights).map_err(Error)?;
        self.renderer.set_haze(lighting.haze);
        let lighting = Arc::new(lighting);
        let asked = self.hours.busy();
        self.hours = Hours::new(lighting.clone());
        self.lighting = lighting;
        self.sky = Some(sky);
        self.bounds = bounds;
        self.receiver = receiver;
        self.seed = seed;
        self.parents = parents;
        self.text = Drawn::new(self.renderer.gpu(), text)?;
        self.glazed = shown_glass(&glass).collect();
        self.opaque = opaque;
        self.pieces = pieces;
        self.generation += 1;
        if change.sky {
            self.light()?;
        } else {
            if asked {
                self.hours.ask(self.hour);
            }
            self.shade(self.shown);
        }
        Ok(change)
    }

    pub fn gpu(&self) -> &Gpu {
        self.renderer.gpu()
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn output_format(&self) -> wgpu::TextureFormat {
        self.renderer.output_format()
    }

    pub fn materials(&self) -> &[String] {
        &self.names
    }

    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    #[cfg(test)]
    pub fn glass_count(&self) -> usize {
        self.glass.len()
    }

    pub fn atlas(&self) -> Option<([u32; 2], u64)> {
        self.atlas.map(|shown| (shown.size, shown.bytes))
    }

    pub fn look_through(&mut self, camera: Option<&Shot>) {
        self.through = camera.cloned();
    }

    pub fn mesh_stats(&self) -> MeshStats {
        self.renderer.mesh_stats()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn looked(&self) -> Option<(Matrix, Matrix)> {
        self.looked
    }

    pub fn depth(&self) -> &wgpu::TextureView {
        &self.renderer.frame().targets.depth_view
    }

    pub fn outlines(&self) -> Vec<Outline<'_>> {
        let drawn = if self.faced.len() == self.instances.len() {
            &self.faced
        } else {
            &self.instances
        };
        let outline = |at: usize, key: u64, model: Matrix, id: u32| {
            let piece = self.pieces.get(at)?;
            Some(Outline {
                key,
                positions: &piece.geometry.positions,
                indices: &piece.geometry.indices,
                model,
                id,
                name: &piece.name,
            })
        };
        let opaque = self
            .opaque
            .iter()
            .zip(drawn)
            .zip(&self.uploaded)
            .filter(|((_, instance), _)| !instance.shadow_only)
            .filter_map(|((&at, instance), uploaded)| {
                outline(at, uploaded.mark.hash, instance.model, instance.id)
            });
        let glass = self
            .glazed
            .iter()
            .zip(&self.glass)
            .zip(&self.glassed)
            .filter(|((_, surface), _)| !surface.shadow_only)
            .filter_map(|((&at, surface), glassed)| {
                outline(at, glassed.mark.hash, surface.model, surface.id)
            });
        opaque.chain(glass).collect()
    }

    pub fn preview(&mut self, object: usize, motion: Matrix) -> Result<(), Error> {
        if object >= self.parents.len() {
            return Err(Error(format!(
                "no object {object} to preview, the recipe has {}",
                self.parents.len()
            )));
        }
        let family = family(&self.parents, object);
        let posed = |id: u32, base: Matrix| {
            let owner = (id as usize).saturating_sub(1);
            if family.get(owner).copied().unwrap_or(false) {
                multiply(motion, base)
            } else {
                base
            }
        };
        for (instance, uploaded) in self.instances.iter_mut().zip(&self.uploaded) {
            let base = self
                .posed
                .get(&uploaded.mark.key.name)
                .copied()
                .unwrap_or(IDENTITY);
            instance.model = posed(instance.id, base);
        }
        for (surface, glassed) in self.glass.iter_mut().zip(&self.glassed) {
            let base = self
                .posed
                .get(&glassed.mark.key.name)
                .copied()
                .unwrap_or(IDENTITY);
            surface.model = posed(surface.id, base);
        }
        Ok(())
    }

    pub fn restore(&mut self) {
        let uploaded = std::mem::take(&mut self.uploaded);
        self.restore_models(&uploaded);
        self.uploaded = uploaded;
        for (surface, glassed) in self.glass.iter_mut().zip(&self.glassed) {
            surface.model = self
                .posed
                .get(&glassed.mark.key.name)
                .copied()
                .unwrap_or(IDENTITY);
        }
    }

    pub fn set_poses(&mut self, posed: BTreeMap<String, Matrix>) -> bool {
        if posed == self.posed {
            return false;
        }
        self.posed = posed;
        self.restore();
        true
    }

    fn restore_models(&mut self, uploaded: &[Uploaded]) {
        for (instance, uploaded) in self.instances.iter_mut().zip(uploaded) {
            instance.model = self
                .posed
                .get(&uploaded.mark.key.name)
                .copied()
                .unwrap_or(IDENTITY);
        }
    }

    pub fn moved(&self) -> Vec<usize> {
        let mut moved: Vec<usize> = self
            .instances
            .iter()
            .map(|instance| (instance.model, instance.id))
            .chain(self.glass.iter().map(|surface| (surface.model, surface.id)))
            .filter(|(model, _)| *model != IDENTITY)
            .map(|(_, id)| (id as usize).saturating_sub(1))
            .collect();
        moved.sort_unstable();
        moved.dedup();
        moved
    }

    pub fn pick(&mut self, x: u32, y: u32) -> Result<(), Error> {
        self.renderer
            .pick(x, y)
            .map_err(|e| Error(format!("pick: {e}")))
    }

    pub fn picked(&mut self) -> Option<Pick> {
        self.renderer.picked().map(|picked| Pick {
            pixel: [picked.x, picked.y],
            object: (picked.id as usize).checked_sub(1),
            depth: picked.depth,
        })
    }

    pub fn resize(&mut self, size: [u32; 2]) -> Result<(), Error> {
        if size == self.size {
            return Ok(());
        }
        self.renderer
            .resize(size[0], size[1])
            .map_err(|e| Error(format!("the live renderer: {e}")))?;
        self.size = size;
        self.previous = None;
        Ok(())
    }

    pub fn set_hour(&mut self, hour: Option<f64>) -> Result<(), Error> {
        check_hour(hour)?;
        if self.fixed_hour(hour)? {
            return Ok(());
        }
        self.hour = hour;
        self.light()
    }

    pub fn request_hour(&mut self, hour: Option<f64>) -> Result<(), Error> {
        check_hour(hour)?;
        if hour != self.hour && !self.fixed_hour(hour)? {
            self.hour = hour;
            self.hours.ask(hour);
        }
        Ok(())
    }

    fn fixed_hour(&mut self, hour: Option<f64>) -> Result<bool, Error> {
        if !self.lighting.fixed_sky()
            || self.hours.busy()
            || !self.waiting.is_empty()
            || self.renderer.sky_settling()
        {
            return Ok(false);
        }
        self.renderer
            .set_hour(self.lighting.shine(hour).hour as f32)
            .map_err(|e| Error(format!("the sky: {e}")))?;
        self.hour = hour;
        self.shown = hour;
        self.shade(hour);
        Ok(true)
    }

    pub fn settling(&self) -> bool {
        self.hours.busy() || !self.waiting.is_empty() || self.renderer.sky_settling()
    }

    #[cfg(test)]
    pub fn settle_sky(&mut self) -> Result<(), Error> {
        if let Some((hour, lit)) = self.hours.wait() {
            self.ask_sky(hour, lit)?;
        }
        self.renderer
            .wait_sky()
            .map_err(|e| Error(format!("the sky: {e}")))?;
        self.sky_shown();
        Ok(())
    }

    pub fn frame_ms(&self) -> Option<f64> {
        self.renderer
            .timings()
            .filter(|timings| !timings.passes.is_empty())
            .map(|timings| timings.passes.iter().map(|t| t.milliseconds).sum())
    }

    pub fn timing_drops(&self) -> u64 {
        let drops = self.renderer.timing_drops();
        drops.ring + drops.late
    }

    pub fn submit(
        &mut self,
        eye: &Eye,
        time: f32,
        output: &wgpu::TextureView,
    ) -> Result<(), Error> {
        if let Some((hour, lit)) = self.hours.take() {
            self.ask_sky(hour, lit)?;
        }
        self.frame(eye, time, output, Renderer::submit)?;
        self.sky_shown();
        if self.renderer.frames_in_flight() > IN_FLIGHT {
            self.renderer
                .wait_frames(IN_FLIGHT)
                .map_err(|e| Error(format!("the live renderer: {e}")))?;
        }
        Ok(())
    }

    pub fn render(
        &mut self,
        eye: &Eye,
        time: f32,
        output: &wgpu::TextureView,
    ) -> Result<(), Error> {
        self.render_measured(eye, time, output).map(|_| ())
    }

    pub fn render_measured(
        &mut self,
        eye: &Eye,
        time: f32,
        output: &wgpu::TextureView,
    ) -> Result<Option<f64>, Error> {
        let timings = self.frame(eye, time, output, Renderer::render)?;
        Ok((!timings.is_empty()).then(|| timings.iter().map(|t| t.milliseconds).sum()))
    }

    fn frame<T>(
        &mut self,
        eye: &Eye,
        time: f32,
        output: &wgpu::TextureView,
        pass: impl FnOnce(
            &mut Renderer,
            &Scene<'_>,
            &Text<'_>,
            &Effects<'_>,
            Finish,
            &wgpu::TextureView,
        ) -> Result<T, String>,
    ) -> Result<T, Error> {
        let (camera, current) = self.camera(eye);
        let aspect = f64::from(self.size[0].max(1)) / f64::from(self.size[1].max(1));
        self.renderer.set_lens(
            self.through
                .as_ref()
                .and_then(|shot| eye.lens(shot, aspect)),
        );
        self.face(eye);
        let drawn = if self.faced.is_empty() {
            &self.instances
        } else {
            &self.faced
        };
        let scene = scene(camera, time, (self.seed, self.sun), drawn, &self.materials);
        let done = pass(
            &mut self.renderer,
            &scene,
            &self.text.text(current, self.size),
            &Effects {
                glass: &self.glass,
                ..Effects::default()
            },
            Finish::Standard,
            output,
        )
        .map_err(|e| Error(format!("the live renderer: {e}")))?;
        self.drawn();
        Ok(done)
    }

    fn camera(&mut self, eye: &Eye) -> (Camera, Matrix) {
        let view = eye.view();
        let (near, far) = eye.depth_range(self.bounds);
        let projection = eye.projection(self.size, near, far);
        let current = multiply(projection, view);
        let camera = Camera {
            view,
            projection,
            previous_view_projection: self.previous.unwrap_or(current),
            position: eye.position.map(|v| v as f32),
        };
        self.previous = Some(current);
        self.looked = Some((view, projection));
        (camera, current)
    }

    fn face(&mut self, eye: &Eye) {
        if self.dress.iter().all(|worn| worn.face().is_none()) {
            self.faced.clear();
            return;
        }
        let fresh = self.faced.len() != self.instances.len();
        if fresh {
            self.faced.clone_from(&self.instances);
        }
        for ((drawn, instance), worn) in self.faced.iter_mut().zip(&self.instances).zip(&self.dress)
        {
            let previous = drawn.model;
            *drawn = *instance;
            if let Some(face) = worn.face() {
                drawn.model = multiply(instance.model, cards::facing(&face, eye));
                drawn.previous_model = if fresh { drawn.model } else { previous };
            }
        }
    }

    fn drawn(&mut self) {
        for instance in &mut self.instances {
            instance.previous_model = instance.model;
        }
    }

    fn ask_sky(&mut self, hour: Option<f64>, lit: crate::light::Lit) -> Result<(), Error> {
        let request = self
            .renderer
            .request_sky(lit.sky, lit.hour)
            .map_err(|e| Error(format!("the sky: {e}")))?;
        self.waiting.push((request, hour));
        Ok(())
    }

    fn sky_shown(&mut self) {
        let shown = self.renderer.frame().sky().shown();
        let Some(&(_, hour)) = self
            .waiting
            .iter()
            .rev()
            .find(|(request, _)| *request <= shown)
        else {
            return;
        };
        self.waiting.retain(|(request, _)| *request > shown);
        self.shown = hour;
        self.shade(hour);
    }

    fn shade(&mut self, hour: Option<f64>) {
        let lit = self.lighting.shine(hour);
        self.sun = lit.sun;
        let [min, max] = self.receiver.map(|corner| corner.map(|v| v as f32));
        let quality = Quality {
            receiver: Some(ReceiverBox { min, max }),
            caster_margin: CASTER_MARGIN,
            sun_radius_deg: lit.radius_deg,
            ..self.renderer.shadows().quality()
        };
        self.renderer.set_shadow_quality(quality);
        self.previous = None;
    }

    fn light(&mut self) -> Result<(), Error> {
        self.waiting.clear();
        self.shown = self.hour;
        self.shade(self.hour);
        let lit = self.lighting.at(self.hour);
        self.renderer
            .set_sky(lit.sky, lit.hour)
            .map_err(|e| Error(format!("the sky: {e}")))
    }
}

fn scene<'a>(
    camera: Camera,
    time: f32,
    (seed, sun): (u32, Sun),
    instances: &'a [Instance],
    materials: &'a [Material],
) -> Scene<'a> {
    Scene {
        camera,
        time,
        seed,
        sun,
        instances,
        materials,
        deformers: &[],
        wind: SceneWind::default(),
    }
}

fn check_hour(hour: Option<f64>) -> Result<(), Error> {
    match hour {
        Some(hour) if !(0.0..=24.0).contains(&hour) => {
            Err(Error(format!("the hour is {hour}, expected 0 to 24")))
        }
        _ => Ok(()),
    }
}

fn refused(error: &RendererError) -> String {
    match error {
        RendererError::Refused(refusal) => format!(
            "this GPU is below the engine's floor, so the live renderer cannot start: {refusal}"
        ),
        RendererError::Setup(message) => {
            format!("the live renderer could not start: {message}")
        }
    }
}

fn data(piece: &Piece) -> MeshData<'_> {
    let geometry = &piece.geometry;
    MeshData {
        positions: &geometry.positions,
        normals: &geometry.normals,
        tangents: &geometry.tangents,
        uvs: &geometry.uvs,
        uvs1: None,
        alpha: None,
        indices: &geometry.indices,
    }
}

fn show(
    renderer: &Renderer,
    pacer: &mut Pacer,
    shown: Option<Shown>,
    atlas: Option<Atlas>,
) -> Result<Option<Shown>, Error> {
    match atlas {
        Some(atlas) if shown.is_some_and(|shown| shown.key == atlas.key) => Ok(shown),
        Some(mut atlas) => {
            if paced(atlas.size) {
                show_paced(renderer.frame(), pacer, &mut atlas)?;
            } else {
                atlas.show(renderer.frame())?;
            }
            Ok(Some(atlas.shown()))
        }
        None => {
            if shown.is_some() {
                renderer.frame().release_content(cards::SLOT);
            }
            Ok(None)
        }
    }
}

fn show_maps(
    renderer: &mut Renderer,
    shown: Option<u64>,
    set: Option<maps::Set>,
) -> Result<Option<u64>, Error> {
    match set {
        Some(set) if shown == Some(set.key) => Ok(shown),
        Some(set) => {
            let [base, normal, roughness, metal] = set.images()?;
            renderer
                .set_maps(&MapImages {
                    base: &base,
                    normal: &normal,
                    roughness: &roughness,
                    metal: &metal,
                })
                .map_err(|e| Error(format!("texture maps: {e}")))?;
            Ok(Some(set.key))
        }
        None => {
            if shown.is_some() {
                renderer
                    .set_maps(&MapImages::default())
                    .map_err(|e| Error(format!("texture maps: {e}")))?;
            }
            Ok(None)
        }
    }
}

fn paced(size: [u32; 2]) -> bool {
    u64::from(size[0]) * u64::from(size[1]) >= PACED_TEXELS
}

fn show_paced(frame: &Frame, pacer: &mut Pacer, atlas: &mut Atlas) -> Result<Stats, Error> {
    let mut turns = TurnState::default();
    let packed = atlas.ensure()?;
    let (_, stats) = packed
        .allocate(frame, cards::SLOT)
        .and_then(|()| {
            packed.upload_paced(frame, cards::SLOT, pacer, |ms| turn_soon(&mut turns, ms))
        })
        .map_err(|e| Error(format!("card images: {e}")))?;
    if turns.pending_ms() > 0.0 {
        turn_now(&mut turns);
    }
    Ok(stats)
}

fn surfaces(renderer: &Renderer, dress: &[Dress]) -> Result<(), Error> {
    let surfaces: Vec<_> = dress.iter().map(|worn| worn.surface).collect();
    renderer
        .frame()
        .set_surfaces(&surfaces)
        .map_err(|e| Error(format!("the live renderer: {e}")))
}

fn upload(renderer: &mut Renderer, piece: &Piece) -> Result<MeshHandle, Error> {
    renderer
        .upload_mesh(data(piece))
        .map_err(|e| Error(format!("object {}: {e}", piece.name)))
}

fn instances(
    pieces: &[Piece],
    slots: &[u32],
    dress: &[Dress],
    uploaded: &[Uploaded],
    at: &[usize],
) -> Vec<Instance> {
    at.iter()
        .zip(dress)
        .zip(uploaded)
        .map(|((&index, worn), uploaded)| {
            let piece = &pieces[index];
            let mut instance = Instance::new(
                uploaded.handle,
                IDENTITY,
                slots[index],
                piece.object as u32 + 1,
            );
            if let Some(card) = worn.card {
                instance.alpha_cutoff = card.cutoff;
            }
            instance.shadow_only = piece.shadow == Shadow::Only;
            instance.casts_shadow = piece.shadow != Shadow::None;
            instance.two_sided = piece.two_sided;
            instance
        })
        .collect()
}

fn picked(glass: &[bool], glassy: bool) -> impl Iterator<Item = usize> + '_ {
    glass
        .iter()
        .enumerate()
        .filter(move |(_, glass)| **glass == glassy)
        .map(|(at, _)| at)
}

fn opaque_at(glass: &[bool]) -> Vec<usize> {
    picked(glass, false).collect()
}

fn shown_glass(glass: &[bool]) -> impl Iterator<Item = usize> + '_ {
    picked(glass, true)
}

fn glaze(
    renderer: &mut Renderer,
    pieces: &[Piece],
    marks: &[Mark],
    glass: &[bool],
    before: Vec<Glassed>,
) -> Result<Vec<Glassed>, Error> {
    let mut kept: BTreeMap<Key, Glassed> = before
        .into_iter()
        .map(|glassed| (glassed.mark.key.clone(), glassed))
        .collect();
    let mut out = Vec::new();
    for at in shown_glass(glass) {
        let mark = &marks[at];
        let refused = |e: String| Error(format!("object {}: glass: {e}", pieces[at].name));
        let handle = match kept.remove(&mark.key) {
            Some(old) if old.mark.hash == mark.hash => old.handle,
            Some(old) => {
                renderer
                    .replace_glass(old.handle, &pieces[at].geometry)
                    .map_err(refused)?;
                old.handle
            }
            None => renderer
                .upload_glass(&pieces[at].geometry)
                .map_err(refused)?,
        };
        out.push(Glassed {
            mark: mark.clone(),
            handle,
        });
    }
    for gone in kept.into_values() {
        renderer
            .release_glass(gone.handle)
            .map_err(|e| Error(format!("object {}: glass: {e}", gone.mark.key.name)))?;
    }
    Ok(out)
}

fn glass_surfaces(
    pieces: &[Piece],
    slots: &[u32],
    materials: &[Material],
    glass: &[bool],
    clips: &[[[f32; 4]; 2]],
    glassed: &[Glassed],
) -> Vec<glass::Surface> {
    shown_glass(glass)
        .zip(glassed)
        .map(|(at, glassed)| glass::Surface {
            mesh: glassed.handle,
            model: IDENTITY,
            material: materials[slots[at] as usize],
            liquid: false,
            fluid_height: 0.0,
            ripple_height: 0.0,
            caustic_strength: 0.0,
            tinted: None,
            casts_shadow: pieces[at].shadow != Shadow::None,
            shadow_only: pieces[at].shadow == Shadow::Only,
            clip: clips[at],
            id: pieces[at].object as u32 + 1,
        })
        .collect()
}

fn parents(recipe: &Recipe) -> Vec<Option<usize>> {
    recipe
        .objects
        .iter()
        .map(|object| {
            object
                .parent
                .as_deref()
                .and_then(|name| recipe.objects.iter().position(|o| o.name == name))
        })
        .collect()
}

pub fn family_of(recipe: &Recipe, object: usize) -> Vec<bool> {
    family(&parents(recipe), object)
}

fn family(parents: &[Option<usize>], object: usize) -> Vec<bool> {
    (0..parents.len())
        .map(|start| {
            let mut at = Some(start);
            for _ in 0..=parents.len() {
                match at {
                    Some(here) if here == object => return true,
                    Some(here) => at = parents[here],
                    None => return false,
                }
            }
            false
        })
        .collect()
}

fn marks(pieces: &[Piece]) -> Vec<Mark> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    pieces
        .iter()
        .map(|piece| {
            let ordinal = seen.entry(piece.name.as_str()).or_insert(0);
            let key = Key {
                name: piece.name.clone(),
                ordinal: *ordinal,
            };
            *ordinal += 1;
            Mark {
                key,
                hash: hash(piece),
            }
        })
        .collect()
}

fn floats(hasher: &mut DefaultHasher, values: &[f32]) {
    let mut run = [0u8; 4096];
    for chunk in values.chunks(run.len() / 4) {
        for (bytes, value) in run.chunks_exact_mut(4).zip(chunk) {
            bytes.copy_from_slice(&value.to_le_bytes());
        }
        hasher.write(&run[..chunk.len() * 4]);
    }
}

fn hash(piece: &Piece) -> u64 {
    let geometry = &piece.geometry;
    let mut hasher = DefaultHasher::new();
    hasher.write_usize(geometry.positions.len());
    floats(&mut hasher, geometry.positions.as_flattened());
    hasher.write_usize(geometry.normals.len());
    floats(&mut hasher, geometry.normals.as_flattened());
    hasher.write_usize(geometry.tangents.len());
    floats(&mut hasher, geometry.tangents.as_flattened());
    hasher.write_usize(geometry.uvs.len());
    floats(&mut hasher, geometry.uvs.as_flattened());
    geometry.indices.hash(&mut hasher);
    hasher.finish()
}

fn plan<'a>(before: impl IntoIterator<Item = &'a Mark>, after: &[Mark]) -> Plan {
    let mut index: BTreeMap<&Key, (usize, u64)> = before
        .into_iter()
        .enumerate()
        .map(|(at, mark)| (&mark.key, (at, mark.hash)))
        .collect();
    let actions = after
        .iter()
        .map(|mark| match index.remove(&mark.key) {
            Some((at, hash)) if hash == mark.hash => Action::Keep(at),
            Some((at, _)) => Action::Replace(at),
            None => Action::Add,
        })
        .collect();
    let mut released: Vec<usize> = index.into_values().map(|(at, _)| at).collect();
    released.sort_unstable();
    Plan { actions, released }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_recipe::parse;
    use std::path::Path;

    const BASE: &str = "manfred = 1\nname = \"diff\"\n\n[materials.glass]\nfamily = \"glass\"\nbase = [0.9, 0.95, 1.0]\nroughness = 0.02\ntransmission = 1.0\nior = 1.5\nthickness = 0.004\n\n[materials.orange]\nfamily = \"lacquer\"\nbase = [0.73, 0.06, 0.0]\nroughness = 0.45\nclearcoat = 1.0\nclearcoat_roughness = 0.07\n\n[[camera]]\nname = \"main\"\nposition = [0.0, 1.0, 6.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 40.0\n\n[[object]]\nname = \"a\"\nkind = \"box\"\nmaterial = \"orange\"\nsize = [1.0, 1.0, 1.0]\n\n[[object]]\nname = \"b\"\nkind = \"box\"\nmaterial = \"orange\"\nsize = [0.5, 0.5, 0.5]\nat = [2.0, 0.0, 0.0]\n\n[[object]]\nname = \"c\"\nkind = \"box\"\nmaterial = \"orange\"\nsize = [0.25, 0.25, 0.25]\nat = [-2.0, 0.0, 0.0]\n";

    const ROOM: &str = "[sky.room]\nwidth = 64\nfloor = [0.22, 0.135, 0.068]\nwall = [0.19, 0.13, 0.07]\nceiling = [0.28, 0.23, 0.16]\n[[sky.room.lights]]\nname = \"window\"\naz = -64.0\nel = 14.0\nwidth = 37.5\nheight = 29.0\npower = 10.5\ncolor = [1.0, 0.86, 0.66]\nsoft = 0.05\nslats = 6.0\nopen = 0.7\n";

    fn texels(prepared: &Prepared) -> Vec<[f32; 4]> {
        match prepared.lighting.at(None).sky {
            pfx_live::sky::SkySource::Hdr(sky) => sky.texels,
            pfx_live::sky::SkySource::Analytic(_) => panic!("a room sky is texels"),
        }
    }

    fn kept(memory: &Memory, text: &str) -> Prepared {
        let recipe = parse(text).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        Stage::prepare_kept(&recipe, &built, memory).unwrap()
    }

    #[test]
    fn a_rebuild_with_the_same_sky_does_not_load_it_again() {
        let memory = Memory::default();
        let text = format!("{BASE}\n[sky]\nkind = \"room\"\nintensity = 1.5\n{ROOM}");
        let first = kept(&memory, &text);
        assert_eq!(memory.sky_loads(), 1);
        let moved = text.replace("at = [2.0, 0.0, 0.0]", "at = [2.5, 0.0, 0.0]");
        let second = kept(&memory, &moved);
        assert_eq!(memory.sky_loads(), 1);
        assert_eq!(first.sky, second.sky);
        assert!(texels(&first) == texels(&second));
        let lit = format!(
            "{moved}[sun]\nmodel = \"authored\"\ntoward = [0.0, 1.0, 0.0]\nirradiance = 2.0\n"
        );
        kept(&memory, &lit);
        assert_eq!(memory.sky_loads(), 1);
        let brighter = moved.replace("intensity = 1.5", "intensity = 2.5");
        let third = kept(&memory, &brighter);
        assert_eq!(memory.sky_loads(), 2);
        assert!(texels(&second) != texels(&third));
        kept(&memory, &brighter);
        assert_eq!(memory.sky_loads(), 2);
    }

    fn flat_hdr(value: u8) -> Vec<u8> {
        let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 4 +X 8\n".to_vec();
        for texel in 0..32u8 {
            bytes.extend_from_slice(&[value.wrapping_add(texel), 100, 60, 129]);
        }
        bytes
    }

    #[test]
    fn a_changed_hdr_under_the_same_name_is_read_again_and_an_unchanged_one_is_not() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp/stage-unit/hdr");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sky.hdr");
        std::fs::write(&file, flat_hdr(10)).unwrap();
        let memory = Memory::default();
        let text = format!(
            "{BASE}\n[sky]\nkind = \"hdr\"\npath = {:?}\n",
            file.display().to_string()
        );
        let first = kept(&memory, &text);
        assert_eq!(memory.sky_loads(), 1);
        let moved = text.replace("at = [2.0, 0.0, 0.0]", "at = [2.5, 0.0, 0.0]");
        let second = kept(&memory, &moved);
        assert_eq!(memory.sky_loads(), 1);
        assert!(texels(&first) == texels(&second));
        std::fs::write(&file, flat_hdr(90)).unwrap();
        let third = kept(&memory, &moved);
        assert_eq!(memory.sky_loads(), 2);
        assert!(texels(&second) != texels(&third));
        kept(&memory, &moved);
        assert_eq!(memory.sky_loads(), 2);
    }

    #[test]
    fn a_failed_sky_is_tried_again() {
        let memory = Memory::default();
        let text = format!("{BASE}\n[sky]\nkind = \"hdr\"\npath = \"nowhere.hdr\"\n");
        let recipe = parse(&text).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        assert!(Stage::prepare_kept(&recipe, &built, &memory).is_err());
        assert!(Stage::prepare_kept(&recipe, &built, &memory).is_err());
        assert_eq!(memory.sky_loads(), 2);
    }

    #[test]
    fn a_kept_prepare_takes_the_generators_materials() {
        let text = BASE.replace("\"orange\"", "\"made\"");
        let recipe = parse(&text).unwrap();
        let mut built = manfred_build::build(&recipe, Path::new("")).unwrap();
        assert!(Stage::prepare_kept(&recipe, &built, &Memory::default()).is_err());
        built.materials.insert(
            "made".into(),
            serde_json::json!({ "base": [0.2, 0.4, 0.6], "roughness": 0.5 }),
        );
        let prepared = Stage::prepare_kept(&recipe, &built, &Memory::default()).unwrap();
        assert_eq!(prepared.names, ["made"]);
        assert_eq!(prepared.materials[0].base, [0.2, 0.4, 0.6]);
    }

    #[test]
    fn a_one_number_edit_on_the_toybox_prepares_without_the_sky() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/toybox/toybox.toml");
        let hdr = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sky/window.hdr");
        let text = std::fs::read_to_string(&path).unwrap().replace(
            "[sky]\nkind = \"analytic\"\nintensity = 0.9\n",
            &format!(
                "[sky]\nkind = \"hdr\"\npath = {:?}\nsource = \"Manfred's own test sky\"\nintensity = 0.9\nprepare.cap = 60.0\nprepare.mean = 0.3\n",
                hdr.display().to_string()
            ),
        );
        let recipe = parse(&text).unwrap();
        assert_eq!(
            recipe.sky.as_ref().unwrap().kind,
            manfred_recipe::SkyKind::Hdr
        );
        let dir = path.parent().unwrap();
        let built = manfred_build::build(&recipe, dir).unwrap();
        let memory = Memory::default();
        let since = std::time::Instant::now();
        Stage::prepare_kept(&recipe, &built, &memory).unwrap();
        let first = since.elapsed();
        let mut moved = recipe.clone();
        moved.objects[0].at[0] += 0.01;
        let built = manfred_build::build(&moved, dir).unwrap();
        let since = std::time::Instant::now();
        Stage::prepare_kept(&moved, &built, &memory).unwrap();
        let kept = since.elapsed();
        let since = std::time::Instant::now();
        Stage::prepare(&moved, &built).unwrap();
        let fresh = since.elapsed();
        assert_eq!(memory.sky_loads(), 1);
        println!(
            "the toybox's prepare: first {} ms, a one-number edit {} ms with the sky kept, {} ms with it loaded again",
            first.as_millis(),
            kept.as_millis(),
            fresh.as_millis()
        );
    }

    fn marked(text: &str) -> Vec<Mark> {
        let recipe = parse(text).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        marks(&parts::pieces(&built.parts))
    }

    fn counts(plan: &Plan) -> [usize; 4] {
        let count = |wanted: fn(&Action) -> bool| plan.actions.iter().filter(|a| wanted(a)).count();
        [
            count(|a| matches!(a, Action::Keep(_))),
            count(|a| matches!(a, Action::Replace(_))),
            count(|a| matches!(a, Action::Add)),
            plan.released.len(),
        ]
    }

    #[test]
    fn the_same_recipe_keeps_every_mesh() {
        let before = marked(BASE);
        assert_eq!(before.len(), 3);
        let plan = plan(&before, &marked(BASE));
        assert_eq!(
            plan.actions,
            [Action::Keep(0), Action::Keep(1), Action::Keep(2)]
        );
        assert!(plan.released.is_empty());
    }

    #[test]
    fn a_recipe_edited_four_ways_keeps_replaces_adds_and_releases() {
        let before = marked(BASE);
        let moved = BASE.replace("at = [2.0, 0.0, 0.0]", "at = [2.5, 0.0, 0.0]");
        assert_eq!(counts(&plan(&before, &marked(&moved))), [2, 1, 0, 0]);
        let resized = BASE.replace("size = [1.0, 1.0, 1.0]", "size = [1.0, 2.0, 1.0]");
        let resized = plan(&before, &marked(&resized));
        assert_eq!(resized.actions[0], Action::Replace(0));
        assert_eq!(counts(&resized), [2, 1, 0, 0]);
        let added = format!(
            "{BASE}\n[[object]]\nname = \"d\"\nkind = \"box\"\nmaterial = \"orange\"\nsize = [1.0, 1.0, 1.0]\nat = [0.0, 3.0, 0.0]\n"
        );
        let added = plan(&before, &marked(&added));
        assert_eq!(counts(&added), [3, 0, 1, 0]);
        assert_eq!(added.actions[3], Action::Add);
        let cut = BASE.find("\n[[object]]\nname = \"b\"").unwrap();
        let tail = BASE.find("\n[[object]]\nname = \"c\"").unwrap();
        let removed = format!("{}{}", &BASE[..cut], &BASE[tail..]);
        let removed = plan(&before, &marked(&removed));
        assert_eq!(removed.actions, [Action::Keep(0), Action::Keep(2)]);
        assert_eq!(removed.released, [1]);
    }

    #[test]
    fn a_renamed_object_is_released_and_added() {
        let before = marked(BASE);
        let renamed = BASE.replace("name = \"c\"", "name = \"e\"");
        let plan = plan(&before, &marked(&renamed));
        assert_eq!(counts(&plan), [2, 0, 1, 1]);
        assert_eq!(plan.released, [2]);
    }

    #[test]
    fn the_mesh_hash_is_stable_and_sees_every_attribute() {
        let first = marked(BASE);
        assert_eq!(first, marked(BASE));
        let hashes: std::collections::BTreeSet<u64> = first.iter().map(|m| m.hash).collect();
        assert_eq!(hashes.len(), 3);
    }

    #[test]
    fn a_family_is_the_object_and_everything_below_it() {
        let parents = [None, Some(0), Some(1), None, Some(3), Some(0)];
        assert_eq!(family(&parents, 0), [true, true, true, false, false, true]);
        assert_eq!(
            family(&parents, 1),
            [false, true, true, false, false, false]
        );
        assert_eq!(
            family(&parents, 4),
            [false, false, false, false, true, false]
        );
        let looped = [Some(1), Some(0), None];
        assert_eq!(family(&looped, 2), [false, false, true]);
    }

    #[test]
    fn parents_are_found_by_name() {
        let text = format!("{BASE}parent = \"a\"\n");
        let recipe = parse(&text).unwrap();
        assert_eq!(parents(&recipe), [None, None, Some(0)]);
    }

    #[test]
    fn a_stage_and_its_prepared_parts_cross_threads() {
        fn send<T: Send>() {}
        send::<Stage>();
        send::<Prepared>();
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn moving_one_object_replaces_exactly_one_mesh() {
        let recipe = parse(BASE).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        let mut stage =
            Stage::new(crate::headless().unwrap(), &recipe, &built, [320, 200]).unwrap();
        let before = stage.mesh_stats();
        assert_eq!(before.live, 3);
        let moved = parse(&BASE.replace("at = [2.0, 0.0, 0.0]", "at = [2.5, 0.0, 0.0]")).unwrap();
        let rebuilt = manfred_build::build(&moved, Path::new("")).unwrap();
        let change = stage.update(&moved, &rebuilt).unwrap();
        assert_eq!(
            change,
            Change {
                kept: 2,
                replaced: 1,
                added: 0,
                released: 0,
                sky: false,
            }
        );
        assert_eq!(stage.mesh_stats(), before);
        assert_eq!(stage.instance_count(), 3);
        pfx::adapter::turn();
    }

    const PANE: &str = "\n[[object]]\nname = \"pane\"\nkind = \"box\"\nmaterial = \"glass\"\nsize = [0.5, 0.5, 0.02]\nat = [0.0, 0.0, 1.0]\n";

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_hundred_alternating_edits_leave_the_meshes_and_the_glass_flat() {
        let edits = [
            format!("{BASE}{PANE}"),
            format!(
                "{}{}",
                BASE.replace("at = [2.0, 0.0, 0.0]", "at = [2.5, 0.0, 0.0]"),
                PANE.replace("at = [0.0, 0.0, 1.0]", "at = [0.0, 0.2, 1.0]")
            ),
            format!(
                "{BASE}{PANE}\n[[object]]\nname = \"d\"\nkind = \"box\"\nmaterial = \"orange\"\nsize = [1.0, 1.0, 1.0]\nat = [0.0, 3.0, 0.0]\n{}",
                PANE.replace("\"pane\"", "\"pane2\"")
                    .replace("at = [0.0, 0.0, 1.0]", "at = [1.0, 0.0, 1.0]")
            ),
            BASE.replace("size = [1.0, 1.0, 1.0]", "size = [1.0, 2.0, 1.0]"),
        ];
        let staged: Vec<(Recipe, Built)> = edits
            .iter()
            .map(|text| {
                let recipe = parse(text).unwrap();
                let built = manfred_build::build(&recipe, Path::new("")).unwrap();
                (recipe, built)
            })
            .collect();
        let gpu = crate::headless().unwrap();
        let mut stage = Stage::new(gpu.clone(), &staged[0].0, &staged[0].1, [320, 200]).unwrap();
        let first = stage.mesh_stats();
        assert_eq!((first.live, first.glass_live), (3, 1), "{first:?}");
        assert!(first.buffers >= 3 && first.glass_buffers >= 1, "{first:?}");
        let eye = Eye::from_camera(crate::camera(&staged[0].0, "main").unwrap(), 1.6);
        let target = gpu.offscreen(320, 200, stage.output_format()).unwrap();
        let mut seen = Vec::new();
        for edit in 1..=100 {
            let (recipe, built) = &staged[edit % staged.len()];
            stage.update(recipe, built).unwrap();
            stage.render(&eye, 0.0, &target.view).unwrap();
            seen.push((stage.mesh_stats(), stage.glass_count()));
            if edit % 10 == 0 {
                pfx::adapter::turn();
            }
        }
        let settled = seen[staged.len() - 1].0;
        println!("mesh_stats after a hundred edits: {:?}", seen[99].0);
        for (edit, (stats, glass)) in seen.iter().enumerate() {
            let expected = [(3, 1), (4, 2), (3, 0), (3, 1)][edit % staged.len()];
            assert_eq!(
                (stats.live, stats.glass_live),
                expected,
                "{edit}: {stats:?}"
            );
            assert_eq!(*glass, expected.1, "{edit}");
        }
        assert!(seen[staged.len()..].iter().all(|(stats, _)| {
            stats.slots == settled.slots && stats.glass_slots == settled.glass_slots
        }));
        assert_eq!((settled.slots, settled.glass_slots), (4, 2), "{settled:?}");
        let per_mesh = first.buffers / first.live;
        let per_glass = first.glass_buffers / first.glass_live;
        assert_eq!(first.buffers, per_mesh * first.live, "{first:?}");
        assert_eq!(
            first.glass_buffers,
            per_glass * first.glass_live,
            "{first:?}"
        );
        assert!(
            seen.iter()
                .all(|(stats, _)| stats.buffers == per_mesh * stats.live
                    && stats.glass_buffers == per_glass * stats.glass_live),
            "{seen:?}"
        );
        assert_eq!(seen[99].0.buffers, first.buffers);
        assert_eq!(seen[99].0.glass_buffers, first.glass_buffers);
        pfx::adapter::turn();
    }

    #[test]
    fn an_atlas_of_eight_thousand_texels_a_side_or_more_uploads_paced() {
        assert!(!paced([256, 256]));
        assert!(!paced([4096, 4096]));
        assert!(!paced([8192, 4096]));
        assert!(paced([8192, 8192]));
        assert!(paced([16384, 16384]));
    }

    fn cards_atlas(count: usize) -> Atlas {
        let pictures: Vec<cards::Picture> = (0..count)
            .map(|card| cards::Picture {
                width: 512,
                height: 512,
                texels: (0..512 * 512)
                    .flat_map(|texel| [(texel % 251) as u8, card as u8, (texel / 512) as u8, 255])
                    .collect(),
            })
            .collect();
        let (atlas, _) = cards::pack(&pictures).unwrap();
        Atlas::packed(atlas, count as u64)
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_large_atlas_uploads_in_short_submissions_and_a_small_one_in_one() {
        let (recipe, built) = table();
        let mut stage = Stage::new(crate::headless().unwrap(), &recipe, &built, [64, 64]).unwrap();
        let gpu = stage.gpu().clone();
        let timer = pfx_gpu::pace::SliceTimer::new(&gpu.device, &gpu.queue);
        for count in [1, 4, 16] {
            let mut atlas = cards_atlas(count);
            let size = atlas.size;
            assert!(!paced(size));
            let mut begin = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            if let Some(timer) = &timer {
                timer.begin(&mut begin);
            }
            gpu.queue.submit(Some(begin.finish()));
            let started = std::time::Instant::now();
            atlas.show(stage.renderer.frame()).unwrap();
            let cpu = started.elapsed().as_secs_f64() * 1000.0;
            let mut end = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            if let Some(timer) = &timer {
                timer.end(&mut end);
            }
            let index = gpu.queue.submit(Some(end.finish()));
            let ms = timer
                .as_ref()
                .and_then(|timer| timer.read_ms(&gpu.device, index));
            println!(
                "{count} cards of 512x512 in a {}x{} atlas, one submission: {ms:?} ms between the stamps, {cpu:.2} ms of it the CPU's cell writes",
                size[0], size[1]
            );
            stage.renderer.frame().release_content(cards::SLOT);
            pfx::adapter::turn();
        }
        let mut atlas = cards_atlas(64);
        let size = atlas.size;
        assert!(paced(size), "{size:?}");
        let stats = show_paced(stage.renderer.frame(), &mut stage.pacer, &mut atlas).unwrap();
        println!(
            "64 cards of 512x512 in a {}x{} atlas, paced: {} submissions, longest {:.3} ms, median {:.3} ms, {:.1} ms in all, {:?} timing",
            size[0],
            size[1],
            stats.count(),
            stats.longest_ms(),
            stats.median_ms(),
            stats.milliseconds.iter().sum::<f64>(),
            stats.source()
        );
        assert!(stats.count() > 1);
        assert!(stats.longest_ms() < 50.0, "{:.3} ms", stats.longest_ms());
        stage.renderer.frame().release_content(cards::SLOT);
        pfx::adapter::turn();
    }

    fn evening(stage: &mut Stage, eye: &Eye, live: bool) -> Vec<u8> {
        let size = stage.size();
        let target = stage
            .gpu()
            .offscreen(size[0], size[1], stage.output_format())
            .unwrap();
        if live {
            stage.request_hour(Some(19.0)).unwrap();
            assert!(stage.settling());
            stage.settle_sky().unwrap();
        } else {
            stage.set_hour(Some(19.0)).unwrap();
        }
        let mut turns = pfx::adapter::TurnState::default();
        for frame in 0..crate::SETTLE_FRAMES {
            let measured = stage
                .render_measured(eye, frame as f32 / crate::FRAME_RATE, &target.view)
                .unwrap();
            pfx::adapter::turn_soon(&mut turns, measured.unwrap_or(crate::UNMEASURED_MS));
        }
        assert!(!stage.settling());
        stage.gpu().readback_rgba8(&target).unwrap()
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_requested_hour_draws_as_the_settled_one() {
        let (recipe, built) = table();
        let size = [480, 300];
        let eye = Eye::from_camera(crate::camera(&recipe, "main").unwrap(), 1.6);
        let gpu = crate::headless().unwrap();
        let mut settled = Stage::new(gpu.clone(), &recipe, &built, size).unwrap();
        let blocking = evening(&mut settled, &eye, false);
        let mut requested = Stage::new(gpu, &recipe, &built, size).unwrap();
        let live = evening(&mut requested, &eye, true);
        let psnr = crate::psnr(&blocking, &live).unwrap();
        println!("a requested hour against the settled one: {psnr:.2} dB");
        assert!(blocking == live, "{psnr:.2} dB");
        pfx::adapter::turn();
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn submit_leaves_the_calling_thread_before_the_gpu_is_done() {
        let (recipe, built) = table();
        let size = [1586, 992];
        let eye = Eye::from_camera(crate::camera(&recipe, "main").unwrap(), 1.6);
        let mut stage = Stage::new(crate::headless().unwrap(), &recipe, &built, size).unwrap();
        let target = stage
            .gpu()
            .offscreen(size[0], size[1], stage.output_format())
            .unwrap();
        let frames = 60;
        let mut turns = pfx::adapter::TurnState::default();
        let mut blocking = std::time::Duration::ZERO;
        for frame in 0..frames {
            let started = std::time::Instant::now();
            let measured = stage
                .render_measured(&eye, frame as f32 / crate::FRAME_RATE, &target.view)
                .unwrap();
            blocking += started.elapsed();
            pfx::adapter::turn_soon(&mut turns, measured.unwrap_or(crate::UNMEASURED_MS));
        }
        let mut submitting = std::time::Duration::ZERO;
        let mut gpu_ms = Vec::new();
        for frame in 0..frames {
            let started = std::time::Instant::now();
            stage
                .submit(&eye, frame as f32 / crate::FRAME_RATE, &target.view)
                .unwrap();
            submitting += started.elapsed();
            let ms = stage.frame_ms();
            gpu_ms.extend(ms);
            pfx::adapter::turn_soon(&mut turns, ms.unwrap_or(crate::UNMEASURED_MS));
        }
        let per = |total: std::time::Duration| total.as_secs_f64() * 1000.0 / f64::from(frames);
        println!(
            "the table at {}x{}: render blocks the calling thread {:.2} ms a frame, submit {:.2} ms; timings arrived for {} of {frames} submitted frames, {} dropped",
            size[0],
            size[1],
            per(blocking),
            per(submitting),
            gpu_ms.len(),
            stage.timing_drops()
        );
        assert!(!gpu_ms.is_empty());
        assert!(gpu_ms.iter().all(|ms| *ms > 0.0));
        pfx::adapter::turn();
        let started = std::time::Instant::now();
        stage.set_hour(Some(19.0)).unwrap();
        let blocking = started.elapsed();
        let started = std::time::Instant::now();
        stage.request_hour(Some(20.0)).unwrap();
        let requesting = started.elapsed();
        stage.settle_sky().unwrap();
        println!(
            "an hour on the calling thread: set_hour blocks {:.2} ms, request_hour {:.3} ms",
            blocking.as_secs_f64() * 1000.0,
            requesting.as_secs_f64() * 1000.0
        );
        assert!(!stage.settling());
        pfx::adapter::turn();
    }

    fn table() -> (Recipe, Built) {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/table/table.toml");
        let loaded = manfred_recipe::load(&path).unwrap();
        let built = manfred_build::build(&loaded.recipe, path.parent().unwrap()).unwrap();
        (loaded.recipe, built)
    }

    fn pixel_of(eye: &Eye, size: [u32; 2], point: [f64; 3]) -> [u32; 2] {
        let matrix = multiply(eye.projection(size, 0.05, 5.0), eye.view());
        let [x, y, z] = point.map(|v| v as f32);
        let clip = pfx_live::frame::transform(matrix, [x, y, z, 1.0]);
        let ndc = [clip[0] / clip[3], clip[1] / clip[3]];
        [
            ((ndc[0] + 1.0) * 0.5 * size[0] as f32) as u32,
            ((1.0 - ndc[1]) * 0.5 * size[1] as f32) as u32,
        ]
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_pick_under_the_balls_centre_returns_the_ball() {
        let (recipe, built) = table();
        let size = [480, 300];
        let mut stage = Stage::new(crate::headless().unwrap(), &recipe, &built, size).unwrap();
        let eye = Eye::from_camera(crate::camera(&recipe, "main").unwrap(), 1.6);
        let ball = recipe
            .objects
            .iter()
            .position(|o| o.name == "ball")
            .unwrap();
        let at = pixel_of(&eye, size, recipe.objects[ball].at);
        let target = stage
            .gpu()
            .offscreen(size[0], size[1], stage.output_format())
            .unwrap();
        stage.pick(at[0], at[1]).unwrap();
        let mut picked = None;
        for _ in 0..20 {
            stage.render(&eye, 0.0, &target.view).unwrap();
            picked = stage.picked();
            if picked.is_some() {
                break;
            }
        }
        let picked = picked.expect("no pick within twenty frames");
        assert_eq!(picked.pixel, at);
        assert_eq!(picked.object, Some(ball), "{picked:?}");
        stage.pick(2, 2).unwrap();
        let mut sky = None;
        for _ in 0..20 {
            stage.render(&eye, 0.0, &target.view).unwrap();
            sky = stage.picked();
            if sky.is_some() {
                break;
            }
        }
        assert_ne!(sky.unwrap().object, Some(ball));
        pfx::adapter::turn();
    }

    const PANES: &str = "manfred = 1\nname = \"panes\"\n\n[[camera]]\nname = \"main\"\nposition = [0.0, 0.25, 2.0]\ntarget = [0.0, 0.15, 0.0]\nfov_y_deg = 35.0\n\n[[object]]\nname = \"back\"\nkind = \"box\"\nmaterial = \"orange\"\nsize = [0.4, 0.4, 0.2]\nat = [-0.3, 0.2, 0.0]\n\n[[object]]\nname = \"pane\"\nkind = \"box\"\nmaterial = \"glass\"\nsize = [0.5, 0.5, 0.02]\nat = [-0.3, 0.2, 0.3]\n\n[[object]]\nname = \"side\"\nkind = \"box\"\nmaterial = \"cream\"\nsize = [0.3, 0.3, 0.2]\nat = [0.35, 0.15, 0.0]\n\n[[object]]\nname = \"label\"\nkind = \"text\"\non = \"side\"\nspans = [{ text = \"\u{2588}\u{2588}\", size = 0.08 }]\n\n[materials.glass]\nfamily = \"glass\"\nbase = [0.9, 0.95, 1.0]\nroughness = 0.02\ntransmission = 1.0\nior = 1.5\nthickness = 0.004\n\n[materials.orange]\nfamily = \"lacquer\"\nbase = [0.73, 0.06, 0.0]\nroughness = 0.45\nclearcoat = 1.0\nclearcoat_roughness = 0.07\n\n[materials.cream]\nfamily = \"lacquer\"\nbase = [0.81, 0.58, 0.33]\nroughness = 0.5\nclearcoat = 1.0\nclearcoat_roughness = 0.1\n";

    fn panes() -> (Recipe, Built) {
        let recipe = parse(PANES).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        (recipe, built)
    }

    fn opaque_and_glass(prepared: &Prepared) -> (Vec<Instance>, Vec<glass::Surface>) {
        let at = opaque_at(&prepared.glass);
        let uploaded: Vec<Uploaded> = at
            .iter()
            .enumerate()
            .map(|(n, &index)| Uploaded {
                mark: prepared.marks[index].clone(),
                handle: MeshHandle(n as u32),
            })
            .collect();
        let dress: Vec<Dress> = at.iter().map(|&index| prepared.dress[index]).collect();
        let instances = instances(&prepared.pieces, &prepared.slots, &dress, &uploaded, &at);
        let glassed: Vec<Glassed> = shown_glass(&prepared.glass)
            .enumerate()
            .map(|(n, index)| Glassed {
                mark: prepared.marks[index].clone(),
                handle: glass::MeshHandle(n as u32),
            })
            .collect();
        let surfaces = glass_surfaces(
            &prepared.pieces,
            &prepared.slots,
            &prepared.materials,
            &prepared.glass,
            &prepared.clips,
            &glassed,
        );
        (instances, surfaces)
    }

    #[test]
    fn a_glass_material_goes_to_the_glass_pass_and_the_rest_stay_instances() {
        let (recipe, built) = panes();
        let prepared = Prepared::of(&recipe, &built.parts).unwrap();
        let pane = recipe
            .objects
            .iter()
            .position(|o| o.name == "pane")
            .unwrap();
        assert_eq!(prepared.glass_parts(), [pane]);
        let routed: Vec<(usize, bool)> = prepared
            .pieces
            .iter()
            .zip(&prepared.glass)
            .map(|(piece, glassy)| (piece.object, *glassy))
            .collect();
        assert_eq!(routed, [(0, false), (1, true), (2, false)]);
        let glass_slot = prepared.slots[1] as usize;
        assert_eq!(prepared.names[glass_slot], "glass");
        let (drawn, surfaces) = opaque_and_glass(&prepared);
        assert_eq!(drawn.len(), 2);
        assert!(drawn.iter().all(|i| !i.shadow_only && i.casts_shadow));
        assert!(drawn.iter().all(|i| i.id != pane as u32 + 1));
        assert_eq!(surfaces.len(), 1);
        assert!(surfaces[0].casts_shadow);
        assert_eq!(surfaces[0].id, pane as u32 + 1);
        assert_eq!(surfaces[0].mesh, glass::MeshHandle(0));
        assert!(surfaces[0].material.transmission > 0.0);
    }

    #[test]
    fn a_glass_surface_casts_from_its_shadow_key_and_only_keeps_an_occluder() {
        let pane_of = |text: &str| {
            let recipe = parse(text).unwrap();
            let built = manfred_build::build(&recipe, Path::new("")).unwrap();
            let prepared = Prepared::of(&recipe, &built.parts).unwrap();
            let pane = recipe
                .objects
                .iter()
                .position(|o| o.name == "pane")
                .unwrap();
            (pane, prepared)
        };
        let none = PANES.replace(
            "material = \"glass\"\n",
            "material = \"glass\"\nshadow = \"none\"\n",
        );
        let (pane, prepared) = pane_of(&none);
        let (drawn, surfaces) = opaque_and_glass(&prepared);
        assert_eq!(surfaces.len(), 1);
        assert!(!surfaces[0].casts_shadow);
        assert!(drawn.iter().all(|i| i.id != pane as u32 + 1));
        assert!(drawn.iter().all(|i| !i.shadow_only));
        let only = PANES.replace(
            "material = \"glass\"\n",
            "material = \"glass\"\nshadow = \"only\"\n",
        );
        let (pane, prepared) = pane_of(&only);
        assert_eq!(prepared.glass_parts(), [pane]);
        assert_eq!(prepared.glass, [false, true, false]);
        let (drawn, surfaces) = opaque_and_glass(&prepared);
        assert_eq!(surfaces.len(), 1);
        assert!(surfaces[0].shadow_only && surfaces[0].casts_shadow);
        assert_eq!(surfaces[0].id, pane as u32 + 1);
        assert!(drawn.iter().all(|i| i.id != pane as u32 + 1));
        assert!(drawn.iter().all(|i| !i.shadow_only));
    }

    #[test]
    fn a_glass_surface_carries_its_movers_clip_planes() {
        let moved = PANES.replace(
            "[materials.glass]",
            "[[mover]]\ngroup = \"slide\"\npivot = [0.0, 0.0, 0.0]\naxis = [0.0, 1.0, 0.0]\ntravel = 0.1\nclip = [0.0, 1.0, 0.0, 0.05]\nclip2 = [1.0, 0.0, 0.0, 0.2]\nparts = [\"pane\"]\n\n[materials.glass]",
        );
        let recipe = parse(&moved).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        let prepared = Prepared::of(&recipe, &built.parts).unwrap();
        let (_, surfaces) = opaque_and_glass(&prepared);
        assert_eq!(surfaces.len(), 1);
        assert_eq!(
            surfaces[0].clip,
            [[0.0, 1.0, 0.0, 0.05], [1.0, 0.0, 0.0, 0.2]]
        );
        let (_, plain) =
            opaque_and_glass(&Prepared::of(&parse(PANES).unwrap(), &built_parts(PANES)).unwrap());
        assert_eq!(plain[0].clip, [[0.0; 4]; 2]);
    }

    #[test]
    fn hidden_glass_draws_no_surface() {
        let hidden = PANES.replace(
            "material = \"glass\"\n",
            "material = \"glass\"\nhidden = true\n",
        );
        let recipe = parse(&hidden).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        let prepared = Prepared::of(&recipe, &built.parts).unwrap();
        assert!(prepared.glass_parts().is_empty());
        assert!(prepared.pieces.iter().all(|piece| piece.name != "pane"));
        let plain = Prepared::of(&parse(BASE).unwrap(), &built_parts(BASE)).unwrap();
        assert!(plain.glass.iter().all(|glassy| !glassy));
    }

    fn built_parts(text: &str) -> Vec<Part> {
        let recipe = parse(text).unwrap();
        manfred_build::build(&recipe, Path::new("")).unwrap().parts
    }

    #[test]
    fn surface_text_is_boxed_on_its_face_for_the_cpu_pick() {
        let (recipe, built) = panes();
        let label = recipe
            .objects
            .iter()
            .position(|o| o.name == "label")
            .unwrap();
        let laid = Laid::of(&recipe, &built.parts).unwrap();
        let boxes = laid.surface_boxes();
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].0, label);
        for corner in boxes[0].1 {
            assert!((corner[2] - (0.1 + text::LIFT)).abs() < 1e-6, "{corner:?}");
            assert!((0.2..0.5).contains(&corner[0]), "{corner:?}");
        }
        assert!(laid.overlay_boxes([320, 200]).is_empty());
    }

    fn picked_at(stage: &mut Stage, eye: &Eye, at: [u32; 2]) -> Pick {
        let size = stage.size();
        let target = stage
            .gpu()
            .offscreen(size[0], size[1], stage.output_format())
            .unwrap();
        stage.pick(at[0], at[1]).unwrap();
        for _ in 0..20 {
            stage.render(eye, 0.0, &target.view).unwrap();
            if let Some(picked) = stage.picked() {
                return picked;
            }
        }
        panic!("no pick within twenty frames at {at:?}")
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_pick_on_glass_or_text_returns_its_object() {
        let (recipe, built) = panes();
        let size = [480, 300];
        let mut stage = Stage::new(crate::headless().unwrap(), &recipe, &built, size).unwrap();
        assert_eq!(stage.glass_count(), 1);
        assert_eq!(stage.instance_count(), 2);
        let eye = Eye::from_camera(crate::camera(&recipe, "main").unwrap(), 1.6);
        let index = |name: &str| recipe.objects.iter().position(|o| o.name == name);
        let pane = pixel_of(&eye, size, [-0.3, 0.2, 0.31]);
        let picked = picked_at(&mut stage, &eye, pane);
        assert_eq!(picked.object, index("pane"), "{picked:?}");
        pfx::adapter::turn();
        let side = pixel_of(&eye, size, [0.25, 0.27, 0.1]);
        let picked = picked_at(&mut stage, &eye, side);
        assert_eq!(picked.object, index("side"), "{picked:?}");
        pfx::adapter::turn();
        let boxes = Laid::of(&recipe, &built.parts).unwrap().surface_boxes();
        let corners = boxes[0].1;
        let centre: [f64; 3] =
            std::array::from_fn(|axis| corners.iter().map(|c| c[axis]).sum::<f64>() / 4.0);
        let text = pixel_of(&eye, size, centre);
        let picked = picked_at(&mut stage, &eye, text);
        assert_eq!(picked.object, index("label"), "{picked:?}");
        pfx::adapter::turn();
        let mut motion = IDENTITY;
        motion[3] = [0.0, 0.5, 0.0, 1.0];
        stage.preview(index("pane").unwrap(), motion).unwrap();
        assert_eq!(stage.moved(), [index("pane").unwrap()]);
        let picked = picked_at(&mut stage, &eye, pane);
        assert_eq!(picked.object, index("back"), "{picked:?}");
        stage.restore();
        assert!(stage.moved().is_empty());
        let moved =
            parse(&PANES.replace("at = [-0.3, 0.2, 0.3]", "at = [-0.3, 0.2, 0.35]")).unwrap();
        let rebuilt = manfred_build::build(&moved, Path::new("")).unwrap();
        stage.update(&moved, &rebuilt).unwrap();
        assert_eq!(stage.glass_count(), 1);
        let picked = picked_at(&mut stage, &eye, pane);
        assert_eq!(picked.object, index("pane"), "{picked:?}");
        pfx::adapter::turn();
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_preview_moves_the_object_and_its_children_and_restore_puts_them_back() {
        let text = BASE.replace(
            "name = \"c\"\nkind = \"box\"",
            "name = \"c\"\nparent = \"b\"\nkind = \"box\"",
        );
        let recipe = parse(&text).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        let mut stage =
            Stage::new(crate::headless().unwrap(), &recipe, &built, [320, 200]).unwrap();
        let mut motion = IDENTITY;
        motion[3] = [0.1, 0.0, 0.0, 1.0];
        stage.preview(1, motion).unwrap();
        assert_eq!(stage.moved(), [1, 2]);
        stage.preview(0, motion).unwrap();
        assert_eq!(stage.moved(), [0]);
        assert!(stage.preview(3, motion).is_err());
        stage.restore();
        assert!(stage.moved().is_empty());
        stage.preview(1, motion).unwrap();
        stage.update(&recipe, &built).unwrap();
        assert!(stage.moved().is_empty());
        pfx::adapter::turn();
    }
}
