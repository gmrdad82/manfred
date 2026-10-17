use crate::{Registry, build_with, library};
use glam::{DMat4, DVec3};
use manf::{Codec, Reader, Writer};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;

pub const GLTF_PATH: &str = "@gltf";
pub const LICENCE: &str = library::LICENCE;

const STANDARD: &[&str] = &["KHR_", "EXT_", "MSFT_"];

#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    pub kind: String,
    pub name: String,
    pub recipe: String,
    pub facts: Map<String, Value>,
    pub nodes: BTreeMap<String, usize>,
    pub json: Vec<u8>,
    pub bin: Vec<u8>,
}

pub struct Glb<'a> {
    pub json: &'a [u8],
    pub bin: &'a [u8],
}

fn word(bytes: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize)
}

pub fn split(bytes: &[u8]) -> Result<Glb<'_>, String> {
    if bytes.get(0..4) != Some(b"glTF") || word(bytes, 4) != Some(2) {
        return Err("not a glTF 2.0 binary (.glb)".into());
    }
    let length = word(bytes, 12).ok_or("no JSON chunk")?;
    if bytes.get(16..20) != Some(b"JSON") {
        return Err("the first chunk is not JSON".into());
    }
    let json = bytes.get(20..20 + length).ok_or("a short JSON chunk")?;
    let padded = json
        .iter()
        .rposition(|&b| b != b' ' && b != 0)
        .map_or(0, |end| end + 1);
    let json = &json[..padded];
    let at = 20 + length;
    let bin = match word(bytes, at) {
        Some(size) if bytes.get(at + 4..at + 8) == Some(b"BIN\0") => bytes
            .get(at + 8..at + 8 + size)
            .ok_or("a short BIN chunk")?,
        _ => &[],
    };
    Ok(Glb { json, bin })
}

