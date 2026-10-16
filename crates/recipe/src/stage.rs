use std::collections::BTreeSet;

use toml_edit::{Item, Table, TableLike};

use crate::include::Edit;
use crate::variant::{glob_matches, replay};
use crate::{Error, Recipe};

fn whole(globs: &[String], name: &str) -> bool {
    globs.iter().any(|g| glob_matches(g, name))
}

fn partly(globs: &[String], name: &str) -> bool {
    globs.iter().any(|g| {
        g.split_once('.')
            .is_some_and(|(head, _)| glob_matches(head, name))
    })
}

pub(crate) fn selects(glob: &str, name: &str) -> bool {
    let one = [glob.to_string()];
    whole(&one, name) || partly(&one, name)
}

fn text<'a>(t: &'a dyn TableLike, key: &str) -> Option<&'a str> {
    t.get(key).and_then(Item::as_str)
}

fn each(item: &Item) -> Vec<&dyn TableLike> {
    match item {
        Item::ArrayOfTables(list) => list.iter().map(|t| t as &dyn TableLike).collect(),
        Item::Value(toml_edit::Value::Array(list)) => list
            .iter()
            .filter_map(|v| v.as_inline_table().map(|t| t as &dyn TableLike))
            .collect(),
        _ => Vec::new(),
    }
}

fn retain(table: &mut Table, key: &str, keep: impl Fn(&dyn TableLike) -> bool) {
    match table.get_mut(key) {
        Some(Item::ArrayOfTables(list)) => list.retain(|t| keep(t)),
        Some(Item::Value(toml_edit::Value::Array(list))) => {
            list.retain(|v| v.as_inline_table().is_none_or(|t| keep(t)));
        }
        _ => {}
    }
}

fn owned_by(dropped: &BTreeSet<String>, node: &str) -> bool {
    dropped.iter().any(|name| {
        node == name
            || node
                .strip_prefix(name.as_str())
                .is_some_and(|rest| rest.starts_with('.'))
    })
}

pub(crate) fn keep_in(table: &mut Table, globs: &[String]) {
    let objects: Vec<(String, Vec<String>)> = table
        .get("object")
        .map(each)
        .unwrap_or_default()
        .into_iter()
        .map(|t| {
            let carriers = ["parent", "on", "into"]
                .iter()
                .filter_map(|key| text(t, key).map(str::to_string))
                .collect();
            (text(t, "name").unwrap_or("").to_string(), carriers)
        })
        .collect();
    let names: BTreeSet<String> = objects.iter().map(|(name, _)| name.clone()).collect();
    let mut kept: BTreeSet<String> = objects
        .iter()
        .filter(|(name, _)| whole(globs, name) || partly(globs, name))
        .map(|(name, _)| name.clone())
        .collect();
    loop {
        let before = kept.len();
        for (name, carriers) in &objects {
            if carriers
                .iter()
                .any(|c| names.contains(c) && !kept.contains(c))
            {
                kept.remove(name);
            }
        }
        if kept.len() == before {
            break;
        }
    }
    let dropped: BTreeSet<String> = names.difference(&kept).cloned().collect();
    retain(table, "object", |t| {
        kept.contains(text(t, "name").unwrap_or(""))
    });
    retain(table, "follow", |t| {
        !dropped.contains(text(t, "object").unwrap_or(""))
    });
    let rig_of = |t: &dyn TableLike| {
        t.get("rig")
            .and_then(Item::as_table_like)
            .and_then(|rig| rig.get("follow"))
            .and_then(Item::as_str)
            .map(str::to_string)
    };
    let follows_dropped = |t: &dyn TableLike| {
        rig_of(t).is_some_and(|follow| dropped.contains(&follow))
            || t.get("constraint")
                .map(each)
                .unwrap_or_default()
                .into_iter()
                .any(|c| {
                    text(c, "type") == Some("rig")
                        && text(c, "follow").is_some_and(|f| owned_by(&dropped, f))
                })
    };
    let cameras: BTreeSet<String> = table
        .get("camera")
        .map(each)
        .unwrap_or_default()
        .into_iter()
        .filter(|t| follows_dropped(*t))
        .filter_map(|t| text(t, "name").map(str::to_string))
        .collect();
    retain(table, "camera", |t| !follows_dropped(t));
    if let Some(output) = table.get_mut("output").and_then(Item::as_table_like_mut)
        && output
            .get("scene_camera")
            .and_then(Item::as_str)
            .is_some_and(|name| cameras.contains(name))
    {
        output.remove("scene_camera");
    }
    for key in ["light", "particles"] {
        retain(table, key, |t| {
            text(t, "parent").is_none_or(|parent| !owned_by(&dropped, parent))
        });
    }
    let aims_at_dropped = |c: &dyn TableLike| {
        ["target", "follow"]
            .iter()
            .any(|key| text(c, key).is_some_and(|name| owned_by(&dropped, name)))
    };
    for key in ["object", "camera", "light"] {
        if let Some(Item::ArrayOfTables(list)) = table.get_mut(key) {
            for entry in list.iter_mut() {
                retain(entry, "constraint", |c| !aims_at_dropped(c));
            }
        }
    }
    table.remove("stage");
    table.remove("variant");
}

pub fn stage_index(r: &Recipe, which: &str) -> Result<usize, Error> {
    let names: Vec<&str> = r.stages.iter().map(|s| s.name.as_str()).collect();
    if let Some(at) = names.iter().position(|name| *name == which) {
        return Ok(at);
    }
    let expected = if names.is_empty() {
        "the recipe has no [[stage]]".to_string()
    } else {
        format!(
            "expected a number from 1 to {} or one of: {}",
            names.len(),
            names.join(", ")
        )
    };
    match which.trim().parse::<usize>() {
        Ok(n) if (1..=names.len()).contains(&n) => Ok(n - 1),
        _ => Err(Error::new(
            "stage",
            None,
            format!("`{which}` is no stage, {expected}"),
        )),
    }
}

pub fn apply_stage(r: &Recipe, which: &str) -> Result<Recipe, Error> {
    let at = stage_index(r, which)?;
    let globs: Vec<String> = r.stages[..=at]
        .iter()
        .flat_map(|stage| stage.objects.iter().cloned())
        .collect();
    replay(r, Edit::Keep(globs))
}

pub fn apply_only(r: &Recipe, globs: &[String]) -> Result<Recipe, Error> {
    if globs.is_empty() {
        return Err(Error::new(
            "only",
            None,
            "expected one glob at least, as road,hero*",
        ));
    }
    for glob in globs {
        if !r.objects.iter().any(|o| selects(glob, &o.name)) {
            return Err(Error::new(
                "only",
                None,
                format!(
                    "`{glob}` matches no object, expected an object's name, a glob, or <object>.<piece>"
                ),
            ));
        }
    }
    replay(r, Edit::Keep(globs.to_vec()))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pick {
    pub stage: Option<String>,
    pub only: Vec<String>,
}

impl Pick {
    pub fn apply(&self, r: &Recipe) -> Result<Recipe, Error> {
        let staged = match &self.stage {
            Some(which) => apply_stage(r, which)?,
            None => r.clone(),
        };
        if self.only.is_empty() {
            Ok(staged)
        } else {
            apply_only(&staged, &self.only)
        }
    }
}
