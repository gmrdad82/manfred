use crate::{Bake, Track, camera_track};
use glam::{DMat4, DQuat, DVec3};
use manfred_motion::{matrix, trs_from_matrix};
use manfred_recipe::{Camera, Rig, RigFocus, Spring};
use pfx_core::fx::{Comfort, Shake, ShakeTuning};
use pfx_core::rig::{Follow, FollowTuning, Input};
use std::f64::consts::{PI, TAU};

const LOOK_AHEAD_EASE: f64 = 0.25;
const BANK_EASE: f64 = 0.5;
const MAX_BANK_DEG: f64 = 30.0;

pub(crate) fn bake(b: &Bake) -> Result<Vec<Track>, String> {
    b.recipe
        .cameras
        .iter()
        .filter_map(|camera| camera.rig.as_ref().map(|rig| (camera, rig)))
        .map(|(camera, rig)| track(b, camera, rig))
        .collect()
}

fn track(b: &Bake, camera: &Camera, rig: &Rig) -> Result<Track, String> {
    let frames = frames(b, rig)?;
    let dt = 1.0 / f64::from(b.timeline.fps.max(1));
    let times: Vec<f64> = (0..frames.len())
        .map(|n| b.timeline.time(n as u32))
        .collect();
    let curve = |name: &str, fallback: f64| -> Vec<f64> {
        match rig.tracks.get(name) {
            Some(keys) if !keys.is_empty() => times.iter().map(|t| hermite(keys, *t)).collect(),
            _ => vec![fallback; times.len()],
        }
    };
    let distance = curve("distance", -rig.offset[2]);
    let lateral = curve("lateral", -rig.offset[0]);
    let height = curve("height", rig.offset[1]);
    let lag: Vec<f64> = curve("lag", rig.lag).iter().map(|v| v.max(0.0)).collect();
    let gain = curve("bank", rig.bank);
    let jolt: Vec<f64> = curve("shake", 1.0).iter().map(|v| v.max(0.0)).collect();
    let ahead_s: Vec<f64> = curve("look_ahead", rig.look_ahead)
        .iter()
        .map(|v| v.max(0.0))
        .collect();
    let turns = headings(&frames, rig.yaw_lag, dt);
    let wanted: Vec<DVec3> = frames
        .iter()
        .zip(&turns)
        .enumerate()
        .map(|(n, ((_, at), turn))| *at + *turn * DVec3::new(-lateral[n], height[n], -distance[n]))
        .collect();
    let crab = rig.tracks.lateral.is_some();
    let looked: Vec<DVec3> = frames
        .iter()
        .zip(&turns)
        .enumerate()
        .map(|(n, ((_, at), turn))| {
            let mut look = DVec3::from(rig.look);
            if crab {
                look.x -= lateral[n] + rig.offset[0];
            }
            *at + *turn * look
        })
        .collect();
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
    let banks = banks(b, rig, &frames, dt, &gain, &calm);
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
    let up = DVec3::from(camera.up);
    let mut keys = Vec::with_capacity(frames.len());
    let mut focus = Vec::with_capacity(frames.len());
    for n in 0..frames.len() {
        let mut eye = eyes[n];
        let mut pose = view(eye, targets[n], up)?;
        if let Some(roll) = banks.get(n) {
            pose *= DQuat::from_rotation_z(-roll.to_radians());
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
        keys.push(trs_from_matrix(DMat4::from_rotation_translation(pose, eye)));
        focus.push(eye.distance(frames[n].1));
    }
    let mut track = camera_track(camera, b.timeline, keys);
    if rig.focus == Some(RigFocus::Follow) {
        track.focus = Some(focus);
    }
    Ok(track)
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
        let yaw = forward.x.atan2(forward.z);
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
    b: &Bake,
    rig: &Rig,
    frames: &[(DQuat, DVec3)],
    dt: f64,
    gain: &[f64],
    calm: &[f64],
) -> Vec<f64> {
    if gain.iter().all(|g| *g == 0.0) {
        return Vec::new();
    }
    let Some(route) = b
        .recipe
        .follows
        .iter()
        .find(|f| f.object == rig.follow)
        .and_then(|f| b.routes.get(&f.route))
    else {
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

fn frames(b: &Bake, rig: &Rig) -> Result<Vec<(DQuat, DVec3)>, String> {
    let count = b.timeline.frames as usize + 1;
    let object = &rig.follow;
    let piece = format!("{object}.");
    let wheel = format!("{object}.wheel_");
    let owned = b.tracks.iter().find(|t| t.node == *object).or_else(|| {
        b.tracks
            .iter()
            .find(|t| t.node.starts_with(&piece) && !t.node.starts_with(&wheel))
    });
    if let Some(track) = owned {
        if track.keys.len() < count {
            return Err(format!(
                "camera rig: the track of `{}` has {} keys, expected {count}",
                track.node,
                track.keys.len()
            ));
        }
        return Ok(track.keys[..count]
            .iter()
            .map(|key| rigid(matrix(key)))
            .collect());
    }
    match b.nodes.iter().find(|n| n.name == *object) {
        Some(node) => Ok(vec![rigid(node.transform); count]),
        None => Err(format!(
            "camera rig: nothing named `{object}` moves or stands in the built scene, expected an object with a `[[follow]]`"
        )),
    }
}

fn rigid(m: DMat4) -> (DQuat, DVec3) {
    let (_, turn, at) = m.to_scale_rotation_translation();
    (turn.normalize(), at)
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
    use crate::Lens;
    use manfred_motion::{Route, Timeline, Trs};
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
            rotation: DQuat::from_rotation_y(0.2 * (std::f64::consts::TAU * t).sin()).to_array(),
            scale: [1.0; 3],
        }
    }

    fn eye(track: &Track, n: usize) -> DVec3 {
        DVec3::from(track.keys[n].translation)
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
}
