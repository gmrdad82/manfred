use crate::{Bake, Track, camera_track};
use glam::{DMat4, DQuat, DVec3};
use manfred_motion::{matrix, trs_from_matrix};
use manfred_recipe::{Camera, Rig, RigFocus};
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
    let turns = headings(&frames, rig.yaw_lag, dt);
    let wanted: Vec<DVec3> = frames
        .iter()
        .zip(&turns)
        .map(|((_, at), turn)| *at + *turn * DVec3::from(rig.offset))
        .collect();
    let looked: Vec<DVec3> = frames
        .iter()
        .zip(&turns)
        .map(|((_, at), turn)| *at + *turn * DVec3::from(rig.look))
        .collect();
    let banks = banks(b, rig, &frames, dt);
    let mut place = smoothing(rig.lag);
    let mut aim = smoothing(LOOK_AHEAD_EASE);
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
        let moving = velocity(&wanted, n, dt);
        let looking = velocity(&looked, n, dt);
        let settled = eased(&mut place, moving, dt);
        let ahead = eased(&mut aim, looking, dt);
        let mut eye = wanted[n] - (moving - settled) * rig.lag.max(0.0);
        let target = looked[n] + ahead * rig.look_ahead;
        let mut pose = view(eye, target, up)?;
        if let Some(roll) = banks.get(n) {
            pose *= DQuat::from_rotation_z(-roll.to_radians());
        }
        if let Some(shake) = &mut shake {
            if n > 0 {
                shake.step(dt as f32, 1.0);
            }
            let offset = shake.offset(1.0);
            let [x, y] = offset.translation.map(f64::from);
            eye += pose * DVec3::X * x + pose * DVec3::Y * y;
            pose *= DQuat::from_rotation_z(f64::from(offset.roll).to_radians());
        }
        keys.push(trs_from_matrix(DMat4::from_rotation_translation(pose, eye)));
        focus.push(eye.distance(looked[n]));
    }
    let mut track = camera_track(camera, b.timeline, keys);
    if rig.focus == Some(RigFocus::Follow) {
        track.focus = Some(focus);
    }
    Ok(track)
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

fn banks(b: &Bake, rig: &Rig, frames: &[(DQuat, DVec3)], dt: f64) -> Vec<f64> {
    if rig.bank == 0.0 {
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
    let mut roll = smoothing(BANK_EASE);
    (0..places.len())
        .map(|n| {
            let speed = velocity(&places, n, dt).length();
            let lateral = route.curvature(route.nearest(places[n]).0) * speed * speed;
            let wanted = (rig.bank * lateral).clamp(-MAX_BANK_DEG, MAX_BANK_DEG);
            roll.tick(
                dt as f32,
                &Input::at([wanted as f32, 0.0, 0.0]),
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

fn view(eye: DVec3, target: DVec3, up: DVec3) -> Result<DQuat, String> {
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
    use manfred_recipe::RigShake;
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
    fn focus_follows_the_distance_to_the_aim() {
        let track = baked(Rig { lag: 0.2, ..rig() });
        let focus = track.focus.as_ref().expect("focus");
        assert_eq!(focus.len(), 61);
        for n in [0u32, 30, 60] {
            let want = eye(&track, n as usize).distance(wanted(n, [0.0, 0.6, 0.0]));
            assert!((focus[n as usize] - want).abs() < 1e-9);
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
}
