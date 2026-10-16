use std::collections::{BTreeMap, BTreeSet};

use glam::{DQuat, DVec3};
use manfred_glb::{CAMERA_PREFIX, Lens, Node, Track, ZNEAR};
use manfred_motion::{Route, Timeline, Trs, matrix, sample};
use manfred_recipe::{Camera, Recipe};

use crate::{Built, Error, Registry};

pub struct Bake<'a> {
    pub recipe: &'a Recipe,
    pub nodes: &'a [Node],
    pub facts: &'a serde_json::Map<String, serde_json::Value>,
    pub timeline: &'a Timeline,
    pub routes: &'a BTreeMap<String, Route>,
    pub tracks: &'a [Track],
}

pub type Baker = fn(&Bake) -> Result<Vec<Track>, String>;

pub type Animates = fn(&Recipe) -> Vec<String>;

pub fn lens(camera: &Camera, timeline: &Timeline) -> Lens {
    let aspect = timeline.aspect();
    Lens {
        yfov: camera.fov_y_deg(aspect).to_radians(),
        aspect,
        znear: ZNEAR,
    }
}

pub fn camera_track(camera: &Camera, timeline: &Timeline, keys: Vec<Trs>) -> Track {
    Track {
        node: format!("{CAMERA_PREFIX}{}", camera.name),
        keys,
        camera: Some(lens(camera, timeline)),
        focus: None,
        fov: None,
    }
}

pub(crate) fn animated(recipe: &Recipe, registry: &Registry) -> BTreeSet<String> {
    if recipe.timeline.is_none() {
        return BTreeSet::new();
    }
    registry
        .animates
        .iter()
        .flat_map(|animates| animates(recipe))
        .collect()
}

pub(crate) fn kept(recipe: &Recipe, animated: &BTreeSet<String>) -> Recipe {
    let mut kept = recipe.clone();
    for object in &mut kept.objects {
        if animated.contains(&object.name) {
            object.keep_transform = true;
        }
    }
    kept
}

pub(crate) fn bake(
    recipe: &Recipe,
    built: &Built,
    routes: &BTreeMap<String, Route>,
    registry: &Registry,
) -> Result<Vec<Track>, Error> {
    let Some(timeline) = &recipe.timeline else {
        return Ok(Vec::new());
    };
    let keys = timeline.frames as usize + 1;
    let owners: Vec<String> = recipe
        .constrained()
        .into_iter()
        .filter_map(|c| match c.owner {
            manfred_recipe::Owner::Object(name) => Some(name),
            _ => None,
        })
        .collect();
    let followed = |piece: &str| {
        owners.iter().any(|owner| {
            piece == owner
                || piece
                    .strip_prefix(owner.as_str())
                    .is_some_and(|rest| rest.starts_with('.'))
        })
    };
    let mut tracks: Vec<Track> = built
        .clips
        .iter()
        .filter(|(piece, k)| !followed(piece) && k.len() == keys)
        .map(|(piece, k)| Track {
            node: piece.clone(),
            keys: k.clone(),
            camera: None,
            focus: None,
            fov: None,
        })
        .collect();
    if registry.bakers.is_empty() {
        return Ok(tracks);
    }
    let nodes: Vec<Node> = built.parts.iter().map(|p| p.node.clone()).collect();
    for (name, baker) in &registry.bakers {
        let bake = Bake {
            recipe,
            nodes: &nodes,
            facts: &built.facts_by_object,
            timeline,
            routes,
            tracks: &tracks,
        };
        let made =
            baker(&bake).map_err(|text| Error::new("timeline", format!("{name}: {text}")))?;
        for track in made {
            let focus = track.focus.as_ref().map_or(keys, Vec::len);
            let fov = track.fov.as_ref().map_or(keys, Vec::len);
            if track.keys.len() != keys || focus != keys || fov != keys {
                return Err(Error::new(
                    "timeline",
                    format!(
                        "{name}: the track of `{}` has {} keys, expected {keys}, one a frame from 0 to {}",
                        track.node,
                        track.keys.len().min(focus).min(fov),
                        timeline.frames
                    ),
                ));
            }
            let known = match (track.camera_name(), track.light_name()) {
                (Some(camera), _) => recipe.cameras.iter().any(|c| c.name == camera),
                (None, light) => {
                    nodes.iter().any(|n| n.name == track.node)
                        || light.is_some_and(|light| recipe.lights.iter().any(|l| l.name == light))
                }
            };
            if !known || tracks.iter().any(|t| t.node == track.node) {
                return Err(Error::new(
                    "timeline",
                    format!(
                        "{name}: the track names `{}`, expected a node or camera of the scene that no other track names",
                        track.node
                    ),
                ));
            }
            tracks.push(track);
        }
    }
    Ok(tracks)
}

