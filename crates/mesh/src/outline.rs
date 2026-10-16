use crate::math::Real;
use glam::DVec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::overlay::{FloatOverlay, OverlayOptions};
use i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_overlay::mesh::float::style::{LineJoin, OutlineStyle};
use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outline(pub Vec<[f64; 2]>);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub outer: Outline,
    pub holes: Vec<Outline>,
}

impl Outline {
    pub fn area(&self) -> f64 {
        signed_area(&points(self))
    }

    pub fn perimeter(&self) -> f64 {
        let p = points(self);
        (0..p.len())
            .map(|i| p[i].distance(p[(i + 1) % p.len()]))
            .sum()
    }

    pub fn ccw(&self) -> Outline {
        if self.area() < 0.0 {
            Outline(self.0.iter().rev().copied().collect())
        } else {
            self.clone()
        }
    }

    pub fn map(&self, f: impl Fn([f64; 2]) -> [f64; 2]) -> Outline {
        Outline(self.0.iter().map(|&p| f(p)).collect())
    }

    pub fn bounds(&self) -> Option<[[f64; 2]; 2]> {
        let first = *self.0.first()?;
        Some(self.0.iter().fold([first, first], |[lo, hi], p| {
            [
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            ]
        }))
    }

    pub fn contains(&self, p: [f64; 2]) -> bool {
        inside(&points(self), DVec2::from(p))
    }
}

impl Shape {
    pub fn new(outer: Outline) -> Shape {
        Shape {
            outer,
            holes: Vec::new(),
        }
    }

    pub fn area(&self) -> f64 {
        self.outer.area().abs() - self.holes.iter().map(|h| h.area().abs()).sum::<f64>()
    }
}

pub fn rounded_rect(rect: [f64; 4], radius: f64, segments: u32) -> Outline {
    let [x0, y0, x1, y1] = rect;
    let r = radius.min((x1 - x0) * 0.5).min((y1 - y0) * 0.5);
    if r <= 0.0 || segments == 0 {
        return Outline(vec![[x1, y0], [x1, y1], [x0, y1], [x0, y0]]);
    }
    let mut out = Vec::new();
    for (cx, cy, start) in [
        (x1 - r, y0 + r, -90.0_f64),
        (x1 - r, y1 - r, 0.0),
        (x0 + r, y1 - r, 90.0),
        (x0 + r, y0 + r, 180.0),
    ] {
        for i in 0..=segments {
            let a = (start + 90.0 * i as f64 / segments as f64).to_radians();
            out.push([cx + r * a.cos_of(), cy + r * a.sin_of()]);
        }
    }
    cleaned(Outline(out))
}

pub fn fillet(corners: &[[f64; 3]], segments: u32) -> Outline {
    let n = corners.len();
    let seg = segments.max(1);
    let mut out = Vec::new();
    for i in 0..n {
        let [px, py, _] = corners[(i + n - 1) % n];
        let [vx, vy, r] = corners[i];
        let [nx, ny, _] = corners[(i + 1) % n];
        let (p, v, q) = (DVec2::new(px, py), DVec2::new(vx, vy), DVec2::new(nx, ny));
        let (l1, l2) = (p.distance(v), q.distance(v));
        if l1 <= 0.0 || l2 <= 0.0 {
            out.push([vx, vy]);
            continue;
        }
        let (d1, d2) = ((p - v) / l1, (q - v) / l2);
        let ang = d1.dot(d2).clamp(-1.0, 1.0).acos_of();
        if r <= 0.0 || !(1e-3..=PI - 1e-3).contains(&ang) {
            out.push([vx, vy]);
            continue;
        }
        let t = r / (ang * 0.5).tan_of();
        let (t1, t2) = (v + d1 * t, v + d2 * t);
        let bis = (d1 + d2).normalize();
        let c = v + bis * (r / (ang * 0.5).sin_of());
        let a1 = (t1.y - c.y).atan2_of(t1.x - c.x);
        let a2 = (t2.y - c.y).atan2_of(t2.x - c.x);
        let mut da = a2 - a1;
        while da > PI {
            da -= TAU;
        }
        while da < -PI {
            da += TAU;
        }
        for k in 0..=seg {
            let a = a1 + da * k as f64 / seg as f64;
            out.push([c.x + r * a.cos_of(), c.y + r * a.sin_of()]);
        }
    }
    cleaned(Outline(out))
}

pub fn circle(centre: [f64; 2], radius: f64, segments: u32) -> Outline {
    let n = segments.max(3);
    Outline(
        (0..n)
            .map(|i| {
                let a = TAU * i as f64 / n as f64;
                [
                    centre[0] + radius * a.cos_of(),
                    centre[1] + radius * a.sin_of(),
                ]
            })
            .collect(),
    )
}

