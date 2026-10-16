use crate::{Mesh, Slot};
use std::collections::HashMap;
use std::ops::Range;

pub const NORMAL_WEIGHT: f32 = 0.5;
pub const UV_WEIGHT: f32 = 1.0;

const MANIFOLD: u8 = 0;
const BORDER: u8 = 1;
const SEAM: u8 = 2;
const COMPLEX: u8 = 3;
const LOCKED: u8 = 4;
const NONE: u32 = u32::MAX;
const NEXT: [usize; 4] = [1, 2, 0, 1];
const EDGE_WEIGHT_SEAM: f32 = 0.5;
const EDGE_WEIGHT_BORDER: f32 = 10.0;
const SORT_BITS: u32 = 12;
const SORT_BINS: usize = 2048 + 512;

const CAN_COLLAPSE: [[u8; 5]; 5] = [
    [1, 1, 1, 1, 1],
    [0, 1, 0, 0, 1],
    [0, 0, 1, 0, 1],
    [0, 0, 0, 1, 1],
    [0, 0, 0, 0, 0],
];

const HAS_OPPOSITE: [[u8; 5]; 5] = [
    [1, 1, 1, 1, 1],
    [1, 0, 1, 0, 0],
    [1, 1, 1, 0, 1],
    [1, 0, 0, 0, 0],
    [1, 0, 1, 0, 0],
];

#[derive(Clone, Debug, PartialEq)]
pub struct Simplified {
    pub mesh: Mesh,
    pub sources: Vec<u32>,
    pub error: f32,
}

pub fn simplify(mesh: &Mesh, target_triangles: usize) -> Simplified {
    let n = mesh.positions.len();
    let normals = (mesh.normals.len() == n).then_some(&mesh.normals);
    let uvs = mesh.uvs.as_ref().filter(|u| u.len() == n);
    let uvs1 = mesh.uvs1.as_ref().filter(|u| u.len() == n);
    let ranges: Vec<Range<usize>> = if mesh.slots.is_empty() {
        std::iter::once(0..mesh.indices.len()).collect()
    } else {
        mesh.slots
            .iter()
            .map(|s| {
                let start = (s.start as usize).min(mesh.indices.len());
                start..(start + s.count as usize).min(mesh.indices.len())
            })
            .collect()
    };
    let mut split: HashMap<(u32, u32), u32> = HashMap::new();
    let mut origin = Vec::new();
    let mut indices = Vec::new();
    let mut tris = Vec::new();
    for (slot, range) in ranges.iter().enumerate() {
        for t in mesh.indices[range.clone()].chunks_exact(3) {
            if t.iter().any(|&i| i as usize >= n) {
                continue;
            }
            for &v in t {
                let id = *split.entry((v, slot as u32)).or_insert_with(|| {
                    origin.push(v);
                    origin.len() as u32 - 1
                });
                indices.push(id);
            }
            tris.push(slot as u32);
        }
    }
    let positions: Vec<[f32; 3]> = origin.iter().map(|&v| mesh.positions[v as usize]).collect();
    let mut weights = Vec::new();
    if normals.is_some() {
        weights.extend([NORMAL_WEIGHT; 3]);
    }
    if uvs.is_some() {
        weights.extend([UV_WEIGHT; 2]);
    }
    if uvs1.is_some() {
        weights.extend([UV_WEIGHT; 2]);
    }
    let mut attributes = Vec::with_capacity(origin.len() * weights.len());
    for &v in &origin {
        let v = v as usize;
        if let Some(normals) = normals {
            attributes.extend(normals[v].map(|a| a * NORMAL_WEIGHT));
        }
        if let Some(uvs) = uvs {
            attributes.extend(uvs[v].map(|a| a * UV_WEIGHT));
        }
        if let Some(uvs1) = uvs1 {
            attributes.extend(uvs1[v].map(|a| a * UV_WEIGHT));
        }
    }
    let error = reduce(
        &positions,
        &attributes,
        weights.len(),
        &mut indices,
        &mut tris,
        target_triangles.saturating_mul(3),
    );
    let mut order: Vec<usize> = (0..tris.len()).collect();
    order.sort_by_key(|&t| tris[t]);
    let mut remap = vec![NONE; origin.len()];
    let mut out = Mesh {
        uvs: uvs.map(|_| Vec::new()),
        uvs1: uvs1.map(|_| Vec::new()),
        ..Mesh::default()
    };
    let mut sources = Vec::new();
    for t in order {
        let slot = tris[t] as usize;
        if let Some(own) = mesh.slots.get(slot) {
            match out.slots.last_mut() {
                Some(last)
                    if last.material == own.material
                        && last.start + last.count == out.indices.len() as u32 =>
                {
                    last.count += 3;
                }
                _ => out.slots.push(Slot {
                    material: own.material.clone(),
                    start: out.indices.len() as u32,
                    count: 3,
                }),
            }
        }
        for k in 0..3 {
            let v = indices[t * 3 + k] as usize;
            if remap[v] == NONE {
                remap[v] = out.positions.len() as u32;
                let source = origin[v];
                let s = source as usize;
                sources.push(source);
                out.positions.push(mesh.positions[s]);
                if let Some(normals) = normals {
                    out.normals.push(normals[s]);
                }
                if let (Some(uvs), Some(list)) = (uvs, out.uvs.as_mut()) {
                    list.push(uvs[s]);
                }
                if let (Some(uvs1), Some(list)) = (uvs1, out.uvs1.as_mut()) {
                    list.push(uvs1[s]);
                }
            }
            out.indices.push(remap[v]);
        }
    }
    Simplified {
        mesh: out,
        sources,
        error,
    }
}