pub fn join(json: &[u8], bin: &[u8]) -> Vec<u8> {
    let mut text = json.to_vec();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let mut data = bin.to_vec();
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
    let total = 20 + text.len() + if data.is_empty() { 0 } else { 8 + data.len() };
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

fn standard(name: &str) -> bool {
    STANDARD.iter().any(|p| name.starts_with(p))
}

fn strip(value: &mut Value, mesh: bool) -> bool {
    let mut changed = false;
    match value {
        Value::Object(map) => {
            if let Some(extras) = map.get_mut("extras") {
                let names = extras.get("targetNames").cloned().filter(|_| mesh);
                match names {
                    Some(names) => {
                        if extras.as_object().is_some_and(|e| e.len() > 1) {
                            *extras = json!({ "targetNames": names });
                            changed = true;
                        }
                    }
                    None => {
                        map.remove("extras");
                        changed = true;
                    }
                }
            }
            if let Some(Value::Object(extensions)) = map.get_mut("extensions") {
                let before = extensions.len();
                extensions.retain(|k, _| standard(k));
                changed |= extensions.len() != before;
                if extensions.is_empty() {
                    map.remove("extensions");
                }
            }
            for key in ["extensionsUsed", "extensionsRequired"] {
                if let Some(Value::Array(list)) = map.get_mut(key) {
                    let before = list.len();
                    list.retain(|v| v.as_str().is_some_and(standard));
                    changed |= list.len() != before;
                    if list.is_empty() {
                        map.remove(key);
                    }
                }
            }
            for (key, inner) in map.iter_mut() {
                if key == "meshes"
                    && let Value::Array(list) = inner
                {
                    for m in list {
                        changed |= strip(m, true);
                    }
                    continue;
                }
                if key != "extras" {
                    changed |= strip(inner, false);
                }
            }
        }
        Value::Array(list) => {
            for inner in list {
                changed |= strip(inner, false);
            }
        }
        _ => {}
    }
    changed
}

fn normal_accessors(json: &Value) -> Vec<usize> {
    let mut out: Vec<usize> = json
        .get("meshes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| m.get("primitives")?.as_array())
        .flatten()
        .filter_map(|p| p.pointer("/attributes/NORMAL")?.as_u64())
        .map(|a| a as usize)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn f32_at(bytes: &[u8], k: usize) -> f32 {
    f32::from_le_bytes([bytes[k], bytes[k + 1], bytes[k + 2], bytes[k + 3]])
}

type Fixed = Vec<(usize, [f32; 3], [f32; 3])>;

fn unit_normals(json: &Value, bin: &mut [u8]) -> Option<Fixed> {
    let mut changed = false;
    let mut bounds = Vec::new();
    for at in normal_accessors(json) {
        let Some(accessor) = json.pointer(&format!("/accessors/{at}")) else {
            continue;
        };
        let float = accessor.get("componentType").and_then(Value::as_u64) == Some(5126);
        let vec3 = accessor.get("type").and_then(Value::as_str) == Some("VEC3");
        if !float || !vec3 || accessor.get("sparse").is_some() {
            continue;
        }
        let Some(view) = accessor
            .get("bufferView")
            .and_then(Value::as_u64)
            .and_then(|v| json.pointer(&format!("/bufferViews/{v}")))
        else {
            continue;
        };
        if view.get("buffer").and_then(Value::as_u64) != Some(0) {
            continue;
        }
        let number =
            |v: &Value, key: &str| v.get(key).and_then(Value::as_u64).unwrap_or(0) as usize;
        let start = number(view, "byteOffset") + number(accessor, "byteOffset");
        let stride = number(view, "byteStride").max(12);
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        let mut fixed = false;
        for i in 0..number(accessor, "count") {
            let at = start + i * stride;
            let Some(bytes) = bin.get_mut(at..at + 12) else {
                break;
            };
            let v = DVec3::new(
                f64::from(f32_at(bytes, 0)),
                f64::from(f32_at(bytes, 4)),
                f64::from(f32_at(bytes, 8)),
            );
            let length = v.length();
            if (length - 1.0).abs() > 1e-4 {
                let unit = if length.is_finite() && length > 1e-12 {
                    v / length
                } else {
                    DVec3::Y
                };
                for (k, c) in unit.to_array().iter().enumerate() {
                    bytes[k * 4..k * 4 + 4].copy_from_slice(&(*c as f32).to_le_bytes());
                }
                fixed = true;
            }
            for k in 0..3 {
                let c = f32_at(bytes, k * 4);
                lo[k] = lo[k].min(c);
                hi[k] = hi[k].max(c);
            }
        }
        if fixed {
            changed = true;
            if accessor.get("min").is_some() || accessor.get("max").is_some() {
                bounds.push((at, lo, hi));
            }
        }
    }
    changed.then_some(bounds)
}

pub fn clean(glb: &[u8]) -> Result<Vec<u8>, String> {
    let parts = split(glb)?;
    let mut json: Value = serde_json::from_slice(parts.json)
        .map_err(|e| format!("the JSON chunk does not parse: {e}"))?;
    let mut bin = parts.bin.to_vec();
    let fixed = unit_normals(&json, &mut bin);
    let mut changed = strip(&mut json, false);
    for (at, lo, hi) in fixed.iter().flatten() {
        if let Some(accessor) = json.pointer_mut(&format!("/accessors/{at}")) {
            accessor["min"] = json!(lo);
            accessor["max"] = json!(hi);
            changed = true;
        }
    }
    if !changed {
        return Ok(join(
            parts.json,
            if fixed.is_some() { &bin } else { parts.bin },
        ));
    }
    let text = serde_json::to_vec(&json).map_err(|e| e.to_string())?;
    Ok(join(&text, &bin))
}

pub fn extras(glb: &[u8]) -> Result<Vec<String>, String> {
    fn walk(value: &Value, at: &str, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, inner) in map {
                    let here = format!("{at}.{key}");
                    let names_only = key == "extras"
                        && at.starts_with("meshes[")
                        && at.matches('.').count() == 0
                        && inner
                            .as_object()
                            .is_some_and(|e| e.keys().all(|k| k == "targetNames"));
                    if key == "extras" && !names_only {
                        out.push(here.clone());
                    }
                    if key == "extensions"
                        && let Some(e) = inner.as_object()
                    {
                        out.extend(
                            e.keys()
                                .filter(|k| !standard(k))
                                .map(|k| format!("{here}.{k}")),
                        );
                    }
                    walk(inner, &here, out);
                }
            }
            Value::Array(list) => {
                for (i, inner) in list.iter().enumerate() {
                    walk(inner, &format!("{at}[{i}]"), out);
                }
            }
            _ => {}
        }
    }
    let parts = split(glb)?;
    let json: Value = serde_json::from_slice(parts.json).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    if let Some(map) = json.as_object() {
        for (key, inner) in map {
            if key == "extras" {
                out.push(key.clone());
            }
            walk(inner, key, &mut out);
        }
    }
    Ok(out)
}

