use std::path::PathBuf;
use std::sync::Arc;

use egui::TextureId;
use egui_wgpu::Renderer;
use manfred_build::Built;
use manfred_recipe::Recipe;
use manfred_scene::{Change, Eye, Laid, Memory, MeshStats, Prepared, Stage};
use pfx_gpu::{Gpu, wgpu};

use crate::Shell;
use crate::document::Document;
use crate::follow::{Follow, Mode};
use crate::gizmo::{Matrix, Poses};
use crate::pick::{self, Lens, Words};
use crate::watched::{self, Event, Live, Watched};
use pfx_editor_shell::paint::clear;
use pfx_editor_shell::worker::{Poll, Wake, Worker};

const SETTLE_FRAMES: u32 = 16;
pub const FAKE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

pub struct Fake;

impl Fake {
    pub fn colour(eye: &Eye) -> egui::Color32 {
        let offset: Vec<f64> = eye
            .position
            .iter()
            .zip(eye.target)
            .map(|(position, target)| position - target)
            .collect();
        let length = offset.iter().map(|value| value * value).sum::<f64>().sqrt();
        let channel = |value: f64| {
            let unit = if length > 0.0 { value / length } else { 0.0 };
            (40.0 + 80.0 * (unit + 1.0)).round() as u8
        };
        egui::Color32::from_rgb(channel(offset[0]), channel(offset[1]), channel(offset[2]))
    }
}

pub enum Drawing {
    Stage(Box<Stage>),
    Scene(Box<Live>),
    Fake(Fake),
}

impl Drawing {
    fn format(&self) -> wgpu::TextureFormat {
        match self {
            Drawing::Stage(stage) => stage.output_format(),
            Drawing::Scene(live) => live.output_format(),
            Drawing::Fake(_) => FAKE_FORMAT,
        }
    }

    fn resize(&mut self, size: [u32; 2]) -> Result<(), String> {
        match self {
            Drawing::Stage(stage) => stage.resize(size).map_err(|error| error.0),
            Drawing::Scene(live) => live.resize(size),
            Drawing::Fake(_) => Ok(()),
        }
    }

    fn set_hour(&mut self, hour: Option<f64>, live: bool) -> Result<(), String> {
        match self {
            Drawing::Stage(stage) if live => stage.request_hour(hour).map_err(|error| error.0),
            Drawing::Stage(stage) => stage.set_hour(hour).map_err(|error| error.0),
            Drawing::Scene(_) | Drawing::Fake(_) => Ok(()),
        }
    }

    fn settling(&self) -> bool {
        match self {
            Drawing::Stage(stage) => stage.settling(),
            Drawing::Scene(live) => live.settling(),
            Drawing::Fake(_) => false,
        }
    }

    fn render(
        &mut self,
        gpu: &Gpu,
        eye: &Eye,
        view: &wgpu::TextureView,
        live_window: bool,
    ) -> Result<(), String> {
        match self {
            Drawing::Stage(stage) if live_window => {
                stage.submit(eye, 0.0, view).map_err(|error| error.0)
            }
            Drawing::Stage(stage) => stage.render(eye, 0.0, view).map_err(|error| error.0),
            Drawing::Scene(live) => live.render(eye, view, live_window),
            Drawing::Fake(_) => {
                let mut encoder =
                    gpu.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("manfred fake viewport"),
                        });
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("manfred fake viewport"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(clear(Fake::colour(eye))),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                gpu.queue.submit([encoder.finish()]);
                Ok(())
            }
        }
    }
}

struct Target {
    draw: wgpu::TextureView,
    show: wgpu::TextureView,
    size: [u32; 2],
    format: wgpu::TextureFormat,
}

fn target(gpu: &Gpu, size: [u32; 2], format: wgpu::TextureFormat) -> Target {
    let shown = format.remove_srgb_suffix();
    let views = if shown == format {
        Vec::new()
    } else {
        vec![shown]
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("manfred viewport"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &views,
    });
    let draw = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let show = texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(shown),
        ..wgpu::TextureViewDescriptor::default()
    });
    Target {
        draw,
        show,
        size,
        format,
    }
}

