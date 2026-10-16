use crate::carve::{Frame, grooves, hinged, regions};
use crate::{Call, Generated, Piece};
use glam::{DVec2, DVec3};
use manfred_mesh::{Mesh, Slot};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::f64::consts::{PI, TAU};

pub const NAME: &str = "manfred/loft";

const PARAMS: [&str; 9] = [
    "axis",
    "stations",
    "silhouettes",
    "subdivide",
    "samples",
    "seed",
    "regions",
    "grooves",
    "hinged",
];
const STATION: [&str; 5] = ["at", "curve", "half", "crease", "sharpness"];
const VIEWS: [&str; 4] = ["side", "top", "front", "rear"];
const MATERIAL: &str = "loft";
const INFINITE: f64 = 10.0;
const FRONT_BINS: usize = 48;

pub struct Station {
    pub at: f64,
    pub points: Vec<DVec2>,
    pub sharp: Vec<f64>,
}

#[derive(Default)]
pub struct Silhouettes {
    pub side: Option<Vec<DVec2>>,
    pub top: Option<Vec<DVec2>>,
    pub front: Option<Vec<DVec2>>,
    pub rear: Option<Vec<DVec2>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    fn vector(self) -> DVec3 {
        match self {
            Axis::X => DVec3::X,
            Axis::Y => DVec3::Y,
            Axis::Z => DVec3::Z,
        }
    }

    fn place(self, m: &mut Mesh) {
        let swap = match self {
            Axis::Z => return,
            Axis::X => [2, 1, 0],
            Axis::Y => [0, 2, 1],
        };
        for p in m.positions.iter_mut().chain(m.normals.iter_mut()) {
            *p = swap.map(|k| p[k]);
        }
        for t in m.indices.chunks_exact_mut(3) {
            t.swap(1, 2);
        }
    }
}

pub struct Loft {
    pub axis: Axis,
    pub stations: Vec<Station>,
    pub silhouettes: Silhouettes,
    pub subdivide: u32,
    pub around: usize,
    pub along: usize,
}

pub fn generate(call: &Call) -> Result<Generated, String> {
    let loft = parse(call.params)?;
    let cut_regions = regions::parse(call.params.get("regions"), "params.regions")?;
    let cut_grooves = grooves::parse(call.params.get("grooves"), "params.grooves")?;
    let panels = hinged::parse(call.params.get("hinged"), "params.hinged", &cut_grooves)?;
    let mut surface = loft.build()?;
    let name = &call.object.name;
    let [lo, hi] = bounds(&surface);
    loft.axis.place(&mut surface);
    let frame = Frame::along(loft.axis.vector()).unwrap_or(Frame::WORLD);
    let mut pieces = Vec::new();
    for piece in regions::apply(&mut surface, &cut_regions, frame) {
        pieces.push(Piece {
            name: format!("{name}.{}", piece.name),
            mesh: piece.mesh,
        });
    }
    grooves::apply(&mut surface, &cut_grooves, frame);
    let mut pivots = BTreeMap::new();
    let mut opened = serde_json::Map::new();
    for ((piece, pivot), panel) in hinged::split(&mut surface, &panels, &cut_grooves, frame)?
        .into_iter()
        .zip(&panels)
    {
        let node = format!("{name}.{}", piece.name);
        pivots.insert(node.clone(), pivot.to_array());
        opened.insert(
            panel.name.clone(),
            json!({ "pivot": pivot.to_array(), "axis": panel.axis().to_array() }),
        );
        pieces.push(Piece {
            name: node,
            mesh: piece.mesh,
        });
    }
    pieces.insert(
        0,
        Piece {
            name: name.clone(),
            mesh: surface,
        },
    );
    let mut loft_facts = json!({
        "length": hi[2] - lo[2],
        "width": hi[0] - lo[0],
        "height": hi[1] - lo[1],
        "stations": loft.stations.len(),
    });
    if !opened.is_empty() {
        loft_facts["hinged"] = Value::Object(opened);
    }
    let Value::Object(facts) = json!({ "loft": loft_facts }) else {
        unreachable!()
    };
    Ok(Generated {
        pieces,
        facts,
        materials: materials(),
        pivots,
        ..Generated::default()
    })
}

pub fn materials() -> BTreeMap<String, Value> {
    BTreeMap::from([(
        MATERIAL.to_string(),
        json!({
            "family": "lacquer",
            "base": [0.42, 0.44, 0.47],
            "roughness": 0.3,
            "clearcoat": 1.0,
            "clearcoat_roughness": 0.03,
        }),
    )])
}

fn bounds(m: &Mesh) -> [[f64; 3]; 2] {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &m.positions {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k] as f64);
            hi[k] = hi[k].max(p[k] as f64);
        }
    }
    [lo, hi]
}

pub fn parse(params: &BTreeMap<String, Value>) -> Result<Loft, String> {
    if let Some(key) = params.keys().find(|k| !PARAMS.contains(&k.as_str())) {
        return Err(format!(
            "params.{key}: unknown param, expected one of: {}",
            PARAMS.join(", ")
        ));
    }
    let axis = match params.get("axis").map(|v| v.as_str()) {
        None | Some(Some("z")) => Axis::Z,
        Some(Some("x")) => Axis::X,
        Some(Some("y")) => Axis::Y,
        Some(_) => return Err("params.axis: expected \"x\", \"y\" or \"z\"".into()),
    };
    let stations = stations(params.get("stations"))?;
    let silhouettes = silhouettes(params.get("silhouettes"))?;
    let subdivide =
        match params.get("subdivide") {
            None => 2,
            Some(v) => v.as_u64().filter(|n| *n <= 4).ok_or_else(|| {
                "params.subdivide: expected a whole number from 0 to 4".to_string()
            })? as u32,
        };
    if let Some(v) = params.get("seed")
        && v.as_u64().is_none()
    {
        return Err("params.seed: expected a whole number from 0".into());
    }
    let n = stations[0].points.len();
    let mut around = n.max(24);
    let mut along = stations.len().max(16);
    match params.get("samples") {
        None => {}
        Some(Value::Object(t)) => {
            for (key, v) in t {
                let (slot, lo) = match key.as_str() {
                    "around" => (&mut around, n),
                    "along" => (&mut along, 2),
                    _ => {
                        return Err(format!(
                            "params.samples.{key}: unknown key, expected around or along"
                        ));
                    }
                };
                *slot = v
                    .as_u64()
                    .map(|v| v as usize)
                    .filter(|v| *v >= lo && *v <= 1024)
                    .ok_or_else(|| {
                        format!("params.samples.{key}: expected a whole number from {lo} to 1024")
                    })?;
            }
        }
        Some(_) => return Err("params.samples: expected { around, along }".into()),
    }
    Ok(Loft {
        axis,
        stations,
        silhouettes,
        subdivide,
        around,
        along,
    })
}

