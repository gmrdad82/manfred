use crate::Eye;
use crate::vector::{cross, dot, sub};
use manfred_recipe::{Camera, Projection};
use pfx_live::frame::Matrix;
use pfx_post::dof;

const SENSOR_MM: f64 = 36.0;

impl Eye {
    pub fn from_camera(camera: &Camera, aspect: f64) -> Eye {
        let ortho_height = match camera.kind {
            Projection::Orthographic => Some(camera.ortho_height.unwrap_or(1.0)),
            Projection::Perspective => None,
        };
        Eye {
            position: camera.position,
            target: camera.target,
            up: camera.up,
            fov_y_deg: camera.fov_y_deg(aspect),
            shift: camera.shift,
            ortho_height,
        }
    }

    pub fn lens(&self, camera: &Camera, aspect: f64) -> Option<dof::Lens> {
        let open = camera.fstop.is_finite() && camera.fstop > 0.0;
        if !open || camera.kind == Projection::Orthographic || self.ortho_height.is_some() {
            return None;
        }
        let tan_y = (self.fov_y_deg.to_radians() * 0.5).tan();
        let tan_x = tan_y * aspect;
        let sensor_mm = camera.sensor_mm.unwrap_or(SENSOR_MM);
        let focal_mm = camera
            .focal_mm
            .filter(|focal| *focal > 0.0)
            .unwrap_or_else(|| sensor_mm / (2.0 * tan_y * aspect.max(1.0)));
        let focus_m = camera.focus_m.unwrap_or_else(|| {
            let toward = sub(self.target, self.position);
            dot(toward, toward).sqrt()
        });
        let (focal_m, focus_m) = ((focal_mm / 1000.0) as f32, focus_m as f32);
        let lens = dof::Lens {
            focal_m,
            sensor_m: dof::Lens::sensor_for_view(focal_m, focus_m, tan_x as f32),
            fstop: camera.fstop as f32,
            aperture: 1.0,
            focus_m,
            near_m: focus_m,
            near_blur: 0.0,
            far_m: focus_m,
            far_blur: 0.0,
            ground_m: None,
        };
        let finite = [lens.focal_m, lens.sensor_m, lens.fstop, lens.focus_m]
            .iter()
            .all(|v| v.is_finite());
        (finite && lens.enabled()).then_some(lens)
    }

    pub fn basis(&self) -> [[f64; 3]; 3] {
        let forward = unit(sub(self.target, self.position)).unwrap_or([0.0, 0.0, -1.0]);
        let right = unit(cross(forward, self.up))
            .or_else(|| unit(cross(forward, least_aligned(forward))))
            .unwrap_or([1.0, 0.0, 0.0]);
        let up = cross(right, forward);
        [right, up, forward]
    }

    pub fn view(&self) -> Matrix {
        let [r, u, f] = self.basis();
        let e = self.position;
        [
            [r[0], u[0], -f[0], 0.0],
            [r[1], u[1], -f[1], 0.0],
            [r[2], u[2], -f[2], 0.0],
            [-dot(r, e), -dot(u, e), dot(f, e), 1.0],
        ]
        .map(|column| column.map(|v| v as f32))
    }

    pub fn projection(&self, size: [u32; 2], near: f64, far: f64) -> Matrix {
        let aspect = f64::from(size[0].max(1)) / f64::from(size[1].max(1));
        let [sx, sy] = self.shift;
        let depth = far / (near - far);
        let matrix = match self.ortho_height {
            Some(height) => {
                let half_y = (height * 0.5).max(1e-9);
                let half_x = half_y * aspect;
                [
                    [1.0 / half_x, 0.0, 0.0, 0.0],
                    [0.0, 1.0 / half_y, 0.0, 0.0],
                    [0.0, 0.0, 1.0 / (near - far), 0.0],
                    [-sx, -sy, near / (near - far), 1.0],
                ]
            }
            None => {
                let tan_y = (self.fov_y_deg.to_radians() * 0.5).tan().max(1e-9);
                let tan_x = tan_y * aspect;
                [
                    [1.0 / tan_x, 0.0, 0.0, 0.0],
                    [0.0, 1.0 / tan_y, 0.0, 0.0],
                    [sx, sy, depth, -1.0],
                    [0.0, 0.0, far * near / (near - far), 0.0],
                ]
            }
        };
        matrix.map(|column| column.map(|v| v as f32))
    }

    pub fn depth_range(&self, bounds: [[f64; 3]; 2]) -> (f64, f64) {
        let [_, _, forward] = self.basis();
        let mut nearest = f64::INFINITY;
        let mut farthest = f64::NEG_INFINITY;
        for x in [bounds[0][0], bounds[1][0]] {
            for y in [bounds[0][1], bounds[1][1]] {
                for z in [bounds[0][2], bounds[1][2]] {
                    let along = dot(sub([x, y, z], self.position), forward);
                    nearest = nearest.min(along);
                    farthest = farthest.max(along);
                }
            }
        }
        if !nearest.is_finite() || !farthest.is_finite() {
            return (0.01, 100.0);
        }
        let span = (farthest - nearest).max(1e-3);
        let far = farthest + span * 0.05 + 1e-3;
        let near = (nearest - span * 0.05).max(far * 1e-4);
        (near, far.max(near * 2.0))
    }
}

