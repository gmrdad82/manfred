use crate::Mesh;
use crate::math::Real;
use std::collections::HashMap;

const NONE: usize = usize::MAX;
const THRESHOLD: f32 = -1.0;
const DEFAULT: [f32; 4] = [1.0, 0.0, 0.0, -1.0];

#[derive(Clone, Debug, PartialEq)]
pub struct Tangents {
    pub mesh: Mesh,
    pub tangents: Vec<[f32; 4]>,
    pub sources: Vec<u32>,
}

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn not_zero(x: f32) -> bool {
    x.abs() > f32::MIN_POSITIVE
}

fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}

fn normalized(a: V3) -> V3 {
    if not_zero(a[0]) || not_zero(a[1]) || not_zero(a[2]) {
        scale(a, length(a).recip())
    } else {
        a
    }
}

fn project(v: V3, n: V3) -> V3 {
    normalized(sub(v, scale(n, dot(n, v))))
}

#[derive(Clone, Copy, Default)]
struct Raw {
    s: V3,
    t: V3,
    mag_s: f32,
    mag_t: f32,
}

#[derive(Clone, Copy)]
struct Info {
    face: usize,
    neighbors: [usize; 3],
    group: [usize; 3],
    raw: Raw,
    any: bool,
    preserving: bool,
}

struct Group {
    faces: Vec<usize>,
    rep: u32,
    preserving: bool,
}

