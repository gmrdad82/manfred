mod common;

use common::*;
use glam::{DMat4, DVec2, DVec3};
use manfred_mesh::outline::{self, Outline, Shape, circle, fillet, rounded_rect, turned};
use manfred_mesh::*;
use std::f64::consts::PI;

const W: f64 = 1600.0;
const H: f64 = 1000.0;
const S: f64 = 0.0004;

fn wx(px: f64) -> f64 {
    (px - W * 0.5) * S
}

fn wz(py: f64) -> f64 {
    (H * 0.5 - py) * S
}

fn metres(o: &Outline) -> Outline {
    o.map(|[x, y]| [wx(x), wz(y)])
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64, r: f64) -> Outline {
    metres(&rounded_rect([x0, y0, x1, y1], r, 12))
}

fn tray(g: f64) -> Vec<[f64; 3]> {
    vec![
        [120.0 - g, 120.0 - g, 30.0 + g],
        [900.0 + g, 120.0 - g, 30.0 + g],
        [900.0 + g, 520.0 + g, (12.0 - g).max(4.0)],
        [700.0 + g, 520.0 + g, (24.0 - g).max(8.0)],
        [700.0 + g, 600.0 + g, 12.0 + g],
        [120.0 - g, 600.0 + g, 30.0 + g],
    ]
}

fn shelf(g: f64) -> Vec<[f64; 3]> {
    vec![
        [160.0 - g, 700.0 - g, 22.0 + g],
        [940.0 + g, 700.0 - g, 30.0 + g],
        [940.0 + g, 900.0 + g, 30.0 + g],
        [80.0 - g, 900.0 + g, 30.0 + g],
        [80.0 - g, 760.0 - g, 22.0 + g],
        [140.0 - g, 760.0 - g, (24.0 - g).max(6.0)],
    ]
}

fn lobed() -> Outline {
    let (x0, y0, x1, y1, r) = (1000.0, 520.0, 1180.0, 740.0, 30.0);
    let bumps = [580.0, 630.0, 680.0];
    let right = |y: f64| {
        let d = bumps
            .iter()
            .map(|b: &f64| (y - b).abs())
            .fold(f64::INFINITY, f64::min);
        x1 - 20.0 * ((d / 24.0).min(1.0) * PI).cos().mul_add(0.5, 0.5)
    };
    let mut pts = Vec::new();
    for k in 0..13 {
        let a = (-90.0 + 90.0 * k as f64 / 12.0_f64).to_radians();
        pts.push([x1 - r + r * a.cos(), y0 + r + r * a.sin()]);
    }
    for k in 1..160 {
        let y = y0 + r + (y1 - y0 - 2.0 * r) * k as f64 / 160.0;
        pts.push([right(y), y]);
    }
    for (start, cx, cy) in [
        (0.0, x1 - r, y1 - r),
        (90.0, x0 + r, y1 - r),
        (180.0, x0 + r, y0 + r),
    ] {
        for k in 0..13 {
            let a = (start + 90.0 * k as f64 / 12.0_f64).to_radians();
            pts.push([cx + r * a.cos(), cy + r * a.sin()]);
        }
    }
    Outline(pts)
}

fn dial_well() -> Outline {
    let pts: Vec<[f64; 3]> = (0..64)
        .map(|a| {
            let t = a as f64 * PI / 32.0;
            [1250.0 + 160.0 * t.cos(), 330.0 + 160.0 * t.sin(), 0.0]
        })
        .collect();
    fillet(&pts, 12)
}

fn rim() -> Profile {
    Profile::round(0.002, 4)
}

fn well(o: Outline, depth: f64) -> Pocket {
    Pocket {
        outline: metres(&o),
        depth,
        rim: rim(),
        floor: rim(),
    }
}

fn shell_outline() -> Outline {
    rect(40.0, 40.0, 1560.0, 960.0, 48.0)
}

fn body() -> Profile {
    Profile::round(0.012, 8)
}

fn wells() -> Vec<Pocket> {
    vec![
        well(fillet(&tray(4.0), 12), 0.010),
        well(dial_well(), 0.008),
        well(lobed(), 0.05),
        well(fillet(&shelf(0.0), 12), 0.1),
        well(
            rounded_rect([1000.0, 780.0, 1480.0, 880.0], 20.0, 12),
            0.012,
        ),
        well(
            rounded_rect([1220.0, 560.0, 1500.0, 640.0], 30.0, 12),
            0.010,
        ),
    ]
}

fn shell() -> Mesh {
    pocket(
        &shell_outline(),
        0.0,
        0.12,
        &body(),
        &wells(),
        UvMode::Local,
    )
}

