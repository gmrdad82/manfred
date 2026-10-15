pub mod content;
mod geometry;
pub mod light;
pub mod raster;
mod scene;
mod sidecar;
pub mod sky;
mod vector;

pub use scene::{
    Camera, Grade, Lamp, Motion, MoverSpec, Piece, Room, Scene, Sky, SkyChoice, Sun, Tri,
    mover_group,
};

pub use content::{Fill, NoContent, Paint, Painter, Picker, Route, Source};
pub use light::{Bake, Bird, Canopy, Framing, Layout, Light, Lighting, MoteLook, SteamLook, Wind};
pub use raster::{IDENTITY, Lens, Look, Parts, Plan, Raster, View};
pub use sidecar::{Reader, Sidecar, facts};

use pfx_load::Mesh;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Glb(String),
    Sidecar(String),
    Read { path: PathBuf, reason: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Glb(reason) => write!(f, "glb: {reason}"),
            Error::Sidecar(reason) => write!(f, "scene json: {reason}"),
            Error::Read { path, reason } => write!(f, "{}: {reason}", path.display()),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    pub fn wanted(key: &str) -> Error {
        Error::Sidecar(format!("wanted `{key}`"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub sky: SkyChoice,
    pub fill: f32,
    pub aspect: Option<f32>,
}

impl Default for Options {
    fn default() -> Self {
        Options::with_sky(None, None)
    }
}

impl Options {
    pub fn with_sky(choice: Option<&str>, fill: Option<&str>) -> Options {
        Options {
            sky: SkyChoice::parse(choice),
            fill: sidecar::fill(fill),
            aspect: None,
        }
    }
}

pub fn load(glb: &[u8], sidecar: &str) -> Result<Scene, Error> {
    load_with(glb, sidecar, &Options::default())
}

pub fn load_with(glb: &[u8], sidecar: &str, options: &Options) -> Result<Scene, Error> {
    load_read_with(glb, sidecar, options, facts)
}

pub fn load_read(glb: &[u8], sidecar: &str, reader: impl Reader) -> Result<Scene, Error> {
    load_read_with(glb, sidecar, &Options::default(), reader)
}

pub fn load_read_with(
    glb: &[u8],
    sidecar: &str,
    options: &Options,
    reader: impl Reader,
) -> Result<Scene, Error> {
    let meta = sidecar::parse(sidecar)?;
    let mesh = Mesh::parse(glb).map_err(Error::Glb)?;
    let read = reader.read(&meta, options)?;
    let movers = sidecar::with_extras(read.movers, &mesh)?;
    let geometry = geometry::build(&mesh)?;
    Ok(Scene {
        tris: geometry.tris,
        pieces: geometry.pieces,
        parts: geometry.parts,
        materials: geometry.materials,
        objects: geometry.objects,
        groups: geometry.groups,
        camera: read.camera,
        sun: read.sun,
        sky: read.sky,
        room: read.room,
        exposure: read.exposure,
        grade: read.grade,
        plate: read.plate,
        motion: read.motion,
        movers,
    })
}

pub fn load_files(glb: &Path, sidecar: &Path) -> Result<Scene, Error> {
    load_files_with(glb, sidecar, &Options::default())
}

pub fn load_files_with(glb: &Path, sidecar: &Path, options: &Options) -> Result<Scene, Error> {
    load_files_read_with(glb, sidecar, options, facts)
}

pub fn load_files_read(glb: &Path, sidecar: &Path, reader: impl Reader) -> Result<Scene, Error> {
    load_files_read_with(glb, sidecar, &Options::default(), reader)
}

pub fn load_files_read_with(
    glb: &Path,
    sidecar: &Path,
    options: &Options,
    reader: impl Reader,
) -> Result<Scene, Error> {
    let bytes = read(glb)?;
    let text = String::from_utf8(read(sidecar)?).map_err(|e| Error::Read {
        path: sidecar.to_path_buf(),
        reason: e.to_string(),
    })?;
    let mut scene = load_read_with(&bytes, &text, options, reader)?;
    if let Some(file) = scene.sky.hdr_file.clone() {
        let beside = sidecar.parent().unwrap_or(Path::new(""));
        scene.sky.hdr = Some(read(&beside.join(file))?);
        scene.sky.choice = SkyChoice::Hdr;
    }
    Ok(scene)
}

pub fn load_dir(dir: &Path, name: &str) -> Result<Scene, Error> {
    load_dir_with(dir, name, &Options::default())
}

pub fn load_dir_with(dir: &Path, name: &str, options: &Options) -> Result<Scene, Error> {
    load_dir_read_with(dir, name, options, facts)
}

pub fn load_dir_read(dir: &Path, name: &str, reader: impl Reader) -> Result<Scene, Error> {
    load_dir_read_with(dir, name, &Options::default(), reader)
}

pub fn load_dir_read_with(
    dir: &Path,
    name: &str,
    options: &Options,
    reader: impl Reader,
) -> Result<Scene, Error> {
    load_files_read_with(
        &dir.join(format!("{name}.glb")),
        &dir.join(format!("{name}.json")),
        options,
        reader,
    )
}

fn read(path: &Path) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(|e| Error::Read {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })
}
