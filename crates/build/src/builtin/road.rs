pub const NAME: &str = "manfred/road";

use crate::{Call, Generated, Piece};
use manfred_mesh::Mesh;
use manfred_motion::{Frame, Route};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const ASPHALT: &str = "road_asphalt";
pub const PAINT: &str = "road_paint";
pub const CONCRETE: &str = "road_concrete";

const KEYS: &[&str] = &[
    "route",
    "lanes",
    "lane_width",
    "sidewalk",
    "curb",
    "curb_width",
    "line",
    "dash",
    "dash_gap",
    "centre",
    "edge",
    "crosswalks",
    "every",
    "offset",
    "crosswalk_depth",
    "stripe",
    "stripe_gap",
    "step",
    "seed",
];

const LIFT: f64 = 0.002;
const MIN_STEP: f64 = 0.1;
const TURN: f64 = 0.05;
const CLEAR: f64 = 0.5;
const DOUBLE_GAP: f64 = 0.1;
const WEAR: f64 = 0.12;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Centre {
    Double,
    Dashed,
    Solid,
}

#[derive(Clone, Debug, PartialEq)]
enum Crossings {
    At(Vec<f64>),
    Every { every: f64, offset: f64 },
}

#[derive(Clone, Debug)]
struct Spec {
    route: String,
    lanes: u32,
    lane_width: f64,
    sidewalk: f64,
    curb: f64,
    curb_width: f64,
    line: f64,
    dash: f64,
    dash_gap: f64,
    centre: Centre,
    edge: f64,
    crosswalks: Crossings,
    crosswalk_depth: f64,
    stripe: f64,
    stripe_gap: f64,
    step: f64,
    seed: u64,
}

pub fn generate(call: &Call) -> Result<Generated, String> {
    let spec = spec(call.params, call.seed)?;
    let route = call
        .route(&spec.route)
        .ok_or_else(|| format!("params.route: no route is named `{}`", spec.route))?;
    road(&call.object.name, route, &spec)
}

fn number(params: &BTreeMap<String, Value>, key: &str, default: f64) -> Result<f64, String> {
    match params.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("params.{key}: expected a number")),
    }
}

fn above(params: &BTreeMap<String, Value>, key: &str, default: f64) -> Result<f64, String> {
    let value = number(params, key, default)?;
    if value > 0.0 {
        Ok(value)
    } else {
        Err(format!("params.{key}: expected a number above 0"))
    }
}

fn at_least_zero(params: &BTreeMap<String, Value>, key: &str, default: f64) -> Result<f64, String> {
    let value = number(params, key, default)?;
    if value >= 0.0 {
        Ok(value)
    } else {
        Err(format!("params.{key}: expected a number of 0 or more"))
    }
}