fn lerp(values: &[f64], fps: u32, time: f64) -> Option<f64> {
    let last = values.len().checked_sub(1)?;
    let at = (time * f64::from(fps.max(1))).clamp(0.0, last as f64);
    let low = (at.floor() as usize).min(last);
    let high = (low + 1).min(last);
    let s = at - low as f64;
    Some(values[low] + (values[high] - values[low]) * s)
}

pub fn time_refusal(recipe: &Recipe, time: f64) -> Option<String> {
    match &recipe.timeline {
        None => Some(
            "--time poses the recipe's [timeline], and the recipe has none (docs/recipe.md, Timeline)"
                .to_string(),
        ),
        Some(timeline) if !timeline.holds(time) => Some(format!(
            "--time {time} lies outside the timeline, expected 0 to {} seconds",
            timeline.seconds()
        )),
        Some(_) => None,
    }
}

fn posed_camera(camera: &mut Camera, trs: &Trs) {
    let turn = DQuat::from_array(trs.rotation).normalize();
    let position = DVec3::from(trs.translation);
    let reach = (DVec3::from(camera.target) - DVec3::from(camera.position)).length();
    let reach = if reach > 0.0 && reach.is_finite() {
        reach
    } else {
        1.0
    };
    camera.position = trs.translation;
    camera.target = (position + turn * DVec3::NEG_Z * reach).to_array();
    camera.up = (turn * DVec3::Y).to_array();
    camera.path = None;
}

pub fn pose_cameras(recipe: &Recipe, built: &Built, time: f64) -> Result<Recipe, Error> {
    if let Some(text) = time_refusal(recipe, time) {
        return Err(Error::new("timeline", text));
    }
    let fps = recipe.timeline.map_or(1, |t| t.fps);
    let mut recipe = recipe.keyed_at(time);
    for track in &built.tracks {
        if let Some(name) = track.light_name()
            && !built.parts.iter().any(|p| p.node.name == track.node)
            && let Some(light) = recipe.lights.iter_mut().find(|l| l.name == name)
        {
            let trs = sample(&track.keys, fps, time);
            let turn = DQuat::from_array(trs.rotation).normalize();
            light.position = trs.translation;
            if let Some(direction) = &mut light.direction {
                *direction = (turn * DVec3::from(*direction)).to_array();
            }
            continue;
        }
        let Some(name) = track.camera_name() else {
            continue;
        };
        if let Some(camera) = recipe.cameras.iter_mut().find(|c| c.name == name) {
            posed_camera(camera, &sample(&track.keys, fps, time));
            if let Some(focus) = track.focus.as_deref().and_then(|f| lerp(f, fps, time)) {
                camera.focus_m = Some(focus);
            }
            if let Some(fov) = track.fov.as_deref().and_then(|f| lerp(f, fps, time)) {
                camera.fov_y_deg = Some(fov);
                camera.focal_mm = None;
            }
        }
    }
    Ok(recipe)
}

pub fn pose(recipe: &Recipe, built: &Built, time: f64) -> Result<(Recipe, Built), Error> {
    let recipe = pose_cameras(recipe, built, time)?;
    let fps = recipe.timeline.map_or(1, |t| t.fps);
    let mut built = built.clone();
    for track in built.tracks.iter().filter(|t| t.camera_name().is_none()) {
        let trs = sample(&track.keys, fps, time);
        for part in built.parts.iter_mut().filter(|p| p.node.name == track.node) {
            part.node.transform = matrix(&trs);
            part.node.keep_transform = true;
        }
    }
    Ok((recipe, built))
}
