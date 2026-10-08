use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

mod include;
mod patch;
mod path;
mod read;
mod set;
mod text;
mod validate;
mod variant;

pub use include::Files;
pub use patch::{
    apply, apply_all, edit, flag, number, numbers, params_in, path_text, reread, save, save_to,
    string, value_at, write,
};
pub use path::{CameraPath, Ease, Orbit, PathKey, PathKind};
pub use read::{load, materials, parse};
pub use set::apply_set;
pub use text::{ANCHORS, MONO, anchor_names, anchor_point, default_anchor};
pub use validate::{bake_product, missing_material_text, slug, validate};
pub use variant::apply_variant;

pub use toml_edit;

pub const SCHEMA: u32 = 1;
pub const OLD_HEADER: &str =
    "`manf = 1` is the old header; Manfred recipes start with `manfred = 1`";
pub const NO_SIDECAR: &str = "none";
pub const CAPPED: &str = "capped";
pub const CAP_LUMINANCE: f64 = 60.0;
pub const CAP_MEAN: f64 = 0.3;
pub const REFERENCE_HOUR: f64 = 16.0;
pub const SCENE_SUFFIX: &str = ".scene.toml";
const SENSOR_MM: f64 = 36.0;
const FOV_Y_DEG: f64 = 45.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub manfred: u32,
    pub name: String,
    pub seed: u64,
    pub params: Option<String>,
    pub plate: Option<Plate>,
    pub output: Output,
    pub objects: Vec<Object>,
    pub cameras: Vec<Camera>,
    pub sun: Option<Sun>,
    pub daylight: Option<Daylight>,
    pub sky: Option<Sky>,
    pub lights: Vec<Light>,
    pub movers: Vec<Mover>,
    pub variants: BTreeMap<String, Variant>,
    pub sidecar: serde_json::Map<String, serde_json::Value>,
    pub look: Option<Look>,
    pub trace: Trace,
    pub bake: Option<Bake>,
    pub materials: BTreeMap<String, serde_json::Value>,
    pub uses: Vec<Use>,
    pub files: Files,
    pub source: String,
    pub base: Option<PathBuf>,
    pub lines: Lines,
    pub twins: Vec<Problem>,
}

#[derive(Clone, Debug, Default)]
pub struct Lines(pub BTreeMap<String, usize>);

