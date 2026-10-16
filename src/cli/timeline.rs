use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use gltf::animation::util::ReadOutputs;
use manfred_build::{Registry, build_with};
use manfred_recipe::toml_edit::{DocumentMut, Item};
use manfred_recipe::{Recipe, load, validate};
use serde_json::{Value, json};

use super::{print_problems, read_failure, refuse};

#[derive(clap::Args)]
pub(super) struct Args {
    #[arg(value_name = "recipe", help = "The recipe whose timeline to report")]
    recipe: String,
    #[arg(long, help = "Print one JSON object")]
    json: bool,
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Keyed {
    pub(super) keys: usize,
    pub(super) channels: Vec<&'static str>,
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Facts {
    pub(super) nodes: BTreeMap<String, Keyed>,
    pub(super) cameras: Vec<String>,
    pub(super) paths: BTreeMap<String, Vec<[f64; 3]>>,
}

pub(super) fn facts(glb: &[u8]) -> Result<Facts, String> {
    let gltf = gltf::Gltf::from_slice(glb).map_err(|e| format!("cannot read the GLB: {e}"))?;
    let blob = gltf.blob.as_deref();
    let mut facts = Facts::default();
    for node in gltf.document.nodes() {
        if node.camera().is_some() {
            facts
                .cameras
                .push(node.name().unwrap_or("unnamed").to_string());
        }
    }
    let Some(animation) = gltf
        .document
        .animations()
        .find(|a| a.name() == Some("timeline"))
    else {
        return Ok(facts);
    };
    for channel in animation.channels() {
        let name = channel
            .target()
            .node()
            .name()
            .unwrap_or("unnamed")
            .to_string();
        let reader = channel.reader(|buffer| match buffer.source() {
            gltf::buffer::Source::Bin => blob,
            gltf::buffer::Source::Uri(_) => None,
        });
        let times = reader.read_inputs().map_or(0, Iterator::count);
        let (property, outputs) = match reader.read_outputs() {
            Some(ReadOutputs::Translations(values)) => (
                "translation",
                Some(values.map(|v| v.map(f64::from)).collect::<Vec<_>>()),
            ),
            Some(ReadOutputs::Rotations(_)) => ("rotation", None),
            Some(ReadOutputs::Scales(_)) => ("scale", None),
            Some(ReadOutputs::MorphTargetWeights(_)) | None => ("weights", None),
        };
        let keyed = facts.nodes.entry(name.clone()).or_default();
        keyed.keys = keyed.keys.max(times);
        keyed.channels.push(property);
        if let Some(path) = outputs {
            facts.paths.insert(name, path);
        }
    }
    Ok(facts)
}

pub(super) fn followed(recipe: &Recipe) -> Vec<String> {
    let Ok(doc) = recipe.source.parse::<DocumentMut>() else {
        return Vec::new();
    };
    match doc.get("follow") {
        Some(Item::ArrayOfTables(list)) => list
            .iter()
            .filter_map(|t| t.get("object").and_then(Item::as_str))
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

pub(super) fn travel(facts: &Facts, object: &str, fps: u32) -> Option<(String, f64, f64)> {
    let prefix = format!("{object}.");
    let node = [object.to_string(), format!("{object}.body")]
        .into_iter()
        .find(|name| facts.paths.contains_key(name))
        .or_else(|| {
            facts
                .paths
                .keys()
                .find(|name| name.starts_with(&prefix))
                .cloned()
        })?;
    let path = &facts.paths[&node];
    let steps: Vec<f64> = path
        .windows(2)
        .map(|pair| {
            let d = [0, 1, 2].map(|i| pair[1][i] - pair[0][i]);
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        })
        .collect();
    let distance = steps.iter().sum();
    let top = steps.iter().copied().fold(0.0, f64::max) * f64::from(fps);
    Some((node, distance, top))
}

fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub(super) fn chosen(file: &str) -> Result<Recipe, String> {
    let recipe = load(Path::new(file))
        .map_err(|e| read_failure(file, &e))?
        .recipe;
    let mut problems = Vec::new();
    print_problems(&mut problems, file, &validate(&recipe));
    if problems.is_empty() {
        Ok(recipe)
    } else {
        Err(String::from_utf8_lossy(&problems).trim_end().to_string())
    }
}

pub(super) fn base(file: &str) -> &Path {
    Path::new(file)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn report(args: &Args, registry: &Registry, out: &mut dyn Write) -> Result<(), String> {
    let file = args.recipe.as_str();
    let recipe = chosen(file)?;
    let timeline = recipe
        .timeline
        .ok_or_else(|| format!("{file} has no [timeline], so nothing moves"))?;
    let built = build_with(&recipe, base(file), registry).map_err(|e| format!("{file}: {e}"))?;
    let glb = built.try_glb().map_err(|e| format!("{file}: {e}"))?;
    let facts = facts(&glb)?;
    let follows: Vec<Value> = followed(&recipe)
        .into_iter()
        .map(|object| match travel(&facts, &object, timeline.fps) {
            Some((node, distance, top)) => json!({
                "object": object,
                "node": node,
                "distance_m": round(distance),
                "top_speed_m_s": round(top),
            }),
            None => {
                json!({ "object": object, "node": null, "distance_m": null, "top_speed_m_s": null })
            }
        })
        .collect();
    let camera = facts.cameras.first().cloned();
    let markers: Vec<Value> = recipe
        .markers
        .iter()
        .map(|m| {
            json!({
                "name": m.name,
                "t": m.t,
                "frame": (m.t * f64::from(timeline.fps) + 1e-6).floor() as u64,
            })
        })
        .collect();
    if args.json {
        let nodes: Vec<Value> = facts
            .nodes
            .iter()
            .map(|(name, keyed)| json!({ "node": name, "keys": keyed.keys, "channels": keyed.channels }))
            .collect();
        let line = json!({
            "recipe": file,
            "frames": u64::from(timeline.frames) + 1,
            "last_frame": timeline.frames,
            "fps": timeline.fps,
            "seconds": timeline.seconds(),
            "size": timeline.size,
            "camera": camera,
            "cameras": facts.cameras,
            "keyed": nodes,
            "follow": follows,
            "markers": markers,
        });
        let _ = writeln!(out, "{line}");
        return Ok(());
    }
    let _ = writeln!(
        out,
        "{file}: {} frames (0 to {}) at {} fps, {} s, {}x{}",
        u64::from(timeline.frames) + 1,
        timeline.frames,
        timeline.fps,
        timeline.seconds(),
        timeline.size[0],
        timeline.size[1]
    );
    let _ = writeln!(out, "camera: {}", camera.as_deref().unwrap_or("none keyed"));
    let _ = writeln!(out, "keyed: {} nodes", facts.nodes.len());
    for (name, keyed) in &facts.nodes {
        let _ = writeln!(
            out,
            "  {name}: {} keys ({})",
            keyed.keys,
            keyed.channels.join(", ")
        );
    }
    for follow in &follows {
        let object = follow["object"].as_str().unwrap_or_default();
        match (
            follow["distance_m"].as_f64(),
            follow["top_speed_m_s"].as_f64(),
        ) {
            (Some(distance), Some(top)) => {
                let _ = writeln!(out, "follow {object}: {distance} m, top speed {top} m/s");
            }
            _ => {
                let _ = writeln!(out, "follow {object}: no keyed node");
            }
        }
    }
    for marker in &markers {
        let _ = writeln!(
            out,
            "marker {}: {} s, frame {}",
            marker["name"].as_str().unwrap_or_default(),
            marker["t"],
            marker["frame"]
        );
    }
    Ok(())
}

pub fn run(args: &Args, registry: &Registry) -> ExitCode {
    match report(args, registry, &mut std::io::stdout()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => refuse("timeline", &message),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn timeline_takes_a_recipe_and_json() {
        let parsed = |words: &[&str]| match super::super::parsed(&[&["timeline"], words].concat()) {
            Ok(super::super::Verb::Timeline(args)) => Ok((args.recipe, args.json)),
            Ok(_) => Err("not timeline".to_string()),
            Err(error) => Err(error),
        };
        assert_eq!(parsed(&["a.toml", "--json"]), Ok(("a.toml".into(), true)));
        assert_eq!(parsed(&["a.toml"]), Ok(("a.toml".into(), false)));
        assert!(parsed(&[]).is_err());
        assert!(parsed(&["a.toml", "--frames", "3"]).is_err());
    }
}
