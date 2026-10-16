use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use manf::{Codec, Reader, Tag, Writer};
use manfred_recipe::toml_edit::{self, DocumentMut, Item};
use manfred_recipe::{Key, Patch, Recipe, files};
use sha2::{Digest, Sha256};

use crate::{Built, Registry, build_with, glb_path};

pub const LICENCE: &str = "MIT, Catalin Ilinca; original, no third-party models or brand marks";
pub const GLTF_NAME: &str = "@gltf";
pub const ENTRY: &str = "main.toml";

const OWNED: [Tag; 10] = [
    manf::GLTF,
    manf::GBIN,
    manf::SCNE,
    manf::SMET,
    manf::RCPE,
    manf::FILE,
    manf::THMB,
    manf::FACT,
    manf::NODE,
    manf::OVRD,
];

const LIGHT_KEYS: [&str; 8] = [
    "intensity",
    "color",
    "position",
    "radius",
    "range",
    "direction",
    "cone",
    "shadow",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Override {
    pub file: String,
    pub path: String,
    pub value: toml::Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneChange {
    pub file: String,
    pub path: String,
    pub was: Option<toml::Value>,
    pub now: toml::Value,
    pub recipe: Option<String>,
}

#[derive(Debug, Default)]
pub struct Document {
    pub recipe_name: String,
    pub recipe_text: String,
    pub files: BTreeMap<PathBuf, Vec<u8>>,
    pub outputs: BTreeSet<PathBuf>,
    pub scene: Vec<(String, Vec<u8>)>,
    pub gltf: Vec<u8>,
    pub gbin: Vec<u8>,
    pub facts: String,
    pub nodes: String,
    pub thumb: Option<Vec<u8>>,
    pub overrides: Vec<Override>,
    pub built_from: Option<String>,
    pub scene_hash: Option<String>,
    foreign: Vec<(Tag, String, Vec<u8>)>,
}

pub fn is_manf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("manf"))
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn scene_hash_of(scene: &[(String, Vec<u8>)]) -> String {
    let mut hash = Sha256::new();
    for (name, bytes) in scene {
        hash.update(name.as_bytes());
        hash.update([0]);
        hash.update(bytes);
    }
    hex(&hash.finalize())
}

fn safe_name(name: &str) -> Result<PathBuf, String> {
    let path = Path::new(name);
    let plain = !name.is_empty()
        && name.len() <= 1024
        && !name.contains('\\')
        && path.components().all(|c| matches!(c, Component::Normal(_)));
    if plain {
        Ok(path.to_path_buf())
    } else {
        Err(format!(
            "`{name}` is not a relative path inside the document"
        ))
    }
}

fn name_of(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn relative(root: &Path, file: &Path) -> Option<PathBuf> {
    file.strip_prefix(root).ok().map(Path::to_path_buf)
}

fn split_glb(glb: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(glb.get(at..at + 4)?.try_into().ok()?))
    };
    if glb.get(..4) != Some(b"glTF") {
        return Err("the build is not a GLB".into());
    }
    let mut at = 12;
    let (mut json, mut bin) = (Vec::new(), Vec::new());
    while let (Some(length), Some(kind)) = (word(at), word(at + 4)) {
        let start = at + 8;
        let end = start + length as usize;
        let data = glb.get(start..end).ok_or("a GLB chunk runs past its end")?;
        match kind {
            0x4E4F_534A => json = data.to_vec(),
            0x004E_4942 => bin = data.to_vec(),
            _ => {}
        }
        at = end;
    }
    Ok((json, bin))
}

pub fn join_glb(json: &[u8], bin: &[u8]) -> Vec<u8> {
    let chunks = 8 + json.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(12 + chunks);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((12 + chunks) as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    out.extend_from_slice(json);
    if !bin.is_empty() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        out.extend_from_slice(bin);
    }
    out
}

fn toml_of(value: &serde_json::Value) -> Option<toml::Value> {
    Some(match value {
        serde_json::Value::Null => return None,
        serde_json::Value::Bool(b) => toml::Value::Boolean(*b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => toml::Value::Integer(i),
            None => toml::Value::Float(n.as_f64()?),
        },
        serde_json::Value::String(s) => toml::Value::String(s.clone()),
        serde_json::Value::Array(items) => {
            toml::Value::Array(items.iter().filter_map(toml_of).collect())
        }
        serde_json::Value::Object(map) => toml::Value::Table(
            map.iter()
                .filter_map(|(k, v)| Some((k.clone(), toml_of(v)?)))
                .collect(),
        ),
    })
}

