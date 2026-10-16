use crate::Camera;
use manfred_mesh::math::Real;

#[derive(Clone, Debug, PartialEq)]
pub struct CameraPath {
    pub kind: PathKind,
    pub ease: Ease,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PathKind {
    Orbit(Orbit),
    Keys(Vec<PathKey>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Orbit {
    pub target: [f64; 3],
    pub turns: f64,
    pub rise_deg: f64,
    pub radius_scale: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathKey {
    pub t: f64,
    pub position: [f64; 3],
    pub target: [f64; 3],
    pub focal_mm: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ease {
    Linear,
    Smooth,
}

impl Ease {
    pub fn apply(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::Smooth => t * t * (3.0 - 2.0 * t),
        }
    }
}

impl CameraPath {
    pub fn time(&self, frame: u32, frames: u32) -> f64 {
        match self.kind {
            PathKind::Orbit(_) if frames > 0 => f64::from(frame) / f64::from(frames),
            PathKind::Keys(_) if frames > 1 => f64::from(frame) / f64::from(frames - 1),
            _ => 0.0,
        }
    }
}

impl Orbit {
    pub fn elevation_deg(&self, position: [f64; 3]) -> Option<f64> {
        let v = sub(position, self.target);
        let flat = v[0].hypot_of(v[2]);
        (flat > 1e-9).then(|| v[1].atan2_of(flat).to_degrees())
    }

    fn place(&self, position: [f64; 3], s: f64) -> [f64; 3] {
        let v = sub(position, self.target).map(|x| x * self.radius_scale);
        let flat = v[0].hypot_of(v[2]);
        let (rise_sin, rise_cos) = (self.rise_deg * s).to_radians().sin_cos_of();
        let lifted_flat = flat * rise_cos - v[1] * rise_sin;
        let height = flat * rise_sin + v[1] * rise_cos;
        let k = if flat > 0.0 { lifted_flat / flat } else { 0.0 };
        let (x, z) = (v[0] * k, v[2] * k);
        let (sin, cos) = (std::f64::consts::TAU * self.turns * s).sin_cos_of();
        [
            self.target[0] + (x * cos + z * sin),
            self.target[1] + height,
            self.target[2] + (z * cos - x * sin),
        ]
    }
}

impl Camera {
    pub fn at(&self, t: f64) -> Camera {
        let Some(path) = &self.path else {
            return self.clone();
        };
        let s = path.ease.apply(t);
        let mut camera = Camera {
            path: None,
            ..self.clone()
        };
        match &path.kind {
            PathKind::Orbit(orbit) => {
                camera.position = orbit.place(self.position, s);
                camera.target = orbit.target;
            }
            PathKind::Keys(keys) => {
                let own = PathKey {
                    t: 0.0,
                    position: self.position,
                    target: self.target,
                    focal_mm: None,
                };
                let first = keys.first().filter(|k| k.t <= 0.0);
                let all: Vec<&PathKey> = first
                    .is_none()
                    .then_some(&own)
                    .into_iter()
                    .chain(keys.iter())
                    .collect();
                let focal = |key: &PathKey| key.focal_mm.or(self.focal_mm);
                let (a, b, u) = segment(&all, s);
                camera.position = lerp3(a.position, b.position, u);
                camera.target = lerp3(a.target, b.target, u);
                if let (Some(fa), Some(fb)) = (focal(a), focal(b)) {
                    camera.focal_mm = Some(fa + (fb - fa) * u);
                }
            }
        }
        camera
    }
}

fn segment<'a>(keys: &[&'a PathKey], s: f64) -> (&'a PathKey, &'a PathKey, f64) {
    let last = keys[keys.len() - 1];
    if s >= last.t {
        return (last, last, 0.0);
    }
    let at = keys.iter().rposition(|k| k.t <= s).unwrap_or(0);
    let (a, b) = (keys[at], keys[(at + 1).min(keys.len() - 1)]);
    let span = b.t - a.t;
    let u = if span > 0.0 {
        ((s - a.t) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a, b, u)
}

fn lerp3(a: [f64; 3], b: [f64; 3], u: f64) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * u)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Projection;

    fn camera(path: Option<CameraPath>) -> Camera {
        Camera {
            name: "main".into(),
            kind: Projection::Perspective,
            position: [0.0, 1.0, 2.0],
            target: [0.0, 1.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: None,
            focal_mm: Some(50.0),
            sensor_mm: Some(36.0),
            shift: [0.0, 0.0],
            focus_m: None,
            fstop: 0.0,
            ortho_height: None,
            path,
            rig: None,
        }
    }

    fn orbit(turns: f64, rise_deg: f64, radius_scale: f64, ease: Ease) -> Camera {
        camera(Some(CameraPath {
            kind: PathKind::Orbit(Orbit {
                target: [0.0, 1.0, 0.0],
                turns,
                rise_deg,
                radius_scale,
            }),
            ease,
        }))
    }

    fn close(a: [f64; 3], b: [f64; 3]) {
        for k in 0..3 {
            assert!((a[k] - b[k]).abs() < 1e-12, "{a:?} against {b:?}");
        }
    }

    #[test]
    fn an_orbit_starts_on_the_camera_and_turns_counter_clockwise_from_above() {
        let c = orbit(1.0, 0.0, 1.0, Ease::Linear);
        assert_eq!(c.at(0.0).position, c.position);
        assert_eq!(c.at(0.0).target, c.target);
        assert_eq!(c.at(0.0).path, None);
        close(c.at(0.25).position, [2.0, 1.0, 0.0]);
        close(c.at(0.5).position, [0.0, 1.0, -2.0]);
        close(c.at(0.75).position, [-2.0, 1.0, 0.0]);
    }

    #[test]
    fn an_orbit_rises_and_scales_its_radius() {
        let c = orbit(0.0, 90.0, 1.0, Ease::Linear);
        close(
            c.at(0.5).position,
            [0.0, 1.0 + 2.0f64.sqrt(), 2.0f64.sqrt()],
        );
        let wide = orbit(0.25, 0.0, 1.5, Ease::Linear);
        close(wide.at(0.0).position, [0.0, 1.0, 3.0]);
        close(wide.at(1.0).position, [3.0, 1.0, 0.0]);
    }

    #[test]
    fn an_orbit_frame_is_its_share_of_the_loop_and_keys_reach_their_last_key() {
        let c = orbit(1.0, 0.0, 1.0, Ease::Linear);
        let path = c.path.as_ref().unwrap();
        assert_eq!(path.time(0, 24), 0.0);
        assert_eq!(path.time(6, 24), 0.25);
        assert_eq!(path.time(23, 24), 23.0 / 24.0);
        close(c.at(path.time(6, 24)).position, [2.0, 1.0, 0.0]);
        let last = c.at(path.time(23, 24)).position;
        let angle = std::f64::consts::TAU * 23.0 / 24.0;
        close(last, [2.0 * angle.sin_of(), 1.0, 2.0 * angle.cos_of()]);
        let keys = CameraPath {
            kind: PathKind::Keys(Vec::new()),
            ease: Ease::Linear,
        };
        assert_eq!(keys.time(0, 3), 0.0);
        assert_eq!(keys.time(2, 3), 1.0);
        assert_eq!(keys.time(0, 1), 0.0);
    }

    #[test]
    fn smooth_eases_in_and_out() {
        assert_eq!(Ease::Smooth.apply(0.0), 0.0);
        assert_eq!(Ease::Smooth.apply(0.5), 0.5);
        assert_eq!(Ease::Smooth.apply(1.0), 1.0);
        assert!((Ease::Smooth.apply(0.25) - 0.15625).abs() < 1e-15);
        assert_eq!(Ease::Linear.apply(0.25), 0.25);
        let c = orbit(1.0, 0.0, 1.0, Ease::Smooth);
        let angle = std::f64::consts::TAU * 0.15625;
        close(
            c.at(0.25).position,
            [2.0 * angle.sin_of(), 1.0, 2.0 * angle.cos_of()],
        );
    }

    #[test]
    fn keys_start_from_the_camera_unless_a_key_sits_at_zero() {
        let key = |t: f64, x: f64, focal_mm: Option<f64>| PathKey {
            t,
            position: [x, 1.0, 2.0],
            target: [x, 1.0, 0.0],
            focal_mm,
        };
        let c = camera(Some(CameraPath {
            kind: PathKind::Keys(vec![key(0.5, 4.0, Some(100.0)), key(1.0, 0.0, None)]),
            ease: Ease::Linear,
        }));
        assert_eq!(c.at(0.0).position, c.position);
        assert_eq!(c.at(0.0).focal_mm, Some(50.0));
        close(c.at(0.25).position, [2.0, 1.0, 2.0]);
        close(c.at(0.25).target, [2.0, 1.0, 0.0]);
        assert_eq!(c.at(0.25).focal_mm, Some(75.0));
        close(c.at(0.5).position, [4.0, 1.0, 2.0]);
        assert_eq!(c.at(0.5).focal_mm, Some(100.0));
        close(c.at(0.75).position, [2.0, 1.0, 2.0]);
        assert_eq!(c.at(0.75).focal_mm, Some(75.0));
        close(c.at(1.0).position, [0.0, 1.0, 2.0]);
        let held = camera(Some(CameraPath {
            kind: PathKind::Keys(vec![key(0.0, 3.0, None), key(0.5, 5.0, None)]),
            ease: Ease::Linear,
        }));
        close(held.at(0.0).position, [3.0, 1.0, 2.0]);
        close(held.at(0.25).position, [4.0, 1.0, 2.0]);
        close(held.at(0.9).position, [5.0, 1.0, 2.0]);
    }

    #[test]
    fn a_camera_without_a_path_stays_put() {
        let c = camera(None);
        assert_eq!(c.at(0.7), c);
    }
}
