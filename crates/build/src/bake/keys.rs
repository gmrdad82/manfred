use std::collections::{BTreeMap, BTreeSet};

use glam::{DMat4, DVec3};
use manfred_motion::trs_from_matrix;
use manfred_recipe::{Owner, Recipe};

use super::rig::view;
use crate::{Bake, Track, camera_track, rotation};

fn carrier(recipe: &Recipe, index: usize) -> Option<usize> {
    let parent = recipe.objects[index].parent.as_ref()?;
    recipe.objects.iter().rposition(|o| o.name == *parent)
}

fn keyed(recipe: &Recipe, name: &str) -> bool {
    recipe.keyed.contains_key(&format!("object.{name}"))
}

pub(crate) fn animates(recipe: &Recipe) -> Vec<String> {
    let followed: BTreeSet<String> = super::constraint::animates(recipe).into_iter().collect();
    (0..recipe.objects.len())
        .filter(|&index| {
            let mut at = index;
            for _ in 0..=recipe.objects.len() {
                let name = recipe.objects[at].name.as_str();
                if followed.contains(name) {
                    return false;
                }
                if keyed(recipe, name) {
                    return true;
                }
                match carrier(recipe, at) {
                    Some(next) => at = next,
                    None => return false,
                }
            }
            false
        })
        .map(|index| recipe.objects[index].name.clone())
        .collect()
}

fn world_at(recipe: &Recipe, index: usize, time: f64) -> DMat4 {
    let mut matrix = DMat4::IDENTITY;
    let mut at = index;
    for _ in 0..=recipe.objects.len() {
        let object = &recipe.objects[at];
        let channel = |property: &str, rest: [f64; 3]| {
            recipe
                .channel(&format!("object.{}.{property}", object.name))
                .map_or(rest, |c| c.vec3(time))
        };
        let local = DMat4::from_translation(DVec3::from(channel("at", object.at)))
            * rotation(channel("rotate", object.rotate))
            * DMat4::from_scale(DVec3::from(channel("scale", object.scale)));
        matrix = local * matrix;
        match carrier(recipe, at) {
            Some(next) => at = next,
            None => return matrix,
        }
    }
    matrix
}

fn owner<'a>(names: &'a [&'a str], node: &str) -> Option<&'a str> {
    names
        .iter()
        .filter(|name| {
            node == **name
                || node
                    .strip_prefix(**name)
                    .is_some_and(|rest| rest.starts_with('.'))
        })
        .max_by_key(|name| name.len())
        .copied()
}

pub(crate) fn bake(b: &Bake) -> Result<Vec<Track>, String> {
    let recipe = b.recipe;
    let times: Vec<f64> = (0..=b.timeline.frames)
        .map(|n| b.timeline.time(n))
        .collect();
    let moving: BTreeSet<String> = animates(recipe).into_iter().collect();
    let mut tracks = Vec::new();
    if !moving.is_empty() {
        let names: Vec<&str> = recipe.objects.iter().map(|o| o.name.as_str()).collect();
        let mut worlds: BTreeMap<&str, Vec<DMat4>> = BTreeMap::new();
        for (index, object) in recipe.objects.iter().enumerate() {
            if moving.contains(&object.name) {
                worlds.insert(
                    object.name.as_str(),
                    times.iter().map(|t| world_at(recipe, index, *t)).collect(),
                );
            }
        }
        for node in b.nodes {
            let Some(world) = owner(&names, &node.name).and_then(|name| worlds.get(name)) else {
                continue;
            };
            tracks.push(Track {
                node: node.name.clone(),
                keys: world.iter().map(|m| trs_from_matrix(*m)).collect(),
                camera: None,
                focus: None,
                fov: None,
            });
        }
    }
    let constrained = recipe.constrained();
    for camera in &recipe.cameras {
        let entry = format!("camera.{}", camera.name);
        let position = recipe.channel(&format!("{entry}.position"));
        let target = recipe.channel(&format!("{entry}.target"));
        let owner = Owner::Camera(camera.name.clone());
        if (position.is_none() && target.is_none()) || constrained.iter().any(|c| c.owner == owner)
        {
            continue;
        }
        let up = DVec3::from(camera.up);
        let keys = times
            .iter()
            .map(|t| {
                let eye = DVec3::from(position.map_or(camera.position, |c| c.vec3(*t)));
                let aim = DVec3::from(target.map_or(camera.target, |c| c.vec3(*t)));
                let turn = view(eye, aim, up)
                    .map_err(|_| format!("camera[{}].keys: at {t} s the camera sits on its target or looks straight along `up`", camera.name))?;
                Ok(trs_from_matrix(DMat4::from_rotation_translation(turn, eye)))
            })
            .collect::<Result<Vec<_>, String>>()?;
        tracks.push(camera_track(camera, b.timeline, keys));
    }
    Ok(tracks)
}