pub fn superellipse(centre: [f64; 2], half: [f64; 2], exponent: f64, segments: u32) -> Outline {
    let quarter = (segments / 4).max(1);
    let power = 2.0 / exponent;
    let mut out = Vec::with_capacity(4 * quarter as usize);
    for side in 0..4 {
        for k in 0..quarter {
            let (x, y) = if k == 0 {
                (1.0, 0.0)
            } else {
                let along = |n: u32| (0.5 * PI * f64::from(n) / f64::from(quarter)).cos_of();
                (along(k).powf_of(power), along(quarter - k).powf_of(power))
            };
            let [u, v] = match side {
                0 => [x, y],
                1 => [-y, x],
                2 => [-x, -y],
                _ => [y, -x],
            };
            out.push([centre[0] + half[0] * u, centre[1] + half[1] * v]);
        }
    }
    Outline(out)
}

pub fn turned(rect: [f64; 4], radius: f64, degrees: f64, segments: u32) -> Outline {
    let [x0, y0, x1, y1] = rect;
    let (cx, cy) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    let (s, c) = degrees.to_radians().sin_cos_of();
    rounded_rect(rect, radius, segments).map(|[x, y]| {
        [
            cx + (x - cx) * c - (y - cy) * s,
            cy + (x - cx) * s + (y - cy) * c,
        ]
    })
}

pub fn union(outlines: &[Outline]) -> Vec<Shape> {
    let subj: Vec<Vec<[f64; 2]>> = outlines.iter().map(|o| o.ccw().0).collect();
    overlay(&subj, &[], OverlayRule::Subject)
}

pub fn merge(shapes: &[Shape]) -> Vec<Shape> {
    let subj: Vec<Vec<[f64; 2]>> = shapes.iter().flat_map(contours).collect();
    overlay(&subj, &[], OverlayRule::Subject)
}

pub fn difference(a: &Shape, b: &[Outline]) -> Vec<Shape> {
    let clip: Vec<Vec<[f64; 2]>> = b.iter().map(|o| o.ccw().0).collect();
    overlay(&contours(a), &clip, OverlayRule::Difference)
}

pub fn even_odd(contours: &[Outline]) -> Vec<Shape> {
    let subj: Vec<Vec<[f64; 2]>> = contours.iter().map(|o| o.ccw().0).collect();
    filled(&subj, &[], OverlayRule::Subject, FillRule::EvenOdd)
}

pub fn clearance(a: &Outline, b: &Outline) -> f64 {
    let (p, q) = (points(a), points(b));
    if p.is_empty() || q.is_empty() {
        return f64::INFINITY;
    }
    let (n, m) = (p.len(), q.len());
    let mut best = f64::INFINITY;
    for i in 0..n {
        let (a0, a1) = (p[i], p[(i + 1) % n]);
        for j in 0..m {
            let (b0, b1) = (q[j], q[(j + 1) % m]);
            if crosses(a0, a1, b0, b1) {
                return 0.0;
            }
            best = best
                .min(segment_distance(a0, b0, b1))
                .min(segment_distance(b0, a0, a1));
        }
    }
    best
}

pub fn pinch(o: &Outline, tol: f64) -> Option<[f64; 2]> {
    let p = points(&cleaned(o.clone()));
    let n = p.len();
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if p[i].distance(p[j]) <= tol {
                return Some(p[i].to_array());
            }
        }
    }
    None
}

pub fn intersection(a: &Shape, b: &Shape) -> Vec<Shape> {
    overlay(&contours(a), &contours(b), OverlayRule::Intersect)
}

pub fn offset(shape: &Shape, distance: f64) -> Vec<Shape> {
    if distance == 0.0 {
        return overlay(&contours(shape), &[], OverlayRule::Subject);
    }
    let style = OutlineStyle::new(distance).line_join(LineJoin::Round(PI / 24.0));
    let source = vec![contours(shape)];
    shapes(source.outline(&style))
}

pub fn triangulate(shape: &Shape) -> Vec<[u32; 3]> {
    let mut pts: Vec<DVec2> = shape.outer.0.iter().map(|&p| DVec2::from(p)).collect();
    let outer: Vec<u32> = (0..pts.len() as u32).collect();
    let mut holes = Vec::new();
    for h in &shape.holes {
        let start = pts.len() as u32;
        pts.extend(h.0.iter().map(|&p| DVec2::from(p)));
        holes.push((start..pts.len() as u32).collect::<Vec<u32>>());
    }
    let scale = extent(&pts);
    let mut ear = Earcut {
        pts: &pts,
        eps: scale * scale * 1e-12,
        nodes: Vec::new(),
        tris: Vec::new(),
    };
    if let Some(mut start) = ear.linked(&outer, true)
        && ear.nodes[start].next != ear.nodes[start].prev
    {
        if !holes.is_empty() {
            start = ear.eliminate_holes(&holes, start);
        }
        ear.cut(Some(start), 0);
    }
    let mut tris = ear.tris;
    restore(&pts, &mut tris, scale * 1e-9);
    tris
}

