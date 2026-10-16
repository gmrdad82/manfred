use super::{rgb, table, tagged};
use crate::{Call, Generated, GeneratedLight, Piece};
use glam::{DMat4, DQuat, DVec2, DVec3};
use manfred_mesh::{self as mesh, Mesh, Slot};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

pub const NAME: &str = "manfred/car";

const PARAMS: [&str; 14] = [
    "seed",
    "length",
    "width",
    "height",
    "wheelbase",
    "track",
    "wheel_radius",
    "paint",
    "style",
    "lights",
    "head_emission",
    "tail_emission",
    "head_light",
    "tail_light",
];
const HEAD_EMISSION: [f64; 3] = [5.0, 4.6, 3.8];
const TAIL_EMISSION: [f64; 3] = [3.2, 0.03, 0.01];
const HEAD_LAMP: Lamp = Lamp {
    intensity: 90.0,
    cone: [20.0, 40.0],
    range: 40.0,
    color: [1.0, 0.93, 0.82],
};
const TAIL_LAMP: Lamp = Lamp {
    intensity: 6.0,
    cone: [15.0, 35.0],
    range: 6.0,
    color: [1.0, 0.06, 0.03],
};

#[derive(Clone, Copy)]
struct Lamp {
    intensity: f64,
    cone: [f64; 2],
    range: f64,
    color: [f64; 3],
}
const STYLES: [&str; 4] = ["sports", "sedan", "hatch", "van"];

const GLASS: &str = "car_glass";
const TYRE: &str = "car_tyre";
const RIM: &str = "car_rim";
const TRIM: &str = "car_trim";
const HEAD: &str = "car_head";
const TAIL: &str = "car_tail";
const HEAD_OFF: &str = "car_head_off";
const TAIL_OFF: &str = "car_tail_off";

const SPORTS_PAINT: [&str; 6] = [
    "#c8102e", "#f2a900", "#0057b8", "#ff6a13", "#00843d", "#e8e8e8",
];
const TRAFFIC_PAINT: [&str; 8] = [
    "#c0c4c8", "#f4f4f2", "#16181b", "#1f2f4f", "#5b6066", "#7a1f23", "#2f4a3a", "#c9b99a",
];

const HALF: usize = 17;
const RING: usize = 2 * HALF - 2;
const ROCKER: f64 = 0.03;
const DISC: f64 = 0.058;

#[derive(Clone, Copy, PartialEq)]
enum Band {
    Under,
    Body,
    Side,
    Edge,
    Top,
}

const BANDS: [Band; HALF - 1] = [
    Band::Under,
    Band::Under,
    Band::Under,
    Band::Body,
    Band::Body,
    Band::Body,
    Band::Body,
    Band::Body,
    Band::Body,
    Band::Body,
    Band::Side,
    Band::Side,
    Band::Edge,
    Band::Edge,
    Band::Top,
    Band::Top,
];

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Sports,
    Sedan,
    Hatch,
    Van,
}

struct Design {
    size: [f64; 3],
    radius: f64,
    wheelbase: f64,
    front: f64,
    clearance: f64,
    tyre: f64,
    flare: f64,
    tumble: f64,
    deck: &'static [(f64, f64)],
    roof: &'static [(f64, f64)],
    pillar: f64,
    nose: f64,
    tail: f64,
    rim: f64,
    shoulder: f64,
    edge: f64,
    crown: [f64; 2],
}

const SPORTS: Design = Design {
    size: [4.4, 1.9, 1.1],
    radius: 0.34,
    wheelbase: 0.6,
    front: 0.52,
    clearance: 0.11,
    tyre: 0.255,
    flare: 0.045,
    tumble: 0.68,
    deck: &[
        (0.0, 0.6),
        (0.07, 0.7),
        (0.22, 0.75),
        (0.42, 0.74),
        (0.66, 0.66),
        (0.87, 0.53),
        (1.0, 0.43),
    ],
    roof: &[
        (0.2, 0.0),
        (0.41, 0.965),
        (0.53, 1.0),
        (0.6, 0.985),
        (0.75, 0.0),
    ],
    pillar: 0.45,
    nose: 0.2,
    tail: 0.21,
    rim: 0.7,
    shoulder: 0.11,
    edge: 0.075,
    crown: [0.03, 0.025],
};

const SEDAN: Design = Design {
    size: [4.7, 1.82, 1.45],
    radius: 0.32,
    wheelbase: 0.595,
    front: 0.5,
    clearance: 0.15,
    tyre: 0.215,
    flare: 0.015,
    tumble: 0.8,
    deck: &[
        (0.0, 0.64),
        (0.12, 0.68),
        (0.27, 0.69),
        (0.68, 0.66),
        (0.9, 0.6),
        (1.0, 0.53),
    ],
    roof: &[
        (0.25, 0.0),
        (0.37, 0.98),
        (0.5, 1.0),
        (0.58, 0.99),
        (0.69, 0.0),
    ],
    pillar: 0.47,
    nose: 0.14,
    tail: 0.12,
    rim: 0.62,
    shoulder: 0.06,
    edge: 0.041,
    crown: [0.02, 0.015],
};

