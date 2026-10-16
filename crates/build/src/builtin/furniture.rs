use crate::{Call, Generated, GeneratedLight, Piece};
use glam::{DMat4, DVec3};
use manfred_mesh::{Mesh, cylinder, rounded_box, sphere, sweep, transform};
use manfred_motion::Route;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const NAME: &str = "manfred/furniture";

const METAL: &str = "furniture_metal";
const LAMP: &str = "furniture_lamp";
const LAMP_OFF: &str = "furniture_lamp_off";
const WIRE: &str = "furniture_wire";
const SIGN: &str = "furniture_sign";
const SIGNAL: &str = "furniture_signal";
const KEYS: [&str; 18] = [
    "route",
    "offset",
    "spacing",
    "sides",
    "height",
    "arm",
    "lamp",
    "wires",
    "sag",
    "extras",
    "crosswalks",
    "seed",
    "max_lights",
    "intensity",
    "range",
    "color",
    "cone",
    "signs",
];
const WIRE_SEGMENTS: usize = 12;
const CLEAR: f64 = 3.5;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Params {
    pub route: String,
    pub offset: f64,
    pub spacing: [f64; 2],
    pub sides: Vec<f64>,
    pub height: f64,
    pub arm: f64,
    pub lamp: bool,
    pub wires: bool,
    pub sag: f64,
    pub extras: bool,
    pub crosswalks: Vec<f64>,
    pub seed: u64,
    pub max_lights: usize,
    pub intensity: f64,
    pub range: f64,
    pub color: [f64; 3],
    pub cone: [f64; 2],
    pub signs: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pole {
    pub distance: f64,
    pub side: f64,
    pub base: DVec3,
    pub top: DVec3,
    pub inward: DVec3,
    pub along: DVec3,
}

pub fn generate(call: &Call) -> Result<Generated, String> {
    let params = Params::read(call.params, call.seed)?;
    let route = call.route(&params.route).ok_or_else(|| {
        format!(
            "params.route: no route is named `{}`, expected the name of a [[route]]",
            params.route
        )
    })?;
    Ok(furnish(route, &call.object.name, &params))
}

fn number(params: &BTreeMap<String, Value>, key: &str, default: f64) -> Result<f64, String> {
    match params.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("params.{key}: expected a number, found {value}")),
    }
}

fn flag(params: &BTreeMap<String, Value>, key: &str, default: bool) -> Result<bool, String> {
    match params.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| format!("params.{key}: expected true or false, found {value}")),
    }
}

fn numbers<const N: usize>(
    params: &BTreeMap<String, Value>,
    key: &str,
    default: [f64; N],
) -> Result<[f64; N], String> {
    let Some(value) = params.get(key) else {
        return Ok(default);
    };
    let wrong = || format!("params.{key}: expected {N} numbers, found {value}");
    let list = value
        .as_array()
        .filter(|l| l.len() == N)
        .ok_or_else(wrong)?;
    let mut out = [0.0; N];
    for (slot, item) in out.iter_mut().zip(list) {
        *slot = item.as_f64().filter(|v| v.is_finite()).ok_or_else(wrong)?;
    }
    Ok(out)
}

fn crosswalks(params: &BTreeMap<String, Value>) -> Result<Vec<f64>, String> {
    let Some(value) = params.get("crosswalks") else {
        return Ok(Vec::new());
    };
    let wrong = |item: &Value| {
        format!(
            "params.crosswalks: expected distances along the route, or tables with `at`, found {item}"
        )
    };
    let list = value.as_array().ok_or_else(|| wrong(value))?;
    let mut out = list
        .iter()
        .map(|item| {
            item.as_f64()
                .or_else(|| item.get("at").and_then(Value::as_f64))
                .or_else(|| item.get("distance").and_then(Value::as_f64))
                .filter(|v| v.is_finite())
                .ok_or_else(|| wrong(item))
        })
        .collect::<Result<Vec<f64>, String>>()?;
    out.sort_by(f64::total_cmp);
    Ok(out)
}

