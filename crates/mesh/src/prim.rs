use crate::build::Build;
use crate::math::Real;
use crate::outline::{Outline, cleaned, points, signed_area};
use crate::{Bend, Mesh, Sheet};
use glam::{DQuat, DVec2, DVec3};
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

pub fn surface(cells: [u32; 2], at: impl Fn(f64, f64) -> [f64; 3]) -> Mesh {
    let (nu, nv) = (cells[0].max(1), cells[1].max(1));
    let mut b = Build::default();
    for j in 0..=nv {
        for i in 0..=nu {
            let (u, v) = (i as f64 / nu as f64, j as f64 / nv as f64);
            b.vert_uv(DVec3::from(at(u, v)), DVec2::new(u, v));
        }
    }
    let row = |j: u32| (j * (nu + 1)..(j + 1) * (nu + 1)).collect::<Vec<u32>>();
    for j in 0..nv {
        b.band(&row(j), &row(j + 1), false);
    }
    b.finish(180.0)
}

pub fn sheet(s: &Sheet) -> Mesh {
    let [sx, sz] = s.size;
    let (nu, nv) = (s.cells[0].max(1), s.cells[1].max(1));
    let drum = |r: f64, d: f64| r - (r * r - d * d).max(0.0).sqrt();
    surface([nu, nv], |u, v| {
        let x = (u - 0.5) * sx;
        let z = (0.5 - v) * sz;
        let mut p = match s.bend {
            Some(Bend::Wrap(r)) if r != 0.0 && r.is_finite() => {
                let t = z / r;
                [x, -r * (1.0 - t.cos_of()), r * t.sin_of()]
            }
            Some(Bend::Lift(r)) if r > 0.0 && r.is_finite() => {
                [x, drum(r, sz * 0.5) - drum(r, z), z]
            }
            _ => [x, 0.0, z],
        };
        if let Some([width, depth]) = s.curl
            && width > 0.0
        {
            let edge = v.min(1.0 - v) * sz;
            let theta = (1.0 - edge / width).max(0.0) * FRAC_PI_2;
            p[1] -= depth * (1.0 - theta.cos_of());
        }
        if let Some([from, depth]) = s.cup
            && from < 1.0
        {
            let side = (((2.0 * u - 1.0).abs() - from) / (1.0 - from)).max(0.0);
            p[1] -= depth * side * side;
        }
        let (i, j) = (
            (u * nu as f64).round() as u32,
            (v * nv as f64).round() as usize,
        );
        if !s.jag.is_empty() && (i == 0 || i == nu) {
            let side = if i == 0 { -1.0 } else { 1.0 };
            let turn = if j % 2 == 1 { 1.0 } else { -1.0 };
            p[0] += side * turn * s.jag[j % s.jag.len()];
        }
        p
    })
}

pub fn bead(outline: &Outline, radius: f64, depth: f64, offset: f64, ring: u32) -> Mesh {
    let pts = points(&cleaned(outline.clone()));
    let n = pts.len();
    if n < 3 {
        return Mesh::default();
    }
    let turn = if signed_area(&pts) > 0.0 { 1.0 } else { -1.0 };
    let sides = ring.max(3) as usize;
    let mut b = Build::default();
    let rings: Vec<Vec<u32>> = (0..n)
        .map(|i| {
            let t = pts[(i + 1) % n] - pts[(i + n - 1) % n];
            let t = if t.length() > 0.0 { t / t.length() } else { t };
            let normal = DVec2::new(t.y, -t.x) * turn;
            let c = pts[i] + normal * offset;
            (0..sides)
                .map(|k| {
                    let (sa, ca) = (TAU * k as f64 / sides as f64).sin_cos_of();
                    b.vert(DVec3::new(
                        c.x + radius * ca * normal.x,
                        c.y + radius * ca * normal.y,
                        -depth + radius * sa,
                    ))
                })
                .collect()
        })
        .collect();
    for i in 0..n {
        b.band(&rings[i], &rings[(i + 1) % n], true);
    }
    b.orient_outward();
    b.finish(60.0)
}

