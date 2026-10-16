use toml_edit::{InlineTable, Item, Table, Value};

use crate::include::{Edit, is_array};
use crate::patch::{Reach, apply, edit_root, morph_room, path_text, reach, value_in, write};
use crate::read::params_ref;
use crate::variant::{keys, replay};
use crate::{Error, Key, Loaded, Patch, Recipe};

#[derive(Clone, Debug)]
pub struct Change {
    pub key: String,
    pub old: Option<Value>,
    pub new: Value,
    pub file: String,
}

fn dotted(set: &str) -> Result<(&str, &str), Error> {
    let (dotted, text) = set.split_once('=').ok_or_else(|| {
        Error::new(
            "",
            None,
            format!("`{set}`: expected <key>=<value>, as sky.intensity=0.4"),
        )
    })?;
    let dotted = dotted.trim();
    if dotted.is_empty() || dotted.split('.').any(str::is_empty) {
        return Err(Error::new(
            dotted,
            None,
            format!("`{dotted}` is not a dotted key, as camera.main.fov_y_deg"),
        ));
    }
    Ok((dotted, text))
}

fn fresh(text: &str) -> Value {
    text.trim()
        .parse::<Value>()
        .unwrap_or_else(|_| Value::from(text))
}

fn nested(path: &[Key], value: Value, inline: bool) -> Item {
    let mut item = Item::Value(value);
    for key in path.iter().rev() {
        let (Key::Table(name) | Key::Object(name)) = key else {
            continue;
        };
        item = if inline {
            let mut t = InlineTable::new();
            if let Item::Value(v) = item {
                t.insert(name, v);
            }
            Item::Value(Value::InlineTable(t))
        } else {
            let mut t = Table::new();
            t.set_implicit(item.is_table());
            t.insert(name, item);
            Item::Table(t)
        };
    }
    item
}

fn texts(l: &Loaded) -> Vec<(String, String)> {
    let files = &l.recipe.files;
    let mut out = vec![(files.main_name().to_string(), write(l))];
    for name in files.names().into_iter().skip(1) {
        if let Some(text) = files.text_of(name) {
            out.push((name.to_string(), text.to_string()));
        }
    }
    out
}

fn patch_for(l: &Loaded, dotted: &str, text: &str) -> Result<(Option<Value>, Patch), Error> {
    let fail = |text: String| Error::new(dotted, None, text);
    let path = keys(dotted);
    let name = |at: usize| match &path[at] {
        Key::Table(n) | Key::Object(n) => n.clone(),
        Key::Index(i) => i.to_string(),
    };
    match reach(l.doc.as_table(), &path) {
        Reach::Value(old) => {
            let new = value_like(&old, text).map_err(fail)?;
            Ok((
                Some(old),
                Patch {
                    path,
                    value: Some(Item::Value(new)),
                },
            ))
        }
        Reach::Missing(0, _) if is_array(&name(0)) => Err(fail(format!(
            "the recipe has no [[{}]] entries to set a value in",
            name(0)
        ))),
        Reach::Missing(at, inline) => {
            let item = nested(&path[at + 1..], fresh(text), inline);
            Ok((
                None,
                Patch {
                    path: path[..=at].to_vec(),
                    value: Some(item),
                },
            ))
        }
        Reach::NoEntry(at) => Err(fail(format!(
            "the recipe has no {} named `{}`",
            path_text(&path[..at]),
            name(at)
        ))),
        Reach::Inside(at) => Err(fail(format!(
            "`{}` is a value, with nothing inside it",
            path_text(&path[..at])
        ))),
        Reach::Item(_) => Err(fail(format!(
            "`{dotted}` is a table; set a value inside it, as {dotted}.<key>=<value>"
        ))),
    }
}

pub fn set_values(l: &mut Loaded, sets: &[String]) -> Result<Vec<Change>, Error> {
    let mut changes = Vec::with_capacity(sets.len());
    for set in sets {
        let (dotted, text) = dotted(set)?;
        let (old, patch) = patch_for(l, dotted, text)?;
        let before = texts(l);
        apply(l, &patch).map_err(|mut e| {
            if e.path.is_empty() {
                e.path = dotted.to_string();
            }
            e
        })?;
        let after = texts(l);
        let file = after
            .iter()
            .find(|(name, text)| !before.iter().any(|(n, t)| n == name && t == text))
            .map_or_else(|| after[0].0.clone(), |(name, _)| name.clone());
        let new = value_of(l, dotted)?;
        changes.push(Change {
            key: dotted.to_string(),
            old,
            new,
            file,
        });
    }
    Ok(changes)
}

