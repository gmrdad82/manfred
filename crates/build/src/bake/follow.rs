use crate::{Bake, Track};
use glam::{DMat4, DQuat, DVec3};
use manfred_glb::Node;
use manfred_motion::{Route, Timeline, Trs, noise1, trs_from_matrix};
use manfred_recipe::{Follow, Recipe};

const STEER_MAX_DEG: f64 = 35.0;
const SLIP_SMOOTH_S: f64 = 0.2;
const CRAWL_M_S: f64 = 1.0;
const WHEELBASE_M: f64 = 2.7;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Drive {
    pub distance: Vec<f64>,
    pub pose: Vec<DMat4>,
}

pub(crate) fn speed_at(keys: &[[f64; 2]], t: f64) -> f64 {
    let Some(first) = keys.first() else {
        return 0.0;
    };
    if t <= first[0] {
        return first[1];
    }
    for pair in keys.windows(2) {
        let [a, b] = [pair[0], pair[1]];
        if t <= b[0] {
            let span = b[0] - a[0];
            if span <= 0.0 {
                return b[1];
            }
            return a[1] + (b[1] - a[1]) * (t - a[0]) / span;
        }
    }
    keys[keys.len() - 1][1]
}

pub(crate) fn distances(follow: &Follow, route: &Route, timeline: &Timeline) -> Vec<f64> {
    let dt = 1.0 / f64::from(timeline.fps.max(1));
    let length = route.length();
    let mut out = Vec::with_capacity(timeline.frames as usize + 1);
    let mut travelled = follow.start;
    let mut before = speed_at(&follow.speed, 0.0);
    out.push(travelled);
    for n in 1..=timeline.frames {
        let now = speed_at(&follow.speed, timeline.time(n));
        travelled += 0.5 * (before + now) * dt;
        before = now;
        out.push(travelled);
    }
    if !route.closed() {
        for d in &mut out {
            *d = d.clamp(0.0, length);
        }
    }
    out
}

fn name_seed(seed: u64, name: &str) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for byte in name.bytes() {
        h ^= u64::from(byte);
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    seed ^ h
}

pub(crate) fn drive(follow: &Follow, route: &Route, timeline: &Timeline) -> Drive {
    let distance = distances(follow, route, timeline);
    let dt = 1.0 / f64::from(timeline.fps.max(1));
    let seed = name_seed(timeline.seed, &follow.object);
    let offset = |t: f64| match follow.drift {
        Some(d) if d.amount != 0.0 && d.period > 0.0 => {
            follow.lane + d.amount * noise1(seed, t / d.period)
        }
        _ => follow.lane,
    };
    let reference = follow
        .drift
        .filter(|d| d.amount > 0.0 && d.period > 0.0)
        .map(|d| d.amount / d.period);
    let slip = follow.drift.map_or(0.0, |d| d.slip_deg.to_radians());
    let ease = 1.0 - (-dt / SLIP_SMOOTH_S).exp();
    let mut yaw = None::<f64>;
    let mut pose = Vec::with_capacity(distance.len());
    for (n, &d) in distance.iter().enumerate() {
        let t = timeline.time(n as u32);
        let lateral = offset(t);
        let target = match reference {
            Some(reference) => {
                let side = (offset(t + dt) - offset(t - dt)) / (2.0 * dt);
                let forward = speed_at(&follow.speed, t).max(CRAWL_M_S);
                side.atan2(forward) + slip * (side / reference).tanh()
            }
            None => 0.0,
        };
        let smooth = match yaw {
            Some(y) => y + (target - y) * ease,
            None => target,
        };
        yaw = Some(smooth);
        let frame = route.at(d);
        let z = frame.tangent.normalize_or(DVec3::Z);
        let x = frame.up.cross(z).normalize_or(DVec3::X);
        let y = z.cross(x);
        let position = frame.position + frame.right * lateral + y * follow.ride;
        let basis = DMat4::from_cols(
            x.extend(0.0),
            y.extend(0.0),
            z.extend(0.0),
            position.extend(1.0),
        );
        pose.push(basis * DMat4::from_rotation_y(-smooth));
    }
    Drive { distance, pose }
}