fn spec(params: &BTreeMap<String, Value>, seed: u64) -> Result<Spec, String> {
    if let Some(key) = params.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(format!(
            "params.{key}: unknown key, expected one of: {}",
            KEYS.join(", ")
        ));
    }
    let route = match params.get("route") {
        Some(Value::String(name)) if !name.is_empty() => name.clone(),
        Some(_) => return Err("params.route: expected a route's name".to_string()),
        None => return Err("params.route: expected the name of a [[route]]".to_string()),
    };
    let lanes = match params.get("lanes") {
        None => 2,
        Some(value) => value
            .as_u64()
            .filter(|n| (1..=8).contains(n))
            .ok_or_else(|| "params.lanes: expected a whole number from 1 to 8".to_string())?
            as u32,
    };
    let centre = match params.get("centre") {
        None => Centre::Double,
        Some(Value::String(s)) if s == "double" => Centre::Double,
        Some(Value::String(s)) if s == "dashed" => Centre::Dashed,
        Some(Value::String(s)) if s == "solid" => Centre::Solid,
        Some(_) => {
            return Err("params.centre: expected \"double\", \"dashed\" or \"solid\"".to_string());
        }
    };
    let seed = match params.get("seed") {
        None => seed,
        Some(value) => value
            .as_u64()
            .ok_or_else(|| "params.seed: expected a whole number of 0 or more".to_string())?,
    };
    let lane_width = above(params, "lane_width", 3.2)?;
    let sidewalk = above(params, "sidewalk", 3.0)?;
    let curb = at_least_zero(params, "curb", 0.15)?;
    let curb_width = above(params, "curb_width", 0.3)?.min(sidewalk);
    let line = above(params, "line", 0.15)?;
    let edge = at_least_zero(params, "edge", 0.3)?;
    let half = lanes as f64 * lane_width;
    if edge >= half {
        return Err("params.edge: expected less than one direction's width".to_string());
    }
    let crosswalks = match (params.get("crosswalks"), params.get("every")) {
        (Some(_), Some(_)) => {
            return Err("params.every: expected `crosswalks` or `every`, not both".to_string());
        }
        (Some(Value::Array(list)), None) => Crossings::At(
            list.iter()
                .map(|v| {
                    v.as_f64().filter(|d| d.is_finite()).ok_or_else(|| {
                        "params.crosswalks: expected distances along the route in metres"
                            .to_string()
                    })
                })
                .collect::<Result<Vec<f64>, String>>()?,
        ),
        (Some(_), None) => {
            return Err(
                "params.crosswalks: expected distances along the route in metres".to_string(),
            );
        }
        (None, Some(_)) => {
            let every = above(params, "every", 1.0)?;
            Crossings::Every {
                every,
                offset: number(params, "offset", every * 0.5)?,
            }
        }
        (None, None) => Crossings::At(Vec::new()),
    };
    if params.contains_key("offset") && !params.contains_key("every") {
        return Err("params.offset: expected with `every`".to_string());
    }
    Ok(Spec {
        route,
        lanes,
        lane_width,
        sidewalk,
        curb,
        curb_width,
        line,
        dash: above(params, "dash", 3.0)?,
        dash_gap: above(params, "dash_gap", 6.0)?,
        centre,
        edge,
        crosswalks,
        crosswalk_depth: above(params, "crosswalk_depth", 4.0)?,
        stripe: above(params, "stripe", 0.5)?,
        stripe_gap: above(params, "stripe_gap", 0.6)?,
        step: above(params, "step", 1.0)?.max(MIN_STEP),
        seed,
    })
}

struct Rng(u64);

impl Rng {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn crosswalk_list(spec: &Spec, length: f64, closed: bool) -> Result<Vec<f64>, String> {
    let half = spec.crosswalk_depth * 0.5;
    let mut list = match &spec.crosswalks {
        Crossings::Every { every, offset } => {
            if *every <= spec.crosswalk_depth {
                return Err(format!(
                    "params.every: expected more than the crosswalk's depth, {} m",
                    spec.crosswalk_depth
                ));
            }
            let start = offset.rem_euclid(*every);
            let count = ((length - start) / every).floor().max(-1.0) as i64 + 1;
            (0..count.max(0))
                .map(|k| start + k as f64 * every)
                .filter(|d| closed || (*d >= half && *d <= length - half))
                .filter(|d| !closed || *d < length)
                .collect::<Vec<f64>>()
        }
        Crossings::At(at) => {
            let mut list = Vec::with_capacity(at.len());
            for &d in at {
                if closed {
                    list.push(d.rem_euclid(length));
                } else if d < half || d > length - half {
                    return Err(format!(
                        "params.crosswalks: {d} is off the route, expected from {half} to {}",
                        length - half
                    ));
                } else {
                    list.push(d);
                }
            }
            list
        }
    };
    list.sort_by(f64::total_cmp);
    list.dedup();
    Ok(list)
}

fn stations(route: &Route, max_step: f64) -> Vec<(f64, Frame)> {
    let length = route.length();
    let mut out = Vec::with_capacity((length / max_step) as usize + 2);
    let mut d = 0.0;
    loop {
        out.push((d, route.at(d)));
        if d >= length {
            break;
        }
        let k = route.curvature(d).abs();
        let step = if k > 1e-9 {
            (TURN / k).clamp(MIN_STEP, max_step)
        } else {
            max_step
        };
        d = (d + step).min(length);
    }
    out
}

fn span(route: &Route, stations: &[(f64, Frame)], from: f64, to: f64) -> Vec<(f64, Frame)> {
    let first = stations.partition_point(|(d, _)| *d <= from);
    let last = stations.partition_point(|(d, _)| *d < to);
    let mut out = Vec::with_capacity(last.saturating_sub(first) + 2);
    out.push((from, route.at(from)));
    if first < last {
        out.extend_from_slice(&stations[first..last]);
    }
    out.push((to, route.at(to)));
    out
}

fn vertex(frame: &Frame, x: f64, h: f64) -> [f32; 3] {
    let p = frame.position + frame.right * x + frame.up * h;
    p.as_vec3().to_array()
}

fn flat(mesh: &mut Mesh, run: &[(f64, Frame)], x0: f64, x1: f64, h: f64, u0: f64) {
    let base = mesh.positions.len() as u32;
    let uvs = mesh.uvs.get_or_insert_with(Vec::new);
    for (d, frame) in run {
        let n = frame.up.as_vec3().to_array();
        for x in [x0, x1] {
            mesh.positions.push(vertex(frame, x, h));
            mesh.normals.push(n);
            uvs.push([(x - u0) as f32, *d as f32]);
        }
    }
    for i in 0..run.len().saturating_sub(1) as u32 {
        let a = base + 2 * i;
        mesh.indices
            .extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
    }
}

fn wall(mesh: &mut Mesh, run: &[(f64, Frame)], x: f64, h0: f64, h1: f64, facing: f64) {
    let base = mesh.positions.len() as u32;
    let uvs = mesh.uvs.get_or_insert_with(Vec::new);
    for (d, frame) in run {
        let n = (frame.right * facing).as_vec3().to_array();
        for h in [h0, h1] {
            mesh.positions.push(vertex(frame, x, h));
            mesh.normals.push(n);
            uvs.push([(h - h0) as f32, *d as f32]);
        }
    }
    for i in 0..run.len().saturating_sub(1) as u32 {
        let a = base + 2 * i;
        if facing > 0.0 {
            mesh.indices
                .extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]);
        } else {
            mesh.indices
                .extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
    }
}

