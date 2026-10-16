use crate::build::{Build, band, degenerate};
use crate::math::Real;
use crate::normals::{find, union};
use crate::outline::{self, Outline, Shape, edge, from_points};
use crate::{Mesh, Pocket, Profile, ProfileKind, UvMode};
use glam::{DVec2, DVec3};
use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_2;

fn width(p: &Profile) -> f64 {
    match p.kind {
        ProfileKind::None => 0.0,
        _ => p.width.max(0.0),
    }
}

fn rows(p: &Profile, w: f64) -> Vec<(f64, f64)> {
    if w <= 0.0 {
        return vec![(0.0, 0.0)];
    }
    match p.kind {
        ProfileKind::None => vec![(0.0, 0.0)],
        ProfileKind::Chamfer => vec![(w, 0.0), (0.0, w)],
        ProfileKind::Round => {
            let n = p.segments.max(1);
            (0..=n)
                .map(|k| {
                    let t = FRAC_PI_2 * k as f64 / n as f64;
                    (w * (1.0 - t.sin_of()), w * (1.0 - t.cos_of()))
                })
                .collect()
        }
    }
}

fn sag(p: &Profile, w: f64, s: f64) -> f64 {
    if w <= 0.0 || s >= w {
        return 0.0;
    }
    match p.kind {
        ProfileKind::None => 0.0,
        ProfileKind::Chamfer => w - s.max(0.0),
        ProfileKind::Round => {
            let d = w - s.max(0.0);
            w - (w * w - d * d).max(0.0).sqrt()
        }
    }
}

const SHARP_DEGREES: f64 = 40.0;

fn corners(o: Vec<DVec2>, p: &Profile) -> Vec<DVec2> {
    let w = width(p);
    let n = o.len();
    if w <= 0.0 || n < 3 {
        return o;
    }
    let limit = SHARP_DEGREES.to_radians().cos_of();
    let sharp: Vec<bool> = (0..n)
        .map(|i| {
            let a = (o[i] - o[(i + n - 1) % n]).normalize_or_zero();
            let c = (o[(i + 1) % n] - o[i]).normalize_or_zero();
            a.dot(c) < limit
        })
        .collect();
    if !sharp.contains(&true) {
        return o;
    }
    let segments = match p.kind {
        ProfileKind::Round => p.segments.max(1),
        _ => 1,
    };
    let mut out = Vec::new();
    for (i, &v) in o.iter().enumerate() {
        if !sharp[i] {
            out.push(v.to_array());
            continue;
        }
        let (prev, next) = ((i + n - 1) % n, (i + 1) % n);
        let reach = |j: usize| {
            let l = o[j].distance(v);
            if sharp[j] { 0.5 * l } else { l }
        };
        let t = w.min(reach(prev)).min(reach(next));
        let a = v + (o[prev] - v).normalize_or_zero() * t;
        let b = v + (o[next] - v).normalize_or_zero() * t;
        let c = a + b - v;
        for k in 0..=segments {
            let (sin, cos) = (FRAC_PI_2 * k as f64 / segments as f64).sin_cos_of();
            out.push((c + (a - c) * cos + (b - c) * sin).to_array());
        }
    }
    outline::ring(&Outline(out))
}

fn at(p: DVec2, depth: f64) -> DVec3 {
    DVec3::new(p.x, p.y, -depth)
}

struct Group {
    ring: Vec<DVec2>,
    depth: f64,
    rim: Profile,
    floor: Profile,
}

