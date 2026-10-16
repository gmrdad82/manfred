use crate::{Error, fail};
use glam::{DMat4, DVec2, DVec3};
use manfred_mesh::math::Real;
use manfred_mesh::{Mesh, triangle_normal};
use manfred_recipe::{DetailUv, Object};
use std::collections::{BTreeMap, VecDeque};

pub const CHART_DEGREES: f64 = 8.0;
pub const SMALLEST: u32 = 64;
pub const LARGEST: u32 = 4096;

const TOUCH: f64 = 1e-9;

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub corners: Vec<[f32; 2]>,
    pub size: u32,
    pub charts: Vec<Vec<usize>>,
}

struct Chart {
    faces: Vec<usize>,
    flat: Vec<[DVec2; 3]>,
    extent: DVec2,
}

impl Chart {
    fn take(&mut self, grid: &mut Grid, face: usize, tri: [DVec2; 3]) {
        grid.insert(&tri, self.flat.len());
        self.flat.push(tri);
        self.faces.push(face);
    }
}

fn basis(axis: DVec3) -> (DVec3, DVec3) {
    let helper = if axis.y.abs() < 0.9 {
        DVec3::Y
    } else {
        DVec3::X
    };
    let u = helper.cross(axis).normalize();
    (u, axis.cross(u))
}

fn reach(tri: &[DVec2; 3]) -> (DVec2, DVec2) {
    (
        tri[0].min(tri[1]).min(tri[2]),
        tri[0].max(tri[1]).max(tri[2]),
    )
}

fn apart(a: &[DVec2; 3], b: &[DVec2; 3], tolerance: f64) -> bool {
    for (one, other) in [(a, b), (b, a)] {
        for k in 0..3 {
            let edge = one[(k + 1) % 3] - one[k];
            let n = DVec2::new(-edge.y, edge.x);
            let span = |t: &[DVec2; 3]| {
                let d = t.map(|p| n.dot(p));
                (d[0].min(d[1]).min(d[2]), d[0].max(d[1]).max(d[2]))
            };
            let ((lo_a, hi_a), (lo_b, hi_b)) = (span(one), span(other));
            let slack = tolerance * n.length();
            if hi_a <= lo_b + slack || hi_b <= lo_a + slack {
                return true;
            }
        }
    }
    false
}

struct Grid {
    cell: f64,
    cells: BTreeMap<(i64, i64), Vec<usize>>,
}