impl Params {
    pub(crate) fn read(params: &BTreeMap<String, Value>, seed: u64) -> Result<Params, String> {
        if let Some(key) = params.keys().find(|key| !KEYS.contains(&key.as_str())) {
            return Err(format!(
                "params.{key}: unknown, expected one of: {}",
                KEYS.join(", ")
            ));
        }
        let route = match params.get("route") {
            Some(Value::String(name)) => name.clone(),
            Some(value) => {
                return Err(format!(
                    "params.route: expected the name of a [[route]], found {value}"
                ));
            }
            None => return Err("params.route: needed, the name of a [[route]]".into()),
        };
        let sides = match params.get("sides").map(|v| (v, v.as_str())) {
            None | Some((_, Some("both"))) => vec![1.0, -1.0],
            Some((_, Some("right"))) => vec![1.0],
            Some((_, Some("left"))) => vec![-1.0],
            Some((value, _)) => {
                return Err(format!(
                    "params.sides: expected \"both\", \"left\" or \"right\", found {value}"
                ));
            }
        };
        let seed = match params.get("seed") {
            None => seed,
            Some(value) => value.as_u64().ok_or_else(|| {
                format!("params.seed: expected a whole number 0 or more, found {value}")
            })?,
        };
        let max_lights = match params.get("max_lights") {
            None => 128,
            Some(value) => value.as_u64().ok_or_else(|| {
                format!("params.max_lights: expected a whole number 0 or more, found {value}")
            })? as usize,
        };
        let read = Params {
            route,
            offset: number(params, "offset", 8.2)?,
            spacing: numbers(params, "spacing", [22.0, 30.0])?,
            sides,
            height: number(params, "height", 8.0)?,
            arm: number(params, "arm", 1.8)?,
            lamp: flag(params, "lamp", true)?,
            wires: flag(params, "wires", true)?,
            sag: number(params, "sag", 0.4)?,
            extras: flag(params, "extras", true)?,
            crosswalks: crosswalks(params)?,
            seed,
            max_lights,
            intensity: number(params, "intensity", 60.0)?,
            range: number(params, "range", 24.0)?,
            color: numbers(params, "color", [1.0, 0.72, 0.42])?,
            cone: numbers(params, "cone", [35.0, 60.0])?,
            signs: number(params, "signs", 0.2)?,
        };
        read.check()?;
        Ok(read)
    }