pub fn value_of(l: &Loaded, dotted: &str) -> Result<Value, Error> {
    let fail = |text: String| Error::new(dotted, None, text);
    if dotted.is_empty() || dotted.split('.').any(str::is_empty) {
        return Err(fail(format!(
            "`{dotted}` is not a dotted key, as camera.main.fov_y_deg"
        )));
    }
    match reach(l.doc.as_table(), &keys(dotted)) {
        Reach::Value(v) => Ok(v),
        Reach::Item(Item::Table(t)) => Ok(Value::InlineTable(t.into_inline_table())),
        Reach::Item(Item::ArrayOfTables(a)) => Ok(Value::Array(a.into_array())),
        _ => Err(fail(format!("the recipe has no value at `{dotted}`"))),
    }
}

pub fn tidy(value: &Value) -> Value {
    let mut v = value.clone();
    tidy_in(&mut v);
    v
}

fn tidy_in(v: &mut Value) {
    match v {
        Value::Array(a) => {
            a.iter_mut().for_each(tidy_in);
            a.fmt();
        }
        Value::InlineTable(t) => {
            t.iter_mut().for_each(|(_, v)| tidy_in(v));
            t.fmt();
        }
        _ => {}
    }
    v.decor_mut().clear();
}

pub fn json_of(value: &Value) -> serde_json::Value {
    match value {
        Value::String(s) => serde_json::Value::from(s.value().as_str()),
        Value::Integer(i) => serde_json::Value::from(*i.value()),
        Value::Float(f) => serde_json::Value::from(*f.value()),
        Value::Boolean(b) => serde_json::Value::from(*b.value()),
        Value::Datetime(d) => serde_json::Value::from(d.value().to_string()),
        Value::Array(a) => a.iter().map(json_of).collect(),
        Value::InlineTable(t) => t
            .iter()
            .map(|(k, v)| (k.to_string(), json_of(v)))
            .collect::<serde_json::Map<_, _>>()
            .into(),
    }
}

pub fn apply_set(r: &Recipe, sets: &[String]) -> Result<Recipe, Error> {
    if sets.is_empty() {
        return Ok(r.clone());
    }
    replay(r, Edit::Set(sets.to_vec()))
}

pub(crate) fn set_in(table: &mut Table, sets: &[String]) -> Result<(), Error> {
    for set in sets {
        let (dotted, text) = set.split_once('=').ok_or_else(|| {
            Error::new(
                "",
                None,
                format!("--set {set}: expected <key>=<value>, as sky.intensity=0.4"),
            )
        })?;
        let dotted = dotted.trim();
        let fail = |text: String| Error::new(dotted, None, format!("--set: {text}"));
        if dotted.is_empty() || dotted.split('.').any(str::is_empty) {
            return Err(fail(format!(
                "`{dotted}` is not a dotted key, as object.liquid.params.liquid"
            )));
        }
        let path = keys(dotted);
        let old = match value_in(table, &path) {
            Some(old) => old,
            None if morph_room(table, &path) => Value::from(0.0),
            None => {
                return Err(fail(format!(
                    "the recipe has no value at `{dotted}`; --set only replaces a value the recipe already has"
                )));
            }
        };
        let new = value_like(&old, text).map_err(fail)?;
        edit_root(table, &path, Some(Item::Value(new))).map_err(fail)?;
    }
    Ok(())
}

fn value_like(old: &Value, text: &str) -> Result<Value, String> {
    let reference = old.as_str().and_then(params_ref).is_some();
    if let Value::String(_) = old
        && !reference
    {
        return Ok(Value::from(text));
    }
    let new: Value = text
        .trim()
        .parse()
        .map_err(|_| format!("`{text}` is not a TOML value, expected {}", kind(old)))?;
    let fits = matches!(
        (old, &new),
        (
            Value::Integer(_) | Value::Float(_),
            Value::Integer(_) | Value::Float(_)
        ) | (Value::Boolean(_), Value::Boolean(_))
            | (Value::Array(_), Value::Array(_))
            | (Value::InlineTable(_), Value::InlineTable(_))
    ) || (reference && matches!(new, Value::Integer(_) | Value::Float(_)));
    if fits {
        Ok(new)
    } else {
        Err(format!("`{text}` is not {}", kind(old)))
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "a string",
        Value::Integer(_) | Value::Float(_) => "a number",
        Value::Boolean(_) => "true or false",
        Value::Datetime(_) => "a date",
        Value::Array(_) => "an array",
        Value::InlineTable(_) => "an inline table",
    }
}

