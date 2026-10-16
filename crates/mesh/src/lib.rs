mod build;
pub mod math;
mod morph;
mod nets;
mod normals;
pub mod outline;
mod prim;
mod simplify;
mod slab;
mod tangents;

use glam::{DMat4, DVec3, Vec3};
pub use morph::Morph;
pub use nets::{Field, Lattice, surface_nets};
pub use normals::{LARGER_RATIO, PLANAR_DEGREES, area, bounds, transform, weld};
use outline::{Outline, Shape};
pub use prim::{
    bead, capsule, cylinder, disc, lathe, rounded_box, sheet, sphere, surface, sweep, torus,
};
pub use simplify::{NORMAL_WEIGHT, Simplified, UV_WEIGHT, simplify};
use std::cmp::Ordering;
pub use tangents::{Tangents, tangents};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Option<Vec<[f32; 2]>>,
    pub uvs1: Option<Vec<[f32; 2]>>,
    pub indices: Vec<u32>,
    pub slots: Vec<Slot>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub material: String,
    pub start: u32,
    pub count: u32,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn point(&self, i: u32) -> DVec3 {
        Vec3::from(self.positions[i as usize]).as_dvec3()
    }

    pub fn face_normal(&self, t: &[u32]) -> DVec3 {
        triangle_normal(&[self.point(t[0]), self.point(t[1]), self.point(t[2])])
    }

    pub fn world_triangles(&self, matrix: DMat4) -> Vec<[DVec3; 3]> {
        self.indices
            .chunks_exact(3)
            .map(|t| [0, 1, 2].map(|k| matrix.transform_point3(self.point(t[k]))))
            .collect()
    }

    pub fn set_material(&mut self, material: &str) {
        self.slots = vec![Slot {
            material: material.to_string(),
            start: 0,
            count: self.indices.len() as u32,
        }];
    }

    pub fn append(&mut self, other: &Mesh) {
        if other.indices.is_empty() {
            return;
        }
        let base = self.positions.len() as u32;
        let start = self.indices.len() as u32;
        let count = self.positions.len();
        self.uvs = joined(self.uvs.take(), &other.uvs, count, other.positions.len());
        self.uvs1 = joined(self.uvs1.take(), &other.uvs1, count, other.positions.len());
        self.positions.extend(&other.positions);
        self.normals.extend(&other.normals);
        self.indices.extend(other.indices.iter().map(|i| i + base));
        for s in &other.slots {
            let shifted = Slot {
                material: s.material.clone(),
                start: s.start + start,
                count: s.count,
            };
            match self.slots.last_mut() {
                Some(last)
                    if last.material == shifted.material
                        && last.start + last.count == shifted.start =>
                {
                    last.count += shifted.count;
                }
                _ => self.slots.push(shifted),
            }
        }
    }
}