struct Adjacency {
    offsets: Vec<u32>,
    next: Vec<u32>,
    prev: Vec<u32>,
}

impl Adjacency {
    fn new(vertices: usize, indices: usize) -> Adjacency {
        Adjacency {
            offsets: vec![0; vertices + 1],
            next: vec![0; indices],
            prev: vec![0; indices],
        }
    }

    fn update(&mut self, indices: &[u32], remap: Option<&[u32]>) {
        let map = |i: u32| remap.map_or(i, |r| r[i as usize]) as usize;
        self.offsets.fill(0);
        for &i in indices {
            self.offsets[map(i) + 1] += 1;
        }
        let mut offset = 0;
        for slot in self.offsets[1..].iter_mut() {
            let count = *slot;
            *slot = offset;
            offset += count;
        }
        for t in indices.chunks_exact(3) {
            let [a, b, c] = [map(t[0]), map(t[1]), map(t[2])];
            for (v, n, p) in [(a, b, c), (b, c, a), (c, a, b)] {
                let at = self.offsets[v + 1] as usize;
                self.next[at] = n as u32;
                self.prev[at] = p as u32;
                self.offsets[v + 1] += 1;
            }
        }
    }

    fn range(&self, v: u32) -> Range<usize> {
        self.offsets[v as usize] as usize..self.offsets[v as usize + 1] as usize
    }

    fn has_edge(&self, a: u32, b: u32) -> bool {
        self.next[self.range(a)].contains(&b)
    }
}

fn position_remap(positions: &[[f32; 3]]) -> (Vec<u32>, Vec<u32>) {
    let key = |p: [f32; 3]| p.map(|v| if v == 0.0 { 0 } else { v.to_bits() });
    let mut first: HashMap<[u32; 3], u32> = HashMap::with_capacity(positions.len());
    let remap: Vec<u32> = positions
        .iter()
        .enumerate()
        .map(|(i, p)| *first.entry(key(*p)).or_insert(i as u32))
        .collect();
    let mut wedge: Vec<u32> = (0..positions.len() as u32).collect();
    for i in 0..positions.len() {
        if remap[i] != i as u32 {
            let r = remap[i] as usize;
            wedge[i] = wedge[r];
            wedge[r] = i as u32;
        }
    }
    (remap, wedge)
}

fn classify(adjacency: &Adjacency, remap: &[u32], wedge: &[u32]) -> (Vec<u8>, Vec<u32>, Vec<u32>) {
    let n = remap.len();
    let mut out = vec![NONE; n];
    let mut inc = vec![NONE; n];
    for vertex in 0..n as u32 {
        let v = vertex as usize;
        for at in adjacency.range(vertex) {
            let target = adjacency.next[at];
            if target == vertex {
                inc[v] = vertex;
                out[v] = vertex;
            } else if !adjacency.has_edge(target, vertex) {
                let t = target as usize;
                inc[t] = if inc[t] == NONE { vertex } else { target };
                out[v] = if out[v] == NONE { target } else { vertex };
            }
        }
    }
    let mut kind = vec![MANIFOLD; n];
    for i in 0..n {
        let iu = i as u32;
        if remap[i] != iu {
            kind[i] = kind[remap[i] as usize];
            continue;
        }
        kind[i] = if wedge[i] == iu {
            let (oi, oo) = (inc[i], out[i]);
            if oi == NONE && oo == NONE {
                MANIFOLD
            } else if oi != NONE
                && oo != NONE
                && remap[oi as usize] == remap[oo as usize]
                && oi != iu
            {
                SEAM
            } else if oi != iu && oo != iu {
                BORDER
            } else {
                LOCKED
            }
        } else if wedge[wedge[i] as usize] == iu {
            let w = wedge[i] as usize;
            let wu = w as u32;
            let (oiv, oov, oiw, oow) = (inc[i], out[i], inc[w], out[w]);
            if oiv != NONE
                && oiv != iu
                && oov != NONE
                && oov != iu
                && oiw != NONE
                && oiw != wu
                && oow != NONE
                && oow != wu
                && remap[oiv as usize] == remap[oow as usize]
                && remap[oov as usize] == remap[oiw as usize]
                && remap[oiv as usize] != remap[oov as usize]
            {
                SEAM
            } else {
                LOCKED
            }
        } else {
            LOCKED
        };
    }
    for k in &mut kind {
        if *k == BORDER {
            *k = LOCKED;
        }
    }
    (kind, out, inc)
}

