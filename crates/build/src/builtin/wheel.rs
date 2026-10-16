use crate::{Call, Generated, Piece};
use glam::{DMat4, DQuat, DVec3};
use manfred_mesh::math::Real;
use manfred_mesh::{self as mesh, Mesh, Slot};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, TAU};

pub const NAME: &str = "manfred/wheel";

const PARAMS: [&str; 5] = ["rim", "tyre", "brake", "side", "segments"];
const RIM_KEYS: [&str; 10] = [
    "diameter_in",
    "width_in",
    "spokes",
    "twin",
    "spoke_width",
    "taper",
    "concavity",
    "lip",
    "cap",
    "metal",
];
const BRAKE_KEYS: [&str; 2] = ["disc", "caliper"];

const INCH: f64 = 0.0254;
const TYRE_DEFAULT: &str = "245/40R19";
const METAL_DEFAULT: &str = "#c4c8cc";
const CALIPER_DEFAULT: &str = "#b3121b";

const TYRE: &str = "wheel_tyre";
const CAP: &str = "wheel_cap";
const DISC: &str = "wheel_disc";

const CALIPER_ANGLE_DEG: f64 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tyre {
    pub width_mm: u32,
    pub aspect: u32,
    pub rim_in: u32,
}

impl Tyre {
    pub fn parse(code: &str) -> Option<Tyre> {
        let (width, rest) = code.trim().split_once('/')?;
        let (aspect, rim) = rest.split_once(['R', 'r'])?;
        let whole = |s: &str| {
            (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                .then(|| s.parse::<u32>().ok())
                .flatten()
        };
        let tyre = Tyre {
            width_mm: whole(width)?,
            aspect: whole(aspect)?,
            rim_in: whole(rim)?,
        };
        ((105..=405).contains(&tyre.width_mm)
            && (20..=95).contains(&tyre.aspect)
            && (13..=26).contains(&tyre.rim_in))
        .then_some(tyre)
    }

    pub fn width(&self) -> f64 {
        f64::from(self.width_mm) / 1000.0
    }

    pub fn sidewall(&self) -> f64 {
        self.width() * f64::from(self.aspect) / 100.0
    }

    pub fn rim_radius(&self) -> f64 {
        f64::from(self.rim_in) * INCH * 0.5
    }

    pub fn radius(&self) -> f64 {
        self.rim_radius() + self.sidewall()
    }

    fn code(&self) -> String {
        format!("{}/{}R{}", self.width_mm, self.aspect, self.rim_in)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Disc {
    Vented,
    Drilled,
}

struct Rim {
    radius: f64,
    width: f64,
    spokes: u32,
    twin: bool,
    spoke_width: f64,
    taper: f64,
    concavity: f64,
    lip: f64,
}

struct Wheel {
    tyre: Tyre,
    rim: Rim,
    disc: Disc,
    segments: u32,
    side: f64,
}

fn table<'a>(
    params: &'a BTreeMap<String, Value>,
    key: &str,
    keys: &[&str],
) -> Result<Option<&'a Map<String, Value>>, String> {
    let Some(value) = params.get(key) else {
        return Ok(None);
    };
    let Some(map) = value.as_object() else {
        return Err(format!(
            "params.{key}: expected an inline table of: {}",
            keys.join(", ")
        ));
    };
    if let Some(k) = map.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(format!(
            "params.{key}.{k}: unknown key, expected one of: {}",
            keys.join(", ")
        ));
    }
    Ok(Some(map))
}

fn ranged(
    map: Option<&Map<String, Value>>,
    path: &str,
    key: &str,
    default: f64,
    [lo, hi]: [f64; 2],
) -> Result<f64, String> {
    let v = match map.and_then(|m| m.get(key)) {
        None => default,
        Some(v) => v
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| format!("params.{path}.{key}: expected a number"))?,
    };
    if v < lo || v > hi {
        return Err(format!(
            "params.{path}.{key}: {v} is out of range, expected {lo} to {hi}"
        ));
    }
    Ok(v)
}

fn text<'a>(
    map: Option<&'a Map<String, Value>>,
    path: &str,
    key: &str,
    default: &'a str,
) -> Result<&'a str, String> {
    match map.and_then(|m| m.get(key)) {
        None => Ok(default),
        Some(v) => v
            .as_str()
            .ok_or_else(|| format!("params.{path}.{key}: expected a string")),
    }
}

fn hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn colour(text: &str, path: &str) -> Result<[u8; 3], String> {
    hex(text)
        .ok_or_else(|| format!("params.{path}: `{text}` is not a colour, expected \"#rrggbb\""))
}