fn search() -> Mesh {
    pocket(
        &rect(1200.0, 620.0, 1520.0, 700.0, 30.0),
        0.001,
        0.009,
        &Profile::round(0.003, 5),
        &[Pocket {
            outline: rect(1240.0, 640.0, 1480.0, 680.0, 9.0),
            depth: 0.0056,
            rim: Profile::NONE,
            floor: Profile::NONE,
        }],
        UvMode::None,
    )
}

fn polygon(o: &Outline) -> (f64, f64) {
    (o.area().abs(), o.perimeter())
}

fn rounded_area(a: f64, l: f64, d: f64) -> f64 {
    a + l * d + PI * d * d
}

fn band(l: f64, w: f64, sign: f64) -> f64 {
    w * (l * PI * 0.5 + sign * 2.0 * PI * w * (PI * 0.5 - 1.0))
}

fn slab_area(o: &Outline, w: f64, thick: f64) -> f64 {
    let (a, l) = polygon(o);
    2.0 * rounded_area(a, l, -w) + 2.0 * band(l, w, -1.0) + l * (thick - 2.0 * w)
}

fn check(m: &Mesh) {
    assert_sane(m);
    assert_closed(m);
    assert_outward(m);
    assert!(min_area(m) > 1e-14, "smallest triangle {}", min_area(m));
}

#[test]
fn every_outline_triangulates_cleanly() {
    let mut all = vec![
        shell_outline(),
        metres(&fillet(&tray(4.0), 12)),
        metres(&fillet(&tray(0.0), 12)),
        metres(&fillet(&tray(3.0), 12)),
        metres(&fillet(&tray(4.0), 16)),
        metres(&dial_well()),
        metres(&lobed()),
        metres(&fillet(&shelf(0.0), 12)),
        metres(&fillet(&shelf(0.0), 16)),
        rect(1000.0, 780.0, 1480.0, 880.0, 20.0),
        rect(1220.0, 560.0, 1500.0, 640.0, 30.0),
        rect(1240.0, 640.0, 1480.0, 680.0, 9.0),
        rect(1200.0, 620.0, 1520.0, 700.0, 30.0),
        rect(1300.0, 100.0, 1500.0, 2100.0, 0.0),
        metres(&turned([600.0, 300.0, 900.0, 600.0], 4.0, -1.4, 6)),
        metres(&turned([700.0, 280.0, 800.0, 310.0], 2.0, -1.9, 6)),
        metres(&circle([1250.0, 330.0], 40.0, 64)),
    ];
    all.push(Outline(
        (0..120)
            .map(|i| {
                let r = 46.0 - 1.1 * (i % 2) as f64;
                let t = i as f64 * PI / 60.0;
                [wx(1250.0 + r * t.cos()), wz(330.0 + r * t.sin())]
            })
            .collect(),
    ));
    for o in all {
        let s = Shape::new(o.clone());
        let tris = outline::triangulate(&s);
        assert_eq!(tris.len(), o.0.len() - 2);
        let pts: Vec<DVec2> = o.0.iter().map(|&p| DVec2::from(p)).collect();
        let mut sum = 0.0;
        for t in &tris {
            let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
            let area = (b - a).perp_dot(c - a) * 0.5;
            assert!(area > 1e-14, "degenerate triangle {area}");
            sum += area;
        }
        assert!((sum - o.area().abs()).abs() < 1e-9 * o.area().abs());
    }
}

#[test]
fn body_slab_matches_its_analytic_area() {
    let o = shell_outline();
    let m = extrude(&o, 0.0, 0.12, &body(), UvMode::Local);
    check(&m);
    let exact = slab_area(&o, 0.012, 0.12);
    assert!(
        (area(&m) - exact).abs() / exact < 0.01,
        "{} vs {exact}",
        area(&m)
    );
    let [lo, hi] = bounds(&m).unwrap();
    assert!((lo[2] + 0.12).abs() < 1e-6 && hi[2].abs() < 1e-6);
    assert!((lo[0] - wx(40.0)).abs() < 1e-6 && (hi[0] - wx(1560.0)).abs() < 1e-6);
}

#[test]
fn tray_well_matches_its_analytic_area() {
    let o = shell_outline();
    let p = well(fillet(&tray(4.0), 12), 0.010);
    let m = pocket(
        &o,
        0.0,
        0.12,
        &body(),
        std::slice::from_ref(&p),
        UvMode::Local,
    );
    check(&m);
    let (r, f, depth) = (0.002, 0.002, 0.010);
    let (ap, lp) = polygon(&p.outline);
    let exact = slab_area(&o, 0.012, 0.12) - rounded_area(ap, lp, r)
        + band(lp, r, 1.0)
        + lp * (depth - r - f)
        + band(lp, f, -1.0)
        + rounded_area(ap, lp, -f);
    assert!(
        (area(&m) - exact).abs() / exact < 0.01,
        "{} vs {exact}",
        area(&m)
    );
    let floor = m
        .positions
        .iter()
        .filter(|p| (p[2] as f64 + depth).abs() < 1e-6)
        .count();
    assert!(floor > 50);
    assert!(
        (volume(&m) - (volume(&extrude(&o, 0.0, 0.12, &body(), UvMode::None)) - ap * depth)).abs()
            < 2e-5
    );
}

