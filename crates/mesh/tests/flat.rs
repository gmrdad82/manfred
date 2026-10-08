#[allow(dead_code)]
mod common;

use common::*;
use glam::Vec3;
use manfred_mesh::outline::rounded_rect;
use manfred_mesh::*;
use std::collections::BTreeSet;

fn strip() -> Mesh {
    extrude(
        &rounded_rect([-0.4, -0.04, 0.4, 0.04], 0.01, 8),
        0.0,
        0.02,
        &Profile::round(0.004, 4),
        UvMode::None,
    )
}

fn plate() -> Mesh {
    let well = Pocket {
        outline: rounded_rect([-0.1, -0.05, 0.1, 0.05], 0.01, 8),
        depth: 0.012,
        rim: Profile::round(0.002, 4),
        floor: Profile::round(0.002, 4),
    };
    pocket(
        &rounded_rect([-0.4, -0.2, 0.4, 0.2], 0.02, 8),
        0.0,
        0.04,
        &Profile::round(0.004, 4),
        &[well],
        UvMode::None,
    )
}

fn smoothed(m: &Mesh, flat: bool) -> Mesh {
    let mut m = m.clone();
    if flat {
        smooth_by_angle_flat_faces(&mut m, 35.0);
    } else {
        smooth_by_angle(&mut m, 35.0);
    }
    m
}

fn corners(m: &Mesh) -> impl Iterator<Item = ([u32; 3], Vec3, Vec3)> + '_ {
    m.indices.chunks_exact(3).flat_map(move |t| {
        let face = m.face_normal(t).as_vec3();
        t.iter().map(move |&i| {
            (
                m.positions[i as usize].map(f32::to_bits),
                Vec3::from(m.normals[i as usize]),
                face,
            )
        })
    })
}

fn on_axis(face: Vec3) -> bool {
    face.abs().max_element() > 1.0 - 1e-6
}

fn tilt(m: &Mesh, floor: Option<f32>) -> f32 {
    corners(m)
        .filter(|(p, _, face)| on_axis(*face) && floor.is_none_or(|z| f32::from_bits(p[2]) == z))
        .map(|(_, n, face)| n.angle_between(face).to_degrees())
        .fold(0.0, f32::max)
}

#[test]
fn every_vertex_of_a_flat_face_has_the_faces_own_normal() {
    let base = strip();
    let flat = smoothed(&base, true);
    assert_sane(&flat);
    assert_closed(&flat);
    let mut faces = 0;
    for (_, n, face) in corners(&flat) {
        if on_axis(face) {
            faces += 1;
            assert!((n - face).length() < 1e-6, "{n:?} against {face:?}");
        }
    }
    assert!(faces > 12);
    assert!(tilt(&smoothed(&base, false), None) > 4.0);
    assert!(tilt(&flat, None) < 1e-4);
}

#[test]
fn the_bevels_middle_vertices_are_unchanged() {
    let base = strip();
    let (flat, smooth) = (smoothed(&base, true), smoothed(&base, false));
    let faced = |m: &Mesh| -> BTreeSet<[u32; 3]> {
        corners(m)
            .filter(|(_, _, face)| on_axis(*face))
            .map(|(p, _, _)| p)
            .collect()
    };
    let touched = faced(&flat);
    assert_eq!(touched, faced(&smooth));
    let rest = |m: &Mesh| -> BTreeSet<([u32; 3], [u32; 3])> {
        corners(m)
            .filter(|(p, _, _)| !touched.contains(p))
            .map(|(p, n, _)| (p, n.to_array().map(f32::to_bits)))
            .collect()
    };
    let middle = rest(&flat);
    assert!(middle.len() > 20);
    assert_eq!(middle, rest(&smooth));
}

#[test]
fn the_bevels_own_segments_stay_smooth_among_themselves() {
    let flat = smoothed(&strip(), true);
    let smooth_corners = corners(&flat)
        .filter(|(_, n, face)| !on_axis(*face) && n.angle_between(*face).to_degrees() > 1.0)
        .count();
    assert!(smooth_corners > 40);
}

#[test]
fn the_same_input_gives_the_same_bytes() {
    let base = strip();
    assert_eq!(bytes(&smoothed(&base, true)), bytes(&smoothed(&base, true)));
    assert_eq!(
        bytes(&smoothed(&base, false)),
        bytes(&smoothed(&base, false))
    );
    assert_ne!(
        bytes(&smoothed(&base, true)),
        bytes(&smoothed(&base, false))
    );
}

#[test]
fn a_plate_with_a_pocket_keeps_its_floor_flat() {
    let base = plate();
    let flat = smoothed(&base, true);
    assert_sane(&flat);
    let floor = Some(-0.012_f32);
    assert!(corners(&flat).any(|(p, _, face)| on_axis(face) && f32::from_bits(p[2]) == -0.012));
    assert!(tilt(&flat, floor) < 1e-4);
    assert!(tilt(&flat, None) < 1e-4);
    assert!(tilt(&smoothed(&base, false), floor) > 1.0);
    assert!(tilt(&smoothed(&base, false), None) > 4.0);
}