fn node_map(json: &[u8]) -> Result<String, String> {
    let doc: serde_json::Value =
        serde_json::from_slice(json).map_err(|e| format!("the built glTF is not JSON: {e}"))?;
    let mut table = toml::Table::new();
    for (index, node) in doc["nodes"].as_array().into_iter().flatten().enumerate() {
        if let Some(name) = node["name"].as_str() {
            table.insert(name.to_string(), toml::Value::Integer(index as i64));
        }
    }
    toml::to_string(&table).map_err(|e| e.to_string())
}

#[derive(Clone, Debug, PartialEq)]
enum Seg {
    Key(String),
    Named(String),
    Index(usize),
}

fn path_text(segs: &[Seg]) -> String {
    let mut out = String::new();
    for seg in segs {
        match seg {
            Seg::Key(k) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(k);
            }
            Seg::Named(n) => {
                out.push('[');
                out.push_str(n);
                out.push(']');
            }
            Seg::Index(i) => {
                out.push('[');
                out.push('#');
                out.push_str(&i.to_string());
                out.push(']');
            }
        }
    }
    out
}

fn segs_of(text: &str) -> Result<Vec<Seg>, String> {
    let mut segs = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(inner) = rest.strip_prefix('[') {
            let end = inner
                .find(']')
                .ok_or_else(|| format!("`{text}`: an unclosed ["))?;
            let name = &inner[..end];
            segs.push(match name.strip_prefix('#') {
                Some(i) => Seg::Index(i.parse().map_err(|_| format!("`{text}`: a bad index"))?),
                None => Seg::Named(name.to_string()),
            });
            rest = &inner[end + 1..];
        } else {
            let rest_key = rest.strip_prefix('.').unwrap_or(rest);
            let end = rest_key.find(['.', '[']).unwrap_or(rest_key.len());
            if end == 0 {
                return Err(format!("`{text}`: an empty key"));
            }
            segs.push(Seg::Key(rest_key[..end].to_string()));
            rest = &rest_key[end..];
        }
    }
    Ok(segs)
}

fn named_in(array: &[toml::Value]) -> bool {
    let mut names = BTreeSet::new();
    array.iter().all(|v| {
        v.as_table()
            .and_then(|t| t.get("name"))
            .and_then(toml::Value::as_str)
            .is_some_and(|n| names.insert(n.to_string()))
    })
}

fn diff(
    at: &mut Vec<Seg>,
    was: Option<&toml::Value>,
    now: &toml::Value,
    out: &mut Vec<(Vec<Seg>, Option<toml::Value>, toml::Value)>,
) {
    match (was, now) {
        (Some(toml::Value::Table(a)), toml::Value::Table(b)) => {
            for (key, value) in b {
                at.push(Seg::Key(key.clone()));
                diff(at, a.get(key), value, out);
                at.pop();
            }
        }
        (Some(toml::Value::Array(a)), toml::Value::Array(b))
            if named_in(a) && named_in(b) && b.iter().all(toml::Value::is_table) =>
        {
            for item in b {
                let name = item["name"].as_str().unwrap_or_default().to_string();
                let old = a.iter().find(|v| v["name"].as_str() == Some(name.as_str()));
                at.push(Seg::Named(name));
                diff(at, old, item, out);
                at.pop();
            }
        }
        (Some(toml::Value::Array(a)), toml::Value::Array(b))
            if a.len() == b.len() && b.iter().all(toml::Value::is_table) =>
        {
            for (i, (old, item)) in a.iter().zip(b).enumerate() {
                at.push(Seg::Index(i));
                diff(at, Some(old), item, out);
                at.pop();
            }
        }
        (was, now) if was != Some(now) => out.push((at.clone(), was.cloned(), now.clone())),
        _ => {}
    }
}