pub(crate) fn steer(route: &Route, distance: f64, wheelbase: f64) -> f64 {
    let max = STEER_MAX_DEG.to_radians();
    (-(wheelbase * route.curvature(distance)).atan()).clamp(-max, max)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Wheel {
    pub centre: DVec3,
    pub radius: f64,
    pub axle: DVec3,
}

pub(crate) fn spin(distance: f64, radius: f64) -> f64 {
    if radius > 0.0 { distance / radius } else { 0.0 }
}

fn vec3(value: Option<&serde_json::Value>) -> Option<DVec3> {
    let list = value?.as_array()?;
    if list.len() != 3 {
        return None;
    }
    Some(DVec3::new(
        list[0].as_f64()?,
        list[1].as_f64()?,
        list[2].as_f64()?,
    ))
}

fn wheel_of(node: &Node, corner: &str, facts: Option<&serde_json::Value>) -> Wheel {
    let fact = facts
        .and_then(|f| f.get("wheels"))
        .and_then(|w| w.get(corner));
    let bounds = manfred_mesh::bounds(&node.mesh);
    let centre = vec3(fact.and_then(|f| f.get("centre")))
        .or_else(|| bounds.map(|[lo, hi]| (DVec3::from_array(lo) + DVec3::from_array(hi)) * 0.5))
        .unwrap_or(DVec3::ZERO);
    let radius = fact
        .and_then(|f| f.get("radius"))
        .and_then(serde_json::Value::as_f64)
        .or_else(|| bounds.map(|[lo, hi]| (hi[1] - lo[1]) * 0.5))
        .unwrap_or(0.0);
    let axle = vec3(fact.and_then(|f| f.get("axle")))
        .and_then(|a| a.try_normalize())
        .unwrap_or(DVec3::X);
    let axle = if axle.x < 0.0 { -axle } else { axle };
    Wheel {
        centre,
        radius,
        axle,
    }
}

pub(crate) struct Sub<'a> {
    name: String,
    rel: DMat4,
    wheel: Option<(bool, Wheel)>,
    facts: Option<&'a serde_json::Value>,
}

impl Sub<'_> {
    fn owns(&self, node: &str) -> bool {
        node.strip_prefix(self.name.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
    }

    fn spins(&self, node: &str) -> bool {
        node.strip_prefix(self.name.as_str())
            .and_then(|rest| rest.strip_prefix('.'))
            .and_then(|piece| self.facts?.get(piece)?.get("spin")?.as_bool())
            .unwrap_or(true)
    }

    fn keys(
        &self,
        node: &str,
        poses: &[DMat4],
        drive: Option<(&Drive, &Route)>,
        base: f64,
        scaled: &dyn Fn(usize) -> DMat4,
    ) -> Vec<Trs> {
        let (Some((front, wheel)), Some((path, route))) = (self.wheel, drive) else {
            return poses
                .iter()
                .enumerate()
                .map(|(n, pose)| trs_from_matrix(*pose * scaled(n) * self.rel))
                .collect();
        };
        let spins = self.spins(node);
        let pivot = self.rel.transform_point3(wheel.centre);
        poses
            .iter()
            .zip(&path.distance)
            .enumerate()
            .map(|(n, (pose, &d))| {
                let turn = if front { steer(route, d, base) } else { 0.0 };
                let roll = if spins { spin(d, wheel.radius) } else { 0.0 };
                let local = DMat4::from_translation(pivot)
                    * DMat4::from_rotation_y(turn)
                    * DMat4::from_translation(-pivot)
                    * self.rel
                    * DMat4::from_translation(wheel.centre)
                    * DMat4::from_quat(DQuat::from_axis_angle(wheel.axle, roll))
                    * DMat4::from_translation(-wheel.centre);
                trs_from_matrix(*pose * scaled(n) * local)
            })
            .collect()
    }
}