fn rescale(positions: &[[f32; 3]]) -> (Vec<[f32; 3]>, f32) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [-f32::MAX; 3];
    for p in positions {
        for j in 0..3 {
            lo[j] = if lo[j] > p[j] { p[j] } else { lo[j] };
            hi[j] = if hi[j] < p[j] { p[j] } else { hi[j] };
        }
    }
    let mut extent = 0.0f32;
    for j in 0..3 {
        let d = hi[j] - lo[j];
        extent = if d < extent { extent } else { d };
    }
    let scale = if extent == 0.0 { 0.0 } else { 1.0 / extent };
    let points = positions
        .iter()
        .map(|p| [0, 1, 2].map(|j| (p[j] - lo[j]) * scale))
        .collect();
    (points, extent)
}

#[derive(Clone, Copy, Default)]
struct Quadric {
    a00: f32,
    a11: f32,
    a22: f32,
    a10: f32,
    a20: f32,
    a21: f32,
    b0: f32,
    b1: f32,
    b2: f32,
    c: f32,
    w: f32,
}

#[derive(Clone, Copy, Default)]
struct Grad {
    gx: f32,
    gy: f32,
    gz: f32,
    gw: f32,
}

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: &mut V3) -> f32 {
    let length = dot(*v, *v).sqrt();
    if length > 0.0 {
        for x in v.iter_mut() {
            *x /= length;
        }
    }
    length
}

impl Quadric {
    fn add(&mut self, r: &Quadric) {
        self.a00 += r.a00;
        self.a11 += r.a11;
        self.a22 += r.a22;
        self.a10 += r.a10;
        self.a20 += r.a20;
        self.a21 += r.a21;
        self.b0 += r.b0;
        self.b1 += r.b1;
        self.b2 += r.b2;
        self.c += r.c;
        self.w += r.w;
    }

    fn eval(&self, v: V3) -> f32 {
        let mut rx = self.b0;
        let mut ry = self.b1;
        let mut rz = self.b2;
        rx += self.a10 * v[1];
        ry += self.a21 * v[2];
        rz += self.a20 * v[0];
        rx *= 2.0;
        ry *= 2.0;
        rz *= 2.0;
        rx += self.a00 * v[0];
        ry += self.a11 * v[1];
        rz += self.a22 * v[2];
        let mut r = self.c;
        r += rx * v[0];
        r += ry * v[1];
        r += rz * v[2];
        r
    }

    fn error(&self, v: V3) -> f32 {
        let r = self.eval(v);
        let s = if self.w == 0.0 { 0.0 } else { 1.0 / self.w };
        r.abs() * s
    }

    fn attribute_error(&self, g: &[Grad], v: V3, va: &[f32]) -> f32 {
        let mut r = self.eval(v);
        for (k, &a) in va.iter().enumerate() {
            let gk = &g[k];
            let gv = v[0] * gk.gx + v[1] * gk.gy + v[2] * gk.gz + gk.gw;
            r += a * (a * self.w - 2.0 * gv);
        }
        r.abs()
    }

    fn plane(a: f32, b: f32, c: f32, d: f32, w: f32) -> Quadric {
        let aw = a * w;
        let bw = b * w;
        let cw = c * w;
        let dw = d * w;
        Quadric {
            a00: a * aw,
            a11: b * bw,
            a22: c * cw,
            a10: a * bw,
            a20: a * cw,
            a21: b * cw,
            b0: a * dw,
            b1: b * dw,
            b2: c * dw,
            c: d * dw,
            w,
        }
    }