enum Job {
    Made(Result<Drawing, String>),
    Prepared(Result<Box<Prepared>, String>),
    Applied(Box<Stage>, Result<Change, String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Work {
    Making(usize),
    Comparing,
    Updating,
}

struct Cast {
    generation: u64,
    built: Option<Arc<Built>>,
    laid: Option<Laid>,
    words: Words,
}

pub struct Viewer {
    follow: Follow,
    shown: u64,
    staged: u64,
    drawing: Option<Drawing>,
    target: Option<Target>,
    texture: Option<TextureId>,
    hour: Option<f64>,
    failure: Option<String>,
    refused: Option<String>,
    drawn: bool,
    worker: Option<Worker<Job>>,
    work: Option<Work>,
    away: bool,
    change: Option<Change>,
    settling: u32,
    gate: Option<Wake>,
    previewed: Option<(usize, Matrix)>,
    picking: bool,
    parts: Option<Cast>,
    memory: Memory,
    scene: Option<Watched>,
    seen: u64,
    steady: bool,
}

impl Viewer {
    pub fn new(path: impl Into<PathBuf>, mode: Mode) -> Viewer {
        Viewer::with(Follow::new(path, mode))
    }

    pub fn idle(path: impl Into<PathBuf>, mode: Mode) -> Viewer {
        Viewer::with(Follow::idle(path, mode))
    }

    pub fn with(follow: Follow) -> Viewer {
        let scene = watched::is_scene(follow.path()).then(|| Watched::open(follow.path()));
        Viewer {
            follow,
            shown: 0,
            staged: 0,
            drawing: None,
            target: None,
            texture: None,
            hour: None,
            failure: None,
            refused: None,
            drawn: false,
            worker: None,
            work: None,
            away: false,
            change: None,
            settling: 0,
            gate: None,
            previewed: None,
            picking: false,
            parts: None,
            memory: Memory::default(),
            scene,
            seen: 0,
            steady: false,
        }
    }

    pub fn scene(path: impl Into<PathBuf>, mode: Mode) -> Viewer {
        let path = path.into();
        let mut viewer = Viewer::with(Follow::idle(&path, mode));
        if viewer.scene.is_none() {
            viewer.scene = Some(Watched::open(path));
        }
        viewer
    }

    pub fn is_scene(&self) -> bool {
        self.scene.is_some()
    }

    pub fn watched(&self) -> Vec<PathBuf> {
        match &self.scene {
            Some(watched) => watched.files(),
            None => self.follow.watched(),
        }
    }

    pub fn reloads(&self) -> u64 {
        self.scene.as_ref().map_or(0, Watched::reloads)
    }

    pub fn background(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        self.worker = Some(Worker::new(Arc::new(wake)));
    }

    pub fn forget(&mut self) {
        self.staged = 0;
        self.drawing = None;
        self.target = None;
        self.texture = None;
        self.hour = None;
        self.failure = None;
        self.refused = None;
        self.drawn = false;
        self.change = None;
        self.previewed = None;
        self.picking = false;
    }

    pub fn follow(&self) -> &Follow {
        &self.follow
    }

    pub fn path(&self) -> String {
        self.follow.path().display().to_string()
    }

    pub fn stale(&self) -> bool {
        self.scene.is_none() && self.follow.stale()
    }

    pub fn staging(&self) -> bool {
        self.work.is_some()
    }

    pub fn showing(&self) -> bool {
        self.drawn
            && (self.away || matches!(self.drawing, Some(Drawing::Stage(_) | Drawing::Scene(_))))
    }

    pub fn change(&self) -> Option<Change> {
        self.change
    }

    pub fn mesh_stats(&self) -> Option<MeshStats> {
        match &self.drawing {
            Some(Drawing::Stage(stage)) => Some(stage.mesh_stats()),
            Some(Drawing::Scene(live)) => Some(live.mesh_stats()),
            _ => None,
        }
    }

    pub fn frame_ms(&self) -> Option<f64> {
        match &self.drawing {
            Some(Drawing::Stage(stage)) => stage.frame_ms(),
            Some(Drawing::Scene(live)) => live.frame_ms(),
            _ => None,
        }
    }

    pub fn timing_drops(&self) -> u64 {
        match &self.drawing {
            Some(Drawing::Stage(stage)) => stage.timing_drops(),
            Some(Drawing::Scene(live)) => live.timing_drops(),
            _ => 0,
        }
    }

    fn live(&self) -> bool {
        self.worker.is_some() && !self.steady
    }

    pub fn steady(&mut self) {
        self.steady = true;
    }

    pub fn request(&mut self, shell: &mut Shell, done: impl Fn() + Send + Sync + 'static) {
        if self.scene.is_some() {
            self.reload(shell);
            return;
        }
        self.follow.request(done);
        self.sync(shell);
    }

    pub fn settle(&mut self, shell: &mut Shell) -> bool {
        if self.scene.is_some() {
            self.sync(shell);
            return false;
        }
        let changed = self.follow.settle();
        self.sync(shell);
        changed
    }

    pub fn reload(&mut self, shell: &mut Shell) -> bool {
        let changed = match self.scene.as_mut() {
            Some(watched) => {
                let event = watched.check();
                self.take(event)
            }
            None => self.follow.reload(),
        };
        self.sync(shell);
        changed
    }

    fn take(&mut self, event: Option<Event>) -> bool {
        match event {
            Some(Event::Reloaded(reload)) => {
                if let Some(Drawing::Scene(live)) = self.drawing.as_mut() {
                    match live.apply(&reload) {
                        Ok(_) => {
                            self.refused = None;
                            self.settling = SETTLE_FRAMES;
                        }
                        Err(error) => self.refused = Some(format!("scene: {error}")),
                    }
                }
                true
            }
            Some(Event::Broken) | None => false,
        }
    }

    fn watch(&mut self) {
        let live = self.live();
        let Some(watched) = self.scene.as_mut() else {
            return;
        };
        let event = if live {
            watched.poll()
        } else {
            watched.check()
        };
        self.take(event);
    }

    pub fn sync(&mut self, shell: &mut Shell) {
        if let Some(watched) = &self.scene {
            if self.seen != watched.generation() {
                self.seen = watched.generation();
                let path = self.path();
                let document = match watched.scene() {
                    Some(scene) => watched::document(scene, &path),
                    None => Document::empty(&path),
                };
                shell.show(document);
            }
            shell.set_trouble(watched.error().map(str::to_string).or(self.refused.clone()));
            shell.set_busy(false);
            shell.set_preparing(None);
            shell.set_view(self.texture.filter(|_| self.drawn), self.view_error());
            return;
        }
        if self.shown != self.follow.generation() {
            self.shown = self.follow.generation();
            let path = self.path();
            let document = match self.follow.recipe() {
                Some(recipe) => Document::from_recipe(recipe, &path),
                None => Document::empty(&path),
            };
            shell.show(document);
            let built = if self.follow.built().is_none()
                && self.follow.mode() == Mode::Fake
                && self.follow.recipe().is_some_and(grouped)
            {
                self.cast_parts().and_then(|cast| cast.built.clone())
            } else {
                None
            };
            if let Some(recipe) = self.follow.recipe() {
                let solids = self
                    .follow
                    .built()
                    .or(built.as_deref())
                    .map(|built| crate::viewport::solids(recipe, built))
                    .unwrap_or_default();
                let poses = match self.follow.doc() {
                    Some(doc) => Poses::in_doc(recipe, doc),
                    None => Poses::of(recipe),
                };
                shell.set_poses(poses.centred(&solids));
                shell.set_solids(solids);
            }
        }
        let trouble = self
            .follow
            .error()
            .map(str::to_string)
            .or(self.refused.clone());
        shell.set_trouble(trouble);
        shell.set_busy(self.follow.busy() || self.work.is_some());
        shell.set_preparing(self.preparing());
        shell.set_view(self.texture.filter(|_| self.drawn), self.view_error());
    }

    fn preparing(&self) -> Option<String> {
        match self.work {
            Some(Work::Making(objects)) => Some(format!(
                "making the stage: the renderer, its pipelines, the sky and the meshes of {objects} {}",
                if objects == 1 { "object" } else { "objects" }
            )),
            Some(Work::Comparing) => {
                Some("rebuilding: comparing each mesh with the uploaded one".into())
            }
            Some(Work::Updating) => {
                Some("rebuilding: swapping meshes and preparing the new sky".into())
            }
            None if self.follow.busy() => Some("reading and building the recipe".into()),
            None => None,
        }
    }

    fn view_error(&self) -> Option<String> {
        if let Some(failure) = &self.failure {
            return Some(failure.clone());
        }
        if self.drawing.is_some() || self.away {
            return None;
        }
        if let Some(watched) = &self.scene {
            return Some(
                watched
                    .error()
                    .or(self.refused.as_deref())
                    .unwrap_or("nothing to draw yet")
                    .to_string(),
            );
        }
        if let Some(error) = self.refused.as_deref().or(self.follow.error()) {
            return Some(error.to_string());
        }
        if self.preparing().is_some() {
            return None;
        }
        Some("nothing to draw yet".to_string())
    }

    fn gated(&self) -> impl Fn() + Send + 'static {
        let gate = self.gate.clone();
        move || {
            if let Some(gate) = &gate {
                gate();
            }
        }
    }

    fn stage(&mut self, gpu: Option<&Gpu>, size: [u32; 2]) {
        if self.scene.is_some() {
            self.stage_scene(gpu, size);
            return;
        }
        self.receive();
        if self.worker.as_ref().is_some_and(Worker::busy)
            || self.staged == self.follow.generation()
            || !self.follow.ready()
        {
            return;
        }
        let Some((recipe, built)) = self.follow.shared() else {
            return;
        };
        let generation = self.follow.generation();
        let objects = recipe.objects.len();
        match (self.follow.mode(), built, gpu) {
            (Mode::Fake, _, _) => {
                self.staged = generation;
                self.run(Work::Making(objects), || Job::Made(Ok(Drawing::Fake(Fake))));
            }
            (Mode::Stage, Some(built), Some(gpu)) => {
                self.staged = generation;
                let memory = self.memory.clone();
                if matches!(self.drawing, Some(Drawing::Stage(_))) {
                    self.run(Work::Comparing, move || {
                        Job::Prepared(
                            Stage::prepare_kept(&recipe, &built, &memory)
                                .map(Box::new)
                                .map_err(|error| format!("stage: {error}")),
                        )
                    });
                } else {
                    let gpu = gpu.clone();
                    self.run(Work::Making(objects), move || {
                        Job::Made(made(gpu, &recipe, &built, size, &memory))
                    });
                }
            }
            _ => {}
        }
    }

    fn stage_scene(&mut self, gpu: Option<&Gpu>, size: [u32; 2]) {
        self.watch();
        let Some(watched) = self.scene.as_ref() else {
            return;
        };
        let Some(scene) = watched.scene() else {
            return;
        };
        let generation = watched.generation();
        if self.drawing.is_some() || self.staged == generation {
            return;
        }
        let made = match (self.follow.mode(), gpu) {
            (Mode::Fake, _) => Ok(Drawing::Fake(Fake)),
            (Mode::Stage, Some(gpu)) => Live::new(gpu.clone(), scene, size)
                .map(|live| Drawing::Scene(Box::new(live)))
                .map_err(|error| format!("scene: {error}")),
            (Mode::Stage, None) => return,
        };
        self.staged = generation;
        self.install(made);
    }

    fn run(&mut self, work: Work, job: impl FnOnce() -> Job + Send + 'static) {
        let gate = self.gated();
        match self.worker.as_mut() {
            Some(worker) => {
                worker.start(move || {
                    gate();
                    job()
                });
                self.work = Some(work);
            }
            None => self.finish(job()),
        }
    }

    fn finish(&mut self, job: Job) {
        match job {
            Job::Made(result) => self.install(result),
            Job::Prepared(Ok(prepared)) => self.compared(*prepared),
            Job::Prepared(Err(error)) => self.refused = Some(error),
            Job::Applied(stage, result) => {
                self.away = false;
                self.drawing = Some(Drawing::Stage(stage));
                self.updated(result);
            }
        }
    }

    fn receive(&mut self) {
        let Some(worker) = self.worker.as_mut() else {
            return;
        };
        match worker.poll() {
            Poll::Idle | Poll::Busy => {}
            Poll::Done(job) => {
                self.work = None;
                self.finish(job);
            }
            Poll::Lost => {
                self.work = None;
                if self.away {
                    self.away = false;
                    self.drawn = false;
                }
                self.refused = Some("stage: the worker stopped; the log says why".into());
            }
        }
    }

    fn install(&mut self, made: Result<Drawing, String>) {
        match made {
            Ok(drawing) => {
                self.drawing = Some(drawing);
                self.previewed = None;
                self.picking = false;
                self.target = None;
                self.hour = None;
                self.failure = None;
                self.refused = None;
                self.drawn = false;
                self.settling = SETTLE_FRAMES;
            }
            Err(error) => self.refused = Some(error),
        }
    }

    fn compared(&mut self, prepared: Prepared) {
        let mut stage = match self.drawing.take() {
            Some(Drawing::Stage(stage)) => stage,
            other => {
                self.drawing = other;
                self.staged = 0;
                return;
            }
        };
        let aside = self.live() && stage.sky_changes(&prepared);
        let apply = move || {
            let result = stage
                .apply(prepared)
                .map_err(|error| format!("stage: {error}"));
            Job::Applied(stage, result)
        };
        if aside {
            self.away = true;
            self.run(Work::Updating, apply);
        } else {
            self.finish(apply());
        }
    }

    fn updated(&mut self, result: Result<Change, String>) {
        match result {
            Ok(change) => {
                self.previewed = None;
                self.change = Some(change);
                self.settling = SETTLE_FRAMES;
                self.refused = None;
                self.failure = None;
            }
            Err(error) => {
                self.drawing = None;
                self.drawn = false;
                self.staged = 0;
                self.refused = Some(error);
            }
        }
    }

    pub fn frame(
        &mut self,
        gpu: &Gpu,
        renderer: &mut Renderer,
        shell: &mut Shell,
        pixels_per_point: f32,
    ) -> bool {
        let before = (
            self.texture.filter(|_| self.drawn),
            self.view_error(),
            self.preparing(),
            self.shown,
        );
        self.sync(shell);
        let Some(rect) = shell.viewport() else {
            return false;
        };
        let size = [
            ((rect.width() * pixels_per_point).round() as u32).max(1),
            ((rect.height() * pixels_per_point).round() as u32).max(1),
        ];
        self.stage(Some(gpu), size);
        let picking = self.point(shell);
        let live = self.live();
        let mut settle = false;
        if !shell.paused()
            && let (Some(drawing), Some(eye)) = (self.drawing.as_mut(), shell.view_eye())
        {
            settle = self.settling > 0;
            self.settling = self.settling.saturating_sub(1);
            let fresh = self
                .target
                .as_ref()
                .is_none_or(|target| target.size != size || target.format != drawing.format());
            let result = (|| {
                if fresh {
                    let made = target(gpu, size, drawing.format());
                    match self.texture {
                        Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                            &gpu.device,
                            &made.show,
                            wgpu::FilterMode::Linear,
                            id,
                        ),
                        None => {
                            self.texture = Some(renderer.register_native_texture(
                                &gpu.device,
                                &made.show,
                                wgpu::FilterMode::Linear,
                            ));
                        }
                    }
                    self.target = Some(made);
                    drawing.resize(size)?;
                }
                let hour = shell.hour();
                if hour != self.hour {
                    drawing.set_hour(hour, live)?;
                    self.hour = hour;
                }
                if let Drawing::Stage(stage) = drawing {
                    stage.look_through(shell.shown_camera());
                }
                let view = &self.target.as_ref().ok_or("no viewport texture")?.draw;
                drawing.render(gpu, &eye, view, live)
            })();
            settle |= live && drawing.settling();
            match result {
                Ok(()) => {
                    self.drawn = true;
                    self.failure = None;
                }
                Err(error) => self.failure = Some(error),
            }
        }
        self.sync(shell);
        settle
            || picking
            || before
                != (
                    self.texture.filter(|_| self.drawn),
                    self.view_error(),
                    self.preparing(),
                    self.shown,
                )
    }