fn standalone(json: &Value) -> Result<(), String> {
    let outside = |list: &str| {
        json.get(list)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
            .find_map(|(i, v)| {
                let uri = v.get("uri")?.as_str()?;
                (!uri.starts_with("data:"))
                    .then(|| format!("{list}[{i}] names an outside file `{uri}`"))
            })
    };
    match outside("buffers").or_else(|| outside("images")) {
        Some(text) => Err(format!("not a standalone GLB: {text}")),
        None => Ok(()),
    }
}

fn rounded(v: f64) -> f64 {
    let r = (v * 1e6).round() / 1e6;
    if r == 0.0 { 0.0 } else { r }
}

fn round3(v: DVec3) -> [f64; 3] {
    v.to_array().map(rounded)
}

#[derive(Clone, Copy)]
struct Bounds {
    lo: DVec3,
    hi: DVec3,
}

impl Bounds {
    const EMPTY: Bounds = Bounds {
        lo: DVec3::splat(f64::INFINITY),
        hi: DVec3::splat(f64::NEG_INFINITY),
    };

    fn add(&mut self, p: DVec3) {
        self.lo = self.lo.min(p);
        self.hi = self.hi.max(p);
    }

    fn join(&mut self, other: Bounds) {
        self.lo = self.lo.min(other.lo);
        self.hi = self.hi.max(other.hi);
    }

    fn some(&self) -> bool {
        self.lo.x.is_finite()
    }
}

struct Walked {
    name: String,
    parent: Option<usize>,
    world: DMat4,
    own: Bounds,
    tree: Bounds,
}

fn corner(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("wheel")
        .or_else(|| lower.rsplit('.').next()?.strip_prefix("wheel"))?;
    let rest: String = rest
        .trim_matches(|c: char| c == '_' || c == '.' || c == '-' || c == ' ')
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    Some(rest)
}

fn anchor_kind(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if corner(name).is_some() {
        return None;
    }
    if lower.contains("head") {
        Some("head")
    } else if lower.contains("tail") {
        Some("tail")
    } else if lower.contains("light") || lower.contains("lamp") {
        Some("lamp")
    } else {
        None
    }
}

