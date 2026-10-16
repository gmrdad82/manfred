use crate::Bake;
use glam::{DMat4, DQuat, DVec3};
use manfred_mesh::math::Real;
use manfred_motion::Route;
use manfred_recipe::{Camera, Rig, RigFrame, RigTracks, Spring};
use pfx_core::fx::{Comfort, Shake, ShakeTuning};
use pfx_core::rig::{Follow, FollowTuning, Input};
use std::f64::consts::{PI, TAU};

const LOOK_AHEAD_EASE: f64 = 0.25;
const BANK_EASE: f64 = 0.5;
const MAX_BANK_DEG: f64 = 30.0;

pub(crate) fn rigged(
    b: &Bake,
    rig: &Rig,
    frames: &[(DQuat, DVec3)],
    up: DVec3,
    route: Option<&Route>,
) -> Result<Vec<DMat4>, String> {
    let dt = 1.0 / f64::from(b.timeline.fps.max(1));
    let times: Vec<f64> = (0..frames.len())
        .map(|n| b.timeline.time(n as u32))
        .collect();
    let curve =
        |name: &str, fallback: f64| -> Vec<f64> { sampled(rig, &times, dt, name, fallback) };
    let spherical = rig.tracks.spherical();
    let reach = DVec3::from(rig.offset).length();
    let distance = curve("distance", if spherical { reach } else { -rig.offset[2] });
    let lateral = curve("lateral", -rig.offset[0]);
    let height = curve("height", rig.offset[1]);
    let lag: Vec<f64> = curve("lag", rig.lag).iter().map(|v| v.max(0.0)).collect();
    let gain = curve("bank", rig.bank);
    let jolt: Vec<f64> = curve("shake", 1.0).iter().map(|v| v.max(0.0)).collect();
    let ahead_s: Vec<f64> = curve("look_ahead", rig.look_ahead)
        .iter()
        .map(|v| v.max(0.0))
        .collect();
    let world = rig.frame == RigFrame::World;
    let turns = if world {
        vec![DQuat::IDENTITY; frames.len()]
    } else {
        headings(frames, rig.yaw_lag, dt)
    };
    let offsets: Vec<DVec3> = if spherical {
        let elevation = if reach > 0.0 {
            (rig.offset[1] / reach)
                .clamp(-1.0, 1.0)
                .asin_of()
                .to_degrees()
        } else {
            0.0
        };
        let azimuth = (-rig.offset[0]).atan2_of(-rig.offset[2]).to_degrees();
        let orbit = curve("orbit", azimuth);
        let pitch = curve("pitch", elevation);
        (0..frames.len())
            .map(|n| around(distance[n], orbit[n], pitch[n]))
            .collect()
    } else {
        (0..frames.len())
            .map(|n| DVec3::new(-lateral[n], height[n], -distance[n]))
            .collect()
    };
    let wanted: Vec<DVec3> = frames
        .iter()
        .zip(&turns)
        .enumerate()
        .map(|(n, ((_, at), turn))| *at + *turn * offsets[n])
        .collect();
    let crab = rig.tracks.lateral.is_some() && !spherical;
    let aims = looks(rig, &times, dt);
    let looked: Vec<DVec3> = frames
        .iter()
        .zip(&turns)
        .enumerate()
        .map(|(n, ((_, at), turn))| {
            let mut look = aims.as_ref().map_or(DVec3::from(rig.look), |a| a[n]);
            if crab {
                look.x -= lateral[n] + rig.offset[0];
            }
            *at + *turn * look
        })
        .collect();
    let rolls = rig.tracks.roll.is_some().then(|| curve("roll", 0.0));
    let mut place = smoothing(rig.lag);
    let mut aim = smoothing(LOOK_AHEAD_EASE);
    let mut eyes = Vec::with_capacity(frames.len());
    let mut targets = Vec::with_capacity(frames.len());
    for n in 0..frames.len() {
        let moving = velocity(&wanted, n, dt);
        let looking = velocity(&looked, n, dt);
        place.tuning_mut().damping = [lag[n] as f32; 3];
        let settled = eased(&mut place, moving, dt);
        let ahead = eased(&mut aim, looking, dt);
        eyes.push(wanted[n] - (moving - settled) * lag[n]);
        targets.push(looked[n] + ahead * ahead_s[n]);
    }
    let calm = released(&mut eyes, &times, rig, dt);
    let banks = banks(rig, frames, dt, &gain, &calm, route);
    let mut shake = rig.shake.map(|s| {
        let mut shake = Shake::new(ShakeTuning {
            translation: [s.translation as f32; 2],
            roll: s.roll as f32,
            frequency: s.frequency as f32,
            decay: 0.0,
            seed: s.seed.unwrap_or(b.timeline.seed),
        });
        shake.add(1.0);
        shake
    });
    let mut keys = Vec::with_capacity(frames.len());
    for n in 0..frames.len() {
        let mut eye = eyes[n];
        let mut pose =
            view(eye, targets[n], up).or_else(|_| view(eye, targets[n], turns[n] * DVec3::Z))?;
        if let Some(roll) = banks.get(n) {
            pose *= DQuat::from_rotation_z(-roll.to_radians());
        }
        if let Some(rolls) = &rolls {
            pose *= DQuat::from_rotation_z(-rolls[n].to_radians());
        }
        if let Some(shake) = &mut shake {
            if n > 0 {
                shake.step(dt as f32, 1.0);
            }
            let offset = shake.offset(1.0);
            let [x, y] = offset.translation.map(f64::from);
            eye += (pose * DVec3::X * x + pose * DVec3::Y * y) * jolt[n];
            pose *= DQuat::from_rotation_z(f64::from(offset.roll).to_radians() * jolt[n]);
        }
        keys.push(DMat4::from_rotation_translation(pose, eye));
    }
    Ok(keys)
}