const HATCH: Design = Design {
    size: [4.1, 1.78, 1.5],
    radius: 0.31,
    wheelbase: 0.62,
    front: 0.55,
    clearance: 0.15,
    tyre: 0.205,
    flare: 0.02,
    tumble: 0.82,
    deck: &[
        (0.0, 0.6),
        (0.05, 0.64),
        (0.64, 0.6),
        (0.9, 0.54),
        (1.0, 0.48),
    ],
    roof: &[
        (0.03, 0.0),
        (0.13, 0.975),
        (0.45, 1.0),
        (0.55, 0.99),
        (0.66, 0.0),
    ],
    pillar: 0.4,
    nose: 0.14,
    tail: 0.1,
    rim: 0.6,
    shoulder: 0.06,
    edge: 0.041,
    crown: [0.02, 0.015],
};

const VAN: Design = Design {
    size: [4.9, 1.95, 1.95],
    radius: 0.33,
    wheelbase: 0.62,
    front: 0.55,
    clearance: 0.17,
    tyre: 0.215,
    flare: 0.015,
    tumble: 0.93,
    deck: &[
        (0.0, 0.48),
        (0.03, 0.5),
        (0.74, 0.47),
        (0.92, 0.42),
        (1.0, 0.37),
    ],
    roof: &[
        (0.005, 0.0),
        (0.03, 0.985),
        (0.5, 1.0),
        (0.7, 0.98),
        (0.79, 0.0),
    ],
    pillar: 0.62,
    nose: 0.16,
    tail: 0.08,
    rim: 0.58,
    shoulder: 0.06,
    edge: 0.041,
    crown: [0.02, 0.015],
};

impl Style {
    fn design(self) -> &'static Design {
        match self {
            Style::Sports => &SPORTS,
            Style::Sedan => &SEDAN,
            Style::Hatch => &HATCH,
            Style::Van => &VAN,
        }
    }

    fn name(self) -> &'static str {
        STYLES[self as usize]
    }
}

struct Car {
    style: Style,
    d: &'static Design,
    length: f64,
    width: f64,
    height: f64,
    wheelbase: f64,
    track: f64,
    radius: f64,
    arch: f64,
    axles: [f64; 2],
    deck_scale: f64,
    roof: Vec<(f64, f64)>,
    fine: bool,
}

struct Station {
    ys: f64,
    yw: f64,
    hw: f64,
    ysh: f64,
    yr: f64,
    hwr: f64,
    open: f64,
}

struct Shell {
    pos: Vec<DVec3>,
    paint: Vec<[u32; 3]>,
    trim: Vec<[u32; 3]>,
    glass: Vec<[u32; 3]>,
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn number(params: &BTreeMap<String, Value>, key: &str) -> Result<Option<f64>, String> {
    match params.get(key) {
        None => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|n| n.is_finite())
            .map(Some)
            .ok_or_else(|| format!("params.{key}: expected a number")),
    }
}

fn ranged(
    params: &BTreeMap<String, Value>,
    key: &str,
    default: f64,
    lo: f64,
    hi: f64,
) -> Result<f64, String> {
    let v = number(params, key)?.unwrap_or(default);
    if v < lo || v > hi {
        return Err(format!(
            "params.{key}: {v} is out of range, expected {lo} to {hi}"
        ));
    }
    Ok(v)
}

