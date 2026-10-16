use super::car::{hex, srgb};
use crate::Piece;
use glam::{DMat4, DQuat, DVec2, DVec3};
use manfred_mesh::outline::Outline;
use manfred_mesh::{self as mesh, Mesh, Profile, Slot, UvMode};
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) type Params = BTreeMap<String, Value>;

pub(super) fn known(params: &Params, keys: &[&str], kind: &str) -> Result<(), String> {
    match params.keys().find(|k| !keys.contains(&k.as_str())) {
        Some(key) => Err(format!(
            "params.{key}: unknown param for a {kind}, expected one of: {}",
            keys.join(", ")
        )),
        None => Ok(()),
    }
}

pub(super) fn number(params: &Params, key: &str, path: &str) -> Result<Option<f64>, String> {
    match params.get(key) {
        None => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|n| n.is_finite())
            .map(Some)
            .ok_or_else(|| format!("{path}: expected a number")),
    }
}

pub(super) fn ranged(
    params: &Params,
    key: &str,
    default: f64,
    lo: f64,
    hi: f64,
) -> Result<f64, String> {
    nested(params, key, &format!("params.{key}"), default, lo, hi)
}

pub(super) fn nested(
    params: &Params,
    key: &str,
    path: &str,
    default: f64,
    lo: f64,
    hi: f64,
) -> Result<f64, String> {
    let v = number(params, key, path)?.unwrap_or(default);
    if v < lo || v > hi {
        return Err(format!(
            "{path}: {v} is out of range, expected {lo} to {hi}"
        ));
    }
    Ok(v)
}

pub(super) fn whole(
    params: &Params,
    key: &str,
    default: u64,
    lo: u64,
    hi: u64,
) -> Result<u64, String> {
    let v = match params.get(key) {
        None => default,
        Some(v) => v
            .as_u64()
            .ok_or_else(|| format!("params.{key}: expected a whole number"))?,
    };
    if v < lo || v > hi {
        return Err(format!(
            "params.{key}: {v} is out of range, expected {lo} to {hi}"
        ));
    }
    Ok(v)
}

pub(super) fn choice<'a>(
    params: &Params,
    key: &str,
    default: &'a str,
    options: &[&'a str],
) -> Result<&'a str, String> {
    match params.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_str()
            .and_then(|s| options.iter().find(|o| **o == s).copied())
            .ok_or_else(|| format!("params.{key}: expected one of: {}", options.join(", "))),
    }
}

pub(super) fn flag(params: &Params, key: &str, default: bool) -> Result<bool, String> {
    match params.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| format!("params.{key}: expected true or false")),
    }
}

pub(super) fn material(params: &Params, default: &str) -> Result<String, String> {
    match params.get("material") {
        None => Ok(default.to_string()),
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        Some(_) => Err("params.material: expected the name of a material".into()),
    }
}

pub(super) fn colour(value: &Value, path: &str) -> Result<[u8; 3], String> {
    value
        .as_str()
        .and_then(hex)
        .ok_or_else(|| format!("{path}: expected a colour \"#rrggbb\""))
}

pub(super) fn linear(rgb: [u8; 3]) -> [f64; 3] {
    rgb.map(|c| srgb(c as f64 / 255.0))
}

pub(super) fn pairs<const N: usize>(
    value: &Value,
    path: &str,
    min: usize,
) -> Result<Vec<[f64; N]>, String> {
    let shape = if N == 2 { "[x, y]" } else { "[x, y, z]" };
    let bad = || format!("{path}: expected an array of at least {min} points {shape}");
    let items = value.as_array().ok_or_else(bad)?;
    if items.len() < min {
        return Err(bad());
    }
    items
        .iter()
        .map(|item| {
            let xs = item.as_array().filter(|a| a.len() == N).ok_or_else(bad)?;
            let mut out = [0.0; N];
            for (o, x) in out.iter_mut().zip(xs) {
                *o = x.as_f64().filter(|n| n.is_finite()).ok_or_else(bad)?;
            }
            Ok(out)
        })
        .collect()
}