fn least_aligned(v: [f64; 3]) -> [f64; 3] {
    if v[0].abs() <= v[1].abs() && v[0].abs() <= v[2].abs() {
        [1.0, 0.0, 0.0]
    } else if v[1].abs() <= v[2].abs() {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    }
}

fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(v, v).sqrt();
    (length > 1e-12 && length.is_finite()).then(|| v.map(|x| x / length))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pfx_live::frame::{multiply, transform};

    fn camera(kind: Projection) -> Camera {
        Camera {
            name: "main".into(),
            kind,
            position: [0.0, 0.12, 1.6],
            target: [0.0, 0.12, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: None,
            focal_mm: None,
            sensor_mm: None,
            shift: [0.0, 0.0],
            focus_m: None,
            fstop: 0.0,
            ortho_height: None,
            path: None,
            rig: None,
        }
    }

    fn ndc(eye: &Eye, size: [u32; 2], point: [f64; 3]) -> [f32; 3] {
        let (near, far) = (0.4, 6.0);
        let matrix = multiply(eye.projection(size, near, far), eye.view());
        let clip = transform(
            matrix,
            [point[0] as f32, point[1] as f32, point[2] as f32, 1.0],
        );
        [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]]
    }

    #[test]
    fn a_perspective_camera_with_shift_is_the_hand_computed_eye() {
        let mut shifted = camera(Projection::Perspective);
        shifted.fov_y_deg = Some(13.3778);
        shifted.shift = [0.1, -0.6395];
        let eye = Eye::from_camera(&shifted, 1586.0 / 992.0);
        assert_eq!(
            eye,
            Eye {
                position: [0.0, 0.12, 1.6],
                target: [0.0, 0.12, 0.0],
                up: [0.0, 1.0, 0.0],
                fov_y_deg: 13.3778,
                shift: [0.1, -0.6395],
                ortho_height: None,
            }
        );
        let target = ndc(&eye, [1586, 992], [0.0, 0.12, 0.0]);
        assert!((target[0] + 0.1).abs() < 1e-5, "{target:?}");
        assert!((target[1] - 0.6395).abs() < 1e-5, "{target:?}");
        let tan_y = (13.3778_f64.to_radians() / 2.0).tan();
        let top = ndc(&eye, [1586, 992], [0.0, 0.12 + 1.6 * tan_y, 0.0]);
        assert!((top[1] - (1.0 + 0.6395)).abs() < 1e-4, "{top:?}");
        let tan_x = tan_y * 1586.0 / 992.0;
        let side = ndc(&eye, [1586, 992], [1.6 * tan_x, 0.12, 0.0]);
        assert!((side[0] - (1.0 - 0.1)).abs() < 1e-4, "{side:?}");
        assert!(ndc(&eye, [1586, 992], [0.0, 0.12, 1.2])[2].abs() < 1e-5);
        assert!((ndc(&eye, [1586, 992], [0.0, 0.12, -4.4])[2] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn focal_length_fits_the_sensor_to_the_longest_side() {
        let mut lens = camera(Projection::Perspective);
        lens.focal_mm = Some(50.0);
        lens.sensor_mm = Some(36.0);
        let wide = Eye::from_camera(&lens, 1.6);
        let expected = 2.0 * (0.36_f64 / 1.6).atan().to_degrees();
        assert!((wide.fov_y_deg - expected).abs() < 1e-12);
        let tall = Eye::from_camera(&lens, 0.5);
        assert!((tall.fov_y_deg - 2.0 * 0.36_f64.atan().to_degrees()).abs() < 1e-12);
        lens.sensor_mm = None;
        assert_eq!(Eye::from_camera(&lens, 1.6).fov_y_deg, expected);
    }

    #[test]
    fn an_open_aperture_gives_the_tracers_thin_lens() {
        let mut open = camera(Projection::Perspective);
        open.fov_y_deg = Some(30.0);
        open.fstop = 2.8;
        let eye = Eye::from_camera(&open, 1.6);
        let lens = eye.lens(&open, 1.6).unwrap();
        let tan_y = 15.0_f64.to_radians().tan();
        let focal_m = 0.036 / (2.0 * tan_y * 1.6);
        assert!((f64::from(lens.focal_m) - focal_m).abs() < 1e-7, "{lens:?}");
        assert!((lens.focus_m - 1.6).abs() < 1e-6, "{lens:?}");
        assert_eq!((lens.fstop, lens.aperture), (2.8, 1.0));
        assert_eq!(
            (lens.near_blur, lens.far_blur, lens.ground_m),
            (0.0, 0.0, None)
        );
        let width = 1600;
        for distance in [0.5_f64, 1.2, 1.6, 3.0, 9.0] {
            let diameter = focal_m / 2.8;
            let disc = diameter * (distance - 1.6) / distance;
            let traced = disc * f64::from(width) / (4.0 * tan_y * 1.6 * 1.6);
            let live = f64::from(lens.thin_lens(distance as f32, width));
            assert!(
                (live - traced).abs() < 1e-3,
                "{distance}: {live} against {traced}"
            );
        }
    }

    #[test]
    fn the_lens_takes_the_cameras_focal_length_and_focus() {
        let mut open = camera(Projection::Perspective);
        open.focal_mm = Some(50.0);
        open.sensor_mm = Some(36.0);
        open.focus_m = Some(0.9);
        open.fstop = 4.0;
        let lens = Eye::from_camera(&open, 0.5).lens(&open, 0.5).unwrap();
        assert!((lens.focal_m - 0.05).abs() < 1e-7);
        assert!((lens.focus_m - 0.9).abs() < 1e-7);
        open.focus_m = Some(0.04);
        assert_eq!(Eye::from_camera(&open, 0.5).lens(&open, 0.5), None);
    }

    #[test]
    fn a_closed_aperture_and_an_orthographic_view_have_no_lens() {
        let mut shut = camera(Projection::Perspective);
        assert_eq!(Eye::from_camera(&shut, 1.0).lens(&shut, 1.0), None);
        shut.fstop = f64::NAN;
        assert_eq!(Eye::from_camera(&shut, 1.0).lens(&shut, 1.0), None);
        shut.fstop = -2.0;
        assert_eq!(Eye::from_camera(&shut, 1.0).lens(&shut, 1.0), None);
        let mut plan = camera(Projection::Orthographic);
        plan.fstop = 2.0;
        assert_eq!(Eye::from_camera(&plan, 1.0).lens(&plan, 1.0), None);
        let mut open = camera(Projection::Perspective);
        open.fstop = 2.0;
        let flat = Eye {
            ortho_height: Some(2.0),
            ..Eye::from_camera(&open, 1.0)
        };
        assert_eq!(flat.lens(&open, 1.0), None);
        assert!(Eye::from_camera(&open, 1.0).lens(&open, 1.0).is_some());
    }

    #[test]
    fn an_orthographic_camera_is_the_hand_computed_eye() {
        let mut plan = camera(Projection::Orthographic);
        plan.position = [0.0, 10.0, 0.0];
        plan.target = [0.0, 0.0, 0.0];
        plan.up = [0.0, 0.0, -1.0];
        plan.ortho_height = Some(2.2);
        let eye = Eye::from_camera(&plan, 2.0);
        assert_eq!(eye.ortho_height, Some(2.2));
        assert_eq!(eye.position, [0.0, 10.0, 0.0]);
        assert_eq!(eye.up, [0.0, 0.0, -1.0]);
        let [right, up, forward] = eye.basis();
        assert_eq!(forward, [0.0, -1.0, 0.0]);
        assert_eq!(right, [1.0, 0.0, 0.0]);
        assert_eq!(up, [0.0, 0.0, -1.0]);
        let size = [800, 400];
        let corner = ndc(&eye, size, [2.2, 0.0, -1.1]);
        assert!((corner[0] - 1.0).abs() < 1e-5, "{corner:?}");
        assert!((corner[1] - 1.0).abs() < 1e-5, "{corner:?}");
        let centre = ndc(&eye, size, [0.0, 3.0, 0.0]);
        assert!(centre[0].abs() < 1e-6 && centre[1].abs() < 1e-6);
        let mut shifted = eye;
        shifted.shift = [0.25, -0.5];
        let moved = ndc(&shifted, size, [0.0, 0.0, 0.0]);
        assert!((moved[0] + 0.25).abs() < 1e-5 && (moved[1] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn the_depth_range_holds_the_bounds() {
        let eye = Eye::from_camera(&camera(Projection::Perspective), 1.0);
        let (near, far) = eye.depth_range([[-0.5, 0.0, -0.5], [0.5, 0.3, 0.5]]);
        assert!(near > 0.0 && near < 1.1, "{near}");
        assert!(far > 2.1 && far < 2.3, "{far}");
        let (near, far) = eye.depth_range([[-5.0, -5.0, -5.0], [5.0, 5.0, 5.0]]);
        assert!(near > 0.0 && near < far);
    }

    #[test]
    fn a_degenerate_up_still_gives_a_basis() {
        let mut down = camera(Projection::Perspective);
        down.position = [0.0, 2.0, 0.0];
        down.target = [0.0, 0.0, 0.0];
        let [right, up, forward] = Eye::from_camera(&down, 1.0).basis();
        assert_eq!(forward, [0.0, -1.0, 0.0]);
        assert!((dot(right, right) - 1.0).abs() < 1e-12);
        assert!(dot(right, up).abs() < 1e-12 && dot(up, forward).abs() < 1e-12);
    }
}