fn srgb(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf_of(2.4)
    }
}

fn read(params: &BTreeMap<String, Value>) -> Result<(Wheel, [u8; 3], [u8; 3]), String> {
    if let Some(key) = params.keys().find(|k| !PARAMS.contains(&k.as_str())) {
        return Err(format!(
            "params.{key}: unknown param, expected one of: {}",
            PARAMS.join(", ")
        ));
    }
    let code = match params.get("tyre") {
        None => TYRE_DEFAULT,
        Some(v) => v.as_str().ok_or_else(|| {
            "params.tyre: expected a size code \"WWW/AARDD\", as \"255/40R19\"".to_string()
        })?,
    };
    let tyre = Tyre::parse(code).ok_or_else(|| {
        format!(
            "params.tyre: `{code}` is not a tyre size, expected \"WWW/AARDD\" (width 105 to 405 mm, aspect 20 to 95 %, rim 13 to 26 in), as \"255/40R19\""
        )
    })?;
    let side = match params.get("side") {
        None => 1.0,
        Some(v) => match v.as_str() {
            Some("left") => 1.0,
            Some("right") => -1.0,
            _ => return Err("params.side: expected \"left\" or \"right\"".into()),
        },
    };
    let segments = match params.get("segments") {
        None => 72,
        Some(v) => v
            .as_u64()
            .filter(|n| (24..=256).contains(n))
            .ok_or_else(|| "params.segments: expected a whole number from 24 to 256".to_string())?
            as u32,
    };
    let rim = table(params, "rim", &RIM_KEYS)?;
    let diameter = ranged(
        rim,
        "rim",
        "diameter_in",
        f64::from(tyre.rim_in),
        [13.0, 26.0],
    )?;
    if diameter != f64::from(tyre.rim_in) {
        return Err(format!(
            "params.rim.diameter_in: {diameter} does not fit the tyre {}, expected {}",
            tyre.code(),
            tyre.rim_in
        ));
    }
    let fit = ((f64::from(tyre.width_mm) / 25.4 * 0.85) * 2.0).round() * 0.5;
    let widest = (tyre.width() + 0.02) / INCH;
    let widest = widest.min(14.0);
    let width_in = ranged(
        rim,
        "rim",
        "width_in",
        fit.clamp(4.0, widest),
        [4.0, widest],
    )?;
    let spokes =
        match rim.and_then(|m| m.get("spokes")) {
            None => 5,
            Some(v) => v.as_u64().filter(|n| (3..=24).contains(n)).ok_or_else(|| {
                "params.rim.spokes: expected a whole number from 3 to 24".to_string()
            })? as u32,
        };
    let twin = match rim.and_then(|m| m.get("twin")) {
        None => false,
        Some(v) => v
            .as_bool()
            .ok_or_else(|| "params.rim.twin: expected true or false".to_string())?,
    };
    let cap = text(rim, "rim", "cap", "blank")?;
    if cap != "blank" {
        return Err(format!(
            "params.rim.cap: `{cap}` is not a cap, expected \"blank\" (no letters or logos)"
        ));
    }
    let metal = colour(text(rim, "rim", "metal", METAL_DEFAULT)?, "rim.metal")?;
    let brake = table(params, "brake", &BRAKE_KEYS)?;
    let disc = match text(brake, "brake", "disc", "vented")? {
        "vented" => Disc::Vented,
        "drilled" => Disc::Drilled,
        other => {
            return Err(format!(
                "params.brake.disc: `{other}` is not a disc, expected \"vented\" or \"drilled\""
            ));
        }
    };
    let caliper = colour(
        text(brake, "brake", "caliper", CALIPER_DEFAULT)?,
        "brake.caliper",
    )?;
    let rim = Rim {
        radius: tyre.rim_radius(),
        width: width_in * INCH,
        spokes,
        twin,
        spoke_width: ranged(rim, "rim", "spoke_width", 0.035, [0.008, 0.12])?,
        taper: ranged(rim, "rim", "taper", 0.35, [0.0, 0.9])?,
        concavity: ranged(rim, "rim", "concavity", 0.3, [0.0, 1.0])?,
        lip: ranged(rim, "rim", "lip", 0.015, [0.0, 0.06])?,
    };
    Ok((
        Wheel {
            tyre,
            rim,
            disc,
            segments,
            side,
        },
        metal,
        caliper,
    ))
}

