use crate::shape::{Source, fail, read_mesh};
use crate::{Built, Error, GENERATOR, Generated, GeneratedLight, Piece, world};
use glam::{DMat4, DQuat, DVec3};
use manfred_glb::{Looks, Node, Order, Targets};
use manfred_mesh::{Mesh, Morph};
use manfred_recipe::{Kind, Object, Recipe};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const LICENCE: &str = "MIT, Catalin Ilinca; original, no third-party models or brand marks";
pub const FOLDER: &str = "library";
pub const INDEX: &str = "index.json";
pub const MODEL: &str = "model.glb";
pub const SIDECAR: &str = "object.toml";
pub const THUMB: &str = "thumb.png";
pub const ROOT: &str = "body";

const EMISSIVE_STRENGTH: &str = "KHR_materials_emissive_strength";
const TRANSMISSION: &str = "KHR_materials_transmission";
const IOR: &str = "KHR_materials_ior";
const CLEARCOAT: &str = "KHR_materials_clearcoat";

#[derive(Clone, Debug, PartialEq)]
pub struct Sidecar {
    pub kind: String,
    pub name: String,
    pub tags: Vec<String>,
    pub licence: String,
    pub generator: Option<String>,
    pub params: BTreeMap<String, Value>,
    pub manfred: String,
    pub facts: Map<String, Value>,
}

#[derive(Clone, Debug)]
pub struct Exported {
    pub glb: Vec<u8>,
    pub sidecar: Sidecar,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub kind: String,
    pub name: String,
    pub tags: Vec<String>,
    pub size_m: [f64; 3],
    pub triangles: usize,
    pub model: String,
    pub sidecar: String,
    pub thumb: Option<String>,
    pub sha256: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub path: String,
    pub text: String,
}

fn word(part: &str) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

pub fn split_name(text: &str) -> Result<(&str, &str), String> {
    match text.split_once('/') {
        Some((kind, name)) if word(kind) && word(name) => Ok((kind, name)),
        _ => Err(format!(
            "expected \"<kind>/<name>\" in lowercase letters, digits, `_` and `-`, found \"{text}\""
        )),
    }
}

impl Sidecar {
    pub fn text(&self) -> String {
        let mut object = Map::new();
        object.insert("kind".into(), json!(self.kind));
        object.insert("name".into(), json!(self.name));
        object.insert("tags".into(), json!(self.tags));
        object.insert("licence".into(), json!(self.licence));
        let mut source = Map::new();
        if let Some(generator) = &self.generator {
            source.insert("generator".into(), json!(generator));
        }
        source.insert("manfred".into(), json!(self.manfred));
        source.insert("params".into(), json!(self.params));
        let mut root = Map::new();
        root.insert("object".into(), Value::Object(object));
        root.insert("source".into(), Value::Object(source));
        root.insert("facts".into(), Value::Object(denull(&self.facts)));
        toml::to_string(&Value::Object(root)).unwrap_or_default()
    }

    pub fn parse(text: &str) -> Result<Sidecar, String> {
        let table: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
        let value = serde_json::to_value(table).map_err(|e| e.to_string())?;
        let mut root = value.as_object().cloned().unwrap_or_default();
        let object = section(&mut root, "object", true)?;
        let source = section(&mut root, "source", true)?;
        let facts = section(&mut root, "facts", false)?;
        if let Some(key) = root.keys().next() {
            return Err(format!(
                "`{key}`: unknown table, expected [object], [source] and [facts]"
            ));
        }
        let text_of = |table: &Map<String, Value>, at: &str, key: &str| {
            table
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("{at}.{key}: missing, expected a string"))
        };
        known(&object, "object", &["kind", "name", "tags", "licence"])?;
        known(&source, "source", &["generator", "manfred", "params"])?;
        let tags = match object.get("tags") {
            None => Vec::new(),
            Some(Value::Array(list)) => list
                .iter()
                .map(|t| t.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
                .ok_or("object.tags: expected an array of strings")?,
            Some(_) => return Err("object.tags: expected an array of strings".into()),
        };
        let generator = match source.get("generator") {
            None => None,
            Some(Value::String(g)) => Some(g.clone()),
            Some(_) => return Err("source.generator: expected a string".into()),
        };
        let params = match source.get("params") {
            None => BTreeMap::new(),
            Some(Value::Object(p)) => p.clone().into_iter().collect(),
            Some(_) => return Err("source.params: expected a table".into()),
        };
        Ok(Sidecar {
            kind: text_of(&object, "object", "kind")?,
            name: text_of(&object, "object", "name")?,
            tags,
            licence: text_of(&object, "object", "licence")?,
            generator,
            params,
            manfred: text_of(&source, "source", "manfred")?,
            facts,
        })
    }