fn item_of(value: &toml::Value) -> Result<Item, String> {
    let mut table = toml::Table::new();
    table.insert("v".into(), value.clone());
    let text = toml::to_string(&table).map_err(|e| e.to_string())?;
    let doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    doc.get("v")
        .cloned()
        .ok_or_else(|| "a value TOML cannot hold".to_string())
}

fn put_override(doc: &mut DocumentMut, segs: &[Seg], value: &toml::Value) -> Result<(), String> {
    let missing = || format!("the scene has no `{}`", path_text(segs));
    let (last, parents) = segs.split_last().ok_or_else(missing)?;
    let mut item: &mut Item = doc.as_item_mut();
    for seg in parents {
        item = match seg {
            Seg::Key(k) => item
                .get_mut(k.as_str())
                .filter(|i| !i.is_none())
                .ok_or_else(missing)?,
            Seg::Index(i) => item.get_mut(*i).ok_or_else(missing)?,
            Seg::Named(n) => {
                let at = match &*item {
                    Item::ArrayOfTables(array) => array
                        .iter()
                        .position(|t| t.get("name").and_then(Item::as_str) == Some(n.as_str())),
                    Item::Value(toml_edit::Value::Array(array)) => array.iter().position(|v| {
                        v.as_inline_table()
                            .and_then(|t| t.get("name"))
                            .and_then(toml_edit::Value::as_str)
                            == Some(n.as_str())
                    }),
                    _ => None,
                }
                .ok_or_else(missing)?;
                item.get_mut(at).ok_or_else(missing)?
            }
        };
    }
    let Seg::Key(key) = last else {
        return Err(format!("`{}` does not end in a key", path_text(segs)));
    };
    let new = item_of(value)?;
    let table = item.as_table_like_mut().ok_or_else(missing)?;
    table.insert(key, new);
    Ok(())
}

fn apply_overrides(scene: &mut [(String, Vec<u8>)], overrides: &[Override]) -> Result<(), String> {
    for o in overrides {
        let Some((_, bytes)) = scene.iter_mut().find(|(name, _)| *name == o.file) else {
            return Err(format!("override {}: the scene has no {}", o.path, o.file));
        };
        let text = String::from_utf8_lossy(bytes).into_owned();
        let mut doc: DocumentMut = text
            .parse()
            .map_err(|e: toml_edit::TomlError| format!("{}: {}", o.file, e.message()))?;
        put_override(&mut doc, &segs_of(&o.path)?, &o.value)
            .map_err(|e| format!("override {}: {e}", o.path))?;
        *bytes = doc.to_string().into_bytes();
    }
    Ok(())
}

fn overrides_text(overrides: &[Override]) -> Result<String, String> {
    let list: Vec<toml::Value> = overrides
        .iter()
        .map(|o| {
            let mut t = toml::Table::new();
            t.insert("file".into(), toml::Value::String(o.file.clone()));
            t.insert("path".into(), toml::Value::String(o.path.clone()));
            t.insert("value".into(), o.value.clone());
            toml::Value::Table(t)
        })
        .collect();
    let mut table = toml::Table::new();
    table.insert("override".into(), toml::Value::Array(list));
    toml::to_string(&table).map_err(|e| e.to_string())
}

fn overrides_of(text: &str) -> Result<Vec<Override>, String> {
    let table: toml::Table = toml::from_str(text).map_err(|e| format!("OVRD: {e}"))?;
    let mut out = Vec::new();
    for item in table
        .get("override")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        let field = |k: &str| {
            item.get(k)
                .and_then(toml::Value::as_str)
                .map(str::to_string)
        };
        match (field("file"), field("path"), item.get("value")) {
            (Some(file), Some(path), Some(value)) => out.push(Override {
                file,
                path,
                value: value.clone(),
            }),
            _ => return Err("OVRD: an override without file, path and value".into()),
        }
    }
    Ok(out)
}

fn stored(bytes: &[u8]) -> Codec {
    if bytes.starts_with(b"\x89PNG") || bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Codec::Store
    } else {
        Codec::Auto
    }
}

fn text_of(bytes: &[u8], what: &str) -> Result<String, String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| format!("{what} is not UTF-8"))
}