fn finite(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

fn pairs(v: &Value, key: &str, least: usize) -> Result<Vec<DVec2>, String> {
    let text = || format!("{key}: expected an array of at least {least} [a, b] points");
    let Value::Array(items) = v else {
        return Err(text());
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match item.as_array().map(Vec::as_slice) {
            Some([a, b]) => match (finite(a), finite(b)) {
                (Some(a), Some(b)) => out.push(DVec2::new(a, b)),
                _ => return Err(text()),
            },
            _ => return Err(text()),
        }
    }
    if out.len() < least {
        return Err(text());
    }
    Ok(out)
}

fn stations(value: Option<&Value>) -> Result<Vec<Station>, String> {
    let Some(Value::Array(items)) = value else {
        return Err(
            "params.stations: expected an array of at least 2 stations, { at, curve, half, crease, sharpness }"
                .into(),
        );
    };
    if items.len() < 2 {
        return Err("params.stations: expected at least 2 stations".into());
    }
    let mut out: Vec<Station> = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let key = format!("params.stations.{i}");
        let Value::Object(t) = item else {
            return Err(format!(
                "{key}: expected {{ at, curve, half, crease, sharpness }}"
            ));
        };
        if let Some(k) = t.keys().find(|k| !STATION.contains(&k.as_str())) {
            return Err(format!(
                "{key}.{k}: unknown key, expected one of: {}",
                STATION.join(", ")
            ));
        }
        let at = t
            .get("at")
            .and_then(finite)
            .ok_or_else(|| format!("{key}.at: expected a number, metres along the axis"))?;
        let half = match t.get("half") {
            None => false,
            Some(v) => v
                .as_bool()
                .ok_or_else(|| format!("{key}.half: expected true or false"))?,
        };
        let least = if half { 2 } else { 3 };
        let curve = pairs(
            t.get("curve").unwrap_or(&Value::Null),
            &format!("{key}.curve"),
            least,
        )?;
        let sharpness = match t.get("sharpness") {
            None => INFINITE,
            Some(v) => finite(v)
                .filter(|s| (0.0..=INFINITE).contains(s))
                .ok_or_else(|| format!("{key}.sharpness: expected a number from 0 to 10"))?,
        };
        let mut sharp = vec![0.0; curve.len()];
        if let Some(v) = t.get("crease") {
            let Value::Array(list) = v else {
                return Err(format!("{key}.crease: expected an array of point indices"));
            };
            for c in list {
                let index = c
                    .as_u64()
                    .map(|c| c as usize)
                    .filter(|c| *c < curve.len())
                    .ok_or_else(|| {
                        format!(
                            "{key}.crease: expected point indices from 0 to {}",
                            curve.len() - 1
                        )
                    })?;
                sharp[index] = sharpness;
            }
        }
        let (points, sharp) = if half {
            mirror(curve, sharp)
        } else {
            (curve, sharp)
        };
        if let Some(last) = out.last() {
            if at <= last.at {
                return Err(format!(
                    "{key}.at: {at} is not past the station before it, at {}",
                    last.at
                ));
            }
            if points.len() != last.points.len() {
                return Err(format!(
                    "{key}.curve: {} points round the loop, the station before it has {}; every station needs as many (a half curve counts twice less its two ends)",
                    points.len(),
                    last.points.len()
                ));
            }
        }
        out.push(Station { at, points, sharp });
    }
    Ok(out)
}

fn mirror(mut curve: Vec<DVec2>, sharp: Vec<f64>) -> (Vec<DVec2>, Vec<f64>) {
    let m = curve.len();
    curve[0].x = 0.0;
    curve[m - 1].x = 0.0;
    let mut points = curve.clone();
    let mut out = sharp.clone();
    for i in (1..m - 1).rev() {
        points.push(DVec2::new(-curve[i].x, curve[i].y));
        out.push(sharp[i]);
    }
    (points, out)
}

fn silhouettes(value: Option<&Value>) -> Result<Silhouettes, String> {
    let mut out = Silhouettes::default();
    let Some(value) = value else {
        return Ok(out);
    };
    let Value::Object(t) = value else {
        return Err("params.silhouettes: expected { side, top, front, rear }".into());
    };
    for (key, v) in t {
        let curve = Some(pairs(v, &format!("params.silhouettes.{key}"), 2)?);
        match key.as_str() {
            "side" => out.side = curve,
            "top" => out.top = curve,
            "front" => out.front = curve,
            "rear" => out.rear = curve,
            _ => {
                return Err(format!(
                    "params.silhouettes.{key}: unknown view, expected one of: {}",
                    VIEWS.join(", ")
                ));
            }
        }
    }
    Ok(out)
}

impl Loft {
    pub fn build(&self) -> Result<Mesh, String> {
        let mut cage = self.cage();
        let z0 = self.stations[0].at;
        let z1 = self.stations[self.stations.len() - 1].at;
        fit(&mut cage.points, z0, z1, self.along, &self.silhouettes);
        for _ in 0..self.subdivide {
            cage = subdivide(&cage)?;
        }
        let rings = (self.along - 1) * (1 << self.subdivide) + 1;
        fit(&mut cage.points, z0, z1, rings, &self.silhouettes);
        finish(&cage, z0, z1, rings)
    }