pub(crate) fn points(o: &Outline) -> Vec<DVec2> {
    o.0.iter().map(|&p| DVec2::from(p)).collect()
}

pub(crate) fn from_points(p: &[DVec2]) -> Outline {
    Outline(p.iter().map(|q| q.to_array()).collect())
}

pub(crate) fn ring(o: &Outline) -> Vec<DVec2> {
    let p = points(&cleaned(o.clone()));
    if signed_area(&p) < 0.0 {
        p.into_iter().rev().collect()
    } else {
        p
    }
}

pub(crate) fn cleaned(o: Outline) -> Outline {
    let pts = points(&o);
    let tol = extent(&pts) * 1e-12;
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(pts.len());
    for p in pts {
        if out.last().is_none_or(|&q| DVec2::from(q).distance(p) > tol) {
            out.push(p.into());
        }
    }
    while out.len() > 1 && DVec2::from(out[0]).distance(DVec2::from(out[out.len() - 1])) <= tol {
        out.pop();
    }
    Outline(out)
}

pub(crate) fn signed_area(p: &[DVec2]) -> f64 {
    (0..p.len())
        .map(|i| p[i].perp_dot(p[(i + 1) % p.len()]))
        .sum::<f64>()
        * 0.5
}

pub(crate) fn extent(p: &[DVec2]) -> f64 {
    let Some(&first) = p.first() else {
        return 1.0;
    };
    let (lo, hi) = p
        .iter()
        .fold((first, first), |(lo, hi), &q| (lo.min(q), hi.max(q)));
    (hi - lo).max_element().max(1e-300)
}

pub(crate) fn miters(p: &[DVec2]) -> Vec<DVec2> {
    let n = p.len();
    (0..n)
        .map(|i| {
            let a = p[(i + n - 1) % n];
            let b = p[i];
            let c = p[(i + 1) % n];
            let e1 = edge_normal(a, b);
            let e2 = edge_normal(b, c);
            let m = (e1 + e2).normalize_or_zero();
            if m == DVec2::ZERO {
                return e1;
            }
            m / m.dot(e1).max(0.25)
        })
        .collect()
}

pub(crate) fn shift(p: &[DVec2], miter: &[DVec2], d: impl Fn(usize) -> f64) -> Vec<DVec2> {
    p.iter()
        .zip(miter)
        .enumerate()
        .map(|(i, (&q, &m))| q + m * d(i))
        .collect()
}

pub(crate) fn reach(p: &[DVec2], miter: &[DVec2], sign: f64) -> f64 {
    let n = p.len();
    (0..n)
        .filter_map(|i| {
            let j = (i + 1) % n;
            let e = p[j] - p[i];
            let shrink = -sign * (miter[j] - miter[i]).dot(e);
            (shrink > 0.0).then(|| e.length_squared() / shrink)
        })
        .fold(f64::INFINITY, f64::min)
}

pub(crate) fn distance(q: DVec2, p: &[DVec2]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| segment_distance(q, p[i], p[(i + 1) % n]))
        .fold(f64::INFINITY, f64::min)
}

pub(crate) fn gap(a: DVec2, b: DVec2, p: &[DVec2]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (c, d) = (p[i], p[(i + 1) % n]);
            if crosses(a, b, c, d) {
                0.0
            } else {
                segment_distance(a, c, d)
                    .min(segment_distance(b, c, d))
                    .min(segment_distance(c, a, b))
                    .min(segment_distance(d, a, b))
            }
        })
        .fold(f64::INFINITY, f64::min)
}

