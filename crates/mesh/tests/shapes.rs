mod common;

use common::*;
use glam::{DMat4, DVec3, Vec3};
use manfred_mesh::outline::{Outline, circle, rounded_rect};
use manfred_mesh::*;
use std::f64::consts::PI;

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1e-12)
}

fn bounds_of(m: &Mesh) -> [[f64; 3]; 2] {
    bounds(m).expect("bounds")
}

fn assert_bounds(m: &Mesh, lo: [f64; 3], hi: [f64; 3]) {
    let [a, b] = bounds_of(m);
    for k in 0..3 {
        assert!((a[k] - lo[k]).abs() < 1e-5, "lo {a:?} vs {lo:?}");
        assert!((b[k] - hi[k]).abs() < 1e-5, "hi {b:?} vs {hi:?}");
    }
}

#[test]
fn cylinder_has_blenders_topology() {
    let m = cylinder(0.5, 2.0, 64, true);
    assert_sane(&m);
    assert_eq!(m.triangle_count(), 64 * 2 + 62 * 2);
    assert_closed(&m);
    assert_outward(&m);
    assert_bounds(&m, [-0.5, -1.0, -0.5], [0.5, 1.0, 0.5]);
    assert_eq!(m.positions[0], [0.0, -1.0, -0.5]);
    let open = cylinder(0.5, 2.0, 16, false);
    assert_eq!(open.triangle_count(), 32);
}

#[test]
fn cylinder_at_thirty_degrees_splits_only_the_caps() {
    let mut m = cylinder(1.0, 1.0, 32, true);
    smooth_by_angle(&mut m, 30.0);
    assert_eq!(m.positions.len(), 32 * 2 + 32 * 2);
    for (p, n) in m.positions.iter().zip(&m.normals) {
        let (p, n) = (Vec3::from(*p), Vec3::from(*n));
        if n.y.abs() > 0.5 {
            assert!((n.y.abs() - 1.0).abs() < 1e-5);
            assert_eq!(n.y.signum(), p.y.signum());
        } else {
            let radial = Vec3::new(p.x, 0.0, p.z).normalize();
            assert!(n.dot(radial) > 0.9999, "{n:?} {radial:?}");
        }
    }
}

#[test]
fn cube_at_thirty_five_degrees_has_flat_faces() {
    let mut m = rounded_box([1.0, 2.0, 3.0], 0.0, 4);
    assert_eq!(m.triangle_count(), 12);
    assert_closed(&m);
    assert_outward(&m);
    smooth_by_angle(&mut m, 35.0);
    assert_eq!(m.positions.len(), 24);
    for t in m.indices.chunks_exact(3) {
        let n = Vec3::from(m.normals[t[0] as usize]);
        assert!(t.iter().all(|&i| Vec3::from(m.normals[i as usize]) == n));
        assert!(n.abs().max_element() > 0.99999);
    }
    let v = volume(&m);
    assert!(close(v, 6.0, 1e-6));
    smooth(&mut m);
    assert_eq!(m.positions.len(), 8);
    flat(&mut m);
    assert_eq!(m.positions.len(), 24);
}

#[test]
fn rounded_box_matches_blenders_bevel_count() {
    let m = rounded_box([0.07, 0.035, 0.022], 0.005, 8);
    assert_sane(&m);
    assert_eq!(m.triangle_count(), 972);
    assert_closed(&m);
    assert_outward(&m);
    assert_bounds(&m, [-0.035, -0.0175, -0.011], [0.035, 0.0175, 0.011]);
    let r = 0.005_f64;
    let (x, y, z) = (0.07 - 2.0 * r, 0.035 - 2.0 * r, 0.022 - 2.0 * r);
    let exact = 2.0 * (x * y + y * z + z * x) + 2.0 * PI * r * (x + y + z) + 4.0 * PI * r * r;
    assert!(close(area(&m), exact, 0.01), "{} {exact}", area(&m));
}

