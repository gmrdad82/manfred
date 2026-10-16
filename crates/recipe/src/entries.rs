use std::path::PathBuf;

use serde_json::{Map, json};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, Value};

use crate::patch::{apply_all, save};
use crate::read::load;
use crate::stage::selects;
use crate::validate::validate;
use crate::{ConstraintKind, Error, Key, Kind, Loaded, Patch, Problem, Recipe};

pub const TABLES: [(&str, &str); 10] = [
    ("object", "name"),
    ("camera", "name"),
    ("light", "name"),
    ("route", "name"),
    ("stage", "name"),
    ("particles", "name"),
    ("follow", "object"),
    ("use", "prefix"),
    ("mover", "group"),
    ("marker", "name"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub table: String,
    pub name: String,
    pub detail: Map<String, serde_json::Value>,
}

pub fn label_key(table: &str) -> Result<&'static str, Error> {
    TABLES
        .iter()
        .find(|(t, _)| *t == table)
        .map(|(_, key)| *key)
        .ok_or_else(|| {
            let known: Vec<&str> = TABLES.iter().map(|(t, _)| *t).collect();
            Error::new(
                table,
                None,
                format!("no such table, expected one of: {}", known.join(", ")),
            )
        })
}

fn list_of<'a>(doc: &'a DocumentMut, table: &str) -> Option<&'a ArrayOfTables> {
    doc.get(table).and_then(Item::as_array_of_tables)
}

fn names_in(doc: &DocumentMut, table: &str, key: &str) -> Vec<String> {
    list_of(doc, table)
        .into_iter()
        .flat_map(ArrayOfTables::iter)
        .filter_map(|t| t.get(key).and_then(Item::as_str))
        .map(str::to_string)
        .collect()
}

fn known(l: &Loaded, table: &str, key: &str) -> Vec<String> {
    let mut names = names_in(&l.doc, table, key);
    if table == "object" {
        for o in &l.recipe.objects {
            if !names.contains(&o.name) {
                names.push(o.name.clone());
            }
        }
    }
    names
}

fn missing(table: &str, name: &str, names: &[String]) -> Error {
    let hint = if names.is_empty() {
        format!("the recipe has no {table} entries")
    } else {
        format!("expected one of: {}", names.join(", "))
    };
    Error::new(
        format!("{table}[{name}]"),
        None,
        format!("no such {table}, {hint}"),
    )
}

fn value_of(text: &str) -> Value {
    let mut value = text
        .trim()
        .parse::<Value>()
        .unwrap_or_else(|_| Value::from(text));
    value.decor_mut().clear();
    value
}

fn put(table: &mut Table, pair: &str) -> Result<(), Error> {
    let (dotted, text) = pair.split_once('=').ok_or_else(|| {
        Error::new(
            "",
            None,
            format!("{pair}: expected <key>=<value>, as radius=0.1"),
        )
    })?;
    let dotted = dotted.trim();
    let keys: Vec<&str> = dotted.split('.').map(str::trim).collect();
    if keys.iter().any(|k| k.is_empty()) {
        return Err(Error::new(
            dotted,
            None,
            "expected a key or a dotted key, as params.radius",
        ));
    }
    let (last, parents) = keys.split_last().unwrap_or((&"", &[]));
    let mut at = table;
    for key in parents {
        if !at.contains_key(key) {
            let mut sub = Table::new();
            sub.set_dotted(true);
            at.insert(key, Item::Table(sub));
        }
        at = match at.get_mut(key) {
            Some(Item::Table(sub)) => sub,
            _ => {
                return Err(Error::new(
                    dotted,
                    None,
                    format!("`{key}` is already a value, expected a table to hold `{last}`"),
                ));
            }
        };
    }
    at.insert(last, Item::Value(value_of(text)));
    Ok(())
}

fn unplaced(table: &mut Table) {
    table.set_position(None);
    for (_, item) in table.iter_mut() {
        match item {
            Item::Table(sub) => unplaced(sub),
            Item::ArrayOfTables(list) => list.iter_mut().for_each(unplaced),
            _ => {}
        }
    }
}

fn appended(l: &Loaded, table: &str, entry: Table) -> Patch {
    match list_of(&l.doc, table) {
        Some(list) => Patch {
            path: vec![Key::Table(table.to_string()), Key::Index(list.len())],
            value: Some(Item::Table(entry)),
        },
        None => {
            let mut list = ArrayOfTables::new();
            list.push(entry);
            Patch {
                path: vec![Key::Table(table.to_string())],
                value: Some(Item::ArrayOfTables(list)),
            }
        }
    }
}

fn fresh(l: &Loaded, table: &str, name: &str) -> Result<&'static str, Error> {
    let key = label_key(table)?;
    if name.trim().is_empty() {
        return Err(Error::new(table, None, "expected a non-empty name"));
    }
    if known(l, table, key).iter().any(|n| n == name) {
        return Err(Error::new(
            format!("{table}[{name}]"),
            None,
            format!("the recipe already has a {table} named `{name}`, expected a new name"),
        ));
    }
    Ok(key)
}

