use toml_edit::{Array, Item, Value};

use crate::patch::value_at;
use crate::{Error, Key, Loaded, Patch, Recipe, RouteSpec};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointEdit {
    Set(usize, [f64; 3]),
    Add(Option<usize>, [f64; 3]),
    Remove(usize),
}

fn point(p: [f64; 3]) -> Value {
    let mut array = Array::new();
    for c in p {
        array.push(c);
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

fn prefix(array: &Array, at: usize) -> Option<String> {
    array
        .get(at)
        .and_then(|v| v.decor().prefix())
        .and_then(|p| p.as_str())
        .map(str::to_string)
}

impl Recipe {
    pub fn route_named(&self, name: &str) -> Result<&RouteSpec, Error> {
        self.route(name).ok_or_else(|| {
            let known: Vec<&str> = self.routes.iter().map(|r| r.name.as_str()).collect();
            let hint = if known.is_empty() {
                "the recipe has no routes".to_string()
            } else {
                format!("expected one of: {}", known.join(", "))
            };
            Error::new(
                format!("route[{name}]"),
                None,
                format!("no such route, {hint}"),
            )
        })
    }
}

pub fn route_patch(l: &Loaded, name: &str, edit: PointEdit) -> Result<Patch, Error> {
    let at = format!("route[{name}].points");
    let fail = |text: String| Error::new(at.clone(), None, text);
    let route = l.recipe.route_named(name)?;
    let path = vec![
        Key::Table("route".to_string()),
        Key::Object(name.to_string()),
        Key::Table("points".to_string()),
    ];
    let Some(Value::Array(mut array)) = value_at(&l.doc, &path) else {
        return Err(fail("expected an array of [x, y, z] points".to_string()));
    };
    let mut points = route.points.clone();
    let len = points.len();
    let missing = |i: usize| fail(format!("no point {i}, the route has {len}"));
    match edit {
        PointEdit::Set(i, p) => {
            if i >= len {
                return Err(missing(i));
            }
            points[i] = p;
            let mut v = point(p);
            *v.decor_mut() = array
                .get(i)
                .map(|old| old.decor().clone())
                .unwrap_or_default();
            array.replace(i, v);
        }
        PointEdit::Add(i, p) => {
            let i = i.unwrap_or(len);
            if i > len {
                return Err(fail(format!(
                    "no room at {i}, the route has {len} points; give 0 to {len}"
                )));
            }
            points.insert(i, p);
            let mut v = point(p);
            if i < len {
                let was = prefix(&array, i);
                v.decor_mut().set_prefix(was.clone().unwrap_or_default());
                array.insert_formatted(i, v);
                if let Some(next) = array.get_mut(i + 1) {
                    next.decor_mut().set_prefix(plain(was.as_deref(), false));
                }
            } else {
                let like = len.checked_sub(1).and_then(|last| prefix(&array, last));
                v.decor_mut().set_prefix(plain(like.as_deref(), len == 0));
                array.insert_formatted(i, v);
            }
        }
        PointEdit::Remove(i) => {
            if i >= len {
                return Err(missing(i));
            }
            points.remove(i);
            let removed = array.remove(i);
            if i == 0
                && let Some(first) = array.get_mut(0)
            {
                let text = removed
                    .decor()
                    .prefix()
                    .and_then(|p| p.as_str())
                    .map(str::to_string);
                first.decor_mut().set_prefix(plain(text.as_deref(), true));
            }
        }
    }
    manfred_motion::Route::new(&points, route.closed).map_err(fail)?;
    Ok(Patch {
        path,
        value: Some(Item::Value(Value::Array(array))),
    })
}

#[cfg(test)]
mod tests {
    use super::{PointEdit, route_patch};
    use crate::{apply, reread, write};

    const RECIPE: &str = r#"manfred = 1
name = "street"

# the hero's way through town
[[route]]
name = "main"
points = [
  [0.0, 0.0, 0.0], # start at the lights
  [0.0, 0.0, 40.0],
  [12.0, 0.0, 80.0],
] # round the corner
closed = false
"#;

    #[test]
    fn point_edits_keep_the_recipe_s_comments_and_round_trip() {
        let path = std::path::Path::new("street.toml");
        let mut l = reread(path, RECIPE).unwrap();
        for edit in [
            PointEdit::Set(1, [1.0, 0.0, 41.0]),
            PointEdit::Add(Some(1), [0.5, 0.0, 20.0]),
            PointEdit::Add(None, [30.0, 0.0, 100.0]),
            PointEdit::Remove(3),
        ] {
            let patch = route_patch(&l, "main", edit).unwrap();
            apply(&mut l, &patch).unwrap();
        }
        let text = write(&l);
        assert_eq!(
            l.recipe.route("main").unwrap().points,
            [
                [0.0, 0.0, 0.0],
                [0.5, 0.0, 20.0],
                [1.0, 0.0, 41.0],
                [30.0, 0.0, 100.0]
            ]
        );
        for comment in [
            "# the hero's way through town",
            "[0.0, 0.0, 0.0], # start at the lights",
            "# round the corner",
        ] {
            assert!(text.contains(comment), "{text}");
        }
        assert_eq!(reread(path, &text).unwrap().recipe, l.recipe);
        let set = crate::apply_set(
            &l.recipe,
            &["route.main.points=[[0, 0, 0], [0, 0, 9]]".to_string()],
        )
        .unwrap();
        assert_eq!(set.route("main").unwrap().points.len(), 2);
        let two = reread(path, &text).unwrap();
        let two = {
            let mut l = two;
            for _ in 0..2 {
                let patch = route_patch(&l, "main", PointEdit::Remove(0)).unwrap();
                apply(&mut l, &patch).unwrap();
            }
            l
        };
        let e = route_patch(&two, "main", PointEdit::Remove(0)).unwrap_err();
        assert!(e.text.contains("at least 2 points"), "{e}");
        assert!(route_patch(&two, "side", PointEdit::Remove(0)).is_err());
    }
}