pub fn hermite(keys: &[[f64; 2]], t: f64) -> f64 {
    let Some(last) = keys.len().checked_sub(1) else {
        return 0.0;
    };
    if last == 0 || t <= keys[0][0] {
        return keys[0][1];
    }
    if t >= keys[last][0] {
        return keys[last][1];
    }
    let i = keys.partition_point(|k| k[0] <= t) - 1;
    let slope = |j: usize| {
        if j == 0 || j == last {
            0.0
        } else {
            (keys[j + 1][1] - keys[j - 1][1]) / (keys[j + 1][0] - keys[j - 1][0])
        }
    };
    let ([t0, v0], [t1, v1]) = (keys[i], keys[i + 1]);
    let h = t1 - t0;
    let u = (t - t0) / h;
    let (u2, u3) = (u * u, u * u * u);
    (2.0 * u3 - 3.0 * u2 + 1.0) * v0
        + (u3 - 2.0 * u2 + u) * h * slope(i)
        + (3.0 * u2 - 2.0 * u3) * v1
        + (u3 - u2) * h * slope(i + 1)
}

fn sampled(rig: &Rig, times: &[f64], dt: f64, name: &str, fallback: f64) -> Vec<f64> {
    let values: Vec<f64> = match rig.tracks.get(name) {
        Some(keys) if !keys.is_empty() => times.iter().map(|t| hermite(keys, *t)).collect(),
        _ => vec![fallback; times.len()],
    };
    match rig.smooth.get(name) {
        Some(&seconds) if seconds > 0.0 => damp(&values, seconds, dt),
        _ => values,
    }
}

fn damp(values: &[f64], seconds: f64, dt: f64) -> Vec<f64> {
    let omega = 2.0 / seconds;
    let fade = (-omega * dt).exp_of();
    let mut at = values.first().copied().unwrap_or(0.0);
    let mut rate = 0.0;
    values
        .iter()
        .enumerate()
        .map(|(n, goal)| {
            if n > 0 {
                let change = at - goal;
                let push = (rate + omega * change) * dt;
                rate = (rate - omega * push) * fade;
                at = goal + (change + push) * fade;
            }
            at
        })
        .collect()
}

fn around(distance: f64, orbit_deg: f64, pitch_deg: f64) -> DVec3 {
    let (sa, ca) = orbit_deg.to_radians().sin_cos_of();
    let (se, ce) = pitch_deg.to_radians().sin_cos_of();
    DVec3::new(-distance * ce * sa, distance * se, -distance * ce * ca)
}

fn looks(rig: &Rig, times: &[f64], dt: f64) -> Option<Vec<DVec3>> {
    let keys = rig.tracks.look_offset.as_ref().filter(|k| !k.is_empty())?;
    let axis = |i: usize| {
        let line: Vec<[f64; 2]> = keys.iter().map(|(t, v)| [*t, v[i]]).collect();
        let values: Vec<f64> = times.iter().map(|t| hermite(&line, *t)).collect();
        match rig.smooth.get(RigTracks::LOOK_OFFSET) {
            Some(&seconds) if seconds > 0.0 => damp(&values, seconds, dt),
            _ => values,
        }
    };
    let [x, y, z] = [axis(0), axis(1), axis(2)];
    Some(
        (0..times.len())
            .map(|n| DVec3::new(x[n], y[n], z[n]))
            .collect(),
    )
}