pub fn generate(call: &Call) -> Result<Generated, String> {
    let (wheel, metal, caliper) = read(call.params)?;
    let name = &call.object.name;
    let metal_name = format!("wheel_rim_{:02x}{:02x}{:02x}", metal[0], metal[1], metal[2]);
    let caliper_name = format!(
        "wheel_caliper_{:02x}{:02x}{:02x}",
        caliper[0], caliper[1], caliper[2]
    );
    let turn = DMat4::from_rotation_z(-wheel.side * FRAC_PI_2);
    let mut tyre = wheel.tyre_mesh();
    let mut rim = wheel.rim_mesh(&metal_name);
    let mut brake = wheel.brake_mesh(&caliper_name);
    for m in [&mut tyre, &mut rim, &mut brake] {
        mesh::transform(m, turn);
    }
    let pieces = vec![
        Piece {
            name: name.clone(),
            mesh: Mesh::default(),
        },
        Piece {
            name: format!("{name}.tyre"),
            mesh: tyre,
        },
        Piece {
            name: format!("{name}.rim"),
            mesh: rim,
        },
        Piece {
            name: format!("{name}.brake"),
            mesh: brake,
        },
    ];
    let materials = BTreeMap::from([
        (
            TYRE.to_string(),
            json!({ "base": [0.022, 0.022, 0.025], "roughness": 0.85 }),
        ),
        (
            metal_name,
            json!({ "family": "metal", "base": metal.map(srgb), "roughness": 0.25, "metalness": 1.0 }),
        ),
        (
            CAP.to_string(),
            json!({ "base": [0.02, 0.02, 0.022], "roughness": 0.15 }),
        ),
        (
            DISC.to_string(),
            json!({ "family": "metal", "base": [0.3, 0.3, 0.31], "roughness": 0.45, "metalness": 1.0 }),
        ),
        (
            caliper_name,
            json!({ "family": "lacquer", "base": caliper.map(srgb), "roughness": 0.35, "clearcoat": 1.0, "clearcoat_roughness": 0.08 }),
        ),
    ]);
    let disc = match wheel.disc {
        Disc::Vented => "vented",
        Disc::Drilled => "drilled",
    };
    let Value::Object(facts) = json!({
        "wheel": {
            "radius": wheel.tyre.radius(),
            "width": wheel.tyre.width(),
            "axle": [1.0, 0.0, 0.0],
        },
        "tyre": {
            "size": wheel.tyre.code(),
            "rim_radius": wheel.tyre.rim_radius(),
        },
        "brake": {
            "spin": false,
            "disc": disc,
        },
    }) else {
        unreachable!()
    };
    Ok(Generated {
        pieces,
        facts,
        materials,
        ..Generated::default()
    })
}

#[derive(Default)]
struct Tris {
    pos: Vec<DVec3>,
    tris: Vec<[u32; 3]>,
}

impl Tris {
    fn vert(&mut self, p: DVec3) -> u32 {
        self.pos.push(p);
        (self.pos.len() - 1) as u32
    }

    fn tri(&mut self, a: u32, b: u32, c: u32, out: DVec3) {
        let [pa, pb, pc] = [a, b, c].map(|i| self.pos[i as usize]);
        let n = (pb - pa).cross(pc - pa);
        if n.length_squared() < 1e-24 {
            return;
        }
        self.tris.push(if n.dot(out) < 0.0 {
            [a, c, b]
        } else {
            [a, b, c]
        });
    }

    fn quad(&mut self, [a, b, c, d]: [u32; 4], out: DVec3) {
        self.tri(a, b, c, out);
        self.tri(a, c, d, out);
    }

    fn centre(&self, ids: &[u32]) -> DVec3 {
        ids.iter().map(|&i| self.pos[i as usize]).sum::<DVec3>() / ids.len() as f64
    }

    fn sweep(&mut self, stations: &[(DVec3, DVec3, DVec3, [f64; 2])]) {
        let rings: Vec<Vec<u32>> = stations
            .iter()
            .map(|&(c, side, up, [a, b])| {
                let k = 0.3 * a.min(b);
                [
                    (a, b - k),
                    (a - k, b),
                    (k - a, b),
                    (-a, b - k),
                    (-a, k - b),
                    (k - a, -b),
                    (a - k, -b),
                    (a, k - b),
                ]
                .iter()
                .map(|&(x, y)| self.vert(c + side * x + up * y))
                .collect()
            })
            .collect();
        for i in 0..rings.len() - 1 {
            let mid = (stations[i].0 + stations[i + 1].0) * 0.5;
            for k in 0..8 {
                let q = [
                    rings[i][k],
                    rings[i][(k + 1) % 8],
                    rings[i + 1][(k + 1) % 8],
                    rings[i + 1][k],
                ];
                let out = self.centre(&q) - mid;
                self.quad(q, out);
            }
        }
        let last = rings.len() - 1;
        for (ring, out) in [
            (0, stations[0].0 - stations[1].0),
            (last, stations[last].0 - stations[last - 1].0),
        ] {
            let c = self.vert(stations[ring].0);
            for k in 0..8 {
                self.tri(c, rings[ring][k], rings[ring][(k + 1) % 8], out);
            }
        }
    }