fn crosses(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let d1 = (b - a).perp_dot(c - a);
    let d2 = (b - a).perp_dot(d - a);
    let d3 = (d - c).perp_dot(a - c);
    let d4 = (d - c).perp_dot(b - c);
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

pub(crate) fn segment_distance(q: DVec2, a: DVec2, b: DVec2) -> f64 {
    let e = b - a;
    let len = e.length_squared();
    let t = if len > 0.0 {
        ((q - a).dot(e) / len).clamp(0.0, 1.0)
    } else {
        0.0
    };
    q.distance(a + e * t)
}

pub(crate) fn inside(p: &[DVec2], q: DVec2) -> bool {
    let n = p.len();
    let mut c = false;
    for i in 0..n {
        let (a, b) = (p[i], p[(i + 1) % n]);
        if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
    }
    c
}

fn edge_normal(a: DVec2, b: DVec2) -> DVec2 {
    let e = (b - a).normalize_or_zero();
    DVec2::new(e.y, -e.x)
}

fn contours(shape: &Shape) -> Vec<Vec<[f64; 2]>> {
    let mut out = vec![shape.outer.ccw().0];
    for h in &shape.holes {
        let mut c = h.ccw().0;
        c.reverse();
        out.push(c);
    }
    out
}

fn overlay(subj: &[Vec<[f64; 2]>], clip: &[Vec<[f64; 2]>], rule: OverlayRule) -> Vec<Shape> {
    filled(subj, clip, rule, FillRule::NonZero)
}

fn filled(
    subj: &[Vec<[f64; 2]>],
    clip: &[Vec<[f64; 2]>],
    rule: OverlayRule,
    fill: FillRule,
) -> Vec<Shape> {
    let mut options = OverlayOptions::<f64, i32>::default();
    options.preserve_input_collinear = true;
    options.preserve_output_collinear = true;
    let subj = subj.to_vec();
    let clip = clip.to_vec();
    let mut ov = FloatOverlay::<[f64; 2], i32>::from_subj_and_clip_custom(
        &subj,
        &clip,
        options,
        Default::default(),
    );
    shapes(ov.overlay(rule, fill))
}

fn shapes(raw: Vec<Vec<Vec<[f64; 2]>>>) -> Vec<Shape> {
    raw.into_iter()
        .filter_map(|mut s| {
            if s.is_empty() {
                return None;
            }
            let outer = Outline(s.remove(0)).ccw();
            let holes = s
                .into_iter()
                .map(|h| {
                    let mut c = Outline(h).ccw().0;
                    c.reverse();
                    Outline(c)
                })
                .collect();
            Some(Shape { outer, holes })
        })
        .collect()
}

fn restore(pts: &[DVec2], tris: &mut Vec<[u32; 3]>, tol: f64) {
    let mut used = vec![false; pts.len()];
    for t in tris.iter() {
        for &i in t {
            used[i as usize] = true;
        }
    }
    let lost: Vec<u32> = (0..pts.len() as u32)
        .filter(|&i| !used[i as usize])
        .filter(|&i| !(0..pts.len()).any(|j| used[j] && pts[j].distance(pts[i as usize]) <= tol))
        .collect();
    if lost.is_empty() {
        return;
    }
    let mut pending = lost;
    loop {
        let mut uses = edge_uses(tris);
        let mut next = Vec::new();
        let mut progress = false;
        for v in pending {
            let q = pts[v as usize];
            let mut hit = None;
            'tris: for (ti, t) in tris.iter().enumerate() {
                for e in 0..3 {
                    let (a, b) = (t[e], t[(e + 1) % 3]);
                    if uses[&edge(a, b)] != 1 {
                        continue;
                    }
                    if on_segment(q, pts[a as usize], pts[b as usize], tol) {
                        hit = Some((ti, e));
                        break 'tris;
                    }
                }
            }
            match hit {
                Some((ti, e)) => {
                    let t = tris[ti];
                    let (a, b, c) = (t[e], t[(e + 1) % 3], t[(e + 2) % 3]);
                    tris[ti] = [a, v, c];
                    tris.push([v, b, c]);
                    *uses.entry(edge(a, b)).or_default() += 1;
                    progress = true;
                }
                None => next.push(v),
            }
        }
        if next.is_empty() || !progress {
            break;
        }
        pending = next;
    }
}

pub(crate) fn edge(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

pub(crate) fn edge_uses(tris: &[[u32; 3]]) -> BTreeMap<(u32, u32), u32> {
    let mut uses = BTreeMap::new();
    for t in tris {
        for e in 0..3 {
            *uses.entry(edge(t[e], t[(e + 1) % 3])).or_default() += 1;
        }
    }
    uses
}

pub(crate) fn on_segment(q: DVec2, a: DVec2, b: DVec2, tol: f64) -> bool {
    let e = b - a;
    let len = e.length_squared();
    if len <= 0.0 {
        return false;
    }
    let t = (q - a).dot(e) / len;
    let margin = tol / len.sqrt();
    t > margin && t < 1.0 - margin && q.distance(a + e * t) <= tol
}

#[derive(Clone, Copy)]
struct Node {
    i: u32,
    p: DVec2,
    prev: usize,
    next: usize,
}

struct Earcut<'a> {
    pts: &'a [DVec2],
    eps: f64,
    nodes: Vec<Node>,
    tris: Vec<[u32; 3]>,
}

fn area(p: DVec2, q: DVec2, r: DVec2) -> f64 {
    (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y)
}

fn in_triangle(a: DVec2, b: DVec2, c: DVec2, p: DVec2) -> bool {
    (c.x - p.x) * (a.y - p.y) >= (a.x - p.x) * (c.y - p.y)
        && (a.x - p.x) * (b.y - p.y) >= (b.x - p.x) * (a.y - p.y)
        && (b.x - p.x) * (c.y - p.y) >= (c.x - p.x) * (b.y - p.y)
}

fn sign(v: f64) -> i32 {
    if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        0
    }
}

fn on_box(p: DVec2, q: DVec2, r: DVec2) -> bool {
    q.x <= p.x.max(r.x) && q.x >= p.x.min(r.x) && q.y <= p.y.max(r.y) && q.y >= p.y.min(r.y)
}