    fn check(&self) -> Result<(), String> {
        let refuse = |ok: bool, key: &str, want: &str, found: String| {
            if ok {
                Ok(())
            } else {
                Err(format!("params.{key}: expected {want}, found {found}"))
            }
        };
        refuse(
            self.offset > 0.0,
            "offset",
            "a distance above 0",
            self.offset.to_string(),
        )?;
        refuse(
            self.spacing[0] >= 2.0 && self.spacing[1] >= self.spacing[0],
            "spacing",
            "[min, max] with 2 <= min <= max",
            format!("{:?}", self.spacing),
        )?;
        refuse(
            self.height > 0.5,
            "height",
            "a height above 0.5",
            self.height.to_string(),
        )?;
        refuse(
            (0.0..self.offset).contains(&self.arm),
            "arm",
            "a length from 0 to below the offset",
            self.arm.to_string(),
        )?;
        refuse(
            (0.0..self.height).contains(&self.sag),
            "sag",
            "a drop from 0 to below the height",
            self.sag.to_string(),
        )?;
        refuse(
            self.intensity >= 0.0,
            "intensity",
            "0 or more",
            self.intensity.to_string(),
        )?;
        refuse(
            self.range > 0.0,
            "range",
            "a distance above 0",
            self.range.to_string(),
        )?;
        refuse(
            self.color.iter().all(|c| *c >= 0.0),
            "color",
            "[r, g, b], each 0 or more",
            format!("{:?}", self.color),
        )?;
        refuse(
            0.0 <= self.cone[0] && self.cone[0] <= self.cone[1] && self.cone[1] <= 180.0,
            "cone",
            "[inner, outer] degrees with 0 <= inner <= outer <= 180",
            format!("{:?}", self.cone),
        )?;
        refuse(
            (0.0..=1.0).contains(&self.signs),
            "signs",
            "a chance from 0 to 1",
            self.signs.to_string(),
        )
    }
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

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

fn flat(v: DVec3, fallback: DVec3) -> DVec3 {
    DVec3::new(v.x, 0.0, v.z).normalize_or(fallback)
}

fn spot(route: &Route, distance: f64, side: f64, lateral: f64) -> (DVec3, DVec3, DVec3) {
    let frame = route.at(distance);
    let right = flat(frame.right, DVec3::X);
    let along = flat(frame.tangent, DVec3::Z);
    (
        frame.position + right * side * lateral,
        -right * side,
        along,
    )
}

fn clear_of(crosswalks: &[f64], distance: f64) -> f64 {
    crosswalks
        .iter()
        .find(|&&c| (distance - c).abs() < CLEAR)
        .map_or(distance, |&c| c + CLEAR)
}

pub(crate) fn poles(route: &Route, p: &Params) -> Vec<Pole> {
    let length = route.length();
    let end = if route.closed() {
        length - p.spacing[0] * 0.5
    } else {
        length
    };
    let mut out = Vec::new();
    for (k, &side) in p.sides.iter().enumerate() {
        let mut rng = Rng(p.seed ^ (0xA5A5_0000 + k as u64));
        let mut distance = rng.range(0.25, 0.75) * p.spacing[0];
        while distance <= end {
            let at = clear_of(&p.crosswalks, distance);
            if at > end {
                break;
            }
            let (base, inward, along) = spot(route, at, side, p.offset);
            out.push(Pole {
                distance: at,
                side,
                base,
                top: base + DVec3::Y * p.height,
                inward,
                along,
            });
            distance = at + rng.range(p.spacing[0], p.spacing[1]);
        }
    }
    out
}

pub(crate) fn wires(poles: &[Pole], p: &Params, closed: bool) -> Vec<Vec<DVec3>> {
    let mut out = Vec::new();
    for &side in &p.sides {
        let row: Vec<&Pole> = poles.iter().filter(|pole| pole.side == side).collect();
        let mut pairs: Vec<(&Pole, &Pole)> = row.windows(2).map(|w| (w[0], w[1])).collect();
        if closed && row.len() > 2 {
            pairs.push((row[row.len() - 1], row[0]));
        }
        for (a, b) in pairs {
            out.push(
                (0..=WIRE_SEGMENTS)
                    .map(|i| {
                        let s = i as f64 / WIRE_SEGMENTS as f64;
                        a.top.lerp(b.top, s) - DVec3::Y * (p.sag * 4.0 * s * (1.0 - s))
                    })
                    .collect(),
            );
        }
    }
    out
}

fn placed(mesh: Mesh, origin: DVec3, x: DVec3, y: DVec3, z: DVec3, material: &str) -> Mesh {
    let mut mesh = mesh;
    transform(
        &mut mesh,
        DMat4::from_cols(
            x.extend(0.0),
            y.extend(0.0),
            z.extend(0.0),
            origin.extend(1.0),
        ),
    );
    mesh.set_material(material);
    mesh
}

fn upright(mesh: Mesh, at: DVec3, along: DVec3, inward: DVec3, material: &str) -> Mesh {
    placed(mesh, at, along, DVec3::Y, inward, material)
}

fn head(pole: &Pole, p: &Params) -> DVec3 {
    pole.top + pole.inward * p.arm - DVec3::Y * 0.06
}

fn street_pole(pole: &Pole, p: &Params, into: &mut Mesh) {
    let (along, inward) = (pole.along, pole.inward);
    let post = cylinder(0.085, p.height, 10, true);
    into.append(&upright(
        post,
        pole.base + DVec3::Y * (p.height * 0.5),
        along,
        inward,
        METAL,
    ));
    into.append(&upright(
        cylinder(0.16, 0.5, 10, true),
        pole.base + DVec3::Y * 0.25,
        along,
        inward,
        METAL,
    ));
    if p.arm > 0.0 {
        let start = pole.top - DVec3::Y * 0.2;
        let path = [
            start.to_array(),
            (start + inward * (p.arm * 0.5) + DVec3::Y * 0.12).to_array(),
            (pole.top + inward * p.arm).to_array(),
        ];
        let mut arm = sweep(&path, &[0.045], 6, false, true);
        arm.set_material(METAL);
        into.append(&arm);
    }
    into.append(&upright(
        rounded_box([0.34, 0.12, 0.64], 0.04, 2),
        head(pole, p),
        along,
        inward,
        METAL,
    ));
}

fn lens(pole: &Pole, p: &Params) -> Mesh {
    let material = if p.lamp { LAMP } else { LAMP_OFF };
    upright(
        rounded_box([0.28, 0.03, 0.54], 0.01, 1),
        head(pole, p) - DVec3::Y * 0.07,
        pole.along,
        pole.inward,
        material,
    )
}

fn light(name: String, pole: &Pole, p: &Params) -> GeneratedLight {
    GeneratedLight {
        name,
        position: (head(pole, p) - DVec3::Y * 0.12).to_array(),
        direction: (pole.inward * 0.25 - DVec3::Y).normalize().to_array(),
        cone: p.cone,
        range: p.range,
        color: p.color,
        intensity: p.intensity,
        shadow: false,
        parent: None,
    }
}

fn bollard(at: DVec3, into: &mut Mesh) {
    into.append(&upright(
        cylinder(0.1, 0.85, 10, true),
        at + DVec3::Y * 0.425,
        DVec3::X,
        DVec3::Z,
        METAL,
    ));
    into.append(&upright(
        sphere(0.1, 10, 5),
        at + DVec3::Y * 0.85,
        DVec3::X,
        DVec3::Z,
        METAL,
    ));
}

fn signal(route: &Route, crossing: f64, side: f64, p: &Params, into: &mut Mesh) {
    let (base, inward, along) = spot(route, crossing - side * 2.5, side, p.offset - 0.4);
    let height = 4.2;
    into.append(&upright(
        cylinder(0.1, height, 10, true),
        base + DVec3::Y * (height * 0.5),
        along,
        inward,
        METAL,
    ));
    let housing = base + DVec3::Y * (height - 0.6) + inward * 0.3;
    let facing = -along * side;
    into.append(&upright(
        rounded_box([0.34, 1.0, 0.3], 0.05, 2),
        housing,
        inward,
        facing,
        METAL,
    ));
    for (k, material) in [SIGNAL, METAL, METAL].into_iter().enumerate() {
        into.append(&upright(
            sphere(0.09, 10, 5),
            housing + DVec3::Y * (0.3 - 0.3 * k as f64) + facing * 0.15,
            inward,
            facing,
            material,
        ));
    }
    for step in [-3.2, -2.0, 2.0, 3.2] {
        let (at, _, _) = spot(route, crossing + step, side, p.offset - 0.9);
        bollard(at, into);
    }
}

fn sign(route: &Route, distance: f64, side: f64, p: &Params, rng: &mut Rng, into: &mut Mesh) {
    let (base, inward, along) = spot(route, distance, side, p.offset - 0.3);
    let height = 2.6;
    into.append(&upright(
        cylinder(0.04, height, 8, true),
        base + DVec3::Y * (height * 0.5),
        along,
        inward,
        METAL,
    ));
    let facing = -along * side;
    let plate = if rng.unit() < 0.5 {
        rounded_box([0.62, 0.62, 0.03], 0.02, 1)
    } else {
        rounded_box([0.5, 0.75, 0.03], 0.02, 1)
    };
    into.append(&upright(
        plate,
        base + DVec3::Y * (height - 0.35) + facing * 0.05,
        inward,
        facing,
        SIGN,
    ));
}

fn extras(route: &Route, poles: &[Pole], p: &Params) -> Mesh {
    let mut out = Mesh::default();
    if !p.extras {
        return out;
    }
    let length = route.length();
    for &crossing in &p.crosswalks {
        if (0.0..=length).contains(&crossing) {
            for &side in &p.sides {
                signal(route, crossing, side, p, &mut out);
            }
        }
    }
    let mut rng = Rng(p.seed ^ 0x5157_0000);
    for pair in poles.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if a.side != b.side || rng.unit() >= p.signs {
            continue;
        }
        let distance = (a.distance + b.distance) * 0.5;
        if p.crosswalks.iter().all(|c| (distance - c).abs() > 6.0) {
            sign(route, distance, a.side, p, &mut rng, &mut out);
        }
    }
    out
}