fn overridden(entry: &mut Table, key: &str, pairs: &[String]) -> Result<(), Error> {
    for pair in pairs {
        if pair.split_once('=').map(|(k, _)| k.trim()) == Some(key) {
            return Err(Error::new(
                key,
                None,
                format!("the name is given by the command, expected no `{key}=`"),
            ));
        }
        put(entry, pair)?;
    }
    Ok(())
}

pub fn add_patch(l: &Loaded, table: &str, name: &str, pairs: &[String]) -> Result<Patch, Error> {
    let key = fresh(l, table, name)?;
    let mut entry = Table::new();
    entry.insert(key, Item::Value(Value::from(name)));
    overridden(&mut entry, key, pairs)?;
    Ok(appended(l, table, entry))
}

pub fn duplicate_patch(
    l: &Loaded,
    table: &str,
    name: &str,
    new: &str,
    pairs: &[String],
) -> Result<Patch, Error> {
    let key = label_key(table)?;
    let source = list_of(&l.doc, table)
        .and_then(|list| {
            list.iter()
                .find(|t| t.get(key).and_then(Item::as_str) == Some(name))
        })
        .ok_or_else(|| missing(table, name, &names_in(&l.doc, table, key)))?;
    fresh(l, table, new)?;
    let mut entry = source.clone();
    unplaced(&mut entry);
    entry.decor_mut().clear();
    entry.insert(key, Item::Value(Value::from(new)));
    overridden(&mut entry, key, pairs)?;
    Ok(appended(l, table, entry))
}

pub fn remove_patch(l: &Loaded, table: &str, name: &str) -> Result<Patch, Error> {
    let key = label_key(table)?;
    let names = names_in(&l.doc, table, key);
    if !names.iter().any(|n| n == name) {
        return Err(missing(table, name, &names));
    }
    if let Some(by) = referrers(&l.recipe, table, name).first() {
        return Err(Error::new(
            format!("{table}[{name}]"),
            None,
            format!("{by} refers to it; change or remove {by} first"),
        ));
    }
    Ok(Patch {
        path: vec![Key::Table(table.to_string()), Key::Object(name.to_string())],
        value: None,
    })
}

fn names_node(parent: &str, object: &str) -> bool {
    parent == object
        || parent
            .strip_prefix(object)
            .is_some_and(|rest| rest.starts_with('.'))
}

pub fn referrers(r: &Recipe, table: &str, name: &str) -> Vec<String> {
    let mut by = Vec::new();
    match table {
        "object" => {
            for o in &r.objects {
                if o.parent.as_deref() == Some(name) {
                    by.push(format!("object[{}].parent", o.name));
                }
                match &o.kind {
                    Kind::Pocket { into, .. } if into == name => {
                        by.push(format!("object[{}].into", o.name));
                    }
                    Kind::Text { on: Some(on), .. } if on == name => {
                        by.push(format!("object[{}].on", o.name));
                    }
                    _ => {}
                }
            }
            for u in &r.uses {
                if u.parent.as_deref() == Some(name) {
                    by.push(format!("use[{}].parent", u.prefix));
                }
            }
            for f in &r.follows {
                if f.object == name {
                    by.push(format!("follow[{}]", f.object));
                }
            }
            for c in &r.cameras {
                if c.rig.as_ref().is_some_and(|rig| rig.follow == name) {
                    by.push(format!("camera[{}].rig.follow", c.name));
                }
            }
            for light in &r.lights {
                if light
                    .parent
                    .as_ref()
                    .is_some_and(|(p, _)| names_node(p, name))
                {
                    by.push(format!("light[{}].parent", light.name));
                }
            }
            for p in &r.effects.particles {
                if p.parent.as_deref().is_some_and(|p| names_node(p, name)) {
                    by.push(format!("particles[{}].parent", p.name));
                }
            }
            for m in &r.movers {
                if m.parts.iter().any(|part| part == name) {
                    by.push(format!("mover[{}].parts", m.group));
                }
            }
            for stage in &r.stages {
                let only = stage.objects.iter().any(|glob| {
                    selects(glob, name)
                        && !r
                            .objects
                            .iter()
                            .any(|o| o.name != name && selects(glob, &o.name))
                });
                if only {
                    by.push(format!("stage[{}].objects", stage.name));
                }
            }
        }
        "route" => {
            for f in &r.follows {
                if f.route == name {
                    by.push(format!("follow[{}].route", f.object));
                }
            }
            for o in &r.objects {
                if let Kind::Generator { params, .. } = &o.kind
                    && params.get("route").and_then(serde_json::Value::as_str) == Some(name)
                {
                    by.push(format!("object[{}].params.route", o.name));
                }
            }
        }
        "camera" => {
            if r.output.scene_camera.as_deref() == Some(name) {
                by.push("output.scene_camera".to_string());
            }
            if r.plate.as_ref().and_then(|p| p.camera.as_deref()) == Some(name) {
                by.push("plate.camera".to_string());
            }
            for stage in &r.stages {
                if stage.camera.as_deref() == Some(name) {
                    by.push(format!("stage[{}].camera", stage.name));
                }
            }
        }
        _ => {}
    }
    let target = match table {
        "object" => Some(name.to_string()),
        "camera" => Some(format!("camera.{name}")),
        "light" => Some(format!("light.{name}")),
        _ => None,
    };
    for c in r.constraints.iter().flat_map(|c| &c.list) {
        if let (Some(target), Some((key, at))) = (&target, c.kind.target())
            && (at == target || (table == "object" && names_node(at, name)))
        {
            by.push(format!("{}.{key}", c.path));
        }
        if table == "route"
            && let ConstraintKind::FollowPath(f) = &c.kind
            && f.route == name
        {
            by.push(format!("{}.route", c.path));
        }
    }
    by
}