pub(super) fn curve(value: &Value, path: &str) -> Result<Outline, String> {
    let mut points: Vec<[f64; 2]> = Vec::new();
    for p in pairs::<2>(value, path, 3)? {
        if points.last() != Some(&p) {
            points.push(p);
        }
    }
    if points.len() > 3 && points.first() == points.last() {
        points.pop();
    }
    let o = Outline(points);
    if o.0.len() < 3 || o.area().abs() < 1e-8 {
        return Err(format!("{path}: the curve encloses no area"));
    }
    Ok(o.ccw())
}

pub(super) fn curve_param(params: &Params, key: &str) -> Result<Option<Outline>, String> {
    params
        .get(key)
        .map(|v| curve(v, &format!("params.{key}")))
        .transpose()
}

pub(super) fn centre(o: &Outline) -> DVec2 {
    let [lo, hi] = o.bounds().unwrap_or([[0.0; 2]; 2]);
    (DVec2::from(lo) + DVec2::from(hi)) * 0.5
}

pub(super) fn half(o: &Outline) -> DVec2 {
    let [lo, hi] = o.bounds().unwrap_or([[0.0; 2]; 2]);
    (DVec2::from(hi) - DVec2::from(lo)) * 0.5
}

pub(super) fn scaled(o: &Outline, about: DVec2, k: f64) -> Outline {
    o.map(|p| (about + (DVec2::from(p) - about) * k).to_array())
}

pub(super) fn slab(o: &Outline, front: f64, back: f64, material: &str) -> Mesh {
    let mut m = mesh::extrude(o, -front, -back, &Profile::NONE, UvMode::Local);
    m.set_material(material);
    m
}

pub(super) fn placed(mut m: Mesh, material: &str, at: DVec3, rotation: DQuat) -> Mesh {
    m.set_material(material);
    mesh::transform(&mut m, DMat4::from_rotation_translation(rotation, at));
    m
}

pub(super) fn piece(name: &str, part: &str, mesh: Mesh) -> Piece {
    Piece {
        name: format!("{name}.{part}"),
        mesh,
    }
}

#[derive(Default)]
pub(super) struct Tris {
    pub(super) pos: Vec<DVec3>,
    pub(super) uv: Vec<DVec2>,
    pub(super) tris: Vec<[u32; 3]>,
}

impl Tris {
    pub(super) fn vert(&mut self, p: DVec3, uv: DVec2) -> u32 {
        self.pos.push(p);
        self.uv.push(uv);
        (self.pos.len() - 1) as u32
    }

    pub(super) fn tri(&mut self, a: u32, b: u32, c: u32) {
        if a != b && b != c && a != c {
            self.tris.push([a, b, c]);
        }
    }

    pub(super) fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.tri(a, b, c);
        self.tri(a, c, d);
    }

    pub(super) fn normal(&self, t: &[u32; 3]) -> DVec3 {
        let [a, b, c] = t.map(|i| self.pos[i as usize]);
        (b - a).cross(c - a)
    }

    pub(super) fn face(&mut self, from: usize, outward: impl Fn(DVec3, DVec3) -> bool) {
        for i in from..self.tris.len() {
            let t = self.tris[i];
            let mid = t.iter().map(|&k| self.pos[k as usize]).sum::<DVec3>() / 3.0;
            if !outward(self.normal(&t), mid) {
                self.tris[i].swap(1, 2);
            }
        }
    }

    pub(super) fn outward(&mut self) {
        let volume: f64 = self
            .tris
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| self.pos[i as usize]);
                a.dot(b.cross(c))
            })
            .sum();
        if volume < 0.0 {
            for t in &mut self.tris {
                t.swap(1, 2);
            }
        }
    }

    pub(super) fn mesh(self, material: &str, degrees: f64) -> Mesh {
        let count = (self.tris.len() * 3) as u32;
        let mut m = Mesh {
            positions: self.pos.iter().map(|p| p.as_vec3().to_array()).collect(),
            normals: vec![[0.0; 3]; self.pos.len()],
            uvs: Some(self.uv.iter().map(|u| u.as_vec2().to_array()).collect()),
            uvs1: None,
            indices: self.tris.into_iter().flatten().collect(),
            slots: vec![Slot {
                material: material.to_string(),
                start: 0,
                count,
            }],
        };
        mesh::smooth_by_angle(&mut m, degrees);
        m
    }
}
