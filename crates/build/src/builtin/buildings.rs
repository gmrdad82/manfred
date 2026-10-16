use crate::{Call, Generated, Piece};
use glam::{DVec2, DVec3};
use manfred_mesh::Mesh;
use manfred_mesh::math::Real;
use manfred_motion::Route;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const NAME: &str = "manfred/buildings";

const PARAMS: [&str; 13] = [
    "route", "side", "style", "setback", "block", "height", "depth", "gap", "storey", "window",
    "lit", "seed", "reveal",
];

const STEP: f64 = 0.5;
const CLEAR: f64 = 0.02;
const RECESS: f64 = 0.35;
const PIER: f64 = 0.6;
const FOOT: f64 = 2.0;
const CORNICE: f64 = 0.6;
const PROUD: f64 = 0.03;

const GLASS: &str = "buildings_glass";
const LIT: &str = "buildings_lit";
const LIT_SOFT: &str = "buildings_lit_soft";
const LIT_NEUTRAL: &str = "buildings_lit_neutral";
const LIT_CURTAIN: &str = "buildings_lit_curtain";

type Choices = [(&'static str, f64)];

const UPPER: &Choices = &[
    (LIT, 0.2),
    (LIT_SOFT, 0.3),
    (LIT_NEUTRAL, 0.2),
    (LIT_CURTAIN, 0.3),
];
const SHOPFRONT: &Choices = &[(LIT, 0.45), (LIT_NEUTRAL, 0.35), (LIT_SOFT, 0.2)];
const ROOF: &str = "buildings_roof";
const PLANT: &str = "buildings_plant";

struct Palette {
    wall: &'static str,
    trim: &'static str,
    curtain: bool,
}

const PALETTES: [Palette; 5] = [
    Palette {
        wall: "buildings_concrete",
        trim: "buildings_trim_stone",
        curtain: false,
    },
    Palette {
        wall: "buildings_brick",
        trim: "buildings_trim_cream",
        curtain: false,
    },
    Palette {
        wall: "buildings_sandstone",
        trim: "buildings_trim_stone",
        curtain: false,
    },
    Palette {
        wall: "buildings_render",
        trim: "buildings_trim_stone",
        curtain: false,
    },
    Palette {
        wall: "buildings_curtain",
        trim: "buildings_trim_metal",
        curtain: true,
    },
];

const BRICKS: [Palette; 4] = [
    Palette {
        wall: "buildings_brick",
        trim: "buildings_trim_cream",
        curtain: false,
    },
    Palette {
        wall: "buildings_brick_dark",
        trim: "buildings_trim_stone",
        curtain: false,
    },
    Palette {
        wall: "buildings_brick_brown",
        trim: "buildings_trim_cream",
        curtain: false,
    },
    Palette {
        wall: "buildings_brick_orange",
        trim: "buildings_trim_stone",
        curtain: false,
    },
];

const IRON: &str = "buildings_iron";
const AWNINGS: [&str; 3] = [
    "buildings_awning_green",
    "buildings_awning_red",
    "buildings_awning_navy",
];

fn materials() -> BTreeMap<String, Value> {
    [
        (
            "buildings_brick_dark",
            json!({ "family": "lacquer", "base": [0.22, 0.10, 0.07], "roughness": 0.7 }),
        ),
        (
            "buildings_brick_brown",
            json!({ "family": "lacquer", "base": [0.30, 0.17, 0.11], "roughness": 0.68 }),
        ),
        (
            "buildings_brick_orange",
            json!({ "family": "lacquer", "base": [0.42, 0.19, 0.11], "roughness": 0.66 }),
        ),
        (
            IRON,
            json!({ "family": "metal", "base": [0.035, 0.035, 0.038], "roughness": 0.55, "metalness": 1.0 }),
        ),
        (
            AWNINGS[0],
            json!({ "family": "plaster", "base": [0.06, 0.20, 0.12], "roughness": 0.9 }),
        ),
        (
            AWNINGS[1],
            json!({ "family": "plaster", "base": [0.32, 0.06, 0.06], "roughness": 0.9 }),
        ),
        (
            AWNINGS[2],
            json!({ "family": "plaster", "base": [0.05, 0.08, 0.20], "roughness": 0.9 }),
        ),
        (
            "buildings_concrete",
            json!({ "family": "plaster", "base": [0.52, 0.51, 0.48], "roughness": 0.88 }),
        ),
        (
            "buildings_brick",
            json!({ "family": "lacquer", "base": [0.34, 0.15, 0.10], "roughness": 0.62 }),
        ),
        (
            "buildings_sandstone",
            json!({ "family": "stone", "base": [0.60, 0.50, 0.38], "roughness": 0.8 }),
        ),
        (
            "buildings_render",
            json!({ "family": "plaster", "base": [0.76, 0.74, 0.70], "roughness": 0.85 }),
        ),
        (
            "buildings_curtain",
            json!({ "family": "lacquer", "base": [0.035, 0.045, 0.055], "roughness": 0.06 }),
        ),
        (
            "buildings_trim_stone",
            json!({ "family": "stone", "base": [0.40, 0.39, 0.37], "roughness": 0.75 }),
        ),
        (
            "buildings_trim_cream",
            json!({ "family": "plaster", "base": [0.72, 0.67, 0.58], "roughness": 0.7 }),
        ),
        (
            "buildings_trim_metal",
            json!({ "family": "metal", "base": [0.30, 0.31, 0.33], "roughness": 0.35, "metalness": 1.0 }),
        ),
        (
            GLASS,
            json!({ "family": "lacquer", "base": [0.018, 0.024, 0.03], "roughness": 0.04 }),
        ),
        (
            LIT,
            json!({ "family": "emissive", "base": [1.0, 0.8, 0.56], "emission": [2.4, 1.75, 1.0] }),
        ),
        (
            LIT_SOFT,
            json!({ "family": "emissive", "base": [1.0, 0.74, 0.46], "emission": [1.4, 0.95, 0.52] }),
        ),
        (
            LIT_NEUTRAL,
            json!({ "family": "emissive", "base": [0.96, 0.95, 0.9], "emission": [1.25, 1.2, 1.08] }),
        ),
        (
            LIT_CURTAIN,
            json!({ "family": "emissive", "base": [0.9, 0.62, 0.4], "emission": [0.6, 0.4, 0.22] }),
        ),
        (ROOF, json!({ "base": [0.11, 0.11, 0.115], "roughness": 0.92 })),
        (
            PLANT,
            json!({ "family": "metal", "base": [0.55, 0.56, 0.57], "roughness": 0.5, "metalness": 1.0 }),
        ),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value))
    .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Side {
    Both,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Style {
    Towers,
    Lowrise,
}

#[derive(Clone, Debug)]
pub struct Params {
    pub route: String,
    pub side: Side,
    pub style: Style,
    pub setback: f64,
    pub block: [f64; 2],
    pub height: [f64; 2],
    pub depth: [f64; 2],
    pub gap: [f64; 2],
    pub storey: f64,
    pub window_width: f64,
    pub window_spacing: f64,
    pub lit: f64,
    pub seed: Option<u64>,
    pub reveal: f64,
}

impl Params {
    pub fn read(params: &BTreeMap<String, Value>) -> Result<Params, String> {
        if let Some(key) = params.keys().find(|key| !PARAMS.contains(&key.as_str())) {
            return Err(format!(
                "params.{key}: unknown, expected one of: {}",
                PARAMS.join(", ")
            ));
        }
        let route = match params.get("route") {
            Some(Value::String(name)) if !name.is_empty() => name.clone(),
            Some(other) => return Err(format!("params.route: expected a name, found {other}")),
            None => return Err("params.route: missing".into()),
        };
        let side = match params.get("side") {
            None => Side::Both,
            Some(Value::String(s)) if s == "both" => Side::Both,
            Some(Value::String(s)) if s == "left" => Side::Left,
            Some(Value::String(s)) if s == "right" => Side::Right,
            Some(other) => {
                return Err(format!(
                    "params.side: expected \"both\", \"left\" or \"right\", found {other}"
                ));
            }
        };
        let style = match params.get("style") {
            None => Style::Towers,
            Some(Value::String(s)) if s == "towers" => Style::Towers,
            Some(Value::String(s)) if s == "lowrise" => Style::Lowrise,
            Some(other) => {
                return Err(format!(
                    "params.style: expected \"towers\" or \"lowrise\", found {other}"
                ));
            }
        };
        let towers = style == Style::Towers;
        let storey = number(params, "storey", 3.2, 2.0, 10.0)?;
        let windows = if towers { (1.4, 3.0) } else { (1.1, 2.4) };
        let (window_width, window_spacing) = window(params, storey, windows)?;
        let seed =
            match params.get("seed") {
                None => None,
                Some(value) => Some(value.as_i64().map(|n| n as u64).ok_or_else(|| {
                    format!("params.seed: expected a whole number, found {value}")
                })?),
            };
        let pick = |a: [f64; 2], b: [f64; 2]| if towers { a } else { b };
        let height = range(
            params,
            "height",
            pick([12.0, 60.0], [3.0 * storey, 5.0 * storey]),
            storey,
            1000.0,
        )?;
        Ok(Params {
            route,
            side,
            style,
            setback: number(params, "setback", 11.0, 0.0, 1000.0)?,
            block: range(params, "block", pick([8.0, 24.0], [7.0, 14.0]), 4.0, 500.0)?,
            height,
            depth: range(
                params,
                "depth",
                pick([12.0, 20.0], [10.0, 16.0]),
                2.0,
                500.0,
            )?,
            gap: range(params, "gap", pick([0.0, 4.0], [1.0, 3.0]), 0.0, 500.0)?,
            storey,
            window_width,
            window_spacing,
            lit: number(params, "lit", if towers { 0.06 } else { 0.03 }, 0.0, 1.0)?,
            seed,
            reveal: number(params, "reveal", 1.0, 0.0, 1.0)?,
        })
    }
}

fn number(
    params: &BTreeMap<String, Value>,
    key: &str,
    default: f64,
    lo: f64,
    hi: f64,
) -> Result<f64, String> {
    let Some(value) = params.get(key) else {
        return Ok(default);
    };
    match value.as_f64() {
        Some(v) if (lo..=hi).contains(&v) => Ok(v),
        _ => Err(format!(
            "params.{key}: expected a number from {lo} to {hi}, found {value}"
        )),
    }
}

fn range(
    params: &BTreeMap<String, Value>,
    key: &str,
    default: [f64; 2],
    lo: f64,
    hi: f64,
) -> Result<[f64; 2], String> {
    let Some(value) = params.get(key) else {
        return Ok(default);
    };
    let pair = match value {
        Value::Array(items) if items.len() == 2 => match (items[0].as_f64(), items[1].as_f64()) {
            (Some(a), Some(b)) => Some([a, b]),
            _ => None,
        },
        other => other.as_f64().map(|v| [v, v]),
    };
    match pair {
        Some([a, b]) if lo <= a && a <= b && b <= hi => Ok([a, b]),
        _ => Err(format!(
            "params.{key}: expected a number or [min, max] with {lo} <= min <= max <= {hi}, found {value}"
        )),
    }
}

fn window(
    params: &BTreeMap<String, Value>,
    storey: f64,
    (mut width, mut spacing): (f64, f64),
) -> Result<(f64, f64), String> {
    match params.get("window") {
        None => {}
        Some(Value::Object(table)) => {
            if let Some(key) = table
                .keys()
                .find(|key| *key != "width" && *key != "spacing")
            {
                return Err(format!(
                    "params.window.{key}: unknown, expected width or spacing"
                ));
            }
            let read = |key: &str, into: &mut f64| -> Result<(), String> {
                if let Some(value) = table.get(key) {
                    *into = value.as_f64().filter(|v| *v > 0.0).ok_or_else(|| {
                        format!("params.window.{key}: expected a positive number, found {value}")
                    })?;
                }
                Ok(())
            };
            read("width", &mut width)?;
            read("spacing", &mut spacing)?;
        }
        Some(other) => {
            return Err(format!(
                "params.window: expected a table {{ width, spacing }}, found {other}"
            ));
        }
    }
    if width + 0.2 > spacing || spacing > 50.0 || storey < 2.0 {
        return Err(format!(
            "params.window: width ({width}) must leave at least 0.2 m of the spacing ({spacing}), up to 50 m"
        ));
    }
    Ok((width, spacing))
}

pub fn generate(call: &Call) -> Result<Generated, String> {
    let params = Params::read(call.params)?;
    let route = call
        .route(&params.route)
        .ok_or_else(|| format!("params.route: no route is named `{}`", params.route))?;
    Ok(street(&call.object.name, route, &params, call.seed))
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn within(&mut self, [a, b]: [f64; 2]) -> f64 {
        a + (b - a) * self.unit()
    }

    fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    fn pick(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn weighted(&mut self, choices: &Choices) -> &'static str {
        let total: f64 = choices.iter().map(|(_, w)| w).sum();
        let mut roll = self.unit() * total;
        for (name, w) in choices {
            if roll < *w {
                return name;
            }
            roll -= w;
        }
        choices[choices.len() - 1].0
    }
}

struct Place {
    origin: DVec3,
    x: DVec3,
    z: DVec3,
}

impl Place {
    fn at(&self, p: DVec3) -> [f32; 3] {
        (self.origin + self.x * p.x + DVec3::Y * p.y + self.z * p.z)
            .as_vec3()
            .to_array()
    }

    fn dir(&self, n: DVec3) -> [f32; 3] {
        (self.x * n.x + DVec3::Y * n.y + self.z * n.z)
            .as_vec3()
            .to_array()
    }
}

#[derive(Default)]
struct Sink {
    meshes: BTreeMap<&'static str, Mesh>,
}

impl Sink {
    fn mesh(&mut self, material: &'static str) -> &mut Mesh {
        self.meshes.entry(material).or_default()
    }

    fn quad(&mut self, material: &'static str, place: &Place, c: [DVec3; 4], n: DVec3) {
        let m = self.mesh(material);
        let base = m.positions.len() as u32;
        let normal = place.dir(n);
        for p in c {
            m.positions.push(place.at(p));
            m.normals.push(normal);
        }
        m.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn cuboid(
        &mut self,
        material: &'static str,
        place: &Place,
        lo: DVec3,
        hi: DVec3,
        skip: &[usize],
    ) {
        const FACES: [([usize; 4], [f64; 3]); 6] = [
            ([0, 4, 6, 2], [-1.0, 0.0, 0.0]),
            ([1, 3, 7, 5], [1.0, 0.0, 0.0]),
            ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
            ([2, 6, 7, 3], [0.0, 1.0, 0.0]),
            ([0, 2, 3, 1], [0.0, 0.0, -1.0]),
            ([4, 5, 7, 6], [0.0, 0.0, 1.0]),
        ];
        let corner = |i: usize| {
            DVec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            )
        };
        for (face, (ids, n)) in FACES.iter().enumerate() {
            if face == 2 || skip.contains(&face) {
                continue;
            }
            self.quad(material, place, ids.map(corner), DVec3::from(*n));
        }
    }

    fn face(&mut self, material: &'static str, place: &Place, c: [DVec3; 4]) {
        let n = (c[1] - c[0])
            .cross(c[2] - c[0])
            .try_normalize()
            .unwrap_or(DVec3::Y);
        self.quad(material, place, c, n);
    }

    fn prism(&mut self, material: &'static str, place: &Place, corner: impl Fn(usize) -> DVec3) {
        for ids in [
            [0, 4, 6, 2],
            [1, 3, 7, 5],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 2, 3, 1],
            [4, 5, 7, 6],
        ] {
            self.face(material, place, ids.map(&corner));
        }
    }

    fn into_mesh(self) -> Mesh {
        let mut out = Mesh::default();
        for (material, mut mesh) in self.meshes {
            mesh.set_material(material);
            out.append(&mesh);
        }
        out
    }
}

const BACK: usize = 5;

struct Footprint {
    origin: DVec2,
    x: DVec2,
    z: DVec2,
    half: f64,
    depth: f64,
}

impl Footprint {
    fn local(&self, p: DVec2) -> DVec2 {
        let d = p - self.origin;
        DVec2::new(d.dot(self.x), d.dot(self.z))
    }

    fn distance(&self, a: DVec2, b: DVec2) -> f64 {
        let (a, b) = (self.local(a), self.local(b));
        let lo = DVec2::new(-self.half, 0.0);
        let hi = DVec2::new(self.half, self.depth);
        if crosses(a, b, lo, hi) {
            return 0.0;
        }
        let to_box = |p: DVec2| (p.clamp(lo, hi) - p).length();
        let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
        corners
            .iter()
            .map(|c| to_segment(*c, a, b))
            .chain([to_box(a), to_box(b)])
            .fold(f64::INFINITY, f64::min)
    }
}

fn crosses(a: DVec2, b: DVec2, lo: DVec2, hi: DVec2) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for axis in 0..2 {
        let (p, v, l, h) = (a[axis], d[axis], lo[axis], hi[axis]);
        if v.abs() < 1e-12 {
            if p < l || p > h {
                return false;
            }
        } else {
            let (mut e, mut f) = ((l - p) / v, (h - p) / v);
            if e > f {
                std::mem::swap(&mut e, &mut f);
            }
            t0 = t0.max(e);
            t1 = t1.min(f);
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

fn to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let d = b - a;
    let len = d.length_squared();
    let t = if len > 0.0 {
        ((p - a).dot(d) / len).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + d * t - p).length()
}

fn plan(v: DVec3) -> DVec2 {
    DVec2::new(v.x, v.z)
}

fn polyline(route: &Route) -> Vec<DVec2> {
    let length = route.length();
    let n = ((length / STEP).ceil() as usize).max(1);
    (0..=n)
        .map(|i| plan(route.at(length * i as f64 / n as f64).position))
        .collect()
}

fn clearance(foot: &Footprint, line: &[DVec2], setback: f64) -> f64 {
    let reach = foot.half.hypot_of(foot.depth) + setback + 1.0;
    line.windows(2)
        .filter(|s| {
            let mid = (s[0] + s[1]) * 0.5;
            (mid - foot.origin).length() <= reach + (s[1] - s[0]).length()
        })
        .map(|s| foot.distance(s[0], s[1]))
        .fold(f64::INFINITY, f64::min)
}

pub fn street(object: &str, route: &Route, params: &Params, seed: u64) -> Generated {
    let seed = params.seed.unwrap_or(seed);
    let line = polyline(route);
    let length = route.length();
    let sides: &[f64] = match params.side {
        Side::Both => &[1.0, -1.0],
        Side::Right => &[1.0],
        Side::Left => &[-1.0],
    };
    let mut facades = Sink::default();
    let mut windows = Sink::default();
    let mut lit = Sink::default();
    let mut roofs = Sink::default();
    let mut count = 0usize;
    for &sign in sides {
        let mut rng = Rng(seed
            ^ if sign > 0.0 {
                0x0052_4947_4854
            } else {
                0x4C45_4654
            });
        let mut d = 0.0;
        while d + params.block[0] <= length + 1e-9 {
            let w = rng.within(params.block).min(length - d);
            let gap = match params.style {
                Style::Towers => rng.within(params.gap),
                Style::Lowrise if rng.chance(LOWRISE_GAPS) => rng.within(params.gap),
                Style::Lowrise => 0.0,
            };
            let at = d;
            d += w + gap;
            if w < params.block[0] - 1e-9 {
                break;
            }
            if params.reveal < 1.0 && at >= params.reveal * length {
                break;
            }
            let frame = route.at(at + w * 0.5);
            let tangent = plan(frame.tangent).try_normalize().unwrap_or(DVec2::Y);
            let right = plan(frame.right)
                .try_normalize()
                .unwrap_or(DVec2::new(-tangent.y, tangent.x));
            let x = tangent * sign;
            let z = right * sign;
            let depth = rng.within(params.depth);
            let mut foot = Footprint {
                origin: plan(frame.position) + z * params.setback,
                x,
                z,
                half: w * 0.5,
                depth,
            };
            let mut placed = false;
            for _ in 0..4 {
                let clear = clearance(&foot, &line, params.setback);
                if clear >= params.setback {
                    placed = true;
                    break;
                }
                foot.origin += z * (params.setback - clear + CLEAR);
            }
            let storeys = (rng.within(params.height) / params.storey).round().max(1.0);
            let palettes: &[Palette] = match params.style {
                Style::Towers => &PALETTES,
                Style::Lowrise => &BRICKS,
            };
            let palette = &palettes[rng.pick(palettes.len())];
            if !placed {
                continue;
            }
            let place = Place {
                origin: DVec3::new(foot.origin.x, frame.position.y, foot.origin.y),
                x: DVec3::new(x.x, 0.0, x.y),
                z: DVec3::new(z.x, 0.0, z.y),
            };
            let base = params.storey * 1.25;
            let shape = Shape {
                half: w * 0.5,
                depth,
                storeys: storeys as usize,
                palette,
                base,
                top: base + (storeys - 1.0) * params.storey,
            };
            let front = match params.style {
                Style::Towers => {
                    shape.facade(&mut facades, &place);
                    shape.windows(
                        &mut facades,
                        &mut windows,
                        &mut lit,
                        &place,
                        params,
                        &mut rng,
                    );
                    0.0
                }
                Style::Lowrise => {
                    shape.lowrise(
                        &mut facades,
                        &mut windows,
                        &mut lit,
                        &place,
                        params,
                        &mut rng,
                    );
                    WALL
                }
            };
            shape.roof(&mut roofs, &place, front, &mut rng);
            count += 1;
        }
    }
    let pieces = [
        ("facades", facades),
        ("windows", windows),
        ("lit", lit),
        ("roofs", roofs),
    ]
    .into_iter()
    .filter(|(_, sink)| !sink.meshes.is_empty())
    .map(|(name, sink)| Piece {
        name: format!("{object}.{name}"),
        mesh: sink.into_mesh(),
    })
    .collect();
    let Value::Object(facts) = json!({
        "buildings": { "count": count, "setback": params.setback },
    }) else {
        unreachable!()
    };
    Generated {
        pieces,
        facts,
        materials: materials(),
        ..Generated::default()
    }
}

struct Shape<'a> {
    half: f64,
    depth: f64,
    storeys: usize,
    palette: &'a Palette,
    base: f64,
    top: f64,
}

fn columns(span: f64, spacing: f64, width: f64) -> Vec<f64> {
    let mut n = (span / spacing).floor().max(0.0) as usize;
    if n == 0 && span >= width + 0.2 {
        n = 1;
    }
    (0..n)
        .map(|i| (i as f64 + 0.5 - n as f64 * 0.5) * spacing)
        .collect()
}

const SHOP: f64 = 3.4;
const WALL: f64 = 1.3;
const BAY: f64 = 0.8;
const LOWRISE_GAPS: f64 = 0.12;
const RAIL: f64 = 0.05;

#[derive(Clone, Copy, PartialEq)]
enum Feature {
    Plain,
    Bay,
    Balconies,
    Escape,
}

fn rails(sink: &mut Sink, place: &Place, l: f64, r: f64, y: f64, out: f64, w: f64) {
    let v = DVec3::new;
    for (lo, hi) in [(y + 0.9, y + 0.9 + RAIL), (y + 0.45, y + 0.45 + RAIL)] {
        sink.cuboid(IRON, place, v(l, lo, out), v(r, hi, out + RAIL), &[]);
        for x in [l, r - RAIL] {
            sink.cuboid(
                IRON,
                place,
                v(x, lo, out + RAIL),
                v(x + RAIL, hi, w),
                &[BACK],
            );
        }
    }
    for x in [l, (l + r - RAIL) * 0.5, r - RAIL] {
        sink.cuboid(
            IRON,
            place,
            v(x, y, out),
            v(x + RAIL, y + 0.9, out + RAIL),
            &[],
        );
    }
}

impl Shape<'_> {
    fn facade(&self, sink: &mut Sink, place: &Place) {
        let (h, d, base, top) = (self.half, self.depth, self.base, self.top);
        let (wall, trim) = (self.palette.wall, self.palette.trim);
        let v = DVec3::new;
        sink.cuboid(trim, place, v(-h, -FOOT, 0.2), v(h, base - 0.8, d), &[]);
        sink.cuboid(trim, place, v(-h, base - 0.8, 0.0), v(h, base, d), &[]);
        if top > base {
            sink.cuboid(wall, place, v(-h, base, RECESS), v(h, top, d), &[]);
            for x in [-h, h - PIER] {
                sink.cuboid(
                    trim,
                    place,
                    v(x, base, 0.0),
                    v(x + PIER, top, RECESS),
                    &[BACK],
                );
            }
            for k in 1..self.storeys.saturating_sub(1) {
                let y = base + (top - base) * k as f64 / (self.storeys - 1) as f64;
                sink.cuboid(
                    trim,
                    place,
                    v(-h + PIER, y - 0.12, 0.12),
                    v(h - PIER, y + 0.12, RECESS),
                    &[BACK, 0, 1],
                );
            }
        }
        sink.cuboid(trim, place, v(-h, top, 0.0), v(h, top + CORNICE, d), &[]);
    }

    fn windows(
        &self,
        facades: &mut Sink,
        glass: &mut Sink,
        lit: &mut Sink,
        place: &Place,
        params: &Params,
        rng: &mut Rng,
    ) {
        let (h, d, s, base) = (self.half, self.depth, params.storey, self.base);
        let curtain = self.palette.curtain;
        let width = if curtain {
            params.window_spacing - 0.25
        } else {
            params.window_width
        };
        let tall = if curtain { s - 0.5 } else { s * 0.55 };
        let v = DVec3::new;
        let mut pane = |rng: &mut Rng, chance: f64, choices: &Choices, c: [DVec3; 4], n: DVec3| {
            if rng.chance(chance) {
                lit.quad(rng.weighted(choices), place, c, n);
            } else {
                glass.quad(GLASS, place, c, n);
            }
        };
        let front = v(0.0, 0.0, -1.0);
        for x in columns(2.0 * (h - PIER), SHOP + 1.1, SHOP) {
            let (a, b) = (x - SHOP * 0.5, x + SHOP * 0.5);
            let (lo, hi, z) = (0.3, base - 1.1, 0.2 - PROUD);
            let c = [v(b, lo, z), v(a, lo, z), v(a, hi, z), v(b, hi, z)];
            pane(rng, (params.lit * 2.5).min(1.0), SHOPFRONT, c, front);
        }
        let across = columns(2.0 * (h - PIER) - 0.4, params.window_spacing, width);
        let span = d - RECESS - 1.6;
        let along = columns(span, params.window_spacing, width);
        let middle = (RECESS + d) * 0.5;
        for k in 0..self.storeys.saturating_sub(1) {
            let lo = base + k as f64 * s + (s - tall) * 0.5;
            let hi = lo + tall;
            for x in &across {
                let (a, b, z) = (x - width * 0.5, x + width * 0.5, RECESS - PROUD);
                let c = [v(b, lo, z), v(a, lo, z), v(a, hi, z), v(b, hi, z)];
                pane(rng, params.lit, UPPER, c, front);
                if !curtain {
                    let sill_lo = v(a - 0.08, lo - 0.12, RECESS - 0.12);
                    let sill_hi = v(b + 0.08, lo, RECESS);
                    facades.cuboid(self.palette.trim, place, sill_lo, sill_hi, &[BACK]);
                }
            }
            for z in &along {
                let (a, b, x) = (
                    middle + z - width * 0.5,
                    middle + z + width * 0.5,
                    h + PROUD,
                );
                let c = [v(x, lo, a), v(x, hi, a), v(x, hi, b), v(x, lo, b)];
                pane(rng, params.lit, UPPER, c, v(1.0, 0.0, 0.0));
                let c = [v(-x, lo, a), v(-x, lo, b), v(-x, hi, b), v(-x, hi, a)];
                pane(rng, params.lit, UPPER, c, v(-1.0, 0.0, 0.0));
            }
        }
    }

    fn lowrise(
        &self,
        facades: &mut Sink,
        glass: &mut Sink,
        lit: &mut Sink,
        place: &Place,
        params: &Params,
        rng: &mut Rng,
    ) {
        let (h, d, s, base, top) = (self.half, self.depth, params.storey, self.base, self.top);
        let (wall, trim) = (self.palette.wall, self.palette.trim);
        let (width, w, shop) = (params.window_width, WALL, WALL + 0.2);
        let v = DVec3::new;
        let front = v(0.0, 0.0, -1.0);
        let mut pane = |rng: &mut Rng, chance: f64, choices: &Choices, c: [DVec3; 4]| {
            if rng.chance(chance) {
                lit.quad(rng.weighted(choices), place, c, front);
            } else {
                glass.quad(GLASS, place, c, front);
            }
        };
        facades.cuboid(trim, place, v(-h, -FOOT, shop), v(h, base - 0.8, d), &[]);
        facades.cuboid(trim, place, v(-h, base - 0.8, w - 0.15), v(h, base, d), &[]);
        let awning = rng.chance(0.65).then(|| AWNINGS[rng.pick(AWNINGS.len())]);
        for x in columns(2.0 * h - 0.6, SHOP + 0.9, SHOP) {
            let (a, b) = (x - SHOP * 0.5, x + SHOP * 0.5);
            let (lo, hi, z) = (0.3, base - 1.1, shop - PROUD);
            let c = [v(b, lo, z), v(a, lo, z), v(a, hi, z), v(b, hi, z)];
            pane(rng, (params.lit * 2.5).min(1.0), SHOPFRONT, c);
            if let Some(cloth) = awning {
                let (y, drop, out) = (base - 0.85, 0.7, 0.4);
                let (l, r) = (a - 0.1, b + 0.1);
                facades.prism(cloth, place, |i| {
                    let back = i & 4 != 0;
                    v(
                        if i & 1 == 0 { l } else { r },
                        (if back { y } else { y - drop }) + if i & 2 == 0 { 0.0 } else { 0.05 },
                        if back { shop } else { out },
                    )
                });
                facades.cuboid(
                    cloth,
                    place,
                    v(l, y - drop - 0.3, out),
                    v(r, y - drop + 0.05, out + 0.05),
                    &[],
                );
            }
        }
        if top > base {
            facades.cuboid(wall, place, v(-h, base, w), v(h, top, d), &[]);
            for k in 1..self.storeys.saturating_sub(1) {
                let y = base + (top - base) * k as f64 / (self.storeys - 1) as f64;
                facades.cuboid(
                    trim,
                    place,
                    v(-h, y - 0.1, w - 0.1),
                    v(h, y + 0.1, w),
                    &[BACK, 0, 1],
                );
            }
        }
        facades.cuboid(
            trim,
            place,
            v(-h, top, w - 0.5),
            v(h, top + CORNICE, d),
            &[],
        );
        facades.cuboid(
            trim,
            place,
            v(-h, top - 0.3, w - 0.25),
            v(h, top, w),
            &[BACK],
        );
        let across = columns(2.0 * h - 0.8, params.window_spacing, width);
        let roll = rng.unit();
        let feature = match roll {
            r if r < 0.25 => Feature::Bay,
            r if r < 0.5 => Feature::Balconies,
            r if r < 0.8 => Feature::Escape,
            _ => Feature::Plain,
        };
        let bay = (feature == Feature::Bay && across.len() >= 3).then_some(across.len() / 2);
        let tall = s * 0.6;
        let level = |k: usize| base + k as f64 * s + (s - tall) * 0.5;
        let floors = self.storeys.saturating_sub(1);
        for k in 0..floors {
            let (lo, hi) = (level(k), level(k) + tall);
            for (i, x) in across.iter().enumerate() {
                let (a, b) = (x - width * 0.5, x + width * 0.5);
                let z = if bay == Some(i) { w - BAY } else { w };
                let p = z - PROUD;
                pane(
                    rng,
                    params.lit,
                    UPPER,
                    [v(b, lo, p), v(a, lo, p), v(a, hi, p), v(b, hi, p)],
                );
                facades.cuboid(
                    trim,
                    place,
                    v(a - 0.08, lo - 0.12, z - 0.12),
                    v(b + 0.08, lo, z),
                    &[BACK],
                );
                facades.cuboid(
                    trim,
                    place,
                    v(a - 0.1, hi, z - 0.06),
                    v(b + 0.1, hi + 0.22, z),
                    &[BACK],
                );
                if feature == Feature::Balconies && k >= 1 && rng.chance(0.4) {
                    let (l, r, out) = (a - 0.3, b + 0.3, w - 0.9);
                    facades.cuboid(
                        trim,
                        place,
                        v(l, lo - 0.2, out),
                        v(r, lo - 0.05, w),
                        &[BACK],
                    );
                    rails(facades, place, l, r, lo - 0.05, out, w);
                }
            }
        }
        if let Some(i) = bay {
            let (x, half) = (across[i], width * 0.5 + 0.4);
            facades.cuboid(
                wall,
                place,
                v(x - half, base + 0.2, w - BAY),
                v(x + half, top - 0.35, w),
                &[BACK],
            );
        }
        if feature == Feature::Escape && across.len() >= 2 && floors > 0 {
            let i = rng.pick(across.len() - 1);
            let (l, r) = (
                across[i] - width * 0.5 - 0.3,
                across[i + 1] + width * 0.5 + 0.3,
            );
            let out = w - 1.2;
            let landing = |k: usize| level(k) - 0.15;
            for k in 0..floors {
                let y = landing(k);
                facades.cuboid(IRON, place, v(l, y - 0.06, out), v(r, y, w), &[BACK]);
                rails(facades, place, l, r, y, out, w);
                if k + 1 < floors {
                    let ends = [(l + 0.35, y), (r - 0.35, landing(k + 1))];
                    let [(xa, ya), (xb, yb)] = if k % 2 == 0 {
                        ends
                    } else {
                        [(l + 0.35, landing(k + 1)), (r - 0.35, y)]
                    };
                    let (z0, z1) = (out + 0.1, out + 0.75);
                    facades.prism(IRON, place, |c| {
                        let high = c & 1 != 0;
                        v(
                            if high { xb } else { xa },
                            (if high { yb } else { ya }) - if c & 2 == 0 { 0.12 } else { 0.0 },
                            if c & 4 == 0 { z0 } else { z1 },
                        )
                    });
                }
            }
            let y = landing(0);
            for x in [l + 0.3, l + 0.75] {
                facades.cuboid(
                    IRON,
                    place,
                    v(x, y - 1.6, out + 0.1),
                    v(x + 0.04, y - 0.06, out + 0.14),
                    &[],
                );
            }
        }
    }

    fn roof(&self, sink: &mut Sink, place: &Place, front: f64, rng: &mut Rng) {
        let (h, d) = (self.half, self.depth);
        let top = self.top + CORNICE;
        let v = DVec3::new;
        sink.cuboid(
            ROOF,
            place,
            v(-h + 0.4, top, front + 0.4),
            v(h - 0.4, top + 0.1, d - 0.4),
            &[],
        );
        let (mid, half) = ((front + d) * 0.5, (d - front) * 0.5);
        for _ in 0..1 + rng.pick(3) {
            let hx = rng.within([0.75, 2.0]).min(h * 0.5);
            let hz = rng.within([0.75, 2.0]).min(half * 0.5);
            let cx = rng.within([-1.0, 1.0]) * (h - 0.5 - hx).max(0.0);
            let cz = mid + rng.within([-1.0, 1.0]) * (half - 0.5 - hz).max(0.0);
            let tall = rng.within([1.2, 3.2]);
            let material = if rng.chance(0.5) { PLANT } else { ROOF };
            sink.cuboid(
                material,
                place,
                v(cx - hx, top + 0.1, cz - hz),
                v(cx + hx, top + 0.1 + tall, cz + hz),
                &[],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(extra: Value) -> Params {
        let mut map: BTreeMap<String, Value> = [("route".to_string(), json!("main"))].into();
        if let Value::Object(more) = extra {
            map.extend(more);
        }
        Params::read(&map).unwrap()
    }

    fn route() -> Route {
        Route::new(
            &[
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 300.0],
                [60.0, 0.0, 450.0],
                [200.0, 0.0, 560.0],
            ],
            false,
        )
        .unwrap()
    }

    #[test]
    fn the_same_seed_builds_the_same_bytes() {
        let p = params(json!({}));
        let a = street("street", &route(), &p, 7);
        let b = street("street", &route(), &p, 7);
        let c = street("street", &route(), &p, 8);
        assert_eq!(format!("{:?}", a), format!("{:?}", b));
        assert_ne!(format!("{:?}", a.pieces), format!("{:?}", c.pieces));
    }

    #[test]
    fn no_building_enters_the_road_and_the_street_stays_light() {
        for style in ["towers", "lowrise"] {
            check(style);
        }
    }

    #[test]
    fn a_reveal_builds_the_buildings_that_start_before_its_share_and_one_builds_them_all() {
        let count = |g: &Generated| g.facts["buildings"]["count"].as_f64().unwrap();
        let whole = street("street", &route(), &params(json!({})), 7);
        let one = street("street", &route(), &params(json!({ "reveal": 1.0 })), 7);
        assert_eq!(format!("{whole:?}"), format!("{one:?}"));
        let half = street("street", &route(), &params(json!({ "reveal": 0.5 })), 7);
        let share = count(&half) / count(&whole);
        assert!((0.35..0.65).contains(&share), "{share}");
        let none = street("street", &route(), &params(json!({ "reveal": 0.0 })), 7);
        assert!(none.pieces.is_empty());
    }

    #[test]
    fn lowrise_is_deterministic_by_seed() {
        let p = params(json!({ "style": "lowrise" }));
        let a = street("street", &route(), &p, 7);
        let b = street("street", &route(), &p, 7);
        assert_eq!(format!("{:?}", a), format!("{:?}", b));
    }

    fn check(style: &str) {
        let route = route();
        let config = params(json!({ "seed": 3, "style": style }));
        let made = street("street", &route, &config, 0);
        let names: Vec<&str> = made.pieces.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "street.facades",
                "street.windows",
                "street.lit",
                "street.roofs"
            ]
        );
        let count = made.facts["buildings"]["count"].as_u64().unwrap();
        assert!(count > 40, "{count}");
        assert_eq!(made.facts["buildings"]["setback"], json!(11.0));
        let triangles: usize = made.pieces.iter().map(|p| p.mesh.triangle_count()).sum();
        assert!(triangles < 200_000, "{triangles}");
        let n = (route.length() / 0.1).ceil() as usize;
        let line: Vec<DVec2> = (0..=n)
            .map(|i| plan(route.at(route.length() * i as f64 / n as f64).position))
            .collect();
        let cell = 16.0;
        let key = |p: DVec2| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
        let mut grid: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
        for (i, p) in line[..line.len() - 1].iter().enumerate() {
            grid.entry(key(*p)).or_default().push(i);
        }
        for piece in &made.pieces {
            for v in &piece.mesh.positions {
                let p = DVec2::new(v[0] as f64, v[2] as f64);
                let (kx, kz) = key(p);
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        for &i in grid.get(&(kx + dx, kz + dz)).into_iter().flatten() {
                            let d = to_segment(p, line[i], line[i + 1]);
                            assert!(d >= config.setback - 0.05, "{} at {d}", piece.name);
                        }
                    }
                }
            }
        }
    }
}