    fn cage(&self) -> Cage {
        let n = self.stations[0].points.len();
        let k = self.stations.len();
        let lengths: Vec<f64> = (0..n)
            .map(|i| {
                self.stations
                    .iter()
                    .map(|s| s.points[i].distance(s.points[(i + 1) % n]))
                    .sum::<f64>()
                    / k as f64
            })
            .collect();
        let total: f64 = lengths.iter().sum::<f64>().max(1e-12);
        let extra = (self.around - n) as f64;
        let counts: Vec<usize> = lengths
            .iter()
            .map(|l| (extra * l / total).round() as usize)
            .collect();
        let rings: Vec<(Vec<DVec2>, Vec<f64>)> = self
            .stations
            .iter()
            .map(|s| refine(&s.points, &s.sharp, &counts))
            .collect();
        let m = rings[0].0.len();
        let span = self.stations[k - 1].at - self.stations[0].at;
        let params: Vec<f64> = self
            .stations
            .iter()
            .map(|s| (s.at - self.stations[0].at) / span)
            .collect();
        let degree = 3.min(k - 1);
        let knots = averaged_knots(&params, degree);
        let matrix: Vec<Vec<f64>> = params
            .iter()
            .map(|&v| {
                let span = find_span(k - 1, degree, v, &knots);
                let basis = basis(span, v, degree, &knots);
                let mut row = vec![0.0; k];
                for (a, b) in basis.iter().enumerate() {
                    row[span - degree + a] = *b;
                }
                row
            })
            .collect();
        let lu = Lu::new(matrix);
        let columns: Vec<Vec<DVec2>> = (0..m)
            .map(|j| {
                let xs = lu.solve(rings.iter().map(|r| r.0[j].x).collect());
                let ys = lu.solve(rings.iter().map(|r| r.0[j].y).collect());
                xs.into_iter()
                    .zip(ys)
                    .map(|(x, y)| DVec2::new(x, y))
                    .collect()
            })
            .collect();
        let r = self.along;
        let mut points = Vec::with_capacity(r * m);
        let mut sharp_rows = Vec::with_capacity(r);
        for row in 0..r {
            let v = row as f64 / (r - 1) as f64;
            let z = self.stations[0].at + span * v;
            let s = find_span(k - 1, degree, v, &knots);
            let b = basis(s, v, degree, &knots);
            for column in &columns {
                let mut p = DVec2::ZERO;
                for (a, w) in b.iter().enumerate() {
                    p += column[s - degree + a] * *w;
                }
                points.push(DVec3::new(p.x, p.y, z));
            }
            let seg = params.windows(2).position(|w| v <= w[1]).unwrap_or(k - 2);
            let t = ((v - params[seg]) / (params[seg + 1] - params[seg])).clamp(0.0, 1.0);
            sharp_rows.push(
                (0..m)
                    .map(|j| rings[seg].1[j] * (1.0 - t) + rings[seg + 1].1[j] * t)
                    .collect::<Vec<f64>>(),
            );
        }
        let at = |row: usize, j: usize| (row * m + j % m) as u32;
        let mut starts = Vec::new();
        let mut verts = Vec::new();
        for row in 0..r - 1 {
            for j in 0..m {
                starts.push(verts.len());
                verts.extend([
                    at(row, j),
                    at(row, j + 1),
                    at(row + 1, j + 1),
                    at(row + 1, j),
                ]);
            }
        }
        starts.push(verts.len());
        verts.extend((0..m).rev().map(|j| at(0, j)));
        starts.push(verts.len());
        verts.extend((0..m).map(|j| at(r - 1, j)));
        let mut sharp = HashMap::new();
        for (row, pair) in sharp_rows.windows(2).enumerate() {
            for (j, (a, b)) in pair[0].iter().zip(&pair[1]).enumerate() {
                let s = (a + b) * 0.5;
                if s > 0.0 {
                    sharp.insert(key(at(row, j), at(row + 1, j)), s);
                }
            }
        }
        let mut cage = Cage {
            points,
            starts,
            verts,
            sharp,
        };
        if cage.volume() < 0.0 {
            cage.flip();
        }
        cage
    }
}

fn refine(points: &[DVec2], sharp: &[f64], counts: &[usize]) -> (Vec<DVec2>, Vec<f64>) {
    let n = points.len();
    let mut out = Vec::new();
    let mut flags = Vec::new();
    for i in 0..n {
        let p1 = points[i];
        let p2 = points[(i + 1) % n];
        let p0 = if sharp[i] > 0.0 {
            p1 * 2.0 - p2
        } else {
            points[(i + n - 1) % n]
        };
        let p3 = if sharp[(i + 1) % n] > 0.0 {
            p2 * 2.0 - p1
        } else {
            points[(i + 2) % n]
        };
        out.push(p1);
        flags.push(sharp[i]);
        let c = counts[i];
        for step in 1..=c {
            out.push(centripetal(p0, p1, p2, p3, step as f64 / (c + 1) as f64));
            flags.push(0.0);
        }
    }
    (out, flags)
}

fn centripetal(p0: DVec2, p1: DVec2, p2: DVec2, p3: DVec2, s: f64) -> DVec2 {
    let knot = |a: DVec2, b: DVec2| a.distance(b).sqrt().max(1e-9);
    let t0 = 0.0;
    let t1 = t0 + knot(p0, p1);
    let t2 = t1 + knot(p1, p2);
    let t3 = t2 + knot(p2, p3);
    let t = t1 + (t2 - t1) * s;
    let a1 = p0 * ((t1 - t) / (t1 - t0)) + p1 * ((t - t0) / (t1 - t0));
    let a2 = p1 * ((t2 - t) / (t2 - t1)) + p2 * ((t - t1) / (t2 - t1));
    let a3 = p2 * ((t3 - t) / (t3 - t2)) + p3 * ((t - t2) / (t3 - t2));
    let b1 = a1 * ((t2 - t) / (t2 - t0)) + a2 * ((t - t0) / (t2 - t0));
    let b2 = a2 * ((t3 - t) / (t3 - t1)) + a3 * ((t - t1) / (t3 - t1));
    b1 * ((t2 - t) / (t2 - t1)) + b2 * ((t - t1) / (t2 - t1))
}

