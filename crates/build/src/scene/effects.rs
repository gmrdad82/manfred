use super::{Paths, shown};
use crate::bake::{Section, list, number, string};
use crate::{Built, Error};
use manfred_recipe::{Colour, LightKind, Recipe, Setting, Settings};
use pfx_scene::types::Wet;
use std::path::Path;

pub(super) fn format_1_key(recipe: &Recipe) -> Option<String> {
    if let Some(key) = recipe.effects.first_key() {
        return Some(key);
    }
    if recipe.look.as_ref().is_some_and(|look| look.lut.is_some()) {
        return Some("look.lut".to_string());
    }
    recipe
        .lights
        .iter()
        .find(|light| {
            light.kind == LightKind::Spot
                || light.parent.is_some()
                || light.direction.is_some()
                || light.cone.is_some()
        })
        .map(|light| format!("light[{}]", light.name))
}

pub(super) fn file(paths: &Paths, base: &Path, file: &str, key: &str) -> Result<String, Error> {
    let path = base.join(file);
    if !path.is_file() {
        return Err(Error::new(
            key,
            format!(
                "{} is not a file, expected a path relative to the recipe",
                path.display()
            ),
        ));
    }
    paths.relative(&path, key)
}

pub(super) struct Parent {
    object: String,
    node: Option<String>,
}

impl Parent {
    pub(super) fn put(&self, s: &mut Section<'_>) {
        s.put("parent", &string(&self.object));
        if let Some(node) = &self.node {
            s.put("node", &string(node));
        }
    }
}

pub(super) fn whole(recipe: &Recipe, built: &Built, format: u32, glb: &Path) -> Option<String> {
    if format != 1 || recipe.timeline.is_none() || built.tracks.is_empty() {
        return None;
    }
    let stem = glb.file_stem().and_then(|stem| stem.to_str()).unwrap_or("");
    let named = !stem.is_empty()
        && stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    Some(if named { stem } else { "timeline" }.to_string())
}

pub(super) fn parent(
    built: &Built,
    recipe: &Recipe,
    whole: Option<&str>,
    name: &str,
    key: &str,
) -> Result<Parent, Error> {
    let mut parts = built.parts.iter().filter(|part| shown(part));
    let node = if parts.clone().any(|part| part.node.name == name) {
        name.to_string()
    } else {
        let owner = recipe.objects.iter().position(|object| object.name == name);
        match owner.and_then(|index| parts.find(|part| part.object == index)) {
            Some(part) => part.node.name.clone(),
            None => {
                return Err(Error::new(
                    key,
                    format!(
                        "\"{name}\" names no object or piece of the built scene, expected a shown object's name or one of its pieces' node names"
                    ),
                ));
            }
        }
    };
    Ok(match whole {
        Some(object) => Parent {
            object: object.to_string(),
            node: Some(node),
        },
        None => Parent {
            object: node,
            node: None,
        },
    })
}

fn colour(value: &Colour, key: &str) -> Result<String, Error> {
    match value {
        Colour::Hex(hex) => Ok(string(hex)),
        Colour::Rgb(rgb) => list(rgb, key),
    }
}

fn text(value: &Setting, key: &str) -> Result<String, Error> {
    Ok(match value {
        Setting::Number(v) => number(*v, key)?,
        Setting::Whole(v) => v.to_string(),
        Setting::Flag(v) => v.to_string(),
        Setting::Word(v) => string(v),
        Setting::Numbers(v) => list(v, key)?,
        Setting::Keys(keys) => {
            let items: Result<Vec<String>, Error> = keys.iter().map(|k| list(k, key)).collect();
            format!("[{}]", items?.join(", "))
        }
        Setting::Colour(c) => colour(c, key)?,
        Setting::Colours(list) => {
            let mut items = Vec::new();
            for (t, c) in list {
                items.push(format!("[{}, {}]", number(*t, key)?, colour(c, key)?));
            }
            format!("[{}]", items.join(", "))
        }
    })
}

pub(super) fn put(s: &mut Section<'_>, settings: &Settings) -> Result<(), Error> {
    for (key, value) in settings {
        let at = format!("{}.{key}", s.at);
        s.put(key, &text(value, &at)?);
    }
    Ok(())
}

pub(super) fn particles(
    out: &mut String,
    recipe: &Recipe,
    built: &Built,
    whole: Option<&str>,
) -> Result<(), Error> {
    for particles in &recipe.effects.particles {
        let at = format!("particles[{}]", particles.name);
        let parent = match &particles.parent {
            Some(name) => Some(parent(built, recipe, whole, name, &format!("{at}.parent"))?),
            None => None,
        };
        let mut s = Section::new(out, "[[particles]]", &at);
        s.put("name", &string(&particles.name));
        if let Some(parent) = parent {
            parent.put(&mut s);
        }
        put(&mut s, &particles.settings)?;
    }
    Ok(())
}

pub(super) fn tables(out: &mut String, recipe: &Recipe) -> Result<(), Error> {
    for (key, settings) in [
        ("flare", &recipe.effects.flare),
        ("motion_blur", &recipe.effects.motion_blur),
    ] {
        if let Some(settings) = settings {
            let mut s = Section::new(out, &format!("[{key}]"), key);
            put(&mut s, settings)?;
        }
    }
    Ok(())
}

pub(super) fn wet(settings: &Settings) -> Wet {
    let mut wet = Wet::default();
    for (key, value) in settings {
        match (*key, value) {
            ("amount", Setting::Number(v)) => wet.amount = Some(*v as f32),
            ("puddles", Setting::Number(v)) => wet.puddles = Some(*v as f32),
            ("puddle_scale", Setting::Number(v)) => wet.puddle_scale = Some(*v as f32),
            ("seed", Setting::Whole(v)) => wet.seed = Some(*v),
            _ => {}
        }
    }
    wet
}

pub(super) fn unplaced(text: String, recipe: &Recipe) -> String {
    if recipe.lights.iter().all(|light| light.parent.is_none()) {
        return text;
    }
    let Ok(mut doc) = text.parse::<manfred_recipe::toml_edit::DocumentMut>() else {
        return text;
    };
    let Some(lights) = doc
        .get_mut("light")
        .and_then(manfred_recipe::toml_edit::Item::as_array_of_tables_mut)
    else {
        return text;
    };
    for light in lights.iter_mut() {
        if light.contains_key("parent") {
            light.remove("position");
        }
    }
    doc.to_string()
}