fn walk(bytes: &[u8]) -> Result<(Vec<Walked>, usize), String> {
    let parsed = gltf::Gltf::from_slice(bytes).map_err(|e| format!("not valid glTF: {e}"))?;
    let buffers = gltf::import_buffers(&parsed.document, None, parsed.blob.clone())
        .map_err(|e| format!("the buffers do not load: {e}"))?;
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
    let mut out: Vec<Walked> = Vec::new();
    let mut seen = vec![false; document.nodes().len()];
    let mut triangles = 0;
    let mut stack: Vec<(gltf::Node, Option<usize>)> =
        roots.into_iter().rev().map(|n| (n, None)).collect();
    while let Some((node, parent)) = stack.pop() {
        if seen
            .get_mut(node.index())
            .is_none_or(|s| std::mem::replace(s, true))
        {
            continue;
        }
        let local = DMat4::from_cols_array_2d(&node.transform().matrix().map(|c| c.map(f64::from)));
        let world = parent.map_or(local, |p| out[p].world * local);
        let mut own = Bounds::EMPTY;
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
                if let Some(points) = reader.read_positions() {
                    let mut count = 0;
                    for p in points {
                        own.add(world.transform_point3(DVec3::from_array(p.map(f64::from))));
                        count += 1;
                    }
                    if primitive.mode() == gltf::mesh::Mode::Triangles {
                        triangles += reader
                            .read_indices()
                            .map_or(count, |i| i.into_u32().count())
                            / 3;
                    }
                }
            }
        }
        out.push(Walked {
            name: node
                .name()
                .map_or_else(|| format!("node{}", node.index()), str::to_string),
            parent,
            world,
            own,
            tree: own,
        });
        let me = out.len() - 1;
        for child in node.children().collect::<Vec<_>>().into_iter().rev() {
            stack.push((child, Some(me)));
        }
    }
    for at in (0..out.len()).rev() {
        if let Some(p) = out[at].parent {
            let tree = out[at].tree;
            out[p].tree.join(tree);
        }
    }
    Ok((out, triangles))
}

pub fn facts(glb: &[u8]) -> Result<Map<String, Value>, String> {
    let (nodes, triangles) = walk(glb)?;
    let mut all = Bounds::EMPTY;
    for n in &nodes {
        all.join(n.own);
    }
    let mut facts = Map::new();
    if all.some() {
        facts.insert("size".into(), json!(round3(all.hi - all.lo)));
        facts.insert("min".into(), json!(round3(all.lo)));
        facts.insert("max".into(), json!(round3(all.hi)));
    }
    facts.insert("nodes".into(), json!(nodes.len()));
    facts.insert("triangles".into(), json!(triangles));
    let mut wheels = Map::new();
    let mut anchors = Vec::new();
    for (at, n) in nodes.iter().enumerate() {
        let parent_matches =
            |test: &dyn Fn(&str) -> bool| n.parent.is_some_and(|p| test(&nodes[p].name));
        let origin = n.world.w_axis.truncate();
        let centre = if n.tree.some() {
            (n.tree.lo + n.tree.hi) * 0.5
        } else {
            origin
        };
        if let Some(c) = corner(&n.name)
            && !parent_matches(&|name| corner(name).is_some())
        {
            let key = if c.is_empty() || wheels.contains_key(&c) {
                format!("wheel{at}")
            } else {
                c
            };
            let extent = if n.tree.some() {
                n.tree.hi - n.tree.lo
            } else {
                DVec3::ZERO
            };
            let axis = if extent.x <= extent.y && extent.x <= extent.z {
                0
            } else if extent.y <= extent.z {
                1
            } else {
                2
            };
            let mut axle = [0.0; 3];
            axle[axis] = 1.0;
            let other = (0..3)
                .filter(|&i| i != axis)
                .map(|i| extent[i])
                .fold(0.0, f64::max);
            wheels.insert(
                key,
                json!({
                    "node": n.name,
                    "centre": round3(centre),
                    "radius": rounded(other * 0.5),
                    "axle": axle,
                }),
            );
        }
        if let Some(kind) = anchor_kind(&n.name) {
            let direction = n
                .world
                .transform_vector3(DVec3::NEG_Z)
                .normalize_or(DVec3::NEG_Z);
            anchors.push(json!({
                "node": n.name,
                "kind": kind,
                "position": round3(centre),
                "direction": round3(direction),
            }));
        }
    }
    if !wheels.is_empty() {
        facts.insert("wheels".into(), Value::Object(wheels));
    }
    if !anchors.is_empty() {
        facts.insert("anchors".into(), Value::Array(anchors));
    }
    Ok(facts)
}