#[test]
fn shell_with_all_six_wells_is_closed() {
    let m = shell();
    check(&m);
    let plain = extrude(&shell_outline(), 0.0, 0.12, &body(), UvMode::None);
    let removed: f64 = wells()
        .iter()
        .map(|w| w.outline.area().abs() * w.depth)
        .sum();
    let carved = volume(&plain) - volume(&m);
    assert!(
        carved > 0.6 * removed && carved < 1.05 * removed,
        "{carved} vs {removed}"
    );
    for w in wells() {
        let deepest = m
            .positions
            .iter()
            .filter(|p| w.outline.contains([p[0] as f64, p[1] as f64]))
            .map(|p| -p[2] as f64)
            .fold(0.0, f64::max);
        assert!((deepest - w.depth).abs() < 1e-6, "{deepest} vs {}", w.depth);
    }
}

#[test]
fn a_slot_is_a_sharp_pocket() {
    let m = search();
    check(&m);
    let slot = rect(1240.0, 640.0, 1480.0, 680.0, 9.0);
    let inside: Vec<&[f32; 3]> = m
        .positions
        .iter()
        .filter(|p| slot.contains([p[0] as f64, p[1] as f64]))
        .collect();
    assert!(inside.iter().any(|p| (p[2] as f64 + 0.0056).abs() < 1e-6));
    assert!(
        inside
            .iter()
            .all(|p| (p[2] as f64 + 0.0056).abs() < 1e-6 || (p[2] as f64 + 0.001).abs() < 1e-6)
    );
}

#[test]
fn overlapping_pockets_are_unioned() {
    let o = rounded_rect([0.0, 0.0, 1.0, 0.6], 0.05, 8);
    let a = Pocket {
        outline: rounded_rect([0.1, 0.1, 0.5, 0.4], 0.04, 8),
        depth: 0.05,
        rim: Profile::round(0.01, 3),
        floor: Profile::round(0.01, 3),
    };
    let b = Pocket {
        outline: circle([0.5, 0.25], 0.12, 48),
        ..a.clone()
    };
    let m = pocket(
        &o,
        0.0,
        0.2,
        &Profile::round(0.02, 4),
        &[a, b],
        UvMode::None,
    );
    check(&m);
}

#[test]
fn the_shells_twice_give_the_same_bytes() {
    assert_eq!(bytes(&shell()), bytes(&shell()));
    assert_eq!(bytes(&search()), bytes(&search()));
}

fn slab(rect_px: [f64; 4], r: f64, front: f64, back: f64, bevel: f64) -> Mesh {
    let [x0, y0, x1, y1] = rect_px;
    extrude(
        &rect(x0, y0, x1, y1, r),
        front,
        back,
        &Profile::round(bevel, 5),
        UvMode::None,
    )
}

fn slab_pts(o: Outline, front: f64, back: f64, bevel: f64) -> Mesh {
    extrude(
        &metres(&o),
        front,
        back,
        &Profile::round(bevel, 5),
        UvMode::None,
    )
}

fn disc_part(cx: f64, cy: f64, r: f64, front: f64, back: f64, bevel: f64) -> Mesh {
    slab_pts(circle([cx, cy], r, 64), front, back, bevel)
}

fn axle(px0: f64, px1: f64, r_px: f64, vertices: u32, bevel: f64, segments: u32) -> Mesh {
    let r = r_px * S;
    extrude(
        &circle([0.0, 0.0], r, vertices),
        0.0,
        (px1 - px0) * S,
        &Profile::round(bevel.min(r * 0.2), segments),
        UvMode::None,
    )
}

fn bead(o: Outline, radius: f64, centre: f64, out: f64) -> Mesh {
    let world = metres(&o).ccw();
    let pts: Vec<DVec2> = world.0.iter().map(|&p| DVec2::from(p)).collect();
    let n = pts.len();
    let path: Vec<[f64; 3]> = (0..n)
        .map(|i| {
            let t = (pts[(i + 1) % n] - pts[(i + n - 1) % n]).normalize();
            let q = pts[i] + DVec2::new(t.y, -t.x) * out;
            [q.x, q.y, -centre]
        })
        .collect();
    sweep(&path, &[radius], 16, true, false)
}