#[test]
fn sphere_matches_blenders_uv_sphere() {
    let m = sphere(1.0, 96, 48);
    assert_sane(&m);
    assert_eq!(m.triangle_count(), 9024);
    assert_eq!(m.positions.len(), 4751);
    assert_closed(&m);
    assert_outward(&m);
    assert_bounds(&m, [-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]);
    assert!(close(area(&m), 4.0 * PI, 0.01));
    assert!(close(volume(&m), 4.0 / 3.0 * PI, 0.01));
}

#[test]
fn torus_capsule_and_disc() {
    let t = torus(1.0, 0.25, 48, 24);
    assert_sane(&t);
    assert_eq!(t.triangle_count(), 48 * 24 * 2);
    assert_closed(&t);
    assert_outward(&t);
    assert!(close(volume(&t), 2.0 * PI * PI * 0.0625, 0.02));
    assert_bounds(&t, [-1.25, -0.25, -1.25], [1.25, 0.25, 1.25]);

    let c = capsule(0.5, 2.0, 32, 8);
    assert_sane(&c);
    assert_eq!(c.triangle_count(), 32 * (2 * 8 + 1) * 2 - 64);
    assert_closed(&c);
    assert_outward(&c);
    assert_bounds(&c, [-0.5, -1.5, -0.5], [0.5, 1.5, 0.5]);
    let exact = PI * 0.25 * 2.0 + 4.0 / 3.0 * PI * 0.125;
    assert!(close(volume(&c), exact, 0.02));

    let d = disc(2.0, 40);
    assert_sane(&d);
    assert_eq!(d.triangle_count(), 38);
    assert!(d.normals.iter().all(|n| n[1] > 0.9999));
    assert_bounds(&d, [-2.0, 0.0, -2.0], [2.0, 0.0, 2.0]);
}

#[test]
fn grid_faces_up_and_bends_on_a_drum() {
    let g = grid([2.0, 1.0], [4, 3], None);
    assert_sane(&g);
    assert_eq!(g.triangle_count(), 24);
    assert_eq!(g.positions.len(), 20);
    assert!(g.normals.iter().all(|n| n[1] > 0.9999));
    assert_bounds(&g, [-1.0, 0.0, -0.5], [1.0, 0.0, 0.5]);
    assert!(close(area(&g), 2.0, 1e-9));
    let b = grid([1.0, PI * 0.5], [2, 32], Some(1.0));
    assert!(close(area(&b), PI * 0.5, 2e-3));
    let [lo, hi] = bounds_of(&b);
    assert!((lo[1] + 1.0 - (PI * 0.25).cos()).abs() < 1e-6 && hi[1].abs() < 1e-9);
    assert!(b.normals.iter().all(|n| n[1] > 0.7));
}

#[test]
fn lathe_closes_on_the_axis() {
    let profile: Vec<[f64; 2]> = (0..=16)
        .map(|k| {
            let a = PI * k as f64 / 16.0 - PI * 0.5;
            [a.cos().max(0.0) * 0.3, a.sin() * 0.3]
        })
        .collect();
    let mut p = profile;
    p[0] = [0.0, -0.3];
    p[16] = [0.0, 0.3];
    let m = lathe(&p, 64, 1e-7);
    assert_sane(&m);
    assert_eq!(m.triangle_count(), 64 * 16 * 2 - 128);
    assert_closed(&m);
    assert_outward(&m);
    let reversed: Vec<[f64; 2]> = p.iter().rev().copied().collect();
    let r = lathe(&reversed, 64, 1e-7);
    assert_closed(&r);
    assert_outward(&r);
}