fn slug(text: &str) -> String {
    let out: String = text
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let out = out.trim_matches('_').to_string();
    if out.is_empty() { "model".into() } else { out }
}

fn toml_string(text: &str) -> String {
    toml::Value::String(text.to_string()).to_string()
}

pub fn recipe_text(name: &str) -> String {
    let name = toml_string(name);
    format!(
        "manfred = 1\nname = {name}\n\n[[object]]\nname = {name}\nkind = \"mesh\"\npath = \"{GLTF_PATH}\"\n"
    )
}

pub fn import(glb: &[u8], name: &str, tags: &[String]) -> Result<Imported, String> {
    let parts = split(glb)?;
    let json: Value = serde_json::from_slice(parts.json)
        .map_err(|e| format!("the JSON chunk does not parse: {e}"))?;
    standalone(&json)?;
    let (kind, name) = match name.split_once('/') {
        Some((kind, name)) => (slug(kind), slug(name)),
        None => ("model".to_string(), slug(name)),
    };
    let detected = facts(glb)?;
    let (walked, _) = walk(glb)?;
    let mut facts = Map::new();
    facts.insert("kind".into(), json!(kind));
    facts.insert("name".into(), json!(name));
    facts.insert("tags".into(), json!(tags));
    facts.insert("licence".into(), json!(LICENCE));
    facts.extend(detected);
    let mut nodes = BTreeMap::new();
    if let Some(list) = json.get("nodes").and_then(Value::as_array) {
        let named: BTreeMap<&str, ()> = walked.iter().map(|w| (w.name.as_str(), ())).collect();
        for (i, node) in list.iter().enumerate() {
            let label = node
                .get("name")
                .and_then(Value::as_str)
                .filter(|n| named.contains_key(n))
                .map_or_else(|| format!("node{i}"), str::to_string);
            nodes.entry(format!("{name}.{label}")).or_insert(i);
        }
    }
    Ok(Imported {
        recipe: recipe_text(&name),
        kind,
        name,
        facts,
        nodes,
        json: parts.json.to_vec(),
        bin: parts.bin.to_vec(),
    })
}

fn toml_of(value: Value) -> Result<String, String> {
    toml::to_string(&value).map_err(|e| e.to_string())
}

impl Imported {
    pub fn facts_text(&self) -> Result<String, String> {
        toml_of(Value::Object(self.facts.clone()))
    }

    pub fn nodes_text(&self) -> Result<String, String> {
        toml_of(json!({ "nodes": self.nodes }))
    }

    pub fn manf(&self, thumb: Option<&[u8]>) -> Result<Vec<u8>, String> {
        self.writer(thumb)?.to_vec().map_err(|e| e.to_string())
    }

    fn writer<'a>(&'a self, thumb: Option<&'a [u8]>) -> Result<Writer<'a>, String> {
        let mut writer = Writer::new()
            .chunk(manf::GLTF, &self.json[..], Codec::Auto)
            .chunk(manf::GBIN, &self.bin[..], Codec::Auto)
            .chunk(manf::RCPE, self.recipe.as_bytes(), Codec::Auto)
            .chunk(manf::FACT, self.facts_text()?.into_bytes(), Codec::Auto)
            .chunk(manf::NODE, self.nodes_text()?.into_bytes(), Codec::Auto);
        if let Some(png) = thumb {
            writer = writer.chunk(manf::THMB, png, Codec::Store);
        }
        Ok(writer)
    }

    pub fn write(&self, out: &Path, thumb: Option<&[u8]>) -> Result<(), String> {
        if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        self.writer(thumb)?
            .write(out)
            .map_err(|e| format!("{}: {e}", out.display()))
    }
}

