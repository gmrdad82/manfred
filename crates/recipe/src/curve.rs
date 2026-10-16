use std::collections::BTreeMap;

use toml_edit::{Array, Item, Value};

use crate::patch::value_at;
use crate::{Error, Key, Kind, Loaded, Ortho, Patch};

const LEAST_POINTS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CurveEdit {
    Set(usize, [f64; 2]),
    Add(Option<usize>, [f64; 2]),
    Remove(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CurveParam {
    pub path: String,
    pub points: Vec<[f64; 2]>,
    pub view: Option<Ortho>,
    pub depth: f64,
}

pub(crate) enum Op<'a> {
    Set(usize, &'a [f64]),
    Add(Option<usize>, &'a [f64]),
    Remove(usize),
}

fn point(p: &[f64]) -> Value {
    let mut array = Array::new();
    for c in p {
        array.push(*c);
    }
    Value::Array(array)
}

fn plain(decor: Option<&str>, first: bool) -> String {
    match decor.and_then(|d| d.rfind('\n').map(|at| &d[at..])) {
        Some(indent) => indent.to_string(),
        None if first => String::new(),
        None => " ".to_string(),
    }
}

fn joined(gone: &str, own: &str, first: bool) -> String {
    let indent = own.rfind('\n').map_or(own, |b| &own[b + 1..]);
    match gone.rfind('\n') {
        Some(a) => format!("{}{indent}", &gone[..=a]),
        None if first => String::new(),
        None => indent.to_string(),
    }
}

fn prefix(array: &Array, at: usize) -> Option<String> {
    array
        .get(at)
        .and_then(|v| v.decor().prefix())
        .and_then(|p| p.as_str())
        .map(str::to_string)
}

pub(crate) fn edit_points(array: &mut Array, op: Op<'_>, noun: &str) -> Result<(), String> {
    let len = array.len();
    let missing = |i: usize| format!("no point {i}, the {noun} has {len}");
    match op {
        Op::Set(i, p) => {
            if i >= len {
                return Err(missing(i));
            }
            let mut v = point(p);
            *v.decor_mut() = array
                .get(i)
                .map(|old| old.decor().clone())
                .unwrap_or_default();
            array.replace(i, v);
        }
        Op::Add(i, p) => {
            let i = i.unwrap_or(len);
            if i > len {
                return Err(format!(
                    "no room at {i}, the {noun} has {len} points; give 0 to {len}"
                ));
            }
            let mut v = point(p);
            if i < len {
                let was = prefix(array, i);
                v.decor_mut().set_prefix(was.clone().unwrap_or_default());
                array.insert_formatted(i, v);
                if let Some(next) = array.get_mut(i + 1) {
                    next.decor_mut().set_prefix(plain(was.as_deref(), false));
                }
            } else {
                let like = len.checked_sub(1).and_then(|last| prefix(array, last));
                v.decor_mut().set_prefix(plain(like.as_deref(), len == 0));
                array.insert_formatted(i, v);
            }
        }
        Op::Remove(i) => {
            if i >= len {
                return Err(missing(i));
            }
            let removed = array.remove(i);
            let gone = removed
                .decor()
                .prefix()
                .and_then(|p| p.as_str())
                .unwrap_or_default()
                .to_string();
            match array.get_mut(i) {
                Some(next) => {
                    let own = next
                        .decor()
                        .prefix()
                        .and_then(|p| p.as_str())
                        .unwrap_or_default()
                        .to_string();
                    next.decor_mut().set_prefix(joined(&gone, &own, i == 0));
                }
                None => {
                    let trailing = array.trailing().as_str().unwrap_or_default().to_string();
                    if let Some(a) = gone.rfind('\n') {
                        let tail = trailing.rfind('\n').map_or("", |b| &trailing[b..]);
                        array.set_trailing(format!("{}{tail}", &gone[..a]));
                    }
                }
            }
        }
    }
    Ok(())
}

fn segments(path: &str) -> Vec<String> {
    path.replace('[', ".")
        .replace(']', "")
        .split('.')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn curve_keys(object: &str, path: &str) -> Vec<Key> {
    let mut keys = vec![
        Key::Table("object".to_string()),
        Key::Object(object.to_string()),
        Key::Table("params".to_string()),
    ];
    for segment in segments(path) {
        keys.push(match segment.parse::<usize>() {
            Ok(i) => Key::Index(i),
            Err(_) => Key::Table(segment),
        });
    }
    keys
}

fn pair(value: &Value) -> Option<[f64; 2]> {
    let array = value.as_array()?;
    let mut out = [0.0; 2];
    if array.len() != 2 {
        return None;
    }
    for (slot, v) in out.iter_mut().zip(array.iter()) {
        *slot = v.as_float().or_else(|| v.as_integer().map(|i| i as f64))?;
    }
    out.iter().all(|c| c.is_finite()).then_some(out)
}

fn located(l: &Loaded, object: &str, path: &str) -> Result<(Vec<Key>, Array), Error> {
    let at = format!("object[{object}].params.{}", segments(path).join("."));
    let fail = |text: String| Error::new(at.clone(), None, text);
    let Some(found) = l.recipe.objects.iter().find(|o| o.name == object) else {
        let known: Vec<&str> = l.recipe.objects.iter().map(|o| o.name.as_str()).collect();
        return Err(Error::new(
            format!("object[{object}]"),
            None,
            format!("no such object, expected one of: {}", known.join(", ")),
        ));
    };
    if !matches!(found.kind, Kind::Generator { .. }) {
        return Err(fail(
            "not a generator object; curves are a generator's params".to_string(),
        ));
    }
    if segments(path).is_empty() {
        return Err(fail(
            "expected a param path, as stations.0.curve or silhouettes.side".to_string(),
        ));
    }
    let keys = curve_keys(object, path);
    let Some(Value::Array(array)) = value_at(&l.doc, &keys) else {
        return Err(fail("expected an array of [a, b] points".to_string()));
    };
    if array.iter().any(|v| pair(v).is_none()) {
        return Err(fail("expected an array of [a, b] points".to_string()));
    }
    Ok((keys, array))
}

pub fn curve_points(l: &Loaded, object: &str, path: &str) -> Result<Vec<[f64; 2]>, Error> {
    let (_, array) = located(l, object, path)?;
    Ok(array.iter().filter_map(pair).collect())
}

pub fn curve_patch(l: &Loaded, object: &str, path: &str, edit: CurveEdit) -> Result<Patch, Error> {
    let (keys, mut array) = located(l, object, path)?;
    let at = format!("object[{object}].params.{}", segments(path).join("."));
    let fail = |text: String| Error::new(at.clone(), None, text);
    if let CurveEdit::Set(_, p) | CurveEdit::Add(_, p) = &edit
        && !p.iter().all(|c| c.is_finite())
    {
        return Err(fail("expected finite numbers".to_string()));
    }
    let op = match &edit {
        CurveEdit::Set(i, p) => Op::Set(*i, p),
        CurveEdit::Add(i, p) => Op::Add(*i, p),
        CurveEdit::Remove(i) => {
            if array.len() <= LEAST_POINTS && *i < array.len() {
                return Err(fail(format!(
                    "a curve keeps at least {LEAST_POINTS} points; it has {}",
                    array.len()
                )));
            }
            Op::Remove(*i)
        }
    };
    edit_points(&mut array, op, "curve").map_err(fail)?;
    Ok(Patch {
        path: keys,
        value: Some(Item::Value(Value::Array(array))),
    })
}

fn json_pair(value: &serde_json::Value) -> Option<[f64; 2]> {
    let list = value.as_array()?;
    match list.as_slice() {
        [a, b] => Some([a.as_f64()?, b.as_f64()?]),
        _ => None,
    }
}

fn walk(
    value: &serde_json::Value,
    path: &str,
    parent: Option<&serde_json::Map<String, serde_json::Value>>,
    key: Option<&str>,
    across: Option<Ortho>,
    out: &mut Vec<CurveParam>,
) {
    let join = |next: &str| {
        if path.is_empty() {
            next.to_string()
        } else {
            format!("{path}.{next}")
        }
    };
    match value {
        serde_json::Value::Array(list) => {
            let points: Vec<[f64; 2]> = list.iter().filter_map(json_pair).collect();
            if list.len() >= LEAST_POINTS && points.len() == list.len() {
                let view = parent
                    .and_then(|p| p.get("view"))
                    .and_then(serde_json::Value::as_str)
                    .and_then(Ortho::named)
                    .or_else(|| key.and_then(Ortho::named))
                    .or(across);
                let depth = parent
                    .and_then(|p| p.get("at"))
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0);
                out.push(CurveParam {
                    path: path.to_string(),
                    points,
                    view,
                    depth,
                });
                return;
            }
            for (i, element) in list.iter().enumerate() {
                walk(element, &join(&i.to_string()), None, None, across, out);
            }
        }
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                walk(v, &join(k), Some(map), Some(k), across, out);
            }
        }
        _ => {}
    }
}

fn across(axis: Option<&serde_json::Value>) -> Option<Ortho> {
    let vector = match axis? {
        serde_json::Value::String(word) => match word.trim_start_matches(['+', '-']) {
            "x" | "X" => [1.0, 0.0, 0.0],
            "y" | "Y" => [0.0, 1.0, 0.0],
            _ => [0.0, 0.0, 1.0],
        },
        serde_json::Value::Array(list) => {
            let mut v = [0.0; 3];
            for (slot, c) in v.iter_mut().zip(list) {
                *slot = c.as_f64().unwrap_or(0.0).abs();
            }
            v
        }
        _ => return None,
    };
    if vector[0] > vector[1] && vector[0] > vector[2] {
        Some(Ortho::Side)
    } else if vector[1] > vector[2] {
        Some(Ortho::Top)
    } else {
        None
    }
}

pub fn curves_of(params: &BTreeMap<String, serde_json::Value>) -> Vec<CurveParam> {
    let map: serde_json::Map<String, serde_json::Value> =
        params.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let plane = across(map.get("axis"));
    let mut out = Vec::new();
    for (k, v) in &map {
        walk(v, k, Some(&map), Some(k), plane, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{apply, reread, write};

    const BODY: &str = r#"manfred = 1
name = "hull"

# the hull, traced from the side reference
[[object]]
name = "hull"
kind = "generator"
generator = "manfred/loft"

[object.params]
subdivide = 2
stations = [
  { at = 0.0, curve = [[0.0, 1.2], [0.8, 0.9], [0.0, 0.2]] }, # the bow
  { at = 4.2, curve = [
    [0.0, 1.3], # deck
    [0.9, 1.0],
    [0.0, 0.2],
  ] },
]
silhouettes = { side = [[0.0, 0.5], [2.0, 1.3], [4.4, 0.6]] } # the sheer line

[[reference]]
name = "side"
image = "refs/side.png"
view = "side"
scale = { from = [[100, 400], [900, 400]], metres = 4.4 }
offset = [0.0, 0.3]
opacity = 0.4
"#;

    fn loaded() -> Loaded {
        reread(std::path::Path::new("hull.toml"), BODY).unwrap()
    }

    #[test]
    fn curve_edits_patch_any_param_path_keep_comments_and_undo_to_the_same_bytes() {
        let mut l = loaded();
        let mut undo = Vec::new();
        for (path, edit) in [
            ("stations.1.curve", CurveEdit::Set(1, [0.95, 1.05])),
            ("stations[1].curve", CurveEdit::Add(Some(1), [0.5, 1.25])),
            ("stations.0.curve", CurveEdit::Remove(1)),
            ("silhouettes.side", CurveEdit::Add(None, [4.5, 0.4])),
        ] {
            let patch = curve_patch(&l, "hull", path, edit).unwrap();
            undo.push(apply(&mut l, &patch).unwrap());
        }
        let text = write(&l);
        for kept in [
            "# the hull, traced from the side reference",
            "# the bow",
            "[0.0, 1.3], # deck",
            "# the sheer line",
        ] {
            assert!(text.contains(kept), "{kept:?} lost in\n{text}");
        }
        assert!(
            text.contains("    [0.0, 1.3], # deck\n    [0.5, 1.25],\n    [0.95, 1.05],\n"),
            "{text}"
        );
        assert_eq!(
            curve_points(&l, "hull", "stations.0.curve").unwrap(),
            [[0.0, 1.2], [0.0, 0.2]]
        );
        assert_eq!(
            curve_points(&l, "hull", "silhouettes.side").unwrap().last(),
            Some(&[4.5, 0.4])
        );
        assert_eq!(reread(l.path.as_path(), &text).unwrap().recipe, l.recipe);
        let refused = curve_patch(&l, "hull", "stations.0.curve", CurveEdit::Remove(0));
        assert!(refused.unwrap_err().text.contains("at least 2 points"));
        assert!(curve_patch(&l, "hull", "subdivide", CurveEdit::Remove(0)).is_err());
        assert!(curve_patch(&l, "mast", "stations.0.curve", CurveEdit::Remove(0)).is_err());
        for inverse in undo.into_iter().rev() {
            apply(&mut l, &inverse).unwrap();
        }
        assert_eq!(write(&l), BODY);
    }

    #[test]
    fn a_reference_reads_validates_and_takes_set_and_its_curves_know_their_views() {
        let l = loaded();
        let reference = &l.recipe.references[0];
        assert_eq!(reference.view, Ortho::Side);
        assert_eq!(reference.from, [[100.0, 400.0], [900.0, 400.0]]);
        assert!((reference.metres_per_pixel() - 4.4 / 800.0).abs() < 1e-12);
        let corners = reference.corners([1000, 500]);
        assert!((corners[0][2] - (-100.0 * 4.4 / 800.0)).abs() < 1e-9);
        assert!((corners[0][1] - (0.3 + 400.0 * 4.4 / 800.0)).abs() < 1e-9);
        assert!(crate::validate(&l.recipe).is_empty());
        let set = crate::apply_set(
            &l.recipe,
            &[
                "reference.side.opacity=0.8".to_string(),
                "reference.side.view=top".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(set.references[0].opacity, 0.8);
        assert_eq!(set.references[0].view, Ortho::Top);
        let bad = crate::apply_set(&l.recipe, &["reference.side.opacity=1.5".to_string()]).unwrap();
        assert!(!crate::validate(&bad).is_empty());
        assert!(crate::apply_set(&l.recipe, &["reference.side.view=under".to_string()]).is_err());
        let Kind::Generator { params, .. } = &l.recipe.objects[0].kind else {
            panic!("a generator");
        };
        let curves = curves_of(params);
        let found: Vec<(&str, Option<Ortho>, f64)> = curves
            .iter()
            .map(|c| (c.path.as_str(), c.view, c.depth))
            .collect();
        assert_eq!(
            found,
            [
                ("silhouettes.side", Some(Ortho::Side), 0.0),
                ("stations.0.curve", None, 0.0),
                ("stations.1.curve", None, 4.2),
            ]
        );
        let mut along_x = params.clone();
        along_x.insert("axis".to_string(), serde_json::json!("x"));
        let views: Vec<Option<Ortho>> = curves_of(&along_x).iter().map(|c| c.view).collect();
        assert_eq!(
            views,
            [Some(Ortho::Side), Some(Ortho::Side), Some(Ortho::Side)]
        );
        along_x.insert("axis".to_string(), serde_json::json!([0.0, 1.0, 0.0]));
        assert_eq!(curves_of(&along_x)[1].view, Some(Ortho::Top));
    }
}