fn parts() -> Vec<(&'static str, Mesh)> {
    let vase_r = |z: f64| 0.03 + (0.036 - 0.03) * z / 0.070;
    let arc = |cx: f64, cz: f64, r: f64, a0: f64, a1: f64, n: usize| -> Vec<[f64; 2]> {
        (0..=n)
            .map(|k| {
                let a = (a0 + (a1 - a0) * k as f64 / n as f64).to_radians();
                [cx + r * a.cos(), cz + r * a.sin()]
            })
            .collect()
    };
    let mut outer = vec![[0.0, 0.0], [0.027, 0.0]];
    outer.extend(arc(0.027, 0.003, 0.003, -90.0, 0.0, 5).into_iter().skip(1));
    outer.extend([0.012, 0.03, 0.05, 0.069].map(|z| [vase_r(z), z]));
    let mut body_profile = outer;
    body_profile.extend([0.069, 0.05, 0.03, 0.012].map(|z| [vase_r(z) - 0.0012, z]));
    body_profile.extend([[0.03 - 0.0012 - 0.003, 0.0045], [0.0, 0.0042]]);
    let mut slat = rounded_box([1.6, 0.026, 0.0024], 0.0, 0);
    transform(
        &mut slat,
        DMat4::from_translation(DVec3::new(0.1, 0.2, 0.3)),
    );
    vec![
        ("shell", shell()),
        ("search", search()),
        (
            "mv_lid",
            slab_pts(fillet(&tray(0.0), 12), 0.0040, 0.0056, 0.0006),
        ),
        (
            "gasket",
            extrude(
                &metres(&fillet(&tray(3.0), 12)),
                0.0070,
                0.0098,
                &Profile::NONE,
                UvMode::None,
            ),
        ),
        (
            "pane",
            slab([140.0, 140.0, 880.0, 500.0], 10.0, 0.0058, 0.0064, 0.0004),
        ),
        (
            "mv_key",
            slab([1010.0, 790.0, 1230.0, 870.0], 13.0, -0.005, 0.011, 0.0045),
        ),
        (
            "mv_lid__button",
            slab([800.0, 140.0, 848.0, 183.0], 8.0, 0.0034, 0.0045, 0.0005),
        ),
        (
            "bed",
            slab([160.0, 710.0, 930.0, 890.0], 10.0, 0.020, 0.030, 0.002),
        ),
        (
            "mv_dial",
            disc_part(1250.0, 330.0, 150.0, -0.003, 0.006, 0.002),
        ),
        (
            "mv_knob__face",
            disc_part(1250.0, 330.0, 60.0, -0.0135, -0.008, 0.0035),
        ),
        ("cap", axle(338.0, 352.0, 25.0, 64, 0.0012, 3)),
        ("drum", axle(26.0, 74.0, 47.0, 96, 0.0012, 3)),
        ("mv_roller", axle(160.0, 860.0, 10.0, 64, 0.0012, 3)),
        (
            "lip_tray",
            bead(fillet(&tray(4.0), 16), 0.002, 0.0009, 0.0008),
        ),
        ("lip_lobed", bead(lobed(), 0.0042, 0.0012, 0.0026)),
        (
            "lip_shelf",
            bead(fillet(&shelf(0.0), 16), 0.0040, 0.0010, 0.0015),
        ),
        (
            "slot_bezel",
            bead(
                rounded_rect([1197.0, 617.0, 1523.0, 703.0], 33.0, 16),
                0.0013,
                0.0012,
                0.0,
            ),
        ),
        ("vase", lathe(&body_profile, 128, 1e-7)),
        (
            "vase_rim",
            lathe(&arc(0.0362, 0.0694, 0.0019, -90.0, 270.0, 18), 128, 1e-7),
        ),
        ("coin", disc(0.03, 96)),
        ("slat", slat),
        ("backdrop", grid([1.6, 1.0], [1, 1], None)),
        ("card", grid([0.12, 0.056], [24, 12], Some(0.12))),
        (
            "lobed_floor",
            extrude(&metres(&lobed()), 0.045, 0.05, &Profile::NONE, UvMode::None),
        ),
        (
            "shelf_back",
            extrude(
                &metres(&fillet(&shelf(0.0), 12)),
                0.09,
                0.1,
                &Profile::NONE,
                UvMode::None,
            ),
        ),
    ]
}

#[test]
fn every_part_is_closed_where_it_should_be() {
    for (name, m) in parts() {
        assert_sane(&m);
        if !matches!(name, "coin" | "backdrop" | "card") {
            assert_closed(&m);
            assert_outward(&m);
        }
    }
}