pub fn import_manf(
    glb: &[u8],
    name: &str,
    tags: &[String],
    thumb: Option<Vec<u8>>,
    out: &Path,
) -> Result<Imported, String> {
    let imported = import(glb, name, tags)?;
    imported.write(out, thumb.as_deref())?;
    Ok(imported)
}

pub fn import_glb(recipe_text: &str, glb: &Path, name: &str) -> Result<String, String> {
    let table: toml::Table = toml::from_str(recipe_text).map_err(|e| e.to_string())?;
    let taken = table
        .get("object")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .any(|o| o.get("name").and_then(toml::Value::as_str) == Some(name));
    if taken {
        return Err(format!("an object is already named `{name}`"));
    }
    let mut text = recipe_text.to_string();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!(
        "\n[[object]]\nname = {}\nkind = \"mesh\"\npath = {}\n",
        toml_string(name),
        toml_string(&glb.to_string_lossy())
    ));
    Ok(text)
}

pub fn manf_glb(reader: &Reader) -> Result<Vec<u8>, String> {
    let json = reader
        .chunk(manf::GLTF)
        .map_err(|e| e.to_string())?
        .ok_or("the .manf holds no GLTF chunk")?;
    let bin = reader
        .chunk(manf::GBIN)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    Ok(join(&json, &bin))
}

pub fn export_glb(
    source: &Path,
    registry: &Registry,
    only: Option<&str>,
) -> Result<Vec<u8>, String> {
    let file = source.display().to_string();
    if source.extension().is_some_and(|e| e == "manf") {
        if only.is_some() {
            return Err("--only takes a recipe, not a .manf".into());
        }
        let reader = Reader::open(source).map_err(|e| e.to_string())?;
        return clean(&manf_glb(&reader).map_err(|t| format!("{file}: {t}"))?);
    }
    let loaded = manfred_recipe::load(source).map_err(|e| format!("{file}: {}", e.text))?;
    let mut recipe = loaded.recipe;
    let problems = manfred_recipe::validate(&recipe);
    if let Some(p) = problems.first() {
        return Err(format!("{file}: {}: {}", p.path, p.text));
    }
    recipe.output.hierarchy = true;
    let base = source.parent().unwrap_or(Path::new(""));
    let built = build_with(&recipe, base, registry).map_err(|e| format!("{file}: {e}"))?;
    let one = only.or(match recipe.objects.as_slice() {
        [single] => Some(single.name.as_str()),
        _ => None,
    });
    let glb = match one {
        Some(object) => {
            library::export(&recipe, &built, object, &format!("export/{}", slug(object)))
                .map_err(|e| format!("{file}: {e}"))?
                .glb
        }
        None => built.try_glb().map_err(|e| format!("{file}: {e}"))?,
    };
    clean(&glb)
}

pub fn validator(path: &Path) -> Option<Result<(), String>> {
    let run = std::process::Command::new("gltf_validator")
        .arg("--stdout")
        .arg(path)
        .output();
    let output = match run {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => return Some(Err(format!("gltf_validator did not run: {e}"))),
    };
    let report: Option<Value> = serde_json::from_slice(&output.stdout).ok();
    let errors = report
        .as_ref()
        .and_then(|r| r.pointer("/issues/numErrors"))
        .and_then(Value::as_u64);
    Some(match errors {
        Some(0) => Ok(()),
        Some(n) => {
            let first = report
                .as_ref()
                .and_then(|r| r.pointer("/issues/messages/0"))
                .map(|m| {
                    format!(
                        ": {} at {}",
                        m.get("message").and_then(Value::as_str).unwrap_or(""),
                        m.get("pointer").and_then(Value::as_str).unwrap_or("")
                    )
                })
                .unwrap_or_default();
            Err(format!(
                "the Khronos glTF validator reports {n} errors{first}"
            ))
        }
        None if output.status.success() => Ok(()),
        None => Err(format!(
            "gltf_validator failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
    })
}