fn groups(pockets: &[Pocket]) -> Vec<Group> {
    let rings: Vec<Vec<DVec2>> = pockets.iter().map(|p| outline::ring(&p.outline)).collect();
    let n = pockets.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in i + 1..n {
            if rings[i].len() < 3 || rings[j].len() < 3 {
                continue;
            }
            let a = Shape::new(from_points(&rings[i]));
            let b = Shape::new(from_points(&rings[j]));
            let common: f64 = outline::intersection(&a, &b).iter().map(Shape::area).sum();
            if common > 0.0 {
                union(&mut parent, i, j);
            }
        }
    }
    let mut out = Vec::new();
    for i in 0..n {
        if find(&mut parent, i) != i || rings[i].len() < 3 {
            continue;
        }
        let members: Vec<usize> = (0..n).filter(|&j| find(&mut parent, j) == i).collect();
        let ring = if members.len() == 1 {
            rings[i].clone()
        } else {
            let outlines: Vec<Outline> = members
                .iter()
                .map(|&j| pockets[j].outline.clone())
                .collect();
            let shapes = outline::union(&outlines);
            shapes
                .iter()
                .max_by(|a, b| a.area().total_cmp(&b.area()))
                .map(|s| outline::ring(&s.outer))
                .unwrap_or_default()
        };
        let depth = members
            .iter()
            .map(|&j| pockets[j].depth)
            .fold(f64::NEG_INFINITY, f64::max);
        out.push(Group {
            ring,
            depth,
            rim: pockets[i].rim,
            floor: pockets[i].floor,
        });
    }
    out
}

struct Index {
    cell: f64,
    tol: f64,
    grid: BTreeMap<(i64, i64), Vec<u32>>,
}

impl Index {
    fn new(tol: f64) -> Index {
        Index {
            cell: tol * 4.0,
            tol,
            grid: BTreeMap::new(),
        }
    }

    fn key(&self, p: DVec2) -> (i64, i64) {
        (
            (p.x / self.cell).floor() as i64,
            (p.y / self.cell).floor() as i64,
        )
    }

    fn add(&mut self, p: DVec2, id: u32) {
        let k = self.key(p);
        self.grid.entry(k).or_default().push(id);
    }

    fn find(&self, p: DVec2, pos: &[DVec3]) -> Option<u32> {
        let (kx, ky) = self.key(p);
        let mut best: Option<(f64, u32)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(list) = self.grid.get(&(kx + dx, ky + dy)) {
                    for &id in list {
                        let d = pos[id as usize].truncate().distance(p);
                        if d <= self.tol
                            && best.is_none_or(|(bd, bi)| d < bd || (d == bd && id < bi))
                        {
                            best = Some((d, id));
                        }
                    }
                }
            }
        }
        best.map(|(_, id)| id)
    }
}