    pub fn read(path: &Path) -> Result<Sidecar, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Sidecar::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

fn section(
    root: &mut Map<String, Value>,
    key: &str,
    needed: bool,
) -> Result<Map<String, Value>, String> {
    match root.remove(key) {
        Some(Value::Object(table)) => Ok(table),
        Some(_) => Err(format!("`{key}`: expected a table")),
        None if needed => Err(format!("[{key}]: missing")),
        None => Ok(Map::new()),
    }
}

fn known(table: &Map<String, Value>, at: &str, keys: &[&str]) -> Result<(), String> {
    match table.keys().find(|k| !keys.contains(&k.as_str())) {
        Some(key) => Err(format!(
            "{at}.{key}: unknown key, expected one of {}",
            keys.join(", ")
        )),
        None => Ok(()),
    }
}

fn denull(map: &Map<String, Value>) -> Map<String, Value> {
    fn clean(value: &Value) -> Option<Value> {
        match value {
            Value::Null => None,
            Value::Object(map) => Some(Value::Object(denull(map))),
            Value::Array(list) => Some(Value::Array(list.iter().filter_map(clean).collect())),
            other => Some(other.clone()),
        }
    }
    map.iter()
        .filter_map(|(k, v)| clean(v).map(|v| (k.clone(), v)))
        .collect()
}

fn vec3(value: Option<&Value>) -> Option<DVec3> {
    let list = value?.as_array()?;
    match list.as_slice() {
        [x, y, z] => Some(DVec3::new(x.as_f64()?, y.as_f64()?, z.as_f64()?)),
        _ => None,
    }
}

fn centre(mesh: &Mesh) -> DVec3 {
    manfred_mesh::bounds(mesh).map_or(DVec3::ZERO, |[lo, hi]| {
        (DVec3::from_array(lo) + DVec3::from_array(hi)) * 0.5
    })
}

fn rounded(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

struct Gathered {
    name: String,
    mesh: Mesh,
    targets: Option<Box<Targets>>,
    parts: usize,
}

pub fn export(
    recipe: &Recipe,
    built: &Built,
    object: &str,
    as_name: &str,
) -> Result<Exported, Error> {
    let (kind, name) = split_name(as_name).map_err(|t| Error::new("as", t))?;
    let index = recipe
        .objects
        .iter()
        .position(|o| o.name == object)
        .ok_or_else(|| Error::new("object", format!("no object is named `{object}`")))?;
    let inverse = world(recipe, index)?.inverse();
    let mut facts = built
        .facts_by_object
        .get(object)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let prefix = format!("{object}.");
    let mut gathered: Vec<Gathered> = Vec::new();
    let mut kept = false;
    for part in built.parts.iter().filter(|p| p.object == index) {
        kept |= part.node.keep_transform;
        let local = inverse * part.node.transform;
        let mut mesh = part.node.mesh.clone();
        manfred_mesh::transform(&mut mesh, local);
        let targets = part.node.targets.as_ref().map(|t| {
            let mut base = t.base.clone();
            manfred_mesh::transform(&mut base, local);
            let mut morph: Morph = t.morph.clone();
            morph.transform(local);
            Box::new(Targets { base, morph })
        });
        let node = part
            .node
            .name
            .strip_prefix(&prefix)
            .unwrap_or(ROOT)
            .to_string();
        match gathered.iter_mut().find(|g| g.name == node) {
            Some(g) => {
                g.mesh.append(&mesh);
                g.targets = None;
                g.parts += 1;
            }
            None => gathered.push(Gathered {
                name: node,
                mesh,
                targets,
                parts: 1,
            }),
        }
    }
    if gathered.is_empty() {
        return Err(Error::new(
            "object",
            format!("`{object}` builds no pieces to export"),
        ));
    }
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for g in &gathered {
        if let Some([a, b]) = manfred_mesh::bounds(&g.mesh) {
            lo = lo.min(DVec3::from_array(a));
            hi = hi.max(DVec3::from_array(b));
        }
    }
    let tree = built.trees.get(object);
    let (nodes, parents, offset) = match tree {
        Some(tree) => from_tree(name, tree, gathered),
        None => {
            let (nodes, parents) = two_level(name, gathered, facts.get("wheels"));
            (nodes, parents, 0)
        }
    };
    let glb = manfred_glb::write_looks(&nodes, &parents, &[], &built.looks, Order::File, GENERATOR)
        .map_err(|e| Error::new("", e.0))?;
    let merged = built.recipe(recipe);
    let glb = pbr(&glb, &merged.materials).map_err(|t| Error::new("", t))?;
    let glb = match tree {
        Some(tree) => animate(&glb, tree, offset).map_err(|t| Error::new("", t))?,
        None => glb,
    };
    let lights: Vec<Value> = built
        .lights
        .iter()
        .filter_map(|light| {
            let local = light.name.strip_prefix(&prefix)?;
            let in_frame = kept && light.parent.is_some();
            let (position, direction) = if in_frame {
                (
                    DVec3::from_array(light.position),
                    DVec3::from_array(light.direction),
                )
            } else {
                (
                    inverse.transform_point3(DVec3::from_array(light.position)),
                    inverse
                        .transform_vector3(DVec3::from_array(light.direction))
                        .normalize_or(DVec3::NEG_Y),
                )
            };
            let node = light
                .parent
                .as_deref()
                .and_then(|p| p.strip_prefix(&prefix))
                .unwrap_or(ROOT);
            Some(json!({
                "name": local,
                "node": node,
                "position": position.to_array().map(rounded),
                "direction": direction.to_array().map(rounded),
                "cone": light.cone,
                "range": light.range,
                "color": light.color,
                "intensity": light.intensity,
                "shadow": light.shadow,
            }))
        })
        .collect();
    if !lights.is_empty() {
        facts.insert("lights".into(), Value::Array(lights));
    }
    if lo.x.is_finite() {
        facts.insert("size".into(), json!((hi - lo).to_array().map(rounded)));
    }
    let source = &recipe.objects[index];
    let (generator, params) = match &source.kind {
        Kind::Generator { generator, params } => (Some(generator.clone()), params.clone()),
        _ => (None, BTreeMap::new()),
    };
    Ok(Exported {
        glb,
        sidecar: Sidecar {
            kind: kind.to_string(),
            name: name.to_string(),
            tags: Vec::new(),
            licence: LICENCE.to_string(),
            generator,
            params,
            manfred: env!("CARGO_PKG_VERSION").to_string(),
            facts,
        },
    })
}

fn node_of(name: String, g: Option<Gathered>, inverse: DMat4, transform: DMat4) -> Node {
    let (mut mesh, targets) = match g {
        Some(g) => {
            let single = g.parts == 1;
            (g.mesh, g.targets.filter(|_| single))
        }
        None => (Mesh::default(), None),
    };
    manfred_mesh::transform(&mut mesh, inverse);
    let targets = targets.map(|mut t| {
        manfred_mesh::transform(&mut t.base, inverse);
        t.morph.transform(inverse);
        t
    });
    Node {
        name,
        mesh,
        transform,
        keep_transform: true,
        targets,
    }
}

fn take(gathered: &mut Vec<Gathered>, name: &str) -> Option<Gathered> {
    let at = gathered.iter().position(|g| g.name == name)?;
    Some(gathered.remove(at))
}

fn two_level(
    name: &str,
    mut gathered: Vec<Gathered>,
    wheels: Option<&Value>,
) -> (Vec<Node>, Vec<Option<usize>>) {
    let root = take(&mut gathered, ROOT);
    let mut nodes = vec![node_of(
        name.to_string(),
        root,
        DMat4::IDENTITY,
        DMat4::IDENTITY,
    )];
    let mut parents = vec![None];
    for g in gathered {
        let pivot = g
            .name
            .strip_prefix("wheel_")
            .and_then(|corner| vec3(wheels?.get(corner)?.get("centre")))
            .unwrap_or_else(|| centre(&g.mesh));
        let at = DMat4::from_translation(pivot);
        nodes.push(node_of(g.name.clone(), Some(g), at.inverse(), at));
        parents.push(Some(0));
    }
    (nodes, parents)
}

fn from_tree(
    name: &str,
    tree: &Tree,
    mut gathered: Vec<Gathered>,
) -> (Vec<Node>, Vec<Option<usize>>, usize) {
    let roots = tree.nodes.iter().filter(|n| n.parent.is_none()).count();
    let offset = usize::from(roots != 1);
    let mut nodes = Vec::new();
    let mut parents = Vec::new();
    if offset == 1 {
        nodes.push(node_of(
            name.to_string(),
            None,
            DMat4::IDENTITY,
            DMat4::IDENTITY,
        ));
        parents.push(None);
    }
    for node in &tree.nodes {
        let g = take(&mut gathered, &node.name);
        let label = if offset == 0 && node.parent.is_none() {
            name.to_string()
        } else {
            node.name.clone()
        };
        nodes.push(node_of(label, g, node.world.inverse(), node.rest));
        parents.push(match node.parent {
            Some(p) => Some(p + offset),
            None if offset == 1 => Some(0),
            None => None,
        });
    }
    for g in gathered {
        nodes.push(node_of(
            g.name.clone(),
            Some(g),
            DMat4::IDENTITY,
            DMat4::IDENTITY,
        ));
        parents.push(Some(0));
    }
    (nodes, parents, offset)
}

fn push_accessor(
    json: &mut Value,
    bin: &mut Vec<u8>,
    data: &[f32],
    kind: &str,
    bounds: bool,
) -> Option<usize> {
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let offset = bin.len();
    for v in data {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    let root = json.as_object_mut()?;
    let views = root
        .entry("bufferViews")
        .or_insert_with(|| json!([]))
        .as_array_mut()?;
    views.push(json!({ "buffer": 0, "byteOffset": offset, "byteLength": data.len() * 4 }));
    let view = views.len() - 1;
    let width = match kind {
        "VEC3" => 3,
        "VEC4" => 4,
        _ => 1,
    };
    let mut accessor = json!({
        "bufferView": view,
        "componentType": 5126,
        "count": data.len() / width,
        "type": kind,
    });
    if bounds {
        let lo = data.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = data.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        accessor["min"] = json!([lo]);
        accessor["max"] = json!([hi]);
    }
    let accessors = root
        .entry("accessors")
        .or_insert_with(|| json!([]))
        .as_array_mut()?;
    accessors.push(accessor);
    Some(accessors.len() - 1)
}

fn animate(glb: &[u8], tree: &Tree, offset: usize) -> Result<Vec<u8>, String> {
    if tree.clips.iter().all(|c| c.channels.is_empty()) {
        return Ok(glb.to_vec());
    }
    let (mut json, mut bin) = split_glb(glb)?;
    let mut animations = Vec::new();
    for clip in &tree.clips {
        let mut samplers = Vec::new();
        let mut channels = Vec::new();
        for channel in &clip.channels {
            let kind = match channel.property {
                Property::Translation | Property::Scale => "VEC3",
                Property::Rotation => "VEC4",
                Property::Weights => "SCALAR",
            };
            let flat: Vec<f32> = channel.values.iter().flatten().copied().collect();
            let input = push_accessor(&mut json, &mut bin, &channel.times, "SCALAR", true)
                .ok_or("the glTF is not an object")?;
            let output = push_accessor(&mut json, &mut bin, &flat, kind, false)
                .ok_or("the glTF is not an object")?;
            samplers.push(json!({
                "input": input,
                "output": output,
                "interpolation": channel.interpolation,
            }));
            channels.push(json!({
                "sampler": samplers.len() - 1,
                "target": { "node": channel.node + offset, "path": channel.property.name() },
            }));
        }
        if channels.is_empty() {
            continue;
        }
        let mut animation = json!({ "channels": channels, "samplers": samplers });
        if let Some(name) = &clip.name {
            animation["name"] = json!(name);
        }
        animations.push(animation);
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let root = json.as_object_mut().ok_or("the glTF is not an object")?;
    root.insert("animations".into(), Value::Array(animations));
    let buffers = root.entry("buffers").or_insert_with(|| json!([{}]));
    if let Some(first) = buffers.as_array_mut().and_then(|b| b.first_mut()) {
        first["byteLength"] = json!(bin.len());
    }
    Ok(join_glb(&json, &bin))
}

pub fn split_glb(bytes: &[u8]) -> Result<(Value, Vec<u8>), String> {
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };
    if bytes.get(0..4) != Some(b"glTF") || word(4) != Some(2) {
        return Err("not a glTF 2.0 binary".into());
    }
    let length = word(12).ok_or("no JSON chunk")? as usize;
    if bytes.get(16..20) != Some(b"JSON") {
        return Err("the first chunk is not JSON".into());
    }
    let text = bytes.get(20..20 + length).ok_or("a short JSON chunk")?;
    let json: Value =
        serde_json::from_slice(text).map_err(|e| format!("the JSON chunk does not parse: {e}"))?;
    let at = 20 + length;
    let bin = match word(at) {
        Some(size) if bytes.get(at + 4..at + 8) == Some(b"BIN\0") => bytes
            .get(at + 8..at + 8 + size as usize)
            .ok_or("a short BIN chunk")?
            .to_vec(),
        _ => Vec::new(),
    };
    Ok((json, bin))
}

pub fn join_glb(json: &Value, bin: &[u8]) -> Vec<u8> {
    let mut text = serde_json::to_vec(json).unwrap_or_default();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let mut data = bin.to_vec();
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
    let total = 12 + 8 + text.len() + if data.is_empty() { 0 } else { 8 + data.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    if !data.is_empty() {
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&data);
    }
    out
}

fn number(spec: &Map<String, Value>, key: &str) -> Option<f64> {
    spec.get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
}

fn pbr(glb: &[u8], materials: &BTreeMap<String, Value>) -> Result<Vec<u8>, String> {
    let (mut json, bin) = split_glb(glb)?;
    let mut used = BTreeSet::new();
    if let Some(list) = json.get_mut("materials").and_then(Value::as_array_mut) {
        for material in list {
            let Some(spec) = material
                .get("name")
                .and_then(Value::as_str)
                .and_then(|n| materials.get(n))
                .and_then(Value::as_object)
                .cloned()
            else {
                continue;
            };
            let textured = material
                .get("pbrMetallicRoughness")
                .and_then(|p| p.get("baseColorTexture"))
                .is_some();
            if !textured {
                let base = vec3(spec.get("base")).unwrap_or(DVec3::splat(0.8));
                let base = base.clamp(DVec3::ZERO, DVec3::ONE);
                material["pbrMetallicRoughness"] = json!({
                    "baseColorFactor": [base.x, base.y, base.z, 1.0],
                    "metallicFactor": number(&spec, "metalness").unwrap_or(0.0).clamp(0.0, 1.0),
                    "roughnessFactor": number(&spec, "roughness").unwrap_or(0.5).clamp(0.0, 1.0),
                });
            }
            let mut extensions = Map::new();
            if let Some(emission) = vec3(spec.get("emission")).filter(|e| e.max_element() > 0.0) {
                let emission = emission.max(DVec3::ZERO);
                let strength = emission.max_element();
                if strength > 1.0 {
                    material["emissiveFactor"] = json!((emission / strength).to_array());
                    extensions.insert(
                        EMISSIVE_STRENGTH.into(),
                        json!({ "emissiveStrength": strength }),
                    );
                } else {
                    material["emissiveFactor"] = json!(emission.to_array());
                }
            }
            if let Some(t) = number(&spec, "transmission").filter(|t| *t > 0.0) {
                extensions.insert(
                    TRANSMISSION.into(),
                    json!({ "transmissionFactor": t.min(1.0) }),
                );
            }
            if let Some(ior) = number(&spec, "ior").filter(|i| *i >= 1.0) {
                extensions.insert(IOR.into(), json!({ "ior": ior }));
            }
            if let Some(c) = number(&spec, "clearcoat").filter(|c| *c > 0.0) {
                extensions.insert(
                    CLEARCOAT.into(),
                    json!({
                        "clearcoatFactor": c.min(1.0),
                        "clearcoatRoughnessFactor": number(&spec, "clearcoat_roughness").unwrap_or(0.0).clamp(0.0, 1.0),
                    }),
                );
            }
            if !extensions.is_empty() {
                used.extend(extensions.keys().cloned());
                let slot = material
                    .as_object_mut()
                    .ok_or("a material is not an object")?
                    .entry("extensions")
                    .or_insert_with(|| json!({}));
                if let Some(slot) = slot.as_object_mut() {
                    slot.extend(extensions);
                }
            }
        }
    }
    if !used.is_empty() {
        let list = json
            .as_object_mut()
            .ok_or("the glTF is not an object")?
            .entry("extensionsUsed")
            .or_insert_with(|| json!([]));
        if let Some(list) = list.as_array_mut() {
            for name in used {
                if !list.iter().any(|v| v.as_str() == Some(&name)) {
                    list.push(json!(name));
                }
            }
        }
    }
    Ok(join_glb(&json, &bin))
}

fn material_of(m: &Value) -> Value {
    let pbr = m.get("pbrMetallicRoughness");
    let factor = |key: &str, default: f64| {
        pbr.and_then(|p| p.get(key))
            .and_then(Value::as_f64)
            .unwrap_or(default)
    };
    let base = pbr
        .and_then(|p| p.get("baseColorFactor"))
        .and_then(Value::as_array)
        .map_or([1.0; 3], |c| {
            [0, 1, 2].map(|i| c.get(i).and_then(Value::as_f64).unwrap_or(1.0))
        });
    let ext = |name: &str, key: &str| {
        m.get("extensions")
            .and_then(|e| e.get(name))
            .and_then(|e| e.get(key))
            .and_then(Value::as_f64)
    };
    let mut out = Map::new();
    out.insert("base".into(), json!(base));
    out.insert("roughness".into(), json!(factor("roughnessFactor", 1.0)));
    let metalness = factor("metallicFactor", 1.0);
    if metalness > 0.0 {
        out.insert("metalness".into(), json!(metalness));
    }
    let emission = vec3(m.get("emissiveFactor"))
        .map(|e| e * ext(EMISSIVE_STRENGTH, "emissiveStrength").unwrap_or(1.0))
        .filter(|e| e.max_element() > 0.0);
    if let Some(e) = emission {
        out.insert("emission".into(), json!(e.to_array()));
    }
    let transmission = ext(TRANSMISSION, "transmissionFactor").filter(|t| *t > 0.0);
    if let Some(t) = transmission {
        out.insert("transmission".into(), json!(t));
    }
    if let Some(ior) = ext(IOR, "ior") {
        out.insert("ior".into(), json!(ior));
    }
    let clearcoat = ext(CLEARCOAT, "clearcoatFactor").filter(|c| *c > 0.0);
    if let Some(c) = clearcoat {
        out.insert("clearcoat".into(), json!(c));
        out.insert(
            "clearcoat_roughness".into(),
            json!(ext(CLEARCOAT, "clearcoatRoughnessFactor").unwrap_or(0.0)),
        );
    }
    let family = if emission.is_some() {
        Some("emissive")
    } else if transmission.is_some() {
        Some("glass")
    } else if clearcoat.is_some() {
        Some("lacquer")
    } else if metalness >= 0.5 {
        Some("metal")
    } else {
        None
    };
    if let Some(family) = family {
        out.insert("family".into(), json!(family));
    }
    Value::Object(out)
}

fn has_index(dir: &Path) -> bool {
    dir.join(INDEX).is_file()
}

pub fn find_root(from: &Path) -> Option<PathBuf> {
    let from = std::path::absolute(from).ok()?;
    from.ancestors().find_map(|dir| {
        if dir.file_name().is_some_and(|n| n == FOLDER) && has_index(dir) {
            return Some(dir.to_path_buf());
        }
        let below = dir.join(FOLDER);
        has_index(&below).then_some(below)
    })
}

pub fn root_for(recipe: &Recipe, base: &Path) -> Option<PathBuf> {
    match &recipe.library {
        Some(path) => Some(base.join(path)),
        None => find_root(base),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Property {
    Translation,
    Rotation,
    Scale,
    Weights,
}

impl Property {
    fn name(self) -> &'static str {
        match self {
            Property::Translation => "translation",
            Property::Rotation => "rotation",
            Property::Scale => "scale",
            Property::Weights => "weights",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Channel {
    pub node: usize,
    pub property: Property,
    pub interpolation: String,
    pub times: Vec<f32>,
    pub values: Vec<Vec<f32>>,
}

#[derive(Clone, Debug)]
pub(crate) struct Clip {
    pub name: Option<String>,
    pub channels: Vec<Channel>,
}

#[derive(Clone, Debug)]
pub(crate) struct TreeNode {
    pub name: String,
    pub parent: Option<usize>,
    pub rest: DMat4,
    pub world: DMat4,
    pub piece: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Tree {
    pub nodes: Vec<TreeNode>,
    pub clips: Vec<Clip>,
}

pub(crate) struct Imported {
    pub generated: Generated,
    pub looks: Looks,
    pub morphs: Vec<Option<Morph>>,
    pub tree: Tree,
    pub clips: Vec<(String, Vec<DMat4>)>,
}

fn clips_of(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    at: &BTreeMap<usize, usize>,
) -> Vec<Clip> {
    document
        .animations()
        .map(|animation| Clip {
            name: animation.name().map(str::to_string),
            channels: animation
                .channels()
                .filter_map(|channel| {
                    let node = *at.get(&channel.target().node().index())?;
                    let reader = channel.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
                    let times: Vec<f32> = reader.read_inputs()?.collect();
                    let (property, values): (Property, Vec<Vec<f32>>) =
                        match reader.read_outputs()? {
                            gltf::animation::util::ReadOutputs::Translations(t) => {
                                (Property::Translation, t.map(|v| v.to_vec()).collect())
                            }
                            gltf::animation::util::ReadOutputs::Rotations(r) => (
                                Property::Rotation,
                                r.into_f32().map(|v| v.to_vec()).collect(),
                            ),
                            gltf::animation::util::ReadOutputs::Scales(s) => {
                                (Property::Scale, s.map(|v| v.to_vec()).collect())
                            }
                            gltf::animation::util::ReadOutputs::MorphTargetWeights(w) => {
                                let flat: Vec<f32> = w.into_f32().collect();
                                let keys = times.len().max(1);
                                let width = (flat.len() / keys).max(1);
                                (
                                    Property::Weights,
                                    flat.chunks(width).map(<[f32]>::to_vec).collect(),
                                )
                            }
                        };
                    let interpolation = match channel.sampler().interpolation() {
                        gltf::animation::Interpolation::Step => "STEP",
                        gltf::animation::Interpolation::CubicSpline => "CUBICSPLINE",
                        gltf::animation::Interpolation::Linear => "LINEAR",
                    };
                    Some(Channel {
                        node,
                        property,
                        interpolation: interpolation.to_string(),
                        times,
                        values,
                    })
                })
                .collect(),
        })
        .collect()
}

fn keyed(channel: &Channel, t: f64) -> Option<Vec<f64>> {
    let cubic = channel.interpolation == "CUBICSPLINE";
    let value = |i: usize| -> Option<Vec<f64>> {
        let at = if cubic { i * 3 + 1 } else { i };
        Some(
            channel
                .values
                .get(at)?
                .iter()
                .map(|&v| f64::from(v))
                .collect(),
        )
    };
    let times = &channel.times;
    let first = f64::from(*times.first()?);
    if t <= first || times.len() == 1 {
        return value(0);
    }
    let last = times.len() - 1;
    if t >= f64::from(times[last]) {
        return value(last);
    }
    let next = times.iter().position(|&k| f64::from(k) > t)?;
    let (a, b) = (value(next - 1)?, value(next)?);
    if channel.interpolation == "STEP" {
        return Some(a);
    }
    let (ta, tb) = (f64::from(times[next - 1]), f64::from(times[next]));
    let u = if tb > ta { (t - ta) / (tb - ta) } else { 0.0 };
    if channel.property == Property::Rotation && a.len() == 4 && b.len() == 4 {
        let qa = DQuat::from_xyzw(a[0], a[1], a[2], a[3]).normalize();
        let qb = DQuat::from_xyzw(b[0], b[1], b[2], b[3]).normalize();
        let q = qa.slerp(qb, u).normalize();
        return Some(q.to_array().to_vec());
    }
    Some(a.iter().zip(&b).map(|(x, y)| x + (y - x) * u).collect())
}

fn duration(clip: &Clip) -> f64 {
    clip.channels
        .iter()
        .filter_map(|c| c.times.last())
        .fold(0.0, |m, &t| m.max(f64::from(t)))
}

fn posed(tree: &Tree, clip: &Clip, t: f64) -> Vec<DMat4> {
    let mut out: Vec<DMat4> = Vec::with_capacity(tree.nodes.len());
    for (at, node) in tree.nodes.iter().enumerate() {
        let (mut scale, mut rotation, mut translation) = node.rest.to_scale_rotation_translation();
        for channel in clip.channels.iter().filter(|c| c.node == at) {
            let Some(v) = keyed(channel, t) else {
                continue;
            };
            match (channel.property, v.as_slice()) {
                (Property::Translation, [x, y, z]) => translation = DVec3::new(*x, *y, *z),
                (Property::Scale, [x, y, z]) => scale = DVec3::new(*x, *y, *z),
                (Property::Rotation, [x, y, z, w]) => {
                    rotation = DQuat::from_xyzw(*x, *y, *z, *w).normalize();
                }
                _ => {}
            }
        }
        let local = DMat4::from_scale_rotation_translation(scale, rotation, translation);
        let parent = node.parent.and_then(|p| out.get(p).copied());
        out.push(parent.map_or(local, |p| p * local));
    }
    out
}

fn sampled(
    recipe: &Recipe,
    object: &Object,
    tree: &Tree,
) -> Result<Vec<(String, Vec<DMat4>)>, Error> {
    let (Some(timeline), Kind::Library { clip, looped, .. }) = (&recipe.timeline, &object.kind)
    else {
        return Ok(Vec::new());
    };
    let chosen = match clip {
        Some(want) => Some(
            tree.clips
                .iter()
                .find(|c| c.name.as_deref() == Some(want.as_str()))
                .ok_or_else(|| {
                    let names: Vec<&str> = tree
                        .clips
                        .iter()
                        .filter_map(|c| c.name.as_deref())
                        .collect();
                    fail(
                        object,
                        "clip",
                        format!(
                            "the model has no animation named `{want}`, expected one of: {}",
                            if names.is_empty() {
                                "none".to_string()
                            } else {
                                names.join(", ")
                            }
                        ),
                    )
                })?,
        ),
        None => tree.clips.first(),
    };
    let Some(chosen) = chosen else {
        return Ok(Vec::new());
    };
    let length = duration(chosen);
    let fps = f64::from(timeline.fps.max(1));
    let frames: Vec<Vec<DMat4>> = (0..=timeline.frames)
        .map(|n| {
            let t = f64::from(n) / fps;
            let t = if *looped && length > 0.0 {
                t.rem_euclid(length)
            } else {
                t
            };
            posed(tree, chosen, t)
        })
        .collect();
    let mut out = Vec::new();
    for (at, node) in tree.nodes.iter().enumerate() {
        let Some(piece) = &node.piece else {
            continue;
        };
        let rest = node.world.inverse();
        let deltas: Vec<DMat4> = frames.iter().map(|f| f[at] * rest).collect();
        let moves = deltas.iter().any(|d| !d.abs_diff_eq(DMat4::IDENTITY, 1e-9));
        if moves {
            out.push((piece.clone(), deltas));
        }
    }
    Ok(out)
}

pub(crate) fn import(recipe: &Recipe, object: &Object, base: &Path) -> Result<Imported, Error> {
    let Kind::Library {
        object: reference, ..
    } = &object.kind
    else {
        return Err(fail(object, "kind", "expected a library object"));
    };
    let (kind, name) = split_name(reference).map_err(|t| fail(object, "object", t))?;
    let root = root_for(recipe, base).ok_or_else(|| {
        fail(
            object,
            "object",
            "no library: set the recipe's top-level `library = \"<path>\"`, or keep the recipe under a folder whose `library/` holds an index.json",
        )
    })?;
    let dir = root.join(kind).join(name);
    let sidecar = Sidecar::read(&dir.join(SIDECAR))
        .map_err(|t| fail(object, "object", format!("{reference}: {t}")))?;
    let model = dir.join(MODEL);
    let bytes = std::fs::read(&model).map_err(|e| {
        fail(
            object,
            "object",
            format!("{reference}: cannot read {}: {e}", model.display()),
        )
    })?;
    let parsed = gltf::Gltf::from_slice(&bytes)
        .map_err(|e| fail(object, "object", format!("{reference}: not glTF: {e}")))?;
    let buffers = gltf::import_buffers(&parsed.document, Some(&dir), parsed.blob.clone())
        .map_err(|e| fail(object, "object", format!("{reference}: buffers: {e}")))?;
    let label = format!("{reference}/{MODEL}");
    let source = Source {
        object,
        path: &label,
        folder: &dir,
        buffers: &buffers,
    };
    let document = &parsed.document;
    let roots: Vec<gltf::Node> = match document
        .default_scene()
        .or_else(|| document.scenes().next())
    {
        Some(scene) => scene.nodes().collect(),
        None => {
            let mut child = vec![false; document.nodes().len()];
            for node in document.nodes() {
                for c in node.children() {
                    child[c.index()] = true;
                }
            }
            document.nodes().filter(|n| !child[n.index()]).collect()
        }
    };
    let mut looks = Looks::new();
    let mut pieces = Vec::new();
    let mut morphs = Vec::new();
    let mut tree = Tree::default();
    let mut at = BTreeMap::new();
    let mut facts = sidecar.facts.clone();
    let mut visited = vec![false; document.nodes().len()];
    let single = roots.len() == 1;
    let mut stack: Vec<(gltf::Node, Option<usize>)> =
        roots.into_iter().rev().map(|n| (n, None)).collect();
    let mut taken = BTreeSet::new();
    while let Some((n, parent)) = stack.pop() {
        if visited
            .get_mut(n.index())
            .is_none_or(|done| std::mem::replace(done, true))
        {
            continue;
        }
        let rest = DMat4::from_cols_array_2d(&n.transform().matrix().map(|c| c.map(f64::from)));
        let world = parent.map_or(rest, |p: usize| tree.nodes[p].world * rest);
        let first = parent.is_none() && tree.nodes.is_empty();
        let mut node = if first && (single || n.mesh().is_some()) {
            ROOT.to_string()
        } else {
            n.name()
                .map_or_else(|| format!("node{}", n.index()), str::to_string)
        };
        if !taken.insert(node.clone()) {
            node = format!("{node}.{}", n.index());
            taken.insert(node.clone());
        }
        let mut piece = None;
        if let Some(m) = n.mesh() {
            let mut mesh = Mesh::default();
            let mut morph = Morph::default();
            read_mesh(&source, &m, world, &mut mesh, &mut morph, &mut looks)?;
            if let Some(corner) = node.strip_prefix("wheel_") {
                wheel_fact(&mut facts, corner, world, &mesh);
            }
            let full = format!("{}.{node}", object.name);
            piece = Some(full.clone());
            pieces.push(Piece { name: full, mesh });
            morphs.push((!morph.is_empty()).then_some(morph));
        }
        at.insert(n.index(), tree.nodes.len());
        tree.nodes.push(TreeNode {
            name: node,
            parent,
            rest,
            world,
            piece,
        });
        let me = tree.nodes.len() - 1;
        for child in n.children().collect::<Vec<_>>().into_iter().rev() {
            stack.push((child, Some(me)));
        }
    }
    tree.clips = clips_of(document, &buffers, &at);
    let clips = sampled(recipe, object, &tree)?;
    looks.retain(|_, look| look.has_maps());
    let materials = split_glb(&bytes)
        .ok()
        .and_then(|(json, _)| json.get("materials").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|m| Some((m.get("name")?.as_str()?.to_string(), material_of(m))))
        .collect();
    let lights = facts
        .get("lights")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|l| light(&object.name, l)).collect())
        .unwrap_or_default();
    Ok(Imported {
        generated: Generated {
            pieces,
            facts,
            materials,
            lights,
        },
        looks,
        morphs,
        tree,
        clips,
    })
}

fn wheel_fact(facts: &mut Map<String, Value>, corner: &str, matrix: DMat4, mesh: &Mesh) {
    let wheels = facts
        .entry("wheels")
        .or_insert_with(|| json!({}))
        .as_object_mut();
    let Some(wheels) = wheels else {
        return;
    };
    let entry = wheels.entry(corner).or_insert_with(|| json!({}));
    let Some(entry) = entry.as_object_mut() else {
        return;
    };
    entry
        .entry("centre")
        .or_insert_with(|| json!(matrix.w_axis.truncate().to_array()));
    if let Some([lo, hi]) = manfred_mesh::bounds(mesh) {
        entry
            .entry("radius")
            .or_insert_with(|| json!((hi[1] - lo[1]) * 0.5));
    }
    entry
        .entry("axle")
        .or_insert_with(|| json!([1.0, 0.0, 0.0]));
}

fn light(object: &str, value: &Value) -> Option<GeneratedLight> {
    let number =
        |key: &str, default: f64| value.get(key).and_then(Value::as_f64).unwrap_or(default);
    let pair = value.get("cone").and_then(Value::as_array);
    let cone = match pair.map(Vec::as_slice) {
        Some([a, b]) => [a.as_f64()?, b.as_f64()?],
        _ => [20.0, 40.0],
    };
    let node = value.get("node").and_then(Value::as_str).unwrap_or(ROOT);
    Some(GeneratedLight {
        name: format!("{object}.{}", value.get("name")?.as_str()?),
        position: vec3(value.get("position"))?.to_array(),
        direction: vec3(value.get("direction"))
            .unwrap_or(DVec3::NEG_Z)
            .to_array(),
        cone,
        range: number("range", 10.0),
        color: vec3(value.get("color")).unwrap_or(DVec3::ONE).to_array(),
        intensity: number("intensity", 1.0),
        shadow: value
            .get("shadow")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        parent: Some(format!("{object}.{node}")),
    })
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn folders(dir: &Path) -> Result<Vec<String>, Error> {
    let fail = |e: std::io::Error| Error::new(dir.display().to_string(), e.to_string());
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(fail)? {
        let entry = entry.map_err(fail)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.file_type().map_err(fail)?.is_dir() && !name.starts_with('.') {
            out.push(name);
        }
    }
    out.sort();
    Ok(out)
}

pub fn index(root: &Path) -> Result<Vec<Entry>, Error> {
    let mut out = Vec::new();
    for kind in folders(root)? {
        for name in folders(&root.join(&kind))? {
            out.push(entry(root, &kind, &name)?);
        }
    }
    Ok(out)
}

fn entry(root: &Path, kind: &str, name: &str) -> Result<Entry, Error> {
    let dir = root.join(kind).join(name);
    let at = |file: &str| format!("{kind}/{name}/{file}");
    let read = |file: &str| {
        std::fs::read(dir.join(file)).map_err(|e| Error::new(at(file), format!("cannot read: {e}")))
    };
    let sidecar_bytes = read(SIDECAR)?;
    let text = String::from_utf8(sidecar_bytes.clone())
        .map_err(|_| Error::new(at(SIDECAR), "not UTF-8"))?;
    let sidecar = Sidecar::parse(&text).map_err(|t| Error::new(at(SIDECAR), t))?;
    let model = read(MODEL)?;
    let summary = manfred_glb::summary(&model).map_err(|e| Error::new(at(MODEL), e.0))?;
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    let mut triangles = 0;
    for node in &summary.nodes {
        triangles += node.triangles;
        lo = lo.min(DVec3::from_array(node.bounds[0]));
        hi = hi.max(DVec3::from_array(node.bounds[1]));
    }
    let size = if lo.x.is_finite() {
        (hi - lo).to_array().map(|v| (v * 1e4).round() / 1e4)
    } else {
        [0.0; 3]
    };
    let mut sha = BTreeMap::new();
    sha.insert("model".to_string(), sha256(&model));
    sha.insert("sidecar".to_string(), sha256(&sidecar_bytes));
    let thumb = match std::fs::read(dir.join(THUMB)) {
        Ok(bytes) => {
            sha.insert("thumb".to_string(), sha256(&bytes));
            Some(at(THUMB))
        }
        Err(_) => None,
    };
    Ok(Entry {
        kind: kind.to_string(),
        name: name.to_string(),
        tags: sidecar.tags,
        size_m: size,
        triangles,
        model: at(MODEL),
        sidecar: at(SIDECAR),
        thumb,
        sha256: sha,
    })
}

impl Entry {
    pub fn json(&self) -> Value {
        let mut files = Map::new();
        files.insert("model".into(), json!(self.model));
        files.insert("sidecar".into(), json!(self.sidecar));
        if let Some(thumb) = &self.thumb {
            files.insert("thumb".into(), json!(thumb));
        }
        json!({
            "kind": self.kind,
            "name": self.name,
            "tags": self.tags,
            "size_m": self.size_m,
            "triangles": self.triangles,
            "files": files,
            "sha256": self.sha256,
        })
    }

    pub fn from_json(value: &Value) -> Option<Entry> {
        let text = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_string);
        let files = value.get("files")?;
        let size = value.get("size_m")?.as_array()?;
        Some(Entry {
            kind: text(value.get("kind"))?,
            name: text(value.get("name"))?,
            tags: value
                .get("tags")?
                .as_array()?
                .iter()
                .map(|t| t.as_str().map(str::to_string))
                .collect::<Option<_>>()?,
            size_m: match size.as_slice() {
                [x, y, z] => [x.as_f64()?, y.as_f64()?, z.as_f64()?],
                _ => return None,
            },
            triangles: value.get("triangles")?.as_u64()? as usize,
            model: text(files.get("model"))?,
            sidecar: text(files.get("sidecar"))?,
            thumb: text(files.get("thumb")),
            sha256: value
                .get("sha256")?
                .as_object()?
                .iter()
                .map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect::<Option<_>>()?,
        })
    }
}

pub fn index_text(entries: &[Entry]) -> String {
    let list: Vec<Value> = entries.iter().map(Entry::json).collect();
    let mut text = serde_json::to_string_pretty(&Value::Array(list)).unwrap_or_default();
    text.push('\n');
    text
}

pub fn read_index(root: &Path) -> Result<Vec<Entry>, Error> {
    let path = root.join(INDEX);
    let fail = |text: String| Error::new(path.display().to_string(), text);
    let text = std::fs::read_to_string(&path).map_err(|e| fail(format!("cannot read: {e}")))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| fail(e.to_string()))?;
    value
        .as_array()
        .ok_or_else(|| fail("expected an array of objects".into()))?
        .iter()
        .map(|v| Entry::from_json(v).ok_or_else(|| fail("an entry is not the index's form".into())))
        .collect()
}

pub fn write_index(root: &Path) -> Result<PathBuf, Error> {
    let entries = index(root)?;
    let path = root.join(INDEX);
    crate::put(&path, index_text(&entries).as_bytes())?;
    Ok(path)
}

fn extras_in(value: &Value, at: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                let here = format!("{at}.{key}");
                if key == "extras" {
                    let morph_names = at.starts_with("meshes[")
                        && inner
                            .as_object()
                            .is_some_and(|e| e.keys().all(|k| k == "targetNames"));
                    if !morph_names {
                        out.push(here.clone());
                    }
                }
                extras_in(inner, &here, out);
            }
        }
        Value::Array(list) => {
            for (i, inner) in list.iter().enumerate() {
                extras_in(inner, &format!("{at}[{i}]"), out);
            }
        }
        _ => {}
    }
}

pub fn manfred_extras(glb: &[u8]) -> Result<Vec<String>, String> {
    let (json, _) = split_glb(glb)?;
    let mut out = Vec::new();
    if let Some(map) = json.as_object() {
        for (key, inner) in map {
            if key == "extras" {
                out.push(key.clone());
            }
            extras_in(inner, key, &mut out);
        }
    }
    Ok(out)
}

pub fn check(root: &Path) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut problem = |path: String, text: String| out.push(Problem { path, text });
    let kinds = match folders(root) {
        Ok(kinds) => kinds,
        Err(e) => {
            problem(e.path, e.text);
            return out;
        }
    };
    let mut fresh = Vec::new();
    for kind in kinds {
        let names = match folders(&root.join(&kind)) {
            Ok(names) => names,
            Err(e) => {
                problem(e.path, e.text);
                continue;
            }
        };
        for name in names {
            let dir = root.join(&kind).join(&name);
            let at = |file: &str| format!("{kind}/{name}/{file}");
            if !word(&kind) || !word(&name) {
                problem(
                    format!("{kind}/{name}"),
                    "folder names are lowercase letters, digits, `_` and `-`".into(),
                );
            }
            match Sidecar::read(&dir.join(SIDECAR)) {
                Ok(sidecar) => {
                    if sidecar.kind != kind || sidecar.name != name {
                        problem(
                            at(SIDECAR),
                            format!(
                                "names {}/{}, expected {kind}/{name} after its folder",
                                sidecar.kind, sidecar.name
                            ),
                        );
                    }
                    if sidecar.licence != LICENCE {
                        problem(
                            at(SIDECAR),
                            format!("object.licence: expected \"{LICENCE}\""),
                        );
                    }
                }
                Err(text) => problem(at(SIDECAR), text),
            }
            match std::fs::read(dir.join(MODEL)) {
                Ok(bytes) => {
                    if let Err(e) = gltf::Gltf::from_slice(&bytes) {
                        problem(at(MODEL), format!("not glTF: {e}"));
                    } else {
                        model_problems(&bytes, &name, &mut |text| problem(at(MODEL), text));
                    }
                }
                Err(e) => problem(at(MODEL), format!("cannot read: {e}")),
            }
            if let Ok(entry) = entry(root, &kind, &name) {
                fresh.push(entry);
            }
        }
    }
    match read_index(root) {
        Ok(listed) => {
            if listed != fresh {
                for entry in &fresh {
                    match listed
                        .iter()
                        .find(|l| l.kind == entry.kind && l.name == entry.name)
                    {
                        None => problem(
                            INDEX.into(),
                            format!("{}/{} is not listed", entry.kind, entry.name),
                        ),
                        Some(l) if l.sha256 != entry.sha256 => problem(
                            INDEX.into(),
                            format!("{}/{}: the sha256s are stale", entry.kind, entry.name),
                        ),
                        Some(l) if l != entry => problem(
                            INDEX.into(),
                            format!("{}/{}: the entry is stale", entry.kind, entry.name),
                        ),
                        Some(_) => {}
                    }
                }
                for l in &listed {
                    if !fresh.iter().any(|e| e.kind == l.kind && e.name == l.name) {
                        problem(INDEX.into(), format!("{}/{} has no folder", l.kind, l.name));
                    }
                }
                let sorted = listed
                    .windows(2)
                    .all(|w| (&w[0].kind, &w[0].name) < (&w[1].kind, &w[1].name));
                if !sorted {
                    problem(INDEX.into(), "not sorted by kind and name".into());
                }
                problem(
                    INDEX.into(),
                    "differs from the folders; `manfred library index` rewrites it".into(),
                );
            }
        }
        Err(e) => problem(INDEX.into(), e.text),
    }
    out
}

fn model_problems(bytes: &[u8], name: &str, problem: &mut dyn FnMut(String)) {
    match manfred_extras(bytes) {
        Ok(found) => {
            for at in found {
                problem(format!(
                    "`{at}`: Manfred's own data stays in object.toml, the GLB carries standard glTF only"
                ));
            }
        }
        Err(text) => problem(text),
    }
    if let Ok((json, _)) = split_glb(bytes) {
        let nodes = json.get("nodes").and_then(Value::as_array);
        let scene = json
            .get("scenes")
            .and_then(|s| s.get(json.get("scene").and_then(Value::as_u64).unwrap_or(0) as usize))
            .and_then(|s| s.get("nodes"))
            .and_then(Value::as_array);
        let root = scene
            .and_then(|s| s.first())
            .and_then(Value::as_u64)
            .and_then(|at| nodes?.get(at as usize))
            .and_then(|n| n.get("name"))
            .and_then(Value::as_str);
        if root != Some(name) {
            problem(format!(
                "the scene's first root node is {}, expected `{name}`",
                root.map_or_else(|| "unnamed".to_string(), |r| format!("`{r}`"))
            ));
        }
    }
}