pub(crate) fn zoom(b: &Bake, rig: &Rig, camera: &Camera, count: usize) -> Option<Vec<f64>> {
    let dt = 1.0 / f64::from(b.timeline.fps.max(1));
    let times: Vec<f64> = (0..count).map(|n| b.timeline.time(n as u32)).collect();
    let aspect = b.timeline.aspect();
    if rig.tracks.fov.is_some() {
        return Some(sampled(rig, &times, dt, "fov", camera.fov_y_deg(aspect)));
    }
    rig.tracks.focal.as_ref()?;
    let mut lens = camera.clone();
    lens.fov_y_deg = None;
    let millimetres = sampled(rig, &times, dt, "focal", camera.focal_mm.unwrap_or(50.0));
    Some(
        millimetres
            .into_iter()
            .map(|focal| {
                lens.focal_mm = Some(focal);
                lens.fov_y_deg(aspect)
            })
            .collect(),
    )
}

fn released(eyes: &mut [DVec3], times: &[f64], rig: &Rig, dt: f64) -> Vec<f64> {
    let mut calm = vec![1.0; eyes.len()];
    let Some(release) = rig.release else {
        return calm;
    };
    let Some(from) = times.iter().position(|t| *t >= release) else {
        return calm;
    };
    let ease = rig.release_ease.max(dt);
    let base = eyes[from];
    let speed = velocity(eyes, from, dt);
    for n in from..eyes.len() {
        let u = ((times[n] - times[from]) / ease).min(1.0);
        let (u2, u3) = (u * u, u * u * u);
        eyes[n] = base + speed * ease * (u - u3 + 0.5 * u2 * u2);
        calm[n] = 1.0 - 3.0 * u2 + 2.0 * u3;
    }
    calm
}

fn spring(wanted: &[f64], dt: f64, spring: Spring) -> Vec<f64> {
    let omega = TAU * spring.frequency;
    let steps = ((omega * dt) / 0.05).ceil().max(1.0);
    let h = dt / steps;
    let mut roll = wanted.first().copied().unwrap_or(0.0);
    let mut rate = 0.0;
    wanted
        .iter()
        .enumerate()
        .map(|(n, goal)| {
            if n > 0 {
                for _ in 0..steps as usize {
                    rate +=
                        (omega * omega * (goal - roll) - 2.0 * spring.damping * omega * rate) * h;
                    roll += rate * h;
                }
            }
            roll
        })
        .collect()
}

fn headings(frames: &[(DQuat, DVec3)], lag: f64, dt: f64) -> Vec<DQuat> {
    if lag <= 0.0 || frames.is_empty() {
        return frames.iter().map(|(turn, _)| *turn).collect();
    }
    let mut yaws: Vec<f64> = Vec::with_capacity(frames.len());
    for (turn, _) in frames {
        let forward = *turn * DVec3::Z;
        let yaw = forward.x.atan2_of(forward.z);
        let unwrapped = match yaws.last() {
            Some(last) => last + (yaw - last + PI).rem_euclid(TAU) - PI,
            None => yaw,
        };
        yaws.push(unwrapped);
    }
    let mut heading = smoothing(lag);
    frames
        .iter()
        .zip(&yaws)
        .map(|((turn, _), yaw)| {
            heading.tick(
                dt as f32,
                &Input::at([*yaw as f32, 0.0, 0.0]),
                Comfort::FULL,
            );
            let smooth = f64::from(heading.pose(1.0).at[0]);
            DQuat::from_rotation_y(smooth - yaw) * *turn
        })
        .collect()
}

fn banks(
    rig: &Rig,
    frames: &[(DQuat, DVec3)],
    dt: f64,
    gain: &[f64],
    calm: &[f64],
    route: Option<&Route>,
) -> Vec<f64> {
    if gain.iter().all(|g| *g == 0.0) {
        return Vec::new();
    }
    let Some(route) = route else {
        return Vec::new();
    };
    let places: Vec<DVec3> = frames.iter().map(|(_, at)| *at).collect();
    let wanted: Vec<f64> = (0..places.len())
        .map(|n| {
            let speed = velocity(&places, n, dt).length();
            let lateral = route.curvature(route.nearest(places[n]).0) * speed * speed;
            (gain[n] * lateral).clamp(-MAX_BANK_DEG, MAX_BANK_DEG) * calm[n]
        })
        .collect();
    if let Some(tuning) = rig.bank_spring {
        return spring(&wanted, dt, tuning);
    }
    let mut roll = smoothing(BANK_EASE);
    wanted
        .iter()
        .map(|goal| {
            roll.tick(
                dt as f32,
                &Input::at([*goal as f32, 0.0, 0.0]),
                Comfort::FULL,
            );
            f64::from(roll.pose(1.0).at[0])
        })
        .collect()
}