fn descends(recipe: &Recipe, from: usize, to: usize) -> bool {
    let mut at = from;
    for _ in 0..recipe.objects.len() {
        let Some(parent) = &recipe.objects[at].parent else {
            return false;
        };
        let Some(up) = recipe.objects.iter().rposition(|o| o.name == *parent) else {
            return false;
        };
        if up == to {
            return true;
        }
        at = up;
    }
    false
}

pub(crate) fn subs<'a>(b: &Bake<'a>, object: &str) -> Result<Vec<Sub<'a>>, String> {
    let prefix = format!("{object}.");
    let wheel_prefix = format!("{object}.wheel_");
    let Some(root) = b.recipe.objects.iter().position(|o| o.name == object) else {
        return Ok(Vec::new());
    };
    let world = |at: usize| crate::world(b.recipe, at).map_err(|e| e.to_string());
    let mut out = Vec::new();
    for (at, sub) in b.recipe.objects.iter().enumerate() {
        if !sub.name.starts_with(&prefix) {
            continue;
        }
        let rel = if descends(b.recipe, at, root) {
            world(root)?.inverse() * world(at)?
        } else {
            world(at)?
        };
        let facts = b.facts.get(&sub.name);
        let wheel = sub
            .name
            .strip_prefix(&wheel_prefix)
            .filter(|corner| !corner.contains('.'))
            .map(|corner| {
                let fact = facts.and_then(|f| f.get("wheel"));
                let radius = fact
                    .and_then(|f| f.get("radius"))
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or_else(|| sub_radius(b.nodes, &sub.name));
                let axle = vec3(fact.and_then(|f| f.get("axle")))
                    .and_then(|a| a.try_normalize())
                    .unwrap_or(DVec3::X);
                let axle = if axle.x < 0.0 { -axle } else { axle };
                (
                    corner.starts_with('f'),
                    Wheel {
                        centre: DVec3::ZERO,
                        radius,
                        axle,
                    },
                )
            });
        out.push(Sub {
            name: sub.name.clone(),
            rel,
            wheel,
            facts,
        });
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.name.len()));
    Ok(out)
}

fn sub_radius(nodes: &[Node], name: &str) -> f64 {
    let dotted = format!("{name}.");
    nodes
        .iter()
        .filter(|n| n.name == name || n.name.starts_with(&dotted))
        .filter_map(|n| manfred_mesh::bounds(&n.mesh))
        .map(|[lo, hi]| (hi[1] - lo[1]) * 0.5)
        .fold(0.0, f64::max)
}

fn wheelbase(wheels: &[(bool, Wheel)], facts: Option<&serde_json::Value>) -> f64 {
    let mean = |front: bool| {
        let z: Vec<f64> = wheels
            .iter()
            .filter(|(f, _)| *f == front)
            .map(|(_, w)| w.centre.z)
            .collect();
        (!z.is_empty()).then(|| z.iter().sum::<f64>() / z.len() as f64)
    };
    match (mean(true), mean(false)) {
        (Some(front), Some(rear)) if front != rear => (front - rear).abs(),
        _ => facts
            .and_then(|f| f.get("wheelbase"))
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(WHEELBASE_M),
    }
}