impl Grid {
    fn keys(&self, tri: &[DVec2; 3]) -> impl Iterator<Item = (i64, i64)> {
        let (lo, hi) = reach(tri);
        let (x0, y0) = (
            (lo.x / self.cell).floor() as i64,
            (lo.y / self.cell).floor() as i64,
        );
        let (x1, y1) = (
            (hi.x / self.cell).floor() as i64,
            (hi.y / self.cell).floor() as i64,
        );
        (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
    }

    fn clear(&self, tri: &[DVec2; 3], placed: &[[DVec2; 3]], tolerance: f64) -> bool {
        let mut seen = Vec::new();
        for key in self.keys(tri) {
            for &other in self.cells.get(&key).into_iter().flatten() {
                if seen.contains(&other) {
                    continue;
                }
                seen.push(other);
                if !apart(tri, &placed[other], tolerance) {
                    return false;
                }
            }
        }
        true
    }

    fn insert(&mut self, tri: &[DVec2; 3], index: usize) {
        let keys: Vec<(i64, i64)> = self.keys(tri).collect();
        for key in keys {
            self.cells.entry(key).or_default().push(index);
        }
    }
}

fn hull(points: &[DVec2]) -> Vec<DVec2> {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    sorted.dedup();
    if sorted.len() < 3 {
        return sorted;
    }
    let turn = |o: DVec2, a: DVec2, b: DVec2| (a - o).perp_dot(b - o);
    let mut lower: Vec<DVec2> = Vec::new();
    for &p in &sorted {
        while lower.len() >= 2 && turn(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<DVec2> = Vec::new();
    for &p in sorted.iter().rev() {
        while upper.len() >= 2 && turn(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

fn settle(flat: &mut [[DVec2; 3]]) -> DVec2 {
    let points: Vec<DVec2> = flat.iter().flatten().copied().collect();
    let ring = hull(&points);
    let mut best = (f64::INFINITY, DVec2::X);
    for k in 0..ring.len() {
        let Some(along) = (ring[(k + 1) % ring.len()] - ring[k]).try_normalize() else {
            continue;
        };
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for p in &ring {
            let q = DVec2::new(along.dot(*p), along.perp_dot(*p));
            lo = lo.min(q);
            hi = hi.max(q);
        }
        let area = (hi - lo).x * (hi - lo).y;
        if area < best.0 {
            best = (area, along);
        }
    }
    let along = best.1;
    let turn = |p: DVec2| DVec2::new(along.dot(p), along.perp_dot(p));
    for tri in flat.iter_mut() {
        *tri = tri.map(turn);
    }
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in flat.iter().flatten() {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    if !lo.x.is_finite() {
        return DVec2::ZERO;
    }
    let mut extent = hi - lo;
    let upright = extent.y > extent.x;
    for tri in flat.iter_mut() {
        *tri = tri.map(|p| {
            let q = p - lo;
            if upright {
                DVec2::new(extent.y - q.y, q.x)
            } else {
                q
            }
        });
    }
    if upright {
        extent = DVec2::new(extent.y, extent.x);
    }
    extent
}

fn charts(world: &[[DVec3; 3]], keys: &[[u32; 3]]) -> Vec<Chart> {
    let normals: Vec<DVec3> = world.iter().map(triangle_normal).collect();
    let mut edges: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
    for (face, key) in keys.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (key[k], key[(k + 1) % 3]);
            if a != b {
                edges.entry((a.min(b), a.max(b))).or_default().push(face);
            }
        }
    }
    let longest = world
        .iter()
        .map(|t| {
            (t[0] - t[1])
                .length()
                .max((t[1] - t[2]).length())
                .max((t[2] - t[0]).length())
        })
        .fold(0.0, f64::max);
    let spread = world
        .iter()
        .flatten()
        .fold(0.0f64, |m, p| m.max(p.abs().max_element()));
    let tolerance = TOUCH * spread.max(longest).max(1e-12);
    let limit = CHART_DEGREES.to_radians().cos_of();
    let mut owner = vec![usize::MAX; world.len()];
    let mut out = Vec::new();
    let seeds: Vec<usize> = (0..world.len())
        .filter(|&f| normals[f] != DVec3::ZERO)
        .chain((0..world.len()).filter(|&f| normals[f] == DVec3::ZERO))
        .collect();
    let mean = world
        .iter()
        .map(|t| (t[0] - t[1]).length() + (t[1] - t[2]).length() + (t[2] - t[0]).length())
        .sum::<f64>()
        / (3 * world.len().max(1)) as f64;
    for seed in seeds {
        if owner[seed] != usize::MAX {
            continue;
        }
        let axis = if normals[seed] == DVec3::ZERO {
            DVec3::Z
        } else {
            normals[seed]
        };
        let (u, v) = basis(axis);
        let flat_of = |f: usize| world[f].map(|p| DVec2::new(p.dot(u), p.dot(v)));
        let mut grid = Grid {
            cell: mean.max(1e-9),
            cells: BTreeMap::new(),
        };
        let mut chart = Chart {
            faces: Vec::new(),
            flat: Vec::new(),
            extent: DVec2::ZERO,
        };
        let index = out.len();
        owner[seed] = index;
        chart.take(&mut grid, seed, flat_of(seed));
        let mut queue = VecDeque::from([seed]);
        while let Some(face) = queue.pop_front() {
            let key = keys[face];
            for k in 0..3 {
                let (a, b) = (key[k], key[(k + 1) % 3]);
                let Some(around) = edges.get(&(a.min(b), a.max(b))) else {
                    continue;
                };
                for &next in around {
                    if owner[next] != usize::MAX {
                        continue;
                    }
                    let n = normals[next];
                    if n != DVec3::ZERO && n.dot(axis) < limit {
                        continue;
                    }
                    let tri = flat_of(next);
                    if grid.clear(&tri, &chart.flat, tolerance) {
                        owner[next] = index;
                        chart.take(&mut grid, next, tri);
                        queue.push_back(next);
                    }
                }
            }
        }
        chart.extent = settle(&mut chart.flat);
        out.push(chart);
    }
    out
}

fn texels(length: f64, texels_per_m: f64) -> u32 {
    ((length * texels_per_m).ceil() as u32).max(1)
}

fn pack(sizes: &[[u32; 2]], gutter: u32) -> Option<(u32, Vec<[u32; 2]>)> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by(|&a, &b| {
        sizes[b][1]
            .cmp(&sizes[a][1])
            .then(sizes[b][0].cmp(&sizes[a][0]))
            .then(a.cmp(&b))
    });
    let mut side = SMALLEST;
    while side <= LARGEST {
        if let Some(spots) = shelves(sizes, &order, gutter, side) {
            return Some((side, spots));
        }
        side *= 2;
    }
    None
}

fn shelves(sizes: &[[u32; 2]], order: &[usize], gutter: u32, side: u32) -> Option<Vec<[u32; 2]>> {
    let mut spots = vec![[0, 0]; sizes.len()];
    let (mut x, mut y, mut tall) = (gutter, gutter, 0);
    for &i in order {
        let [w, h] = sizes[i];
        if x + w + gutter > side && x > gutter {
            y += tall + gutter;
            x = gutter;
            tall = 0;
        }
        if x + w + gutter > side || y + h + gutter > side {
            return None;
        }
        spots[i] = [x, y];
        x += w + gutter;
        tall = tall.max(h);
    }
    Some(spots)
}

pub fn layout(world: &[[DVec3; 3]], texels_per_m: f64, gutter: u32) -> Result<Layout, String> {
    let mut ids: BTreeMap<[u64; 3], u32> = BTreeMap::new();
    let keys: Vec<[u32; 3]> = world
        .iter()
        .map(|t| {
            t.map(|p| {
                let next = ids.len() as u32;
                *ids.entry(p.to_array().map(f64::to_bits)).or_insert(next)
            })
        })
        .collect();
    let found = charts(world, &keys);
    let sizes: Vec<[u32; 2]> = found
        .iter()
        .map(|c| {
            [
                texels(c.extent.x, texels_per_m),
                texels(c.extent.y, texels_per_m),
            ]
        })
        .collect();
    let Some((size, spots)) = pack(&sizes, gutter) else {
        let widest = sizes.iter().map(|s| s[0].max(s[1])).max().unwrap_or(0);
        return Err(format!(
            "the layout does not fit {LARGEST} texels across at {texels_per_m} texels per metre (its widest chart is {widest} texels), expected a lower `texels_per_m`"
        ));
    };
    let scale = texels_per_m / f64::from(size);
    let mut corners = vec![[0.0f32; 2]; world.len() * 3];
    for (chart, spot) in found.iter().zip(&spots) {
        let origin = DVec2::new(f64::from(spot[0]), f64::from(spot[1])) / f64::from(size);
        for (face, tri) in chart.faces.iter().zip(&chart.flat) {
            for (k, p) in tri.iter().enumerate() {
                let q = origin + *p * scale;
                corners[face * 3 + k] = [q.x as f32, (1.0 - q.y) as f32];
            }
        }
    }
    Ok(Layout {
        corners,
        size,
        charts: found.into_iter().map(|c| c.faces).collect(),
    })
}

fn assign(m: &mut Mesh, corners: &[[f32; 2]]) -> Vec<u32> {
    let mut sources: Vec<u32> = (0..m.positions.len() as u32).collect();
    let mut first: Vec<Option<[u32; 2]>> = vec![None; m.positions.len()];
    let mut copies: BTreeMap<(u32, [u32; 2]), u32> = BTreeMap::new();
    let mut uvs1 = vec![[0.0f32; 2]; m.positions.len()];
    for (slot, &uv) in m.indices.iter_mut().zip(corners) {
        let old = *slot;
        let bits = uv.map(f32::to_bits);
        match first[old as usize] {
            None => {
                first[old as usize] = Some(bits);
                uvs1[old as usize] = uv;
            }
            Some(seen) if seen == bits => {}
            Some(_) => {
                *slot = *copies.entry((old, bits)).or_insert_with(|| {
                    let i = old as usize;
                    sources.push(old);
                    m.positions.push(m.positions[i]);
                    if m.normals.len() > i {
                        m.normals.push(m.normals[i]);
                    }
                    if let Some(uvs) = &mut m.uvs {
                        uvs.push(uvs[i]);
                    }
                    uvs1.push(uv);
                    (m.positions.len() - 1) as u32
                });
            }
        }
    }
    m.uvs1 = Some(uvs1);
    sources
}

pub(crate) fn apply(
    object: &Object,
    matrix: DMat4,
    m: &mut Mesh,
) -> Result<Option<Vec<u32>>, Error> {
    let Some(DetailUv {
        texels_per_m,
        gutter,
    }) = object.detail_uv
    else {
        return Ok(None);
    };
    if m.indices.is_empty() {
        return Ok(None);
    }
    let flip = matrix.determinant() < 0.0;
    let mut world = m.world_triangles(matrix);
    if flip {
        for tri in &mut world {
            tri.swap(1, 2);
        }
    }
    let mut found =
        layout(&world, texels_per_m, gutter).map_err(|text| fail(object, "detail_uv", text))?;
    if flip {
        for c in found.corners.chunks_exact_mut(3) {
            c.swap(1, 2);
        }
    }
    Ok(Some(assign(m, &found.corners)))
}
