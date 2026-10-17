use manfred_recipe::toml_edit::{Array, InlineTable, Item, Table, Value};
use manfred_recipe::{Key, Loaded, Patch, Recipe, Rig, value_at};

use crate::edit::round;

pub const TRACKS: [&str; 7] = [
    "distance",
    "lateral",
    "height",
    "bank",
    "lag",
    "shake",
    "look_ahead",
];
pub const LEAST_KEYS: usize = 2;
const DECIMALS: usize = 3;
const APART: f64 = 1e-3;

#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Move { key: usize, at: [f64; 2] },
    Add { t: f64 },
    Remove { key: usize },
    Create,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CurveEdit {
    pub camera: String,
    pub track: String,
    pub change: Change,
}

impl CurveEdit {
    pub fn label(&self) -> String {
        let (camera, track) = (&self.camera, &self.track);
        match &self.change {
            Change::Move { key, .. } => format!("move {camera} {track} key {key}"),
            Change::Add { .. } => format!("add {camera} {track} key"),
            Change::Remove { key } => format!("remove {camera} {track} key {key}"),
            Change::Create => format!("add {camera} {track} track"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Applied {
    pub patch: Patch,
    pub key: Option<usize>,
}

pub fn rigged(recipe: &Recipe) -> Vec<(&str, &Rig)> {
    recipe
        .cameras
        .iter()
        .filter_map(|camera| Some((camera.name.as_str(), camera.rig.as_ref()?)))
        .collect()
}

pub fn keys<'a>(rig: &'a Rig, track: &str) -> Option<&'a [[f64; 2]]> {
    let t = &rig.tracks;
    let keys = match track {
        "distance" => &t.distance,
        "lateral" => &t.lateral,
        "height" => &t.height,
        "bank" => &t.bank,
        "lag" => &t.lag,
        "shake" => &t.shake,
        "look_ahead" => &t.look_ahead,
        _ => return None,
    };
    keys.as_deref()
}

pub fn rest(rig: &Rig, track: &str) -> f64 {
    match track {
        "distance" => -rig.offset[2],
        "lateral" => rig.offset[0],
        "height" => rig.offset[1],
        "bank" => rig.bank,
        "lag" => rig.lag,
        "shake" => 1.0,
        "look_ahead" => rig.look_ahead,
        _ => 0.0,
    }
}

pub fn value(keys: &[[f64; 2]], t: f64) -> f64 {
    manfred_build::track_value(keys, t)
}

pub fn clamped(keys: &[[f64; 2]], key: usize, at: [f64; 2], seconds: f64) -> [f64; 2] {
    let low = if key == 0 {
        0.0
    } else {
        keys[key - 1][0] + APART
    };
    let high = keys
        .get(key + 1)
        .map_or(seconds.max(low), |next| next[0] - APART);
    let t = if high < low {
        low
    } else {
        at[0].clamp(low, high)
    };
    [round(t, DECIMALS), round(at[1], DECIMALS)]
}

fn track_path(camera: &str, track: Option<&str>) -> Vec<Key> {
    let mut path = vec![
        Key::Table("camera".into()),
        Key::Object(camera.into()),
        Key::Table("rig".into()),
        Key::Table("tracks".into()),
    ];
    if let Some(track) = track {
        path.push(Key::Table(track.into()));
    }
    path
}

fn pair(at: [f64; 2]) -> Value {
    let mut array = Array::new();
    array.push(at[0]);
    array.push(at[1]);
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

fn own_prefix(value: &Value) -> String {
    value
        .decor()
        .prefix()
        .and_then(|p| p.as_str())
        .unwrap_or_default()
        .to_string()
}

fn insert(array: &mut Array, i: usize, at: [f64; 2]) {
    let len = array.len();
    let mut v = pair(at);
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

fn remove(array: &mut Array, i: usize) {
    let gone = own_prefix(&array.remove(i));
    if let Some(next) = array.get_mut(i) {
        let own = own_prefix(next);
        let joined = match (gone.rfind('\n'), own.rfind('\n')) {
            (Some(a), Some(b)) => format!("{}{}", &gone[..=a], &own[b + 1..]),
            (Some(a), None) => format!("{}{own}", &gone[..=a]),
            (None, _) if i == 0 => String::new(),
            (None, Some(b)) => own[b + 1..].to_string(),
            (None, None) => own,
        };
        next.decor_mut().set_prefix(joined);
    }
}

fn fresh(rest: f64, seconds: f64) -> Value {
    let mut array = Array::new();
    let v = round(rest, DECIMALS);
    let end = round(seconds.max(APART), DECIMALS);
    array.push(pair([0.0, v]));
    array.push(pair([end, v]));
    for (i, element) in array.iter_mut().enumerate() {
        element
            .decor_mut()
            .set_prefix(if i == 0 { "" } else { " " });
    }
    Value::Array(array)
}

fn rig_item<'a>(l: &'a Loaded, camera: &str) -> Option<&'a Item> {
    l.doc
        .get("camera")?
        .as_array_of_tables()?
        .iter()
        .find(|t| t.get("name").and_then(Item::as_str) == Some(camera))?
        .get("rig")
}

fn created(l: &Loaded, camera: &str, track: &str, keys: Value) -> Result<Patch, String> {
    let rig = rig_item(l, camera).ok_or_else(|| format!("camera[{camera}] has no [rig] table"))?;
    let tracks = match rig {
        Item::Table(t) => t.get("tracks").is_some(),
        Item::Value(Value::InlineTable(t)) => t.get("tracks").is_some(),
        _ => return Err(format!("camera[{camera}].rig is not a table")),
    };
    if tracks {
        return Ok(Patch {
            path: track_path(camera, Some(track)),
            value: Some(Item::Value(keys)),
        });
    }
    let value = match rig {
        Item::Table(_) => {
            let mut table = Table::new();
            table.set_dotted(true);
            table.insert(track, Item::Value(keys));
            Item::Table(table)
        }
        _ => {
            let mut table = InlineTable::new();
            table.insert(track, keys);
            Item::Value(Value::InlineTable(table))
        }
    };
    Ok(Patch {
        path: track_path(camera, None),
        value: Some(value),
    })
}

pub fn apply(l: &Loaded, edit: &CurveEdit) -> Result<Applied, String> {
    let (camera, track) = (edit.camera.as_str(), edit.track.as_str());
    if !TRACKS.contains(&track) {
        return Err(format!(
            "no rig track `{track}`, expected one of: {}",
            TRACKS.join(", ")
        ));
    }
    let rig = l
        .recipe
        .cameras
        .iter()
        .find(|c| c.name == camera)
        .ok_or_else(|| format!("no camera `{camera}`"))?
        .rig
        .as_ref()
        .ok_or_else(|| format!("camera[{camera}] has no [rig] table"))?;
    let seconds = l.recipe.timeline.as_ref().map_or(0.0, |t| t.seconds());
    let present = keys(rig, track);
    if edit.change == Change::Create {
        if present.is_some() {
            return Err(format!(
                "camera[{camera}].rig.tracks.{track} is already keyed"
            ));
        }
        let patch = created(l, camera, track, fresh(rest(rig, track), seconds))?;
        return Ok(Applied { patch, key: None });
    }
    let path = track_path(camera, Some(track));
    let at = format!("camera[{camera}].rig.tracks.{track}");
    let (Some(keys), Some(Value::Array(mut array))) = (present, value_at(&l.doc, &path)) else {
        return Err(format!("{at} has no keys; add the track first"));
    };
    let len = keys.len();
    let missing = |key: usize| format!("{at} has no key {key}, it has {len}");
    let key = match edit.change {
        Change::Move { key, at } => {
            if key >= len {
                return Err(missing(key));
            }
            let next = clamped(keys, key, at, seconds);
            let mut v = pair(next);
            *v.decor_mut() = array
                .get(key)
                .map(|old| old.decor().clone())
                .unwrap_or_default();
            array.replace(key, v);
            Some(key)
        }
        Change::Add { t } => {
            let t = round(t.max(0.0), DECIMALS);
            if keys.iter().any(|k| (k[0] - t).abs() < APART) {
                return Err(format!("{at} already has a key at {t} s"));
            }
            let i = keys.iter().position(|k| k[0] > t).unwrap_or(len);
            insert(&mut array, i, [t, round(value(keys, t), DECIMALS)]);
            Some(i)
        }
        Change::Remove { key } => {
            if key >= len {
                return Err(missing(key));
            }
            if len <= LEAST_KEYS {
                return Err(format!(
                    "{at} keeps at least {LEAST_KEYS} keys; it has {len}"
                ));
            }
            remove(&mut array, key);
            None
        }
        Change::Create => unreachable!(),
    };
    Ok(Applied {
        patch: Patch {
            path,
            value: Some(Item::Value(Value::Array(array))),
        },
        key,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub low: f64,
    pub high: f64,
}

impl Range {
    pub fn of(keys: &[[f64; 2]], seconds: f64) -> Range {
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        let mut take = |v: f64| {
            low = low.min(v);
            high = high.max(v);
        };
        for key in keys {
            take(key[1]);
        }
        let end = seconds.max(keys.last().map_or(0.0, |k| k[0]));
        for step in 0..=64 {
            take(value(keys, end * f64::from(step) / 64.0));
        }
        if !low.is_finite() {
            return Range {
                low: -1.0,
                high: 1.0,
            };
        }
        let span = high - low;
        let pad = if span < 1e-9 {
            (low.abs() * 0.1).max(0.05)
        } else {
            span * 0.15
        };
        Range {
            low: low - pad,
            high: high + pad,
        }
    }

    pub fn along(&self, v: f64) -> f64 {
        (v - self.low) / (self.high - self.low)
    }

    pub fn at(&self, along: f64) -> f64 {
        self.low + along * (self.high - self.low)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub track: String,
    pub key: usize,
    pub start: [f64; 2],
    pub at: [f64; 2],
    pub join: u64,
    pub range: Range,
    pub rows: [f32; 2],
    pub offset: [f32; 2],
}

#[derive(Clone, Debug, Default)]
pub struct Strip {
    pub open: bool,
    pub camera: Option<String>,
    pub selected: Option<(String, usize)>,
    pub drag: Option<Drag>,
    joins: u64,
}

impl Strip {
    pub fn fit(&mut self, recipe: Option<&Recipe>) {
        let rigs = recipe.map(rigged).unwrap_or_default();
        let known = |name: &str| rigs.iter().any(|(camera, _)| *camera == name);
        if !self.camera.as_deref().is_some_and(known) {
            self.camera = rigs.first().map(|(name, _)| name.to_string());
            self.selected = None;
            self.drag = None;
        }
        let rig = rigs
            .iter()
            .find(|(name, _)| Some(*name) == self.camera.as_deref())
            .map(|(_, rig)| *rig);
        let keep = self.selected.as_ref().is_some_and(|(track, key)| {
            rig.and_then(|rig| keys(rig, track))
                .is_some_and(|keys| *key < keys.len())
        });
        if !keep {
            self.selected = None;
        }
    }

    pub fn join(&mut self) -> u64 {
        self.joins += 1;
        self.joins
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{Editor, Gesture, Join};
    use manfred_recipe::{apply as patch, reread, write};

    const CHASE: &str = r#"manfred = 1
name = "chase"

[timeline]
fps = 30
seconds = 4

[[object]]
name = "hero"
kind = "box"
size = [1.0, 1.0, 1.0]

# the chase camera
[[camera]]
name = "chase"
fov_y_deg = 38.0

[camera.rig]
follow = "hero" # the car
offset = [0.0, 1.2, -6.0]
lag = 0.25

[camera.rig.tracks]
distance = [
  [0.0, 6.0], # wide
  [2.0, 4.0],
  [4.0, 5.0], # settle
]

# the post
[[object]]
name = "post"
kind = "box"
size = [0.2, 2.0, 0.2]
"#;

    const BARE: &str = r#"manfred = 1
name = "bare"

[timeline]
fps = 30
seconds = 4

[[object]]
name = "hero"
kind = "box"
size = [1.0, 1.0, 1.0]

[[camera]]
name = "chase"
fov_y_deg = 38.0

[camera.rig]
follow = "hero" # the car
offset = [0.5, 1.2, -6.0]

# the end
"#;

    fn edit(change: Change, track: &str) -> CurveEdit {
        CurveEdit {
            camera: "chase".into(),
            track: track.into(),
            change,
        }
    }

    fn round_trip(text: &str, edit: CurveEdit, expect: &[&str], kept: &[&str]) -> Loaded {
        let mut l = reread(std::path::Path::new("chase.toml"), text).unwrap();
        let applied = apply(&l, &edit).unwrap();
        let inverse = patch(&mut l, &applied.patch).unwrap();
        let written = write(&l);
        for line in expect.iter().chain(kept) {
            assert!(written.contains(line), "{line:?} not in\n{written}");
        }
        let after = l.clone();
        patch(&mut l, &inverse).unwrap();
        assert_eq!(write(&l), text);
        after
    }

    #[test]
    fn curve_gestures_patch_the_track_keep_comments_and_undo_to_the_same_bytes() {
        let kept = [
            "# the chase camera",
            "# the car",
            "# wide",
            "# settle",
            "# the post",
        ];
        round_trip(
            CHASE,
            edit(
                Change::Move {
                    key: 1,
                    at: [2.5, 3.25],
                },
                "distance",
            ),
            &["  [2.5, 3.25],\n  [4.0, 5.0], # settle"],
            &kept,
        );
        let clamped = round_trip(
            CHASE,
            edit(
                Change::Move {
                    key: 1,
                    at: [9.0, 3.0],
                },
                "distance",
            ),
            &["  [3.999, 3.0],"],
            &kept,
        );
        assert_eq!(
            clamped.recipe.cameras[0]
                .rig
                .as_ref()
                .unwrap()
                .tracks
                .distance,
            Some(vec![[0.0, 6.0], [3.999, 3.0], [4.0, 5.0]])
        );
        round_trip(
            CHASE,
            edit(Change::Add { t: 1.0 }, "distance"),
            &["# wide\n  [1.0, 5.063],\n  [2.0, 4.0],"],
            &kept,
        );
        round_trip(
            CHASE,
            edit(Change::Remove { key: 1 }, "distance"),
            &["# wide\n  [4.0, 5.0], # settle"],
            &kept,
        );
        round_trip(
            CHASE,
            edit(Change::Create, "height"),
            &["height = [[0.0, 1.2], [4.0, 1.2]]"],
            &kept,
        );
        let created = round_trip(
            BARE,
            edit(Change::Create, "lateral"),
            &["tracks.lateral = [[0.0, 0.5], [4.0, 0.5]]"],
            &["# the car", "# the end"],
        );
        assert_eq!(
            created.recipe.cameras[0]
                .rig
                .as_ref()
                .unwrap()
                .tracks
                .lateral,
            Some(vec![[0.0, 0.5], [4.0, 0.5]])
        );
        let l = reread(std::path::Path::new("chase.toml"), CHASE).unwrap();
        let mut two = l.clone();
        patch(
            &mut two,
            &apply(&l, &edit(Change::Remove { key: 0 }, "distance"))
                .unwrap()
                .patch,
        )
        .unwrap();
        assert_eq!(
            apply(&two, &edit(Change::Remove { key: 0 }, "distance")).unwrap_err(),
            "camera[chase].rig.tracks.distance keeps at least 2 keys; it has 2"
        );
        assert!(apply(&l, &edit(Change::Add { t: 2.0 }, "distance")).is_err());
        assert!(apply(&l, &edit(Change::Create, "distance")).is_err());
    }

    #[test]
    fn a_key_drag_is_one_undo_step_that_undoes_to_the_same_bytes() {
        let path = crate::scratch("curves-drag");
        std::fs::write(&path, CHASE).unwrap();
        let mut editor = Editor::new(&path);
        assert!(editor.fresh());
        for v in [3.9, 3.5, 3.0] {
            let applied = apply(
                editor.loaded().unwrap(),
                &edit(
                    Change::Move {
                        key: 1,
                        at: [2.0, v],
                    },
                    "distance",
                ),
            )
            .unwrap();
            let outcome = editor.gesture(&Gesture {
                label: "move chase distance key 1".into(),
                patches: vec![applied.patch],
                join: Some(Join::Into(1)),
            });
            assert!(!outcome.refused);
        }
        let dragged = std::fs::read_to_string(&path).unwrap();
        assert!(dragged.contains("  [2.0, 3.0],\n"), "{dragged}");
        assert_eq!(editor.steps(), (1, 0));
        editor.undo();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), CHASE);
    }
}