pub(crate) fn slab(
    shape: &Shape,
    front: f64,
    back: f64,
    profile: &Profile,
    pockets: &[Pocket],
    uv: UvMode,
) -> Mesh {
    let o = corners(outline::ring(&shape.outer), profile);
    if o.len() < 3 {
        return Mesh::default();
    }
    let mut contours = vec![o.clone()];
    for h in &shape.holes {
        let mut ring = corners(outline::ring(h), profile);
        if ring.len() >= 3 {
            ring.reverse();
            contours.push(ring);
        }
    }
    let (front, back) = if front <= back {
        (front, back)
    } else {
        (back, front)
    };
    let scale = outline::extent(&o);
    let tol = scale * 1e-7;
    let miters: Vec<Vec<DVec2>> = contours.iter().map(|c| outline::miters(c)).collect();
    let mut w = width(profile)
        .min(0.49 * (back - front))
        .min(0.95 * outline::reach(&o, &miters[0], -1.0));
    for (c, m) in contours.iter().zip(&miters).skip(1) {
        w = w.min(0.95 * outline::reach(c, m, -1.0));
    }
    w = w.min(0.475 * wall(&contours));
    let w = finite_or_zero(w);
    let prof = rows(profile, w);
    let n = prof.len() - 1;
    let rings: Vec<Vec<Vec<DVec2>>> = contours
        .iter()
        .zip(&miters)
        .map(|(c, m)| {
            prof.iter()
                .map(|&(s, _)| outline::shift(c, m, |_| -s))
                .collect()
        })
        .collect();
    let mut b = Build::default();
    let mut fronts: Vec<Vec<Vec<u32>>> = Vec::with_capacity(rings.len());
    for rs in &rings {
        fronts.push(
            rs.iter()
                .zip(&prof)
                .map(|(r, &(_, dz))| r.iter().map(|&p| b.vert(at(p, front + dz))).collect())
                .collect(),
        );
    }
    let mut backs: Vec<Vec<Vec<u32>>> = Vec::with_capacity(rings.len());
    for rs in &rings {
        backs.push(
            rs.iter()
                .zip(&prof)
                .map(|(r, &(_, dz))| r.iter().map(|&p| b.vert(at(p, back - dz))).collect())
                .collect(),
        );
    }
    for (fr, bk) in fronts.iter().zip(&backs) {
        b.strip(&fr[n], &bk[n], true);
        for j in (0..n).rev() {
            b.strip(&bk[j + 1], &bk[j], true);
        }
    }
    let cap_shape = Shape {
        outer: from_points(&rings[0][0]),
        holes: rings[1..].iter().map(|r| from_points(&r[0])).collect(),
    };
    let back_ids: Vec<u32> = backs.iter().flat_map(|bk| bk[0].iter().copied()).collect();
    for t in &outline::triangulate(&cap_shape) {
        b.tri(
            back_ids[t[0] as usize],
            back_ids[t[2] as usize],
            back_ids[t[1] as usize],
        );
    }
    let near = |q: DVec2| {
        contours
            .iter()
            .map(|c| outline::distance(q, c))
            .fold(f64::INFINITY, f64::min)
    };
    let surface = |q: DVec2| front + sag(profile, w, near(q));
    let floor_limit = back - w;

    let mut sheet: Vec<[u32; 3]> = Vec::new();
    let mut edges: Vec<(Vec<DVec2>, Vec<u32>)> = Vec::new();
    let all = groups(pockets);
    let limit = |gi: usize, a: DVec2, c: DVec2| -> f64 {
        let others = all
            .iter()
            .enumerate()
            .filter(|&(k, _)| k != gi)
            .map(|(_, h)| outline::gap(a, c, &h.ring))
            .fold(f64::INFINITY, f64::min);
        let edge = contours
            .iter()
            .map(|k| outline::gap(a, c, k))
            .fold(f64::INFINITY, f64::min);
        (0.45 * others).min(0.9 * edge)
    };
    let crossings = |a: DVec2, c: DVec2, r: f64| -> usize {
        if w <= 0.0 {
            return 0;
        }
        let d = [a, (a + c) * 0.5, c].map(near);
        let lo = d.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = d.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let within = |lo: f64, hi: f64| prof.iter().filter(|&&(s, _)| s >= lo && s <= hi).count();
        within(lo, hi).max(within(lo - r, hi - r))
    };
    let refined: Vec<Vec<DVec2>> = all
        .iter()
        .enumerate()
        .map(|(gi, g)| {
            let r = width(&g.rim);
            let fine = (w / n.max(1) as f64).max(0.5 * r).max(tol);
            let count = g.ring.len();
            let mut out = Vec::with_capacity(count);
            for i in 0..count {
                let mut stack = vec![(g.ring[i], g.ring[(i + 1) % count])];
                while let Some((a, c)) = stack.pop() {
                    let len = a.distance(c);
                    let close = r > 0.0 && len > r && limit(gi, a, c) < r;
                    let steep = len > fine && crossings(a, c, r) > 1;
                    if (close || steep) && out.len() < 1 << 16 {
                        let m = (a + c) * 0.5;
                        stack.push((m, c));
                        stack.push((a, m));
                    } else {
                        out.push(a);
                    }
                }
            }
            out
        })
        .collect();
    for (gi, g) in all.iter().enumerate() {
        let p = &refined[gi];
        let edge_limit: Vec<f64> = (0..p.len())
            .map(|i| limit(gi, p[i], p[(i + 1) % p.len()]))
            .collect();
        let pm = outline::miters(p);
        let depth = g.depth.min(floor_limit).max(front);
        let span = (depth - front).max(0.0);
        let rim_w = finite_or_zero(
            width(&g.rim)
                .min(0.95 * outline::reach(p, &pm, 1.0))
                .min(0.5 * span),
        );
        let floor_w = finite_or_zero(
            width(&g.floor)
                .min(0.95 * outline::reach(p, &pm, -1.0))
                .min(0.5 * span),
        );
        let unit = rows(&g.rim, 1.0);
        let fill: Vec<(f64, f64)> = rows(&g.floor, floor_w).into_iter().rev().collect();
        let mut cols: Vec<Vec<u32>> = vec![Vec::with_capacity(p.len()); unit.len() + fill.len()];
        let mut outer = Vec::with_capacity(p.len());
        for (v, (&q, &m)) in p.iter().zip(&pm).enumerate() {
            let before = edge_limit[(v + p.len() - 1) % p.len()];
            let r = if rim_w > 0.0 {
                rim_w.min(before).min(edge_limit[v])
            } else {
                0.0
            };
            let mut z = f64::NEG_INFINITY;
            for (k, &(s, dz)) in unit.iter().enumerate() {
                let pt = q + m * (s * r);
                z = surface(pt) + dz * r;
                cols[k].push(b.vert(at(pt, z)));
                if k == 0 {
                    outer.push(pt);
                }
            }
            for (k, &(s, rise)) in fill.iter().enumerate() {
                let pt = q - m * s;
                z = z.max(depth - rise).max(surface(pt));
                cols[unit.len() + k].push(b.vert(at(pt, z)));
            }
        }
        for k in 0..unit.len() - 1 {
            for [ai, aj, cj, ci] in band(&cols[k], &cols[k + 1], true) {
                for t in [[ai, aj, cj], [ai, cj, ci]] {
                    if !degenerate(&t) {
                        sheet.push(t);
                    }
                }
            }
        }
        for k in unit.len() - 1..cols.len() - 1 {
            b.band(&cols[k], &cols[k + 1], true);
        }
        let last = cols.last().cloned().unwrap_or_default();
        let floor_pts: Vec<[f64; 2]> = last
            .iter()
            .map(|&id| b.pos[id as usize].truncate().to_array())
            .collect();
        for t in outline::triangulate(&Shape::new(Outline(floor_pts))) {
            b.tri(
                last[t[0] as usize],
                last[t[1] as usize],
                last[t[2] as usize],
            );
        }
        edges.push((outer, cols[0].clone()));
    }

    let reaches: Vec<f64> = contours
        .iter()
        .map(|k| {
            edges
                .iter()
                .flat_map(|(pts, _)| pts.iter())
                .map(|&q| outline::distance(q, k))
                .fold(f64::INFINITY, f64::min)
        })
        .collect();
    let margin = |s: f64| s + 1e-3 * w + tol;
    let touched: Vec<usize> = reaches
        .iter()
        .map(|&reach| {
            if edges.is_empty() {
                0
            } else {
                (0..n).take_while(|&j| reach < margin(prof[j].0)).count()
            }
        })
        .collect();
    for (fr, &t) in fronts.iter().zip(&touched) {
        for j in t..n {
            for [ai, ak, ck, ci] in band(&fr[j], &fr[j + 1], true) {
                sheet.push([ai, ci, ck]);
                sheet.push([ai, ck, ak]);
            }
        }
    }
    let holes: Vec<Outline> = edges.iter().map(|(e, _)| from_points(e)).collect();
    if edges.is_empty() || reaches.iter().all(|&reach| reach >= margin(prof[0].0)) {
        let mut ids: Vec<u32> = fronts.iter().flat_map(|fr| fr[0].iter().copied()).collect();
        for (_, id) in &edges {
            ids.extend(id);
        }
        let mut cap = cap_shape.clone();
        cap.holes.extend(holes);
        for t in outline::triangulate(&cap) {
            sheet.push([ids[t[0] as usize], ids[t[1] as usize], ids[t[2] as usize]]);
        }
    } else {
        let mut known = Index::new(tol);
        for (fr, &t) in fronts.iter().zip(&touched) {
            for ring in fr.iter().take(t + 1) {
                for &id in ring {
                    known.add(b.pos[id as usize].truncate(), id);
                }
            }
        }
        for (_, ids) in &edges {
            for &id in ids {
                known.add(b.pos[id as usize].truncate(), id);
            }
        }
        let mut fresh = Index::new(tol);
        let mut extra: Vec<u32> = Vec::new();
        let mut pieces = vec![cap_shape];
        for (k, (rs, &t)) in rings.iter().zip(&touched).enumerate() {
            for j in 0..t {
                let (outer, inner) = if k == 0 {
                    (&rs[j + 1], &rs[j])
                } else {
                    (&rs[j], &rs[j + 1])
                };
                pieces.push(Shape {
                    outer: from_points(outer),
                    holes: vec![from_points(inner)],
                });
            }
        }
        for piece in pieces {
            for s in outline::difference(&piece, &holes) {
                let mut pts = outline::points(&s.outer);
                for h in &s.holes {
                    pts.extend(outline::points(h));
                }
                let ids: Vec<u32> = pts
                    .iter()
                    .map(|&q| {
                        if let Some(id) = known.find(q, &b.pos) {
                            return id;
                        }
                        if let Some(id) = fresh.find(q, &b.pos) {
                            return id;
                        }
                        let id = b.vert(at(q, surface(q)));
                        fresh.add(q, id);
                        extra.push(id);
                        id
                    })
                    .collect();
                for t in outline::triangulate(&s) {
                    let t = [ids[t[0] as usize], ids[t[1] as usize], ids[t[2] as usize]];
                    if !degenerate(&t) {
                        sheet.push(t);
                    }
                }
            }
        }
        mend(&b.pos, &mut sheet, &extra, tol);
    }
    for t in sheet {
        b.tri(t[0], t[1], t[2]);
    }
    b.weld(0.0);
    b.orient_outward();
    if uv == UvMode::Local {
        let lo = o.iter().fold(o[0], |a, &p| a.min(p));
        let hi = o.iter().fold(o[0], |a, &p| a.max(p));
        let size = (hi - lo).max(DVec2::splat(1e-300));
        b.uv = b.pos.iter().map(|p| (p.truncate() - lo) / size).collect();
    }
    b.finish(30.0)
}