impl PartialEq for Lines {
    fn eq(&self, _: &Lines) -> bool {
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Centre,
    TopLeft,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plate {
    pub width: f64,
    pub height: f64,
    pub metres_per_pixel: f64,
    pub origin: Origin,
    pub camera: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    File,
    Natural,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub glb: Option<String>,
    pub sidecar: Option<String>,
    pub sidecar_dialect: String,
    pub order: Order,
    pub bake: Option<String>,
    pub hierarchy: bool,
    pub scene: Option<String>,
    pub scene_camera: Option<String>,
    pub scene_format: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shadow {
    Cast,
    Only,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub name: String,
    pub id: Option<String>,
    pub kind: Kind,
    pub at: [f64; 3],
    pub rotate: [f64; 3],
    pub scale: [f64; 3],
    pub keep_transform: bool,
    pub parent: Option<String>,
    pub material: Option<String>,
    pub materials: Vec<String>,
    pub smooth_angle: Option<f64>,
    pub smooth: bool,
    pub flat_faces: bool,
    pub bevel: Option<Bevel>,
    pub uv: Option<Uv>,
    pub detail_uv: Option<DetailUv>,
    pub shadow: Shadow,
    pub two_sided: bool,
    pub hidden: bool,
    pub dynamic: Option<bool>,
    pub proxy: Option<Proxy>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proxy {
    Box,
    Hull,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caps {
    Ngon,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSpace {
    Surface,
    Overlay,
    Lit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub family: Option<String>,
    pub size: f64,
    pub weight: Option<u32>,
    pub italic: bool,
    pub color: [f64; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Box {
        size: [f64; 3],
    },
    Cylinder {
        radius: f64,
        depth: f64,
        segments: u32,
        caps: Caps,
    },
    Sphere {
        radius: f64,
        segments: u32,
        rings: u32,
    },
    Plane {
        size: [f64; 2],
        grid: Option<[u32; 2]>,
        bend: Option<Bend>,
        curl: Option<Curl>,
        cup: Option<Cup>,
        jag: Vec<f64>,
    },
    Disc {
        radius: f64,
        segments: u32,
    },
    Torus {
        major: f64,
        minor: f64,
        segments: u32,
        rings: u32,
    },
    Capsule {
        radius: f64,
        length: f64,
    },
    Outline {
        outline: Outline,
        holes: Vec<Outline>,
        front: f64,
        back: f64,
        profile: Profile,
    },
    Pocket {
        into: String,
        outline: Outline,
        depth: f64,
        rim: Option<Profile>,
        floor: Option<Profile>,
    },
    Lathe {
        profile: Vec<[f64; 2]>,
        segments: u32,
        weld: f64,
    },
    Sweep {
        path: Vec<[f64; 3]>,
        radii: Vec<f64>,
        ring: u32,
        closed: bool,
        caps: bool,
        along: Option<Along>,
    },
    Mesh {
        path: String,
        node: Option<String>,
    },
    Generator {
        generator: String,
        params: BTreeMap<String, serde_json::Value>,
    },
    Card {
        size: [f64; 2],
        image: Option<String>,
        slot: Option<String>,
        face_camera: bool,
        alpha_cutoff: f64,
    },
    Text {
        spans: Vec<Span>,
        wrap: Option<f64>,
        anchor: Option<String>,
        space: TextSpace,
        on: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BendForm {
    Wrap,
    Lift,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bend {
    pub radius: f64,
    pub form: BendForm,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Curl {
    pub width: f64,
    pub depth: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cup {
    pub from: f64,
    pub depth: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Along {
    pub outline: Outline,
    pub offset: f64,
    pub depth: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outline {
    Rect {
        rect: [f64; 4],
        radius: f64,
        turned: f64,
        segments: u32,
    },
    Circle {
        centre: [f64; 2],
        radius: f64,
        segments: u32,
    },
    Points {
        points: Vec<[f64; 3]>,
        segments: u32,
    },
    Union(Vec<Outline>),
    Difference(Box<Outline>, Box<Outline>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileKind {
    Round,
    Chamfer,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub kind: ProfileKind,
    pub width: f64,
    pub segments: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DetailUv {
    pub texels_per_m: f64,
    pub gutter: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bevel {
    pub width: f64,
    pub segments: u32,
    pub angle: f64,
    pub clamp: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Uv {
    Plate,
    Local,
    None,
    Cylinder {
        degrees: f64,
        atlas: [f64; 4],
        band: Option<[f64; 2]>,
        wall: Option<[f64; 2]>,
        away: [f64; 2],
    },
    Rect {
        rect: [f64; 4],
        atlas: [f64; 4],
        facing: f64,
        clamp: bool,
        away: [f64; 2],
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    Perspective,
    Orthographic,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    pub name: String,
    pub kind: Projection,
    pub position: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    pub fov_y_deg: Option<f64>,
    pub focal_mm: Option<f64>,
    pub sensor_mm: Option<f64>,
    pub shift: [f64; 2],
    pub focus_m: Option<f64>,
    pub fstop: f64,
    pub ortho_height: Option<f64>,
    pub path: Option<CameraPath>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sun {
    pub toward: [f64; 3],
    pub radius_deg: f64,
    pub color: [f64; 3],
    pub irradiance: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Daylight {
    pub hour: f64,
    pub day: u32,
    pub latitude: f64,
    pub heading: f64,
    pub reference_hour: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkyKind {
    Hdr,
    Analytic,
    Room,
    Mix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkyAmbient {
    Authored,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SkyLight {
    pub name: String,
    pub az: f64,
    pub el: f64,
    pub power: f64,
    pub color: [f64; 3],
    pub width: f64,
    pub height: f64,
    pub soft: f64,
    pub slats: f64,
    pub open: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Room {
    pub width: u32,
    pub floor: [f64; 3],
    pub wall: [f64; 3],
    pub ceiling: [f64; 3],
    pub lights: Vec<SkyLight>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sky {
    pub kind: SkyKind,
    pub path: Option<String>,
    pub source: Option<String>,
    pub prepare: Option<String>,
    pub rotation_deg: f64,
    pub intensity: f64,
    pub balance: Option<[f64; 3]>,
    pub fill: Option<f64>,
    pub ambient: Option<SkyAmbient>,
    pub shade: BTreeMap<u32, [f64; 3]>,
    pub room: Option<Room>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    pub name: String,
    pub position: [f64; 3],
    pub color: [f64; 3],
    pub intensity: f64,
    pub radius: f64,
    pub range: f64,
    pub shadow: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mover {
    pub group: String,
    pub pivot: [f64; 3],
    pub axis: [f64; 3],
    pub travel: f64,
    pub clip: Option<[f64; 4]>,
    pub clip2: Option<[f64; 4]>,
    pub parts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub drop: Vec<String>,
    pub set: BTreeMap<String, serde_json::Value>,
    pub remove: Vec<String>,
    pub output: VariantOutput,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VariantOutput {
    pub glb: Option<String>,
    pub sidecar: Option<String>,
    pub bake: Option<String>,
    pub scene: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Grade {
    pub vignette: f64,
    pub saturation: f64,
    pub warmth: f64,
    pub bloom: f64,
    pub haze: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Graded,
    Standard,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub view: View,
    pub exposure: f64,
    pub grade: Grade,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Trace {
    pub transmissive_shadows: bool,
    pub clamp_indirect: f64,
    pub filter_glossy: f64,
}

impl Trace {
    pub const CLAMP_INDIRECT_MAX: f64 = 10_000.0;
    pub const FILTER_GLOSSY_MAX: f64 = 1.0;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Volume {
    pub name: String,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub spacing: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bake {
    pub product: Option<String>,
    pub anchors: Vec<f64>,
    pub samples: u32,
    pub volumes: Vec<Volume>,
    pub plates: Option<Plates>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Casters {
    Meshes,
    Proxies,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plates {
    pub size: [u32; 2],
    pub overscan: f64,
    pub scale: f64,
    pub anchors: Option<Vec<f64>>,
    pub layers: Vec<String>,
    pub threshold: f64,
    pub min_samples: u32,
    pub max_samples: u32,
    pub growth: f64,
    pub seed: Option<u32>,
    pub key: Option<String>,
    pub dir: Option<String>,
    pub casters: Casters,
    pub clamp_indirect: Option<f64>,
    pub filter_glossy: Option<f64>,
    pub views: Vec<PlateView>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlateView {
    pub name: String,
    pub camera: Option<String>,
    pub at: Option<[f64; 3]>,
    pub look_at: Option<[f64; 3]>,
    pub up: Option<[f64; 3]>,
    pub fov: Option<f64>,
    pub focal: Option<f64>,
    pub sensor: Option<f64>,
    pub shift: Option<[f64; 2]>,
    pub near: Option<f64>,
    pub far: Option<f64>,
}

impl Plates {
    pub const OVERSCAN: f64 = 0.1;
    pub const SCALE: f64 = 1.0;
    pub const LAYERS: [&str; 6] = ["color", "depth", "normal", "id", "sun", "transfer"];
    pub const THRESHOLD: f64 = 0.01;
    pub const MIN_SAMPLES: u32 = 16;
    pub const MAX_SAMPLES: u32 = 1024;
    pub const GROWTH: f64 = 2.0;
    pub const SAMPLES_CAP: u32 = 65536;
    pub const SIDE: [u32; 2] = [16, 16384];
    pub const DEFAULT_VIEW: &str = "main";

    pub fn all_layers(&self) -> bool {
        self.layers.len() == Plates::LAYERS.len()
            && Plates::LAYERS
                .iter()
                .all(|name| self.layers.iter().any(|layer| layer == name))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Use {
    pub prefix: String,
    pub file: String,
    pub at: [f64; 3],
    pub rotate: [f64; 3],
    pub scale: [f64; 3],
    pub parent: Option<String>,
    pub objects: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Loaded {
    pub recipe: Recipe,
    pub doc: toml_edit::DocumentMut,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Table(String),
    Object(String),
    Index(usize),
}

#[derive(Clone, Debug)]
pub struct Patch {
    pub path: Vec<Key>,
    pub value: Option<toml_edit::Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub path: String,
    pub line: Option<usize>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub path: String,
    pub line: Option<usize>,
    pub text: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.path.is_empty()) {
            (Some(line), false) => write!(f, "line {line}: {}: {}", self.path, self.text),
            (Some(line), true) => write!(f, "line {line}: {}", self.text),
            (None, false) => write!(f, "{}: {}", self.path, self.text),
            (None, true) => write!(f, "{}", self.text),
        }
    }
}

impl std::error::Error for Error {}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}: {}", self.path, self.text),
            None => write!(f, "{}: {}", self.path, self.text),
        }
    }
}

impl Error {
    pub(crate) fn new(
        path: impl Into<String>,
        line: Option<usize>,
        text: impl Into<String>,
    ) -> Error {
        Error {
            path: path.into(),
            line,
            text: text.into(),
        }
    }
}

impl Kind {
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Box { .. } => "box",
            Kind::Cylinder { .. } => "cylinder",
            Kind::Sphere { .. } => "sphere",
            Kind::Plane { .. } => "plane",
            Kind::Disc { .. } => "disc",
            Kind::Torus { .. } => "torus",
            Kind::Capsule { .. } => "capsule",
            Kind::Outline { .. } => "outline",
            Kind::Pocket { .. } => "pocket",
            Kind::Lathe { .. } => "lathe",
            Kind::Sweep { .. } => "sweep",
            Kind::Mesh { .. } => "mesh",
            Kind::Generator { .. } => "generator",
            Kind::Card { .. } => "card",
            Kind::Text { .. } => "text",
        }
    }
}

impl Camera {
    pub fn fov_y_deg(&self, aspect: f64) -> f64 {
        match (self.fov_y_deg, self.focal_mm) {
            (Some(fov), _) => fov,
            (None, Some(focal)) if focal > 0.0 => {
                let longest = self.sensor_mm.unwrap_or(SENSOR_MM) / (2.0 * focal);
                2.0 * (longest / aspect.max(1.0)).atan().to_degrees()
            }
            _ => FOV_Y_DEG,
        }
    }
}

impl Plate {
    fn zero(&self) -> [f64; 2] {
        match self.origin {
            Origin::Centre => [0.0, 0.0],
            Origin::TopLeft => [self.width / 2.0, self.height / 2.0],
        }
    }

    pub fn to_pixels(&self, at: [f64; 2]) -> [f64; 2] {
        let [ox, oy] = self.zero();
        [
            at[0] / self.metres_per_pixel + ox,
            -at[1] / self.metres_per_pixel + oy,
        ]
    }

    pub fn to_metres(&self, px: [f64; 2]) -> [f64; 2] {
        let [ox, oy] = self.zero();
        [
            (px[0] - ox) * self.metres_per_pixel,
            -(px[1] - oy) * self.metres_per_pixel,
        ]
    }
}

impl Mover {
    pub fn owns(&self, object: &str) -> bool {
        object
            .strip_prefix("mv_")
            .and_then(|rest| rest.strip_prefix(self.group.as_str()))
            .is_some_and(|rest| rest.is_empty() || rest.starts_with("__"))
            || self.parts.iter().any(|part| part == object)
    }
}

impl Output {
    pub fn writes_sidecar(&self) -> bool {
        self.sidecar_dialect != NO_SIDECAR
    }
}

impl Recipe {
    pub fn dynamic(&self, index: usize) -> bool {
        let mut at = index;
        for _ in 0..=self.objects.len() {
            let Some(object) = self.objects.get(at) else {
                return false;
            };
            if let Some(dynamic) = object.dynamic {
                return dynamic;
            }
            let turns = matches!(
                object.kind,
                Kind::Card {
                    face_camera: true,
                    ..
                }
            );
            if turns || self.movers.iter().any(|mover| mover.owns(&object.name)) {
                return true;
            }
            let carrier = match (&object.parent, &object.kind) {
                (Some(parent), _) => Some(parent),
                (None, Kind::Text { on, .. }) => on.as_ref(),
                _ => None,
            };
            match carrier.and_then(|name| self.objects.iter().position(|o| &o.name == name)) {
                Some(next) => at = next,
                None => return false,
            }
        }
        false
    }

    pub fn line_of(&self, index_path: &str) -> Option<usize> {
        let mut key = index_path;
        loop {
            if let Some(line) = self.lines.0.get(key) {
                return Some(*line);
            }
            let cut = key.rfind('.')?;
            key = &key[..cut];
        }
    }
}
