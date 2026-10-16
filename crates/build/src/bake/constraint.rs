use crate::bake::{follow, rig};
use crate::{Bake, Track, camera_track, rotation};
use glam::{DMat4, DQuat, DVec3};
use manfred_glb::LIGHT_PREFIX;
use manfred_motion::{Route, matrix, trs_from_matrix};
use manfred_recipe::{
    Axis, Constrained, Constraint, ConstraintKind, CopyOf, LookAt, Owner, Recipe, RigFocus,
};
use std::f64::consts::{FRAC_PI_2, PI};

pub(crate) fn animates(recipe: &Recipe) -> Vec<String> {
    let owners: Vec<String> = recipe
        .constrained()
        .into_iter()
        .filter_map(|c| match c.owner {
            Owner::Object(name) => Some(name),
            _ => None,
        })
        .collect();
    let mut out = owners.clone();
    for owner in &owners {
        let prefix = format!("{owner}.");
        out.extend(
            recipe
                .objects
                .iter()
                .filter(|o| o.name.starts_with(&prefix))
                .map(|o| o.name.clone()),
        );
    }
    out
}

pub(crate) fn bake(b: &Bake) -> Result<Vec<Track>, String> {
    let all = b.recipe.constrained();
    if all.is_empty() {
        return Ok(Vec::new());
    }
    let order = b
        .recipe
        .constraint_order(&all)
        .map_err(|cycle| format!("{}: {}", cycle.path, cycle.text))?;
    let mut known: Vec<Track> = b.tracks.to_vec();
    let mut made: Vec<(usize, Vec<Track>)> = Vec::with_capacity(all.len());
    for at in order {
        let tracks = owner(b, &all, &all[at], &known)?;
        known.extend(tracks.iter().cloned());
        made.push((at, tracks));
    }
    made.sort_by_key(|(at, _)| *at);
    Ok(made.into_iter().flat_map(|(_, tracks)| tracks).collect())
}

struct State<'a> {
    poses: Vec<DMat4>,
    scales: Option<Vec<DVec3>>,
    drive: Option<(follow::Drive, &'a Route)>,
    aims: Option<Vec<DVec3>>,
    fov: Option<Vec<f64>>,
}

type Frame = (DQuat, DVec3, DVec3);

fn owner(
    b: &Bake,
    all: &[Constrained],
    c: &Constrained,
    known: &[Track],
) -> Result<Vec<Track>, String> {
    let count = b.timeline.frames as usize + 1;
    let mut s = State {
        poses: vec![base(b, &c.owner)?; count],
        scales: None,
        drive: None,
        aims: None,
        fov: None,
    };
    for constraint in &c.list {
        match &constraint.kind {
            ConstraintKind::FollowPath(f) => {
                let route = b.routes.get(&f.route).ok_or_else(|| {
                    format!("{}.route: no route named `{}`", constraint.path, f.route)
                })?;
                let drive = follow::drive(f, route, b.timeline);
                s.poses = drive.pose.clone();
                s.drive = Some((drive, route));
            }
            ConstraintKind::LookAt(l) => look_at(b, known, &c.owner, constraint, l, &mut s)?,
            ConstraintKind::Copy(copy) => copied(b, known, constraint, copy, &mut s)?,
            ConstraintKind::Rig(r) => {
                let at = format!("{}.follow", constraint.path);
                let frames: Vec<(DQuat, DVec3)> = target(b, known, &r.follow, &at)?
                    .into_iter()
                    .map(|(turn, at, _)| (turn, at))
                    .collect();
                let camera = match &c.owner {
                    Owner::Camera(name) => {
                        b.recipe.cameras.iter().find(|camera| camera.name == *name)
                    }
                    _ => None,
                };
                let up = camera.map_or(DVec3::Y, |camera| DVec3::from(camera.up));
                s.poses = rig::rigged(b, r, &frames, up, route_of(b, all, &r.follow))
                    .map_err(|text| format!("{}: {text}", constraint.path))?;
                if let Some(zoom) = camera.and_then(|camera| rig::zoom(b, r, camera, count)) {
                    s.fov = Some(zoom);
                }
                s.aims = (r.focus == Some(RigFocus::Follow))
                    .then(|| frames.iter().map(|(_, at)| *at).collect());
            }
        }
    }
    Ok(match &c.owner {
        Owner::Object(name) => {
            let scale = b
                .recipe
                .objects
                .iter()
                .find(|o| o.name == *name)
                .map_or(DVec3::ONE, |o| DVec3::from_array(o.scale));
            let subs = follow::subs(b, name)?;
            follow::nodes(
                name,
                &s.poses,
                s.scales.as_deref(),
                scale,
                s.drive.as_ref().map(|(drive, route)| (drive, *route)),
                (b.nodes, &subs),
                b.facts.get(name),
            )
        }
        Owner::Camera(name) => {
            let Some(camera) = b.recipe.cameras.iter().find(|camera| camera.name == *name) else {
                return Ok(Vec::new());
            };
            let keys = s.poses.iter().map(|m| trs_from_matrix(*m)).collect();
            let mut track = camera_track(camera, b.timeline, keys);
            if let Some(aims) = &s.aims {
                track.focus = Some(
                    s.poses
                        .iter()
                        .zip(aims)
                        .map(|(m, aim)| m.w_axis.truncate().distance(*aim))
                        .collect(),
                );
            }
            if let Some(fov) = s.fov {
                if let (Some(lens), Some(first)) = (&mut track.camera, fov.first()) {
                    lens.yfov = first.to_radians();
                }
                track.fov = Some(fov);
            }
            vec![track]
        }
        Owner::Light(name) => vec![Track {
            node: format!("{LIGHT_PREFIX}{name}"),
            keys: s.poses.iter().map(|m| trs_from_matrix(*m)).collect(),
            camera: None,
            focus: None,
            fov: None,
        }],
    })
}

