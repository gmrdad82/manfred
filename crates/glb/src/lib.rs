mod compare;
mod reader;
mod sidecars;
mod writer;

pub use compare::diff;
pub use reader::summary;
pub use sidecars::{
    FACTS, SidecarWriter, facts, grade_json, merge, mover_json, room_json, sidecar_text,
};
pub use writer::{write, write_tree, write_with_extras};

pub use glam::DMat4;
use manfred_mesh::Mesh;
use serde::Serialize;
use std::fmt;

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub mesh: Mesh,
    pub transform: DMat4,
    pub keep_transform: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    File,
    Natural,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraFacts {
    pub name: String,
    pub orthographic: bool,
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
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SunFacts {
    pub toward: [f64; 3],
    pub radius_deg: f64,
    pub color: [f64; 3],
    pub irradiance: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkyFacts {
    pub intensity: f64,
    pub rotation_deg: f64,
    pub balance: Option<[f64; 3]>,
    pub room: Option<RoomFacts>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoomFacts {
    pub width: u32,
    pub floor: [f64; 3],
    pub wall: [f64; 3],
    pub ceiling: [f64; 3],
    pub lights: Vec<LampFacts>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LampFacts {
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DaylightFacts {
    pub hour: f64,
    pub day: u32,
    pub latitude: f64,
    pub heading: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradeFacts {
    pub vignette: f64,
    pub saturation: f64,
    pub warmth: f64,
    pub bloom: f64,
    pub haze: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MoverFacts {
    pub group: String,
    pub pivot: [f64; 3],
    pub axis: [f64; 3],
    pub travel: f64,
    pub clip: Option<[f64; 4]>,
    pub clip2: Option<[f64; 4]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LookFacts {
    pub exposure: f64,
    pub grade: GradeFacts,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneFacts {
    pub cameras: Vec<CameraFacts>,
    pub sun: Option<SunFacts>,
    pub daylight: Option<DaylightFacts>,
    pub sky: Option<SkyFacts>,
    pub plate: Option<[f64; 2]>,
    pub movers: Vec<MoverFacts>,
    pub look: Option<LookFacts>,
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NodeSummary {
    pub name: String,
    pub index: usize,
    pub triangles: usize,
    pub vertices: usize,
    pub bounds: [[f64; 3]; 2],
    pub area: f64,
    pub slots: Vec<String>,
    pub uvs: bool,
    pub uvs1: bool,
    pub uv1_area: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extras: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Summary {
    pub generator: Option<String>,
    pub nodes: Vec<NodeSummary>,
    pub materials: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct NodeDiff {
    pub name: String,
    pub marks: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Report {
    pub nodes: Vec<NodeDiff>,
    pub only_a: Vec<String>,
    pub only_b: Vec<String>,
    pub order_differs: bool,
    pub materials_differ: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub materials_a: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub materials_b: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}