fn minus(from: f64, to: f64, gaps: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut start = from;
    for &(a, b) in gaps {
        if b <= start || a >= to {
            continue;
        }
        if a > start {
            out.push((start, a));
        }
        start = start.max(b);
    }
    if start < to {
        out.push((start, to));
    }
    out
}

fn line_runs(
    spec: &Spec,
    length: f64,
    dashed: bool,
    phase: f64,
    gaps: &[(f64, f64)],
) -> Vec<(f64, f64)> {
    if !dashed {
        return minus(0.0, length, gaps);
    }
    let pitch = spec.dash + spec.dash_gap;
    let mut out = Vec::new();
    let mut a = phase - pitch;
    while a < length {
        let b = (a + spec.dash).min(length);
        if b > 0.0 {
            out.extend(minus(a.max(0.0), b, gaps));
        }
        a += pitch;
    }
    out.retain(|(a, b)| b - a > 0.05);
    out
}

fn named(mesh: Mesh, name: String, material: &str) -> Option<Piece> {
    if mesh.indices.is_empty() {
        return None;
    }
    let mut mesh = mesh;
    mesh.set_material(material);
    Some(Piece { name, mesh })
}

pub fn materials() -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    out.insert(
        ASPHALT.to_string(),
        json!({ "base": [0.045, 0.045, 0.05], "roughness": 0.6 }),
    );
    out.insert(
        PAINT.to_string(),
        json!({
            "base": [0.82, 0.82, 0.8],
            "roughness": 0.55,
            "ageing": {
                "fade": 0.1, "yellow": 0.05, "ink": 0.0, "scratch": 0.3,
                "edge": 0.0, "dust": 0.25, "patina": 0.0, "seed": 0
            }
        }),
    );
    out.insert(
        CONCRETE.to_string(),
        json!({ "base": [0.5, 0.49, 0.46], "roughness": 0.85 }),
    );
    out
}