fn real_folder(manf: &Path) -> PathBuf {
    match manf.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

impl Document {
    pub fn pack(recipe: &Path, registry: &Registry) -> Result<Document, String> {
        let shown = recipe.display().to_string();
        let recorder = files::record();
        let loaded = manfred_recipe::load(recipe).map_err(|e| format!("{shown}: {e}"))?;
        let base = real_folder(recipe);
        let built =
            build_with(&loaded.recipe, &base, registry).map_err(|e| format!("{shown}: {e}"))?;
        let mut doc = Document::default();
        doc.fill(&built, &loaded.recipe, &base)?;
        let mut seen = recorder.seen();
        drop(recorder);
        if let Some(sky) = &loaded.recipe.sky
            && let Some(path) = &sky.path
        {
            let file = files::clean(&base.join(path));
            if file.is_file() {
                seen.insert(file);
            }
        }
        let main = files::clean(recipe);
        seen.remove(&main);
        let mut root = main.parent().map(Path::to_path_buf).unwrap_or_default();
        for file in &seen {
            while !file.starts_with(&root) {
                if !root.pop() {
                    break;
                }
            }
        }
        if root.parent().is_none() {
            return Err(format!(
                "{shown}: its files share no folder below the file system's root, expected them beside the recipe or in folders near it"
            ));
        }
        doc.recipe_name = relative(&root, &main)
            .map(|p| name_of(&p))
            .unwrap_or_default();
        doc.recipe_text = files::read_to_string(recipe).map_err(|e| format!("{shown}: {e}"))?;
        for file in seen {
            let Some(name) = relative(&root, &file) else {
                continue;
            };
            let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            doc.files.insert(name, bytes);
        }
        let outputs = std::mem::take(&mut doc.outputs);
        let scene_dir = Path::new(&doc.recipe_name)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        doc.outputs = outputs.into_iter().map(|p| scene_dir.join(p)).collect();
        let beside = std::mem::take(&mut doc.foreign);
        for (_, name, bytes) in beside {
            doc.files.entry(scene_dir.join(&name)).or_insert(bytes);
        }
        doc.built_from = Some(doc.hash());
        doc.facts = doc.facts_text(&built)?;
        Ok(doc)
    }

    fn fill(&mut self, built: &Built, recipe: &Recipe, base: &Path) -> Result<(), String> {
        let glb = built.try_glb().map_err(|e| e.to_string())?;
        let (json, bin) = split_glb(&glb)?;
        self.nodes = node_map(&json)?;
        self.gltf = json;
        self.gbin = bin;
        let scene_file = base.join(ENTRY);
        let glb_file = glb_path(recipe, base, None);
        self.scene.clear();
        self.outputs.clear();
        let mut beside = Vec::new();
        match crate::scene::text_named(
            built,
            recipe,
            (&scene_file, &glb_file, Some(GLTF_NAME)),
            base,
        ) {
            Ok(written) => {
                self.scene
                    .push((ENTRY.to_string(), written.text.into_bytes()));
                let folder = files::clean(base);
                if let Some((path, text)) = written.library
                    && let Some(name) = relative(&folder, &files::clean(&path))
                {
                    self.scene.push((name_of(&name), text.into_bytes()));
                }
                for (path, bytes) in written.files {
                    if let Some(name) = relative(&folder, &files::clean(&path)) {
                        self.outputs.insert(name.clone());
                        beside.push((manf::FILE, name_of(&name), bytes.into_owned()));
                    }
                }
            }
            Err(e) if recipe.output.scene.is_some() => return Err(e.to_string()),
            Err(_) => {}
        }
        apply_overrides(&mut self.scene, &self.overrides)?;
        self.scene_hash = Some(scene_hash_of(&self.scene));
        self.foreign.retain(|(tag, _, _)| *tag != manf::FILE);
        self.foreign.extend(beside);
        Ok(())
    }

    fn facts_text(&self, built: &Built) -> Result<String, String> {
        let mut table = toml::Table::new();
        table.insert("licence".into(), toml::Value::String(LICENCE.into()));
        table.insert(
            "manfred".into(),
            toml::Value::String(env!("CARGO_PKG_VERSION").into()),
        );
        let mut build = toml::Table::new();
        if let Some(from) = &self.built_from {
            build.insert("from".into(), toml::Value::String(from.clone()));
        }
        build.insert(
            "outputs".into(),
            toml::Value::Array(
                self.outputs
                    .iter()
                    .map(|p| toml::Value::String(name_of(p)))
                    .collect(),
            ),
        );
        table.insert("build".into(), toml::Value::Table(build));
        let objects: toml::Table = built
            .object_facts()
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), toml_of(v)?)))
            .collect();
        table.insert("objects".into(), toml::Value::Table(objects));
        toml::to_string(&table).map_err(|e| format!("the facts: {e}"))
    }

    pub fn hash(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(self.recipe_name.as_bytes());
        hash.update([0]);
        hash.update(self.recipe_text.as_bytes());
        for (name, bytes) in self.inputs() {
            hash.update([0]);
            hash.update(name_of(name).as_bytes());
            hash.update([0]);
            hash.update(bytes);
        }
        hex(&hash.finalize())
    }

    pub fn inputs(&self) -> impl Iterator<Item = (&PathBuf, &Vec<u8>)> {
        self.files
            .iter()
            .filter(|(name, _)| !self.outputs.contains(*name))
    }

    pub fn fresh(&self) -> bool {
        !self.gltf.is_empty() && self.built_from.as_deref() == Some(self.hash().as_str())
    }

    pub fn edited(&self) -> bool {
        self.scene_hash
            .as_deref()
            .is_some_and(|recorded| recorded != scene_hash_of(&self.scene))
    }

    pub fn glb(&self) -> Vec<u8> {
        join_glb(&self.gltf, &self.gbin)
    }

    pub fn open(path: &Path) -> Result<Document, String> {
        let reader = Reader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Document::read(&reader).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn read(reader: &Reader<'_>) -> Result<Document, String> {
        let mut doc = Document::default();
        let mut recipe = None;
        for (index, entry) in reader.entries().iter().enumerate() {
            let bytes = reader.read(index).map_err(|e| e.to_string())?.into_owned();
            match entry.tag {
                manf::RCPE => {
                    let name = if entry.name.is_empty() {
                        "recipe.toml".to_string()
                    } else {
                        entry.name.clone()
                    };
                    safe_name(&name)?;
                    recipe = Some((name, text_of(&bytes, "RCPE")?));
                }
                manf::FILE => {
                    doc.files.insert(safe_name(&entry.name)?, bytes);
                }
                manf::SCNE => {
                    safe_name(&entry.name)?;
                    doc.scene.push((entry.name.clone(), bytes));
                }
                manf::GLTF => doc.gltf = bytes,
                manf::GBIN => doc.gbin = bytes,
                manf::FACT => doc.facts = text_of(&bytes, "FACT")?,
                manf::NODE => doc.nodes = text_of(&bytes, "NODE")?,
                manf::THMB => doc.thumb = Some(bytes),
                manf::OVRD => doc.overrides = overrides_of(&text_of(&bytes, "OVRD")?)?,
                manf::SMET => {
                    let table: toml::Table = toml::from_str(&text_of(&bytes, "SMET")?)
                        .map_err(|e| format!("SMET: {e}"))?;
                    doc.scene_hash = table
                        .get("scene")
                        .and_then(toml::Value::as_str)
                        .map(str::to_string);
                }
                tag => doc.foreign.push((tag, entry.name.clone(), bytes)),
            }
        }
        let (name, text) = recipe.ok_or("the document has no recipe (RCPE)")?;
        doc.recipe_name = name;
        doc.recipe_text = text;
        if let Ok(facts) = toml::from_str::<toml::Table>(&doc.facts)
            && let Some(build) = facts.get("build").and_then(toml::Value::as_table)
        {
            doc.built_from = build
                .get("from")
                .and_then(toml::Value::as_str)
                .map(str::to_string);
            doc.outputs = build
                .get("outputs")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(toml::Value::as_str)
                .map(PathBuf::from)
                .collect();
        }
        Ok(doc)
    }

    pub fn to_vec(&self) -> Result<Vec<u8>, String> {
        self.writer()?.to_vec().map_err(|e| e.to_string())
    }

    fn writer(&self) -> Result<Writer<'_>, String> {
        let mut smet = toml::Table::new();
        smet.insert("format".into(), toml::Value::Integer(1));
        smet.insert("entry".into(), toml::Value::String(ENTRY.into()));
        smet.insert(
            "recipe".into(),
            toml::Value::String(sha256(self.recipe_text.as_bytes())),
        );
        smet.insert(
            "scene".into(),
            toml::Value::String(
                self.scene_hash
                    .clone()
                    .unwrap_or_else(|| scene_hash_of(&self.scene)),
            ),
        );
        let smet = toml::to_string(&smet).map_err(|e| e.to_string())?;
        let mut w = Writer::new()
            .chunk(manf::SMET, smet.into_bytes(), Codec::Store)
            .named(
                manf::RCPE,
                self.recipe_name.as_str(),
                self.recipe_text.as_bytes(),
                Codec::Auto,
            );
        for (name, bytes) in &self.files {
            w = w.named(manf::FILE, name_of(name), &bytes[..], stored(bytes));
        }
        for (name, bytes) in &self.scene {
            w = w.named(manf::SCNE, name.as_str(), &bytes[..], Codec::Auto);
        }
        if !self.gltf.is_empty() {
            w = w.chunk(manf::GLTF, &self.gltf[..], Codec::Auto).chunk(
                manf::GBIN,
                &self.gbin[..],
                Codec::Auto,
            );
        }
        w = w
            .chunk(manf::FACT, self.facts.as_bytes(), Codec::Auto)
            .chunk(manf::NODE, self.nodes.as_bytes(), Codec::Auto);
        if let Some(thumb) = &self.thumb {
            w = w.chunk(manf::THMB, &thumb[..], Codec::Store);
        }
        if !self.overrides.is_empty() {
            w = w.chunk(
                manf::OVRD,
                overrides_text(&self.overrides)?.into_bytes(),
                Codec::Auto,
            );
        }
        for (tag, name, bytes) in &self.foreign {
            if !OWNED.contains(tag) {
                w = w.named(*tag, name.as_str(), &bytes[..], Codec::Auto);
            }
        }
        Ok(w)
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        self.writer()?
            .write(path)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn unpack(&self, dir: &Path) -> Result<PathBuf, String> {
        let fail = |path: &Path, e: std::io::Error| format!("{}: {e}", path.display());
        let recipe = dir.join(safe_name(&self.recipe_name)?);
        let mut writes: Vec<(PathBuf, &[u8])> = vec![(recipe.clone(), self.recipe_text.as_bytes())];
        for (name, bytes) in self.inputs() {
            writes.push((dir.join(safe_name(&name_of(name))?), bytes));
        }
        for (path, bytes) in writes {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| fail(parent, e))?;
            }
            std::fs::write(&path, bytes).map_err(|e| fail(&path, e))?;
        }
        Ok(recipe)
    }

    pub fn mount(&self, manf: &Path) -> Result<(PathBuf, files::Mount), String> {
        let folder = real_folder(manf);
        let recipe = folder.join(safe_name(&self.recipe_name)?);
        let mut mounted = BTreeMap::new();
        mounted.insert(recipe.clone(), self.recipe_text.clone().into_bytes());
        for (name, bytes) in &self.files {
            mounted.insert(folder.join(safe_name(&name_of(name))?), bytes.clone());
        }
        Ok((recipe, files::mount(mounted)))
    }

    pub fn take_changes(&mut self, manf: &Path, mount: &files::Mount) -> bool {
        let folder = files::clean(&real_folder(manf));
        let changed = mount.changed();
        for (path, bytes) in &changed {
            let Some(name) = relative(&folder, path) else {
                continue;
            };
            if name_of(&name) == self.recipe_name {
                self.recipe_text = String::from_utf8_lossy(bytes).into_owned();
            } else {
                self.files.insert(name, bytes.clone());
            }
        }
        mount.settle();
        !changed.is_empty()
    }

    pub fn rebuild(&mut self, manf: &Path, registry: &Registry) -> Result<(), String> {
        let (recipe_path, _mount) = self.mount(manf)?;
        let shown = manf.display().to_string();
        let loaded = manfred_recipe::load(&recipe_path).map_err(|e| format!("{shown}: {e}"))?;
        let base = real_folder(&recipe_path);
        let built =
            build_with(&loaded.recipe, &base, registry).map_err(|e| format!("{shown}: {e}"))?;
        let outputs: Vec<PathBuf> = self.outputs.iter().cloned().collect();
        for name in outputs {
            self.files.remove(&name);
        }
        self.fill(&built, &loaded.recipe, &base)?;
        let scene_dir = Path::new(&self.recipe_name)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        self.outputs = std::mem::take(&mut self.outputs)
            .into_iter()
            .map(|p| scene_dir.join(p))
            .collect();
        let beside = std::mem::take(&mut self.foreign);
        for (tag, name, bytes) in beside {
            if tag == manf::FILE {
                self.files.entry(scene_dir.join(&name)).or_insert(bytes);
            } else {
                self.foreign.push((tag, name, bytes));
            }
        }
        self.built_from = Some(self.hash());
        self.facts = self.facts_text(&built)?;
        Ok(())
    }

    pub fn scene_changes(
        &self,
        manf: &Path,
        registry: &Registry,
    ) -> Result<Vec<SceneChange>, String> {
        let mut fresh = Document {
            recipe_name: self.recipe_name.clone(),
            recipe_text: self.recipe_text.clone(),
            files: self.files.clone(),
            outputs: self.outputs.clone(),
            overrides: self.overrides.clone(),
            ..Document::default()
        };
        fresh.rebuild(manf, registry)?;
        let lights: BTreeSet<String> = {
            let (path, _mount) = self.mount(manf)?;
            manfred_recipe::load(&path)
                .map(|l| l.recipe.lights.iter().map(|l| l.name.clone()).collect())
                .unwrap_or_default()
        };
        let mut out = Vec::new();
        for (file, bytes) in &self.scene {
            let parse = |b: &[u8]| -> Result<toml::Value, String> {
                toml::from_str::<toml::Table>(&String::from_utf8_lossy(b))
                    .map(toml::Value::Table)
                    .map_err(|e| format!("{file}: {e}"))
            };
            let now = parse(bytes)?;
            let was = match fresh.scene.iter().find(|(name, _)| name == file) {
                Some((_, b)) => Some(parse(b)?),
                None => None,
            };
            let mut found = Vec::new();
            diff(&mut Vec::new(), was.as_ref(), &now, &mut found);
            for (segs, was, now) in found {
                let recipe = match (file.as_str(), segs.as_slice()) {
                    (ENTRY, [Seg::Key(light), Seg::Named(name), Seg::Key(key)])
                        if light == "light"
                            && lights.contains(name)
                            && LIGHT_KEYS.contains(&key.as_str()) =>
                    {
                        Some(format!("light.{name}.{key}"))
                    }
                    _ => None,
                };
                out.push(SceneChange {
                    file: file.clone(),
                    path: path_text(&segs),
                    was,
                    now,
                    recipe,
                });
            }
        }
        Ok(out)
    }

    pub fn keep(&mut self, changes: &[SceneChange]) {
        for change in changes {
            self.overrides
                .retain(|o| !(o.file == change.file && o.path == change.path));
            self.overrides.push(Override {
                file: change.file.clone(),
                path: change.path.clone(),
                value: change.now.clone(),
            });
        }
        self.scene_hash = Some(scene_hash_of(&self.scene));
    }

    pub fn fold(
        &mut self,
        manf: &Path,
        changes: &[SceneChange],
    ) -> Result<Vec<SceneChange>, String> {
        let (path, mount) = self.mount(manf)?;
        let mut loaded = manfred_recipe::load(&path).map_err(|e| e.to_string())?;
        let mut kept = Vec::new();
        for change in changes {
            let Some(target) = &change.recipe else {
                kept.push(change.clone());
                continue;
            };
            let parts: Vec<&str> = target.splitn(3, '.').collect();
            let [table, name, key] = parts[..] else {
                kept.push(change.clone());
                continue;
            };
            let patch = Patch {
                path: vec![
                    Key::Table(table.to_string()),
                    Key::Object(name.to_string()),
                    Key::Table(key.to_string()),
                ],
                value: Some(item_of(&change.now)?),
            };
            manfred_recipe::apply(&mut loaded, &patch).map_err(|e| format!("{target}: {e}"))?;
        }
        manfred_recipe::save(&loaded).map_err(|e| e.to_string())?;
        self.take_changes(manf, &mount);
        drop(mount);
        self.keep(&kept);
        Ok(kept)
    }
}