pub fn tangents(mesh: &Mesh) -> Option<Tangents> {
    let n = mesh.positions.len();
    let uvs = mesh.uvs.as_ref().filter(|u| u.len() == n)?;
    if mesh.normals.len() != n || mesh.indices.iter().any(|&i| i as usize >= n) {
        return None;
    }
    let faces = mesh.indices.len() / 3;
    let corner = |f: usize, k: usize| mesh.indices[f * 3 + k] as usize;
    let mut welded_ids: HashMap<[u32; 8], u32> = HashMap::new();
    let welded: Vec<u32> = (0..faces * 3)
        .map(|c| {
            let v = mesh.indices[c] as usize;
            let p = mesh.positions[v];
            let q = mesh.normals[v];
            let t = uvs[v];
            let key = [p[0], p[1], p[2], q[0], q[1], q[2], t[0], t[1]].map(f32::to_bits);
            let next = welded_ids.len() as u32;
            *welded_ids.entry(key).or_insert(next)
        })
        .collect();
    let mut good = Vec::new();
    let mut degenerate = Vec::new();
    for f in 0..faces {
        let p = [0, 1, 2].map(|k| mesh.positions[corner(f, k)]);
        if p[0] == p[1] || p[1] == p[2] || p[2] == p[0] {
            degenerate.push(f);
            continue;
        }
        let tx = [0, 1, 2].map(|k| uvs[corner(f, k)]);
        let d_tx = [1, 2].map(|t| [0, 1].map(|i| tx[t][i] - tx[0][i]));
        let d_v = [sub(p[1], p[0]), sub(p[2], p[0])];
        let signed = d_tx[0][0] * d_tx[1][1] - d_tx[0][1] * d_tx[1][0];
        let area = signed.abs();
        let s = sub(scale(d_v[0], d_tx[1][1]), scale(d_v[1], d_tx[0][1]));
        let t = add(scale(d_v[0], -d_tx[1][0]), scale(d_v[1], d_tx[0][0]));
        let mut info = Info {
            face: f,
            neighbors: [NONE; 3],
            group: [NONE; 3],
            raw: Raw::default(),
            any: true,
            preserving: signed > 0.0,
        };
        if not_zero(area) {
            let sign = if info.preserving { 1.0 } else { -1.0 };
            info.raw = Raw {
                s: scale(normalized(s), sign),
                t: scale(normalized(t), sign),
                mag_s: length(s) / area,
                mag_t: length(t) / area,
            };
            if not_zero(info.raw.mag_s) && not_zero(info.raw.mag_t) {
                info.any = false;
            }
        }
        good.push(info);
    }
    let verts = |g: usize| -> [u32; 3] {
        let f = good[g].face;
        [welded[f * 3], welded[f * 3 + 1], welded[f * 3 + 2]]
    };
    let good_verts: Vec<[u32; 3]> = (0..good.len()).map(verts).collect();
    neighbors(&mut good, &good_verts);
    let groups = groups(&mut good, &good_verts);
    let normal_of = |f: usize, k: usize| mesh.normals[corner(f, k)];
    let mut spaces: Vec<Option<(Raw, bool)>> = vec![None; faces * 3];
    for (g, group) in groups.iter().enumerate() {
        let mut subgroups: Vec<(Vec<usize>, Raw)> = Vec::new();
        for &f in &group.faces {
            let a = good[f];
            let Some(index) = a.group.iter().position(|&x| x == g) else {
                continue;
            };
            let n = normal_of(a.face, index);
            let s_a = project(a.raw.s, n);
            let t_a = project(a.raw.t, n);
            let mut members: Vec<usize> = group
                .faces
                .iter()
                .copied()
                .filter(|&t| {
                    let b = good[t];
                    let s_b = project(b.raw.s, n);
                    let t_b = project(b.raw.t, n);
                    let meets = dot(s_a, s_b) > THRESHOLD && dot(t_a, t_b) > THRESHOLD;
                    a.any || b.any || a.face == b.face || meets
                })
                .collect();
            members.sort_unstable();
            let raw = match subgroups.iter().find(|(m, _)| *m == members) {
                Some((_, raw)) => *raw,
                None => {
                    let raw = evaluate(&members, &good, &good_verts, mesh, group.rep);
                    subgroups.push((members, raw));
                    raw
                }
            };
            spaces[a.face * 3 + index] = Some((raw, group.preserving));
        }
    }
    if !degenerate.is_empty() {
        let mut first: HashMap<u32, usize> = HashMap::new();
        for (g, info) in good.iter().enumerate() {
            for (j, &v) in good_verts[g].iter().enumerate() {
                first.entry(v).or_insert(info.face * 3 + j);
            }
        }
        for &f in &degenerate {
            for k in 0..3 {
                if let Some(&src) = first.get(&welded[f * 3 + k]) {
                    spaces[f * 3 + k] = spaces[src];
                }
            }
        }
    }
    let corners: Vec<[f32; 4]> = spaces
        .iter()
        .map(|space| match space {
            Some((raw, preserving)) => [
                raw.s[0],
                raw.s[1],
                raw.s[2],
                if *preserving { 1.0 } else { -1.0 },
            ],
            None => DEFAULT,
        })
        .collect();
    let mut split: HashMap<(u32, [u32; 4]), u32> = HashMap::new();
    let mut out = Mesh {
        uvs: Some(Vec::new()),
        uvs1: mesh
            .uvs1
            .as_ref()
            .filter(|u| u.len() == n)
            .map(|_| Vec::new()),
        slots: mesh.slots.clone(),
        ..Mesh::default()
    };
    let mut tangents = Vec::new();
    let mut sources = Vec::new();
    for (c, &v) in mesh.indices[..faces * 3].iter().enumerate() {
        let t = corners[c];
        let next = sources.len() as u32;
        let id = *split.entry((v, t.map(f32::to_bits))).or_insert_with(|| {
            let s = v as usize;
            sources.push(v);
            tangents.push(t);
            out.positions.push(mesh.positions[s]);
            out.normals.push(mesh.normals[s]);
            if let Some(list) = out.uvs.as_mut() {
                list.push(uvs[s]);
            }
            if let (Some(list), Some(from)) = (out.uvs1.as_mut(), mesh.uvs1.as_ref()) {
                list.push(from[s]);
            }
            next
        });
        out.indices.push(id);
    }
    Some(Tangents {
        mesh: out,
        tangents,
        sources,
    })
}

fn edge_of(verts: [u32; 3], i0: u32, i1: u32) -> Option<(usize, u32, u32)> {
    let key = (i0.min(i1), i0.max(i1));
    (0..3).find_map(|n| {
        let (a, b) = (verts[n], verts[(n + 1) % 3]);
        ((a.min(b), a.max(b)) == key).then_some((n, a, b))
    })
}