pub fn disc(radius: f64, segments: u32) -> Mesh {
    let n = segments.max(3);
    let mut b = Build::default();
    let ring: Vec<u32> = (0..n)
        .map(|i| {
            let (s, co) = (TAU * i as f64 / n as f64).sin_cos_of();
            b.vert_uv(
                DVec3::new(radius * s, 0.0, radius * co),
                DVec2::new(0.5 + 0.5 * s, 0.5 - 0.5 * co),
            )
        })
        .collect();
    b.fan(&ring, false);
    b.finish(180.0)
}

fn revolve(profile: &[(f64, f64, f64)], segments: u32) -> Build {
    let n = segments.max(3);
    let mut b = Build::default();
    let rows: Vec<Vec<u32>> = profile
        .iter()
        .map(|&(r, y, v)| {
            (0..=n)
                .map(|i| {
                    let u = i as f64 / n as f64;
                    let (s, c) = (TAU * (i % n) as f64 / n as f64).sin_cos_of();
                    b.vert_uv(DVec3::new(r * s, y, r * c), DVec2::new(u, v))
                })
                .collect()
        })
        .collect();
    for k in 0..rows.len().saturating_sub(1) {
        let (top, bot) = (&rows[k], &rows[k + 1]);
        let (r0, r1) = (profile[k].0, profile[k + 1].0);
        for i in 0..n as usize {
            if r0 == 0.0 {
                b.tri(top[i], bot[i], bot[i + 1]);
            } else if r1 == 0.0 {
                b.tri(top[i], bot[i], top[i + 1]);
            } else {
                b.quad(top[i], bot[i], bot[i + 1], top[i + 1]);
            }
        }
    }
    b.orient_outward();
    b
}

pub fn sphere(radius: f64, segments: u32, rings: u32) -> Mesh {
    let rings = rings.max(2);
    let profile: Vec<(f64, f64, f64)> = (0..=rings)
        .map(|j| {
            let v = j as f64 / rings as f64;
            let a = PI * v;
            let r = if j == 0 || j == rings {
                0.0
            } else {
                radius * a.sin_of()
            };
            (r, radius * a.cos_of(), v)
        })
        .collect();
    revolve(&profile, segments).finish(180.0)
}

pub fn capsule(radius: f64, length: f64, segments: u32, rings: u32) -> Mesh {
    let rings = rings.max(1);
    let half = length.max(0.0) * 0.5;
    let arc = FRAC_PI_2 * radius;
    let total = (2.0 * arc + 2.0 * half).max(1e-300);
    let mut profile = Vec::new();
    for j in 0..=rings {
        let a = FRAC_PI_2 * j as f64 / rings as f64;
        let r = if j == 0 { 0.0 } else { radius * a.sin_of() };
        profile.push((
            r,
            half + radius * a.cos_of(),
            arc * j as f64 / rings as f64 / total,
        ));
    }
    for j in 0..=rings {
        let a = FRAC_PI_2 * j as f64 / rings as f64;
        let r = if j == rings { 0.0 } else { radius * a.cos_of() };
        profile.push((
            r,
            -half - radius * a.sin_of(),
            (arc + 2.0 * half + arc * j as f64 / rings as f64) / total,
        ));
    }
    revolve(&profile, segments).finish(180.0)
}

pub fn torus(major: f64, minor: f64, segments: u32, rings: u32) -> Mesh {
    let (n, k) = (segments.max(3), rings.max(3));
    let mut b = Build::default();
    let rows: Vec<Vec<u32>> = (0..=n)
        .map(|i| {
            let u = i as f64 / n as f64;
            let (s, c) = (TAU * (i % n) as f64 / n as f64).sin_cos_of();
            let radial = DVec3::new(s, 0.0, c);
            (0..=k)
                .map(|j| {
                    let v = j as f64 / k as f64;
                    let (bs, bc) = (TAU * (j % k) as f64 / k as f64).sin_cos_of();
                    let p = radial * (major + minor * bc) + DVec3::Y * minor * bs;
                    b.vert_uv(p, DVec2::new(u, v))
                })
                .collect()
        })
        .collect();
    for i in 0..n as usize {
        b.strip(&rows[i], &rows[i + 1], false);
    }
    b.orient_outward();
    b.finish(180.0)
}