fn averaged_knots(params: &[f64], degree: usize) -> Vec<f64> {
    let n = params.len();
    let mut knots = vec![0.0; degree + 1];
    for i in 1..n - degree {
        knots.push(params[i..i + degree].iter().sum::<f64>() / degree as f64);
    }
    knots.extend(std::iter::repeat_n(1.0, degree + 1));
    knots
}

fn find_span(n: usize, degree: usize, u: f64, knots: &[f64]) -> usize {
    if u >= knots[n + 1] - 1e-12 {
        return n;
    }
    if u <= knots[degree] + 1e-12 {
        return degree;
    }
    let (mut low, mut high) = (degree, n + 1);
    let mut mid = (low + high) / 2;
    while u < knots[mid] || u >= knots[mid + 1] {
        if u < knots[mid] {
            high = mid;
        } else {
            low = mid;
        }
        mid = (low + high) / 2;
    }
    mid
}

fn basis(span: usize, u: f64, degree: usize, knots: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; degree + 1];
    let mut left = vec![0.0; degree + 1];
    let mut right = vec![0.0; degree + 1];
    out[0] = 1.0;
    for j in 1..=degree {
        left[j] = u - knots[span + 1 - j];
        right[j] = knots[span + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            let temp = out[r] / (right[r + 1] + left[j - r]);
            out[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        out[j] = saved;
    }
    out
}

struct Lu {
    a: Vec<Vec<f64>>,
    pivot: Vec<usize>,
}

impl Lu {
    fn new(mut a: Vec<Vec<f64>>) -> Lu {
        let n = a.len();
        let mut pivot: Vec<usize> = (0..n).collect();
        for c in 0..n {
            let p = (c..n)
                .max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()).then(j.cmp(&i)))
                .unwrap_or(c);
            a.swap(c, p);
            pivot.swap(c, p);
            let d = a[c][c];
            if d.abs() < 1e-300 {
                continue;
            }
            let (top, rest) = a.split_at_mut(c + 1);
            let pivot_row = &top[c];
            for row in rest.iter_mut() {
                let f = row[c] / d;
                row[c] = f;
                for (x, p) in row.iter_mut().zip(pivot_row).skip(c + 1) {
                    *x -= f * p;
                }
            }
        }
        Lu { a, pivot }
    }

    fn solve(&self, b: Vec<f64>) -> Vec<f64> {
        let n = self.a.len();
        let mut x: Vec<f64> = self.pivot.iter().map(|&p| b[p]).collect();
        for r in 0..n {
            for k in 0..r {
                x[r] -= self.a[r][k] * x[k];
            }
        }
        for r in (0..n).rev() {
            for k in r + 1..n {
                x[r] -= self.a[r][k] * x[k];
            }
            x[r] /= self.a[r][r];
        }
        x
    }
}

fn key(a: u32, b: u32) -> (u32, u32) {
    if a < b { (a, b) } else { (b, a) }
}

struct Cage {
    points: Vec<DVec3>,
    starts: Vec<usize>,
    verts: Vec<u32>,
    sharp: HashMap<(u32, u32), f64>,
}

impl Cage {
    fn faces(&self) -> usize {
        self.starts.len()
    }

    fn face(&self, f: usize) -> &[u32] {
        let end = self.starts.get(f + 1).copied().unwrap_or(self.verts.len());
        &self.verts[self.starts[f]..end]
    }

    fn volume(&self) -> f64 {
        (0..self.faces())
            .map(|f| {
                let face = self.face(f);
                let a = self.points[face[0] as usize];
                (1..face.len() - 1)
                    .map(|k| {
                        a.dot(
                            self.points[face[k] as usize].cross(self.points[face[k + 1] as usize]),
                        )
                    })
                    .sum::<f64>()
            })
            .sum::<f64>()
            / 6.0
    }

    fn flip(&mut self) {
        for f in 0..self.faces() {
            let start = self.starts[f];
            let end = self.starts.get(f + 1).copied().unwrap_or(self.verts.len());
            self.verts[start..end].reverse();
        }
    }
}

struct Topology {
    edges: Vec<[u32; 2]>,
    halves: Vec<[(u32, u32); 2]>,
    sharp: Vec<f64>,
    face_edges: Vec<u32>,
    vertex_edges: Vec<Vec<u32>>,
    vertex_faces: Vec<Vec<u32>>,
}

fn topology(cage: &Cage) -> Result<Topology, String> {
    let mut index: HashMap<(u32, u32), u32> = HashMap::with_capacity(cage.verts.len());
    let mut edges = Vec::with_capacity(cage.verts.len() / 2);
    let mut halves: Vec<[(u32, u32); 2]> = Vec::with_capacity(cage.verts.len() / 2);
    let mut face_edges = vec![0u32; cage.verts.len()];
    let mut vertex_faces = vec![Vec::new(); cage.points.len()];
    for f in 0..cage.faces() {
        let face = cage.face(f);
        let n = face.len();
        for k in 0..n {
            let (a, b) = (face[k], face[(k + 1) % n]);
            vertex_faces[a as usize].push(f as u32);
            let id = *index.entry(key(a, b)).or_insert_with(|| {
                edges.push([a, b]);
                halves.push([(f as u32, k as u32), (u32::MAX, u32::MAX)]);
                (edges.len() - 1) as u32
            });
            if halves[id as usize][0] != (f as u32, k as u32) {
                if halves[id as usize][1].0 != u32::MAX {
                    return Err("the loft is not manifold: an edge has more than two faces; check the station curves cross nothing".into());
                }
                halves[id as usize][1] = (f as u32, k as u32);
            }
            face_edges[cage.starts[f] + k] = id;
        }
    }
    if halves.iter().any(|h| h[1].0 == u32::MAX) {
        return Err("the loft is not closed".into());
    }
    let mut vertex_edges = vec![Vec::new(); cage.points.len()];
    for (i, e) in edges.iter().enumerate() {
        vertex_edges[e[0] as usize].push(i as u32);
        vertex_edges[e[1] as usize].push(i as u32);
    }
    let sharp = edges
        .iter()
        .map(|e| cage.sharp.get(&key(e[0], e[1])).copied().unwrap_or(0.0))
        .collect();
    Ok(Topology {
        edges,
        halves,
        sharp,
        face_edges,
        vertex_edges,
        vertex_faces,
    })
}