pub fn triangle_normal(t: &[DVec3; 3]) -> DVec3 {
    (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero()
}

fn joined(
    mine: Option<Vec<[f32; 2]>>,
    theirs: &Option<Vec<[f32; 2]>>,
    count: usize,
    added: usize,
) -> Option<Vec<[f32; 2]>> {
    match (mine, theirs) {
        (None, None) => None,
        (mine, theirs) => {
            let mut u = mine.unwrap_or_else(|| vec![[0.0, 0.0]; count]);
            match theirs {
                Some(t) => u.extend(t),
                None => u.extend(std::iter::repeat_n([0.0, 0.0], added)),
            }
            Some(u)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileKind {
    Round,
    Chamfer,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    pub kind: ProfileKind,
    pub width: f64,
    pub segments: u32,
}

impl Profile {
    pub const NONE: Profile = Profile {
        kind: ProfileKind::None,
        width: 0.0,
        segments: 0,
    };

    pub fn round(width: f64, segments: u32) -> Profile {
        Profile {
            kind: ProfileKind::Round,
            width,
            segments,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UvMode {
    None,
    Local,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bend {
    Wrap(f64),
    Lift(f64),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sheet {
    pub size: [f64; 2],
    pub cells: [u32; 2],
    pub bend: Option<Bend>,
    pub curl: Option<[f64; 2]>,
    pub cup: Option<[f64; 2]>,
    pub jag: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pocket {
    pub outline: Outline,
    pub depth: f64,
    pub rim: Profile,
    pub floor: Profile,
}

pub fn extrude(outline: &Outline, front: f64, back: f64, profile: &Profile, uv: UvMode) -> Mesh {
    slab::slab(&Shape::new(outline.clone()), front, back, profile, &[], uv)
}

pub fn extrude_shape(shape: &Shape, front: f64, back: f64, profile: &Profile, uv: UvMode) -> Mesh {
    slab::slab(shape, front, back, profile, &[], uv)
}

pub fn pocket(
    slab: &Outline,
    front: f64,
    back: f64,
    profile: &Profile,
    pockets: &[Pocket],
    uv: UvMode,
) -> Mesh {
    slab::slab(&Shape::new(slab.clone()), front, back, profile, pockets, uv)
}

pub fn pocket_shape(
    shape: &Shape,
    front: f64,
    back: f64,
    profile: &Profile,
    pockets: &[Pocket],
    uv: UvMode,
) -> Mesh {
    slab::slab(shape, front, back, profile, pockets, uv)
}

pub fn grid(size: [f64; 2], cells: [u32; 2], bend: Option<f64>) -> Mesh {
    sheet(&Sheet {
        size,
        cells,
        bend: bend.map(Bend::Wrap),
        ..Sheet::default()
    })
}

pub fn smooth_by_angle(m: &mut Mesh, degrees: f64) {
    normals::split(m, degrees);
}

pub fn smooth_by_angle_flat_faces(m: &mut Mesh, degrees: f64) {
    normals::split_with(m, degrees, true);
}

pub fn smooth(m: &mut Mesh) {
    normals::split(m, 180.0);
}

pub fn flat(m: &mut Mesh) {
    normals::split(m, 0.01);
}

pub fn smooth_morphed(m: &mut Mesh, morph: &mut Morph, degrees: f64, flat_faces: bool) {
    normals::split_morphed(m, degrees, flat_faces, morph);
}

fn digits_end(text: &[u8], from: usize) -> usize {
    from + text[from..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count()
}

fn number_cmp(a: &[u8], b: &[u8], tiebreak: &mut Ordering) -> Ordering {
    let zeros_a = a.iter().take_while(|&&c| c == b'0').count();
    let zeros_b = b.iter().take_while(|&&c| c == b'0').count();
    let (a, b) = (&a[zeros_a..], &b[zeros_b..]);
    let order = a.len().cmp(&b.len()).then_with(|| a.cmp(b));
    if order == Ordering::Equal && *tiebreak == Ordering::Equal {
        *tiebreak = zeros_a.cmp(&zeros_b);
    }
    order
}

pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    let mut tiebreak = Ordering::Equal;
    loop {
        if i < x.len() && j < y.len() && x[i].is_ascii_digit() && y[j].is_ascii_digit() {
            let (end_i, end_j) = (digits_end(x, i), digits_end(y, j));
            let order = number_cmp(&x[i..end_i], &y[j..end_j], &mut tiebreak);
            if order != Ordering::Equal {
                return order;
            }
            i = end_i;
            j = end_j;
        }
        if i >= x.len() || j >= y.len() {
            break;
        }
        let (c, d) = (x[i].to_ascii_lowercase(), y[j].to_ascii_lowercase());
        if c != d {
            return match (c, d) {
                (b'.', _) => Ordering::Less,
                (_, b'.') => Ordering::Greater,
                _ => c.cmp(&d),
            };
        }
        i += 1;
        j += 1;
    }
    if tiebreak != Ordering::Equal {
        return tiebreak;
    }
    x.cmp(y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(names: &[&str]) -> Vec<String> {
        let mut out: Vec<String> = names.iter().map(|n| n.to_string()).collect();
        out.sort_by(|a, b| natural_cmp(a, b));
        out
    }

    #[test]
    fn natural_order_counts_numbers_and_ignores_case() {
        assert_eq!(
            sorted(&[
                "wall_bottom",
                "slat10",
                "mv_knob__cap",
                "Wall",
                "slat2",
                "mv_knob"
            ]),
            [
                "mv_knob",
                "mv_knob__cap",
                "slat2",
                "slat10",
                "Wall",
                "wall_bottom"
            ]
        );
        assert_eq!(sorted(&["a01", "a1", "a001"]), ["a1", "a01", "a001"]);
        assert_eq!(sorted(&["foo 1.bar", "foo.bar"]), ["foo.bar", "foo 1.bar"]);
        assert_eq!(natural_cmp("Deck", "deck"), Ordering::Less);
        assert_eq!(natural_cmp("deck", "deck"), Ordering::Equal);
    }
}
