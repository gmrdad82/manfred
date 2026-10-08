use crate::normals::split;
use crate::{Mesh, Slot};
use glam::{DVec2, DVec3};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Build {
    pub pos: Vec<DVec3>,
    pub uv: Vec<DVec2>,
    pub tris: Vec<[u32; 3]>,
}

impl Build {
    pub fn vert(&mut self, p: DVec3) -> u32 {
        self.pos.push(p);
        (self.pos.len() - 1) as u32
    }

    pub fn vert_uv(&mut self, p: DVec3, uv: DVec2) -> u32 {
        self.uv.push(uv);
        self.vert(p)
    }

    pub fn tri(&mut self, a: u32, b: u32, c: u32) {
        if !degenerate(&[a, b, c]) {
            self.tris.push([a, b, c]);
        }
    }

    pub fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.tri(a, b, c);
        self.tri(a, c, d);
    }

    pub fn strip(&mut self, a: &[u32], b: &[u32], closed: bool) {
        for [ai, aj, bj, bi] in band(a, b, closed) {
            self.quad(ai, bi, bj, aj);
        }
    }

    pub fn band(&mut self, a: &[u32], c: &[u32], closed: bool) {
        for [ai, aj, cj, ci] in band(a, c, closed) {
            self.quad(ai, aj, cj, ci);
        }
    }

    pub fn fan(&mut self, ring: &[u32], flip: bool) {
        for i in 1..ring.len().saturating_sub(1) {
            if flip {
                self.tri(ring[0], ring[i + 1], ring[i]);
            } else {
                self.tri(ring[0], ring[i], ring[i + 1]);
            }
        }
    }

    pub fn volume(&self) -> f64 {
        signed_volume(&self.pos, &self.tris)
    }

    pub fn orient_outward(&mut self) {
        if self.volume() < 0.0 {
            for t in &mut self.tris {
                t.swap(1, 2);
            }
        }
    }

    pub fn weld(&mut self, dist: f64) {
        let map = cluster(&self.pos, dist);
        for t in &mut self.tris {
            for i in t.iter_mut() {
                *i = map[*i as usize];
            }
        }
        self.tris.retain(|t| !degenerate(t));
    }

    pub fn mesh(self) -> Mesh {
        let mut keep = vec![u32::MAX; self.pos.len()];
        let mut positions = Vec::new();
        let mut uvs = Vec::new();
        let has_uv = self.uv.len() == self.pos.len() && !self.pos.is_empty();
        let mut indices = Vec::with_capacity(self.tris.len() * 3);
        for t in &self.tris {
            for &i in t {
                let i = i as usize;
                if keep[i] == u32::MAX {
                    keep[i] = positions.len() as u32;
                    positions.push(self.pos[i].as_vec3().to_array());
                    if has_uv {
                        uvs.push(self.uv[i].as_vec2().to_array());
                    }
                }
                indices.push(keep[i]);
            }
        }
        let count = indices.len() as u32;
        Mesh {
            normals: vec![[0.0, 0.0, 0.0]; positions.len()],
            positions,
            uvs: has_uv.then_some(uvs),
            uvs1: None,
            indices,
            slots: vec![Slot {
                material: String::new(),
                start: 0,
                count,
            }],
        }
    }

    pub fn finish(self, degrees: f64) -> Mesh {
        let mut m = self.mesh();
        split(&mut m, degrees);
        m
    }
}

pub(crate) fn degenerate(t: &[u32; 3]) -> bool {
    t[0] == t[1] || t[1] == t[2] || t[0] == t[2]
}

pub(crate) fn band<'a>(
    a: &'a [u32],
    c: &'a [u32],
    closed: bool,
) -> impl Iterator<Item = [u32; 4]> + 'a {
    let n = a.len().min(c.len());
    let edges = if closed { n } else { n.saturating_sub(1) };
    (0..edges).map(move |i| {
        let j = (i + 1) % n;
        [a[i], a[j], c[j], c[i]]
    })
}

pub(crate) fn signed_volume(pos: &[DVec3], tris: &[[u32; 3]]) -> f64 {
    let Some(&first) = pos.first() else {
        return 0.0;
    };
    let (lo, hi) = pos
        .iter()
        .fold((first, first), |(lo, hi), &p| (lo.min(p), hi.max(p)));
    let c = (lo + hi) * 0.5;
    tris.iter()
        .map(|t| {
            let a = pos[t[0] as usize] - c;
            let b = pos[t[1] as usize] - c;
            let d = pos[t[2] as usize] - c;
            a.dot(b.cross(d))
        })
        .sum::<f64>()
        / 6.0
}

pub(crate) fn cluster(points: &[DVec3], dist: f64) -> Vec<u32> {
    let mut map = Vec::with_capacity(points.len());
    if dist <= 0.0 {
        let mut seen = BTreeMap::<[u64; 3], u32>::new();
        for (i, p) in points.iter().enumerate() {
            let key = (*p + DVec3::ZERO).to_array().map(f64::to_bits);
            map.push(*seen.entry(key).or_insert(i as u32));
        }
        return map;
    }
    let mut grid = BTreeMap::<[i64; 3], Vec<u32>>::new();
    for (i, p) in points.iter().enumerate() {
        let cell = (*p / dist).floor().as_i64vec3().to_array();
        let mut found: Option<u32> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = grid.get(&[cell[0] + dx, cell[1] + dy, cell[2] + dz]) {
                        for &j in list {
                            if points[j as usize].distance(*p) <= dist
                                && found.is_none_or(|f| j < f)
                            {
                                found = Some(j);
                            }
                        }
                    }
                }
            }
        }
        match found {
            Some(j) => map.push(map[j as usize]),
            None => {
                map.push(i as u32);
                grid.entry(cell).or_default().push(i as u32);
            }
        }
    }
    map
}
