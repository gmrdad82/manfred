use crate::{Bake, Track, camera_track};
use glam::{DMat4, DQuat, DVec3};
use manfred_motion::{matrix, trs_from_matrix};
use manfred_recipe::{Camera, Rig, RigFocus};
use pfx_core::fx::{Comfort, Shake, ShakeTuning};
use pfx_core::rig::{Follow, FollowTuning, Input};

const AHEAD_SPEED: f32 = 1.0e4;

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
    let origin = frames.first().map_or(DVec3::ZERO, |(_, p)| *p);
    let mut place = Follow::new(FollowTuning {
        offset: [0.0; 3],
        arm: [0.0; 3],
        damping: [rig.lag as f32; 3],
        handover: 0.0,
        ..FollowTuning::follow3()
    });
    let mut aim = Follow::new(FollowTuning {
        offset: [0.0; 3],
        arm: [0.0; 3],
        damping: [0.0; 3],
        look_ahead: rig.look_ahead as f32 * AHEAD_SPEED,
        look_ahead_speed: AHEAD_SPEED,
        handover: 0.0,
        ..FollowTuning::follow3()
    });
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
    for (n, (turn, at)) in frames.iter().enumerate() {
        let wanted = *at + *turn * DVec3::from(rig.offset);
        let looked = *at + *turn * DVec3::from(rig.look);
        place.tick(dt as f32, &Input::at(local(wanted, origin)), Comfort::FULL);
        aim.tick(dt as f32, &Input::at(local(looked, origin)), Comfort::FULL);
        let mut eye = world(place.pose(1.0).at, origin);
        let target = world(aim.pose(1.0).look_at, origin);
        let mut pose = view(eye, target, up)?;
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
        focus.push(eye.distance(looked));
    }
    let mut track = camera_track(camera, b.timeline, keys);
    if rig.focus == Some(RigFocus::Follow) {
        track.focus = Some(focus);
    }
    Ok(track)
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

fn local(point: DVec3, origin: DVec3) -> [f32; 3] {
    (point - origin).as_vec3().to_array()
}

fn world(point: [f32; 3], origin: DVec3) -> DVec3 {
    origin + glam::Vec3::from(point).as_dvec3()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Lens;
    use manfred_motion::{Timeline, Trs};
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
            shake: None,
            focus: Some(RigFocus::Follow),
        }
    }

    fn baked(rig: Rig) -> Track {
        let r = recipe(rig);
        let tracks = [Track {
            node: "hero".into(),
            keys: (0..=TIMELINE.frames)
                .map(|n| car(TIMELINE.time(n)))
                .collect(),
            camera: None,
            focus: None,
        }];
        let facts = serde_json::Map::new();
        let routes = BTreeMap::new();
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
    fn with_lag_the_camera_trails_the_offset() {
        let track = baked(Rig { lag: 0.3, ..rig() });
        assert!(eye(&track, 0).distance(wanted(0, [0.0, 1.1, -5.5])) < 1e-4);
        let behind = wanted(40, [0.0, 0.0, -1.0]) - wanted(40, [0.0; 3]);
        let gap = eye(&track, 40) - wanted(40, [0.0, 1.1, -5.5]);
        assert!(gap.length() > 0.5);
        assert!(gap.dot(behind) > 0.0);
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