fn rigid(m: DMat4) -> DMat4 {
    let (_, turn, at) = m.to_scale_rotation_translation();
    DMat4::from_rotation_translation(turn.normalize(), at)
}

fn base(b: &Bake, owner: &Owner) -> Result<DMat4, String> {
    match owner {
        Owner::Object(name) => {
            let piece = format!("{name}.");
            Ok(b.nodes
                .iter()
                .find(|n| n.name == *name)
                .or_else(|| b.nodes.iter().find(|n| n.name.starts_with(&piece)))
                .map_or(DMat4::IDENTITY, |n| rigid(n.transform)))
        }
        Owner::Camera(name) => match b.recipe.cameras.iter().find(|c| c.name == *name) {
            Some(camera) => standing(camera),
            None => Ok(DMat4::IDENTITY),
        },
        Owner::Light(name) => Ok(b
            .recipe
            .lights
            .iter()
            .find(|l| l.name == *name)
            .map_or(DMat4::IDENTITY, |l| {
                DMat4::from_translation(DVec3::from(l.position))
            })),
    }
}

fn standing(camera: &manfred_recipe::Camera) -> Result<DMat4, String> {
    let eye = DVec3::from(camera.position);
    let turn =
        rig::view(eye, DVec3::from(camera.target), DVec3::from(camera.up)).map_err(|_| {
            format!(
                "camera[{}]: the camera sits on its target or looks straight along `up`",
                camera.name
            )
        })?;
    Ok(DMat4::from_rotation_translation(turn, eye))
}

fn split(m: DMat4) -> Frame {
    let (scale, turn, at) = m.to_scale_rotation_translation();
    (turn.normalize(), at, scale)
}

