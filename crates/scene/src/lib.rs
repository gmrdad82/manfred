use manfred_build::{Built, Part, Registry};
use manfred_recipe::{Camera, Recipe};
use pfx::adapter::{TurnState, turn_soon};
use pfx_gpu::{Gpu, OffscreenTarget, wgpu};
use std::fmt;
use std::path::Path;

mod cards;
mod eye;
mod light;
mod maps;
mod memory;
mod parts;
mod picture;
mod stage;
mod text;
mod vector;

pub use cards::{
    Card, Cards, Face, Picture, clips, decode, facing, layered, layered_as, linear, placeholder,
    premultiplied,
};
pub use light::{Lighting, Lit, Shine};
pub use maps::LARGEST_MAP;
pub use memory::Memory;
pub use parts::{Palette, Piece, glb_parts, material, palette, palette_with, pieces};
use pfx_live::frame::Matrix;
pub use pfx_live::frame::MeshStats;
pub use picture::{difference, psnr, read_png, read_png_size, write_frame, write_png};
pub use stage::{Change, Pick, Prepared, Stage, family_of};
pub use text::{Laid, fonts};

pub const SETTLE_FRAMES: u32 = 48;
pub const IDENTITY: Matrix = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];
pub const FRAME_RATE: f32 = 60.0;
pub const UNMEASURED_MS: f64 = 50.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Eye {
    pub position: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    pub fov_y_deg: f64,
    pub shift: [f64; 2],
    pub ortho_height: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub fn still(recipe: &Path, camera: &str, size: [u32; 2]) -> Result<Vec<u8>, Error> {
    still_with(recipe, camera, size, None, &Registry::new())
}

pub fn still_with(
    recipe: &Path,
    camera: &str,
    size: [u32; 2],
    variant: Option<&str>,
    registry: &Registry,
) -> Result<Vec<u8>, Error> {
    let (recipe, built) = load(recipe, variant, &[], registry)?;
    let gpu = headless()?;
    shoot(&gpu, &recipe, &built.parts, camera, size)
}

pub fn load(
    path: &Path,
    variant: Option<&str>,
    set: &[String],
    registry: &Registry,
) -> Result<(Recipe, Built), Error> {
    load_at(path, variant, set, registry, None)
}

pub fn poses(built: &Built, fps: u32, time: f64) -> std::collections::BTreeMap<String, Matrix> {
    let mut posed = std::collections::BTreeMap::new();
    for track in built.tracks.iter().filter(|track| track.camera.is_none()) {
        if track.keys.is_empty() {
            continue;
        }
        let world = manfred_build::matrix(&manfred_build::sample(&track.keys, fps, time));
        for part in built
            .parts
            .iter()
            .filter(|part| part.node.name == track.node)
        {
            let model = if part.node.keep_transform {
                world * part.node.transform.inverse()
            } else {
                world
            };
            let model = model
                .to_cols_array_2d()
                .map(|column| column.map(|v| v as f32));
            posed.insert(part.node.name.clone(), model);
        }
    }
    posed
}

pub fn posed(recipe: &Recipe, built: &Built, time: f64) -> Result<(Recipe, Built), Error> {
    manfred_build::pose(recipe, built, time).map_err(|e| Error(e.to_string()))
}

pub fn load_at(
    path: &Path,
    variant: Option<&str>,
    set: &[String],
    registry: &Registry,
    time: Option<f64>,
) -> Result<(Recipe, Built), Error> {
    let failed = |e: manfred_recipe::Error| Error(e.to_string());
    let loaded = manfred_recipe::load(path).map_err(failed)?;
    let recipe = match variant {
        Some(name) => manfred_recipe::apply_variant(&loaded.recipe, name).map_err(failed)?,
        None => loaded.recipe,
    };
    let recipe = manfred_recipe::apply_set(&recipe, set).map_err(failed)?;
    let base = path.parent().unwrap_or(Path::new(""));
    let built =
        manfred_build::build_with(&recipe, base, registry).map_err(|e| Error(e.to_string()))?;
    let recipe = built.recipe(&recipe);
    match time {
        Some(time) => posed(&recipe, &built, time),
        None => Ok((recipe, built)),
    }
}

pub fn seed(recipe: &Recipe) -> u32 {
    (recipe.seed & u64::from(u32::MAX)) as u32
}

pub fn headless() -> Result<Gpu, Error> {
    block_on(Gpu::headless()).map_err(|e| Error(format!("no GPU: {e}")))
}

pub fn camera<'a>(recipe: &'a Recipe, name: &str) -> Result<&'a Camera, Error> {
    recipe
        .cameras
        .iter()
        .find(|camera| camera.name == name)
        .ok_or_else(|| {
            let known: Vec<&str> = recipe.cameras.iter().map(|c| c.name.as_str()).collect();
            Error(if known.is_empty() {
                format!("camera.{name}: the recipe has no cameras")
            } else {
                format!(
                    "camera.{name}: no such camera, expected one of: {}",
                    known.join(", ")
                )
            })
        })
}

pub fn shoot(
    gpu: &Gpu,
    recipe: &Recipe,
    parts: &[Part],
    camera_name: &str,
    size: [u32; 2],
) -> Result<Vec<u8>, Error> {
    let lens = camera(recipe, camera_name)?;
    let mut reel = Reel::new(gpu, recipe, parts, size)?;
    reel.stage.look_through(Some(lens));
    reel.shoot(&reel.eye(lens)).map(|(rgba, _)| rgba)
}

pub struct Reel {
    stage: Stage,
    target: OffscreenTarget,
    turns: TurnState,
    size: [u32; 2],
}

impl Reel {
    pub fn new(gpu: &Gpu, recipe: &Recipe, parts: &[Part], size: [u32; 2]) -> Result<Reel, Error> {
        if size[0] == 0 || size[1] == 0 {
            return Err(Error(format!("the size {}x{} is empty", size[0], size[1])));
        }
        let stage = Stage::with_parts(gpu.clone(), recipe, parts, size)?;
        let target = gpu
            .offscreen(size[0], size[1], wgpu::TextureFormat::Rgba8UnormSrgb)
            .map_err(Error)?;
        Ok(Reel {
            stage,
            target,
            turns: TurnState::default(),
            size,
        })
    }

    pub fn pose(&mut self, recipe: &Recipe, parts: &[Part]) -> Result<(), Error> {
        self.stage.apply(Prepared::of(recipe, parts)?).map(|_| ())
    }

    pub fn preview(&mut self, object: usize, motion: Matrix) -> Result<(), Error> {
        self.stage.preview(object, motion)
    }

    pub fn set_poses(&mut self, posed: std::collections::BTreeMap<String, Matrix>) -> bool {
        self.stage.set_poses(posed)
    }

    pub fn eye(&self, camera: &Camera) -> Eye {
        Eye::from_camera(camera, f64::from(self.size[0]) / f64::from(self.size[1]))
    }

    pub fn shoot(&mut self, eye: &Eye) -> Result<(Vec<u8>, f64), Error> {
        let mut gpu_ms = 0.0;
        for frame in 0..SETTLE_FRAMES {
            let measured =
                self.stage
                    .render_measured(eye, frame as f32 / FRAME_RATE, &self.target.view)?;
            gpu_ms += measured.unwrap_or(0.0);
            turn_soon(&mut self.turns, measured.unwrap_or(UNMEASURED_MS));
        }
        let rgba = self
            .stage
            .gpu()
            .readback_rgba8(&self.target)
            .map_err(Error)?;
        Ok((rgba, gpu_ms))
    }
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::yield_now();
    }
}
