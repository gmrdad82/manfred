use glam::{DQuat, DVec3};
use manfred_scene::Eye;

pub const DEGREES_PER_POINT: f64 = 0.25;
pub const DOLLY_PER_POINT: f64 = 0.002;
pub const NEAREST: f64 = 0.001;
const WORLD_UP: DVec3 = DVec3::Y;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Basis {
    pub right: DVec3,
    pub up: DVec3,
    pub forward: DVec3,
}

pub fn basis(eye: &Eye) -> Basis {
    let position = DVec3::from_array(eye.position);
    let target = DVec3::from_array(eye.target);
    let forward = (target - position).try_normalize().unwrap_or(DVec3::NEG_Z);
    let up = DVec3::from_array(eye.up);
    let right = forward
        .cross(up)
        .try_normalize()
        .unwrap_or_else(|| forward.any_orthonormal_vector());
    Basis {
        right,
        up: right.cross(forward),
        forward,
    }
}

pub fn turn(eye: &Eye, points: [f64; 2]) -> Eye {
    let position = DVec3::from_array(eye.position);
    let target = DVec3::from_array(eye.target);
    let yaw = DQuat::from_axis_angle(WORLD_UP, -(points[0] * DEGREES_PER_POINT).to_radians());
    let offset = yaw * (position - target);
    let up = yaw * DVec3::from_array(eye.up);
    let turned = Eye {
        position: (target + offset).to_array(),
        up: up.to_array(),
        ..*eye
    };
    let right = basis(&turned).right;
    let pitch = DQuat::from_axis_angle(right, -(points[1] * DEGREES_PER_POINT).to_radians());
    let offset = pitch * offset;
    let up = pitch * basis(&turned).up;
    Eye {
        position: (target + offset).to_array(),
        up: up.to_array(),
        ..*eye
    }
}

pub fn span(eye: &Eye, height_points: f64) -> f64 {
    if height_points <= 0.0 {
        return 0.0;
    }
    let tall = match eye.ortho_height {
        Some(height) => height,
        None => {
            let distance =
                (DVec3::from_array(eye.position) - DVec3::from_array(eye.target)).length();
            2.0 * distance * (eye.fov_y_deg.to_radians() / 2.0).tan()
        }
    };
    tall / height_points
}

pub fn pan(eye: &Eye, points: [f64; 2], height_points: f64) -> Eye {
    let frame = basis(eye);
    let metres = span(eye, height_points);
    let shift = (-points[0] * frame.right + points[1] * frame.up) * metres;
    Eye {
        position: (DVec3::from_array(eye.position) + shift).to_array(),
        target: (DVec3::from_array(eye.target) + shift).to_array(),
        ..*eye
    }
}

pub fn dolly(eye: &Eye, points: f64) -> Eye {
    let target = DVec3::from_array(eye.target);
    let offset = DVec3::from_array(eye.position) - target;
    let distance = offset.length();
    let factor = (-points * DOLLY_PER_POINT).exp();
    if let Some(height) = eye.ortho_height {
        return Eye {
            ortho_height: Some((height * factor).max(NEAREST)),
            ..*eye
        };
    }
    let Some(direction) = offset.try_normalize() else {
        return *eye;
    };
    let nearer = (distance * factor).max(NEAREST.min(distance));
    Eye {
        position: (target + direction * nearer).to_array(),
        ..*eye
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eye() -> Eye {
        Eye {
            position: [0.0, 0.0, 2.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 40.0,
            shift: [0.0, 0.0],
            ortho_height: None,
        }
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-9)
    }

    fn offset(eye: &Eye) -> DVec3 {
        DVec3::from_array(eye.position) - DVec3::from_array(eye.target)
    }

    #[test]
    fn a_sideways_drag_turns_around_the_target_by_its_angle() {
        let start = eye();
        let turned = turn(&start, [120.0, 0.0]);
        let angle = offset(&start).angle_between(offset(&turned)).to_degrees();
        assert!((angle - 120.0 * DEGREES_PER_POINT).abs() < 1e-9, "{angle}");
        assert_eq!(turned.target, start.target);
        assert!((offset(&turned).length() - 2.0).abs() < 1e-12);
        assert!(turned.position[0] < 0.0, "{:?}", turned.position);
        assert!(close(turned.up, start.up));
    }

    #[test]
    fn a_drag_down_lifts_the_eye_by_its_angle() {
        let start = eye();
        let turned = turn(&start, [0.0, 80.0]);
        let angle = offset(&start).angle_between(offset(&turned)).to_degrees();
        assert!((angle - 80.0 * DEGREES_PER_POINT).abs() < 1e-9, "{angle}");
        assert!(turned.position[1] > 0.0, "{:?}", turned.position);
        let frame = basis(&turned);
        assert!(frame.up.dot(frame.forward).abs() < 1e-12);
    }

    #[test]
    fn turning_back_returns_home() {
        let start = Eye {
            position: [0.3, 1.1, 2.5],
            target: [0.1, 0.2, -0.4],
            ..eye()
        };
        let there = turn(&start, [37.0, 0.0]);
        let back = turn(&there, [-37.0, 0.0]);
        assert!(close(back.position, start.position));
    }

    #[test]
    fn a_straight_down_camera_still_turns() {
        let plan = Eye {
            position: [0.0, 10.0, 0.0],
            up: [0.0, 0.0, -1.0],
            ortho_height: Some(2.2),
            ..eye()
        };
        let turned = turn(&plan, [0.0, 40.0]);
        assert!(turned.position.iter().all(|value| value.is_finite()));
        let angle = offset(&plan).angle_between(offset(&turned)).to_degrees();
        assert!((angle - 10.0).abs() < 1e-9, "{angle}");
    }

    #[test]
    fn dolly_never_crosses_the_target() {
        let mut moved = eye();
        for _ in 0..2000 {
            moved = dolly(&moved, 400.0);
            let along = offset(&moved).dot(offset(&eye()));
            assert!(along > 0.0);
        }
        assert!((offset(&moved).length() - NEAREST).abs() < 1e-12);
        let away = dolly(&eye(), -100.0);
        assert!(offset(&away).length() > 2.0);
        let ortho = Eye {
            ortho_height: Some(2.0),
            ..eye()
        };
        let zoomed = dolly(&ortho, 100.0);
        assert_eq!(zoomed.position, ortho.position);
        assert!(zoomed.ortho_height.unwrap() < 2.0);
    }

    #[test]
    fn a_pan_moves_eye_and_target_together_by_the_view_span() {
        let start = eye();
        let moved = pan(&start, [100.0, 0.0], 500.0);
        let tall = 2.0 * 2.0 * 20.0_f64.to_radians().tan();
        let expected = -100.0 * tall / 500.0;
        assert!((moved.position[0] - expected).abs() < 1e-12);
        assert!((moved.target[0] - expected).abs() < 1e-12);
        assert!(close(offset(&moved).to_array(), offset(&start).to_array()));
        let up = pan(&start, [0.0, 50.0], 500.0);
        assert!(up.target[1] > 0.0);
    }
}