fn decrement(s: f64) -> f64 {
    if s <= 0.0 {
        0.0
    } else if s >= INFINITE {
        INFINITE
    } else if s > 1.0 {
        s - 1.0
    } else {
        0.0
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Rule {
    Smooth,
    Crease,
    Corner,
}

fn rule(count: usize) -> Rule {
    match count {
        0 | 1 => Rule::Smooth,
        2 => Rule::Crease,
        _ => Rule::Corner,
    }
}

fn subdivide(cage: &Cage) -> Result<Cage, String> {
    let topo = topology(cage)?;
    let v = cage.points.len();
    let e = topo.edges.len();
    let faces: Vec<DVec3> = (0..cage.faces())
        .map(|f| {
            let face = cage.face(f);
            face.iter().map(|&i| cage.points[i as usize]).sum::<DVec3>() / face.len() as f64
        })
        .collect();
    let mut points = Vec::with_capacity(v + e + faces.len());
    for i in 0..v {
        let p = cage.points[i];
        let around = &topo.vertex_edges[i];
        let other = |id: u32| {
            let edge = topo.edges[id as usize];
            cage.points[if edge[0] as usize == i {
                edge[1]
            } else {
                edge[0]
            } as usize]
        };
        let parent: Vec<u32> = around
            .iter()
            .copied()
            .filter(|&id| topo.sharp[id as usize] > 0.0)
            .collect();
        let child: Vec<u32> = parent
            .iter()
            .copied()
            .filter(|&id| decrement(topo.sharp[id as usize]) > 0.0)
            .collect();
        let mask = |r: Rule, sharp: &[u32]| match r {
            Rule::Corner => p,
            Rule::Crease => (p * 6.0 + other(sharp[0]) + other(sharp[1])) / 8.0,
            Rule::Smooth => {
                let n = around.len() as f64;
                let edges: DVec3 = around.iter().map(|&id| other(id)).sum();
                let fs: DVec3 = topo.vertex_faces[i]
                    .iter()
                    .map(|&f| faces[f as usize])
                    .sum();
                p * ((n - 2.0) / n) + (edges + fs) / (n * n)
            }
        };
        let (pr, cr) = (rule(parent.len()), rule(child.len()));
        let point = if pr == cr {
            mask(pr, &parent)
        } else {
            let fractional: Vec<f64> = parent
                .iter()
                .map(|&id| topo.sharp[id as usize])
                .filter(|&s| s <= 1.0)
                .collect();
            let w = if fractional.is_empty() {
                0.0
            } else {
                (fractional.iter().sum::<f64>() / fractional.len() as f64).min(1.0)
            };
            mask(pr, &parent) * w + mask(cr, &child) * (1.0 - w)
        };
        points.push(point);
    }
    for (id, edge) in topo.edges.iter().enumerate() {
        let a = cage.points[edge[0] as usize];
        let b = cage.points[edge[1] as usize];
        let s = topo.sharp[id];
        let crease = (a + b) * 0.5;
        let [h0, h1] = topo.halves[id];
        let smooth = (a + b + faces[h0.0 as usize] + faces[h1.0 as usize]) * 0.25;
        points.push(if s <= 0.0 {
            smooth
        } else if decrement(s) > 0.0 {
            crease
        } else {
            crease * s + smooth * (1.0 - s)
        });
    }
    points.extend(&faces);
    let mut starts = Vec::with_capacity(cage.verts.len());
    let mut verts = Vec::with_capacity(cage.verts.len() * 4);
    for f in 0..cage.faces() {
        let face = cage.face(f);
        let n = face.len();
        let s = cage.starts[f];
        let centre = (v + e + f) as u32;
        for (k, &corner) in face.iter().enumerate() {
            starts.push(verts.len());
            verts.extend([
                corner,
                v as u32 + topo.face_edges[s + k],
                centre,
                v as u32 + topo.face_edges[s + (k + n - 1) % n],
            ]);
        }
    }
    let mut sharp = HashMap::new();
    for (id, edge) in topo.edges.iter().enumerate() {
        let d = decrement(topo.sharp[id]);
        if d > 0.0 {
            let mid = (v + id) as u32;
            sharp.insert(key(edge[0], mid), d);
            sharp.insert(key(mid, edge[1]), d);
        }
    }
    Ok(Cage {
        points,
        starts,
        verts,
        sharp,
    })
}

struct Branches(Vec<Vec<DVec2>>);

impl Branches {
    fn new(curve: &[DVec2]) -> Branches {
        let n = curve.len();
        let arg = |better: fn(f64, f64) -> bool| {
            (0..n).fold(0, |b, i| if better(curve[i].x, curve[b].x) { i } else { b })
        };
        let imax = arg(|a, b| a > b);
        let imin = arg(|a, b| a < b);
        let ends = |i: usize| i == 0 || i == n - 1;
        let split = if !ends(imax) {
            Some(imax)
        } else if !ends(imin) {
            Some(imin)
        } else {
            None
        };
        let sorted = |part: &[DVec2]| {
            let mut part = part.to_vec();
            part.sort_by(|a, b| a.x.total_cmp(&b.x));
            part
        };
        Branches(match split {
            Some(i) => vec![sorted(&curve[..=i]), sorted(&curve[i..])],
            None => vec![sorted(curve)],
        })
    }

    fn looped(&self) -> bool {
        self.0.len() == 2
    }

    fn at(&self, a: f64) -> (f64, f64) {
        let values: Vec<f64> = self.0.iter().map(|b| linear(b, a)).collect();
        let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (lo, hi)
    }
}

fn linear(points: &[DVec2], a: f64) -> f64 {
    let n = points.len();
    if a <= points[0].x {
        return points[0].y;
    }
    if a >= points[n - 1].x {
        return points[n - 1].y;
    }
    let i = points.windows(2).position(|w| a <= w[1].x).unwrap_or(n - 2);
    let (p, q) = (points[i], points[i + 1]);
    let d = q.x - p.x;
    if d <= 1e-12 {
        return q.y;
    }
    p.y + (q.y - p.y) * (a - p.x) / d
}

fn half_width(curve: &[DVec2], y: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for w in curve.windows(2) {
        let (p, q) = (w[0], w[1]);
        let (lo, hi) = if p.y < q.y { (p.y, q.y) } else { (q.y, p.y) };
        if y < lo || y > hi {
            continue;
        }
        let x = if hi - lo <= 1e-12 {
            p.x.abs().max(q.x.abs())
        } else {
            (p.x + (q.x - p.x) * (y - p.y) / (q.y - p.y)).abs()
        };
        best = Some(best.map_or(x, |b: f64| b.max(x)));
    }
    best
}

struct Bins {
    z0: f64,
    z1: f64,
    count: usize,
}

impl Bins {
    fn place(&self, z: f64) -> f64 {
        let span = (self.z1 - self.z0).max(1e-12);
        ((z - self.z0) / span * (self.count - 1) as f64).clamp(0.0, (self.count - 1) as f64)
    }

    fn of(&self, z: f64) -> usize {
        self.place(z).round() as usize
    }

    fn centre(&self, b: usize) -> f64 {
        self.z0 + (self.z1 - self.z0) * b as f64 / (self.count - 1) as f64
    }

    fn extremes(&self, points: &[DVec3]) -> Vec<[f64; 4]> {
        let mut out = vec![
            [
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY
            ];
            self.count
        ];
        for p in points {
            let e = &mut out[self.of(p.z)];
            e[0] = e[0].min(p.x);
            e[1] = e[1].max(p.x);
            e[2] = e[2].min(p.y);
            e[3] = e[3].max(p.y);
        }
        let filled: Vec<usize> = (0..self.count).filter(|&b| out[b][0].is_finite()).collect();
        for b in 0..self.count {
            if out[b][0].is_finite() {
                continue;
            }
            let near = filled
                .iter()
                .copied()
                .min_by_key(|&f| (f as i64 - b as i64).abs())
                .unwrap_or(0);
            out[b] = out[near];
        }
        out
    }

    fn warp<T: Copy>(&self, z: f64, table: &[T], mix: impl Fn(T, T, f64) -> T) -> T {
        let place = self.place(z);
        let lo = place.floor() as usize;
        let hi = (lo + 1).min(self.count - 1);
        mix(table[lo], table[hi], place - lo as f64)
    }
}

fn range_map(lo: f64, hi: f64, target: (f64, f64)) -> [f64; 3] {
    let k = if hi - lo > 1e-9 {
        ((target.1 - target.0) / (hi - lo)).clamp(0.05, 20.0)
    } else {
        1.0
    };
    [lo, target.0, k]
}

fn mix3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn fit(points: &mut [DVec3], z0: f64, z1: f64, count: usize, s: &Silhouettes) {
    let bins = Bins { z0, z1, count };
    if let Some(side) = &s.side {
        let side = Branches::new(side);
        let ex = bins.extremes(points);
        let table: Vec<[f64; 3]> = (0..count)
            .map(|b| {
                let (lo, hi) = side.at(bins.centre(b));
                let target = if side.looped() {
                    (lo, hi)
                } else {
                    (ex[b][2], hi)
                };
                range_map(ex[b][2], ex[b][3], target)
            })
            .collect();
        for p in points.iter_mut() {
            let [lo, to, k] = bins.warp(p.z, &table, mix3);
            p.y = to + (p.y - lo) * k;
        }
    }
    if s.front.is_some() || s.rear.is_some() {
        front_rear(points, z0, z1, s);
    }
    if let Some(top) = &s.top {
        let top = Branches::new(top);
        let ex = bins.extremes(points);
        let table: Vec<[f64; 3]> = (0..count)
            .map(|b| {
                let (lo, hi) = top.at(bins.centre(b));
                if top.looped() {
                    range_map(ex[b][0], ex[b][1], (lo, hi))
                } else {
                    let half = ex[b][0].abs().max(ex[b][1].abs());
                    let k = if half > 1e-9 {
                        (hi.abs() / half).clamp(0.05, 20.0)
                    } else {
                        1.0
                    };
                    [0.0, 0.0, k]
                }
            })
            .collect();
        for p in points.iter_mut() {
            let [lo, to, k] = bins.warp(p.z, &table, mix3);
            p.x = to + (p.x - lo) * k;
        }
    }
}

fn front_rear(points: &mut [DVec3], z0: f64, z1: f64, s: &Silhouettes) {
    let (ylo, yhi) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
            (a.min(p.y), b.max(p.y))
        });
    if yhi - ylo <= 1e-9 {
        return;
    }
    let bin = |y: f64| {
        (((y - ylo) / (yhi - ylo)) * (FRONT_BINS - 1) as f64).clamp(0.0, (FRONT_BINS - 1) as f64)
    };
    let weight = |z: f64| smoothstep(z0, z1, z);
    let mut envelope = [[0.0f64; FRONT_BINS]; 2];
    for p in points.iter() {
        let side = usize::from(weight(p.z) >= 0.5);
        let b = bin(p.y).round() as usize;
        envelope[side][b] = envelope[side][b].max(p.x.abs());
    }
    let ratios = |curve: &[DVec2], side: usize| -> Vec<f64> {
        let mut out: Vec<Option<f64>> = (0..FRONT_BINS)
            .map(|b| {
                let y = ylo + (yhi - ylo) * b as f64 / (FRONT_BINS - 1) as f64;
                let e = envelope[side][b];
                half_width(curve, y)
                    .filter(|_| e > 1e-9)
                    .map(|t| (t / e).clamp(0.05, 20.0))
            })
            .collect();
        let known: Vec<usize> = (0..FRONT_BINS).filter(|&b| out[b].is_some()).collect();
        for b in 0..FRONT_BINS {
            if out[b].is_none() {
                out[b] = known
                    .iter()
                    .copied()
                    .min_by_key(|&k| (k as i64 - b as i64).abs())
                    .and_then(|k| out[k]);
            }
        }
        out.into_iter().map(|r| r.unwrap_or(1.0)).collect()
    };
    let front = s
        .front
        .as_deref()
        .or(s.rear.as_deref())
        .map(|c| ratios(c, 1));
    let rear = s
        .rear
        .as_deref()
        .or(s.front.as_deref())
        .map(|c| ratios(c, 0));
    let (Some(front), Some(rear)) = (front, rear) else {
        return;
    };
    let sample = |table: &[f64], y: f64| {
        let place = bin(y);
        let lo = place.floor() as usize;
        let hi = (lo + 1).min(FRONT_BINS - 1);
        table[lo] + (table[hi] - table[lo]) * (place - lo as f64)
    };
    for p in points.iter_mut() {
        let w = weight(p.z);
        let k = sample(&rear, p.y) * (1.0 - w) + sample(&front, p.y) * w;
        p.x *= k;
    }
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0).max(1e-12)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn find(parent: &mut [u32], mut i: u32) -> u32 {
    while parent[i as usize] != i {
        parent[i as usize] = parent[parent[i as usize] as usize];
        i = parent[i as usize];
    }
    i
}