fn materials() -> BTreeMap<String, Value> {
    BTreeMap::from([
        (
            METAL.to_string(),
            json!({"base": [0.2, 0.21, 0.22], "roughness": 0.45, "metalness": 1.0}),
        ),
        (
            LAMP.to_string(),
            json!({"family": "emissive", "base": [1.0, 0.82, 0.6], "emission": [6.0, 4.3, 2.5]}),
        ),
        (
            LAMP_OFF.to_string(),
            json!({"base": [0.85, 0.85, 0.8], "roughness": 0.2}),
        ),
        (
            WIRE.to_string(),
            json!({"base": [0.03, 0.03, 0.03], "roughness": 0.6}),
        ),
        (
            SIGN.to_string(),
            json!({"base": [0.82, 0.83, 0.8], "roughness": 0.5}),
        ),
        (
            SIGNAL.to_string(),
            json!({"family": "emissive", "base": [1.0, 0.15, 0.08], "emission": [6.0, 0.6, 0.3]}),
        ),
    ])
}

pub(crate) fn furnish(route: &Route, object: &str, p: &Params) -> Generated {
    let mut poles = poles(route, p);
    let mut posts = Mesh::default();
    let mut lamps = Mesh::default();
    for pole in &poles {
        street_pole(pole, p, &mut posts);
        lamps.append(&lens(pole, p));
    }
    let mut cables = Mesh::default();
    if p.wires {
        for path in wires(&poles, p, route.closed()) {
            let path: Vec<[f64; 3]> = path.iter().map(|v| v.to_array()).collect();
            let mut wire = sweep(&path, &[0.012], 4, false, false);
            wire.set_material(WIRE);
            cables.append(&wire);
        }
    }
    let extra = extras(route, &poles, p);
    let mut used = std::collections::BTreeSet::new();
    let mut pieces = Vec::new();
    for (suffix, mesh) in [
        ("poles", posts),
        ("wires", cables),
        ("lamps", lamps),
        ("extras", extra),
    ] {
        if mesh.indices.is_empty() {
            continue;
        }
        used.extend(mesh.slots.iter().map(|slot| slot.material.clone()));
        pieces.push(Piece {
            name: format!("{object}.{suffix}"),
            mesh,
        });
    }
    let mut offered = materials();
    offered.retain(|name, _| used.contains(name));
    poles.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    let lights = if p.lamp {
        poles
            .iter()
            .take(p.max_lights)
            .enumerate()
            .map(|(k, pole)| light(format!("{object}.lamp{k}"), pole, p))
            .collect()
    } else {
        Vec::new()
    };
    Generated {
        pieces,
        materials: offered,
        lights,
        ..Generated::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route() -> Route {
        Route::new(
            &[
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 120.0],
                [60.0, 0.0, 200.0],
                [60.0, 0.0, 320.0],
            ],
            false,
        )
        .unwrap()
    }

    fn params(extra: Value) -> Params {
        let mut map: BTreeMap<String, Value> = BTreeMap::from([("route".into(), json!("main"))]);
        if let Value::Object(more) = extra {
            map.extend(more);
        }
        Params::read(&map, 7).unwrap()
    }

    #[test]
    fn same_seed_same_furniture() {
        let p = params(json!({"crosswalks": [100.0, {"at": 250.0}]}));
        let a = furnish(&route(), "f", &p);
        let b = furnish(&route(), "f", &p);
        assert_eq!(
            a.pieces.iter().map(|x| &x.mesh).collect::<Vec<_>>(),
            b.pieces.iter().map(|x| &x.mesh).collect::<Vec<_>>()
        );
        assert_eq!(a.lights, b.lights);
        let names: Vec<&str> = a.pieces.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["f.poles", "f.wires", "f.lamps", "f.extras"]);
        let c = furnish(&route(), "f", &params(json!({"seed": 8})));
        assert_ne!(a.pieces[0].mesh, c.pieces[0].mesh);
    }

    #[test]
    fn wires_hang_from_pole_tops() {
        let p = params(json!({}));
        let poles = poles(&route(), &p);
        let wires = wires(&poles, &p, false);
        assert_eq!(wires.len(), poles.len() - p.sides.len());
        let tops: Vec<DVec3> = poles.iter().map(|pole| pole.top).collect();
        for wire in &wires {
            let (first, last) = (wire[0], wire[wire.len() - 1]);
            assert!(tops.iter().any(|t| t.distance(first) < 1e-9));
            assert!(tops.iter().any(|t| t.distance(last) < 1e-9));
            let low = wire.iter().map(|v| v.y).fold(f64::INFINITY, f64::min);
            assert!((first.y.min(last.y) - low - p.sag).abs() < 1e-9);
        }
        for pole in &poles {
            assert!((pole.top.y - pole.base.y - p.height).abs() < 1e-12);
        }
    }

    #[test]
    fn lights_are_capped_nearest_the_start_first() {
        let all = furnish(&route(), "f", &params(json!({}))).lights;
        assert!(all.len() > 5 && all.len() <= 128);
        let capped = furnish(&route(), "f", &params(json!({"max_lights": 5}))).lights;
        assert_eq!(capped.len(), 5);
        assert_eq!(capped[..], all[..5]);
        let off = furnish(&route(), "f", &params(json!({"lamp": false})));
        assert!(off.lights.is_empty());
        assert!(off.materials.contains_key(LAMP_OFF) && !off.materials.contains_key(LAMP));
    }
}