fn road(object: &str, route: &Route, spec: &Spec) -> Result<Generated, String> {
    let length = route.length();
    if length <= spec.crosswalk_depth {
        return Err(format!(
            "params.route: the route `{}` is {length:.2} m long, expected longer than a crosswalk",
            spec.route
        ));
    }
    let closed = route.closed();
    let crosswalks = crosswalk_list(spec, length, closed)?;
    let all = stations(route, spec.step);
    let whole = &all[..];
    let lw = spec.lane_width;
    let half = spec.lanes as f64 * lw;
    let inner = half + spec.curb_width;
    let outer = half + spec.sidewalk;
    let mut rng = Rng(spec.seed);

    let mut asphalt = Mesh::default();
    flat(&mut asphalt, whole, -half, half, 0.0, -half);

    let mut curbs = Mesh::default();
    let mut sidewalks = Mesh::default();
    for side in [-1.0, 1.0] {
        wall(&mut curbs, whole, side * half, 0.0, spec.curb, -side);
        let (a, b) = if side > 0.0 {
            (half, inner)
        } else {
            (-inner, -half)
        };
        flat(&mut curbs, whole, a, b, spec.curb, a);
        if outer > inner {
            let (a, b) = if side > 0.0 {
                (inner, outer)
            } else {
                (-outer, -inner)
            };
            flat(&mut sidewalks, whole, a, b, spec.curb, a);
        }
        wall(&mut sidewalks, whole, side * outer, 0.0, spec.curb, side);
    }

    let depth = spec.crosswalk_depth * 0.5;
    let mut gaps: Vec<(f64, f64)> = Vec::with_capacity(crosswalks.len() + 2);
    for c in &crosswalks {
        let (a, b) = (c - depth - CLEAR, c + depth + CLEAR);
        gaps.push((a, b));
        if closed {
            gaps.push((a + length, b + length));
            gaps.push((a - length, b - length));
        }
    }
    gaps.sort_by(|x, y| x.0.total_cmp(&y.0));

    let mut markings = Mesh::default();
    let h = LIFT;
    let w = spec.line * 0.5;
    let mut lines: Vec<(f64, bool)> = Vec::new();
    match spec.centre {
        Centre::Double => {
            let x = DOUBLE_GAP * 0.5 + w;
            lines.push((-x, false));
            lines.push((x, false));
        }
        Centre::Dashed => lines.push((0.0, true)),
        Centre::Solid => lines.push((0.0, false)),
    }
    for k in 1..spec.lanes {
        let x = k as f64 * lw;
        lines.push((-x, true));
        lines.push((x, true));
    }
    if spec.edge > 0.0 {
        lines.push((-(half - spec.edge), false));
        lines.push((half - spec.edge, false));
    }
    let phase = rng.unit() * (spec.dash + spec.dash_gap);
    for (x, dashed) in lines {
        for (a, b) in line_runs(spec, length, dashed, phase, &gaps) {
            flat(
                &mut markings,
                &span(route, whole, a, b),
                x - w,
                x + w,
                h,
                x - w,
            );
        }
    }

    let mut zebra = Mesh::default();
    let pitch = spec.stripe + spec.stripe_gap;
    let usable = 2.0 * (half - spec.edge.max(0.0)) + spec.stripe_gap;
    let count = (usable / pitch).floor().max(0.0) as u32;
    let first = -(count as f64 * pitch - spec.stripe_gap) * 0.5;
    for &c in &crosswalks {
        for i in 0..count {
            let x0 = first + i as f64 * pitch;
            let a = c - depth + rng.unit() * WEAR;
            let b = c + depth - rng.unit() * WEAR;
            for (a, b) in wrapped(a, b, length, closed) {
                flat(
                    &mut zebra,
                    &span(route, whole, a, b),
                    x0,
                    x0 + spec.stripe,
                    h,
                    x0,
                );
            }
        }
    }

    let mut lane_centres = Vec::with_capacity(2 * spec.lanes as usize);
    for k in (0..spec.lanes).rev() {
        lane_centres.push(-(k as f64 * lw + lw * 0.5));
    }
    for k in 0..spec.lanes {
        lane_centres.push(k as f64 * lw + lw * 0.5);
    }

    let pieces = [
        named(asphalt, format!("{object}.asphalt"), ASPHALT),
        named(curbs, format!("{object}.curbs"), CONCRETE),
        named(sidewalks, format!("{object}.sidewalks"), CONCRETE),
        named(markings, format!("{object}.markings"), PAINT),
        named(zebra, format!("{object}.crosswalks"), PAINT),
    ]
    .into_iter()
    .flatten()
    .collect();

    let mut facts = serde_json::Map::new();
    facts.insert(
        "road".to_string(),
        json!({
            "route": spec.route,
            "length": length,
            "width": 2.0 * half,
            "lanes": spec.lanes,
            "lane_width": lw,
            "lane_centres": lane_centres,
            "curb": spec.curb,
            "sidewalk": [half, outer],
            "outer": outer,
            "crosswalks": crosswalks,
            "crosswalk_depth": spec.crosswalk_depth,
        }),
    );
    Ok(Generated {
        pieces,
        facts,
        materials: materials(),
        ..Generated::default()
    })
}

