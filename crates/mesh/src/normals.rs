use crate::build::{cluster, degenerate};
use crate::math::{acos, cos};
use crate::{Mesh, Slot};
use glam::{DMat4, DVec3, Vec3};
use std::collections::BTreeMap;

pub const PLANAR_DEGREES: f64 = 0.5;
pub const LARGER_RATIO: f64 = 2.0;

type Edges = BTreeMap<(u32, u32), Vec<(usize, usize, usize)>>;

struct Regions {
    flat: Vec<Option<usize>>,
    normal: Vec<DVec3>,
}

fn regions(pos: &[DVec3], tris: &[[u32; 3]], face: &[DVec3], edges: &Edges, limit: f64) -> Regions {
    let area: Vec<f64> = tris
        .iter()
        .map(|t| {
            let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
            (b - a).cross(c - a).length() * 0.5
        })
        .collect();
    let planar = cos(PLANAR_DEGREES.to_radians());
    let mut adjacent = vec![Vec::new(); face.len()];
    for list in edges.values() {
        if list.len() == 2 {
            adjacent[list[0].0].push(list[1].0);
            adjacent[list[1].0].push(list[0].0);
        }
    }
    let mut of = vec![usize::MAX; face.len()];
    let mut normal = Vec::new();
    let mut size = Vec::new();
    for seed in 0..face.len() {
        if of[seed] != usize::MAX || face[seed] == DVec3::ZERO {
            continue;
        }
        let id = normal.len();
        let mut sum = DVec3::ZERO;
        let mut total = 0.0;
        let mut stack = vec![seed];
        of[seed] = id;
        while let Some(t) = stack.pop() {
            sum += face[t] * area[t];
            total += area[t];
            for &next in &adjacent[t] {
                if of[next] == usize::MAX
                    && face[next] != DVec3::ZERO
                    && face[seed].dot(face[next]) >= planar
                {
                    of[next] = id;
                    stack.push(next);
                }
            }
        }
        let n = sum.normalize_or_zero();
        normal.push(if n == DVec3::ZERO { face[seed] } else { n });
        size.push(total);
    }
    let mut largest = vec![0.0f64; normal.len()];
    for list in edges.values() {
        if list.len() != 2 {
            continue;
        }
        let (a, b) = (list[0].0, list[1].0);
        let (ra, rb) = (of[a], of[b]);
        if ra == usize::MAX || rb == usize::MAX || ra == rb || face[a].dot(face[b]) < limit {
            continue;
        }
        largest[ra] = largest[ra].max(size[rb]);
        largest[rb] = largest[rb].max(size[ra]);
    }
    let flat = of
        .iter()
        .map(|&r| (r != usize::MAX && size[r] >= LARGER_RATIO * largest[r]).then_some(r))
        .collect();
    Regions { flat, normal }
}

pub(crate) fn split(m: &mut Mesh, degrees: f64) {
    split_with(m, degrees, false);
}

pub(crate) fn split_with(m: &mut Mesh, degrees: f64, flat_faces: bool) {
    let pos: Vec<DVec3> = m
        .positions
        .iter()
        .map(|&p| Vec3::from(p).as_dvec3())
        .collect();
    let pid = cluster(&pos, 0.0);
    let tris: Vec<[u32; 3]> = m
        .indices
        .chunks_exact(3)
        .map(|t| [t[0], t[1], t[2]])
        .collect();
    let face: Vec<DVec3> = tris
        .iter()
        .map(|t| {
            let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
            (b - a).cross(c - a).normalize_or_zero()
        })
        .collect();
    let limit = cos(degrees.clamp(0.0, 180.0).to_radians()) - 1e-12;
    let mut parent: Vec<usize> = (0..tris.len() * 3).collect();
    let mut edges = Edges::new();
    for (ti, t) in tris.iter().enumerate() {
        for e in 0..3 {
            let (a, b) = (pid[t[e] as usize], pid[t[(e + 1) % 3] as usize]);
            if a == b {
                continue;
            }
            let (ca, cb) = if a < b {
                (e, (e + 1) % 3)
            } else {
                ((e + 1) % 3, e)
            };
            edges
                .entry((a.min(b), a.max(b)))
                .or_default()
                .push((ti, ti * 3 + ca, ti * 3 + cb));
        }
    }
    let regions = flat_faces.then(|| regions(&pos, &tris, &face, &edges, limit));
    let flat_of = |t: usize| regions.as_ref().and_then(|r| r.flat[t]);
    for list in edges.values() {
        if list.len() != 2 {
            continue;
        }
        let (t1, a1, b1) = list[0];
        let (t2, a2, b2) = list[1];
        let (n1, n2) = (face[t1], face[t2]);
        let smooth = match (flat_of(t1), flat_of(t2)) {
            (None, None) => n1 == DVec3::ZERO || n2 == DVec3::ZERO || n1.dot(n2) >= limit,
            (Some(r1), Some(r2)) => r1 == r2,
            _ => false,
        };
        if smooth {
            union(&mut parent, a1, a2);
            union(&mut parent, b1, b2);
        }
    }
    let mut sums = vec![DVec3::ZERO; tris.len() * 3];
    for (ti, t) in tris.iter().enumerate() {
        for c in 0..3 {
            let root = find(&mut parent, ti * 3 + c);
            if let Some(n) = regions
                .as_ref()
                .and_then(|r| r.flat[ti].map(|g| r.normal[g]))
            {
                sums[root] = n;
                continue;
            }
            let p = pos[t[c] as usize];
            let u = (pos[t[(c + 1) % 3] as usize] - p).normalize_or_zero();
            let v = (pos[t[(c + 2) % 3] as usize] - p).normalize_or_zero();
            sums[root] += face[ti] * acos(u.dot(v).clamp(-1.0, 1.0));
        }
    }
    let mut keys = BTreeMap::<(usize, [u32; 2], [u32; 2]), u32>::new();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut uvs1 = Vec::new();
    let mut indices = Vec::with_capacity(m.indices.len());
    for (ti, t) in tris.iter().enumerate() {
        for (c, &v) in t.iter().enumerate() {
            let v = v as usize;
            let root = find(&mut parent, ti * 3 + c);
            let uv = m.uvs.as_ref().map_or([0.0, 0.0], |u| u[v]);
            let uv1 = m.uvs1.as_ref().map_or([0.0, 0.0], |u| u[v]);
            let key = (root, uv.map(f32::to_bits), uv1.map(f32::to_bits));
            let id = *keys.entry(key).or_insert_with(|| {
                positions.push(m.positions[v]);
                let n = sums[root].normalize_or_zero();
                let n = if n == DVec3::ZERO { face[ti] } else { n };
                normals.push(n.as_vec3().to_array());
                uvs.push(uv);
                uvs1.push(uv1);
                (positions.len() - 1) as u32
            });
            indices.push(id);
        }
    }
    m.positions = positions;
    m.normals = normals;
    if m.uvs.is_some() {
        m.uvs = Some(uvs);
    }
    if m.uvs1.is_some() {
        m.uvs1 = Some(uvs1);
    }
    m.indices = indices;
}