fn target(b: &Bake, known: &[Track], name: &str, at: &str) -> Result<Vec<Frame>, String> {
    let count = b.timeline.frames as usize + 1;
    let piece = format!("{name}.");
    let wheel = format!("{name}.wheel_");
    let track = known.iter().find(|t| t.node == name).or_else(|| {
        known
            .iter()
            .find(|t| t.node.starts_with(&piece) && !t.node.starts_with(&wheel))
    });
    if let Some(track) = track {
        if track.keys.len() < count {
            return Err(format!(
                "{at}: the track of `{}` has {} keys, expected {count}",
                track.node,
                track.keys.len()
            ));
        }
        return Ok(track.keys[..count]
            .iter()
            .map(|key| split(matrix(key)))
            .collect());
    }
    let standing = b
        .nodes
        .iter()
        .find(|n| n.name == name)
        .or_else(|| b.nodes.iter().find(|n| n.name.starts_with(&piece)))
        .map(|n| Ok(n.transform))
        .or_else(|| {
            let camera = name.strip_prefix(manfred_glb::CAMERA_PREFIX)?;
            let camera = b.recipe.cameras.iter().find(|c| c.name == camera)?;
            Some(standing(camera))
        })
        .or_else(|| {
            let light = name.strip_prefix(LIGHT_PREFIX)?;
            let light = b.recipe.lights.iter().find(|l| l.name == light)?;
            Some(Ok(DMat4::from_translation(DVec3::from(light.position))))
        });
    match standing {
        Some(m) => Ok(vec![split(m?); count]),
        None => Err(format!(
            "{at}: `{name}` names nothing that moves or stands in the built scene, expected an object, one of its pieces' nodes, `camera.<name>` or `light.<name>`"
        )),
    }
}

fn route_of<'a>(b: &'a Bake, all: &[Constrained], name: &str) -> Option<&'a Route> {
    let owner = b.recipe.target_owner(name)?;
    let entry = all.iter().find(|c| c.owner == owner)?;
    entry.list.iter().rev().find_map(|c| match &c.kind {
        ConstraintKind::FollowPath(f) => b.routes.get(&f.route),
        _ => None,
    })
}

fn fixed(axis: Axis) -> DQuat {
    match axis {
        Axis::NegZ => DQuat::IDENTITY,
        Axis::Z => DQuat::from_rotation_y(PI),
        Axis::X => DQuat::from_rotation_y(FRAC_PI_2),
        Axis::NegX => DQuat::from_rotation_y(-FRAC_PI_2),
        Axis::Y => DQuat::from_rotation_x(-FRAC_PI_2),
        Axis::NegY => DQuat::from_rotation_x(FRAC_PI_2),
    }
}

fn look_at(
    b: &Bake,
    known: &[Track],
    owner: &Owner,
    constraint: &Constraint,
    l: &LookAt,
    s: &mut State,
) -> Result<(), String> {
    let targets = target(b, known, &l.target, &format!("{}.target", constraint.path))?;
    let up = DVec3::from(l.up);
    let offset = DVec3::from(l.offset);
    let axis = l.axis.unwrap_or(match owner {
        Owner::Object(_) => Axis::Z,
        Owner::Camera(_) | Owner::Light(_) => Axis::NegZ,
    });
    let fix = fixed(axis);
    for (n, m) in s.poses.iter_mut().enumerate() {
        let (turn, at, _) = targets[n];
        let aim = at + turn * offset;
        let eye = m.w_axis.truncate();
        let look = rig::view(eye, aim, up).map_err(|_| {
            format!(
                "{}: at frame {n} the owner sits on its aim point or looks straight along `up`",
                constraint.path
            )
        })?;
        *m = DMat4::from_rotation_translation((look * fix).normalize(), eye);
    }
    Ok(())
}

fn copied(
    b: &Bake,
    known: &[Track],
    constraint: &Constraint,
    copy: &CopyOf,
    s: &mut State,
) -> Result<(), String> {
    let targets = target(
        b,
        known,
        &copy.target,
        &format!("{}.target", constraint.path),
    )?;
    let offset = DVec3::from(copy.offset);
    let turned = DQuat::from_mat4(&rotation(copy.rotate)).normalize();
    if copy.scale {
        s.scales = Some(targets.iter().map(|(_, _, scale)| *scale).collect());
    }
    for (n, m) in s.poses.iter_mut().enumerate() {
        let (turn, at, _) = targets[n];
        let (_, own_turn, own_at) = m.to_scale_rotation_translation();
        let place = if copy.translation {
            at + turn * offset
        } else {
            own_at
        };
        let facing = if copy.rotation {
            (turn * turned).normalize()
        } else {
            own_turn.normalize()
        };
        *m = DMat4::from_rotation_translation(facing, place);
    }
    Ok(())
}