#[test]
fn sweep_tubes_and_closed_beads() {
    let path: Vec<[f64; 3]> = (0..=40)
        .map(|i| {
            let t = i as f64 / 40.0;
            [t, (t * 6.0).sin() * 0.2, (t * 4.0).cos() * 0.1]
        })
        .collect();
    let m = sweep(&path, &[0.02], 12, false, true);
    assert_sane(&m);
    assert_eq!(m.triangle_count(), 40 * 12 * 2 + 20);
    assert_closed(&m);
    assert_outward(&m);
    let open = sweep(&path, &[0.02, 0.01], 10, false, false);
    assert_eq!(open.triangle_count(), 40 * 10 * 2);
    assert!(volume(&open) > 0.0);

    let ring: Vec<[f64; 3]> = circle([0.0, 0.0], 1.0, 64)
        .0
        .iter()
        .map(|p| [p[0], p[1], 0.0])
        .collect();
    let bead = sweep(&ring, &[0.1], 16, true, false);
    assert_sane(&bead);
    assert_eq!(bead.triangle_count(), 64 * 16 * 2);
    assert_closed(&bead);
    assert_outward(&bead);
    assert!(close(volume(&bead), 2.0 * PI * PI * 0.01, 0.03));

    let knot: Vec<[f64; 3]> = (0..120)
        .map(|i| {
            let t = 2.0 * PI * i as f64 / 120.0;
            [
                (2.0 + (3.0 * t).cos()) * (2.0 * t).cos(),
                (2.0 + (3.0 * t).cos()) * (2.0 * t).sin(),
                (3.0 * t).sin(),
            ]
        })
        .collect();
    let k = sweep(&knot, &[0.3], 12, true, false);
    assert_closed(&k);
    assert_outward(&k);
    assert!(min_area(&k) > 1e-6);
}

#[test]
fn profiled_extrusion_is_closed_and_outward() {
    let o = rounded_rect([0.0, 0.0, 0.2, 0.1], 0.02, 12);
    for profile in [
        Profile::round(0.005, 6),
        Profile::NONE,
        Profile {
            kind: ProfileKind::Chamfer,
            width: 0.004,
            segments: 1,
        },
    ] {
        let m = extrude(&o, 0.0, 0.03, &profile, UvMode::Local);
        assert_sane(&m);
        assert_closed(&m);
        assert_outward(&m);
        assert_bounds(&m, [0.0, 0.0, -0.03], [0.2, 0.1, 0.0]);
        let uv = m.uvs.as_ref().unwrap();
        assert!(
            uv.iter()
                .all(|u| (0.0..=1.0).contains(&u[0]) && (0.0..=1.0).contains(&u[1]))
        );
    }
    let disc = extrude(
        &circle([0.0, 0.0], 0.01, 64),
        -0.003,
        0.006,
        &Profile::round(0.002, 5),
        UvMode::None,
    );
    assert_closed(&disc);
    assert_outward(&disc);
    assert!(disc.uvs.is_none());
    let flat_side = extrude(&o, 0.03, 0.0, &Profile::NONE, UvMode::None);
    assert_eq!(flat_side.triangle_count(), 2 * 52 + 2 * 50);
}

#[test]
fn transform_weld_append_and_area() {
    let mut m = rounded_box([1.0, 1.0, 1.0], 0.0, 0);
    transform(&mut m, DMat4::from_scale(DVec3::new(-2.0, 1.0, 1.0)));
    assert_closed(&m);
    assert_outward(&m);
    assert!(close(area(&m), 10.0, 1e-9));
    let mut flat_box = m.clone();
    flat(&mut flat_box);
    weld(&mut flat_box, 1e-6);
    assert_eq!(flat_box.positions.len(), 8);
    assert_eq!(flat_box.triangle_count(), 12);
    let mut both = m.clone();
    both.set_material("a");
    let mut other = sphere(0.5, 8, 4);
    other.set_material("b");
    both.append(&other);
    assert_eq!(both.slots.len(), 2);
    assert_eq!(both.slots[1].start, 36);
    assert_eq!(both.triangle_count(), 12 + other.triangle_count());
    assert_eq!(both.uvs.as_ref().map(Vec::len), Some(both.positions.len()));
    assert_sane(&both);
}

