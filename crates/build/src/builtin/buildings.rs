use crate::{Call, Generated, Piece};
use glam::{DVec2, DVec3};
use manfred_mesh::Mesh;
use manfred_motion::Route;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const NAME: &str = "manfred/buildings";

const PARAMS: [&str; 11] = [
    "route", "side", "setback", "block", "height", "depth", "gap", "storey", "window", "lit",
    "seed",
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

fn materials() -> BTreeMap<String, Value> {
    [
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
            json!({ "family": "glass", "base": [0.04, 0.05, 0.06], "roughness": 0.03, "ior": 1.5 }),
        ),
        (
            LIT,
            json!({ "family": "emissive", "base": [1.0, 0.78, 0.5], "emission": [4.0, 2.9, 1.6] }),
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

#[derive(Clone, Debug)]
pub struct Params {
    pub route: String,
    pub side: Side,
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
        let storey = number(params, "storey", 3.2, 2.0, 10.0)?;
        let (window_width, window_spacing) = window(params, storey)?;
        let seed =
            match params.get("seed") {
                None => None,
                Some(value) => Some(value.as_i64().map(|n| n as u64).ok_or_else(|| {
                    format!("params.seed: expected a whole number, found {value}")
                })?),
            };
        let height = range(params, "height", [12.0, 60.0], storey, 1000.0)?;
        Ok(Params {
            route,
            side,
            setback: number(params, "setback", 11.0, 0.0, 1000.0)?,
            block: range(params, "block", [8.0, 24.0], 4.0, 500.0)?,
            height,
            depth: range(params, "depth", [12.0, 20.0], 2.0, 500.0)?,
            gap: range(params, "gap", [0.0, 4.0], 0.0, 500.0)?,
            storey,
            window_width,
            window_spacing,
            lit: number(params, "lit", 0.25, 0.0, 1.0)?,
            seed,
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

fn window(params: &BTreeMap<String, Value>, storey: f64) -> Result<(f64, f64), String> {
    let (mut width, mut spacing) = (1.4, 3.0);
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
    let reach = foot.half.hypot(foot.depth) + setback + 1.0;
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
            let gap = rng.within(params.gap);
            let at = d;
            d += w + gap;
            if w < params.block[0] - 1e-9 {
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
            let palette = &PALETTES[rng.pick(PALETTES.len())];
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
            shape.facade(&mut facades, &place);
            shape.windows(
                &mut facades,
                &mut windows,
                &mut lit,
                &place,
                params,
                &mut rng,
            );
            shape.roof(&mut roofs, &place, &mut rng);
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
        let mut pane = |rng: &mut Rng, chance: f64, c: [DVec3; 4], n: DVec3| {
            if rng.chance(chance) {
                lit.quad(LIT, place, c, n);
            } else {
                glass.quad(GLASS, place, c, n);
            }
        };
        let front = v(0.0, 0.0, -1.0);
        for x in columns(2.0 * (h - PIER), SHOP + 1.1, SHOP) {
            let (a, b) = (x - SHOP * 0.5, x + SHOP * 0.5);
            let (lo, hi, z) = (0.3, base - 1.1, 0.2 - PROUD);
            let c = [v(b, lo, z), v(a, lo, z), v(a, hi, z), v(b, hi, z)];
            pane(rng, (params.lit * 2.0).min(1.0), c, front);
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
                pane(rng, params.lit, c, front);
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
                pane(rng, params.lit, c, v(1.0, 0.0, 0.0));
                let c = [v(-x, lo, a), v(-x, lo, b), v(-x, hi, b), v(-x, hi, a)];
                pane(rng, params.lit, c, v(-1.0, 0.0, 0.0));
            }
        }
    }

    fn roof(&self, sink: &mut Sink, place: &Place, rng: &mut Rng) {
        let (h, d) = (self.half, self.depth);
        let top = self.top + CORNICE;
        let v = DVec3::new;
        sink.cuboid(
            ROOF,
            place,
            v(-h + 0.4, top, 0.4),
            v(h - 0.4, top + 0.1, d - 0.4),
            &[],
        );
        for _ in 0..1 + rng.pick(3) {
            let hx = rng.within([0.75, 2.0]).min(h * 0.5);
            let hz = rng.within([0.75, 2.0]).min(d * 0.25);
            let cx = rng.within([-1.0, 1.0]) * (h - 0.5 - hx).max(0.0);
            let cz = d * 0.5 + rng.within([-1.0, 1.0]) * (d * 0.5 - 0.5 - hz).max(0.0);
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
        let route = route();
        let config = params(json!({ "seed": 3 }));
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