pub fn cylinder(radius: f64, depth: f64, segments: u32, caps: bool) -> Mesh {
    let n = segments.max(3) as usize;
    let h = depth * 0.5;
    let mut b = Build::default();
    let (bot, top): (Vec<u32>, Vec<u32>) = (0..n)
        .map(|a| {
            let (s, c) = (TAU * a as f64 / n as f64).sin_cos_of();
            (
                b.vert(DVec3::new(-radius * s, -h, -radius * c)),
                b.vert(DVec3::new(-radius * s, h, -radius * c)),
            )
        })
        .unzip();
    b.strip(&bot, &top, true);
    if caps {
        b.fan(&bot, false);
        b.fan(&top, true);
    }
    b.orient_outward();
    b.finish(30.0)
}

pub fn rounded_box(size: [f64; 3], radius: f64, segments: u32) -> Mesh {
    let half = DVec3::from(size).abs() * 0.5;
    let r = radius.max(0.0).min(half.min_element());
    let m = if r > 0.0 {
        segments.max(1).div_ceil(2)
    } else {
        0
    };
    let axis = |h: f64| -> Vec<(f64, f64)> {
        let a = h - r;
        let mut out = Vec::new();
        for k in (1..=m).rev() {
            out.push((-a, -(FRAC_PI_4 * k as f64 / m as f64).tan_of()));
        }
        out.push((-a, 0.0));
        out.push((a, 0.0));
        for k in 1..=m {
            out.push((a, (FRAC_PI_4 * k as f64 / m as f64).tan_of()));
        }
        out
    };
    let mut b = Build::default();
    for (n, u, v) in [
        (DVec3::X, DVec3::Y, DVec3::Z),
        (-DVec3::X, DVec3::Z, DVec3::Y),
        (DVec3::Y, DVec3::Z, DVec3::X),
        (-DVec3::Y, DVec3::X, DVec3::Z),
        (DVec3::Z, DVec3::X, DVec3::Y),
        (-DVec3::Z, DVec3::Y, DVec3::X),
    ] {
        let hn = half.dot(n.abs());
        let (lu, lv) = (axis(half.dot(u)), axis(half.dot(v)));
        let base = b.pos.len() as u32;
        for &(cv, tv) in &lv {
            for &(cu, tu) in &lu {
                let inner = u * cu + v * cv + n * (hn - r);
                let dir = (u * tu + v * tv + n).normalize();
                b.vert(inner + dir * r);
            }
        }
        let w = lu.len() as u32;
        for j in 0..lv.len() as u32 - 1 {
            for i in 0..w - 1 {
                let a = base + j * w + i;
                b.quad(a, a + 1, a + w + 1, a + w);
            }
        }
    }
    b.weld(half.max_element() * 1e-9);
    b.orient_outward();
    b.finish(30.0)
}

pub fn lathe(profile: &[[f64; 2]], segments: u32, weld: f64) -> Mesh {
    let n = segments.max(3) as usize;
    let mut b = Build::default();
    let rings: Vec<Vec<u32>> = (0..n)
        .map(|s| {
            let (sa, ca) = (TAU * s as f64 / n as f64).sin_cos_of();
            profile
                .iter()
                .map(|&[r, y]| b.vert(DVec3::new(r * sa, y, r * ca)))
                .collect()
        })
        .collect();
    for s in 0..n {
        b.strip(&rings[s], &rings[(s + 1) % n], false);
    }
    b.weld(weld.max(0.0));
    b.orient_outward();
    b.finish(30.0)
}