fn neighbors(good: &mut [Info], verts: &[[u32; 3]]) {
    let mut edges: Vec<(u32, u32, usize)> = Vec::with_capacity(verts.len() * 3);
    for (f, v) in verts.iter().enumerate() {
        for n in 0..3 {
            let (a, b) = (v[n], v[(n + 1) % 3]);
            edges.push((a.min(b), a.max(b), f));
        }
    }
    edges.sort_unstable();
    for (index, &(i0, i1, fa)) in edges.iter().enumerate() {
        let Some((n_a, a0, a1)) = edge_of(verts[fa], i0, i1) else {
            continue;
        };
        if good[fa].neighbors[n_a] != NONE {
            continue;
        }
        let found = edges[index + 1..]
            .iter()
            .take_while(|&&(j0, j1, _)| (j0, j1) == (i0, i1))
            .find_map(|&(_, _, fb)| {
                let (n_b, b1, b0) = edge_of(verts[fb], i0, i1)?;
                ((a0, a1) == (b0, b1) && good[fb].neighbors[n_b] == NONE).then_some((fb, n_b))
            });
        if let Some((fb, n_b)) = found {
            good[fa].neighbors[n_a] = fb;
            good[fb].neighbors[n_b] = fa;
        }
    }
}

fn groups(good: &mut [Info], verts: &[[u32; 3]]) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for f in 0..good.len() {
        for i in 0..3 {
            if good[f].any || good[f].group[i] != NONE {
                continue;
            }
            let id = out.len();
            let mut group = Group {
                faces: vec![f],
                rep: verts[f][i],
                preserving: good[f].preserving,
            };
            good[f].group[i] = id;
            let around = [good[f].neighbors[(i + 2) % 3], good[f].neighbors[i]];
            for neighbor in around {
                if neighbor != NONE {
                    assign(good, verts, neighbor, &mut group, id);
                }
            }
            out.push(group);
        }
    }
    out
}

fn assign(good: &mut [Info], verts: &[[u32; 3]], f: usize, group: &mut Group, id: usize) -> bool {
    let Some(i) = verts[f].iter().position(|&v| v == group.rep) else {
        return false;
    };
    let info = &mut good[f];
    if info.group[i] != NONE {
        return info.group[i] == id;
    }
    if info.any && info.group.iter().all(|&g| g == NONE) {
        info.preserving = group.preserving;
    }
    if info.preserving != group.preserving {
        return false;
    }
    group.faces.push(f);
    info.group[i] = id;
    let around = [info.neighbors[(i + 2) % 3], info.neighbors[i]];
    for neighbor in around {
        if neighbor != NONE {
            assign(good, verts, neighbor, group, id);
        }
    }
    true
}

fn evaluate(members: &[usize], good: &[Info], verts: &[[u32; 3]], mesh: &Mesh, rep: u32) -> Raw {
    let mut res = Raw::default();
    let mut angle_sum = 0.0f32;
    for &f in members {
        let info = &good[f];
        if info.any {
            continue;
        }
        let Some(i) = verts[f].iter().position(|&v| v == rep) else {
            continue;
        };
        let at = |k: usize| mesh.indices[info.face * 3 + k] as usize;
        let n = mesh.normals[at(i)];
        let p = [1, 0, 2].map(|j| mesh.positions[at((i + j) % 3)]);
        let v = [sub(p[0], p[1]), sub(p[2], p[1])].map(|v| project(v, n));
        let cos = dot(v[0], v[1]).clamp(-1.0, 1.0);
        let angle = cos.acos_of();
        let s = scale(project(info.raw.s, n), angle);
        let t = scale(project(info.raw.t, n), angle);
        res.s = add(res.s, s);
        res.t = add(res.t, t);
        res.mag_s += angle * info.raw.mag_s;
        res.mag_t += angle * info.raw.mag_t;
        angle_sum += angle;
    }
    res.s = normalized(res.s);
    res.t = normalized(res.t);
    if angle_sum > 0.0 {
        res.mag_s /= angle_sum;
        res.mag_t /= angle_sum;
    }
    res
}