    fn point(&mut self, shell: &mut Shell) -> bool {
        let caught_up = self.work.is_none() && self.staged == self.follow.generation();
        let wanted = shell.preview();
        let asked = shell.take_pick();
        match self.drawing.as_mut() {
            Some(Drawing::Stage(stage)) => {
                if wanted != self.previewed {
                    match wanted {
                        Some((object, motion)) => {
                            if stage.preview(object, motion).is_ok() {
                                self.previewed = wanted;
                            }
                        }
                        None if caught_up => {
                            stage.restore();
                            self.previewed = None;
                        }
                        None => {}
                    }
                }
                if let Some(uv) = asked {
                    let [x, y] = pick::pixel(uv, stage.size());
                    self.picking = stage.pick(x, y).is_ok();
                }
                if self.picking
                    && let Some(found) = stage.picked()
                {
                    self.picking = false;
                    shell.picked(found.object);
                }
                self.picking
            }
            Some(Drawing::Scene(live)) => {
                if let Some(uv) = asked {
                    let [x, y] = pick::pixel(uv, live.size());
                    self.picking = live.pick(x, y).is_ok();
                }
                if self.picking
                    && let Some(id) = live.picked()
                {
                    self.picking = false;
                    let found = self
                        .scene
                        .as_ref()
                        .and_then(Watched::scene)
                        .and_then(|scene| watched::object_of(scene, id));
                    shell.picked(found);
                }
                self.picking
            }
            Some(Drawing::Fake(_)) => {
                if let Some(uv) = asked {
                    let found = match &self.scene {
                        Some(watched) => cast_scene(watched, shell, uv),
                        None => self.cast(shell, uv),
                    };
                    shell.picked(found);
                }
                false
            }
            None => false,
        }
    }

    fn cast_parts(&mut self) -> Option<&Cast> {
        let generation = self.follow.generation();
        if self
            .parts
            .as_ref()
            .is_none_or(|cast| cast.generation != generation)
        {
            let built = match self.follow.shared() {
                Some((_, Some(built))) => Some(built),
                _ => self.follow.recipe().and_then(|recipe| {
                    let base = self
                        .follow
                        .path()
                        .parent()
                        .unwrap_or(std::path::Path::new(""));
                    manfred_build::build_with(recipe, base, self.follow.registry())
                        .ok()
                        .map(Arc::new)
                }),
            };
            let laid = match (self.follow.recipe(), &built) {
                (Some(recipe), Some(built)) => Laid::of(recipe, &built.parts).ok(),
                _ => None,
            };
            let words = laid
                .as_ref()
                .map(|laid| Words::laid(&laid.surface_boxes()))
                .unwrap_or_default();
            self.parts = Some(Cast {
                generation,
                built,
                laid,
                words,
            });
        }
        self.parts.as_ref()
    }

    fn cast(&mut self, shell: &Shell, uv: [f32; 2]) -> Option<usize> {
        let (eye, image) = (shell.view_eye()?, shell.viewport()?);
        self.cast_parts()?;
        let cast = self.parts.as_ref()?;
        let built = cast.built.as_ref()?;
        let size = [image.width(), image.height()].map(|side| side.round().max(1.0) as u32);
        let overlay = cast
            .laid
            .as_ref()
            .map(|laid| laid.overlay_boxes(size))
            .unwrap_or_default();
        let words = cast.words.clone().with_overlay(image, size, &overlay);
        let at = image.min + egui::Vec2::new(uv[0] * image.width(), uv[1] * image.height());
        pick::cast(&built.parts, &words, &Lens::new(&eye, image).ray(at), at)
    }

    #[cfg(test)]
    fn hold(&mut self, gate: impl Fn() + Send + Sync + 'static) {
        self.gate = Some(Arc::new(gate));
    }
}

fn grouped(recipe: &Recipe) -> bool {
    recipe.objects.iter().any(|group| {
        (group.hidden || matches!(group.kind, manfred_recipe::Kind::Text { .. }))
            && recipe
                .objects
                .iter()
                .any(|child| child.parent.as_deref() == Some(group.name.as_str()))
    })
}

fn cast_scene(watched: &Watched, shell: &Shell, uv: [f32; 2]) -> Option<usize> {
    let (eye, image) = (shell.view_eye()?, shell.viewport()?);
    let at = image.min + egui::Vec2::new(uv[0] * image.width(), uv[1] * image.height());
    watched::cast(watched.scene()?, &Lens::new(&eye, image).ray(at))
}