pub(crate) fn find(parent: &mut [usize], i: usize) -> usize {
    let mut r = i;
    while parent[r] != r {
        r = parent[r];
    }
    let mut j = i;
    while parent[j] != r {
        let next = parent[j];
        parent[j] = r;
        j = next;
    }
    r
}

pub(crate) fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        let (lo, hi) = (ra.min(rb), ra.max(rb));
        parent[hi] = lo;
    }
}

pub fn weld(m: &mut Mesh, dist: f64) {
    let pos: Vec<DVec3> = m
        .positions
        .iter()
        .map(|&p| Vec3::from(p).as_dvec3())
        .collect();
    let map = cluster(&pos, dist);
    let mut kept: Vec<u32> = Vec::with_capacity(m.indices.len());
    let mut slots = Vec::with_capacity(m.slots.len());
    for s in &m.slots {
        let start = kept.len() as u32;
        let end = (s.start + s.count).min(m.indices.len() as u32);
        for t in m.indices[s.start as usize..end as usize].chunks_exact(3) {
            let t = [map[t[0] as usize], map[t[1] as usize], map[t[2] as usize]];
            if !degenerate(&t) {
                kept.extend(t);
            }
        }
        slots.push(Slot {
            material: s.material.clone(),
            start,
            count: kept.len() as u32 - start,
        });
    }
    m.indices = kept;
    m.slots = slots;
    compact(m);
}

pub(crate) fn compact(m: &mut Mesh) {
    let mut keep = vec![u32::MAX; m.positions.len()];
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut uvs1 = Vec::new();
    for i in m.indices.iter_mut() {
        let v = *i as usize;
        if keep[v] == u32::MAX {
            keep[v] = positions.len() as u32;
            positions.push(m.positions[v]);
            normals.push(m.normals.get(v).copied().unwrap_or([0.0, 0.0, 0.0]));
            if let Some(u) = &m.uvs {
                uvs.push(u[v]);
            }
            if let Some(u) = &m.uvs1 {
                uvs1.push(u[v]);
            }
        }
        *i = keep[v];
    }
    m.positions = positions;
    m.normals = normals;
    if m.uvs.is_some() {
        m.uvs = Some(uvs);
    }
    if m.uvs1.is_some() {
        m.uvs1 = Some(uvs1);
    }
}

pub fn transform(m: &mut Mesh, matrix: DMat4) {
    let normal = matrix.inverse().transpose();
    for p in &mut m.positions {
        *p = matrix
            .transform_point3(Vec3::from(*p).as_dvec3())
            .as_vec3()
            .to_array();
    }
    for n in &mut m.normals {
        *n = normal
            .transform_vector3(Vec3::from(*n).as_dvec3())
            .normalize_or_zero()
            .as_vec3()
            .to_array();
    }
    if matrix.determinant() < 0.0 {
        for t in m.indices.chunks_exact_mut(3) {
            t.swap(1, 2);
        }
    }
}

pub fn bounds(m: &Mesh) -> Option<[[f64; 3]; 2]> {
    let first = Vec3::from(*m.positions.first()?).as_dvec3();
    let (lo, hi) = m.positions.iter().fold((first, first), |(lo, hi), &p| {
        let p = Vec3::from(p).as_dvec3();
        (lo.min(p), hi.max(p))
    });
    Some([lo.to_array(), hi.to_array()])
}

pub fn area(m: &Mesh) -> f64 {
    m.indices
        .chunks_exact(3)
        .map(|t| {
            let p = |i: u32| Vec3::from(m.positions[i as usize]).as_dvec3();
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            (b - a).cross(c - a).length() * 0.5
        })
        .sum()
}