fn intersects(p1: DVec2, q1: DVec2, p2: DVec2, q2: DVec2) -> bool {
    let o1 = sign(area(p1, q1, p2));
    let o2 = sign(area(p1, q1, q2));
    let o3 = sign(area(p2, q2, p1));
    let o4 = sign(area(p2, q2, q1));
    (o1 != o2 && o3 != o4)
        || (o1 == 0 && on_box(p1, p2, q1))
        || (o2 == 0 && on_box(p1, q2, q1))
        || (o3 == 0 && on_box(p2, p1, q2))
        || (o4 == 0 && on_box(p2, q1, q2))
}

impl Earcut<'_> {
    fn p(&self, n: usize) -> DVec2 {
        self.nodes[n].p
    }

    fn next(&self, n: usize) -> usize {
        self.nodes[n].next
    }

    fn prev(&self, n: usize) -> usize {
        self.nodes[n].prev
    }

    fn insert(&mut self, i: u32, last: Option<usize>) -> usize {
        let id = self.nodes.len();
        let p = self.pts[i as usize];
        match last {
            None => self.nodes.push(Node {
                i,
                p,
                prev: id,
                next: id,
            }),
            Some(l) => {
                let next = self.nodes[l].next;
                self.nodes.push(Node {
                    i,
                    p,
                    prev: l,
                    next,
                });
                self.nodes[next].prev = id;
                self.nodes[l].next = id;
            }
        }
        id
    }

    fn remove(&mut self, n: usize) {
        let Node { prev, next, .. } = self.nodes[n];
        self.nodes[next].prev = prev;
        self.nodes[prev].next = next;
    }

    fn linked(&mut self, idx: &[u32], ccw: bool) -> Option<usize> {
        let ring: Vec<DVec2> = idx.iter().map(|&i| self.pts[i as usize]).collect();
        let forward = (signed_area(&ring) > 0.0) == ccw;
        let mut last = None;
        if forward {
            for &i in idx {
                last = Some(self.insert(i, last));
            }
        } else {
            for &i in idx.iter().rev() {
                last = Some(self.insert(i, last));
            }
        }
        let l = last?;
        if self.p(l) == self.p(self.next(l)) {
            let n = self.next(l);
            self.remove(l);
            return Some(n);
        }
        Some(l)
    }

    fn filter(&mut self, start: usize, end: Option<usize>) -> usize {
        let mut end = end.unwrap_or(start);
        let mut p = start;
        loop {
            let again;
            let (pp, pn) = (self.prev(p), self.next(p));
            if self.p(p) == self.p(pn) || area(self.p(pp), self.p(p), self.p(pn)).abs() <= self.eps
            {
                self.remove(p);
                p = pp;
                end = pp;
                if p == self.next(p) {
                    break;
                }
                again = true;
            } else {
                p = pn;
                again = false;
            }
            if !again && p == end {
                break;
            }
        }
        end
    }

    fn cut(&mut self, ear: Option<usize>, pass: u32) {
        let Some(mut ear) = ear else {
            return;
        };
        let mut stop = ear;
        while self.prev(ear) != self.next(ear) {
            let (prev, next) = (self.prev(ear), self.next(ear));
            if self.is_ear(ear) {
                self.tris
                    .push([self.nodes[prev].i, self.nodes[ear].i, self.nodes[next].i]);
                self.remove(ear);
                ear = self.next(next);
                stop = ear;
                continue;
            }
            ear = next;
            if ear == stop {
                match pass {
                    0 => {
                        let e = self.filter(ear, None);
                        self.cut(Some(e), 1);
                    }
                    1 => {
                        let e = self.filter(ear, None);
                        let e = self.cure(e);
                        self.cut(Some(e), 2);
                    }
                    _ => self.split(ear),
                }
                break;
            }
        }
    }

    fn is_ear(&self, ear: usize) -> bool {
        let (a, b, c) = (self.prev(ear), ear, self.next(ear));
        let (pa, pb, pc) = (self.p(a), self.p(b), self.p(c));
        if area(pa, pb, pc) >= -self.eps {
            return false;
        }
        let lo = pa.min(pb).min(pc);
        let hi = pa.max(pb).max(pc);
        let mut p = self.next(c);
        while p != a {
            let q = self.p(p);
            if q.x >= lo.x
                && q.x <= hi.x
                && q.y >= lo.y
                && q.y <= hi.y
                && q != pa
                && in_triangle(pa, pb, pc, q)
                && area(self.p(self.prev(p)), q, self.p(self.next(p))) >= -self.eps
            {
                return false;
            }
            p = self.next(p);
        }
        true
    }

    fn cure(&mut self, start: usize) -> usize {
        let mut start = start;
        let mut p = start;
        loop {
            let a = self.prev(p);
            let b = self.next(self.next(p));
            if self.p(a) != self.p(b)
                && intersects(self.p(a), self.p(p), self.p(self.next(p)), self.p(b))
                && self.locally_inside(a, b)
                && self.locally_inside(b, a)
            {
                self.tris
                    .push([self.nodes[a].i, self.nodes[p].i, self.nodes[b].i]);
                let pn = self.next(p);
                self.remove(p);
                self.remove(pn);
                p = b;
                start = b;
            }
            p = self.next(p);
            if p == start {
                break;
            }
        }
        self.filter(p, None)
    }

    fn split(&mut self, start: usize) {
        let mut a = start;
        loop {
            let mut b = self.next(self.next(a));
            while b != self.prev(a) {
                if self.nodes[a].i != self.nodes[b].i && self.valid_diagonal(a, b) {
                    let c = self.split_polygon(a, b);
                    let an = self.next(a);
                    let a2 = self.filter(a, Some(an));
                    let cn = self.next(c);
                    let c2 = self.filter(c, Some(cn));
                    self.cut(Some(a2), 0);
                    self.cut(Some(c2), 0);
                    return;
                }
                b = self.next(b);
            }
            a = self.next(a);
            if a == start {
                return;
            }
        }
    }

    fn valid_diagonal(&self, a: usize, b: usize) -> bool {
        let (pa, pb) = (self.p(a), self.p(b));
        self.nodes[self.next(a)].i != self.nodes[b].i
            && self.nodes[self.prev(a)].i != self.nodes[b].i
            && !self.intersects_polygon(a, b)
            && ((self.locally_inside(a, b)
                && self.locally_inside(b, a)
                && self.middle_inside(a, b)
                && (area(self.p(self.prev(a)), pa, self.p(self.prev(b))) != 0.0
                    || area(pa, self.p(self.prev(b)), pb) != 0.0))
                || (pa == pb
                    && area(self.p(self.prev(a)), pa, self.p(self.next(a))) > 0.0
                    && area(self.p(self.prev(b)), pb, self.p(self.next(b))) > 0.0))
    }

    fn intersects_polygon(&self, a: usize, b: usize) -> bool {
        let (ia, ib) = (self.nodes[a].i, self.nodes[b].i);
        let mut p = a;
        loop {
            let n = self.next(p);
            let (ip, inn) = (self.nodes[p].i, self.nodes[n].i);
            if ip != ia
                && inn != ia
                && ip != ib
                && inn != ib
                && intersects(self.p(p), self.p(n), self.p(a), self.p(b))
            {
                return true;
            }
            p = n;
            if p == a {
                return false;
            }
        }
    }

    fn locally_inside(&self, a: usize, b: usize) -> bool {
        let (pa, pb) = (self.p(a), self.p(b));
        let (prev, next) = (self.p(self.prev(a)), self.p(self.next(a)));
        if area(prev, pa, next) < 0.0 {
            area(pa, pb, next) >= 0.0 && area(pa, prev, pb) >= 0.0
        } else {
            area(pa, pb, prev) < 0.0 || area(pa, next, pb) < 0.0
        }
    }

    fn middle_inside(&self, a: usize, b: usize) -> bool {
        let m = (self.p(a) + self.p(b)) * 0.5;
        let mut p = a;
        let mut inside = false;
        loop {
            let (q, r) = (self.p(p), self.p(self.next(p)));
            if (q.y > m.y) != (r.y > m.y)
                && r.y != q.y
                && m.x < (r.x - q.x) * (m.y - q.y) / (r.y - q.y) + q.x
            {
                inside = !inside;
            }
            p = self.next(p);
            if p == a {
                return inside;
            }
        }
    }

    fn split_polygon(&mut self, a: usize, b: usize) -> usize {
        let a2 = self.nodes.len();
        self.nodes.push(self.nodes[a]);
        let b2 = self.nodes.len();
        self.nodes.push(self.nodes[b]);
        let an = self.next(a);
        let bp = self.prev(b);
        self.nodes[a].next = b;
        self.nodes[b].prev = a;
        self.nodes[a2].next = an;
        self.nodes[an].prev = a2;
        self.nodes[b2].next = a2;
        self.nodes[a2].prev = b2;
        self.nodes[bp].next = b2;
        self.nodes[b2].prev = bp;
        b2
    }

    fn eliminate_holes(&mut self, holes: &[Vec<u32>], outer: usize) -> usize {
        let mut queue = Vec::new();
        for h in holes {
            if let Some(list) = self.linked(h, false) {
                queue.push(self.leftmost(list));
            }
        }
        queue.sort_by(|&a, &b| {
            let (pa, pb) = (self.p(a), self.p(b));
            pa.x.total_cmp(&pb.x)
                .then(pa.y.total_cmp(&pb.y))
                .then_with(|| {
                    let sa = self.slope(a);
                    let sb = self.slope(b);
                    sa.total_cmp(&sb)
                })
        });
        let mut outer = outer;
        for h in queue {
            outer = self.eliminate_hole(h, outer);
        }
        outer
    }

    fn slope(&self, n: usize) -> f64 {
        let (p, q) = (self.p(n), self.p(self.next(n)));
        (q.y - p.y) / (q.x - p.x)
    }

    fn leftmost(&self, start: usize) -> usize {
        let mut p = start;
        let mut best = start;
        loop {
            let (q, b) = (self.p(p), self.p(best));
            if q.x < b.x || (q.x == b.x && q.y < b.y) {
                best = p;
            }
            p = self.next(p);
            if p == start {
                return best;
            }
        }
    }

    fn eliminate_hole(&mut self, hole: usize, outer: usize) -> usize {
        let Some(bridge) = self.bridge(hole, outer) else {
            return outer;
        };
        let reverse = self.split_polygon(bridge, hole);
        let rn = self.next(reverse);
        self.filter(reverse, Some(rn));
        let bn = self.next(bridge);
        self.filter(bridge, Some(bn))
    }

    fn bridge(&self, hole: usize, outer: usize) -> Option<usize> {
        let h = self.p(hole);
        let mut p = outer;
        let mut qx = f64::NEG_INFINITY;
        let mut m = None;
        if h == self.p(p) {
            return Some(p);
        }
        loop {
            let n = self.next(p);
            let (pp, pn) = (self.p(p), self.p(n));
            if h == pn {
                return Some(n);
            }
            if h.y <= pp.y && h.y >= pn.y && pn.y != pp.y {
                let x = pp.x + (h.y - pp.y) * (pn.x - pp.x) / (pn.y - pp.y);
                if x <= h.x && x > qx {
                    qx = x;
                    let cand = if pp.x < pn.x { p } else { n };
                    m = Some(cand);
                    if x == h.x {
                        return m;
                    }
                }
            }
            p = n;
            if p == outer {
                break;
            }
        }
        let mut m = m?;
        let stop = m;
        let pm = self.p(m);
        let mut tan_min = f64::INFINITY;
        let mut p = m;
        loop {
            let q = self.p(p);
            let (a, c) = if h.y < pm.y {
                (DVec2::new(h.x, h.y), DVec2::new(qx, h.y))
            } else {
                (DVec2::new(qx, h.y), DVec2::new(h.x, h.y))
            };
            if h.x >= q.x && q.x >= pm.x && h.x != q.x && in_triangle(a, pm, c, q) {
                let tan = (h.y - q.y).abs() / (h.x - q.x);
                let pmx = self.p(m);
                if self.locally_inside(p, hole)
                    && (tan < tan_min
                        || (tan == tan_min
                            && (q.x > pmx.x || (q.x == pmx.x && self.sector_contains(m, p)))))
                {
                    m = p;
                    tan_min = tan;
                }
            }
            p = self.next(p);
            if p == stop {
                break;
            }
        }
        Some(m)
    }

    fn sector_contains(&self, m: usize, p: usize) -> bool {
        area(self.p(self.prev(m)), self.p(m), self.p(self.prev(p))) < 0.0
            && area(self.p(self.next(p)), self.p(m), self.p(self.next(m))) < 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri_area(pts: &[DVec2], t: &[u32; 3]) -> f64 {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        (b - a).perp_dot(c - a) * 0.5
    }

    fn all_points(s: &Shape) -> Vec<DVec2> {
        let mut p = points(&s.outer);
        for h in &s.holes {
            p.extend(points(h));
        }
        p
    }

    fn check(s: &Shape) -> f64 {
        let tris = triangulate(s);
        let pts = all_points(s);
        let mut sum = 0.0;
        for t in &tris {
            let a = tri_area(&pts, t);
            assert!(a > 1e-14, "degenerate triangle {a}");
            sum += a;
        }
        let mut used = vec![false; pts.len()];
        for t in &tris {
            for &i in t {
                used[i as usize] = true;
            }
        }
        assert!(used.iter().all(|&u| u), "unused vertex");
        assert!((sum - s.area()).abs() < 1e-9 * s.area().abs().max(1.0));
        sum
    }

    #[test]
    fn rounded_rect_matches_deck_order() {
        let o = rounded_rect([0.0, 0.0, 10.0, 6.0], 2.0, 12);
        assert_eq!(o.0.len(), 52);
        assert!((o.0[0][0] - 8.0).abs() < 1e-12 && (o.0[0][1] - 0.0).abs() < 1e-12);
        let exact = 60.0 - (4.0 - PI) * 4.0;
        assert!((o.area() - exact).abs() / exact < 2e-3);
        assert!(o.area() > 0.0);
    }

    #[test]
    fn square_rect_and_circle() {
        assert_eq!(rounded_rect([0.0, 0.0, 2.0, 1.0], 0.0, 12).0.len(), 4);
        let c = circle([1.0, 2.0], 3.0, 64);
        assert_eq!(c.0.len(), 64);
        let exact = 0.5 * 64.0 * 9.0 * (TAU / 64.0).sin_of();
        assert!((c.area() - exact).abs() < 1e-9);
    }

    #[test]
    fn fillet_rounds_each_corner() {
        let o = fillet(
            &[
                [0.0, 0.0, 1.0],
                [10.0, 0.0, 1.0],
                [10.0, 10.0, 0.0],
                [0.0, 10.0, 2.0],
            ],
            12,
        );
        assert_eq!(o.0.len(), 13 * 3 + 1);
        let exact = 100.0 - (4.0 - PI) * 0.25 * (1.0 + 1.0 + 4.0);
        assert!((o.area().abs() - exact).abs() < 0.1);
    }

    #[test]
    fn turned_keeps_area() {
        let a = rounded_rect([0.0, 0.0, 4.0, 2.0], 0.5, 6);
        let b = turned([0.0, 0.0, 4.0, 2.0], 0.5, 17.0, 6);
        assert!((a.area() - b.area()).abs() < 1e-12);
        assert_ne!(a, b);
        let quarter = turned([0.0, 0.0, 4.0, 2.0], 0.0, 90.0, 0);
        let [x, y] = quarter.0[0];
        assert_eq!(rounded_rect([0.0, 0.0, 4.0, 2.0], 0.0, 0).0[0], [4.0, 0.0]);
        assert!(
            (x - 3.0).abs() < 1e-12 && (y - 3.0).abs() < 1e-12,
            "a positive turn is counter-clockwise"
        );
    }

    #[test]
    fn booleans_match_hand_areas() {
        let a = Outline(vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]]);
        let b = Outline(vec![[2.0, 2.0], [6.0, 2.0], [6.0, 6.0], [2.0, 6.0]]);
        let u = union(&[a.clone(), b.clone()]);
        assert_eq!(u.len(), 1);
        assert!((u[0].area() - 28.0).abs() < 1e-6);
        let d = difference(&Shape::new(a.clone()), std::slice::from_ref(&b));
        assert!((d.iter().map(Shape::area).sum::<f64>() - 12.0).abs() < 1e-6);
        let hole = Outline(vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]]);
        let ring = difference(&Shape::new(a.clone()), &[hole]);
        assert_eq!(ring.len(), 1);
        assert_eq!(ring[0].holes.len(), 1);
        assert!((ring[0].area() - 12.0).abs() < 1e-6);
        let i = intersection(&Shape::new(a), &Shape::new(b));
        assert!((i.iter().map(Shape::area).sum::<f64>() - 4.0).abs() < 1e-6);
        let apart = union(&[
            Outline(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
            Outline(vec![[3.0, 0.0], [4.0, 0.0], [4.0, 1.0], [3.0, 1.0]]),
        ]);
        assert_eq!(apart.len(), 2);
    }

    #[test]
    fn offsets_match_hand_areas() {
        let sq = Shape::new(Outline(vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
        ]));
        let grown = offset(&sq, 1.0);
        assert_eq!(grown.len(), 1);
        let exact = 100.0 + 40.0 + PI;
        assert!(
            (grown[0].area() - exact).abs() / exact < 2e-3,
            "{}",
            grown[0].area()
        );
        let shrunk = offset(&sq, -1.0);
        assert_eq!(shrunk.len(), 1);
        assert!(
            (shrunk[0].area() - 64.0).abs() < 1e-3,
            "{}",
            shrunk[0].area()
        );
        let disc = Shape::new(circle([0.0, 0.0], 5.0, 256));
        let g = offset(&disc, 2.0);
        let exact = PI * 49.0;
        assert!((g[0].area() - exact).abs() / exact < 2e-3);
    }

    #[test]
    fn triangulates_concave_and_holed_shapes() {
        let l = Shape::new(Outline(vec![
            [0.0, 0.0],
            [4.0, 0.0],
            [4.0, 1.0],
            [1.0, 1.0],
            [1.0, 4.0],
            [0.0, 4.0],
        ]));
        assert!((check(&l) - 7.0).abs() < 1e-12);
        let mut collinear = vec![];
        for i in 0..=10 {
            collinear.push([i as f64, 0.0]);
        }
        collinear.push([10.0, 3.0]);
        for i in (0..10).rev() {
            collinear.push([i as f64, 3.0]);
        }
        check(&Shape::new(Outline(collinear)));
        let frame = Shape {
            outer: rounded_rect([0.0, 0.0, 20.0, 10.0], 2.0, 12),
            holes: vec![
                circle([5.0, 5.0], 2.0, 32),
                rounded_rect([10.0, 2.0, 18.0, 8.0], 1.0, 6),
            ],
        };
        check(&frame);
        let cw = Shape::new(Outline(l.outer.0.iter().rev().copied().collect()));
        let tris = triangulate(&cw);
        let pts = points(&cw.outer);
        assert!(tris.iter().all(|t| tri_area(&pts, t) > 0.0));
    }
}