#[cfg(test)]
mod tests {
    use super::apply_set;
    use crate::{Kind, parse};

    const RECIPE: &str = r#"manfred = 1
name = "truth"

[sky]
kind = "hdr"
path = "sky.hdr"
source = "a test sky"
intensity = 0.4

[[object]]
name = "liquid"
kind = "generator"
generator = "app/liquid"
params = { liquid = "fixture.liquid", width = 64, height = 48 }
two_sided = false
"#;

    fn sets(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_set_replaces_values_the_recipe_has() {
        let recipe = parse(RECIPE).unwrap();
        let set = apply_set(
            &recipe,
            &sets(&[
                "object.liquid.params.liquid=/abs/tmp/lab/run1.liquid",
                "object.liquid.params.width=1586",
                "sky.intensity=1.4",
                "sky.path=other sky.hdr",
                "object.liquid.two_sided=true",
            ]),
        )
        .unwrap();
        let sky = set.sky.as_ref().unwrap();
        assert_eq!(sky.intensity, 1.4);
        assert_eq!(sky.path.as_deref(), Some("other sky.hdr"));
        let object = &set.objects[0];
        assert!(object.two_sided);
        let Kind::Generator { params, .. } = &object.kind else {
            panic!("a generator stays a generator");
        };
        assert_eq!(params["liquid"], "/abs/tmp/lab/run1.liquid");
        assert_eq!(params["width"], 1586);
        assert_eq!(params["height"], 48);
        assert!(set.source.contains("intensity = 1.4\n"));
        assert_eq!(apply_set(&recipe, &[]).unwrap(), recipe);
    }

    #[test]
    fn an_unknown_key_is_refused_by_name() {
        let recipe = parse(RECIPE).unwrap();
        for (set, key) in [
            ("sky.intensty=0.4", "sky.intensty"),
            (
                "object.liquid.params.liquidd=x",
                "object.liquid.params.liquidd",
            ),
            ("object.nope.params.liquid=x", "object.nope.params.liquid"),
            ("camera.eye.position=[0, 0, 0]", "camera.eye.position"),
            ("sky=0.4", "sky"),
        ] {
            let e = apply_set(&recipe, &sets(&[set])).unwrap_err();
            assert_eq!(e.path, key, "{set}");
            assert!(
                e.text.contains(&format!("no value at `{key}`")),
                "{set}: {e}"
            );
        }
    }

    #[test]
    fn a_value_must_be_of_the_kind_it_replaces() {
        let recipe = parse(RECIPE).unwrap();
        for set in [
            "sky.intensity=bright",
            "object.liquid.two_sided=1",
            "object.liquid.params.width=[1]",
        ] {
            let e = apply_set(&recipe, &sets(&[set])).unwrap_err();
            assert!(e.text.starts_with("--set: "), "{set}: {e}");
        }
        for set in ["sky.intensity", "=0.4", "sky..intensity=1", ".sky=1"] {
            assert!(apply_set(&recipe, &sets(&[set])).is_err(), "{set}");
        }
    }

    #[test]
    fn the_recipe_still_validates_after_a_set() {
        let recipe = parse(RECIPE).unwrap();
        let e = apply_set(&recipe, &sets(&["sky.kind=nowhere"])).unwrap_err();
        assert!(e.path.starts_with("sky"), "{e}");
    }

    const CHASE: &str = r#"manfred = 1
name = "chase"

# the chase camera
[[camera]]
name = "chase"
kind = "perspective"
position = [1.0, 1.0, -7.0] # behind the hero
target = [1.0, 2.0, 8.0]
fov_y_deg = 80.0

[sky]
kind = "analytic"
intensity = 1.5 # golden hour

[look]
exposure = 3.0

[[object]]
name = "box"
kind = "box"
size = [1.0, 1.0, 1.0]
"#;

    fn loaded(text: &str) -> crate::Loaded {
        crate::reread(std::path::Path::new("chase.toml"), text).unwrap()
    }

    #[test]
    fn a_written_set_keeps_comments_and_round_trips() {
        let mut l = loaded(CHASE);
        let changes = super::set_values(
            &mut l,
            &sets(&[
                "camera.chase.position=[2, 1.5, -9]",
                "sky.intensity=1.8",
                "object.box.size=[2.0, 1.0, 1.0]",
            ]),
        )
        .unwrap();
        let text = crate::write(&l);
        for kept in ["# the chase camera", "# behind the hero", "# golden hour"] {
            assert!(text.contains(kept), "{text}");
        }
        assert!(text.contains("intensity = 1.8 # golden hour"), "{text}");
        assert_eq!(
            changes[1].old.as_ref().and_then(|v| v.as_float()),
            Some(1.5)
        );
        assert_eq!(
            super::json_of(&changes[0].new),
            serde_json::json!([2, 1.5, -9])
        );
        assert_eq!(changes[0].file, "chase.toml");
        let again = loaded(&text);
        assert_eq!(again.recipe, l.recipe);
        assert_eq!(again.recipe.sky.as_ref().unwrap().intensity, 1.8);
        assert_eq!(
            super::tidy(&super::value_of(&again, "camera.chase.position").unwrap()).to_string(),
            "[2, 1.5, -9]"
        );
    }

    #[test]
    fn a_written_set_adds_a_missing_key_or_table_in_an_existing_table() {
        let mut l = loaded(CHASE);
        let changes = super::set_values(
            &mut l,
            &sets(&["look.view=standard", "look.grade.bloom=0.2"]),
        )
        .unwrap();
        assert!(changes.iter().all(|c| c.old.is_none()));
        let text = crate::write(&l);
        assert!(
            text.contains("[look]\nexposure = 3.0\nview = \"standard\"\n"),
            "{text}"
        );
        assert!(text.contains("[look.grade]\nbloom = 0.2\n"), "{text}");
        let again = loaded(&text);
        assert_eq!(
            super::value_of(&again, "look.grade.bloom")
                .unwrap()
                .as_float(),
            Some(0.2)
        );
        for (set, key) in [
            ("sky.intensity=bright", "sky.intensity"),
            ("camera.wide.fov_y_deg=50", "camera.wide.fov_y_deg"),
            ("light.key.intensity=2", "light.key.intensity"),
            ("look.nope=1", "look"),
            ("bogus.amount=1", ""),
        ] {
            let mut l = loaded(CHASE);
            let e = super::set_values(&mut l, &sets(&[set])).unwrap_err();
            assert!(e.to_string().contains(key), "{set}: {e}");
            assert_eq!(crate::write(&l), CHASE, "{set}");
        }
    }

    const FACES: &str = r#"manfred = 1
name = "m"

[[object]]
name = "face"
kind = "mesh"
path = "head.glb"
morph = { up = 1.0 }

[[object]]
name = "plain"
kind = "mesh"
path = "head.glb"

[variant.calm]
set = { "object.face.morph.up" = 0.0, "object.plain.morph.wide" = -0.5 }
"#;

    fn morph(r: &crate::Recipe, at: usize) -> Vec<(String, f64)> {
        match &r.objects[at].kind {
            Kind::Mesh { morph, .. } => morph.clone().into_iter().collect(),
            _ => panic!("a mesh stays a mesh"),
        }
    }

    #[test]
    fn a_set_and_a_variant_reach_a_mesh_s_morph_weights() {
        let recipe = parse(FACES).unwrap();
        let set = apply_set(
            &recipe,
            &sets(&["object.face.morph.wide=0.25", "object.plain.morph.up=2"]),
        )
        .unwrap();
        assert_eq!(
            morph(&set, 0),
            [("up".to_string(), 1.0), ("wide".to_string(), 0.25)]
        );
        assert_eq!(morph(&set, 1), [("up".to_string(), 2.0)]);
        let calm = crate::apply_variant(&recipe, "calm").unwrap();
        assert_eq!(morph(&calm, 0), [("up".to_string(), 0.0)]);
        assert_eq!(morph(&calm, 1), [("wide".to_string(), -0.5)]);
        assert!(apply_set(&recipe, &sets(&["object.face.morph.up=x"])).is_err());
        let e = parse(
            "manfred = 1\nname = \"m\"\n[[object]]\nname = \"b\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\nmorph = { up = 1.0 }\n",
        )
        .unwrap_err();
        assert!(e.text.contains("goes with kind = \"mesh\""), "{e}");
    }
}