pub fn sweep(path: &[[f64; 3]], radii: &[f64], ring: u32, closed: bool, caps: bool) -> Mesh {
    let mut pts: Vec<DVec3> = Vec::new();
    for &p in path {
        let p = DVec3::from(p);
        if pts.last().is_none_or(|q| q.distance(p) > 0.0) {
            pts.push(p);
        }
    }
    let closed = closed && pts.len() > 2;
    if closed && pts.first() == pts.last() {
        pts.pop();
    }
    let n = pts.len();
    if n < 2 {
        return Mesh::default();
    }
    let sides = ring.max(3) as usize;
    let radius = |i: usize| -> f64 {
        match radii.len() {
            0 => 0.0,
            1 => radii[0],
            k if k == n => radii[i],
            k => {
                let span = if closed { n } else { n - 1 };
                let t = i as f64 / span.max(1) as f64 * (k - 1) as f64;
                let j = (t.floor() as usize).min(k - 2);
                let f = t - j as f64;
                radii[j] * (1.0 - f) + radii[j + 1] * f
            }
        }
    };
    let tangent: Vec<DVec3> = (0..n)
        .map(|i| {
            let (a, c) = if closed {
                (pts[(i + n - 1) % n], pts[(i + 1) % n])
            } else {
                (pts[i.saturating_sub(1)], pts[(i + 1).min(n - 1)])
            };
            (c - a).normalize_or_zero()
        })
        .collect();
    let first = {
        let side = tangent[0].cross(DVec3::Z);
        let side = if side.length() < 1e-4 {
            tangent[0].cross(DVec3::X)
        } else {
            side
        };
        side.normalize()
    };
    let transport = |r: DVec3, x0: DVec3, x1: DVec3, t0: DVec3, t1: DVec3| -> DVec3 {
        let v1 = x1 - x0;
        let c1 = v1.length_squared();
        if c1 <= 0.0 {
            return r;
        }
        let rl = r - v1 * (2.0 / c1 * v1.dot(r));
        let tl = t0 - v1 * (2.0 / c1 * v1.dot(t0));
        let v2 = t1 - tl;
        let c2 = v2.length_squared();
        if c2 <= 1e-30 {
            return rl.normalize();
        }
        (rl - v2 * (2.0 / c2 * v2.dot(rl))).normalize()
    };
    let mut frames = vec![first];
    for i in 1..n {
        let r = transport(
            frames[i - 1],
            pts[i - 1],
            pts[i],
            tangent[i - 1],
            tangent[i],
        );
        frames.push(r);
    }
    if closed {
        let back = transport(
            frames[n - 1],
            pts[n - 1],
            pts[0],
            tangent[n - 1],
            tangent[0],
        );
        let angle = back
            .cross(frames[0])
            .dot(tangent[0])
            .atan2_of(back.dot(frames[0]));
        for (i, f) in frames.iter_mut().enumerate() {
            *f = DQuat::from_axis_angle(tangent[i], angle * i as f64 / n as f64) * *f;
        }
    }
    let mut b = Build::default();
    let mut length = 0.0;
    let count = if closed { n + 1 } else { n };
    let mut rows: Vec<Vec<u32>> = Vec::with_capacity(count);
    for k in 0..count {
        let i = k % n;
        if k > 0 {
            length += pts[i].distance(pts[k - 1]);
        }
        let (t, nrm) = (tangent[i], frames[i]);
        let bin = t.cross(nrm);
        let r = radius(i);
        rows.push(
            (0..=sides)
                .map(|s| {
                    let a = TAU * s as f64 / sides as f64;
                    let (sa, ca) = (TAU * (s % sides) as f64 / sides as f64).sin_cos_of();
                    let d = nrm * ca + bin * sa;
                    b.vert_uv(pts[i] + d * r, DVec2::new(a * r, length))
                })
                .collect(),
        );
    }
    for k in 0..count - 1 {
        b.band(&rows[k], &rows[k + 1], false);
    }
    if caps && !closed {
        b.fan(&rows[0][..sides], true);
        b.fan(&rows[n - 1][..sides], false);
    }
    b.finish(60.0)
}
