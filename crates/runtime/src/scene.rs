#[derive(Debug, Clone, PartialEq)]
pub struct Tri {
    pub p: [[f32; 3]; 3],
    pub n: [[f32; 3]; 3],
    pub uv: [[f32; 2]; 3],
    pub material: u32,
    pub object: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub material: u32,
    pub object: u32,
    pub part: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub fov_y: f32,
    pub shift: [f32; 2],
    pub aperture: f32,
    pub focus: f32,
    pub orthographic: bool,
    pub ortho_height: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sun {
    pub toward: [f32; 3],
    pub radius_deg: f32,
    pub color: [f32; 3],
    pub irradiance: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SkyChoice {
    #[default]
    Room,
    Hdr,
    Mix,
}

impl SkyChoice {
    pub fn parse(choice: Option<&str>) -> SkyChoice {
        match choice {
            Some("daylight" | "hdr") => SkyChoice::Hdr,
            Some("mix") => SkyChoice::Mix,
            _ => SkyChoice::Room,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sky {
    pub choice: SkyChoice,
    pub fill: f32,
    pub balance: Option<[f32; 3]>,
    pub intensity: f32,
    pub rotation_deg: f32,
    pub hdr_file: Option<String>,
    pub hdr: Option<Vec<u8>>,
}

impl Sky {
    pub fn rotation(&self) -> f32 {
        self.rotation_deg.to_radians()
    }
}

pub type Lamp = pfx_load::RoomLamp;

pub type Room = pfx_load::RoomSky;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grade {
    pub vignette: f32,
    pub saturation: f32,
    pub warmth: f32,
    pub bloom: f32,
    pub haze: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Motion {
    pub cup: [f32; 3],
    pub radius: f32,
    pub lo: [f32; 3],
    pub hi: [f32; 3],
    pub density: f32,
    pub anisotropy: f32,
    pub dust: f32,
    pub mote: f32,
    pub ambient: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MoverSpec {
    pub name: String,
    pub pivot: [f32; 3],
    pub axis: [f32; 3],
    pub travel: f32,
    pub clip: [f32; 4],
    pub clip2: [f32; 4],
}

pub fn mover_group(object: &str) -> Option<&str> {
    object
        .strip_prefix("mv_")
        .map(|rest| rest.split("__").next().unwrap_or(rest))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub tris: Vec<Tri>,
    pub pieces: Vec<Piece>,
    pub parts: u32,
    pub materials: Vec<String>,
    pub objects: Vec<String>,
    pub groups: Vec<Option<String>>,
    pub camera: Camera,
    pub sun: Sun,
    pub sky: Sky,
    pub room: Room,
    pub exposure: f32,
    pub grade: Grade,
    pub plate: Option<(f32, f32)>,
    pub motion: Option<Motion>,
    pub movers: Vec<MoverSpec>,
}

impl Scene {
    pub fn mover(&self, group: &str) -> Option<&MoverSpec> {
        self.movers.iter().find(|mover| mover.name == group)
    }

    pub fn group_of(&self, object: u32) -> Option<&str> {
        let name = self.objects.get(object as usize)?;
        match self.groups.get(object as usize) {
            Some(Some(group)) => Some(group),
            _ => mover_group(name),
        }
    }

    pub fn mover_of(&self, object: u32) -> Option<&MoverSpec> {
        self.mover(self.group_of(object)?)
    }
}