    fn point(p: V3, w: f32) -> Quadric {
        Quadric {
            a00: w,
            a11: w,
            a22: w,
            a10: 0.0,
            a20: 0.0,
            a21: 0.0,
            b0: -p[0] * w,
            b1: -p[1] * w,
            b2: -p[2] * w,
            c: (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]) * w,
            w,
        }
    }

    fn triangle(p0: V3, p1: V3, p2: V3, weight: f32) -> Quadric {
        let mut normal = cross(sub(p1, p0), sub(p2, p0));
        let area = normalize(&mut normal);
        let distance = dot(normal, p0);
        Quadric::plane(
            normal[0],
            normal[1],
            normal[2],
            -distance,
            area.sqrt() * weight,
        )
    }

    fn triangle_edge(p0: V3, p1: V3, p2: V3, weight: f32) -> Quadric {
        let p10 = sub(p1, p0);
        let lengthsq = dot(p10, p10);
        let length = lengthsq.sqrt();
        let p20 = sub(p2, p0);
        let p20p = dot(p20, p10);
        let mut perp = [
            p20[0] * lengthsq - p10[0] * p20p,
            p20[1] * lengthsq - p10[1] * p20p,
            p20[2] * lengthsq - p10[2] * p20p,
        ];
        normalize(&mut perp);
        let distance = dot(perp, p0);
        Quadric::plane(perp[0], perp[1], perp[2], -distance, length * weight)
    }

    fn attributes(p: [V3; 3], va: [&[f32]; 3], g: &mut [Grad]) -> Quadric {
        let v0 = sub(p[1], p[0]);
        let v1 = sub(p[2], p[0]);
        let normal = cross(v0, v1);
        let area = dot(normal, normal).sqrt() * 0.5;
        let w = area;
        let d00 = dot(v0, v0);
        let d01 = dot(v0, v1);
        let d11 = dot(v1, v1);
        let denom = d00 * d11 - d01 * d01;
        let denomr = if denom == 0.0 { 0.0 } else { 1.0 / denom };
        let gx1 = (d11 * v0[0] - d01 * v1[0]) * denomr;
        let gx2 = (d00 * v1[0] - d01 * v0[0]) * denomr;
        let gy1 = (d11 * v0[1] - d01 * v1[1]) * denomr;
        let gy2 = (d00 * v1[1] - d01 * v0[1]) * denomr;
        let gz1 = (d11 * v0[2] - d01 * v1[2]) * denomr;
        let gz2 = (d00 * v1[2] - d01 * v0[2]) * denomr;
        let mut q = Quadric {
            w,
            ..Quadric::default()
        };
        for (k, gk) in g.iter_mut().enumerate() {
            let (a0, a1, a2) = (va[0][k], va[1][k], va[2][k]);
            let gx = gx1 * (a1 - a0) + gx2 * (a2 - a0);
            let gy = gy1 * (a1 - a0) + gy2 * (a2 - a0);
            let gz = gz1 * (a1 - a0) + gz2 * (a2 - a0);
            let gw = a0 - p[0][0] * gx - p[0][1] * gy - p[0][2] * gz;
            q.a00 += w * (gx * gx);
            q.a11 += w * (gy * gy);
            q.a22 += w * (gz * gz);
            q.a10 += w * (gy * gx);
            q.a20 += w * (gz * gx);
            q.a21 += w * (gz * gy);
            q.b0 += w * (gx * gw);
            q.b1 += w * (gy * gw);
            q.b2 += w * (gz * gw);
            q.c += w * (gw * gw);
            *gk = Grad {
                gx: w * gx,
                gy: w * gy,
                gz: w * gz,
                gw: w * gw,
            };
        }
        q
    }
}

fn add_grads(g: &mut [Grad], r: &[Grad]) {
    for (a, b) in g.iter_mut().zip(r) {
        a.gx += b.gx;
        a.gy += b.gy;
        a.gz += b.gz;
        a.gw += b.gw;
    }
}

struct Quadrics {
    vertex: Vec<Quadric>,
    attribute: Vec<Quadric>,
    grads: Vec<Grad>,
    count: usize,
}

impl Quadrics {
    fn grads(&self, v: u32) -> &[Grad] {
        let at = v as usize * self.count;
        &self.grads[at..at + self.count]
    }
}

struct Topology<'a> {
    remap: &'a [u32],
    wedge: &'a [u32],
    kind: &'a [u8],
    lp: &'a [u32],
    lb: &'a [u32],
}

