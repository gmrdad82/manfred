use crate::noise::perlin;
use glam::Vec3;
use manfred_build::Piece;
use manfred_mesh::{Mesh, Slot};
use std::cmp::Ordering;
use std::f64::consts::PI;
use std::ops::{Add, Mul, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector(pub [f32; 3]);

impl Vector {
    pub fn new(x: f64, y: f64, z: f64) -> Vector {
        Vector([x as f32, y as f32, z as f32])
    }

    pub fn flat(x: f64, y: f64) -> Vector {
        Vector::new(x, y, 0.0)
    }

    pub fn x(self) -> f64 {
        f64::from(self.0[0])
    }

    pub fn y(self) -> f64 {
        f64::from(self.0[1])
    }

    pub fn scale(self, s: f32) -> Vector {
        Vector(self.0.map(|c| c * s))
    }

    pub fn cross(self, o: Vector) -> Vector {
        let [a, b] = [self.0, o.0];
        Vector([
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ])
    }

    pub fn square(self) -> f64 {
        self.0.iter().rev().map(|&c| f64::from(c * c)).sum()
    }

    pub fn length(self) -> f64 {
        self.square().sqrt()
    }

    pub fn normalized(self) -> Vector {
        let d = self.square();
        if d > 1.0e-35 {
            self.scale(1.0f32 / d.sqrt() as f32)
        } else {
            Vector([0.0; 3])
        }
    }

    pub fn lerp(self, o: Vector, factor: f64) -> Vector {
        let t = 1.0f32 - factor as f32;
        let s = 1.0f32 - t;
        Vector([0, 1, 2].map(|i| s * o.0[i] + t * self.0[i]))
    }

    pub fn point(self) -> [f64; 3] {
        self.0.map(f64::from)
    }

    pub fn yup(self) -> [f32; 3] {
        let [x, y, z] = self.0;
        [x, z, -y]
    }
}

impl Add for Vector {
    type Output = Vector;
    fn add(self, o: Vector) -> Vector {
        Vector([0, 1, 2].map(|i| self.0[i] + o.0[i]))
    }
}

impl Sub for Vector {
    type Output = Vector;
    fn sub(self, o: Vector) -> Vector {
        Vector([0, 1, 2].map(|i| self.0[i] - o.0[i]))
    }
}

impl Mul<f64> for Vector {
    type Output = Vector;
    fn mul(self, s: f64) -> Vector {
        self.scale(s as f32)
    }
}

pub fn noise(p: Vector) -> f64 {
    perlin(p.point())
}

pub fn fractal(p: Vector, octaves: u32) -> f64 {
    crate::noise::fractal(p.point(), octaves)
}

pub fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn interp(table: &[[f64; 2]], x: f64) -> f64 {
    let (first, last) = (table[0], table[table.len() - 1]);
    if x <= first[0] {
        return first[1];
    }
    if x >= last[0] {
        return last[1];
    }
    let (mut lo, mut hi) = (0, table.len() - 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if table[mid][0] <= x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let t = (x - table[lo][0]) / (table[hi][0] - table[lo][0]).max(1e-12);
    table[lo][1] + (table[hi][1] - table[lo][1]) * t
}

#[derive(Clone, Debug)]
pub struct Face {
    pub corners: Vec<u32>,
    pub uvs: Vec<[f64; 2]>,
    pub slot: usize,
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub name: String,
    pub verts: Vec<Vector>,
    pub faces: Vec<Face>,
    pub slots: Vec<String>,
    pub smooth: bool,
}

impl Shape {
    pub fn make(
        name: impl Into<String>,
        verts: Vec<Vector>,
        faces: Vec<Vec<u32>>,
        uvs: &[[f64; 2]],
        material: &str,
        smooth: bool,
    ) -> Shape {
        let faces = faces
            .into_iter()
            .map(|corners| Face {
                uvs: corners.iter().map(|&c| uvs[c as usize]).collect(),
                corners,
                slot: 0,
            })
            .collect();
        Shape {
            name: name.into(),
            verts,
            faces,
            slots: vec![material.to_string()],
            smooth,
        }
    }
}

pub fn grid(
    name: impl Into<String>,
    nu: u32,
    nv: u32,
    mut at: impl FnMut(f64, f64) -> Vector,
    mut uv: impl FnMut(f64, f64) -> [f64; 2],
    material: &str,
    smooth: bool,
) -> Shape {
    let count = ((nu + 1) * (nv + 1)) as usize;
    let mut verts = Vec::with_capacity(count);
    let mut uvs = Vec::with_capacity(count);
    for j in 0..=nv {
        for i in 0..=nu {
            let (u, v) = (f64::from(i) / f64::from(nu), f64::from(j) / f64::from(nv));
            verts.push(at(u, v));
            uvs.push(uv(u, v));
        }
    }
    let mut faces = Vec::with_capacity((nu * nv) as usize);
    for j in 0..nv {
        for i in 0..nu {
            let a = j * (nu + 1) + i;
            faces.push(vec![a, a + 1, a + nu + 2, a + nu + 1]);
        }
    }
    Shape::make(name, verts, faces, &uvs, material, smooth)
}

pub fn tube(
    name: impl Into<String>,
    points: &[Vector],
    radius: impl Fn(f64) -> f64,
    material: &str,
    sides: u32,
    caps: bool,
) -> Shape {
    let n = points.len();
    let mut verts = Vec::with_capacity(n * (sides as usize + 1) + 2);
    let mut uvs = Vec::with_capacity(verts.capacity());
    let mut length = 0.0;
    for (k, &p) in points.iter().enumerate() {
        if k > 0 {
            length += (p - points[k - 1]).length();
        }
        let t = (points[(k + 1).min(n - 1)] - points[k.saturating_sub(1)]).normalized();
        let mut side = t.cross(Vector::new(0.0, 0.0, 1.0));
        if side.length() < 1e-4 {
            side = t.cross(Vector::new(1.0, 0.0, 0.0));
        }
        let side = side.normalized();
        let up = side.cross(t).normalized();
        let r = radius(k as f64 / (n - 1) as f64);
        for s in 0..=sides {
            let a = 2.0 * PI * f64::from(s) / f64::from(sides);
            verts.push(p + (side * a.cos() + up * a.sin()) * r);
            uvs.push([a * r, length]);
        }
    }
    let mut faces = Vec::new();
    for k in 0..n as u32 - 1 {
        for s in 0..sides {
            let a = k * (sides + 1) + s;
            faces.push(vec![a, a + sides + 1, a + sides + 2, a + 1]);
        }
    }
    if caps {
        for (k, flip) in [(0, true), (n - 1, false)] {
            let c = verts.len() as u32;
            verts.push(points[k]);
            uvs.push([0.0, 0.0]);
            let base = k as u32 * (sides + 1);
            for s in 0..sides {
                faces.push(if flip {
                    vec![c, base + s + 1, base + s]
                } else {
                    vec![c, base + s, base + s + 1]
                });
            }
        }
    }
    Shape::make(name, verts, faces, &uvs, material, true)
}

fn chunks(name: &str) -> Vec<(bool, &str)> {
    let bytes = name.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    while start <= bytes.len() {
        let mut end = start;
        while end < bytes.len() && !bytes[end].is_ascii_digit() {
            end += 1;
        }
        out.push((false, &name[start..end]));
        if end == bytes.len() {
            break;
        }
        let digits = end;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        out.push((true, &name[digits..end]));
        start = end;
    }
    out
}

fn number_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

pub fn export_cmp(a: &str, b: &str) -> Ordering {
    let (x, y) = (chunks(a), chunks(b));
    for (p, q) in x.iter().zip(&y) {
        let order = if p.0 {
            number_cmp(p.1, q.1)
        } else {
            p.1.to_lowercase().cmp(&q.1.to_lowercase())
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    x.len().cmp(&y.len())
}

fn corner_angle(a: Vec3, b: Vec3) -> f32 {
    let x = a.dot(b);
    let f = x.abs();
    let m = if f < 1.0 { 1.0 - (1.0 - f) } else { 1.0 };
    let angle = (1.0 - m).sqrt()
        * (std::f32::consts::FRAC_PI_2
            + m * (-0.213_300_99 + m * (0.077_980_48 + m * -0.021_640_95)));
    if x < 0.0 {
        std::f32::consts::PI - angle
    } else {
        angle
    }
}

pub fn unit(v: Vec3) -> Vec3 {
    let length = v.length();
    if length > 0.0 { v / length } else { Vec3::ZERO }
}

fn face_normal(p: &[Vec3]) -> Vec3 {
    let n = match p {
        [a, b, c] => unit((*a - *b).cross(*b - *c)),
        [a, b, c, d] => unit((*a - *c).cross(*b - *d)),
        _ => {
            let mut n = Vec3::ZERO;
            let mut prev = p[p.len() - 1];
            for &cur in p {
                n += Vec3::new(
                    (prev.y - cur.y) * (prev.z + cur.z),
                    (prev.z - cur.z) * (prev.x + cur.x),
                    (prev.x - cur.x) * (prev.y + cur.y),
                );
                prev = cur;
            }
            unit(n)
        }
    };
    if n == Vec3::ZERO { Vec3::Z } else { n }
}

pub fn flips(p: &[Vec3]) -> bool {
    let d13 = p[2] - p[0];
    let a = (p[1] - p[0]).cross(d13);
    let b = (p[3] - p[0]).cross(d13);
    a.dot(b) > 0.0
}

pub fn exported(n: Vec3) -> [f32; 3] {
    let r = n
        .to_array()
        .map(|c| (c * 10000.0).round_ties_even() / 10000.0);
    let norm = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
    let r = if norm != 0.0 { r.map(|c| c / norm) } else { r };
    let r = if r == [0.0; 3] { [0.0, 0.0, 1.0] } else { r };
    let zero = |c: f32| if c == 0.0 { 0.0 } else { c };
    [zero(r[0]), zero(r[2]), zero(-r[1])]
}

fn triangles(len: usize, p: &[Vec3]) -> Vec<[usize; 3]> {
    match len {
        3 => vec![[0, 1, 2]],
        4 if flips(p) => vec![[0, 1, 3], [1, 2, 3]],
        _ => (1..len - 1).map(|k| [0, k, k + 1]).collect(),
    }
}

pub fn piece(shape: &Shape) -> Piece {
    let points: Vec<Vec3> = shape.verts.iter().map(|v| Vec3::from(v.0)).collect();
    let corners =
        |face: &Face| -> Vec<Vec3> { face.corners.iter().map(|&c| points[c as usize]).collect() };
    let normals: Vec<Vec3> = shape
        .faces
        .iter()
        .map(|f| face_normal(&corners(f)))
        .collect();
    let smooth = shape.smooth.then(|| {
        let mut sums = vec![Vec3::ZERO; points.len()];
        for (face, &normal) in shape.faces.iter().zip(&normals) {
            let n = face.corners.len();
            for k in 0..n {
                let at = points[face.corners[k] as usize];
                let before = unit(points[face.corners[(k + n - 1) % n] as usize] - at);
                let after = unit(points[face.corners[(k + 1) % n] as usize] - at);
                sums[face.corners[k] as usize] += normal * corner_angle(before, after);
            }
        }
        sums.into_iter()
            .map(|sum| exported(unit(sum)).map(f32::to_bits))
            .collect::<Vec<_>>()
    });
    type Key = (u32, [u32; 3], [u32; 2]);
    let mut loops: Vec<Key> = Vec::new();
    let mut tris: Vec<(usize, [usize; 3])> = Vec::new();
    for (f, face) in shape.faces.iter().enumerate() {
        let first = loops.len();
        let flat = exported(normals[f]).map(f32::to_bits);
        for (k, &c) in face.corners.iter().enumerate() {
            let normal = match &smooth {
                Some(bits) => bits[c as usize],
                None => flat,
            };
            let [u, v] = face.uvs[k];
            let uv = [u as f32, 1.0f32 - v as f32];
            loops.push((c, normal, uv.map(f32::to_bits)));
        }
        for t in triangles(face.corners.len(), &corners(face)) {
            tris.push((face.slot, t.map(|k| first + k)));
        }
    }
    let mut unique = loops.clone();
    unique.sort_unstable();
    unique.dedup();
    let index = |key: &Key| unique.binary_search(key).unwrap_or_default() as u32;
    let mut mesh = Mesh {
        positions: unique
            .iter()
            .map(|&(v, _, _)| shape.verts[v as usize].yup())
            .collect(),
        normals: unique
            .iter()
            .map(|&(_, n, _)| n.map(f32::from_bits))
            .collect(),
        uvs: Some(
            unique
                .iter()
                .map(|&(_, _, t)| t.map(f32::from_bits))
                .collect(),
        ),
        uvs1: None,
        indices: Vec::with_capacity(tris.len() * 3),
        slots: Vec::new(),
    };
    for (slot, material) in shape.slots.iter().enumerate() {
        let start = mesh.indices.len() as u32;
        for (_, t) in tris.iter().filter(|(s, _)| *s == slot) {
            mesh.indices.extend(t.map(|l| index(&loops[l])));
        }
        let count = mesh.indices.len() as u32 - start;
        if count > 0 {
            mesh.slots.push(Slot {
                material: material.clone(),
                start,
                count,
            });
        }
    }
    Piece {
        name: shape.name.clone(),
        mesh,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_export_order_puts_a_plain_number_before_a_dashed_one_and_counts_numbers() {
        let mut names = vec![
            "rail0_post",
            "rail0_cap-1",
            "rail0_peg-1",
            "rail0_cap1",
            "rail0_peg1",
            "rail0",
            "rail0_slat10",
            "rail0_slat2",
            "Roof",
            "base",
        ];
        names.sort_by(|a, b| export_cmp(a, b));
        assert_eq!(
            names,
            [
                "base",
                "rail0",
                "rail0_cap1",
                "rail0_cap-1",
                "rail0_peg1",
                "rail0_peg-1",
                "rail0_post",
                "rail0_slat2",
                "rail0_slat10",
                "Roof",
            ]
        );
    }

    #[test]
    fn interp_clamps_at_the_ends_and_runs_straight_between() {
        let table = [[0.0, 1.0], [1.0, 3.0], [2.0, 2.0]];
        assert_eq!(interp(&table, -1.0), 1.0);
        assert_eq!(interp(&table, 0.5), 2.0);
        assert_eq!(interp(&table, 1.5), 2.5);
        assert_eq!(interp(&table, 9.0), 2.0);
    }

    #[test]
    fn a_capped_tube_has_its_rings_and_two_centres() {
        let points = [Vector::new(0.0, 0.0, 0.0), Vector::new(1.0, 0.0, 0.0)];
        let shape = tube("t", &points, |_| 0.1, "metal", 14, true);
        assert_eq!(shape.verts.len(), 32);
        let made = piece(&shape);
        assert_eq!(made.mesh.positions.len(), 32);
        assert_eq!(made.mesh.triangle_count(), 56);
    }
}