fn detail(r: &Recipe, table: &str, t: &Table) -> Map<String, serde_json::Value> {
    let mut out = Map::new();
    let text = |key: &str| t.get(key).and_then(Item::as_str).map(str::to_string);
    match table {
        "object" => {
            let name = text("name").unwrap_or_default();
            let object = r.objects.iter().find(|o| o.name == name);
            let kind = object.map(|o| match &o.kind {
                Kind::Generator { generator, .. } => generator.clone(),
                other => other.name().to_string(),
            });
            out.insert("kind".into(), json!(kind.or_else(|| text("kind"))));
            out.insert(
                "parent".into(),
                json!(
                    object
                        .and_then(|o| o.parent.clone())
                        .or_else(|| text("parent"))
                ),
            );
        }
        "marker" => {
            out.insert(
                "t".into(),
                json!(
                    t.get("t")
                        .and_then(Item::as_value)
                        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
                ),
            );
        }
        "follow" => {
            out.insert("route".into(), json!(text("route")));
        }
        "light" | "particles" => {
            out.insert("parent".into(), json!(text("parent")));
        }
        "camera" => {
            let follow = t
                .get("rig")
                .and_then(Item::as_table_like)
                .and_then(|rig| rig.get("follow"))
                .and_then(Item::as_str);
            out.insert("follow".into(), json!(follow));
        }
        _ => {}
    }
    out
}

pub fn entries(l: &Loaded, table: Option<&str>) -> Result<Vec<Entry>, Error> {
    let chosen: Vec<(&str, &str)> = match table {
        Some(table) => vec![(table, label_key(table)?)],
        None => TABLES.to_vec(),
    };
    let mut out = Vec::new();
    for (table, key) in chosen {
        for t in list_of(&l.doc, table)
            .into_iter()
            .flat_map(ArrayOfTables::iter)
        {
            out.push(Entry {
                table: table.to_string(),
                name: t
                    .get(key)
                    .and_then(Item::as_str)
                    .unwrap_or_default()
                    .to_string(),
                detail: detail(&l.recipe, table, t),
            });
        }
    }
    Ok(out)
}

fn files(l: &Loaded) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    let mut paths = vec![l.path.clone()];
    if let Some(base) = l.recipe.base.as_deref() {
        for name in l.recipe.files.names().into_iter().skip(1) {
            paths.push(base.join(name));
        }
    }
    paths
        .into_iter()
        .map(|p| {
            let bytes = std::fs::read(&p).ok();
            (p, bytes)
        })
        .collect()
}

fn restore(files: &[(PathBuf, Option<Vec<u8>>)]) {
    for (path, bytes) in files {
        match bytes {
            Some(bytes) => {
                if std::fs::read(path).ok().as_deref() != Some(bytes.as_slice()) {
                    let _ = std::fs::write(path, bytes);
                }
            }
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn same(a: &Problem, b: &Problem) -> bool {
    a.path == b.path && a.text == b.text
}

pub fn write_checked(l: &mut Loaded, patches: &[Patch]) -> Result<(), Error> {
    let before = validate(&l.recipe);
    let kept = files(l);
    let mut edited = l.clone();
    apply_all(&mut edited, patches)?;
    let checked = save(&edited).and_then(|()| load(&edited.path));
    let after = match checked {
        Ok(after) => after,
        Err(e) => {
            restore(&kept);
            return Err(Error::new(
                e.path,
                e.line,
                format!("{}; the recipe is left as it was", e.text),
            ));
        }
    };
    let problems = validate(&after.recipe);
    if let Some(p) = problems.iter().find(|p| !before.iter().any(|b| same(b, p))) {
        restore(&kept);
        return Err(Error::new(
            p.path.clone(),
            p.line,
            format!("{}; the recipe is left as it was", p.text),
        ));
    }
    *l = after;
    Ok(())
}