fn fill(q: &mut Quadrics, indices: &[u32], points: &[V3], attributes: &[f32], topo: &Topology) {
    let remap = topo.remap;
    for t in indices.chunks_exact(3) {
        let quadric = Quadric::triangle(
            points[t[0] as usize],
            points[t[1] as usize],
            points[t[2] as usize],
            1.0,
        );
        for &i in t {
            q.vertex[remap[i as usize] as usize].add(&quadric);
        }
    }
    for i in 0..points.len() {
        if remap[i] != i as u32 {
            continue;
        }
        let w = q.vertex[i].w * 1e-7;
        q.vertex[i].add(&Quadric::point(points[i], w));
    }
    let (kind, lp, lb) = (topo.kind, topo.lp, topo.lb);
    let edgy = |k: u8| k == BORDER || k == SEAM;
    for t in indices.chunks_exact(3) {
        for e in 0..3 {
            let i0 = t[e];
            let i1 = t[NEXT[e]];
            let k0 = kind[i0 as usize];
            let k1 = kind[i1 as usize];
            if !edgy(k0) && !edgy(k1) {
                continue;
            }
            if edgy(k0) && lp[i0 as usize] != i1 {
                continue;
            }
            if edgy(k1) && lb[i1 as usize] != i0 {
                continue;
            }
            let i2 = t[NEXT[e + 1]];
            let weight = if k0 == BORDER || k1 == BORDER {
                EDGE_WEIGHT_BORDER
            } else {
                EDGE_WEIGHT_SEAM
            };
            let (p0, p1, p2) = (
                points[i0 as usize],
                points[i1 as usize],
                points[i2 as usize],
            );
            let mut quadric = Quadric::triangle_edge(p0, p1, p2, weight);
            let mut qt = Quadric::triangle(p0, p1, p2, weight);
            qt.w = 0.0;
            quadric.add(&qt);
            q.vertex[remap[i0 as usize] as usize].add(&quadric);
            q.vertex[remap[i1 as usize] as usize].add(&quadric);
        }
    }
    let count = q.count;
    if count == 0 {
        return;
    }
    let mut g = vec![Grad::default(); count];
    for t in indices.chunks_exact(3) {
        let [i0, i1, i2] = [t[0] as usize, t[1] as usize, t[2] as usize];
        let qa = Quadric::attributes(
            [points[i0], points[i1], points[i2]],
            [
                &attributes[i0 * count..(i0 + 1) * count],
                &attributes[i1 * count..(i1 + 1) * count],
                &attributes[i2 * count..(i2 + 1) * count],
            ],
            &mut g,
        );
        for i in [i0, i1, i2] {
            q.attribute[i].add(&qa);
            add_grads(&mut q.grads[i * count..(i + 1) * count], &g);
        }
    }
}

fn has_flip(a: V3, b: V3, c: V3, d: V3) -> bool {
    let eb = sub(b, a);
    let ec = sub(c, a);
    let ed = sub(d, a);
    let nbc = cross(eb, ec);
    let nbd = cross(eb, ed);
    let ndp = dot(nbc, nbd);
    let abc = dot(nbc, nbc);
    let abd = dot(nbd, nbd);
    ndp <= 0.25 * (abc * abd).sqrt()
}