fn union(parent: &mut [u32], a: u32, b: u32) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
        parent[hi as usize] = lo;
    }
}

fn finish(cage: &Cage, z0: f64, z1: f64, rings: usize) -> Result<Mesh, String> {
    let topo = topology(cage)?;
    let corners = cage.verts.len();
    let corner = |f: u32, k: u32| {
        let start = cage.starts[f as usize];
        let n = cage.face(f as usize).len() as u32;
        (start + ((k % n) as usize)) as u32
    };
    let mut parent: Vec<u32> = (0..corners as u32).collect();
    for (id, halves) in topo.halves.iter().enumerate() {
        if topo.sharp[id] >= 1.0 {
            continue;
        }
        let [(f0, k0), (f1, k1)] = *halves;
        union(&mut parent, corner(f0, k0), corner(f1, k1 + 1));
        union(&mut parent, corner(f0, k0 + 1), corner(f1, k1));
    }
    let normals: Vec<DVec3> = (0..cage.faces())
        .map(|f| {
            let face = cage.face(f);
            let n = face.len();
            let mut sum = DVec3::ZERO;
            for k in 0..n {
                let a = cage.points[face[k] as usize];
                let b = cage.points[face[(k + 1) % n] as usize];
                sum += a.cross(b);
            }
            sum * 0.5
        })
        .collect();
    let mut groups = vec![DVec3::ZERO; corners];
    for (f, normal) in normals.iter().enumerate() {
        for c in cage.starts[f]..cage.starts[f] + cage.face(f).len() {
            let root = find(&mut parent, c as u32) as usize;
            groups[root] += *normal;
        }
    }
    let bins = Bins {
        z0,
        z1,
        count: rings,
    };
    let ex = bins.extremes(&cage.points);
    let theta: Vec<f64> = cage
        .points
        .iter()
        .map(|p| {
            let e = ex[bins.of(p.z)];
            let (cx, cy) = ((e[0] + e[1]) * 0.5, (e[2] + e[3]) * 0.5);
            let (dx, dy) = (p.x - cx, p.y - cy);
            if dx.abs() + dy.abs() < 1e-12 {
                return 0.0;
            }
            let a = dx.atan2(-dy);
            if a < 0.0 { a + TAU } else { a }
        })
        .collect();
    let mut by_bin: Vec<Vec<u32>> = vec![Vec::new(); rings];
    for (i, p) in cage.points.iter().enumerate() {
        by_bin[bins.of(p.z)].push(i as u32);
    }
    let mut girth = vec![0.0f64; rings];
    let mut along = vec![0.0f64; cage.points.len()];
    for (b, list) in by_bin.iter_mut().enumerate() {
        list.sort_by(|&i, &j| {
            theta[i as usize]
                .total_cmp(&theta[j as usize])
                .then(i.cmp(&j))
        });
        let mut s = 0.0;
        for w in 0..list.len() {
            let i = list[w] as usize;
            if w > 0 {
                let q = cage.points[list[w - 1] as usize];
                s += DVec2::new(cage.points[i].x - q.x, cage.points[i].y - q.y).length();
            }
            along[i] = s;
        }
        if let (Some(&first), Some(&last)) = (list.first(), list.last()) {
            let (p, q) = (cage.points[first as usize], cage.points[last as usize]);
            girth[b] = s + DVec2::new(p.x - q.x, p.y - q.y).length();
        }
    }
    let mut out = Mesh::default();
    let mut uvs = Vec::new();
    let mut made: HashMap<(u32, u32, bool), u32> = HashMap::new();
    let mut corner_index = vec![0u32; corners];
    for f in 0..cage.faces() {
        let face = cage.face(f);
        let start = cage.starts[f];
        let ts: Vec<f64> = face.iter().map(|&i| theta[i as usize]).collect();
        let lo = ts.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = ts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let seam = hi - lo > PI;
        for (k, &vi) in face.iter().enumerate() {
            let c = start + k;
            let wrap = seam && ts[k] < PI;
            let group = find(&mut parent, c as u32);
            let next = out.positions.len() as u32;
            let index = *made.entry((vi, group, wrap)).or_insert_with(|| {
                let p = cage.points[vi as usize];
                out.positions.push([p.x as f32, p.y as f32, p.z as f32]);
                let n = groups[group as usize].normalize_or_zero();
                out.normals.push([n.x as f32, n.y as f32, n.z as f32]);
                let mut v = along[vi as usize];
                if wrap {
                    v += girth[bins.of(p.z)];
                }
                uvs.push([(p.z - z0) as f32, v as f32]);
                next
            });
            corner_index[c] = index;
        }
        for k in 1..face.len() - 1 {
            out.indices.extend([
                corner_index[start],
                corner_index[start + k],
                corner_index[start + k + 1],
            ]);
        }
    }
    out.uvs = Some(uvs);
    out.slots = vec![Slot {
        material: MATERIAL.to_string(),
        start: 0,
        count: out.indices.len() as u32,
    }];
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn surface(params: Value) -> Mesh {
        let Value::Object(map) = params else {
            unreachable!()
        };
        let params: BTreeMap<String, Value> = map.into_iter().collect();
        let loft = parse(&params).unwrap();
        let mut m = loft.build().unwrap();
        loft.axis.place(&mut m);
        m
    }

    fn shape(crease: Value) -> Value {
        let stations: Vec<Value> = (0..12)
            .map(|i| {
                let z = -2.0 + i as f64 * 0.36;
                let h = 0.6 + 0.4 * (1.0 - ((z - 0.2) / 2.2).powi(2)).max(0.0);
                json!({
                    "at": z,
                    "half": true,
                    "curve": [[0.0, h], [0.55, h - 0.05], [0.85, 0.75], [0.9, 0.4], [0.7, 0.15], [0.0, 0.12]],
                    "crease": crease.clone(),
                })
            })
            .collect();
        json!({
            "stations": stations,
            "silhouettes": { "top": [[-2.0, 0.8], [0.0, 0.92], [2.0, 0.85]] },
            "samples": { "around": 20, "along": 14 },
        })
    }

    fn welded_edges(m: &Mesh) -> BTreeMap<(u32, u32), u32> {
        let mut ids: BTreeMap<[u32; 3], u32> = BTreeMap::new();
        let weld: Vec<u32> = m
            .positions
            .iter()
            .map(|p| {
                let k = p.map(f32::to_bits);
                let next = ids.len() as u32;
                *ids.entry(k).or_insert(next)
            })
            .collect();
        let mut edges = BTreeMap::new();
        for t in m.indices.chunks_exact(3) {
            for k in 0..3 {
                let (a, b) = (weld[t[k] as usize], weld[t[(k + 1) % 3] as usize]);
                *edges.entry(key(a, b)).or_insert(0) += 1;
            }
        }
        edges
    }

    #[test]
    fn the_surface_is_watertight() {
        let m = surface(shape(json!([2])));
        assert!(m.triangle_count() > 1000);
        let edges = welded_edges(&m);
        assert!(edges.values().all(|&c| c == 2));
    }

    #[test]
    fn a_crease_keeps_a_sharp_edge() {
        let split = |m: &Mesh| {
            let mut seen: BTreeMap<[u32; 3], BTreeSet<[u32; 3]>> = BTreeMap::new();
            for (p, n) in m.positions.iter().zip(&m.normals) {
                seen.entry(p.map(f32::to_bits))
                    .or_default()
                    .insert(n.map(|c| (c * 100.0).round().to_bits()));
            }
            seen.values().filter(|s| s.len() > 1).count()
        };
        let smooth = surface(shape(json!([])));
        let sharp = surface(shape(json!([2])));
        let rings = (14 - 1) * 4 + 1;
        assert!(split(&sharp) >= 2 * (rings - 2), "{}", split(&sharp));
        assert!(split(&sharp) > split(&smooth) + rings);
    }

    #[test]
    fn an_x_axis_runs_the_stations_along_x_outward() {
        let mut params = shape(json!([2]));
        params["axis"] = json!("x");
        let m = surface(params);
        let [lo, hi] = bounds(&m);
        assert!(hi[0] - lo[0] > 3.5 && hi[2] - lo[2] < 2.5);
        let volume: f64 = m
            .indices
            .chunks_exact(3)
            .map(|t| {
                let p = [0, 1, 2].map(|k| m.point(t[k]));
                p[0].dot(p[1].cross(p[2]))
            })
            .sum();
        assert!(volume > 0.0);
    }

    #[test]
    fn the_same_params_build_the_same_bytes() {
        let a = surface(shape(json!([2])));
        let b = surface(shape(json!([2])));
        assert_eq!(a, b);
    }

    #[test]
    fn a_half_curve_mirrors_to_a_symmetric_surface() {
        let m = surface(shape(json!([2])));
        let q = |x: f32, y: f32, z: f32| [x, y, z].map(|c| (c as f64 * 1e4).round() as i64);
        let all: BTreeSet<[i64; 3]> = m.positions.iter().map(|p| q(p[0], p[1], p[2])).collect();
        let near = |k: [i64; 3]| {
            (-1..=1).any(|dx| {
                (-1..=1)
                    .any(|dy| (-1..=1).any(|dz| all.contains(&[k[0] + dx, k[1] + dy, k[2] + dz])))
            })
        };
        assert!(m.positions.iter().all(|p| near(q(-p[0], p[1], p[2]))));
        let [lo, hi] = bounds(&m);
        assert!((lo[0] + hi[0]).abs() < 1e-4);
    }
}
