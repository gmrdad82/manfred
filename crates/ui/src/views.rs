use glam::DVec3;
use manfred_recipe::Ortho;
use manfred_scene::Eye;

use crate::orbit;
use crate::viewport::Solid;

const MATCH: f64 = 0.9999;
const ROOM: f64 = 1.15;
const EMPTY: f64 = 4.0;
const BACK: f64 = 4.0;

pub fn ortho_of(eye: &Eye) -> Option<Ortho> {
    eye.ortho_height?;
    let basis = orbit::basis(eye);
    Ortho::ALL.into_iter().find(|view| {
        basis.forward.dot(DVec3::from_array(view.forward())) > MATCH
            && basis.up.dot(DVec3::from_array(view.up())) > MATCH
    })
}

pub fn standard(view: Ortho, solids: &[&Solid], aspect: f64, fov_y_deg: f64) -> Eye {
    let (min, max) = solids.iter().fold(
        (DVec3::INFINITY, DVec3::NEG_INFINITY),
        |(min, max), solid| (min.min(solid.min), max.max(solid.max)),
    );
    let (centre, size) = if min.is_finite() && max.is_finite() {
        ((min + max) * 0.5, max - min)
    } else {
        (DVec3::ZERO, DVec3::splat(EMPTY))
    };
    let forward = DVec3::from_array(view.forward());
    let up = DVec3::from_array(view.up());
    let right = forward.cross(up);
    let across = size.dot(right.abs());
    let tall = size.dot(up.abs());
    let height = (tall.max(across / aspect.max(1e-6)) * ROOM).max(0.01);
    let reach = size.length().max(1.0) * BACK;
    Eye {
        position: (centre - forward * reach).to_array(),
        target: centre.to_array(),
        up: up.to_array(),
        fov_y_deg,
        shift: [0.0, 0.0],
        ortho_height: Some(height),
    }
}

pub fn perspective(eye: &Eye) -> Eye {
    let Some(height) = eye.ortho_height else {
        return *eye;
    };
    let target = DVec3::from_array(eye.target);
    let forward = orbit::basis(eye).forward;
    let distance = height * 0.5 / (eye.fov_y_deg.to_radians() * 0.5).tan().max(1e-6);
    Eye {
        position: (target - forward * distance).to_array(),
        ortho_height: None,
        ..*eye
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_standard_view_is_orthographic_frames_the_solids_and_is_recognised() {
        let solid = Solid {
            min: DVec3::new(-1.0, 0.0, -2.0),
            max: DVec3::new(1.0, 1.4, 2.5),
            objects: vec![0],
            name: String::new(),
        };
        for view in Ortho::ALL {
            let eye = standard(view, &[&solid], 1.6, 40.0);
            assert_eq!(ortho_of(&eye), Some(view), "{view:?}");
            assert!(eye.ortho_height.unwrap() >= 1.4);
            assert_eq!(ortho_of(&perspective(&eye)), None);
        }
        let side = standard(Ortho::Side, &[&solid], 1.6, 40.0);
        let right = orbit::basis(&side).right;
        assert!(
            right.dot(DVec3::Z) > 0.999,
            "the side view runs +Z to the right"
        );
        let top = standard(Ortho::Top, &[&solid], 1.6, 40.0);
        assert!(orbit::basis(&top).right.dot(DVec3::Z) > 0.999);
    }
}