    fn mesh(self, material: &str, degrees: f64) -> Mesh {
        let mut m = Mesh {
            normals: vec![[0.0; 3]; self.pos.len()],
            positions: self.pos.iter().map(|p| p.as_vec3().to_array()).collect(),
            indices: self.tris.iter().flatten().copied().collect(),
            ..Mesh::default()
        };
        if m.indices.is_empty() {
            return Mesh::default();
        }
        m.slots = vec![Slot {
            material: material.to_string(),
            start: 0,
            count: m.indices.len() as u32,
        }];
        mesh::smooth_by_angle(&mut m, degrees);
        m
    }
}

fn lathed(profile: &[[f64; 2]], segments: u32, material: &str) -> Mesh {
    let mut ring = profile.to_vec();
    ring.push(profile[0]);
    let mut m = mesh::lathe(&ring, segments, 1e-7);
    m.set_material(material);
    m
}

fn radial(a: f64) -> DVec3 {
    DVec3::new(a.sin_of(), 0.0, a.cos_of())
}

impl Wheel {
    fn face(&self) -> f64 {
        self.rim.width * 0.5
    }

    fn hub_radius(&self) -> f64 {
        (self.rim.radius * 0.3).clamp(0.055, 0.11)
    }

    fn hub_y(&self) -> f64 {
        self.face() - 0.008 - self.rim.concavity * self.rim.width * 0.45
    }

    fn rim_segments(&self) -> u32 {
        self.segments.div_ceil(self.rim.spokes) * self.rim.spokes
    }

    fn tyre_mesh(&self) -> Mesh {
        let t = self.tyre;
        let r = t.radius();
        let rr = t.rim_radius();
        let h = t.sidewall();
        let half = t.width() * 0.5;
        let bead = self.face() + 0.006;
        let widest = half.max(bead + 0.004);
        let shoulder = (0.3 * h).min(0.25 * half).min(0.025);
        let tread = half * 0.9 - shoulder;
        let top = tread + shoulder;
        let grooves = if t.width() >= 0.215 {
            4
        } else if t.width() >= 0.165 {
            3
        } else {
            2
        };
        let gw = (t.width() * 0.045).min(0.011);
        let gd = (h * 0.12).min(0.008);
        let mut side: Vec<[f64; 2]> = vec![[rr - 0.004, bead - 0.01], [rr + 0.004, bead]];
        let steps = 10;
        let peak = 0.4;
        for k in 0..=steps {
            let u = f64::from(k) / f64::from(steps);
            let y = if u <= peak {
                bead + (widest - bead) * (FRAC_PI_2 * u / peak).sin_of()
            } else {
                widest - (widest - top) * (1.0 - (FRAC_PI_2 * (u - peak) / (1.0 - peak)).cos_of())
            };
            side.push([rr + 0.01 + u * (h - shoulder - 0.01), y]);
        }
        for k in 1..=5 {
            let a = FRAC_PI_2 * f64::from(k) / 5.0;
            side.push([
                r - shoulder + shoulder * a.sin_of(),
                tread + shoulder * a.cos_of(),
            ]);
        }
        let mut crown: Vec<[f64; 2]> = Vec::new();
        for i in 0..grooves {
            let yc = tread - 2.0 * tread * f64::from(i + 1) / f64::from(grooves + 1);
            if yc <= 0.0 {
                break;
            }
            crown.extend([
                [r, yc + gw * 0.5],
                [r - gd, yc + gw * 0.5 - 0.001],
                [r - gd, yc - gw * 0.5 + 0.001],
                [r, yc - gw * 0.5],
            ]);
        }
        let middle = grooves % 2 == 1;
        let mut profile = side.clone();
        profile.extend(crown.iter().copied());
        if middle {
            profile.extend([
                [r, gw * 0.5],
                [r - gd, gw * 0.5 - 0.001],
                [r - gd, 0.001 - gw * 0.5],
                [r, -gw * 0.5],
            ]);
        }
        profile.extend(crown.iter().rev().map(|&[x, y]| [x, -y]));
        profile.extend(side.iter().rev().map(|&[x, y]| [x, -y]));
        lathed(&profile, self.segments, TYRE)
    }