fn has_flips(adjacency: &Adjacency, points: &[V3], collapse: &[u32], i0: u32, i1: u32) -> bool {
    let v0 = points[i0 as usize];
    let v1 = points[i1 as usize];
    for at in adjacency.range(i0) {
        let a = collapse[adjacency.next[at] as usize];
        let b = collapse[adjacency.prev[at] as usize];
        if a == i1 || b == i1 || a == b {
            continue;
        }
        if has_flip(points[a as usize], points[b as usize], v0, v1) {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy)]
struct Collapse {
    v0: u32,
    v1: u32,
    bidi: bool,
    error: f32,
}

fn bound(adjacency: &Adjacency, kind: &[u8], index_count: usize) -> usize {
    let dual: usize = (0..kind.len())
        .filter(|&i| kind[i] == MANIFOLD || kind[i] == SEAM)
        .map(|i| adjacency.range(i as u32).len())
        .sum();
    index_count - dual / 2 + 3
}

fn pick(collapses: &mut Vec<Collapse>, capacity: usize, indices: &[u32], topo: &Topology) {
    collapses.clear();
    let (remap, kind, lp, lb) = (topo.remap, topo.kind, topo.lp, topo.lb);
    let edgy = |k: u8| k == BORDER || k == SEAM;
    for t in indices.chunks_exact(3) {
        if collapses.len() + 3 > capacity {
            break;
        }
        for e in 0..3 {
            let i0 = t[e];
            let i1 = t[NEXT[e]];
            let (u0, u1) = (i0 as usize, i1 as usize);
            if remap[u0] == remap[u1] {
                continue;
            }
            let k0 = kind[u0] as usize;
            let k1 = kind[u1] as usize;
            if CAN_COLLAPSE[k0][k1] | CAN_COLLAPSE[k1][k0] == 0 {
                continue;
            }
            if HAS_OPPOSITE[k0][k1] != 0 && remap[u1] > remap[u0] {
                continue;
            }
            if edgy(k0 as u8) && k1 as u8 != MANIFOLD && lp[u0] != i1 {
                continue;
            }
            if edgy(k1 as u8) && k0 as u8 != MANIFOLD && lb[u1] != i0 {
                continue;
            }
            let (v0, v1, bidi) = if CAN_COLLAPSE[k0][k1] & CAN_COLLAPSE[k1][k0] != 0 {
                (i0, i1, true)
            } else if CAN_COLLAPSE[k0][k1] != 0 {
                (i0, i1, false)
            } else {
                (i1, i0, false)
            };
            collapses.push(Collapse {
                v0,
                v1,
                bidi,
                error: 0.0,
            });
        }
    }
}

fn seam_pair(topo: &Topology, i0: u32, i1: u32) -> (u32, u32) {
    let (lp, lb) = (topo.lp, topo.lb);
    let s0 = topo.wedge[i0 as usize];
    let s1 = if lp[i0 as usize] == i1 {
        lb[s0 as usize]
    } else {
        lp[s0 as usize]
    };
    let s1 = if s1 != NONE {
        s1
    } else {
        topo.wedge[i1 as usize]
    };
    (s0, s1)
}

fn rank(
    collapses: &mut [Collapse],
    points: &[V3],
    attributes: &[f32],
    q: &Quadrics,
    topo: &Topology,
) {
    let count = q.count;
    let attr = |v: u32| &attributes[v as usize * count..(v as usize + 1) * count];
    let aerr = |from: u32, to: u32| {
        q.attribute[from as usize].attribute_error(q.grads(from), points[to as usize], attr(to))
    };
    let (remap, wedge, kind) = (topo.remap, topo.wedge, topo.kind);
    for c in collapses.iter_mut() {
        let (i0, i1, bidi) = (c.v0, c.v1, c.bidi);
        let mut ei = q.vertex[remap[i0 as usize] as usize].error(points[i1 as usize]);
        let mut ej = if bidi {
            q.vertex[remap[i1 as usize] as usize].error(points[i0 as usize])
        } else {
            f32::MAX
        };
        if count > 0 {
            ei += aerr(i0, i1);
            ej += if bidi { aerr(i1, i0) } else { 0.0 };
            if kind[i0 as usize] == SEAM {
                let (s0, s1) = seam_pair(topo, i0, i1);
                ei += aerr(s0, s1);
                ej += if bidi { aerr(s1, s0) } else { 0.0 };
            } else {
                if kind[i0 as usize] == COMPLEX {
                    let mut v = wedge[i0 as usize];
                    while v != i0 {
                        ei += aerr(v, i1);
                        v = wedge[v as usize];
                    }
                }
                if kind[i1 as usize] == COMPLEX && bidi {
                    let mut v = wedge[i1 as usize];
                    while v != i1 {
                        ej += aerr(v, i0);
                        v = wedge[v as usize];
                    }
                }
            }
        }
        let rev = bidi && ej < ei;
        c.v0 = if rev { i1 } else { i0 };
        c.v1 = if rev { i0 } else { i1 };
        c.error = if ej < ei { ej } else { ei };
    }
}

fn sort(order: &mut Vec<u32>, collapses: &[Collapse]) {
    let key =
        |c: &Collapse| (((c.error.to_bits() << 1) >> (32 - SORT_BITS)) as usize).min(SORT_BINS - 1);
    let mut histogram = vec![0u32; SORT_BINS];
    for c in collapses {
        histogram[key(c)] += 1;
    }
    let mut sum = 0;
    for h in histogram.iter_mut() {
        let count = *h;
        *h = sum;
        sum += count;
    }
    order.clear();
    order.resize(collapses.len(), 0);
    for (i, c) in collapses.iter().enumerate() {
        let k = key(c);
        order[histogram[k] as usize] = i as u32;
        histogram[k] += 1;
    }
}

struct Pass<'a> {
    collapse: &'a mut [u32],
    locked: &'a mut [bool],
    result_error: &'a mut f32,
}