fn made(
    gpu: Gpu,
    recipe: &Recipe,
    built: &Built,
    size: [u32; 2],
    memory: &Memory,
) -> Result<Drawing, String> {
    Stage::kept(gpu, recipe, built, size, memory)
        .map(|stage| Drawing::Stage(Box::new(stage)))
        .map_err(|error| format!("stage: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Manfred;
    use crate::{Script, gpu_turn, scratch, theme};
    use manfred_build::Registry;
    use pfx_editor_shell::{drive, pixels};
    use std::sync::Mutex;
    use std::sync::mpsc;
    use std::time::Duration;

    const SCENE: &str = "manfred = 1\nname = \"fake\"\n\n[[object]]\nname = \"a\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n\n[[camera]]\nname = \"main\"\nposition = [0.0, 0.5, 2.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 30.0\n";

    fn centre(rgba: &[u8], size: [u32; 2], shell: &Shell) -> [u8; 4] {
        let image = shell.viewport().unwrap();
        let mut seen: std::collections::BTreeMap<[u8; 4], usize> = Default::default();
        for row in 0..5 {
            for column in 0..5 {
                let at = image.min
                    + image.size()
                        * egui::Vec2::new(0.3 + 0.03 * column as f32, 0.3 + 0.03 * row as f32);
                let index = ((at.y as u32 * size[0] + at.x as u32) * 4) as usize;
                *seen
                    .entry(rgba[index..index + 4].try_into().unwrap())
                    .or_default() += 1;
            }
        }
        seen.into_iter().max_by_key(|(_, count)| *count).unwrap().0
    }

    fn rgba(colour: egui::Color32) -> [u8; 4] {
        [colour.r(), colour.g(), colour.b(), 255]
    }

    #[test]
    fn the_shell_draws_frames_while_the_stage_is_made() {
        let path = scratch("viewer-worker");
        std::fs::write(&path, SCENE).unwrap();
        let mut viewer = Viewer::new(&path, Mode::Fake);
        let (woke, wakes) = mpsc::channel();
        viewer.background(move || {
            let _ = woke.send(());
        });
        let (open, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        viewer.hold(move || {
            let _ = gate.lock().unwrap().recv_timeout(Duration::from_secs(20));
        });
        let mut shell = Shell::open(&path.display().to_string());
        let ctx = egui::Context::default();
        theme::apply_theme(&ctx, &theme::manfred());
        let size = [1586, 992];
        let mut passes = 0;
        for _ in 0..12 {
            drive(&ctx, size, &Script::default(), |ui| {
                shell.ui(ui);
                viewer.stage(None, size);
                viewer.sync(&mut shell);
                passes += 1;
            });
            assert!(viewer.staging());
            assert!(shell.busy());
            assert_eq!(shell.view_error(), None);
            assert_eq!(
                shell.preparing(),
                Some(
                    "making the stage: the renderer, its pipelines, the sky and the meshes of 1 object"
                )
            );
        }
        assert_eq!(passes, 24);
        assert!(viewer.drawing.is_none());
        open.send(()).unwrap();
        wakes.recv_timeout(Duration::from_secs(20)).unwrap();
        viewer.stage(None, size);
        viewer.sync(&mut shell);
        assert!(!viewer.staging());
        assert!(matches!(viewer.drawing, Some(Drawing::Fake(_))));
        assert_eq!(shell.preparing(), None);
        assert!(!shell.busy());
    }

    #[test]
    fn an_inline_viewer_stages_at_once() {
        let path = scratch("viewer-inline");
        std::fs::write(&path, SCENE).unwrap();
        let mut viewer = Viewer::new(&path, Mode::Fake);
        let mut shell = Shell::open(&path.display().to_string());
        viewer.sync(&mut shell);
        assert_eq!(shell.view_error(), Some("nothing to draw yet"));
        assert_eq!(shell.preparing(), None);
        viewer.stage(None, [64, 40]);
        assert!(!viewer.staging());
        assert!(matches!(viewer.drawing, Some(Drawing::Fake(_))));
    }

    #[test]
    fn a_recipe_being_read_is_preparing_not_an_error() {
        let path = scratch("viewer-reading");
        std::fs::write(&path, SCENE).unwrap();
        let mut viewer = Viewer::idle(&path, Mode::Fake);
        let mut shell = Shell::open(&path.display().to_string());
        let (done, finished) = mpsc::channel();
        viewer.request(&mut shell, move || {
            let _ = done.send(());
        });
        assert!(viewer.follow().busy());
        assert_eq!(shell.preparing(), Some("reading and building the recipe"));
        assert_eq!(shell.view_error(), None);
        finished.recv_timeout(Duration::from_secs(20)).unwrap();
        viewer.settle(&mut shell);
        assert_eq!(shell.preparing(), None);
        assert_eq!(shell.view_error(), Some("nothing to draw yet"));
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn the_fake_viewport_follows_the_orbit() {
        let since = std::time::Instant::now();
        let path = scratch("viewer-fake");
        std::fs::write(&path, SCENE).unwrap();
        let size = [1586, 992];
        let mut viewer = Viewer::new(&path, Mode::Fake);
        let mut shell = Shell::open(&path.display().to_string());
        let still = pixels(
            &mut Manfred::lent(&mut shell, &mut viewer),
            size,
            &Script::default(),
        )
        .unwrap();
        let home = shell.eye().unwrap();
        assert_eq!(centre(&still, size, &shell), rgba(Fake::colour(&home)));
        let script = Script::parse("drag 700 400 1000 400").unwrap();
        let turned = pixels(&mut Manfred::lent(&mut shell, &mut viewer), size, &script).unwrap();
        let eye = shell.eye().unwrap();
        assert_ne!(eye, home);
        assert_ne!(Fake::colour(&eye), Fake::colour(&home));
        assert_eq!(centre(&turned, size, &shell), rgba(Fake::colour(&eye)));
        let again = pixels(
            &mut Manfred::lent(&mut shell, &mut viewer),
            size,
            &Script::default(),
        )
        .unwrap();
        assert_eq!(again, turned);
        gpu_turn(since);
    }

    fn pass(
        ctx: &egui::Context,
        gpu: &Gpu,
        painter: &mut pfx_editor_shell::paint::Painter,
        shell: &mut Shell,
        viewer: &mut Viewer,
    ) {
        drive(ctx, [800, 500], &Script::default(), |ui| {
            shell.ui(ui);
            viewer.frame(gpu, painter.renderer(), shell, ui.ctx().pixels_per_point());
        });
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_rebuild_swaps_one_mesh_in_the_stage_it_keeps() {
        let since = std::time::Instant::now();
        let path = scratch("viewer-swap");
        let two = format!(
            "{}\n[[object]]\nname = \"b\"\nkind = \"box\"\nmaterial = \"wood\"\nsize = [0.5, 0.5, 0.5]\nat = [1.0, 0.0, 0.0]\n\n[materials.orange]\nfamily = \"lacquer\"\nbase = [0.73, 0.06, 0.0]\nroughness = 0.45\nclearcoat = 1.0\nclearcoat_roughness = 0.07\n\n[materials.wood]\nfamily = \"wood\"\nbase = [0.11, 0.042, 0.016]\nroughness = 0.42\n",
            SCENE.replace(
                "kind = \"box\"\n",
                "kind = \"box\"\nmaterial = \"orange\"\n"
            )
        );
        std::fs::write(&path, &two).unwrap();
        let mut viewer = Viewer::new(&path, Mode::Stage);
        let (woke, wakes) = mpsc::channel();
        viewer.background(move || {
            let _ = woke.send(());
        });
        let gpu = manfred_scene::block_on(Gpu::headless()).unwrap();
        let mut painter = pfx_editor_shell::paint::Painter::new(
            &gpu,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            egui_wgpu::RendererOptions::PREDICTABLE,
        );
        let mut shell = Shell::open(&path.display().to_string());
        let ctx = egui::Context::default();
        theme::apply_theme(&ctx, &theme::manfred());
        pass(&ctx, &gpu, &mut painter, &mut shell, &mut viewer);
        assert!(viewer.staging());
        assert!(shell.preparing().unwrap().starts_with("making the stage"));
        while viewer.staging() {
            wakes.recv_timeout(Duration::from_secs(60)).unwrap();
            pass(&ctx, &gpu, &mut painter, &mut shell, &mut viewer);
        }
        assert!(viewer.showing(), "{:?}", shell.view_error());
        let before = viewer.mesh_stats().unwrap();
        assert_eq!(before.live, 2);
        std::fs::write(
            &path,
            two.replace("at = [1.0, 0.0, 0.0]", "at = [1.2, 0.0, 0.0]"),
        )
        .unwrap();
        viewer.reload(&mut shell);
        pass(&ctx, &gpu, &mut painter, &mut shell, &mut viewer);
        assert!(viewer.showing());
        while viewer.staging() {
            wakes.recv_timeout(Duration::from_secs(60)).unwrap();
            pass(&ctx, &gpu, &mut painter, &mut shell, &mut viewer);
        }
        assert!(!viewer.staging());
        assert_eq!(
            viewer.change(),
            Some(Change {
                kept: 1,
                replaced: 1,
                added: 0,
                released: 0,
                sky: false,
            })
        );
        assert_eq!(viewer.mesh_stats(), Some(before));
        gpu_turn(since);
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_hundred_edits_in_the_window_keep_the_mesh_buffers_flat() {
        let since = std::time::Instant::now();
        let path = scratch("viewer-hundred");
        let two = format!(
            "{}\n[[object]]\nname = \"b\"\nkind = \"box\"\nmaterial = \"wood\"\nsize = [0.5, 0.5, 0.5]\nat = [1.0, 0.0, 0.0]\n\n[materials.orange]\nfamily = \"lacquer\"\nbase = [0.73, 0.06, 0.0]\nroughness = 0.45\nclearcoat = 1.0\nclearcoat_roughness = 0.07\n\n[materials.wood]\nfamily = \"wood\"\nbase = [0.11, 0.042, 0.016]\nroughness = 0.42\n",
            SCENE.replace(
                "kind = \"box\"\n",
                "kind = \"box\"\nmaterial = \"orange\"\n"
            )
        );
        let edits = [
            two.clone(),
            two.replace("at = [1.0, 0.0, 0.0]", "at = [1.2, 0.0, 0.0]"),
            format!(
                "{two}\n[[object]]\nname = \"c\"\nkind = \"box\"\nmaterial = \"wood\"\nsize = [0.25, 0.25, 0.25]\nat = [-1.0, 0.0, 0.0]\n"
            ),
            two.replace("size = [0.5, 0.5, 0.5]", "size = [0.5, 0.8, 0.5]"),
        ];
        std::fs::write(&path, &edits[0]).unwrap();
        let mut viewer = Viewer::new(&path, Mode::Stage);
        let (woke, wakes) = mpsc::channel();
        viewer.background(move || {
            let _ = woke.send(());
        });
        let gpu = manfred_scene::block_on(Gpu::headless()).unwrap();
        let mut painter = pfx_editor_shell::paint::Painter::new(
            &gpu,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            egui_wgpu::RendererOptions::PREDICTABLE,
        );
        let mut shell = Shell::open(&path.display().to_string());
        let ctx = egui::Context::default();
        theme::apply_theme(&ctx, &theme::manfred());
        pass(&ctx, &gpu, &mut painter, &mut shell, &mut viewer);
        while viewer.staging() {
            wakes.recv_timeout(Duration::from_secs(60)).unwrap();
            pass(&ctx, &gpu, &mut painter, &mut shell, &mut viewer);
        }
        assert!(viewer.showing(), "{:?}", shell.view_error());
        let first = viewer.mesh_stats().unwrap();
        assert_eq!(first.live, 2);
        let per_mesh = first.buffers / first.live;
        assert!(per_mesh >= 1, "{first:?}");
        let mut seen = Vec::new();
        let (mut work, mut longest) = (0.0_f64, 0.0_f64);
        for edit in 1..=100 {
            std::fs::write(&path, &edits[edit % edits.len()]).unwrap();
            viewer.reload(&mut shell);
            pass(&ctx, &gpu, &mut painter, &mut shell, &mut viewer);
            while viewer.staging() {
                wakes.recv_timeout(Duration::from_secs(60)).unwrap();
                pass(&ctx, &gpu, &mut painter, &mut shell, &mut viewer);
            }
            pass(&ctx, &gpu, &mut painter, &mut shell, &mut viewer);
            assert!(viewer.showing(), "{:?}", shell.view_error());
            seen.push(viewer.mesh_stats().unwrap());
            let ms = viewer.frame_ms().unwrap_or(5.0);
            work += ms * 3.0;
            longest = longest.max(ms);
            if work >= 40.0 {
                let _ = std::process::Command::new("pgpu")
                    .args([
                        "turn",
                        "--work-ms",
                        &format!("{:.0}", work.ceil()),
                        "--longest-ms",
                        &format!("{:.0}", longest.ceil()),
                    ])
                    .status();
                (work, longest) = (0.0, 0.0);
            }
        }
        println!(
            "mesh_stats in the window: first {first:?}, after a hundred edits {:?}, frame {:?} ms, {} timings dropped",
            seen[99],
            viewer.frame_ms(),
            viewer.timing_drops()
        );
        assert!(
            seen.iter()
                .all(|stats| stats.buffers == per_mesh * stats.live),
            "{seen:?}"
        );
        assert!(seen.iter().all(|stats| stats.slots <= 3), "{seen:?}");
        assert_eq!(seen[99].buffers, first.buffers);
        assert!(viewer.frame_ms().is_some_and(|ms| ms > 0.0));
        gpu_turn(since);
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_stage_that_cannot_draw_shows_why_in_the_viewport() {
        let since = std::time::Instant::now();
        let path = scratch("viewer-error");
        std::fs::write(
            &path,
            SCENE.replace(
                "kind = \"box\"\nsize = [1.0, 1.0, 1.0]",
                "kind = \"mesh\"\npath = \"absent.glb\"",
            ),
        )
        .unwrap();
        let size = [1586, 992];
        let mut viewer = Viewer::new(&path, Mode::Stage);
        let mut shell = Shell::open(&path.display().to_string());
        pixels(
            &mut Manfred::lent(&mut shell, &mut viewer),
            size,
            &Script::default(),
        )
        .unwrap();
        assert!(shell.view_error().is_some());
        assert_eq!(shell.document().objects.len(), 1);
        gpu_turn(since);
    }

    const PAIR: &str = "manfred = 1\nname = \"pair\"\n\n[[object]]\nname = \"left\"\nkind = \"box\"\nsize = [0.4, 0.4, 0.4]\nat = [-0.6, 0.0, 0.0]\n\n[[object]]\nname = \"right\"\nkind = \"box\"\nsize = [0.4, 0.4, 0.4]\nat = [0.6, 0.0, 0.0]\n\n[[object]]\nname = \"lid\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\nparent = \"right\"\nat = [0.0, 0.3, 0.0]\n\n[[camera]]\nname = \"front\"\nposition = [0.0, 0.5, 3.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 40.0\n";
    const WIDE: [u32; 2] = [1586, 992];

    struct Bench {
        ctx: egui::Context,
        shell: Shell,
        viewer: Viewer,
        time: f64,
    }

    impl Bench {
        fn new(name: &str) -> Bench {
            Bench::with(name, PAIR)
        }

        fn with(name: &str, recipe: &str) -> Bench {
            Bench::over(name, recipe, Registry::new())
        }

        fn over(name: &str, recipe: &str, registry: Registry) -> Bench {
            let path = scratch(name);
            std::fs::write(&path, recipe).unwrap();
            let mut viewer = Viewer::with(Follow::new_with(&path, Mode::Fake, registry));
            let mut shell = Shell::open(&path.display().to_string());
            viewer.sync(&mut shell);
            let ctx = egui::Context::default();
            theme::apply_theme(&ctx, &theme::manfred());
            let mut bench = Bench {
                ctx,
                shell,
                viewer,
                time: 0.0,
            };
            bench.script("frame");
            bench.pass(Vec::new(), egui::Modifiers::NONE);
            bench
        }

        fn pass(&mut self, events: Vec<egui::Event>, modifiers: egui::Modifiers) {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::new(WIDE[0] as f32, WIDE[1] as f32),
                )),
                time: Some(self.time),
                predicted_dt: 1.0 / 60.0,
                focused: true,
                events: std::iter::once(egui::Event::ModifiersChanged(modifiers))
                    .chain(events)
                    .collect(),
                ..egui::RawInput::default()
            };
            self.time += 1.0 / 60.0;
            let (shell, viewer) = (&mut self.shell, &mut self.viewer);
            self.ctx
                .run_ui(input, |ui| {
                    shell.ui(ui);
                    viewer.stage(None, WIDE);
                    viewer.point(shell);
                })
                .textures_delta
                .clear();
        }

        fn script(&mut self, text: &str) {
            let ctx = egui::Context::default();
            theme::apply_theme(&ctx, &theme::manfred());
            let (shell, viewer) = (&mut self.shell, &mut self.viewer);
            drive(&ctx, WIDE, &Script::parse(text).unwrap(), |ui| {
                shell.ui(ui);
                viewer.stage(None, WIDE);
                viewer.point(shell);
            });
        }

        fn lens(&self) -> Lens {
            self.shell.lens().unwrap()
        }

        fn at(&self, point: [f64; 3]) -> egui::Pos2 {
            self.lens().project(glam::DVec3::from_array(point)).unwrap()
        }

        fn object(&self, name: &str) -> usize {
            self.shell.document().object(name).unwrap()
        }

        fn key(&mut self, key: egui::Key) {
            let event = |pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            };
            self.pass(vec![event(true), event(false)], egui::Modifiers::NONE);
        }
    }

    fn handle_x(bench: &Bench, object: [f64; 3]) -> (egui::Pos2, f64) {
        let lens = bench.lens();
        let centre = glam::DVec3::from_array(object);
        let mpp = lens.metres_per_point(centre);
        let along = centre + glam::DVec3::X * mpp * 60.0;
        (lens.project(along).unwrap(), mpp)
    }

    fn patch_text(patch: &manfred_recipe::Patch) -> String {
        patch.value.as_ref().unwrap().to_string().trim().to_string()
    }

    #[test]
    fn a_click_on_text_selects_the_text_and_beside_it_the_object_it_sits_on() {
        let words = format!(
            "{PAIR}\n[[object]]\nname = \"label\"\nkind = \"text\"\non = \"right\"\nspans = [{{ text = \"label\", size = 0.05 }}]\n\n[[object]]\nname = \"title\"\nkind = \"text\"\nspace = \"overlay\"\nat = [24.0, 24.0, 0.0]\nspans = [{{ text = \"pair\", size = 32.0 }}]\n"
        );
        let mut bench = Bench::with("pick-text", &words);
        let label = bench.at([0.6, 0.0, 0.2]);
        bench.script(&format!("click {} {}\nframe", label.x, label.y));
        assert_eq!(bench.shell.selected(), Some(bench.object("label")));
        let beside = bench.at([0.75, 0.15, 0.2]);
        bench.script(&format!("click {} {}\nframe", beside.x, beside.y));
        assert_eq!(bench.shell.selected(), Some(bench.object("right")));
        let image = bench.shell.viewport().unwrap();
        bench.script(&format!(
            "click {} {}\nframe",
            image.left() + 40.0,
            image.top() + 40.0
        ));
        assert_eq!(bench.shell.selected(), Some(bench.object("title")));
        bench.script(&format!(
            "click {} {}\nframe",
            image.right() - 4.0,
            image.bottom() - 4.0
        ));
        assert_eq!(bench.shell.selected(), None);
        assert!(bench.shell.edits().is_empty());
    }

    fn slab(_: &manfred_build::Call) -> Result<Vec<manfred_build::Piece>, String> {
        let mut piece = manfred_build::Piece {
            name: "slab".into(),
            mesh: Default::default(),
        };
        piece.mesh.positions = vec![
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ];
        piece.mesh.normals = vec![[0.0, 0.0, 1.0]; 4];
        piece.mesh.indices = vec![0, 1, 2, 0, 2, 3];
        Ok(vec![piece])
    }

    #[test]
    fn a_fake_click_picks_generator_objects_through_the_apps_registry() {
        let recipe = "manfred = 1\nname = \"gen\"\n\n[[object]]\nname = \"pad\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\nat = [0.9, 0.0, 0.0]\n\n[[object]]\nname = \"slab\"\nkind = \"generator\"\ngenerator = \"test/slab\"\n\n[[camera]]\nname = \"main\"\nposition = [0.0, 0.5, 2.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 30.0\n";
        let mut registry = Registry::new();
        registry.register("test/slab", slab);
        let mut bench = Bench::over("pick-generator", recipe, registry);
        assert_eq!(bench.shell.selected(), Some(bench.object("pad")));
        let at = bench.at([0.0, 0.0, 0.0]);
        bench.script(&format!("click {} {}\nframe", at.x, at.y));
        assert_eq!(bench.shell.selected(), Some(bench.object("slab")));
        let mut plain = Bench::with("pick-generator-plain", recipe);
        plain.script(&format!("click {} {}\nframe", at.x, at.y));
        assert_eq!(plain.shell.selected(), None);
    }

    #[test]
    fn a_click_selects_what_is_under_it_and_a_click_on_nothing_clears() {
        let mut bench = Bench::new("gizmo-click");
        let right = bench.at([0.65, 0.05, 0.2]);
        let [x, y] = [right.x, right.y];
        bench.script(&format!("click {x} {y}\nframe"));
        assert_eq!(bench.shell.selected(), Some(bench.object("right")));
        assert_eq!(bench.shell.focus(), crate::panel::Panel::C1);
        let left = bench.at([-0.55, 0.05, 0.2]);
        bench.script(&format!("click {} {}\nframe", left.x, left.y));
        assert_eq!(bench.shell.selected(), Some(bench.object("left")));
        let lid = bench.at([0.62, 0.31, 0.05]);
        bench.script(&format!("click {} {}\nframe", lid.x, lid.y));
        assert_eq!(bench.shell.selected(), Some(bench.object("lid")));
        let image = bench.shell.viewport().unwrap();
        bench.script(&format!(
            "click {} {}\nframe",
            image.left() + 4.0,
            image.top() + 4.0
        ));
        assert_eq!(bench.shell.selected(), None);
        assert!(bench.shell.edits().is_empty());
    }

    #[test]
    fn a_drag_on_the_x_arrow_moves_the_preview_and_pushes_one_gesture() {
        let mut bench = Bench::new("gizmo-drag");
        let right = bench.object("right");
        let lid = bench.object("lid");
        bench.shell.picked(Some(right));
        let eye = bench.shell.eye();
        let (from, mpp) = handle_x(&bench, [0.6, 0.0, 0.0]);
        let to = from + egui::Vec2::new(50.0, 0.0);
        bench.script(&format!(
            "drag {} {} {} {}\nframe",
            from.x, from.y, to.x, to.y
        ));
        assert_eq!(bench.shell.eye(), eye);
        let (object, motion) = bench.shell.preview().unwrap();
        assert_eq!(object, right);
        let metres = 50.0 * mpp;
        assert!(
            (f64::from(motion[3][0]) - metres).abs() < 1e-5,
            "{motion:?}"
        );
        assert!(motion[3][1].abs() < 1e-6);
        let gestures = bench.shell.edits().take();
        assert_eq!(gestures.len(), 1);
        assert_eq!(gestures[0].label, "move right along x");
        assert_eq!(gestures[0].patches.len(), 1);
        let x = (0.6 + metres) * 1e6;
        assert_eq!(
            patch_text(&gestures[0].patches[0]),
            format!("[{}, 0.0, 0.0]", x.round() / 1e6)
        );
        let shown = bench.shell.gizmo().shown(right).unwrap().centre();
        assert!((shown.x - (0.6 + metres)).abs() < 1e-9);
        assert_eq!(
            bench.shell.gizmo().shown(lid),
            bench.shell.gizmo().poses().get(lid)
        );
        std::fs::write(
            bench.viewer.follow().path(),
            PAIR.replace(
                "at = [0.6, 0.0, 0.0]",
                &format!("at = [{}, 0.0, 0.0]", x.round() / 1e6),
            ),
        )
        .unwrap();
        bench.viewer.reload(&mut bench.shell);
        assert_eq!(bench.shell.preview(), None);
        let moved = bench.shell.gizmo().poses().get(right).unwrap().centre();
        assert!((moved.x - (0.6 + metres)).abs() < 1e-6);
    }

    fn walk(from: egui::Pos2, along: f32, steps: u32) -> String {
        (1..=steps)
            .map(|step| format!("move {} {}\nframe\n", from.x + along * step as f32, from.y))
            .collect()
    }

    #[test]
    fn esc_in_the_middle_of_a_drag_restores_and_the_rest_of_the_drag_does_nothing() {
        let mut bench = Bench::new("gizmo-esc-held");
        let right = bench.object("right");
        bench.shell.picked(Some(right));
        let (from, _) = handle_x(&bench, [0.6, 0.0, 0.0]);
        bench.script(&format!(
            "press {} {}\n{}",
            from.x,
            from.y,
            walk(from, 10.0, 4)
        ));
        assert!(bench.shell.gizmo().dragging().is_some());
        assert!(bench.shell.preview().is_some());

        let mut bench = Bench::new("gizmo-esc");
        let right = bench.object("right");
        bench.shell.picked(Some(right));
        let eye = bench.shell.eye();
        let (from, _) = handle_x(&bench, [0.6, 0.0, 0.0]);
        let end = from + egui::Vec2::new(80.0, 30.0);
        bench.script(&format!(
            "press {} {}\n{}key Escape\nframe\nmove {} {}\nframe\nrelease {} {}\nframe\nframe",
            from.x,
            from.y,
            walk(from, 10.0, 4),
            end.x,
            end.y,
            end.x,
            end.y
        ));
        assert_eq!(bench.shell.preview(), None);
        assert_eq!(bench.shell.eye(), eye);
        assert!(bench.shell.edits().is_empty());
        assert_eq!(bench.shell.selected(), Some(right));
        assert!(!bench.shell.gizmo().swallowing());
    }

    #[test]
    fn ctrl_snaps_a_drag_to_whole_millimetres() {
        let mut bench = Bench::new("gizmo-snap");
        let right = bench.object("right");
        bench.shell.picked(Some(right));
        let (from, mpp) = handle_x(&bench, [0.6, 0.0, 0.0]);
        let moved = (0.0537 / mpp) as f32;
        bench.script(&format!(
            "press {} {} ctrl\n{}release {} {}\nframe",
            from.x,
            from.y,
            walk(from, moved / 4.0, 4),
            from.x + moved,
            from.y
        ));
        let gestures = bench.shell.edits().take();
        assert_eq!(patch_text(&gestures[0].patches[0]), "[0.654, 0.0, 0.0]");
    }

    #[test]
    fn keys_switch_the_tool_and_the_space() {
        let mut bench = Bench::new("gizmo-keys");
        bench.shell.picked(Some(bench.object("right")));
        assert_eq!(bench.shell.gizmo().mode, crate::gizmo::Mode::Move);
        bench.key(egui::Key::R);
        assert_eq!(bench.shell.gizmo().mode, crate::gizmo::Mode::Rotate);
        bench.key(egui::Key::S);
        assert_eq!(bench.shell.gizmo().mode, crate::gizmo::Mode::Scale);
        bench.key(egui::Key::L);
        assert_eq!(bench.shell.gizmo().space, crate::gizmo::Space::Local);
        bench.key(egui::Key::G);
        assert_eq!(bench.shell.gizmo().mode, crate::gizmo::Mode::Move);
        bench.key(egui::Key::L);
        assert_eq!(bench.shell.gizmo().space, crate::gizmo::Space::World);
    }

    #[test]
    fn a_scale_drag_on_the_centre_box_scales_the_child_in_its_own_key() {
        let mut bench = Bench::new("gizmo-scale");
        let lid = bench.object("lid");
        bench.shell.picked(Some(lid));
        bench.key(egui::Key::S);
        let centre = bench.at([0.6, 0.3, 0.0]);
        let to = centre + egui::Vec2::new(40.0, 0.0);
        bench.script(&format!(
            "drag {} {} {} {}\nframe",
            centre.x, centre.y, to.x, to.y
        ));
        let gestures = bench.shell.edits().take();
        assert_eq!(gestures[0].label, "scale lid");
        assert_eq!(patch_text(&gestures[0].patches[0]), "[1.5, 1.5, 1.5]");
        let (object, motion) = bench.shell.preview().unwrap();
        assert_eq!(object, lid);
        assert!((motion[0][0] - 1.5).abs() < 1e-6);
        assert!(
            (motion[3][0] - 0.6 * (1.0 - 1.5)).abs() < 1e-5,
            "{motion:?}"
        );
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn the_shot_shows_the_gizmo_mid_drag_and_a_pick_on_the_bench_selects_the_ball() {
        let since = std::time::Instant::now();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = scratch("gizmo-bench");
        std::fs::copy(root.join("tests/golden/table/table.toml"), &path).unwrap();
        let mut viewer = Viewer::new(&path, Mode::Stage);
        let mut shell = Shell::open(&path.display().to_string());
        viewer.sync(&mut shell);
        let gpu = manfred_scene::block_on(Gpu::headless()).unwrap();
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut painter = pfx_editor_shell::paint::Painter::new(
            &gpu,
            format,
            egui_wgpu::RendererOptions::PREDICTABLE,
        );
        let ctx = egui::Context::default();
        theme::apply_theme(&ctx, &theme::manfred());
        let mut time = 0.0;
        let textures = std::cell::RefCell::new(egui::TexturesDelta::default());
        let mut frame = |events: Vec<egui::Event>,
                         shell: &mut Shell,
                         viewer: &mut Viewer,
                         painter: &mut pfx_editor_shell::paint::Painter| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::new(WIDE[0] as f32, WIDE[1] as f32),
                )),
                time: Some(time),
                predicted_dt: 1.0 / 60.0,
                focused: true,
                events,
                ..egui::RawInput::default()
            };
            time += 1.0 / 60.0;
            let mut output = ctx.run_ui(input, |ui| {
                shell.ui(ui);
                viewer.frame(&gpu, painter.renderer(), shell, ui.ctx().pixels_per_point());
            });
            textures
                .borrow_mut()
                .append(std::mem::take(&mut output.textures_delta));
            output
        };
        for _ in 0..3 {
            frame(Vec::new(), &mut shell, &mut viewer, &mut painter);
        }
        assert!(viewer.showing(), "{:?}", shell.view_error());
        let lens = shell.lens().unwrap();
        let ball = shell.document().object("ball").unwrap();
        let centre = glam::DVec3::new(0.03, 0.042, -0.03);
        let at = lens.project(centre).unwrap();
        let button = |at: egui::Pos2, pressed: bool| {
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        frame(button(at, true), &mut shell, &mut viewer, &mut painter);
        frame(button(at, false), &mut shell, &mut viewer, &mut painter);
        for _ in 0..20 {
            if shell.selected() == Some(ball) {
                break;
            }
            frame(Vec::new(), &mut shell, &mut viewer, &mut painter);
        }
        assert_eq!(shell.selected(), Some(ball));
        let mpp = lens.metres_per_point(centre);
        let from = lens.project(centre + glam::DVec3::X * mpp * 60.0).unwrap();
        frame(button(from, true), &mut shell, &mut viewer, &mut painter);
        let mut output = None;
        for step in 1..=6 {
            let to = from + egui::Vec2::new(10.0 * step as f32, 0.0);
            output = Some(frame(
                vec![egui::Event::PointerMoved(to)],
                &mut shell,
                &mut viewer,
                &mut painter,
            ));
        }
        assert!(shell.preview().is_some());
        let Some(Drawing::Stage(stage)) = &viewer.drawing else {
            panic!("no stage");
        };
        assert_eq!(stage.moved(), [ball]);
        let output = output.unwrap();
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        let target = gpu.offscreen(WIDE[0], WIDE[1], format).unwrap();
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: WIDE,
            pixels_per_point: output.pixels_per_point,
        };
        painter.paint(
            &gpu,
            &target.view,
            &screen,
            &textures.borrow(),
            &primitives,
            theme::BG_BASE,
        );
        let rgba = gpu.readback_rgba8(&target).unwrap();
        let png = crate::encode_png(WIDE, &rgba).unwrap();
        std::fs::write(root.join("tmp/gizmo-drag.png"), png).unwrap();
        let last = from + egui::Vec2::new(60.0, 0.0);
        frame(button(last, false), &mut shell, &mut viewer, &mut painter);
        let gestures = shell.edits().take();
        assert_eq!(gestures.len(), 1);
        assert_eq!(gestures[0].label, "move ball along x");
        let mut loaded = manfred_recipe::load(&path).unwrap();
        for patch in &gestures[0].patches {
            manfred_recipe::apply(&mut loaded, patch).unwrap();
        }
        manfred_recipe::save_to(&loaded, &root.join("tmp/gizmo-bench.toml")).unwrap();
        textures.borrow_mut().clear();
        gpu_turn(since);
    }

    fn scene_pass(shell: &mut Shell, viewer: &mut Viewer, text: &str) {
        let ctx = egui::Context::default();
        theme::apply_theme(&ctx, &theme::manfred());
        drive(&ctx, WIDE, &Script::parse(text).unwrap(), |ui| {
            shell.ui(ui);
            viewer.stage(None, WIDE);
            viewer.sync(shell);
            viewer.point(shell);
        });
    }

    fn watching(name: &str) -> (std::path::PathBuf, Shell, Viewer) {
        let path = crate::watched::fixture(name);
        let viewer = Viewer::scene(&path, Mode::Fake);
        let shell = Shell::open(&path.display().to_string());
        (path, shell, viewer)
    }

    #[test]
    fn a_scene_file_opens_in_the_fake_window_with_its_outliner_and_camera() {
        let (path, mut shell, mut viewer) = watching("scene-open");
        assert!(viewer.is_scene() && shell.is_scene());
        assert!(Viewer::with(Follow::idle(&path, Mode::Fake)).is_scene());
        scene_pass(&mut shell, &mut viewer, "frame");
        assert!(matches!(viewer.drawing, Some(Drawing::Fake(_))));
        assert_eq!(shell.trouble(), None);
        let rows: Vec<&str> = shell
            .document()
            .nodes
            .iter()
            .map(|node| node.label.as_str())
            .collect();
        assert_eq!(rows, ["floor", "plinth", "ball", "crate"]);
        let eye = shell.eye().unwrap();
        assert_eq!(eye.position, [0.0, 1.2, 3.2]);
        assert_eq!(eye.target, [0.0, 0.35, 0.0]);
        assert_eq!(shell.shown_camera().unwrap().name, "camera");
        assert!(
            viewer
                .watched()
                .iter()
                .any(|file| file.ends_with("stand.materials.toml"))
        );
        assert!(!viewer.stale());
    }

    #[test]
    fn an_edit_saved_on_disk_reloads_between_frames() {
        let (path, mut shell, mut viewer) = watching("scene-reload");
        scene_pass(&mut shell, &mut viewer, "frame");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            text.replace("at = [0.7, 0.2, 0.3]", "at = [0.9, 0.2, 0.3]"),
        )
        .unwrap();
        scene_pass(&mut shell, &mut viewer, "frame");
        assert_eq!(viewer.reloads(), 1);
        assert_eq!(shell.document().objects[3].location, [0.9, 0.2, 0.3]);
        let library = path.with_file_name("stand.materials.toml");
        let materials = std::fs::read_to_string(&library).unwrap();
        std::fs::write(
            &library,
            materials.replace("roughness = 0.3", "roughness = 0.6"),
        )
        .unwrap();
        scene_pass(&mut shell, &mut viewer, "frame");
        assert_eq!(viewer.reloads(), 2);
        scene_pass(&mut shell, &mut viewer, "frame");
        assert_eq!(viewer.reloads(), 2);
        assert_eq!(shell.trouble(), None);
    }

    #[test]
    fn a_broken_save_keeps_the_last_view_and_reports_its_line_in_d1() {
        let (path, mut shell, mut viewer) = watching("scene-broken");
        scene_pass(&mut shell, &mut viewer, "frame");
        let good = std::fs::read_to_string(&path).unwrap();
        let line = good.lines().position(|line| line == "[sky]").unwrap() + 2;
        std::fs::write(&path, good.replace("[sky]\n", "[sky]\nshade = 0.5\n")).unwrap();
        scene_pass(&mut shell, &mut viewer, "frame");
        let trouble = shell.trouble().unwrap().to_string();
        assert!(
            trouble.starts_with(&format!("stand.scene.toml:{line}:")),
            "{trouble}"
        );
        assert!(matches!(viewer.drawing, Some(Drawing::Fake(_))));
        assert_eq!(shell.document().objects.len(), 4);
        assert_eq!(shell.view_error(), None);
        std::fs::write(&path, &good).unwrap();
        scene_pass(&mut shell, &mut viewer, "frame");
        assert_eq!(shell.trouble(), None);
        assert_eq!(viewer.reloads(), 1);
    }

    #[test]
    fn a_click_in_the_fake_view_picks_the_scenes_objects_by_their_ids() {
        let path = crate::watched::fixture("scene-pick");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            text.replace("scale = 0.4\n", "scale = 0.4\npick = 40\n"),
        )
        .unwrap();
        let mut viewer = Viewer::scene(&path, Mode::Fake);
        let mut shell = Shell::open(&path.display().to_string());
        scene_pass(&mut shell, &mut viewer, "frame");
        let lens = shell.lens().unwrap();
        let at = |point: [f64; 3]| lens.project(glam::DVec3::from_array(point)).unwrap();
        let ball = at([-0.5, 0.7, 0.0]);
        scene_pass(
            &mut shell,
            &mut viewer,
            &format!("click {} {}\nframe", ball.x, ball.y),
        );
        assert_eq!(shell.selected(), Some(2));
        let crate_top = at([0.7, 0.4, 0.3]);
        scene_pass(
            &mut shell,
            &mut viewer,
            &format!("click {} {}\nframe", crate_top.x, crate_top.y),
        );
        assert_eq!(shell.selected(), Some(3));
        let image = shell.viewport().unwrap();
        scene_pass(
            &mut shell,
            &mut viewer,
            &format!("click {} {}\nframe", image.left() + 4.0, image.top() + 4.0),
        );
        assert_eq!(shell.selected(), None);
        assert!(shell.edits().is_empty());
    }

    #[test]
    fn an_orbit_keeps_the_camera_it_left_for_the_lens() {
        let recipe = PAIR.replace("fov_y_deg = 40.0\n", "fov_y_deg = 40.0\nfstop = 2.8\n");
        let mut bench = Bench::with("lens-orbit", &recipe);
        assert_eq!(bench.shell.shown_camera().unwrap().fstop, 2.8);
        let image = bench.shell.viewport().unwrap();
        let from = image.left_top() + egui::Vec2::new(30.0, 30.0);
        bench.script(&format!(
            "drag {} {} {} {}\nframe",
            from.x,
            from.y,
            from.x + 120.0,
            from.y
        ));
        assert_ne!(
            bench.shell.eye(),
            Some(Eye::from_camera(
                bench.shell.shown_camera().unwrap(),
                1586.0 / 992.0
            ))
        );
        assert_eq!(bench.shell.shown_camera().unwrap().name, "front");
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn the_fake_shot_of_a_scene_file_follows_its_camera() {
        let since = std::time::Instant::now();
        let (_, mut shell, mut viewer) = watching("scene-fake-shot");
        let still = pixels(
            &mut Manfred::lent(&mut shell, &mut viewer),
            WIDE,
            &Script::default(),
        )
        .unwrap();
        let home = shell.eye().unwrap();
        assert_eq!(home.position, [0.0, 1.2, 3.2]);
        assert_eq!(centre(&still, WIDE, &shell), rgba(Fake::colour(&home)));
        gpu_turn(since);
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_scene_file_draws_through_staged_and_reloads_a_material_in_place() {
        let since = std::time::Instant::now();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = crate::watched::fixture("scene-stage");
        let mut viewer = Viewer::scene(&path, Mode::Stage);
        let mut shell = Shell::open(&path.display().to_string());
        let before = crate::shot(
            &mut crate::Manfred::lent(&mut shell, &mut viewer),
            WIDE,
            &Script::default(),
        )
        .unwrap();
        std::fs::write(root.join("tmp/scene-watch.png"), &before).unwrap();
        assert!(viewer.showing(), "{:?}", shell.view_error());
        let Some(Drawing::Scene(_)) = &viewer.drawing else {
            panic!("the scene is not drawn through Staged");
        };
        let stats = viewer.mesh_stats().unwrap();
        assert_eq!(stats.live, 2);
        gpu_turn(since);
        let since = std::time::Instant::now();
        let gpu = manfred_scene::block_on(Gpu::headless()).unwrap();
        let mut painter = pfx_editor_shell::paint::Painter::new(
            &gpu,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            egui_wgpu::RendererOptions::PREDICTABLE,
        );
        let ctx = egui::Context::default();
        theme::apply_theme(&ctx, &theme::manfred());
        viewer.forget();
        pass(&ctx, &gpu, &mut painter, &mut shell, &mut viewer);
        pass(&ctx, &gpu, &mut painter, &mut shell, &mut viewer);
        assert!(viewer.showing(), "{:?}", shell.view_error());
        let library = path.with_file_name("stand.materials.toml");
        let materials = std::fs::read_to_string(&library).unwrap();
        std::fs::write(
            &library,
            materials.replace("base = [0.70, 0.32, 0.22]", "base = [0.15, 0.35, 0.75]"),
        )
        .unwrap();
        let started = std::time::Instant::now();
        assert!(viewer.reload(&mut shell));
        let reloaded = started.elapsed();
        pass(&ctx, &gpu, &mut painter, &mut shell, &mut viewer);
        let drawn = started.elapsed();
        println!(
            "a one-material edit: re-read, diffed and applied in {:.2} ms, drawn after {:.2} ms",
            reloaded.as_secs_f64() * 1000.0,
            drawn.as_secs_f64() * 1000.0
        );
        assert_eq!(viewer.reloads(), 1);
        assert_eq!(viewer.mesh_stats(), Some(stats));
        assert_eq!(shell.trouble(), None);
        let good = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, good.replace("[camera]\n", "[camera]\nlens = 2\n")).unwrap();
        assert!(!viewer.reload(&mut shell));
        pass(&ctx, &gpu, &mut painter, &mut shell, &mut viewer);
        assert!(viewer.showing());
        assert!(
            shell.trouble().unwrap().contains("stand.scene.toml:"),
            "{:?}",
            shell.trouble()
        );
        gpu_turn(since);
        let since = std::time::Instant::now();
        let after = crate::shot(
            &mut crate::Manfred::lent(&mut shell, &mut viewer),
            WIDE,
            &Script::default(),
        )
        .unwrap();
        std::fs::write(root.join("tmp/scene-watch-edit.png"), &after).unwrap();
        assert_ne!(before, after);
        gpu_turn(since);
    }
}