pub fn generate(call: &Call) -> Result<Generated, String> {
    let params = call.params;
    if let Some(key) = params.keys().find(|k| !PARAMS.contains(&k.as_str())) {
        return Err(format!(
            "params.{key}: unknown param, expected one of: {}",
            PARAMS.join(", ")
        ));
    }
    let style = match params.get("style") {
        None => Style::Sports,
        Some(v) => match v.as_str() {
            Some("sports") => Style::Sports,
            Some("sedan") => Style::Sedan,
            Some("hatch") => Style::Hatch,
            Some("van") => Style::Van,
            _ => {
                return Err(format!(
                    "params.style: expected one of: {}",
                    STYLES.join(", ")
                ));
            }
        },
    };
    let seed = match params.get("seed") {
        None => call.seed,
        Some(v) => v
            .as_u64()
            .ok_or_else(|| "params.seed: expected a whole number from 0".to_string())?,
    };
    let lights = match params.get("lights") {
        None => true,
        Some(v) => v
            .as_bool()
            .ok_or_else(|| "params.lights: expected true or false".to_string())?,
    };
    let emission = |key: &str, default: [f64; 3]| match params.get(key) {
        None => Ok(default),
        Some(v) => rgb(v, key, None),
    };
    let head_emission = emission("head_emission", HEAD_EMISSION)?;
    let tail_emission = emission("tail_emission", TAIL_EMISSION)?;
    let head_lamp = lamp(params, "head_light", HEAD_LAMP)?;
    let tail_lamp = lamp(params, "tail_light", TAIL_LAMP)?;
    let d = style.design();
    let length = ranged(params, "length", d.size[0], 2.0, 12.0)?;
    let width = ranged(params, "width", d.size[1], 1.0, 3.0)?;
    let height = ranged(params, "height", d.size[2], 0.8, 3.5)?;
    let radius = ranged(params, "wheel_radius", d.radius, 0.15, height * 0.35)?;
    let wheelbase = ranged(
        params,
        "wheelbase",
        length * d.wheelbase,
        length * 0.4,
        length * 0.8,
    )?;
    let track = ranged(
        params,
        "track",
        width - d.tyre - 2.0 * d.flare - 0.02,
        width * 0.5,
        width - d.tyre,
    )?;
    let mut rng = Rng(seed);
    let (paint, offered) = paint(params.get("paint"), style, &mut rng)?;
    let overhang = length - wheelbase;
    let front = length * 0.5 - overhang * d.front;
    let mut car = Car {
        style,
        d,
        length,
        width,
        height,
        wheelbase,
        track,
        radius,
        arch: radius * 1.12,
        axles: [front - wheelbase, front],
        deck_scale: 1.0 + 0.04 * (rng.next() - 0.5),
        roof: Vec::new(),
        fine: style == Style::Sports,
    };
    let last = d.roof.len() - 1;
    car.roof = d
        .roof
        .iter()
        .enumerate()
        .map(|(i, &(s, h))| {
            if i == 0 || i == last {
                (s, car.deck(s))
            } else {
                (s, h * height)
            }
        })
        .collect();
    let name = &call.object.name;
    let mut pieces = Vec::new();
    let shell = car.shell();
    let mut body = shell.mesh(&shell.paint, &paint, 50.0);
    body.append(&shell.mesh(&shell.trim, TRIM, 50.0));
    car.trim(&mut body, &paint);
    let mut glass = shell.mesh(&shell.glass, GLASS, 50.0);
    if style == Style::Van {
        let (hw, lo, hi) = car.cap(-1.0);
        let h = (hi - lo) * 0.34;
        glass.append(&placed(
            mesh::rounded_box([2.0 * hw * 0.82, h, 0.016], 0.03, 2),
            GLASS,
            DVec3::new(0.0, hi - h * 0.5 - 0.08, -length * 0.5 - 0.002),
            DQuat::IDENTITY,
        ));
    }
    let (fixed, head, tail) = fixed(lights, head_emission, tail_emission);
    pieces.push(Piece {
        name: format!("{name}.body"),
        mesh: body,
    });
    pieces.push(Piece {
        name: format!("{name}.glass"),
        mesh: glass,
    });
    let mut wheels = Map::new();
    for (key, side, axle) in [
        ("fl", 1.0, 1),
        ("fr", -1.0, 1),
        ("rl", 1.0, 0),
        ("rr", -1.0, 0),
    ] {
        let centre = DVec3::new(side * track * 0.5, radius, car.axles[axle]);
        pieces.push(Piece {
            name: format!("{name}.wheel_{key}"),
            mesh: car.wheel(centre, side),
        });
        wheels.insert(
            key.to_string(),
            json!({ "centre": centre.to_array(), "radius": radius, "axle": [1.0, 0.0, 0.0] }),
        );
    }
    pieces.push(Piece {
        name: format!("{name}.headlights"),
        mesh: car.headlights(&head),
    });
    pieces.push(Piece {
        name: format!("{name}.taillights"),
        mesh: car.taillights(&tail),
    });
    let mut materials = offered;
    for (key, value) in fixed {
        materials.insert(key, value);
    }
    let Value::Object(facts) = json!({
        "style": style.name(),
        "length": length,
        "width": width,
        "height": height,
        "wheelbase": car.wheelbase,
        "track": car.track,
        "wheels": wheels,
    }) else {
        unreachable!()
    };
    Ok(Generated {
        lights: if lights {
            car.lights(name, &head_lamp, &tail_lamp)
        } else {
            Vec::new()
        },
        pieces,
        facts,
        materials,
    })
}

fn srgb(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn lacquer(base: [f64; 3]) -> Value {
    json!({
        "family": "lacquer",
        "base": base,
        "roughness": 0.3,
        "clearcoat": 1.0,
        "clearcoat_roughness": 0.03,
    })
}

fn hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn paint(
    value: Option<&Value>,
    style: Style,
    rng: &mut Rng,
) -> Result<(String, BTreeMap<String, Value>), String> {
    let pick = rng.next();
    let text = match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => {
            let rgb: Vec<f64> = items.iter().filter_map(Value::as_f64).collect();
            if rgb.len() != 3 || items.len() != 3 || rgb.iter().any(|c| !(0.0..=1.0).contains(c)) {
                return Err(
                    "params.paint: expected [r, g, b] from 0 to 1, \"#rrggbb\" or a material name"
                        .into(),
                );
            }
            let q = [0, 1, 2].map(|i| (rgb[i] * 255.0).round() as u8);
            let name = format!("car_paint_linear_{:02x}{:02x}{:02x}", q[0], q[1], q[2]);
            let base = q.map(|c| c as f64 / 255.0);
            return Ok((name.clone(), BTreeMap::from([(name, lacquer(base))])));
        }
        Some(_) => {
            return Err(
                "params.paint: expected [r, g, b] from 0 to 1, \"#rrggbb\" or a material name"
                    .into(),
            );
        }
        None => {
            let palette: &[&str] = if style == Style::Sports {
                &SPORTS_PAINT
            } else {
                &TRAFFIC_PAINT
            };
            palette[((pick * palette.len() as f64) as usize).min(palette.len() - 1)].to_string()
        }
    };
    if text.starts_with('#') {
        let Some(rgb) = hex(&text) else {
            return Err(format!(
                "params.paint: `{text}` is not a colour, expected \"#rrggbb\""
            ));
        };
        let name = format!("car_paint_{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
        let base = rgb.map(|c| srgb(c as f64 / 255.0));
        return Ok((name.clone(), BTreeMap::from([(name, lacquer(base))])));
    }
    if text.is_empty() {
        return Err("params.paint: expected a material name or a colour".into());
    }
    Ok((text, BTreeMap::new()))
}

fn lamp(params: &BTreeMap<String, Value>, key: &str, default: Lamp) -> Result<Lamp, String> {
    let Some(given) = params.get(key) else {
        return Ok(default);
    };
    let given = table(given, key, &["intensity", "cone", "range", "color"])?;
    let mut out = default;
    let at_least_zero = |name: &str| -> Result<Option<f64>, String> {
        match given.get(name) {
            None => Ok(None),
            Some(v) => v
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0)
                .map(Some)
                .ok_or_else(|| format!("params.{key}.{name}: expected a number of 0 or more")),
        }
    };
    if let Some(v) = at_least_zero("intensity")? {
        out.intensity = v;
    }
    if let Some(v) = at_least_zero("range")? {
        out.range = v;
    }
    if let Some(v) = given.get("color") {
        out.color = rgb(v, &format!("{key}.color"), None)?;
    }
    if let Some(v) = given.get("cone") {
        let want = || {
            format!(
                "params.{key}.cone: expected [inner, outer] degrees with 0 <= inner <= outer <= 180"
            )
        };
        let list = v.as_array().filter(|l| l.len() == 2).ok_or_else(want)?;
        let inner = list[0].as_f64().ok_or_else(want)?;
        let outer = list[1].as_f64().ok_or_else(want)?;
        if !(0.0 <= inner && inner <= outer && outer <= 180.0) {
            return Err(want());
        }
        out.cone = [inner, outer];
    }
    Ok(out)
}