    fn rim_mesh(&self, metal: &str) -> Mesh {
        let rr = self.rim.radius;
        let f = self.face();
        let lip = self.rim.lip;
        let n = self.rim_segments();
        let barrel = [
            [rr, f - 0.010],
            [rr + 0.013, f - 0.004],
            [rr + 0.013, f + 0.001],
            [rr + 0.008, f + 0.005],
            [rr - lip, f + 0.005],
            [rr - lip - 0.004, f + 0.001],
            [rr - lip - 0.006, f - 0.012],
            [rr - 0.014, f - 0.035],
            [rr - 0.014, 0.004 - f],
            [rr - 0.006, -f - 0.003],
            [rr + 0.011, -f - 0.003],
            [rr + 0.011, 0.004 - f],
            [rr, 0.010 - f],
            [rr - 0.004, 0.0],
        ];
        let mut m = lathed(&barrel, n, metal);
        let hub = self.hub_radius();
        let cap = hub * 0.62;
        let y = self.hub_y();
        m.append(&lathed(
            &[
                [cap - 0.002, y + 0.006],
                [hub - 0.008, y + 0.010],
                [hub, y + 0.004],
                [hub + 0.002, y - 0.016],
                [cap - 0.002, y - 0.020],
            ],
            n,
            metal,
        ));
        m.append(&lathed(
            &[
                [0.0, y + 0.016],
                [cap * 0.6, y + 0.0145],
                [cap, y + 0.010],
                [cap, y - 0.004],
                [0.0, y - 0.004],
            ],
            n,
            CAP,
        ));
        m.append(&self.spokes(metal));
        m
    }

    fn spokes(&self, metal: &str) -> Mesh {
        let rim = &self.rim;
        let r0 = self.hub_radius() - 0.01;
        let r1 = rim.radius - rim.lip - 0.003;
        let y0 = self.hub_y();
        let rise = self.face() - 0.008 - y0;
        let depth = (0.6 * rim.spoke_width).clamp(0.016, 0.04);
        let lanes: Vec<(f64, f64)> = if rim.twin {
            vec![(1.0, 0.55), (-1.0, 0.55)]
        } else {
            vec![(0.0, 1.0)]
        };
        let steps = 10;
        let mut t = Tris::default();
        for k in 0..rim.spokes {
            let a = TAU * f64::from(k) / f64::from(rim.spokes);
            let out = radial(a);
            let side = DVec3::new(a.cos_of(), 0.0, -a.sin_of());
            for &(lane, share) in &lanes {
                let stations: Vec<(DVec3, DVec3, DVec3, [f64; 2])> = (0..=steps)
                    .map(|i| {
                        let s = f64::from(i) / f64::from(steps);
                        let rho = r0 + (r1 - r0) * s;
                        let y = y0 + rise * (0.35 * s + 0.65 * s * s);
                        let dy = rise * (0.35 + 1.3 * s);
                        let up = (DVec3::Y * (r1 - r0) - out * dy).normalize();
                        let width = 0.5 * rim.spoke_width * share * (1.0 - rim.taper * s);
                        let shift = lane * (width + 0.0015 + 0.6 * rim.spoke_width * s);
                        let centre = out * rho + DVec3::Y * y + side * shift;
                        (centre, side, up, [width, 0.5 * depth * (1.0 - 0.4 * s)])
                    })
                    .collect();
                t.sweep(&stations);
            }
        }
        t.mesh(metal, 40.0)
    }

