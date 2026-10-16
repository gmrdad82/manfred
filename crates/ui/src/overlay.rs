use egui::{Color32, Painter, Stroke};
use glam::{DMat4, DVec3};
use manfred_build::Built;
use manfred_recipe::{LightKind, Recipe, Setting, Settings};
use manfred_scene::Eye;

use crate::pick::Lens;

pub const FAR: f64 = 2.5;
const NEAR: f64 = 0.05;
const RING: usize = 32;
const MARKER: f64 = 0.15;
const DOTS: usize = 36;
const DOT_RADIUS: f32 = 1.6;
const SPRAY_DEFAULT: f64 = 1.0;
const CONE_DEFAULT: f64 = 15.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Lamp {
    pub name: String,
    pub position: DVec3,
    pub cone: Option<(DVec3, [f64; 2], f64)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Guides {
    pub path: Vec<DVec3>,
    pub lamps: Vec<Lamp>,
    pub dots: Vec<DVec3>,
}

fn sample(seed: u64, index: u64) -> f64 {
    let mut x = seed ^ index.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

fn across(direction: DVec3) -> (DVec3, DVec3) {
    let side = direction.any_orthonormal_vector();
    (side, direction.cross(side))
}

pub fn ring(apex: DVec3, direction: DVec3, half_degrees: f64, range: f64) -> Vec<DVec3> {
    let direction = direction.try_normalize().unwrap_or(DVec3::NEG_Y);
    let (a, b) = across(direction);
    let angle = half_degrees.to_radians().min(1.55);
    let centre = apex + direction * range * angle.cos();
    let radius = range * angle.sin();
    (0..=RING)
        .map(|k| {
            let turn = std::f64::consts::TAU * k as f64 / RING as f64;
            centre + (a * turn.cos() + b * turn.sin()) * radius
        })
        .collect()
}

pub fn cone(apex: DVec3, direction: DVec3, cone: [f64; 2], range: f64) -> Vec<Vec<DVec3>> {
    let outer = ring(apex, direction, cone[1], range);
    let mut lines: Vec<Vec<DVec3>> = (0..4).map(|k| vec![apex, outer[k * RING / 4]]).collect();
    lines.push(ring(apex, direction, cone[0], range));
    lines.push(outer);
    lines
}

pub fn marker(at: DVec3) -> Vec<Vec<DVec3>> {
    [DVec3::X, DVec3::Y, DVec3::Z]
        .into_iter()
        .map(|axis| vec![at - axis * MARKER, at + axis * MARKER])
        .collect()
}

pub fn frustum(eye: &Eye, aspect: f64, far: f64) -> Vec<Vec<DVec3>> {
    let [right, up, forward] = eye.basis().map(DVec3::from_array);
    let apex = DVec3::from_array(eye.position);
    let (half_y, apex_width) = match eye.ortho_height {
        Some(height) => (height * 0.5, true),
        None => ((eye.fov_y_deg.to_radians() * 0.5).tan() * far, false),
    };
    let half_x = half_y * aspect;
    let corner = |x: f64, y: f64| apex + forward * far + right * (x * half_x) + up * (y * half_y);
    let corners = [
        corner(-1.0, -1.0),
        corner(1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, 1.0),
    ];
    let mut lines: Vec<Vec<DVec3>> = corners
        .iter()
        .map(|far_corner| {
            let near = if apex_width {
                *far_corner - forward * far
            } else {
                apex
            };
            vec![near, *far_corner]
        })
        .collect();
    let mut rim = corners.to_vec();
    rim.push(corners[0]);
    lines.push(rim);
    let top = apex + forward * far + up * (half_y * 1.35);
    lines.push(vec![corners[3], top, corners[2]]);
    lines
}

pub fn spray(seed: u64, at: DVec3, direction: DVec3, cone_degrees: f64, reach: f64) -> Vec<DVec3> {
    let direction = direction.try_normalize().unwrap_or(DVec3::Y);
    let (a, b) = across(direction);
    let spread = cone_degrees.to_radians().min(std::f64::consts::PI);
    (0..DOTS as u64)
        .map(|k| {
            let tilt = spread * sample(seed, 3 * k).sqrt();
            let turn = std::f64::consts::TAU * sample(seed, 3 * k + 1);
            let far = reach * (0.1 + 0.9 * sample(seed, 3 * k + 2));
            let heading = direction * tilt.cos() + (a * turn.cos() + b * turn.sin()) * tilt.sin();
            at + heading * far
        })
        .collect()
}

fn number(settings: &Settings, key: &str) -> Option<f64> {
    settings
        .iter()
        .find(|(name, _)| *name == key)
        .and_then(|(_, value)| match value {
            Setting::Number(number) => Some(*number),
            Setting::Whole(whole) => Some(*whole as f64),
            _ => None,
        })
}

fn triple(settings: &Settings, key: &str) -> Option<DVec3> {
    settings
        .iter()
        .find(|(name, _)| *name == key)
        .and_then(|(_, value)| match value {
            Setting::Numbers(numbers) if numbers.len() == 3 => {
                Some(DVec3::new(numbers[0], numbers[1], numbers[2]))
            }
            _ => None,
        })
}

fn node_world(built: &Built, fps: u32, time: f64, node: &str) -> Option<DMat4> {
    if let Some(track) = built
        .tracks
        .iter()
        .find(|track| track.node == node && !track.keys.is_empty())
    {
        return Some(manfred_build::matrix(&manfred_build::sample(
            &track.keys,
            fps,
            time,
        )));
    }
    built
        .parts
        .iter()
        .find(|part| part.node.name == node)
        .map(|part| part.node.transform)
}

pub fn guides(recipe: &Recipe, built: &Built, at: Option<(u32, f64)>) -> Guides {
    let (fps, time) = at.unwrap_or((1, 0.0));
    let place = |parent: Option<&str>, local: DVec3| match parent {
        Some(node) => node_world(built, fps, time, node).map(|world| world.transform_point3(local)),
        None => Some(local),
    };
    let turn = |parent: Option<&str>, local: DVec3| match parent {
        Some(node) => {
            node_world(built, fps, time, node).map_or(local, |world| world.transform_vector3(local))
        }
        None => local,
    };
    let path = built
        .tracks
        .iter()
        .find(|track| track.camera.is_some())
        .map(|track| {
            track
                .keys
                .iter()
                .map(|key| DVec3::from_array(key.translation))
                .collect()
        })
        .unwrap_or_default();
    let lamps = recipe
        .lights
        .iter()
        .filter_map(|light| {
            let (parent, local) = match &light.parent {
                Some((node, at)) => (Some(node.as_str()), DVec3::from_array(*at)),
                None => (None, DVec3::from_array(light.position)),
            };
            let position = place(parent, local)?;
            let cone = match (light.kind, light.direction, light.cone) {
                (LightKind::Spot, Some(direction), Some(cone)) => Some((
                    turn(parent, DVec3::from_array(direction)),
                    cone,
                    if light.range > 0.0 { light.range } else { FAR },
                )),
                _ => None,
            };
            Some(Lamp {
                name: light.name.clone(),
                position,
                cone,
            })
        })
        .collect();
    let dots = recipe
        .effects
        .particles
        .iter()
        .enumerate()
        .flat_map(|(index, particles)| {
            let parent = particles.parent.as_deref();
            let local = triple(&particles.settings, "at").unwrap_or(DVec3::ZERO);
            let Some(at) = place(parent, local) else {
                return Vec::new();
            };
            let direction = turn(
                parent,
                triple(&particles.settings, "direction").unwrap_or(DVec3::Y),
            );
            let reach = match (
                number(&particles.settings, "speed"),
                number(&particles.settings, "lifetime"),
            ) {
                (Some(speed), Some(life)) => (speed * life).clamp(0.2, 5.0),
                _ => SPRAY_DEFAULT,
            };
            let cone = number(&particles.settings, "cone").unwrap_or(CONE_DEFAULT);
            let seed = number(&particles.settings, "seed").map_or(index as u64, |seed| seed as u64);
            spray(seed, at, direction, cone, reach)
        })
        .collect();
    Guides { path, lamps, dots }
}

pub fn clip(lens: &Lens, from: DVec3, to: DVec3) -> Option<(DVec3, DVec3)> {
    let (a, b) = (lens.depth(from), lens.depth(to));
    let near = NEAR * 1.01;
    if a < near && b < near {
        return None;
    }
    let cut = |inside: DVec3, outside: DVec3, din: f64, dout: f64| {
        inside + (outside - inside) * ((din - near) / (din - dout))
    };
    match (a < near, b < near) {
        (true, false) => Some((cut(to, from, b, a), to)),
        (false, true) => Some((from, cut(from, to, a, b))),
        _ => Some((from, to)),
    }
}

pub fn polyline(painter: &Painter, lens: &Lens, points: &[DVec3], stroke: Stroke) {
    for pair in points.windows(2) {
        let Some((from, to)) = clip(lens, pair[0], pair[1]) else {
            continue;
        };
        if let (Some(a), Some(b)) = (lens.project(from), lens.project(to)) {
            painter.line_segment([a, b], stroke);
        }
    }
}

pub fn dots(painter: &Painter, lens: &Lens, points: &[DVec3], colour: Color32) {
    for point in points {
        if lens.depth(*point) > NEAR
            && let Some(at) = lens.project(*point)
        {
            painter.circle_filled(at, DOT_RADIUS, colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frustum_reaches_its_far_corners_and_a_spray_stays_inside_its_cone() {
        let eye = Eye {
            position: [0.0, 0.0, 0.0],
            target: [0.0, 0.0, -1.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 90.0,
            shift: [0.0, 0.0],
            ortho_height: None,
        };
        let lines = frustum(&eye, 0.5, 2.0);
        let far = lines[0][1];
        assert!((far.z + 2.0).abs() < 1e-9 && (far.y + 2.0).abs() < 1e-9);
        assert!((far.x + 1.0).abs() < 1e-9, "{far:?}");
        let dots = spray(7, DVec3::ZERO, DVec3::Z, 20.0, 1.0);
        assert_eq!(dots, spray(7, DVec3::ZERO, DVec3::Z, 20.0, 1.0));
        assert!(
            dots.iter()
                .all(|dot| dot.normalize().angle_between(DVec3::Z) <= 20f64.to_radians() + 1e-9)
        );
    }
}