#[test]
fn a_bead_rings_an_outline_offset_outward() {
    let square = rounded_rect([-1.0, -0.5, 1.0, 0.5], 0.2, 8);
    let m = bead(&square, 0.05, 0.1, 0.02, 16);
    assert_sane(&m);
    assert_closed(&m);
    assert_outward(&m);
    assert_eq!(m.triangle_count(), 36 * 16 * 2);
    assert_bounds(&m, [-1.07, -0.57, -0.15], [1.07, 0.57, -0.05]);
    let reversed = Outline(square.0.iter().rev().copied().collect());
    let again = bead(&reversed, 0.05, 0.1, 0.02, 16);
    assert_bounds(&again, [-1.07, -0.57, -0.15], [1.07, 0.57, -0.05]);
    assert_outward(&again);
    assert_eq!(bytes(&m), bytes(&bead(&square, 0.05, 0.1, 0.02, 16)));
}

#[test]
fn a_sheet_lifts_curls_cups_and_jags() {
    let base = Sheet {
        size: [0.4, 0.8],
        cells: [8, 16],
        ..Sheet::default()
    };
    let plain = sheet(&base);
    assert_eq!(bytes(&plain), bytes(&grid([0.4, 0.8], [8, 16], None)));
    let wrapped = sheet(&Sheet {
        bend: Some(Bend::Wrap(1.0)),
        ..base.clone()
    });
    assert_eq!(
        bytes(&wrapped),
        bytes(&grid([0.4, 0.8], [8, 16], Some(1.0)))
    );
    let lifted = sheet(&Sheet {
        bend: Some(Bend::Lift(1.0)),
        ..base.clone()
    });
    let sag = 1.0 - (1.0f64 - 0.16).sqrt();
    assert_bounds(&lifted, [-0.2, 0.0, -0.4], [0.2, sag, 0.4]);
    let shaped = sheet(&Sheet {
        bend: Some(Bend::Lift(1.0)),
        curl: Some([0.05, 0.004]),
        cup: Some([0.5, 0.002]),
        jag: vec![0.001, 0.003],
        ..base
    });
    assert_sane(&shaped);
    assert_eq!(shaped.triangle_count(), 8 * 16 * 2);
    assert_bounds(&shaped, [-0.203, -0.006, -0.4], [0.203, sag, 0.4]);
    assert!(shaped.normals.iter().all(|n| n[1] > 0.5));
}

#[test]
fn everything_twice_gives_the_same_bytes() {
    let build = || {
        let mut all = vec![
            cylinder(0.5, 2.0, 64, true),
            rounded_box([1.0, 2.0, 3.0], 0.2, 6),
            sphere(1.0, 24, 12),
            torus(1.0, 0.2, 24, 12),
            capsule(0.3, 1.0, 16, 4),
            disc(1.0, 32),
            grid([1.0, 2.0], [3, 5], Some(1.5)),
            lathe(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], 32, 1e-7),
            sweep(
                &[
                    [0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [1.0, 1.0, 0.0],
                    [2.0, 1.0, 1.0],
                ],
                &[0.1],
                8,
                false,
                true,
            ),
            extrude(
                &rounded_rect([0.0, 0.0, 1.0, 0.5], 0.1, 12),
                0.0,
                0.2,
                &Profile::round(0.03, 4),
                UvMode::Local,
            ),
            pocket(
                &rounded_rect([0.0, 0.0, 1.0, 0.5], 0.1, 12),
                0.0,
                0.2,
                &Profile::round(0.03, 4),
                &[Pocket {
                    outline: circle([0.5, 0.25], 0.1, 32),
                    depth: 0.05,
                    rim: Profile::round(0.01, 3),
                    floor: Profile::round(0.01, 3),
                }],
                UvMode::Local,
            ),
            bead(&circle([0.0, 0.0], 0.5, 40), 0.02, 0.01, 0.005, 8),
            sheet(&Sheet {
                size: [0.2, 0.3],
                cells: [6, 9],
                bend: Some(Bend::Lift(0.4)),
                curl: Some([0.02, 0.004]),
                cup: Some([0.7, 0.002]),
                jag: vec![0.001, 0.002, 0.003],
            }),
        ];
        let mut m = all.remove(0);
        for x in &all {
            m.append(x);
        }
        smooth_by_angle(&mut m, 35.0);
        bytes(&m)
    };
    assert_eq!(build(), build());
}

