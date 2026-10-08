mod common;

use common::*;
use glam::DVec2;
use manfred_mesh::outline::{self, Outline, Shape, circle, rounded_rect};
use manfred_mesh::*;
use std::collections::BTreeSet;
use std::f64::consts::PI;

const FRONT: f64 = 0.0;
const BACK: f64 = 0.02;

fn check(m: &Mesh) {
    assert_sane(m);
    assert_closed(m);
    assert_outward(m);
    assert!(min_area(m) > 1e-14, "smallest triangle {}", min_area(m));
}

fn euler(m: &Mesh) -> i64 {
    let id = ids(m);
    let vertices: BTreeSet<u32> = m.indices.iter().map(|&i| id[i as usize]).collect();
    let mut edges = BTreeSet::new();
    for t in m.indices.chunks_exact(3) {
        for e in 0..3 {
            let (a, b) = (id[t[e] as usize], id[t[(e + 1) % 3] as usize]);
            edges.insert((a.min(b), a.max(b)));
        }
    }
    vertices.len() as i64 - edges.len() as i64 + m.triangle_count() as i64
}

struct Areas {
    caps: f64,
    inner: f64,
    outer: f64,
}

fn areas(m: &Mesh, inner: impl Fn(DVec2) -> bool) -> Areas {
    let mut out = Areas {
        caps: 0.0,
        inner: 0.0,
        outer: 0.0,
    };
    for t in m.indices.chunks_exact(3) {
        let p = [m.point(t[0]), m.point(t[1]), m.point(t[2])];
        let a = (p[1] - p[0]).cross(p[2] - p[0]).length() * 0.5;
        let flat = p.iter().all(|q| q.z == p[0].z);
        let on_cap = flat && ((p[0].z + FRONT).abs() < 1e-7 || (p[0].z + BACK).abs() < 1e-7);
        if on_cap {
            out.caps += a;
            continue;
        }
        let c = (p[0] + p[1] + p[2]) / 3.0;
        if inner(c.truncate()) {
            out.inner += a;
        } else {
            out.outer += a;
        }
    }
    out
}

fn band(l: f64, w: f64, sign: f64) -> f64 {
    w * (l * PI * 0.5 + sign * 2.0 * PI * w * (PI * 0.5 - 1.0))
}

fn wall(l: f64, w: f64, sign: f64) -> f64 {
    2.0 * band(l, w, sign) + l * (BACK - FRONT - 2.0 * w)
}

fn close(got: f64, want: f64, rel: f64, what: &str) {
    assert!(
        (got - want).abs() <= rel * want.abs(),
        "{what}: {got} vs {want}, off by {:.2e}",
        (got - want).abs() / want.abs()
    );
}

fn into_hole(m: &Mesh, centre: DVec2, inner: impl Fn(DVec2) -> bool) {
    for t in m.indices.chunks_exact(3) {
        let p = [m.point(t[0]), m.point(t[1]), m.point(t[2])];
        let c = ((p[0] + p[1] + p[2]) / 3.0).truncate();
        let n = m.face_normal(t);
        if !inner(c) || n.z.abs() > 0.999 {
            continue;
        }
        assert!(
            n.truncate().dot(centre - c) > 0.0,
            "an inner wall faces away from the hole at {c:?}: {n:?}"
        );
    }
}

fn washer() -> (Shape, Profile) {
    let shape = Shape {
        outer: circle([0.0, 0.0], 0.05, 64),
        holes: vec![circle([0.0, 0.0], 0.02, 64)],
    };
    (shape, Profile::round(0.004, 16))
}

#[test]
fn a_washer_is_one_closed_solid_with_a_rounded_inner_wall() {
    let (shape, edge) = washer();
    let m = extrude_shape(&shape, FRONT, BACK, &edge, UvMode::None);
    check(&m);
    assert_eq!(euler(&m), 0, "a washer has one hole through it");
    let w = edge.width;
    let k = 1.0 / (PI / 64.0).cos();
    let ngon = |r: f64| 32.0 * r * r * (2.0 * PI / 64.0).sin();
    let caps = 2.0 * (ngon(0.05 - w * k) - ngon(0.02 + w * k));
    let inner_l = shape.holes[0].perimeter();
    let outer_l = shape.outer.perimeter();
    let got = areas(&m, |c| c.length() < 0.035);
    close(got.caps, caps, 1e-6, "caps");
    close(got.inner, wall(inner_l, w, 1.0), 5e-4, "inner wall");
    close(got.outer, wall(outer_l, w, -1.0), 5e-4, "outer wall");
    into_hole(&m, DVec2::ZERO, |c| c.length() < 0.035);
}

#[test]
fn a_frame_is_one_closed_solid_with_its_corners_rounded_both_ways() {
    let shape = Shape {
        outer: Outline(vec![
            [-0.05, -0.04],
            [0.05, -0.04],
            [0.05, 0.04],
            [-0.05, 0.04],
        ]),
        holes: vec![Outline(vec![
            [-0.02, -0.015],
            [0.02, -0.015],
            [0.02, 0.015],
            [-0.02, 0.015],
        ])],
    };
    let r = 0.004;
    let m = extrude_shape(&shape, FRONT, BACK, &Profile::round(r, 24), UvMode::None);
    check(&m);
    assert_eq!(euler(&m), 0);
    let w = -m
        .positions
        .iter()
        .map(|p| p[2] as f64)
        .filter(|&z| z > -0.5 * BACK)
        .fold(0.0, f64::min);
    assert!(w > 0.9 * r && w < r, "the profile's width {w}");
    let rounded = |x: f64, y: f64| {
        let a = x * y - (4.0 - PI) * r * r;
        let l = 2.0 * (x + y) - (8.0 - 2.0 * PI) * r;
        (a, l)
    };
    let (outer_a, outer_l) = rounded(0.1, 0.08);
    let (hole_a, hole_l) = rounded(0.04, 0.03);
    let outer_cap = outer_a - outer_l * w + PI * w * w;
    let hole_cap = hole_a + hole_l * w + PI * w * w;
    let inside = |c: DVec2| c.x.abs() < 0.02 + 1.5 * w && c.y.abs() < 0.015 + 1.5 * w;
    let got = areas(&m, inside);
    close(got.caps, 2.0 * (outer_cap - hole_cap), 5e-5, "caps");
    close(got.inner, wall(hole_l, w, 1.0), 5e-4, "inner wall");
    close(got.outer, wall(outer_l, w, -1.0), 5e-4, "outer wall");
    into_hole(&m, DVec2::ZERO, inside);
}

