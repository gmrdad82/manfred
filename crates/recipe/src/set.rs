use toml_edit::{Item, Table, Value};

use crate::include::Edit;
use crate::patch::{edit_root, value_in};
use crate::read::params_ref;
use crate::variant::{keys, replay};
use crate::{Error, Recipe};

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
        let old = value_in(table, &path).ok_or_else(|| {
            fail(format!(
                "the recipe has no value at `{dotted}`; --set only replaces a value the recipe already has"
            ))
        })?;
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
}