    fn brake_mesh(&self, caliper: &str) -> Mesh {
        let barrel = self.rim.radius - 0.014;
        let outer = barrel - 0.006;
        let rc = outer - 0.032;
        let rd = rc + 0.014;
        let hub = self.hub_radius();
        let ri = (rd * 0.55).max(hub + 0.02);
        let mount = self.hub_y() - 0.022;
        let yd = mount - 0.04;
        let thick = 0.026;
        let n = self.segments;
        let mut m = match self.disc {
            Disc::Vented => self.vented(ri, rd, yd, thick),
            Disc::Drilled => drilled(ri, rd, yd, thick),
        };
        m.append(&lathed(
            &[
                [hub * 0.85, mount],
                [ri + 0.004, mount],
                [ri + 0.004, yd],
                [ri - 0.004, yd],
                [ri - 0.004, mount - 0.005],
                [hub * 0.85, mount - 0.005],
            ],
            n,
            DISC,
        ));
        let span = (rd * 0.8).clamp(0.09, 0.2) / rc;
        let centre = -self.side * (FRAC_PI_2 + CALIPER_ANGLE_DEG.to_radians());
        let axial = thick * 0.5 + 0.022;
        let steps = 8;
        let stations: Vec<(DVec3, DVec3, DVec3, [f64; 2])> = (0..=steps)
            .map(|i| {
                let a = centre + span * (f64::from(i) / f64::from(steps) - 0.5);
                (
                    radial(a) * rc + DVec3::Y * yd,
                    DVec3::Y,
                    radial(a),
                    [axial, 0.032],
                )
            })
            .collect();
        let mut t = Tris::default();
        t.sweep(&stations);
        m.append(&t.mesh(caliper, 40.0));
        m
    }

    fn vented(&self, ri: f64, rd: f64, yd: f64, thick: f64) -> Mesh {
        let plate = 0.008;
        let gap = thick - 2.0 * plate;
        let n = self.segments;
        let mut m = Mesh::default();
        for sign in [-1.0, 1.0] {
            let lo = yd + sign * gap * 0.5;
            let hi = lo + sign * plate;
            m.append(&lathed(&[[ri, lo], [rd, lo], [rd, hi], [ri, hi]], n, DISC));
        }
        let vanes = 36;
        let length = (rd - ri) * 0.92;
        for k in 0..vanes {
            let a = TAU * f64::from(k) / f64::from(vanes);
            let turn = DQuat::from_rotation_y(a);
            let mut vane = mesh::rounded_box([0.004, gap + 0.001, length], 0.0, 1);
            vane.set_material(DISC);
            mesh::transform(
                &mut vane,
                DMat4::from_rotation_translation(turn, turn * DVec3::new(0.0, yd, (ri + rd) * 0.5)),
            );
            m.append(&vane);
        }
        m
    }
}

fn drilled(ri: f64, rd: f64, yd: f64, thick: f64) -> Mesh {
    let rings = 7usize;
    let sectors = 72usize;
    let hole = |j: usize, s: usize| (j == 2 && s.is_multiple_of(4)) || (j == 4 && s % 4 == 2);
    let mut t = Tris::default();
    let grid: Vec<Vec<Vec<u32>>> = [yd - thick * 0.5, yd + thick * 0.5]
        .iter()
        .map(|&y| {
            (0..=rings)
                .map(|j| {
                    let rho = ri + (rd - ri) * j as f64 / rings as f64;
                    (0..sectors)
                        .map(|s| {
                            let a = TAU * s as f64 / sectors as f64;
                            t.vert(radial(a) * rho + DVec3::Y * y)
                        })
                        .collect()
                })
                .collect()
        })
        .collect();
    let corner = |f: usize, j: usize, s: usize| grid[f][j][s % sectors];
    for j in 0..rings {
        for s in 0..sectors {
            let cell = [(j, s), (j + 1, s), (j + 1, s + 1), (j, s + 1)];
            if hole(j, s) {
                let middle = t.centre(&cell.map(|(j, s)| corner(0, j, s)))
                    + t.centre(&cell.map(|(j, s)| corner(1, j, s)));
                let middle = middle * 0.5;
                for e in 0..4 {
                    let (a, b) = (cell[e], cell[(e + 1) % 4]);
                    let q = [
                        corner(0, a.0, a.1),
                        corner(0, b.0, b.1),
                        corner(1, b.0, b.1),
                        corner(1, a.0, a.1),
                    ];
                    let out = middle - t.centre(&q);
                    t.quad(q, out);
                }
                continue;
            }
            for (f, out) in [(0, -DVec3::Y), (1, DVec3::Y)] {
                t.quad(cell.map(|(j, s)| corner(f, j, s)), out);
            }
        }
    }
    for s in 0..sectors {
        for (j, sign) in [(0, -1.0), (rings, 1.0)] {
            let q = [
                corner(0, j, s),
                corner(0, j, s + 1),
                corner(1, j, s + 1),
                corner(1, j, s),
            ];
            let a = TAU * (s as f64 + 0.5) / sectors as f64;
            t.quad(q, radial(a) * sign);
        }
    }
    t.mesh(DISC, 40.0)
}