fn perform(
    pass: Pass,
    collapses: &[Collapse],
    order: &[u32],
    topo: &Topology,
    points: &[V3],
    adjacency: &Adjacency,
    triangle_goal: usize,
) -> usize {
    let mut edge_collapses = 0;
    let mut triangle_collapses = 0;
    let mut edge_goal = triangle_goal / 2;
    let count = collapses.len();
    let (remap, wedge, kind) = (topo.remap, topo.wedge, topo.kind);
    for &at in order {
        let c = collapses[at as usize];
        if triangle_collapses >= triangle_goal {
            break;
        }
        let error_goal = if edge_goal < count {
            1.5 * collapses[order[edge_goal] as usize].error
        } else {
            f32::MAX
        };
        if c.error > error_goal
            && c.error > *pass.result_error
            && triangle_collapses > triangle_goal / 6
        {
            break;
        }
        let (i0, i1) = (c.v0, c.v1);
        let (r0, r1) = (remap[i0 as usize], remap[i1 as usize]);
        let k = kind[i0 as usize];
        if pass.locked[r0 as usize] || pass.locked[r1 as usize] {
            continue;
        }
        if has_flips(adjacency, points, pass.collapse, r0, r1) {
            edge_goal += 1;
            continue;
        }
        if k == COMPLEX {
            let mut v = i0;
            loop {
                pass.collapse[v as usize] = i1;
                v = wedge[v as usize];
                if v == i0 {
                    break;
                }
            }
        } else if k == SEAM {
            let (s0, s1) = seam_pair(topo, i0, i1);
            pass.collapse[i0 as usize] = i1;
            pass.collapse[s0 as usize] = s1;
        } else {
            pass.collapse[i0 as usize] = i1;
        }
        pass.locked[r0 as usize] = true;
        pass.locked[r1 as usize] = true;
        triangle_collapses += if k == BORDER { 1 } else { 2 };
        edge_collapses += 1;
        if *pass.result_error < c.error {
            *pass.result_error = c.error;
        }
    }
    edge_collapses
}

fn merge(q: &mut Quadrics, collapse: &[u32], remap: &[u32]) {
    let count = q.count;
    for i in 0..collapse.len() {
        let i1 = collapse[i] as usize;
        if i1 == i {
            continue;
        }
        let r0 = remap[i] as usize;
        let r1 = remap[i1] as usize;
        if i == r0 {
            let from = q.vertex[r0];
            q.vertex[r1].add(&from);
        }
        if count > 0 {
            let from = q.attribute[i];
            q.attribute[i1].add(&from);
            let grads: Vec<Grad> = q.grads[i * count..(i + 1) * count].to_vec();
            add_grads(&mut q.grads[i1 * count..(i1 + 1) * count], &grads);
        }
    }
}

fn remap_loops(lp: &mut [u32], collapse: &[u32]) {
    for i in 0..lp.len() {
        if lp[i] == NONE {
            continue;
        }
        let l = lp[i];
        let r = collapse[l as usize];
        lp[i] = if i as u32 == r {
            let next = lp[l as usize];
            if next != NONE {
                collapse[next as usize]
            } else {
                NONE
            }
        } else {
            r
        };
    }
}

fn remap_indices(indices: &mut Vec<u32>, tris: &mut Vec<u32>, collapse: &[u32], remap: &[u32]) {
    let mut write = 0;
    for t in 0..indices.len() / 3 {
        let v = [0, 1, 2].map(|k| collapse[indices[t * 3 + k] as usize]);
        let r = v.map(|x| remap[x as usize]);
        if r[0] != r[1] && r[0] != r[2] && r[1] != r[2] {
            indices[write..write + 3].copy_from_slice(&v);
            tris[write / 3] = tris[t];
            write += 3;
        }
    }
    indices.truncate(write);
    tris.truncate(write / 3);
}