fn wrapped(a: f64, b: f64, length: f64, closed: bool) -> Vec<(f64, f64)> {
    if !closed {
        return vec![(a.max(0.0), b.min(length))];
    }
    if a < 0.0 {
        vec![(a + length, length), (0.0, b)]
    } else if b > length {
        vec![(a, length), (0.0, b - length)]
    } else {
        vec![(a, b)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(text: &str) -> BTreeMap<String, Value> {
        serde_json::from_str(text).unwrap()
    }

    fn street() -> Route {
        Route::new(
            &[
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 200.0],
                [40.0, 0.0, 400.0],
                [40.0, 0.0, 600.0],
            ],
            false,
        )
        .unwrap()
    }

    fn bytes(generated: &Generated) -> Vec<u8> {
        let mut out = Vec::new();
        for piece in &generated.pieces {
            out.extend_from_slice(piece.name.as_bytes());
            let m = &piece.mesh;
            for p in m.positions.iter().chain(&m.normals) {
                for v in p {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
            for uv in m.uvs.iter().flatten() {
                for v in uv {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
            for i in &m.indices {
                out.extend_from_slice(&i.to_le_bytes());
            }
        }
        out
    }

    fn built(text: &str, seed: u64) -> Generated {
        road("main", &street(), &spec(&params(text), seed).unwrap()).unwrap()
    }

    #[test]
    fn the_same_seed_builds_the_same_bytes_and_another_seed_wears_the_paint_differently() {
        let text = r#"{ "route": "street", "every": 80, "offset": 30 }"#;
        let a = bytes(&built(text, 7));
        assert_eq!(a, bytes(&built(text, 7)));
        assert_ne!(a, bytes(&built(text, 8)));
        let pinned = r#"{ "route": "street", "every": 80, "offset": 30, "seed": 3 }"#;
        assert_eq!(bytes(&built(pinned, 7)), bytes(&built(pinned, 8)));
    }

    #[test]
    fn the_facts_give_the_lanes_the_sidewalk_and_the_crosswalks() {
        let g = built(r#"{ "route": "street", "every": 100, "offset": 50 }"#, 1);
        let road = &g.facts["road"];
        let near = |value: &Value, expected: &[f64]| {
            let got: Vec<f64> = match value {
                Value::Array(list) => list.iter().map(|v| v.as_f64().unwrap()).collect(),
                one => vec![one.as_f64().unwrap()],
            };
            assert_eq!(got.len(), expected.len(), "{value}");
            for (g, e) in got.iter().zip(expected) {
                assert!((g - e).abs() < 1e-9, "{value}");
            }
        };
        near(&road["width"], &[12.8]);
        assert_eq!(road["lanes"], json!(2));
        near(&road["lane_width"], &[3.2]);
        near(&road["lane_centres"], &[-4.8, -1.6, 1.6, 4.8]);
        near(&road["sidewalk"], &[6.4, 9.4]);
        near(&road["outer"], &[9.4]);
        let length = road["length"].as_f64().unwrap();
        let expected: Vec<f64> = (0..10)
            .map(|k| 50.0 + 100.0 * k as f64)
            .filter(|d| *d <= length - 2.0)
            .collect();
        assert_eq!(road["crosswalks"], json!(expected));
        let names: Vec<&str> = g.pieces.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "main.asphalt",
                "main.curbs",
                "main.sidewalks",
                "main.markings",
                "main.crosswalks"
            ]
        );
        let listed = built(
            r#"{ "route": "street", "crosswalks": [120, 40], "lanes": 1 }"#,
            1,
        );
        assert_eq!(listed.facts["road"]["crosswalks"], json!([40.0, 120.0]));
        near(&listed.facts["road"]["lane_centres"], &[-1.6, 1.6]);
    }

    #[test]
    fn the_materials_are_ones_pfx_reads() {
        for (name, value) in materials() {
            assert!(
                serde_json::from_value::<pfx_materials::Material>(value).is_ok(),
                "{name}"
            );
        }
    }
}