#[test]
fn a_chamfered_and_a_sharp_washer_are_closed_too() {
    let (shape, _) = washer();
    for edge in [
        Profile {
            kind: ProfileKind::Chamfer,
            width: 0.003,
            segments: 1,
        },
        Profile::NONE,
    ] {
        let m = extrude_shape(&shape, FRONT, BACK, &edge, UvMode::Local);
        check(&m);
        assert_eq!(euler(&m), 0);
    }
}

#[test]
fn two_holes_make_a_closed_solid_of_genus_two() {
    let shape = Shape {
        outer: rounded_rect([0.0, 0.0, 0.12, 0.06], 0.01, 8),
        holes: vec![
            circle([0.03, 0.03], 0.015, 48),
            rounded_rect([0.065, 0.015, 0.1, 0.045], 0.004, 6),
        ],
    };
    let m = extrude_shape(&shape, FRONT, BACK, &Profile::round(0.003, 6), UvMode::None);
    check(&m);
    assert_eq!(euler(&m), -2);
}

#[test]
fn a_ring_in_a_ring_by_even_odd_is_two_solids_in_one_mesh() {
    let contours = [
        circle([0.0, 0.0], 0.05, 64),
        circle([0.0, 0.0], 0.04, 64),
        circle([0.0, 0.0], 0.03, 64),
        circle([0.0, 0.0], 0.02, 64),
    ];
    let found = outline::even_odd(&contours);
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|s| s.holes.len() == 1));
    let mut m = Mesh::default();
    for s in &found {
        m.append(&extrude_shape(
            s,
            FRONT,
            BACK,
            &Profile::round(0.002, 4),
            UvMode::None,
        ));
    }
    check(&m);
    assert_eq!(euler(&m), 0);
    let island = outline::even_odd(&contours[..3]);
    assert_eq!(island.len(), 2);
    assert_eq!(island.iter().map(|s| s.holes.len()).sum::<usize>(), 1);
}

#[test]
fn a_pocket_cuts_into_a_holed_outline() {
    let shape = Shape {
        outer: rounded_rect([0.0, 0.0, 0.2, 0.1], 0.01, 8),
        holes: vec![circle([0.05, 0.05], 0.02, 48)],
    };
    let well = Pocket {
        outline: rounded_rect([0.1, 0.02, 0.18, 0.08], 0.008, 8),
        depth: 0.01,
        rim: Profile::round(0.002, 3),
        floor: Profile::round(0.002, 3),
    };
    let edge = Profile::round(0.004, 6);
    let m = pocket_shape(
        &shape,
        FRONT,
        BACK,
        &edge,
        std::slice::from_ref(&well),
        UvMode::None,
    );
    check(&m);
    assert_eq!(euler(&m), 0);
    let close_by = Pocket {
        outline: rounded_rect([0.0735, 0.03, 0.1, 0.07], 0.004, 8),
        ..well
    };
    let m = pocket_shape(&shape, FRONT, BACK, &edge, &[close_by], UvMode::None);
    check(&m);
    assert_eq!(euler(&m), 0);
}

#[test]
fn a_shape_without_holes_builds_as_the_outline_does() {
    let o = rounded_rect([0.0, 0.0, 0.2, 0.1], 0.01, 8);
    let edge = Profile::round(0.004, 6);
    assert_eq!(
        bytes(&extrude(&o, FRONT, BACK, &edge, UvMode::Local)),
        bytes(&extrude_shape(
            &Shape::new(o),
            FRONT,
            BACK,
            &edge,
            UvMode::Local
        ))
    );
}

#[test]
fn a_holed_shape_twice_gives_the_same_bytes() {
    let (shape, edge) = washer();
    assert_eq!(
        bytes(&extrude_shape(&shape, FRONT, BACK, &edge, UvMode::Local)),
        bytes(&extrude_shape(&shape, FRONT, BACK, &edge, UvMode::Local))
    );
}

#[test]
fn clearance_and_pinch_find_touching_contours() {
    let a = Outline(vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]]);
    let inside = Outline(vec![[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]]);
    let crossing = Outline(vec![[3.0, 1.0], [5.0, 1.0], [5.0, 2.0], [3.0, 2.0]]);
    assert!((outline::clearance(&a, &inside) - 1.0).abs() < 1e-12);
    assert_eq!(outline::clearance(&a, &crossing), 0.0);
    let touch = Outline(vec![[1.0, 0.0], [2.0, 1.0], [1.0, 2.0], [0.5, 1.0]]);
    let cut = outline::difference(&Shape::new(a.clone()), &[touch]);
    assert_eq!(outline::pinch(&cut[0].outer, 1e-9), Some([1.0, 0.0]));
    assert_eq!(outline::pinch(&a, 1e-9), None);
}