fn reduce(
    positions: &[V3],
    attributes: &[f32],
    count: usize,
    indices: &mut Vec<u32>,
    tris: &mut Vec<u32>,
    target_index_count: usize,
) -> f32 {
    let n = positions.len();
    if indices.len() <= target_index_count || n == 0 {
        return 0.0;
    }
    let mut adjacency = Adjacency::new(n, indices.len());
    adjacency.update(indices, None);
    let (remap, wedge) = position_remap(positions);
    let (kind, mut lp, mut lb) = classify(&adjacency, &remap, &wedge);
    let (points, scale) = rescale(positions);
    let mut q = Quadrics {
        vertex: vec![Quadric::default(); n],
        attribute: vec![Quadric::default(); if count > 0 { n } else { 0 }],
        grads: vec![Grad::default(); n * count],
        count,
    };
    fill(
        &mut q,
        indices,
        &points,
        attributes,
        &Topology {
            remap: &remap,
            wedge: &wedge,
            kind: &kind,
            lp: &lp,
            lb: &lb,
        },
    );
    let capacity = bound(&adjacency, &kind, indices.len());
    let mut collapses = Vec::with_capacity(capacity);
    let mut order = Vec::with_capacity(capacity);
    let mut collapse = vec![0u32; n];
    let mut locked = vec![false; n];
    let mut result_error = 0.0f32;
    while indices.len() > target_index_count {
        adjacency.update(indices, Some(&remap));
        let pass = Topology {
            remap: &remap,
            wedge: &wedge,
            kind: &kind,
            lp: &lp,
            lb: &lb,
        };
        pick(&mut collapses, capacity, indices, &pass);
        if collapses.is_empty() {
            break;
        }
        rank(&mut collapses, &points, attributes, &q, &pass);
        sort(&mut order, &collapses);
        let goal = (indices.len() - target_index_count) / 3;
        for (i, c) in collapse.iter_mut().enumerate() {
            *c = i as u32;
        }
        locked.fill(false);
        let done = perform(
            Pass {
                collapse: &mut collapse,
                locked: &mut locked,
                result_error: &mut result_error,
            },
            &collapses,
            &order,
            &pass,
            &points,
            &adjacency,
            goal,
        );
        if done == 0 {
            break;
        }
        merge(&mut q, &collapse, &remap);
        remap_loops(&mut lp, &collapse);
        remap_loops(&mut lb, &collapse);
        remap_indices(indices, tris, &collapse, &remap);
    }
    result_error.sqrt() * scale
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Morph, surface, torus};
    use std::collections::{BTreeMap, BTreeSet};

    fn key(p: [f32; 3]) -> [u32; 3] {
        p.map(f32::to_bits)
    }

    fn position_edges(m: &Mesh) -> BTreeMap<([u32; 3], [u32; 3]), usize> {
        let mut edges = BTreeMap::new();
        for t in m.indices.chunks_exact(3) {
            for e in 0..3 {
                let a = key(m.positions[t[e] as usize]);
                let b = key(m.positions[t[(e + 1) % 3] as usize]);
                *edges.entry((a, b)).or_insert(0) += 1;
            }
        }
        edges
    }

    #[test]
    fn a_closed_torus_reaches_its_target_and_stays_closed_with_its_seams_whole() {
        let ring = torus(1.0, 0.4, 48, 24);
        let target = ring.triangle_count() / 4;
        let s = simplify(&ring, target);
        let got = s.mesh.triangle_count();
        assert!(
            got <= target && got + target / 10 >= target,
            "{got} for {target}"
        );
        let edges = position_edges(&s.mesh);
        for (&(a, b), &count) in &edges {
            assert_eq!(count, 1, "an edge is used once each way");
            assert_eq!(edges.get(&(b, a)), Some(&1), "the surface stays closed");
        }
        let uvs = s.mesh.uvs.as_ref().unwrap();
        for t in s.mesh.indices.chunks_exact(3) {
            for axis in [0, 1] {
                let at = |i: &u32| uvs[*i as usize][axis];
                let lo = t.iter().map(at).fold(f32::MAX, f32::min);
                let hi = t.iter().map(at).fold(f32::MIN, f32::max);
                assert!(hi - lo < 0.5, "no triangle wraps across a UV seam");
            }
        }
        for (v, &source) in s.sources.iter().enumerate() {
            assert_eq!(s.mesh.positions[v], ring.positions[source as usize]);
            assert_eq!(uvs[v], ring.uvs.as_ref().unwrap()[source as usize]);
        }
        assert_eq!(
            s,
            simplify(&ring, target),
            "the same input gives the same output"
        );
    }

    #[test]
    fn an_open_border_keeps_every_vertex_and_morph_deltas_follow_the_kept_vertices() {
        let sheet = surface([24, 24], |u, v| {
            [u, v, (u * 3.0).sin() * (v * 2.0).cos() * 0.2]
        });
        let s = simplify(&sheet, sheet.triangle_count() / 5);
        assert!(s.mesh.triangle_count() < sheet.triangle_count() / 3);
        let kept: BTreeSet<[u32; 3]> = s.mesh.positions.iter().map(|&p| key(p)).collect();
        for p in &sheet.positions {
            if p[0] == 0.0 || p[0] == 1.0 || p[1] == 0.0 || p[1] == 1.0 {
                assert!(kept.contains(&key(*p)), "border vertex {p:?} is kept");
            }
        }
        let mut morph = Morph {
            names: vec!["lift".into()],
            weights: vec![1.0],
            positions: vec![
                sheet
                    .positions
                    .iter()
                    .map(|p| [0.0, 0.0, p[0] * 2.0])
                    .collect(),
            ],
            normals: Vec::new(),
        };
        morph.remap(&s.sources);
        assert_eq!(morph.positions[0].len(), s.mesh.positions.len());
        for (p, d) in s.mesh.positions.iter().zip(&morph.positions[0]) {
            assert_eq!(d[2], p[0] * 2.0);
        }
    }
}