#[test]
fn natural_order_is_blenders_deck_order() {
    let deck = [
        "backlight",
        "bumper",
        "cap0",
        "cap1",
        "cap2",
        "cap3",
        "coffee",
        "desk",
        "drum_bottom",
        "drum_bottom_rim",
        "drum_top",
        "drum_top_rim",
        "film",
        "flange0",
        "flange1",
        "flange2",
        "flange3",
        "foot_left",
        "foot_right",
        "gasket",
        "hub0",
        "hub1",
        "hub2",
        "hub3",
        "led_strip0",
        "led_strip1",
        "lflange0",
        "lflange1",
        "lflange2",
        "lflange3",
        "lhub0",
        "lhub1",
        "lhub2",
        "lhub3",
        "lip_page",
        "lip_record",
        "lip_spool",
        "lip_tray",
        "mug_body",
        "mug_handle",
        "mug_rim",
        "mv_key_cut",
        "mv_key_export",
        "mv_key_import",
        "mv_knob",
        "mv_knob__cap",
        "mv_knob__dot",
        "mv_knob__face",
        "mv_marker",
        "mv_page",
        "mv_page__button0",
        "mv_page__button1",
        "mv_page__button2",
        "mv_page__button3",
        "mv_page__button4",
        "mv_page__photo",
        "mv_page__print",
        "mv_page__tape",
        "mv_playhead",
        "mv_playhead__needle",
        "mv_record",
        "mv_record__label",
        "mv_roller",
        "mv_strip__card0",
        "mv_strip__card1",
        "mv_strip__card2",
        "mv_strip__card3",
        "mv_strip__card4",
        "mv_tape",
        "post_left",
        "post_right",
        "rail_front",
        "ruler",
        "screen_glass",
        "screen_mask",
        "search",
        "search_bezel",
        "search_field",
        "shaft0",
        "shaft1",
        "shaft2",
        "shaft3",
        "shell",
        "slat0",
        "slat1",
        "slat2",
        "slat3",
        "slat4",
        "slat5",
        "slat6",
        "slat7",
        "slat8",
        "slat9",
        "slat10",
        "slat11",
        "slat12",
        "slat13",
        "slat14",
        "slider",
        "slider_knob",
        "spool_floor",
        "spool_leather",
        "tape_line",
        "tape_slot_bottom",
        "tape_slot_top",
        "timeline_bed",
        "timeline_light",
        "timeline_marks",
        "tray_back",
        "wall",
        "wall_bottom",
        "wall_left",
        "wall_right",
        "wall_top",
        "wheel0",
        "wheel1",
        "wheel2",
        "wheel3",
        "wrap0",
        "wrap1",
        "wrap2",
        "wrap3",
    ];
    let mut shuffled: Vec<&str> = deck.to_vec();
    shuffled.reverse();
    shuffled.rotate_left(37);
    shuffled.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(shuffled, deck);
    let bench = [
        "ball",
        "desk",
        "key_cream",
        "key_mustard",
        "key_oxblood",
        "knob",
        "led",
        "roller",
        "sheet",
        "shell",
        "tray",
        "wall",
    ];
    let mut b: Vec<&str> = bench.iter().rev().copied().collect();
    b.sort_by(|a, c| natural_cmp(a, c));
    assert_eq!(b, bench);
}