fn eased(follow: &mut Follow, value: DVec3, dt: f64) -> DVec3 {
    follow.tick(
        dt as f32,
        &Input::at(value.as_vec3().to_array()),
        Comfort::FULL,
    );
    glam::Vec3::from(follow.pose(1.0).at).as_dvec3()
}

fn smoothing(time: f64) -> Follow {
    Follow::new(FollowTuning {
        offset: [0.0; 3],
        arm: [0.0; 3],
        damping: [time.max(0.0) as f32; 3],
        handover: 0.0,
        ..FollowTuning::follow3()
    })
}

fn velocity(points: &[DVec3], n: usize, dt: f64) -> DVec3 {
    let (from, to) = match (n.checked_sub(1), points.get(n + 1)) {
        (Some(before), _) => (points[before], points[n]),
        (None, Some(next)) => (points[n], *next),
        (None, None) => return DVec3::ZERO,
    };
    (to - from) / dt
}

pub(crate) fn view(eye: DVec3, target: DVec3, up: DVec3) -> Result<DQuat, String> {
    let back = (eye - target).normalize_or_zero();
    let right = up.cross(back).normalize_or_zero();
    if back == DVec3::ZERO || right == DVec3::ZERO {
        return Err(
            "camera rig: the camera sits on its aim point or looks straight along `up`".into(),
        );
    }
    Ok(DQuat::from_mat3(&glam::DMat3::from_cols(
        right,
        back.cross(right),
        back,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bake::constraint::bake;
    use crate::{Lens, Track};
    use manfred_motion::matrix;
    use manfred_motion::{Route, Timeline, Trs};
    use manfred_recipe::RigFocus;
    use manfred_recipe::{RigShake, RigTracks};
    use std::collections::BTreeMap;

    const TIMELINE: Timeline = Timeline {
        fps: 30,
        frames: 60,
        size: [1080, 1920],
        seed: 7,
    };

    fn car(t: f64) -> Trs {
        let turn = DQuat::from_rotation_y(0.4 * t);
        Trs {
            translation: [120.0 + 3.0 * t * t, 0.0, -40.0 + 14.0 * t],
            rotation: turn.to_array(),
            scale: [1.0; 3],
        }
    }

    fn recipe(rig: Rig) -> manfred_recipe::Recipe {
        let text = "manfred = 1\nname = \"rig\"\n\n[[object]]\nname = \"hero\"\nkind = \"box\"\nsize = [1, 1, 1]\n\n[[camera]]\nname = \"chase\"\nposition = [0, 1, -5]\ntarget = [0, 0, 0]\nfov_y_deg = 40\nfstop = 2.8\n";
        let mut r = manfred_recipe::parse(text).expect("recipe");
        r.cameras[0].rig = Some(rig);
        r
    }

    fn rig() -> Rig {
        Rig {
            follow: "hero".into(),
            offset: [0.0, 1.1, -5.5],
            look: [0.0, 0.6, 0.0],
            lag: 0.0,
            look_ahead: 0.0,
            yaw_lag: 0.0,
            bank: 0.0,
            shake: None,
            focus: Some(RigFocus::Follow),
            tracks: RigTracks::default(),
            bank_spring: None,
            release: None,
            release_ease: 1.5,
            smooth: BTreeMap::new(),
            frame: RigFrame::Target,
        }
    }

    fn baked(rig: Rig) -> Track {
        moved(rig, car, None)
    }

    fn moved(rig: Rig, motion: fn(f64) -> Trs, route: Option<Route>) -> Track {
        let mut r = recipe(rig);
        let mut routes = BTreeMap::new();
        if let Some(route) = route {
            routes.insert("main".to_string(), route);
            r.follows.push(manfred_recipe::Follow {
                object: "hero".into(),
                route: "main".into(),
                lane: 0.0,
                start: 0.0,
                speed: vec![[0.0, 20.0]],
                drift: None,
                ride: 0.0,
            });
        }
        let tracks = [Track {
            node: "hero".into(),
            keys: (0..=TIMELINE.frames)
                .map(|n| motion(TIMELINE.time(n)))
                .collect(),
            camera: None,
            focus: None,
            fov: None,
        }];
        let facts = serde_json::Map::new();
        let b = Bake {
            recipe: &r,
            nodes: &[],
            facts: &facts,
            timeline: &TIMELINE,
            routes: &routes,
            tracks: &tracks,
        };
        let mut out = bake(&b).expect("rig");
        assert_eq!(out.len(), 1);
        out.remove(0)
    }

    fn bend() -> Route {
        Route::new(
            &[
                [0.0; 3],
                [-6.0, 0.0, 25.0],
                [-24.0, 0.0, 45.0],
                [-50.0, 0.0, 55.0],
            ],
            false,
        )
        .expect("route")
    }

    fn on_route(route: &Route, t: f64) -> Trs {
        let frame = route.at(20.0 * t);
        let basis = glam::DMat3::from_cols(frame.up.cross(frame.tangent), frame.up, frame.tangent);
        Trs {
            translation: frame.position.to_array(),
            rotation: DQuat::from_mat3(&basis).normalize().to_array(),
            scale: [1.0; 3],
        }
    }

    fn straight(t: f64) -> Trs {
        on_route(
            &Route::new(&[[0.0; 3], [0.0, 0.0, 500.0]], false).expect("route"),
            t,
        )
    }

    fn bending(t: f64) -> Trs {
        on_route(&bend(), t)
    }

    fn drifting(t: f64) -> Trs {
        Trs {
            translation: [0.0, 0.0, 20.0 * t],
            rotation: DQuat::from_rotation_y(0.2 * (std::f64::consts::TAU * t).sin_of()).to_array(),
            scale: [1.0; 3],
        }
    }

    fn eye(track: &Track, n: usize) -> DVec3 {
        DVec3::from(track.keys[n].translation)
    }

    fn rigid(m: DMat4) -> (DQuat, DVec3) {
        let (_, turn, at) = m.to_scale_rotation_translation();
        (turn.normalize(), at)
    }

    fn wanted(n: u32, point: [f64; 3]) -> DVec3 {
        let (turn, at) = rigid(matrix(&car(TIMELINE.time(n))));
        at + turn * DVec3::from(point)
    }

    #[test]
    fn without_lag_or_shake_the_camera_sits_at_the_offset_and_looks_at_the_aim() {
        let track = baked(rig());
        assert_eq!(track.node, "camera.chase");
        assert_eq!(track.keys.len(), 61);
        let Some(Lens { aspect, znear, .. }) = track.camera else {
            panic!("no lens");
        };
        assert!((aspect - 1080.0 / 1920.0).abs() < 1e-12 && znear == 0.05);
        for n in [0, 17, 60] {
            assert!(eye(&track, n as usize).distance(wanted(n, [0.0, 1.1, -5.5])) < 1e-4);
            let forward = DQuat::from_array(track.keys[n as usize].rotation) * DVec3::NEG_Z;
            let toward = (wanted(n, [0.0, 0.6, 0.0]) - eye(&track, n as usize)).normalize();
            assert!(forward.dot(toward) > 1.0 - 1e-9);
        }
    }

    #[test]
    fn with_lag_frame_zero_sits_at_the_offset_and_the_first_frames_move_smoothly() {
        let lagged = baked(Rig { lag: 0.3, ..rig() });
        assert!(eye(&lagged, 0).distance(wanted(0, [0.0, 1.1, -5.5])) < 1e-3);
        let shaken = baked(Rig {
            lag: 0.3,
            shake: Some(RigShake {
                translation: 0.05,
                roll: 1.0,
                frequency: 3.0,
                seed: Some(4),
            }),
            ..rig()
        });
        for n in 1..=10u32 {
            let own = wanted(n, [0.0, 1.1, -5.5]).distance(wanted(n - 1, [0.0, 1.1, -5.5]));
            let i = n as usize;
            assert!((eye(&lagged, i).distance(eye(&lagged, i - 1)) - own).abs() < 0.01);
            assert!(
                eye(&shaken, i).distance(eye(&shaken, i - 1)) <= own + 0.01 + 0.1 * 2f64.sqrt()
            );
        }
        let gap = eye(&lagged, 40) - wanted(40, [0.0, 1.1, -5.5]);
        assert!(gap.length() > 0.3);
        assert!(gap.x < 0.0);
    }

    #[test]
    fn bank_rolls_into_bends_and_never_on_a_straight_road() {
        let level = moved(
            rig(),
            straight,
            Some(Route::new(&[[0.0; 3], [0.0, 0.0, 500.0]], false).expect("route")),
        );
        let banked = moved(
            Rig { bank: 5.0, ..rig() },
            straight,
            Some(Route::new(&[[0.0; 3], [0.0, 0.0, 500.0]], false).expect("route")),
        );
        assert_eq!(level.keys, banked.keys);
        let route = bend();
        let bent = moved(Rig { bank: 5.0, ..rig() }, bending, Some(bend()));
        let n = 40;
        let turning = route.curvature(20.0 * TIMELINE.time(n as u32));
        assert!(turning.abs() > 1e-3);
        let dip = (DQuat::from_array(bent.keys[n].rotation) * DVec3::X).y;
        assert!(dip.abs() > 0.01);
        assert_eq!(dip < 0.0, turning > 0.0);
    }

    #[test]
    fn yaw_lag_keeps_the_camera_on_the_road_while_the_car_yaws() {
        let tight = moved(rig(), drifting, None);
        let loose = moved(
            Rig {
                yaw_lag: 0.5,
                ..rig()
            },
            drifting,
            None,
        );
        assert!(eye(&loose, 0).distance(eye(&tight, 0)) < 1e-3);
        for n in 0..=TIMELINE.frames {
            let pose = drifting(TIMELINE.time(n));
            let at = DVec3::from(pose.translation)
                + DQuat::from_array(pose.rotation) * DVec3::from([0.0, 1.1, -5.5]);
            assert!(eye(&tight, n as usize).distance(at) < 1e-6);
        }
        let swing = |t: &Track| {
            t.keys
                .iter()
                .map(|k| k.translation[0].abs())
                .fold(0.0, f64::max)
        };
        assert!(swing(&loose) < 0.5 * swing(&tight));
    }

    #[test]
    fn shake_is_deterministic_by_seed() {
        let shaken = |seed| {
            baked(Rig {
                shake: Some(RigShake {
                    translation: 0.05,
                    roll: 1.0,
                    frequency: 3.0,
                    seed: Some(seed),
                }),
                ..rig()
            })
        };
        let still = baked(rig());
        let a = shaken(11);
        assert_eq!(a, shaken(11));
        assert_ne!(a.keys, shaken(12).keys);
        assert_ne!(a.keys, still.keys);
        assert!(a.keys.iter().zip(&still.keys).all(|(a, b)| {
            DVec3::from(a.translation).distance(DVec3::from(b.translation))
                <= 0.05 * 2f64.sqrt() + 1e-6
        }));
    }

    #[test]
    fn focus_follows_the_distance_to_the_followed_object_not_the_aim() {
        let track = baked(Rig {
            lag: 0.2,
            look: [0.0, 2.9, 8.0],
            ..rig()
        });
        let focus = track.focus.as_ref().expect("focus");
        assert_eq!(focus.len(), 61);
        for n in [0u32, 30, 60] {
            let want = eye(&track, n as usize).distance(wanted(n, [0.0, 0.0, 0.0]));
            assert!((focus[n as usize] - want).abs() < 1e-9);
            let aim = eye(&track, n as usize).distance(wanted(n, [0.0, 2.9, 8.0]));
            assert!(aim - focus[n as usize] > 2.0, "{aim} {}", focus[n as usize]);
        }
        assert_eq!(
            baked(Rig {
                focus: None,
                ..rig()
            })
            .focus,
            None
        );
    }

    #[test]
    fn a_track_overrides_its_static_key() {
        let track = baked(Rig {
            tracks: RigTracks {
                distance: Some(vec![[0.0, 9.0]]),
                lateral: Some(vec![[0.0, 2.0]]),
                ..RigTracks::default()
            },
            ..rig()
        });
        for n in [0u32, 30, 60] {
            assert!(eye(&track, n as usize).distance(wanted(n, [-2.0, 1.1, -9.0])) < 1e-4);
        }
        let held = baked(Rig {
            tracks: RigTracks {
                height: Some(vec![[0.0, 1.1]]),
                lag: Some(vec![[0.0, 0.0]]),
                ..RigTracks::default()
            },
            ..rig()
        });
        assert_eq!(held.keys, baked(rig()).keys);
    }

    #[test]
    fn a_track_runs_through_its_keys_and_holds_past_the_ends() {
        let keys = [[0.5, 3.0], [1.0, 10.0], [2.0, 6.0], [3.0, 15.0]];
        for [t, v] in keys {
            assert!((hermite(&keys, t) - v).abs() < 1e-12);
        }
        assert_eq!(hermite(&keys, 0.0), 3.0);
        assert_eq!(hermite(&keys, 9.0), 15.0);
        let even = [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0], [3.0, 3.0]];
        assert!((hermite(&even, 1.5) - 1.5).abs() < 1e-12);
        assert!((hermite(&even, 0.5) - 0.375).abs() < 1e-12);
        let rising: Vec<f64> = (0..=100)
            .map(|i| hermite(&keys, 0.5 + 0.005 * f64::from(i)))
            .collect();
        assert!(rising.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn release_brings_the_camera_to_rest_while_the_target_drives_away() {
        let track = baked(Rig {
            release: Some(1.0),
            release_ease: 0.5,
            ..rig()
        });
        let free = baked(rig());
        assert_eq!(track.keys[..30], free.keys[..30]);
        let rest = eye(&track, 45);
        for n in 45..=60 {
            assert!(eye(&track, n).distance(rest) < 1e-9);
        }
        let steps: Vec<f64> = (31..=45)
            .map(|n| eye(&track, n).distance(eye(&track, n - 1)))
            .collect();
        assert!(steps.windows(2).all(|w| w[1] <= w[0] + 1e-9));
        let focus = track.focus.as_ref().expect("focus");
        assert!(focus[46..].windows(2).all(|w| w[1] > w[0]));
        assert!(focus[60] > focus[30] + 5.0);
        for n in [45usize, 60] {
            let forward = DQuat::from_array(track.keys[n].rotation) * DVec3::NEG_Z;
            let toward = (wanted(n as u32, [0.0, 0.6, 0.0]) - eye(&track, n)).normalize();
            assert!(forward.dot(toward) > 1.0 - 1e-9);
        }
    }

    #[test]
    fn a_bank_spring_overshoots_then_settles() {
        let step: Vec<f64> = (0..240).map(|n| if n < 10 { 0.0 } else { 12.0 }).collect();
        let roll = spring(
            &step,
            1.0 / 60.0,
            Spring {
                frequency: 1.5,
                damping: 0.4,
            },
        );
        assert_eq!(roll[0], 0.0);
        let peak = roll.iter().copied().fold(0.0, f64::max);
        assert!(peak > 13.0 && peak < 20.0, "{peak}");
        assert!((roll[239] - 12.0).abs() < 0.1, "{}", roll[239]);
        let bent = |spring| {
            moved(
                Rig {
                    bank: 5.0,
                    bank_spring: spring,
                    ..rig()
                },
                bending,
                Some(bend()),
            )
        };
        assert_ne!(
            bent(None).keys,
            bent(Some(Spring {
                frequency: 1.5,
                damping: 0.4
            }))
            .keys
        );
    }

    #[test]
    fn a_directed_rig_is_deterministic() {
        let directed = || {
            moved(
                Rig {
                    lag: 0.2,
                    yaw_lag: 0.3,
                    bank: 6.0,
                    shake: Some(RigShake {
                        translation: 0.03,
                        roll: 0.5,
                        frequency: 6.0,
                        seed: Some(5),
                    }),
                    tracks: RigTracks {
                        distance: Some(vec![[0.0, 4.0], [0.6, 9.0], [1.5, 14.0]]),
                        lateral: Some(vec![[0.0, -2.0], [1.0, 3.0]]),
                        height: Some(vec![[0.0, 0.5], [1.0, 1.2]]),
                        bank: Some(vec![[0.0, 2.0], [1.0, 9.0]]),
                        lag: Some(vec![[0.0, 0.1], [1.0, 0.4]]),
                        shake: Some(vec![[0.0, 1.0], [2.0, 0.3]]),
                        look_ahead: Some(vec![[0.0, 0.0], [1.0, 0.3]]),
                        ..RigTracks::default()
                    },
                    bank_spring: Some(Spring {
                        frequency: 1.2,
                        damping: 0.45,
                    }),
                    release: Some(1.5),
                    release_ease: 0.4,
                    ..rig()
                },
                bending,
                Some(bend()),
            )
        };
        let a = directed();
        assert_eq!(a, directed());
        assert!(
            a.keys
                .iter()
                .all(|k| k.translation.iter().all(|v| v.is_finite()))
        );
    }

    fn orbiting(orbit: f64, pitch: f64, frame: RigFrame) -> Track {
        baked(Rig {
            tracks: RigTracks {
                orbit: Some(vec![[0.0, orbit]]),
                pitch: Some(vec![[0.0, pitch]]),
                distance: Some(vec![[0.0, 6.0]]),
                ..RigTracks::default()
            },
            frame,
            ..rig()
        })
    }

    #[test]
    fn orbit_zero_is_behind_ninety_the_right_side_and_pitch_ninety_straight_above() {
        let behind = orbiting(0.0, 0.0, RigFrame::Target);
        let right = orbiting(90.0, 0.0, RigFrame::Target);
        let front = orbiting(180.0, 0.0, RigFrame::Target);
        let above = orbiting(0.0, 90.0, RigFrame::Target);
        for n in [0u32, 30, 60] {
            let i = n as usize;
            assert!(eye(&behind, i).distance(wanted(n, [0.0, 0.0, -6.0])) < 1e-9);
            assert!(eye(&right, i).distance(wanted(n, [-6.0, 0.0, 0.0])) < 1e-9);
            assert!(eye(&front, i).distance(wanted(n, [0.0, 0.0, 6.0])) < 1e-9);
            assert!(eye(&above, i).distance(wanted(n, [0.0, 6.0, 0.0])) < 1e-9);
            let down = DQuat::from_array(above.keys[i].rotation) * DVec3::NEG_Z;
            assert!(down.dot(DVec3::NEG_Y) > 1.0 - 1e-9);
            let toward = (wanted(n, [0.0, 0.6, 0.0]) - eye(&right, i)).normalize();
            let forward = DQuat::from_array(right.keys[i].rotation) * DVec3::NEG_Z;
            assert!(forward.dot(toward) > 1.0 - 1e-9);
        }
    }

    #[test]
    fn a_world_frame_orbit_ignores_the_target_s_heading() {
        let free = orbiting(0.0, 0.0, RigFrame::World);
        let tied = orbiting(0.0, 0.0, RigFrame::Target);
        for n in [0u32, 30, 60] {
            let at = DVec3::from(car(TIMELINE.time(n)).translation);
            assert!(eye(&free, n as usize).distance(at + DVec3::new(0.0, 0.0, -6.0)) < 1e-9);
        }
        assert!(eye(&tied, 60).distance(eye(&free, 60)) > 1.0);
    }

    #[test]
    fn smoothing_lags_a_step_and_settles_without_overshoot() {
        let step: Vec<f64> = (0..240).map(|n| if n < 10 { 0.0 } else { 5.0 }).collect();
        let eased = damp(&step, 0.3, 1.0 / 60.0);
        assert_eq!(eased[..10], step[..10]);
        assert!(eased[11] < 1.0 && eased[20] < 5.0 && eased[20] > eased[11]);
        assert!(eased.windows(2).all(|w| w[1] >= w[0] && w[1] <= 5.0));
        assert!((eased[239] - 5.0).abs() < 1e-3, "{}", eased[239]);
        let keyed = |smooth: BTreeMap<String, f64>| {
            baked(Rig {
                tracks: RigTracks {
                    height: Some(vec![[0.0, 1.0], [0.5, 1.0], [0.6, 3.0]]),
                    ..RigTracks::default()
                },
                smooth,
                ..rig()
            })
        };
        let sharp = keyed(BTreeMap::new());
        let soft = keyed(BTreeMap::from([("height".to_string(), 0.4)]));
        assert_eq!(sharp.keys[0], soft.keys[0]);
        assert!(eye(&soft, 20).y < eye(&sharp, 20).y - 0.3);
        assert!((eye(&soft, 60).y - eye(&sharp, 60).y).abs() < 0.2);
    }

    #[test]
    fn a_keyed_lens_zooms_the_camera_and_roll_adds_to_the_horizon() {
        let mut r = recipe(rig());
        r.cameras[0].rig = Some(Rig {
            tracks: RigTracks {
                fov: Some(vec![[0.0, 30.0], [2.0, 60.0]]),
                roll: Some(vec![[0.0, 10.0]]),
                ..RigTracks::default()
            },
            ..rig()
        });
        let camera = r.cameras[0].clone();
        let lens = zoom(
            &Bake {
                recipe: &r,
                nodes: &[],
                facts: &serde_json::Map::new(),
                timeline: &TIMELINE,
                routes: &BTreeMap::new(),
                tracks: &[],
            },
            camera.rig.as_ref().unwrap(),
            &camera,
            61,
        )
        .expect("a lens");
        assert_eq!((lens[0], lens[60]), (30.0, 60.0));
        let rolled = baked(Rig {
            tracks: RigTracks {
                roll: Some(vec![[0.0, 10.0]]),
                ..RigTracks::default()
            },
            ..rig()
        });
        let level = baked(rig());
        let tilt = (DQuat::from_array(level.keys[30].rotation).inverse()
            * DQuat::from_array(rolled.keys[30].rotation))
        .to_axis_angle();
        assert!((tilt.1.to_degrees() - 10.0).abs() < 1e-6);
    }
}