fn fixed(
    lights: bool,
    head_emission: [f64; 3],
    tail_emission: [f64; 3],
) -> (Vec<(String, Value)>, String, String) {
    let out = vec![
        (
            GLASS,
            json!({ "family": "glass", "base": [0.32, 0.36, 0.38], "roughness": 0.02, "transmission": 0.85, "ior": 1.5, "thickness": 0.004 }),
        ),
        (
            TYRE,
            json!({ "base": [0.022, 0.022, 0.025], "roughness": 0.85 }),
        ),
        (
            RIM,
            json!({ "family": "metal", "base": [0.74, 0.75, 0.77], "roughness": 0.25, "metalness": 1.0 }),
        ),
        (
            TRIM,
            json!({ "base": [0.018, 0.018, 0.02], "roughness": 0.55 }),
        ),
    ];
    let mut out: Vec<(String, Value)> = out
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect();
    let (head, tail) = if lights {
        let head =
            json!({ "family": "emissive", "base": [1.0, 0.95, 0.85], "emission": head_emission });
        let tail =
            json!({ "family": "emissive", "base": [0.6, 0.0, 0.0], "emission": tail_emission });
        let head_name = if head_emission == HEAD_EMISSION {
            HEAD.to_string()
        } else {
            tagged(HEAD, &head)
        };
        let tail_name = if tail_emission == TAIL_EMISSION {
            TAIL.to_string()
        } else {
            tagged(TAIL, &tail)
        };
        out.push((head_name.clone(), head));
        out.push((tail_name.clone(), tail));
        (head_name, tail_name)
    } else {
        out.push((
            HEAD_OFF.to_string(),
            json!({ "base": [0.85, 0.86, 0.88], "roughness": 0.08 }),
        ));
        out.push((
            TAIL_OFF.to_string(),
            json!({ "base": [0.45, 0.02, 0.02], "roughness": 0.1 }),
        ));
        (HEAD_OFF.to_string(), TAIL_OFF.to_string())
    };
    (out, head, tail)
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn curve(keys: &[(f64, f64)], s: f64) -> f64 {
    let n = keys.len();
    if s <= keys[0].0 {
        return keys[0].1;
    }
    if s >= keys[n - 1].0 {
        return keys[n - 1].1;
    }
    let slope = |i: usize| (keys[i + 1].1 - keys[i].1) / (keys[i + 1].0 - keys[i].0);
    let tangent = |i: usize| {
        if i == 0 {
            slope(0)
        } else if i == n - 1 {
            slope(n - 2)
        } else {
            let (a, b) = (slope(i - 1), slope(i));
            if a * b <= 0.0 {
                0.0
            } else {
                2.0 * a * b / (a + b)
            }
        }
    };
    let i = (0..n - 1).find(|&i| s < keys[i + 1].0).unwrap_or(n - 2);
    let h = keys[i + 1].0 - keys[i].0;
    let t = (s - keys[i].0) / h;
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * keys[i].1
        + (t3 - 2.0 * t2 + t) * h * tangent(i)
        + (-2.0 * t3 + 3.0 * t2) * keys[i + 1].1
        + (t3 - t2) * h * tangent(i + 1)
}

fn arc(out: &mut Vec<DVec2>, centre: DVec2, radius: f64, from: f64, to: f64, steps: usize) {
    for k in 0..=steps {
        let a = from + (to - from) * k as f64 / steps as f64;
        out.push(centre + DVec2::new(a.cos(), a.sin()) * radius);
    }
}

fn placed(mut m: Mesh, material: &str, at: DVec3, rotation: DQuat) -> Mesh {
    m.set_material(material);
    mesh::transform(&mut m, DMat4::from_rotation_translation(rotation, at));
    m
}

impl Car {
    fn s(&self, z: f64) -> f64 {
        (z / self.length + 0.5).clamp(0.0, 1.0)
    }

    fn z(&self, s: f64) -> f64 {
        (s - 0.5) * self.length
    }

    fn deck(&self, s: f64) -> f64 {
        curve(self.d.deck, s) * self.height * self.deck_scale
    }

    fn near(&self, z: f64) -> f64 {
        self.axles
            .iter()
            .map(|a| (z - a).abs())
            .fold(f64::INFINITY, f64::min)
    }

    fn station(&self, z: f64) -> Station {
        let s = self.s(z);
        let yb = self.deck(s);
        let (first, last) = (self.roof[0].0, self.roof[self.roof.len() - 1].0);
        let yr = if s > first && s < last {
            curve(&self.roof, s).max(yb)
        } else {
            yb
        };
        let dz = self.near(z);
        let ra = self.arch;
        let top = self.radius + ra + 0.075;
        let ysh = yb + (top - yb).max(0.0) * smoothstep(1.7 * ra, 0.8 * ra, dz);
        let ys = self.d.clearance;
        let mut yw = if dz <= ra {
            self.radius + (ra * ra - dz * dz).max(0.0).sqrt()
        } else {
            ys
        };
        yw = yw.min(ysh - self.d.shoulder - ROCKER - 0.01).max(ys);
        let base = self.width * 0.5 - self.d.flare;
        let plan = (1.0 - 0.07 * smoothstep(0.8, 1.0, s)) * (1.0 - 0.03 * smoothstep(0.2, 0.0, s));
        let hw = (base + self.d.flare * smoothstep(1.5 * ra, 0.9 * ra, dz)) * plan;
        let hwr = (base * self.d.tumble * (1.0 - 0.05 * smoothstep(0.75, 1.0, s)))
            .min(hw - self.d.shoulder - 0.02);
        Station {
            ys,
            yw,
            hw,
            ysh,
            yr,
            hwr,
            open: ((yr - ysh) / 0.12).clamp(0.0, 1.0),
        }
    }

    fn section(&self, z: f64) -> Vec<DVec2> {
        let t = self.station(z);
        let sh = self.d.shoulder;
        let xi = (self.track - self.d.tyre) * 0.5 - 0.04;
        let mut p = vec![
            DVec2::new(0.0, t.ys),
            DVec2::new(xi, t.ys),
            DVec2::new(xi, t.yw),
        ];
        arc(
            &mut p,
            DVec2::new(t.hw - ROCKER, t.yw + ROCKER),
            ROCKER,
            -FRAC_PI_2,
            0.0,
            2,
        );
        let mid = (t.yw + ROCKER + t.ysh - sh) * 0.5;
        p.push(DVec2::new(t.hw + 0.006, mid));
        arc(
            &mut p,
            DVec2::new(t.hw - sh, t.ysh - sh),
            sh,
            0.0,
            FRAC_PI_2,
            3,
        );
        let rr = 0.004 + self.d.edge * t.open;
        let belt = DVec2::new(t.hw - sh, t.ysh);
        let edge = DVec2::new(t.hwr, t.yr - rr);
        p.push((belt + edge) * 0.5 + DVec2::new(0.01 * t.open, 0.0));
        arc(
            &mut p,
            DVec2::new(t.hwr - rr, t.yr - rr),
            rr,
            0.0,
            FRAC_PI_2,
            2,
        );
        let crown = self.d.crown[0] + self.d.crown[1] * t.open;
        p.push(DVec2::new((t.hwr - rr) * 0.5, t.yr + crown * 0.75));
        p.push(DVec2::new(0.0, t.yr + crown));
        debug_assert_eq!(p.len(), HALF);
        p
    }

    fn stations(&self) -> Vec<f64> {
        let (z0, z1) = (
            -self.length * 0.5 + self.d.tail,
            self.length * 0.5 - self.d.nose,
        );
        let step = if self.fine { 0.045 } else { 0.12 };
        let n = ((z1 - z0) / step).ceil().max(1.0) as usize;
        let edge = self.arch + 0.004;
        let mut zs: Vec<f64> = (0..=n)
            .map(|i| z0 + (z1 - z0) * i as f64 / n as f64)
            .filter(|&z| self.near(z) > edge)
            .collect();
        let arch = if self.fine { 18 } else { 8 };
        for &a in &self.axles {
            for i in 0..=arch {
                zs.push(a - self.arch * (PI * i as f64 / arch as f64).cos());
            }
            zs.push(a - edge);
            zs.push(a + edge);
        }
        for &(s, _) in &self.roof {
            zs.push(self.z(s));
        }
        let pillar = self.z(self.d.pillar);
        zs.push(pillar - 0.035);
        zs.push(pillar + 0.035);
        zs.retain(|&z| z >= z0 && z <= z1);
        zs.sort_by(f64::total_cmp);
        zs.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        zs
    }

    fn shell(&self) -> Shell {
        let zs = self.stations();
        let (z0, z1) = (zs[0], zs[zs.len() - 1]);
        let mut rings: Vec<(f64, Vec<DVec2>)> = Vec::new();
        let steps = 4;
        let inset = |ring: &[DVec2], d: f64| -> Vec<DVec2> {
            let wide = ring.iter().map(|p| p.x.abs()).fold(0.0, f64::max).max(1e-6);
            let lo = ring.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
            let hi = ring.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
            let yc = (lo + hi) * 0.5;
            ring.iter()
                .map(|p| {
                    let y = if p.y > yc {
                        p.y - d * 0.6 * (p.y - yc) / (hi - yc).max(1e-6)
                    } else {
                        p.y + d * 0.3 * (yc - p.y) / (yc - lo).max(1e-6)
                    };
                    DVec2::new(p.x * (1.0 - d / wide), y)
                })
                .collect()
        };
        let rear = self.section(z0);
        for k in (1..=steps).rev() {
            let a = FRAC_PI_2 * k as f64 / steps as f64;
            rings.push((
                z0 - self.d.tail * a.sin(),
                inset(&rear, self.d.tail * (1.0 - a.cos())),
            ));
        }
        for &z in &zs {
            rings.push((z, self.section(z)));
        }
        let front = self.section(z1);
        for k in 1..=steps {
            let a = FRAC_PI_2 * k as f64 / steps as f64;
            rings.push((
                z1 + self.d.nose * a.sin(),
                inset(&front, self.d.nose * (1.0 - a.cos())),
            ));
        }
        let mut shell = Shell {
            pos: Vec::with_capacity(rings.len() * RING + 2),
            paint: Vec::new(),
            trim: Vec::new(),
            glass: Vec::new(),
        };
        for (z, half) in &rings {
            for p in half {
                shell.pos.push(DVec3::new(p.x, p.y, *z));
            }
            for p in half[1..HALF - 1].iter().rev() {
                shell.pos.push(DVec3::new(-p.x, p.y, *z));
            }
        }
        let band = |k: usize| {
            if k < HALF - 1 {
                BANDS[k]
            } else {
                BANDS[RING - 1 - k]
            }
        };
        for i in 0..rings.len() - 1 {
            let mid = (rings[i].0 + rings[i + 1].0) * 0.5;
            let station = self.station(mid);
            let s = self.s(mid);
            for k in 0..RING {
                let kn = (k + 1) % RING;
                let a = (i * RING + k) as u32;
                let b = (i * RING + kn) as u32;
                let c = ((i + 1) * RING + kn) as u32;
                let e = ((i + 1) * RING + k) as u32;
                let list = match band(k) {
                    Band::Under => &mut shell.trim,
                    other if self.glazed(other, s, mid, &station) => &mut shell.glass,
                    _ => &mut shell.paint,
                };
                list.push([a, b, c]);
                list.push([a, c, e]);
            }
        }
        for (ring, flip) in [(0, true), (rings.len() - 1, false)] {
            let start = ring * RING;
            let centre = (start..start + RING)
                .map(|i| shell.pos[i])
                .fold(DVec3::ZERO, |a, p| a + p)
                / RING as f64;
            let c = shell.pos.len() as u32;
            shell.pos.push(centre);
            for k in 0..RING {
                let a = (start + k) as u32;
                let b = (start + (k + 1) % RING) as u32;
                shell.paint.push(if flip { [c, b, a] } else { [c, a, b] });
            }
        }
        shell
    }

    fn glazed(&self, band: Band, s: f64, z: f64, t: &Station) -> bool {
        let r = &self.roof;
        let n = r.len();
        match band {
            Band::Side => {
                t.yr - t.ysh > 0.07
                    && s > r[1].0 - 0.01
                    && s < r[n - 1].0 - 0.015
                    && (z - self.z(self.d.pillar)).abs() > 0.035
            }
            Band::Top => {
                t.yr - self.deck(s) > 0.05
                    && ((s > r[n - 2].0 - 0.01 && s < r[n - 1].0 - 0.012)
                        || (s > r[0].0 + 0.02 && s < r[1].0))
            }
            _ => false,
        }
    }

    fn cap(&self, side: f64) -> (f64, f64, f64) {
        let (z, r) = if side > 0.0 {
            (self.length * 0.5 - self.d.nose, self.d.nose)
        } else {
            (-self.length * 0.5 + self.d.tail, self.d.tail)
        };
        let t = self.station(z);
        let top = t.yr + 0.02;
        let yc = (t.ys + top) * 0.5;
        (
            t.hw - r,
            t.ys + 0.3 * r * (yc - t.ys) / (yc - t.ys).max(1e-6),
            top - 0.6 * r,
        )
    }

    fn lamp(&self, side: f64, size: [f64; 3], x: f64, y: f64, material: &str) -> Mesh {
        let z = side * (self.length * 0.5 + 0.004);
        placed(
            mesh::rounded_box(size, (size[1] * 0.45).min(0.02), 2),
            material,
            DVec3::new(x, y, z),
            DQuat::IDENTITY,
        )
    }

    fn heads(&self) -> ([f64; 2], [DVec3; 2]) {
        let (hw, lo, hi) = self.cap(1.0);
        let y = lo + (hi - lo) * 0.78;
        let (w, h) = if self.style == Style::Sports {
            (0.42, 0.045)
        } else {
            (0.34, 0.1)
        };
        let z = self.length * 0.5 + 0.004;
        let at = |side: f64| DVec3::new(side * (hw - w * 0.5 - 0.04), y, z);
        ([w, h], [at(1.0), at(-1.0)])
    }

    fn headlights(&self, material: &str) -> Mesh {
        let (hw, _, _) = self.cap(1.0);
        let ([w, h], lamps) = self.heads();
        let mut m = Mesh::default();
        for at in [lamps[1], lamps[0]] {
            m.append(&self.lamp(1.0, [w, h, 0.02], at.x, at.y, material));
        }
        if self.style == Style::Sports {
            m.append(&self.lamp(
                1.0,
                [2.0 * (hw - w - 0.04), 0.012, 0.016],
                0.0,
                lamps[0].y + h * 0.25,
                material,
            ));
        }
        m
    }

    fn discs(&self) -> [DVec3; 4] {
        let (hw, lo, hi) = self.cap(-1.0);
        let y = lo + (hi - lo) * 0.7;
        let z = -(self.length * 0.5 + 0.004);
        let at = |x: f64| DVec3::new(x, y, z);
        let (outer, inner) = (hw - 0.1, hw - 0.1 - 2.6 * DISC);
        [at(-outer), at(-inner), at(inner), at(outer)]
    }

    fn tails(&self) -> ([f64; 2], [DVec3; 2]) {
        let (hw, lo, hi) = self.cap(-1.0);
        let (size, x, y) = match self.style {
            Style::Sports => {
                let d = self.discs();
                return ([2.0 * DISC; 2], [(d[2] + d[3]) * 0.5, (d[0] + d[1]) * 0.5]);
            }
            Style::Sedan | Style::Hatch => ([0.34, 0.11], hw - 0.19, 0.72),
            Style::Van => ([0.13, 0.42], hw - 0.08, 0.42),
        };
        let y = lo + (hi - lo) * y;
        let z = -(self.length * 0.5 + 0.004);
        (size, [DVec3::new(x, y, z), DVec3::new(-x, y, z)])
    }

    fn taillights(&self, material: &str) -> Mesh {
        let (hw, _, _) = self.cap(-1.0);
        let mut m = Mesh::default();
        if self.style == Style::Sports {
            for at in self.discs() {
                m.append(&placed(
                    mesh::cylinder(DISC, 0.03, 24, true),
                    material,
                    at,
                    DQuat::from_rotation_x(FRAC_PI_2),
                ));
                m.append(&placed(
                    mesh::cylinder(DISC + 0.012, 0.02, 24, false),
                    TRIM,
                    at + DVec3::Z * 0.006,
                    DQuat::from_rotation_x(FRAC_PI_2),
                ));
            }
            return m;
        }
        let ([w, h], lamps) = self.tails();
        let y = lamps[0].y;
        if matches!(self.style, Style::Sedan | Style::Hatch) {
            m.append(&self.lamp(-1.0, [2.0 * hw - 0.6, 0.02, 0.016], 0.0, y, material));
        }
        for at in [lamps[1], lamps[0]] {
            m.append(&self.lamp(-1.0, [w, h, 0.02], at.x, y, material));
        }
        m
    }

    fn lights(&self, name: &str, head: &Lamp, tail: &Lamp) -> Vec<GeneratedLight> {
        let body = format!("{name}.body");
        let (_, heads) = self.heads();
        let (_, tails) = self.tails();
        let mut out = Vec::new();
        for (side, at) in ["l", "r"].into_iter().zip(heads) {
            out.push(GeneratedLight {
                name: format!("{name}.headlight_{side}"),
                position: (at + DVec3::Z * 0.06).to_array(),
                direction: DVec3::new(0.0, -0.06, 1.0).normalize().to_array(),
                cone: head.cone,
                range: head.range,
                color: head.color,
                intensity: head.intensity,
                shadow: false,
                parent: Some(body.clone()),
            });
        }
        for (side, at) in ["l", "r"].into_iter().zip(tails) {
            out.push(GeneratedLight {
                name: format!("{name}.taillight_{side}"),
                position: (at - DVec3::Z * 0.06).to_array(),
                direction: DVec3::new(0.0, -0.15, -1.0).normalize().to_array(),
                cone: tail.cone,
                range: tail.range,
                color: tail.color,
                intensity: tail.intensity,
                shadow: false,
                parent: Some(body.clone()),
            });
        }
        out
    }

    fn trim(&self, body: &mut Mesh, paint: &str) {
        let half = self.length * 0.5;
        let (fw, flo, fhi) = self.cap(1.0);
        body.append(&placed(
            mesh::rounded_box([2.0 * fw * 0.66, (fhi - flo) * 0.22, 0.03], 0.012, 2),
            TRIM,
            DVec3::new(0.0, flo + (fhi - flo) * 0.25, half - 0.005),
            DQuat::IDENTITY,
        ));
        let (rw, rlo, rhi) = self.cap(-1.0);
        let ys = self.d.clearance;
        if self.style == Style::Sports {
            let valance = (rhi - rlo) * 0.34;
            body.append(&placed(
                mesh::rounded_box([2.0 * rw * 0.94, valance, 0.06], 0.02, 2),
                TRIM,
                DVec3::new(0.0, rlo + valance * 0.5, -half + 0.012),
                DQuat::IDENTITY,
            ));
            body.append(&placed(
                mesh::rounded_box([2.0 * rw * 0.66, 0.09, 0.32], 0.01, 2),
                TRIM,
                DVec3::new(0.0, ys + 0.045, -half + 0.12),
                DQuat::IDENTITY,
            ));
            for k in 0..4 {
                let x = (k as f64 - 1.5) * rw * 0.3;
                body.append(&placed(
                    mesh::rounded_box([0.012, 0.11, 0.3], 0.0, 1),
                    TRIM,
                    DVec3::new(x, ys + 0.04, -half + 0.12),
                    DQuat::IDENTITY,
                ));
            }
            for side in [-1.0, 1.0] {
                body.append(&placed(
                    mesh::cylinder(0.045, 0.14, 20, true),
                    RIM,
                    DVec3::new(side * rw * 0.58, rlo + valance * 0.45, -half + 0.03),
                    DQuat::from_rotation_x(FRAC_PI_2),
                ));
            }
        } else {
            body.append(&placed(
                mesh::rounded_box([2.0 * rw * 0.96, (rhi - rlo) * 0.14, 0.05], 0.015, 2),
                TRIM,
                DVec3::new(0.0, rlo + (rhi - rlo) * 0.07, -half + 0.005),
                DQuat::IDENTITY,
            ));
        }
        let zw = self.z(self.roof[self.roof.len() - 1].0) - 0.12;
        let t = self.station(zw);
        for side in [-1.0, 1.0] {
            body.append(&placed(
                mesh::rounded_box([0.13, 0.08, 0.05], 0.015, 2),
                paint,
                DVec3::new(side * (t.hw - self.d.shoulder + 0.06), t.ysh + 0.07, zw),
                DQuat::from_rotation_y(side * 0.15),
            ));
        }
    }

    fn wheel(&self, centre: DVec3, side: f64) -> Mesh {
        let r = self.radius;
        let w = self.d.tyre * 0.5;
        let rim = r * self.d.rim;
        let sh = (w * 0.4).min(0.035);
        let (segments, arcs) = if self.fine { (40, 3) } else { (16, 1) };
        let mut profile: Vec<DVec2> = vec![
            DVec2::new(rim, -w * 0.9),
            DVec2::new((rim + r) * 0.5, -w - 0.01),
        ];
        arc(
            &mut profile,
            DVec2::new(r - sh, -w + sh),
            sh,
            -FRAC_PI_2,
            0.0,
            arcs,
        );
        arc(
            &mut profile,
            DVec2::new(r - sh, w - sh),
            sh,
            0.0,
            FRAC_PI_2,
            arcs,
        );
        profile.push(DVec2::new((rim + r) * 0.5, w + 0.01));
        profile.push(DVec2::new(rim, w * 0.9));
        profile.push(profile[0]);
        let ring: Vec<[f64; 2]> = profile.iter().map(|p| [p.x, p.y]).collect();
        let mut m = Mesh::default();
        let mut tyre = mesh::lathe(&ring, segments, 1e-7);
        tyre.set_material(TYRE);
        m.append(&tyre);
        let mut back = mesh::disc(rim, segments);
        back.set_material(TRIM);
        m.append(&back);
        let face = w * 0.72;
        m.append(&placed(
            mesh::torus(
                rim * 0.93,
                rim * 0.07,
                segments,
                if self.fine { 6 } else { 4 },
            ),
            RIM,
            DVec3::new(0.0, face, 0.0),
            DQuat::IDENTITY,
        ));
        m.append(&placed(
            mesh::cylinder(rim * 0.22, 0.05, segments / 2, true),
            RIM,
            DVec3::new(0.0, face, 0.0),
            DQuat::IDENTITY,
        ));
        let spokes = if self.style == Style::Sports { 10 } else { 5 };
        for k in 0..spokes {
            let a = TAU * k as f64 / spokes as f64;
            let turn = DQuat::from_rotation_y(a);
            let wide = if spokes > 5 { 0.028 } else { 0.05 };
            m.append(&placed(
                mesh::rounded_box([wide, 0.028, rim * 0.78], 0.0, 1),
                RIM,
                turn * DVec3::new(0.0, face - 0.01, rim * 0.5),
                turn,
            ));
        }
        mesh::transform(
            &mut m,
            DMat4::from_rotation_translation(DQuat::from_rotation_z(-side * FRAC_PI_2), centre),
        );
        m
    }
}

impl Shell {
    fn mesh(&self, tris: &[[u32; 3]], material: &str, degrees: f64) -> Mesh {
        let mut keep = vec![u32::MAX; self.pos.len()];
        let mut m = Mesh::default();
        for t in tris {
            for &i in t {
                let i = i as usize;
                if keep[i] == u32::MAX {
                    keep[i] = m.positions.len() as u32;
                    m.positions.push(self.pos[i].as_vec3().to_array());
                }
                m.indices.push(keep[i]);
            }
        }
        if m.indices.is_empty() {
            return m;
        }
        m.normals = vec![[0.0; 3]; m.positions.len()];
        m.slots = vec![Slot {
            material: material.to_string(),
            start: 0,
            count: m.indices.len() as u32,
        }];
        mesh::smooth_by_angle(&mut m, degrees);
        m
    }
}