pub(crate) fn nodes(
    object: &str,
    poses: &[DMat4],
    scales: Option<&[DVec3]>,
    scale: DVec3,
    drive: Option<(&Drive, &Route)>,
    (nodes, subs): (&[Node], &[Sub]),
    facts: Option<&serde_json::Value>,
) -> Vec<Track> {
    let prefix = format!("{object}.");
    let wheel_prefix = format!("{object}.wheel_");
    let owned: Vec<&Node> = nodes
        .iter()
        .filter(|n| n.name == object || n.name.starts_with(&prefix))
        .collect();
    let sub_of = |node: &Node| subs.iter().find(|s| s.owns(&node.name));
    let wheels: Vec<(bool, Wheel)> = owned
        .iter()
        .filter(|n| sub_of(n).is_none())
        .filter_map(|n| {
            let corner = n.name.strip_prefix(&wheel_prefix)?;
            Some((corner.starts_with('f'), wheel_of(n, corner, facts)))
        })
        .collect();
    let mut axles = wheels.clone();
    axles.extend(subs.iter().filter_map(|s| {
        let (front, wheel) = s.wheel?;
        Some((
            front,
            Wheel {
                centre: s.rel.transform_point3(wheel.centre),
                ..wheel
            },
        ))
    }));
    let base = wheelbase(&axles, facts);
    let constant = DMat4::from_scale(scale);
    let scaled = |n: usize| match scales {
        Some(scales) => DMat4::from_scale(scales[n]),
        None => constant,
    };
    let mut tracks = Vec::with_capacity(owned.len());
    let mut wheel_at = 0;
    for node in owned {
        let keys = match (sub_of(node), drive) {
            (Some(sub), _) => sub.keys(&node.name, poses, drive, base, &scaled),
            (None, Some((path, route))) if node.name.starts_with(&wheel_prefix) => {
                let (front, wheel) = wheels[wheel_at];
                wheel_at += 1;
                poses
                    .iter()
                    .zip(&path.distance)
                    .enumerate()
                    .map(|(n, (pose, &d))| {
                        let turn = if front { steer(route, d, base) } else { 0.0 };
                        let local = DMat4::from_translation(wheel.centre)
                            * DMat4::from_rotation_y(turn)
                            * DMat4::from_quat(DQuat::from_axis_angle(
                                wheel.axle,
                                spin(d, wheel.radius),
                            ))
                            * DMat4::from_translation(-wheel.centre);
                        trs_from_matrix(*pose * scaled(n) * local)
                    })
                    .collect()
            }
            _ => poses
                .iter()
                .enumerate()
                .map(|(n, pose)| trs_from_matrix(*pose * scaled(n)))
                .collect(),
        };
        tracks.push(Track {
            node: node.name.clone(),
            keys,
            camera: None,
            focus: None,
            fov: None,
        });
    }
    tracks
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_recipe::Drift;

    fn timeline(frames: u32) -> Timeline {
        Timeline {
            fps: 60,
            frames,
            size: [1080, 1920],
            seed: 7,
        }
    }

    fn follow(speed: Vec<[f64; 2]>, drift: Option<Drift>) -> Follow {
        Follow {
            object: "hero".to_string(),
            route: "main".to_string(),
            lane: 1.5,
            start: 0.0,
            speed,
            drift,
            ride: 0.0,
        }
    }

    fn straight() -> Route {
        Route::new(&[[0.0, 0.0, 0.0], [0.0, 0.0, 500.0]], false).unwrap()
    }

    #[test]
    fn constant_speed_travels_v_times_t() {
        let tl = timeline(120);
        let d = distances(&follow(vec![[0.0, 12.0]], None), &straight(), &tl);
        for (n, d) in d.iter().enumerate() {
            assert!((d - 12.0 * tl.time(n as u32)).abs() < 1e-9);
        }
        let ramp = distances(
            &follow(vec![[0.0, 0.0], [2.0, 20.0]], None),
            &straight(),
            &tl,
        );
        assert!((ramp[120] - 20.0).abs() < 1e-9);
        let held = distances(
            &follow(vec![[0.0, 100.0]], None),
            &straight(),
            &timeline(600),
        );
        assert_eq!(held[600], 500.0);
    }

    #[test]
    fn wheel_spin_is_distance_over_radius() {
        assert!((spin(10.0, 0.35) - 10.0 / 0.35).abs() < 1e-12);
        assert_eq!(spin(10.0, 0.0), 0.0);
    }

    #[test]
    fn a_straight_route_does_not_steer() {
        let route = straight();
        for d in [0.0, 10.0, 250.0, 499.0] {
            assert_eq!(steer(&route, d, 2.7), 0.0);
        }
    }

    #[test]
    fn a_drive_is_deterministic() {
        let drift = Some(Drift {
            amount: 0.6,
            period: 1.5,
            slip_deg: 8.0,
        });
        let f = follow(vec![[0.0, 10.0], [3.0, 30.0]], drift);
        let tl = timeline(600);
        let a = drive(&f, &straight(), &tl);
        let b = drive(&f, &straight(), &tl);
        assert_eq!(a, b);
        assert_ne!(a.pose[0], a.pose[300]);
    }
}