fn wall(contours: &[Vec<DVec2>]) -> f64 {
    let mut out = f64::INFINITY;
    for i in 0..contours.len() {
        for j in i + 1..contours.len() {
            out = out.min(outline::clearance(
                &from_points(&contours[i]),
                &from_points(&contours[j]),
            ));
        }
    }
    out
}

fn finite_or_zero(w: f64) -> f64 {
    if w.is_finite() { w.max(0.0) } else { 0.0 }
}

fn mend(pos: &[DVec3], tris: &mut Vec<[u32; 3]>, extra: &[u32], tol: f64) {
    if extra.is_empty() {
        return;
    }
    loop {
        let uses = outline::edge_uses(tris);
        let mut added = Vec::new();
        let mut changed = false;
        for t in tris.iter_mut() {
            let tri = *t;
            'edges: for e in 0..3 {
                let (a, b) = (tri[e], tri[(e + 1) % 3]);
                if uses[&edge(a, b)] != 1 {
                    continue;
                }
                let (pa, pb) = (pos[a as usize].truncate(), pos[b as usize].truncate());
                for &v in extra {
                    if v == a || v == b {
                        continue;
                    }
                    if outline::on_segment(pos[v as usize].truncate(), pa, pb, tol) {
                        let c = tri[(e + 2) % 3];
                        *t = [a, v, c];
                        added.push([v, b, c]);
                        changed = true;
                        break 'edges;
                    }
                }
            }
        }
        tris.extend(added);
        if !changed {
            break;
        }
    }
}
