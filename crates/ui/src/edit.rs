use std::path::PathBuf;
use std::sync::Arc;

use manfred_build::{Registry, build_with, object_ids};
use manfred_recipe::toml_edit::Value;
use manfred_recipe::{
    Key, Kind, Loaded, Object, Patch, Plate, apply_all, number, numbers, params_in, path_text,
    reread, save, string, validate, value_at, write,
};

use crate::follow::{Own, Seen};
use crate::undo::{Entry, History};

pub const POCKET_LOCK: &str =
    "a pocket sits in its target's outline; its at, rotate and scale are ignored";

#[derive(Clone, Debug)]
pub struct Gesture {
    pub label: String,
    pub patches: Vec<Patch>,
}

#[derive(Debug, Default)]
pub struct Edits {
    pending: Vec<Gesture>,
}

impl Edits {
    pub fn push(&mut self, label: impl Into<String>, patches: Vec<Patch>) {
        if !patches.is_empty() {
            self.pending.push(Gesture {
                label: label.into(),
                patches,
            });
        }
    }

    pub fn take(&mut self) -> Vec<Gesture> {
        std::mem::take(&mut self.pending)
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Said {
    Patched(String),
    Refused(String),
    Reloaded(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub changed: bool,
    pub refused: bool,
    pub renamed: Vec<(String, String)>,
}

enum Fault {
    Blocked(String),
    Refused(String),
}

#[derive(Debug)]
pub struct Editor {
    path: PathBuf,
    loaded: Option<Loaded>,
    text: Option<String>,
    broken: Option<String>,
    history: History,
    own: Own,
    said: Option<Said>,
    registry: Option<Arc<Registry>>,
}

pub(crate) fn object_path(name: &str, key: &str) -> Vec<Key> {
    vec![
        Key::Table("object".into()),
        Key::Object(name.into()),
        Key::Table(key.into()),
    ]
}

fn line(label: &str, patches: &[Patch]) -> String {
    let Some(first) = patches.first() else {
        return label.to_string();
    };
    let mut text = format!("{label} · {}", path_text(&first.path));
    if patches.len() > 1 {
        text.push_str(&format!(" +{}", patches.len() - 1));
    }
    text
}

fn renames(patches: &[Patch]) -> Vec<(String, String)> {
    patches
        .iter()
        .filter_map(|p| match (p.path.as_slice(), &p.value) {
            ([Key::Table(table), Key::Object(old), Key::Table(key)], Some(value))
                if table == "object" && key == "name" =>
            {
                Some((old.clone(), value.as_str()?.to_string()))
            }
            _ => None,
        })
        .collect()
}

fn problems(l: &Loaded) -> Vec<(String, String)> {
    validate(&l.recipe)
        .into_iter()
        .map(|problem| (problem.path, problem.text))
        .collect()
}

impl Editor {
    pub fn new(path: impl Into<PathBuf>) -> Editor {
        Editor {
            path: path.into(),
            loaded: None,
            text: None,
            broken: None,
            history: History::default(),
            own: Own::default(),
            said: None,
            registry: None,
        }
    }

    pub fn with_registry(mut self, registry: Arc<Registry>) -> Editor {
        self.registry = Some(registry);
        self
    }

    fn id_patches(&self, loaded: &Loaded) -> Vec<Patch> {
        let recipe = &loaded.recipe;
        if recipe.output.scene.is_none() || recipe.objects.iter().all(|o| o.id.is_some()) {
            return Vec::new();
        }
        let base = recipe
            .base
            .clone()
            .or_else(|| self.path.parent().map(PathBuf::from))
            .unwrap_or_default();
        let generated = recipe
            .objects
            .iter()
            .any(|o| o.id.is_none() && matches!(o.kind, Kind::Generator { .. }));
        let built = if generated {
            let registry = self.registry.clone().unwrap_or_default();
            build_with(recipe, &base, &registry).ok()
        } else {
            None
        };
        object_ids(recipe, built.as_ref(), &base, None)
            .unwrap_or_default()
            .into_iter()
            .map(|(name, id)| Patch {
                path: object_path(&name, "id"),
                value: Some(string(&id)),
            })
            .collect()
    }

    pub fn loaded(&self) -> Option<&Loaded> {
        self.loaded.as_ref()
    }

    pub fn said(&self) -> Option<&Said> {
        self.said.as_ref()
    }

    pub fn steps(&self) -> (usize, usize) {
        (self.history.undo_len(), self.history.redo_len())
    }

    pub fn refuse(&mut self, why: String) {
        self.said = Some(Said::Refused(why));
    }

    pub fn seen(&mut self, source: u64) -> Seen {
        let seen = self.own.seen(source);
        if seen == Seen::Outside {
            self.fresh();
        }
        seen
    }

    pub fn fresh(&mut self) -> bool {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) => {
                self.broken = Some(format!("cannot read the recipe: {error}"));
                return false;
            }
        };
        if self.text.as_deref() == Some(text.as_str()) && self.broken.is_none() {
            return false;
        }
        let first = self.loaded.is_none();
        self.own.read(&text);
        let read = reread(&self.path, &text);
        self.text = Some(text);
        match read {
            Ok(loaded) => {
                self.broken = None;
                let dropped = self.history.prune(&loaded.doc);
                self.loaded = Some(loaded);
                if !first {
                    let steps = match dropped {
                        0 => "every undo step still applies".to_string(),
                        1 => "1 undo step no longer applies and is dropped".to_string(),
                        n => format!("{n} undo steps no longer apply and are dropped"),
                    };
                    self.said = Some(Said::Reloaded(format!(
                        "the recipe changed outside the window; {steps}"
                    )));
                }
                true
            }
            Err(error) => {
                self.broken = Some(error.to_string());
                false
            }
        }
    }

    fn commit(&mut self, patches: &[Patch], ids: bool) -> Result<(Vec<Patch>, Vec<Patch>), Fault> {
        self.fresh();
        if let Some(broken) = &self.broken {
            return Err(Fault::Blocked(format!(
                "the recipe on disk does not read, so nothing is written until it does: {broken}"
            )));
        }
        let Some(loaded) = &self.loaded else {
            return Err(Fault::Blocked("no recipe is open".into()));
        };
        for p in patches {
            if let Some(value) = value_at(&loaded.doc, &p.path)
                && let Some(reference) = params_in(&value)
            {
                return Err(Fault::Refused(format!(
                    "{} comes from {reference}; change it in the params file",
                    path_text(&p.path)
                )));
            }
        }
        let before = problems(loaded);
        let mut applied = if ids {
            self.id_patches(loaded)
        } else {
            Vec::new()
        };
        applied.extend(patches.iter().cloned());
        let mut next = loaded.clone();
        let inverses =
            apply_all(&mut next, &applied).map_err(|error| Fault::Refused(error.to_string()))?;
        if let Some((path, text)) = problems(&next)
            .into_iter()
            .find(|problem| !before.contains(problem))
        {
            return Err(Fault::Refused(format!("{path}: {text}")));
        }
        save(&next).map_err(|error| Fault::Refused(error.to_string()))?;
        let text = write(&next);
        self.own.wrote(&text);
        self.text = Some(text);
        self.loaded = Some(next);
        Ok((inverses, applied))
    }

    pub fn gesture(&mut self, gesture: &Gesture) -> Outcome {
        match self.commit(&gesture.patches, true) {
            Ok((undo, redo)) => {
                self.history.done(Entry {
                    label: gesture.label.clone(),
                    undo,
                    redo,
                });
                self.said = Some(Said::Patched(line(&gesture.label, &gesture.patches)));
                Outcome {
                    changed: true,
                    refused: false,
                    renamed: renames(&gesture.patches),
                }
            }
            Err(Fault::Blocked(why) | Fault::Refused(why)) => self.refused(&gesture.label, why),
        }
    }

    fn refused(&mut self, label: &str, why: String) -> Outcome {
        let reloaded = matches!(self.said, Some(Said::Reloaded(_)));
        self.said = Some(Said::Refused(format!("{label}: {why}")));
        Outcome {
            changed: reloaded,
            refused: true,
            renamed: Vec::new(),
        }
    }

    pub fn undo(&mut self) -> Outcome {
        self.replay(true)
    }

    pub fn redo(&mut self) -> Outcome {
        self.replay(false)
    }

    fn replay(&mut self, back: bool) -> Outcome {
        let word = if back { "undo" } else { "redo" };
        let Some(entry) = self.history.pop(back) else {
            self.said = Some(Said::Patched(format!("nothing to {word}")));
            return Outcome::default();
        };
        let label = format!("{word} {}", entry.label);
        let patches = if back { &entry.undo } else { &entry.redo };
        match self.commit(patches, false) {
            Ok((inverse, _)) => {
                self.said = Some(Said::Patched(line(&label, patches)));
                let renamed = renames(patches);
                let entry = if back {
                    Entry {
                        redo: inverse,
                        ..entry
                    }
                } else {
                    Entry {
                        undo: inverse,
                        ..entry
                    }
                };
                self.history.push(!back, entry);
                Outcome {
                    changed: true,
                    refused: false,
                    renamed,
                }
            }
            Err(Fault::Blocked(why)) => {
                self.history.push(back, entry);
                self.refused(&label, why)
            }
            Err(Fault::Refused(why)) => self.refused(&label, format!("{why}; the step is dropped")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Triple {
    Location,
    Rotation,
    Scale,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Smoothing {
    Flat,
    Smooth,
    Angle(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Slots {
    One(Option<String>),
    Many(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fields {
    pub name: String,
    pub place: &'static str,
    pub location: [f64; 3],
    pub rotation: [f64; 3],
    pub scale: [f64; 3],
    pub params: [[Option<String>; 3]; 3],
    pub locked: Option<&'static str>,
    pub material: Slots,
    pub smoothing: Smoothing,
    pub angle_params: Option<String>,
}

impl Fields {
    pub fn values(&self, triple: Triple) -> [f64; 3] {
        match triple {
            Triple::Location => self.location,
            Triple::Rotation => self.rotation,
            Triple::Scale => self.scale,
        }
    }

    pub fn key(&self, triple: Triple) -> &'static str {
        match triple {
            Triple::Location => self.place,
            Triple::Rotation => "rotate",
            Triple::Scale => "scale",
        }
    }

    pub fn unit(&self, triple: Triple, axis: usize) -> &'static str {
        match (triple, self.place, axis) {
            (Triple::Location, "at_px", 0 | 1) => "px",
            (Triple::Location, _, _) => "m",
            (Triple::Rotation, _, _) => "°",
            (Triple::Scale, _, _) => "",
        }
    }

    pub fn decimals(&self, triple: Triple, axis: usize) -> usize {
        match self.unit(triple, axis) {
            "px" | "°" => 2,
            _ => 4,
        }
    }
}

fn object<'a>(l: &'a Loaded, name: &str) -> Result<&'a Object, String> {
    l.recipe
        .objects
        .iter()
        .find(|object| object.name == name)
        .ok_or_else(|| format!("no object named `{name}`"))
}

fn to_pixels(plate: Option<&Plate>, at: [f64; 3]) -> [f64; 3] {
    let Some(plate) = plate.filter(|plate| plate.metres_per_pixel > 0.0) else {
        return at;
    };
    let fine = |value: f64| (value * 1e6).round() / 1e6;
    let [x, y] = plate.to_pixels([at[0], at[1]]);
    [fine(x), fine(y), at[2]]
}

pub(crate) fn round(value: f64, decimals: usize) -> f64 {
    let scale = 10f64.powi(decimals as i32);
    let rounded = (value * scale).round() / scale;
    if rounded == 0.0 { 0.0 } else { rounded }
}

fn params_of(l: &Loaded, name: &str, key: &str) -> [Option<String>; 3] {
    match value_at(&l.doc, &object_path(name, key)) {
        Some(Value::Array(array)) => {
            std::array::from_fn(|axis| array.get(axis).and_then(params_in))
        }
        Some(value) => {
            let reference = params_in(&value);
            std::array::from_fn(|_| reference.clone())
        }
        None => Default::default(),
    }
}

pub fn fields(l: &Loaded, name: &str) -> Option<Fields> {
    let object = object(l, name).ok()?;
    let has = |key: &str| value_at(&l.doc, &object_path(name, key)).is_some();
    let place = if has("at_px") || (!has("at") && l.recipe.plate.is_some()) {
        "at_px"
    } else {
        "at"
    };
    let location = if place == "at_px" {
        to_pixels(l.recipe.plate.as_ref(), object.at)
    } else {
        object.at
    };
    let material = if object.materials.is_empty() {
        Slots::One(object.material.clone())
    } else {
        Slots::Many(object.materials.clone())
    };
    let smoothing = match (object.smooth_angle, object.smooth) {
        (Some(angle), _) => Smoothing::Angle(angle),
        (None, true) => Smoothing::Smooth,
        (None, false) => Smoothing::Flat,
    };
    Some(Fields {
        name: name.to_string(),
        place,
        location,
        rotation: object.rotate,
        scale: object.scale,
        params: [
            params_of(l, name, place),
            params_of(l, name, "rotate"),
            params_of(l, name, "scale"),
        ],
        locked: matches!(object.kind, Kind::Pocket { .. }).then_some(POCKET_LOCK),
        material,
        smoothing,
        angle_params: params_of(l, name, "smooth_angle")[0].clone(),
    })
}

pub fn set_triple(
    l: &Loaded,
    name: &str,
    triple: Triple,
    axis: usize,
    value: f64,
) -> Result<Vec<Patch>, String> {
    let fields = fields(l, name).ok_or_else(|| format!("no object named `{name}`"))?;
    let key = fields.key(triple);
    let index = triple as usize;
    if let Some(reason) = fields.locked {
        return Err(format!("{}: {reason}", path_text(&object_path(name, key))));
    }
    if let Some(reference) = &fields.params[index][axis] {
        return Err(format!(
            "{}[{axis}] comes from {reference}; change it in the params file",
            path_text(&object_path(name, key))
        ));
    }
    let value = round(value, fields.decimals(triple, axis));
    let mut full = fields.values(triple);
    if (full[axis] - value).abs() < 1e-9 {
        return Ok(Vec::new());
    }
    full[axis] = value;
    let path = object_path(name, key);
    let whole = |full: [f64; 3]| Patch {
        path: path.clone(),
        value: Some(numbers(&full)),
    };
    let patch = match value_at(&l.doc, &path) {
        Some(Value::Array(array)) if axis <= array.len() => {
            let mut path = path.clone();
            path.push(Key::Index(axis));
            Patch {
                path,
                value: Some(number(value)),
            }
        }
        Some(Value::Array(_) | Value::Float(_) | Value::Integer(_)) | None => whole(full),
        Some(_) => return Err(format!("{} is not a list of numbers", path_text(&path))),
    };
    Ok(vec![patch])
}

pub fn rename(l: &Loaded, old: &str, new: &str) -> Result<Vec<Patch>, String> {
    let new = new.trim();
    if new == old {
        return Ok(Vec::new());
    }
    if new.is_empty() {
        return Err("a name cannot be empty".into());
    }
    object(l, old)?;
    if l.recipe.objects.iter().any(|object| object.name == new) {
        return Err(format!("`{new}` is already the name of another object"));
    }
    let mut patches = vec![Patch {
        path: object_path(old, "name"),
        value: Some(string(new)),
    }];
    for object in &l.recipe.objects {
        let mut point = |key: &str| {
            patches.push(Patch {
                path: object_path(&object.name, key),
                value: Some(string(new)),
            })
        };
        if object.parent.as_deref() == Some(old) {
            point("parent");
        }
        match &object.kind {
            Kind::Pocket { into, .. } if into == old => point("into"),
            Kind::Text { on: Some(on), .. } if on == old => point("on"),
            _ => {}
        }
    }
    for mover in &l.recipe.movers {
        for (index, part) in mover.parts.iter().enumerate() {
            if part == old {
                patches.push(Patch {
                    path: vec![
                        Key::Table("mover".into()),
                        Key::Object(mover.group.clone()),
                        Key::Table("parts".into()),
                        Key::Index(index),
                    ],
                    value: Some(string(new)),
                });
            }
        }
    }
    Ok(patches)
}

pub fn set_material(l: &Loaded, name: &str, material: &str) -> Result<Vec<Patch>, String> {
    let material = material.trim();
    let fields = fields(l, name).ok_or_else(|| format!("no object named `{name}`"))?;
    match &fields.material {
        Slots::Many(_) => Err(format!(
            "`{name}` has several material slots; change `materials` in the recipe"
        )),
        _ if material.is_empty() => Err("a material needs a name".into()),
        Slots::One(Some(now)) if now == material => Ok(Vec::new()),
        Slots::One(_) => Ok(vec![Patch {
            path: object_path(name, "material"),
            value: Some(string(material)),
        }]),
    }
}

pub fn set_smoothing(l: &Loaded, name: &str, smoothing: &Smoothing) -> Result<Vec<Patch>, String> {
    let fields = fields(l, name).ok_or_else(|| format!("no object named `{name}`"))?;
    let has = |key: &str| value_at(&l.doc, &object_path(name, key)).is_some();
    let smoothing = match smoothing {
        Smoothing::Angle(angle) => Smoothing::Angle(round(*angle, 2)),
        other => other.clone(),
    };
    if smoothing == fields.smoothing {
        return Ok(Vec::new());
    }
    let remove = |key: &str| Patch {
        path: object_path(name, key),
        value: None,
    };
    let mut patches = Vec::new();
    match smoothing {
        Smoothing::Flat => {
            if has("smooth_angle") {
                patches.push(remove("smooth_angle"));
            }
            if has("flat_faces") {
                patches.push(remove("flat_faces"));
            }
            if has("smooth") {
                patches.push(remove("smooth"));
            }
        }
        Smoothing::Smooth => {
            if has("smooth_angle") {
                patches.push(remove("smooth_angle"));
            }
            if has("flat_faces") {
                patches.push(remove("flat_faces"));
            }
            patches.push(Patch {
                path: object_path(name, "smooth"),
                value: Some(manfred_recipe::flag(true)),
            });
        }
        Smoothing::Angle(angle) => {
            if let Some(reference) = &fields.angle_params {
                return Err(format!(
                    "{} comes from {reference}; change it in the params file",
                    path_text(&object_path(name, "smooth_angle"))
                ));
            }
            if has("smooth") {
                patches.push(remove("smooth"));
            }
            patches.push(Patch {
                path: object_path(name, "smooth_angle"),
                value: Some(number(angle)),
            });
        }
    }
    Ok(patches)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lit {
    Sun,
    Sky,
    Light(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dial {
    Position(usize, f64),
    Color(usize, f64),
    Strength(f64),
    Range(f64),
    Rotation(f64),
    Shadow(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LitFields {
    pub lit: Lit,
    pub position: [f64; 3],
    pub color: [f64; 3],
    pub strength: f64,
    pub range: f64,
    pub rotation: f64,
    pub shadow: bool,
}

impl Lit {
    pub fn path(&self, key: &str) -> Vec<Key> {
        match self {
            Lit::Sun => vec![Key::Table("sun".into()), Key::Table(key.into())],
            Lit::Sky => vec![Key::Table("sky".into()), Key::Table(key.into())],
            Lit::Light(name) => vec![
                Key::Table("light".into()),
                Key::Object(name.clone()),
                Key::Table(key.into()),
            ],
        }
    }

    pub fn key(&self, dial: &Dial) -> Option<&'static str> {
        match (self, dial) {
            (Lit::Light(_), Dial::Position(..)) => Some("position"),
            (Lit::Light(_) | Lit::Sun, Dial::Color(..)) => Some("color"),
            (Lit::Light(_) | Lit::Sky, Dial::Strength(_)) => Some("intensity"),
            (Lit::Sun, Dial::Strength(_)) => Some("irradiance"),
            (Lit::Light(_), Dial::Range(_)) => Some("range"),
            (Lit::Sky, Dial::Rotation(_)) => Some("rotation_deg"),
            (Lit::Light(_), Dial::Shadow(_)) => Some("shadow"),
            _ => None,
        }
    }

    pub fn has(&self, dial: &Dial) -> bool {
        self.key(dial).is_some()
    }

    pub fn label(&self) -> String {
        match self {
            Lit::Sun => "the sun".into(),
            Lit::Sky => "the sky".into(),
            Lit::Light(name) => format!("light `{name}`"),
        }
    }
}

impl Dial {
    pub fn decimals(&self) -> usize {
        match self {
            Dial::Position(..) => 4,
            Dial::Rotation(_) => 2,
            Dial::Color(..) | Dial::Strength(_) | Dial::Range(_) | Dial::Shadow(_) => 3,
        }
    }
}

pub fn lit_fields(l: &Loaded, lit: &Lit) -> Option<LitFields> {
    let recipe = &l.recipe;
    let fields = |color, strength| LitFields {
        lit: lit.clone(),
        position: [0.0; 3],
        color,
        strength,
        range: 0.0,
        rotation: 0.0,
        shadow: false,
    };
    match lit {
        Lit::Sun => {
            let sun = recipe.sun.as_ref()?;
            Some(fields(sun.color, sun.irradiance))
        }
        Lit::Sky => {
            let sky = recipe.sky.as_ref()?;
            Some(LitFields {
                rotation: sky.rotation_deg,
                ..fields([1.0; 3], sky.intensity)
            })
        }
        Lit::Light(name) => {
            let light = recipe.lights.iter().find(|light| light.name == *name)?;
            Some(LitFields {
                position: light.position,
                range: light.range,
                shadow: light.shadow,
                ..fields(light.color, light.intensity)
            })
        }
    }
}

pub fn set_lit(l: &Loaded, lit: &Lit, dial: Dial) -> Result<Vec<Patch>, String> {
    let fields =
        lit_fields(l, lit).ok_or_else(|| format!("{} is not in the recipe", lit.label()))?;
    let key = lit
        .key(&dial)
        .ok_or_else(|| format!("{} has no such setting", lit.label()))?;
    let path = lit.path(key);
    let now = value_at(&l.doc, &path);
    if let Some(reference) = now.as_ref().and_then(|value| match (value, dial) {
        (Value::Array(array), Dial::Position(axis, _) | Dial::Color(axis, _)) => {
            array.get(axis).and_then(params_in)
        }
        (value, _) => params_in(value),
    }) {
        return Err(format!(
            "{} comes from {reference}; change it in the params file",
            path_text(&path)
        ));
    }
    let decimals = dial.decimals();
    let whole = |value| {
        Ok(vec![Patch {
            path: path.clone(),
            value: Some(value),
        }])
    };
    match dial {
        Dial::Position(axis, value) | Dial::Color(axis, value) => {
            let colour = matches!(dial, Dial::Color(..));
            if axis > 2 {
                return Err(format!("{} has three values", path_text(&path)));
            }
            let value = round(value, decimals);
            if !value.is_finite() || (colour && value < 0.0) {
                return Err(format!("{}: expected a number from 0 up", path_text(&path)));
            }
            let mut full = if colour {
                fields.color
            } else {
                fields.position
            };
            if (full[axis] - value).abs() < 1e-9 {
                return Ok(Vec::new());
            }
            full[axis] = value;
            match now {
                Some(Value::Array(array)) if axis < array.len() => {
                    let mut path = path.clone();
                    path.push(Key::Index(axis));
                    Ok(vec![Patch {
                        path,
                        value: Some(number(value)),
                    }])
                }
                Some(Value::Array(_)) | None => whole(numbers(&full)),
                Some(_) => Err(format!("{} is not a list of numbers", path_text(&path))),
            }
        }
        Dial::Strength(value) | Dial::Range(value) | Dial::Rotation(value) => {
            let value = round(value, decimals);
            let (current, floor) = match dial {
                Dial::Strength(_) => (fields.strength, true),
                Dial::Range(_) => (fields.range, true),
                _ => (fields.rotation, false),
            };
            if !value.is_finite() || (floor && value < 0.0) {
                return Err(format!("{}: expected a number from 0 up", path_text(&path)));
            }
            if (current - value).abs() < 1e-9 {
                return Ok(Vec::new());
            }
            whole(number(value))
        }
        Dial::Shadow(on) => {
            if on == fields.shadow {
                return Ok(Vec::new());
            }
            whole(manfred_recipe::flag(on))
        }
    }
}

pub fn set_hour(l: &Loaded, hour: f64) -> Result<Vec<Patch>, String> {
    let path = vec![Key::Table("sun".into()), Key::Table("hour".into())];
    let Some(daylight) = &l.recipe.daylight else {
        return Err(if l.recipe.sun.is_some() {
            "the authored sun has no hour of its own; it follows the hour of the shot".into()
        } else {
            "the recipe has no [sun] with model = \"daylight\" to give an hour".into()
        });
    };
    if let Some(reference) = value_at(&l.doc, &path).as_ref().and_then(params_in) {
        return Err(format!(
            "{} comes from {reference}; change it in the params file",
            path_text(&path)
        ));
    }
    let hour = round(hour, 2);
    if !hour.is_finite() || !(0.0..=24.0).contains(&hour) {
        return Err(format!(
            "{}: expected an hour from 0 to 24",
            path_text(&path)
        ));
    }
    if (round(daylight.hour, 2) - hour).abs() < 1e-9 {
        return Ok(Vec::new());
    }
    Ok(vec![Patch {
        path,
        value: Some(number(hour)),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch;
    use pfx_scene::Id;
    use std::path::Path;

    const SCENE: &str = r#"# Claude's table, kept as written
manfred = 1
name = "patchy"
params = "patchy.params.json"

[[object]]
name = "table"            # the top
kind = "box"
size = [1.0, 0.1, 1.0]
at = [0.0, 0.5, 0.0]      # sits on the legs
material = "oak"
smooth_angle = 30.0

[[object]]
name = "cup"
kind = "box"
size = [0.1, 0.1, 0.1]
parent = "table"
rotate = [0.0, 45.0, 0.0]
scale = 2.0
material = "enamel"
smooth = true

[[object]]
name = "lamp"
kind = "sphere"
radius = 0.1
at = ["${params.lamp.x}", 1.2, 0.0]
materials = ["brass", "glass"]

[[object]]
name = "cut"
kind = "pocket"
into = "table"
outline = { rect = [-0.1, -0.1, 0.1, 0.1] }
depth = 0.02

[[object]]
name = "mv_lid"
kind = "box"
size = [0.1, 0.01, 0.1]

[[mover]]
group = "lid"
pivot = [0.0, 0.0, 0.0]
axis = [1.0, 0.0, 0.0]
travel = 0.1
parts = ["mv_lid", "cup"]
"#;

    const PARAMS: &str = r#"{ "lamp": { "x": 0.25 } }"#;

    fn scene(name: &str) -> PathBuf {
        let path = scratch(name);
        std::fs::write(&path, SCENE).unwrap();
        std::fs::write(path.with_file_name("patchy.params.json"), PARAMS).unwrap();
        path
    }

    fn opened(name: &str) -> (Editor, PathBuf) {
        let path = scene(name);
        let mut editor = Editor::new(&path);
        assert!(editor.fresh());
        (editor, path)
    }

    fn on_disk(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn gesture(label: &str, patches: Vec<Patch>) -> Gesture {
        Gesture {
            label: label.into(),
            patches,
        }
    }

    fn loaded(editor: &Editor) -> &Loaded {
        editor.loaded().unwrap()
    }

    fn changed_lines(before: &str, after: &str) -> Vec<String> {
        assert_eq!(before.lines().count(), after.lines().count(), "{after}");
        before
            .lines()
            .zip(after.lines())
            .filter(|(a, b)| a != b)
            .map(|(_, b)| b.to_string())
            .collect()
    }

    #[test]
    fn each_field_edit_and_its_undo_leave_the_file_byte_for_byte() {
        type Make = fn(&Loaded) -> Result<Vec<Patch>, String>;
        let cases: Vec<(&str, Make, Vec<&str>)> = vec![
            (
                "location",
                |l| set_triple(l, "table", Triple::Location, 1, 0.75),
                vec!["at = [0.0, 0.75, 0.0]      # sits on the legs"],
            ),
            (
                "rotation",
                |l| set_triple(l, "cup", Triple::Rotation, 1, 90.0),
                vec!["rotate = [0.0, 90.0, 0.0]"],
            ),
            (
                "scale",
                |l| set_triple(l, "cup", Triple::Scale, 2, 3.0),
                vec!["scale = [2.0, 2.0, 3.0]"],
            ),
            (
                "material",
                |l| set_material(l, "table", "walnut"),
                vec!["material = \"walnut\""],
            ),
            (
                "smoothing angle",
                |l| set_smoothing(l, "table", &Smoothing::Angle(45.0)),
                vec!["smooth_angle = 45.0"],
            ),
        ];
        for (what, make, expected) in cases {
            let (mut editor, path) = opened(&format!("edit-field-{}", what.replace(' ', "-")));
            let patches = make(loaded(&editor)).unwrap();
            let outcome = editor.gesture(&gesture(what, patches));
            assert!(outcome.changed, "{what}: {:?}", editor.said());
            assert_eq!(changed_lines(SCENE, &on_disk(&path)), expected, "{what}");
            assert!(editor.undo().changed, "{what}");
            assert_eq!(on_disk(&path), SCENE, "{what}");
            assert!(editor.redo().changed, "{what}");
            assert_eq!(changed_lines(SCENE, &on_disk(&path)), expected, "{what}");
            assert!(editor.undo().changed, "{what}");
            assert_eq!(on_disk(&path), SCENE, "{what}");
        }
    }

    #[test]
    fn smoothing_modes_swap_their_keys_and_undo_restores_them() {
        let (mut editor, path) = opened("edit-smoothing");
        let flat = set_smoothing(loaded(&editor), "table", &Smoothing::Flat).unwrap();
        editor.gesture(&gesture("smoothing", flat));
        assert!(!on_disk(&path).contains("smooth_angle = 30.0"));
        let smooth = set_smoothing(loaded(&editor), "table", &Smoothing::Smooth).unwrap();
        editor.gesture(&gesture("smoothing", smooth));
        let fields = fields(loaded(&editor), "table").unwrap();
        assert_eq!(fields.smoothing, Smoothing::Smooth);
        let angle = set_smoothing(loaded(&editor), "cup", &Smoothing::Angle(20.0)).unwrap();
        editor.gesture(&gesture("smoothing", angle));
        assert_eq!(fields_of(&editor, "cup").smoothing, Smoothing::Angle(20.0));
        assert!(!on_disk(&path).contains("smooth = true\n\n[[object]]\nname = \"lamp\""));
        for _ in 0..3 {
            assert!(editor.undo().changed);
        }
        assert_eq!(on_disk(&path), SCENE);
    }

    #[test]
    fn leaving_angle_smoothing_drops_flat_faces_with_the_angle() {
        for (name, mode) in [
            ("edit-flat-faces-flat", Smoothing::Flat),
            ("edit-flat-faces-smooth", Smoothing::Smooth),
        ] {
            let path = scene(name);
            let text = on_disk(&path).replace(
                "smooth_angle = 30.0\n",
                "smooth_angle = 30.0\nflat_faces = true\n",
            );
            std::fs::write(&path, &text).unwrap();
            let mut editor = Editor::new(&path);
            assert!(editor.fresh());
            let patches = set_smoothing(loaded(&editor), "table", &mode).unwrap();
            editor.gesture(&gesture("smoothing", patches));
            let written = on_disk(&path);
            assert!(!written.contains("flat_faces"), "{written}");
            assert!(!written.contains("smooth_angle = 30.0"), "{written}");
            assert_eq!(fields_of(&editor, "table").smoothing, mode);
        }
    }

    fn fields_of(editor: &Editor, name: &str) -> Fields {
        fields(loaded(editor), name).unwrap()
    }

    #[test]
    fn a_rename_updates_its_references_in_one_step() {
        let (mut editor, path) = opened("edit-rename");
        let patches = rename(loaded(&editor), "table", "desk").unwrap();
        assert_eq!(patches.len(), 3);
        let outcome = editor.gesture(&gesture("rename", patches));
        assert_eq!(outcome.renamed, [("table".to_string(), "desk".to_string())]);
        let recipe = &loaded(&editor).recipe;
        assert_eq!(recipe.objects[0].name, "desk");
        assert_eq!(recipe.objects[1].parent.as_deref(), Some("desk"));
        let Kind::Pocket { into, .. } = &recipe.objects[3].kind else {
            panic!("a pocket");
        };
        assert_eq!(into, "desk");
        assert!(on_disk(&path).contains("name = \"desk\"            # the top"));
        let patches = rename(loaded(&editor), "cup", "mug").unwrap();
        editor.gesture(&gesture("rename", patches));
        assert_eq!(loaded(&editor).recipe.movers[0].parts, ["mv_lid", "mug"]);
        assert_eq!(
            editor.undo().renamed,
            [("mug".to_string(), "cup".to_string())]
        );
        editor.undo();
        assert_eq!(on_disk(&path), SCENE);
    }

    #[test]
    fn a_rename_refuses_a_name_in_use_or_an_empty_one() {
        let (editor, _) = opened("edit-rename-refused");
        assert_eq!(
            rename(loaded(&editor), "cup", "table").unwrap_err(),
            "`table` is already the name of another object"
        );
        assert_eq!(
            rename(loaded(&editor), "cup", "  ").unwrap_err(),
            "a name cannot be empty"
        );
        assert!(rename(loaded(&editor), "cup", "cup").unwrap().is_empty());
    }

    #[test]
    fn a_refused_gesture_leaves_the_file_untouched() {
        let (mut editor, path) = opened("edit-refused");
        let good = set_triple(loaded(&editor), "table", Triple::Location, 0, 0.5).unwrap();
        let mut patches = good.clone();
        patches.push(Patch {
            path: object_path("cup", "hidden"),
            value: Some(string("yes")),
        });
        let outcome = editor.gesture(&gesture("move", patches));
        assert!(!outcome.changed);
        assert_eq!(on_disk(&path), SCENE);
        let Some(Said::Refused(why)) = editor.said() else {
            panic!("{:?}", editor.said());
        };
        assert!(why.starts_with("move: "), "{why}");
        assert!(why.contains("object[cup].hidden"), "{why}");
        let orphan = vec![Patch {
            path: object_path("cup", "parent"),
            value: Some(string("nobody")),
        }];
        editor.gesture(&gesture("parent", orphan));
        let Some(Said::Refused(why)) = editor.said() else {
            panic!("{:?}", editor.said());
        };
        assert!(why.contains("no object named `nobody`"), "{why}");
        assert_eq!(on_disk(&path), SCENE);
        assert_eq!(editor.steps(), (0, 0));
        editor.gesture(&gesture("move", good));
        assert_ne!(on_disk(&path), SCENE);
        assert_eq!(editor.steps(), (1, 0));
    }

    #[test]
    fn a_params_driven_value_is_not_patched() {
        let (mut editor, path) = opened("edit-params");
        let fields = fields_of(&editor, "lamp");
        assert_eq!(fields.params[0][0].as_deref(), Some("params.lamp.x"));
        assert_eq!(fields.params[0][1], None);
        assert_eq!(fields.location, [0.25, 1.2, 0.0]);
        assert_eq!(
            set_triple(loaded(&editor), "lamp", Triple::Location, 0, 0.3).unwrap_err(),
            "object[lamp].at[0] comes from params.lamp.x; change it in the params file"
        );
        let whole = vec![Patch {
            path: object_path("lamp", "at"),
            value: Some(numbers(&[0.3, 1.2, 0.0])),
        }];
        editor.gesture(&gesture("move", whole));
        assert_eq!(
            editor.said(),
            Some(&Said::Refused(
                "move: object[lamp].at comes from params.lamp.x; change it in the params file"
                    .into()
            ))
        );
        assert_eq!(on_disk(&path), SCENE);
        let next = set_triple(loaded(&editor), "lamp", Triple::Location, 1, 1.5).unwrap();
        editor.gesture(&gesture("move", next));
        assert!(on_disk(&path).contains("at = [\"${params.lamp.x}\", 1.5, 0.0]"));
    }

    #[test]
    fn a_pockets_location_rotation_and_scale_are_not_patched() {
        let (editor, path) = opened("edit-pocket");
        let fields = fields_of(&editor, "cut");
        assert_eq!(fields.locked, Some(POCKET_LOCK));
        assert_eq!(fields_of(&editor, "table").locked, None);
        for (triple, key) in [
            (Triple::Location, "at"),
            (Triple::Rotation, "rotate"),
            (Triple::Scale, "scale"),
        ] {
            assert_eq!(
                set_triple(loaded(&editor), "cut", triple, 0, 0.3).unwrap_err(),
                format!("object[cut].{key}: {POCKET_LOCK}")
            );
        }
        assert_eq!(on_disk(&path), SCENE);
    }

    #[test]
    fn an_outside_write_between_two_edits_keeps_both() {
        let (mut editor, path) = opened("edit-outside");
        let first = set_triple(loaded(&editor), "table", Triple::Location, 0, 0.5).unwrap();
        editor.gesture(&gesture("move", first));
        let first_lid = set_triple(loaded(&editor), "mv_lid", Triple::Location, 0, 0.1).unwrap();
        editor.gesture(&gesture("move", first_lid));
        let outside = on_disk(&path)
            .replace("name = \"mv_lid\"", "name = \"mv_cap\"")
            .replace(
                "parts = [\"mv_lid\", \"cup\"]",
                "parts = [\"mv_cap\", \"cup\"]",
            )
            .replace("material = \"enamel\"", "material = \"tin\"");
        std::fs::write(&path, &outside).unwrap();
        let second = set_triple(loaded(&editor), "cup", Triple::Rotation, 0, 10.0).unwrap();
        let outcome = editor.gesture(&gesture("turn", second));
        assert!(outcome.changed);
        let text = on_disk(&path);
        assert!(text.contains("material = \"tin\""), "{text}");
        assert!(text.contains("name = \"mv_cap\""), "{text}");
        assert!(text.contains("at = [0.5, 0.5, 0.0]"), "{text}");
        assert!(text.contains("rotate = [10.0, 45.0, 0.0]"), "{text}");
        assert_eq!(editor.steps(), (2, 0));
        assert!(editor.undo().changed);
        assert!(on_disk(&path).contains("rotate = [0.0, 45.0, 0.0]"));
        assert!(editor.undo().changed);
        let text = on_disk(&path);
        assert!(text.contains("at = [0.0, 0.5, 0.0]"), "{text}");
        assert!(text.contains("material = \"tin\""), "{text}");
        assert_eq!(editor.steps(), (0, 2));
    }

    #[test]
    fn an_outside_write_is_noted_and_drops_only_the_steps_it_must() {
        let (mut editor, path) = opened("edit-outside-note");
        let lid = set_triple(loaded(&editor), "mv_lid", Triple::Location, 0, 0.1).unwrap();
        editor.gesture(&gesture("move lid", lid));
        let table = set_triple(loaded(&editor), "table", Triple::Location, 0, 0.2).unwrap();
        editor.gesture(&gesture("move table", table));
        let outside = on_disk(&path).replace("name = \"mv_lid\"", "name = \"mv_cap\"");
        std::fs::write(&path, outside.replace("\"mv_lid\", ", "\"mv_cap\", ")).unwrap();
        assert!(editor.fresh());
        assert_eq!(
            editor.said(),
            Some(&Said::Reloaded(
                "the recipe changed outside the window; 1 undo step no longer applies and is dropped"
                    .into()
            ))
        );
        assert_eq!(editor.steps(), (1, 0));
        assert!(!editor.fresh());
        editor.undo();
        assert!(on_disk(&path).contains("at = [0.0, 0.5, 0.0]"));
    }

    #[test]
    fn a_broken_file_on_disk_blocks_writes_until_it_reads() {
        let (mut editor, path) = opened("edit-broken");
        std::fs::write(&path, "manfred = 1\n[[object]\n").unwrap();
        let patches = set_triple(loaded(&editor), "table", Triple::Location, 0, 0.5).unwrap();
        let outcome = editor.gesture(&gesture("move", patches.clone()));
        assert!(!outcome.changed);
        assert_eq!(on_disk(&path), "manfred = 1\n[[object]\n");
        let Some(Said::Refused(why)) = editor.said() else {
            panic!("{:?}", editor.said());
        };
        assert!(why.contains("does not read"), "{why}");
        std::fs::write(&path, SCENE).unwrap();
        assert!(editor.gesture(&gesture("move", patches)).changed);
    }

    #[test]
    fn a_plate_object_moves_in_pixels() {
        let path = scratch("edit-plate");
        let text = "manfred = 1\nname = \"plate\"\n\n[plate]\nwidth = 200\nheight = 100\nmetres_per_pixel = 0.01\norigin = \"top_left\"\n\n[[object]]\nname = \"card\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\nat_px = [120, 30]\n\n[[object]]\nname = \"bare\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n";
        std::fs::write(&path, text).unwrap();
        let mut editor = Editor::new(&path);
        editor.fresh();
        let card = fields_of(&editor, "card");
        assert_eq!(card.place, "at_px");
        assert_eq!(card.location, [120.0, 30.0, 0.0]);
        assert_eq!(card.unit(Triple::Location, 0), "px");
        let x = set_triple(loaded(&editor), "card", Triple::Location, 0, 140.0).unwrap();
        editor.gesture(&gesture("move", x));
        assert!(on_disk(&path).contains("at_px = [140.0, 30]"));
        let z = set_triple(loaded(&editor), "card", Triple::Location, 2, 0.05).unwrap();
        editor.gesture(&gesture("move", z));
        assert!(on_disk(&path).contains("at_px = [140.0, 30, 0.05]"));
        assert_eq!(loaded(&editor).recipe.objects[0].at, [0.4, 0.2, 0.05]);
        let bare = set_triple(loaded(&editor), "bare", Triple::Location, 1, 40.0).unwrap();
        editor.gesture(&gesture("move", bare));
        assert!(on_disk(&path).contains("at_px = [100.0, 40.0, 0.0]"));
        for _ in 0..3 {
            editor.undo();
        }
        assert_eq!(on_disk(&path), text);
    }

    #[test]
    fn nothing_changed_is_no_patch() {
        let (editor, _) = opened("edit-same");
        let l = loaded(&editor);
        assert!(
            set_triple(l, "table", Triple::Location, 1, 0.500_01)
                .unwrap()
                .is_empty()
        );
        assert!(set_material(l, "table", "oak").unwrap().is_empty());
        assert!(
            set_smoothing(l, "table", &Smoothing::Angle(30.0))
                .unwrap()
                .is_empty()
        );
        assert!(set_material(l, "lamp", "steel").is_err());
    }

    const LIT: &str = r#"manfred = 1
name = "lit"
params = "lit.params.json"

[[object]]
name = "floor"
kind = "plane"
size = [4.0, 4.0]

[[light]]
name = "lamp"             # over the desk
position = [0.2, 0.3, 0.1]
color = [1.0, 0.8, 0.6]
intensity = 20.0
range = 3.0
shadow = true

[[light]]
name = "fill"
position = [-1.0, "${params.fill.y}", 0.0]
intensity = 5.0
"#;

    fn lit_scene(name: &str) -> (Editor, PathBuf) {
        let path = scratch(name);
        std::fs::write(&path, LIT).unwrap();
        std::fs::write(
            path.with_file_name("lit.params.json"),
            r#"{ "fill": { "y": 2.0 } }"#,
        )
        .unwrap();
        let mut editor = Editor::new(&path);
        assert!(editor.fresh());
        (editor, path)
    }

    fn lamp(name: &str) -> Lit {
        Lit::Light(name.into())
    }

    #[test]
    fn each_light_edit_and_its_undo_leave_the_file_byte_for_byte() {
        let cases: Vec<(&str, &str, Dial, Vec<&str>)> = vec![
            (
                "position",
                "lamp",
                Dial::Position(1, 0.45),
                vec!["position = [0.2, 0.45, 0.1]"],
            ),
            (
                "colour",
                "lamp",
                Dial::Color(2, 0.5),
                vec!["color = [1.0, 0.8, 0.5]"],
            ),
            (
                "intensity",
                "lamp",
                Dial::Strength(12.5),
                vec!["intensity = 12.5"],
            ),
            ("range", "lamp", Dial::Range(4.0), vec!["range = 4.0"]),
            (
                "shadow",
                "lamp",
                Dial::Shadow(false),
                vec!["shadow = false"],
            ),
            (
                "unparamed axis",
                "fill",
                Dial::Position(0, -0.5),
                vec!["position = [-0.5, \"${params.fill.y}\", 0.0]"],
            ),
        ];
        for (what, name, dial, expected) in cases {
            let (mut editor, path) = lit_scene(&format!("edit-light-{}", what.replace(' ', "-")));
            let patches = set_lit(loaded(&editor), &lamp(name), dial).unwrap();
            let outcome = editor.gesture(&gesture(what, patches));
            assert!(outcome.changed, "{what}: {:?}", editor.said());
            assert_eq!(changed_lines(LIT, &on_disk(&path)), expected, "{what}");
            assert!(editor.undo().changed, "{what}");
            assert_eq!(on_disk(&path), LIT, "{what}");
            assert!(editor.redo().changed, "{what}");
            assert!(editor.undo().changed, "{what}");
            assert_eq!(on_disk(&path), LIT, "{what}");
        }
    }

    #[test]
    fn a_light_key_left_out_is_written_whole_and_undo_takes_it_away() {
        let (mut editor, path) = lit_scene("edit-light-insert");
        let fill = lit_fields(loaded(&editor), &lamp("fill")).unwrap();
        assert_eq!(fill.color, [1.0, 1.0, 1.0]);
        assert_eq!(fill.position, [-1.0, 2.0, 0.0]);
        assert!(!fill.shadow);
        for (what, dial, line) in [
            ("colour", Dial::Color(0, 0.9), "color = [0.9, 1.0, 1.0]"),
            ("range", Dial::Range(2.5), "range = 2.5"),
            ("shadow", Dial::Shadow(true), "shadow = true"),
        ] {
            let patches = set_lit(loaded(&editor), &lamp("fill"), dial).unwrap();
            assert!(editor.gesture(&gesture(what, patches)).changed, "{what}");
            assert!(on_disk(&path).ends_with(&format!("{line}\n")), "{what}");
            assert!(editor.undo().changed, "{what}");
            assert_eq!(on_disk(&path), LIT, "{what}");
        }
        assert_eq!(editor.steps(), (0, 1));
    }

    #[test]
    fn a_light_edit_refuses_params_negatives_and_a_missing_light() {
        let (editor, _) = lit_scene("edit-light-refused");
        let l = loaded(&editor);
        assert_eq!(
            set_lit(l, &lamp("fill"), Dial::Position(1, 1.0)).unwrap_err(),
            "light[fill].position comes from params.fill.y; change it in the params file"
        );
        assert_eq!(
            set_lit(l, &lamp("lamp"), Dial::Strength(-1.0)).unwrap_err(),
            "light[lamp].intensity: expected a number from 0 up"
        );
        assert_eq!(
            set_lit(l, &lamp("lamp"), Dial::Color(0, -0.1)).unwrap_err(),
            "light[lamp].color: expected a number from 0 up"
        );
        assert_eq!(
            set_lit(l, &lamp("ghost"), Dial::Range(1.0)).unwrap_err(),
            "light `ghost` is not in the recipe"
        );
        assert!(
            set_lit(l, &lamp("lamp"), Dial::Strength(20.0004))
                .unwrap()
                .is_empty()
        );
        assert!(
            set_lit(l, &lamp("lamp"), Dial::Shadow(true))
                .unwrap()
                .is_empty()
        );
        assert!(
            set_lit(l, &lamp("lamp"), Dial::Position(1, 0.3))
                .unwrap()
                .is_empty()
        );
    }

    const SKY: &str = r#"manfred = 1
name = "sky"

[sun]
model = "authored"
toward = [0.3, 0.8, 0.5]   # late morning
irradiance = 3.0

[sky]
kind = "analytic"
intensity = 1.5
[sky.shade]
hours = { 6 = [0.6, 0.6, 0.7], 12 = [1.0, 1.0, 1.0] }
"#;

    fn sky_scene(name: &str, text: &str) -> (Editor, PathBuf) {
        let path = scratch(name);
        std::fs::write(&path, text).unwrap();
        let mut editor = Editor::new(&path);
        assert!(editor.fresh());
        (editor, path)
    }

    #[test]
    fn each_sun_and_sky_edit_and_its_undo_leave_the_file_byte_for_byte() {
        let cases: Vec<(&str, Lit, Dial, &str)> = vec![
            (
                "irradiance",
                Lit::Sun,
                Dial::Strength(4.25),
                "irradiance = 4.25\n\n[sky]",
            ),
            (
                "sun colour",
                Lit::Sun,
                Dial::Color(2, 0.9),
                "irradiance = 3.0\ncolor = [1.0, 1.0, 0.9]\n\n[sky]",
            ),
            (
                "sky intensity",
                Lit::Sky,
                Dial::Strength(0.75),
                "kind = \"analytic\"\nintensity = 0.75\n[sky.shade]",
            ),
            (
                "sky rotation",
                Lit::Sky,
                Dial::Rotation(-30.5),
                "intensity = 1.5\nrotation_deg = -30.5\n[sky.shade]",
            ),
        ];
        for (what, lit, dial, expected) in cases {
            let (mut editor, path) =
                sky_scene(&format!("edit-sky-{}", what.replace(' ', "-")), SKY);
            let patches = set_lit(loaded(&editor), &lit, dial).unwrap();
            assert!(
                editor.gesture(&gesture(what, patches)).changed,
                "{what}: {:?}",
                editor.said()
            );
            let text = on_disk(&path);
            assert!(text.contains(expected), "{what}: {text}");
            assert!(
                text.contains("toward = [0.3, 0.8, 0.5]   # late morning"),
                "{what}"
            );
            assert!(
                text.contains("[sun]\nmodel = \"authored\"\ntoward"),
                "{what}: {text}"
            );
            assert!(
                !text.contains("[daylight]") && !text.contains("colour"),
                "{what}"
            );
            assert!(editor.undo().changed, "{what}");
            assert_eq!(on_disk(&path), SKY, "{what}");
            assert!(editor.redo().changed, "{what}");
            assert!(editor.undo().changed, "{what}");
            assert_eq!(on_disk(&path), SKY, "{what}");
        }
        let (editor, _) = sky_scene("edit-sky-read", SKY);
        let sun = lit_fields(loaded(&editor), &Lit::Sun).unwrap();
        assert_eq!((sun.color, sun.strength), ([1.0; 3], 3.0));
        let sky = lit_fields(loaded(&editor), &Lit::Sky).unwrap();
        assert_eq!((sky.strength, sky.rotation), (1.5, 0.0));
    }

    #[test]
    fn a_sky_edit_leaves_the_preparation_table_as_the_scene_file_spells_it() {
        let hdr = SKY.replace(
            "kind = \"analytic\"\nintensity = 1.5\n",
            "kind = \"hdr\"\npath = \"sky.hdr\"\nsource = \"test\"\nintensity = 1.5\n[sky.prepare]\ncap = 60.0\nbalance = [1.0, 0.92, 0.8]\nmean = 0.3\n",
        );
        let (mut editor, path) = sky_scene("edit-sky-prepare", &hdr);
        let patches = set_lit(loaded(&editor), &Lit::Sky, Dial::Strength(2.5)).unwrap();
        assert!(editor.gesture(&gesture("sky intensity", patches)).changed);
        let text = on_disk(&path);
        assert!(text.contains("intensity = 2.5\n[sky.prepare]\ncap = 60.0\nbalance = [1.0, 0.92, 0.8]\nmean = 0.3\n"), "{text}");
        assert!(editor.undo().changed);
        assert_eq!(on_disk(&path), hdr);
    }

    #[test]
    fn the_sun_and_sky_take_only_their_own_keys() {
        let (editor, _) = sky_scene("edit-sky-refused", SKY);
        let l = loaded(&editor);
        assert_eq!(
            set_lit(l, &Lit::Sky, Dial::Color(0, 0.5)).unwrap_err(),
            "the sky has no such setting"
        );
        assert_eq!(
            set_lit(l, &Lit::Sun, Dial::Range(1.0)).unwrap_err(),
            "the sun has no such setting"
        );
        assert_eq!(
            set_lit(l, &Lit::Sun, Dial::Rotation(10.0)).unwrap_err(),
            "the sun has no such setting"
        );
        assert_eq!(
            set_lit(l, &Lit::Sky, Dial::Strength(-0.5)).unwrap_err(),
            "sky.intensity: expected a number from 0 up"
        );
        assert_eq!(
            set_lit(l, &Lit::Sun, Dial::Strength(-1.0)).unwrap_err(),
            "sun.irradiance: expected a number from 0 up"
        );
        assert!(
            set_lit(l, &Lit::Sky, Dial::Rotation(0.004))
                .unwrap()
                .is_empty()
        );
        let (editor, _) = sky_scene(
            "edit-sky-daylight",
            &SKY.replace(
                "[sun]\nmodel = \"authored\"\ntoward = [0.3, 0.8, 0.5]   # late morning\nirradiance = 3.0",
                "[sun]\nmodel = \"daylight\"\nhour = 9.0\nlatitude = 52.0\nheading = 0.0",
            ),
        );
        assert_eq!(lit_fields(loaded(&editor), &Lit::Sun), None);
        assert_eq!(
            set_lit(loaded(&editor), &Lit::Sun, Dial::Strength(2.0)).unwrap_err(),
            "the sun is not in the recipe"
        );
    }

    const DAY: &str = "manfred = 1\nname = \"day\"\n\n[sun]\nmodel = \"daylight\"\nhour = 9.0   # morning\nlatitude = 52.0\nheading = 0.0\n";

    #[test]
    fn the_hour_is_written_to_the_daylight_sun_and_undone() {
        let (mut editor, path) = sky_scene("edit-hour", DAY);
        let patches = set_hour(loaded(&editor), 14.256).unwrap();
        assert_eq!(patches.len(), 1);
        assert!(editor.gesture(&gesture("hour", patches)).changed);
        let text = on_disk(&path);
        assert!(text.contains("hour = 14.26   # morning\n"), "{text}");
        assert_eq!(
            loaded(&editor).recipe.daylight.as_ref().unwrap().hour,
            14.26
        );
        assert!(editor.undo().changed);
        assert_eq!(on_disk(&path), DAY);
        assert!(editor.redo().changed);
        assert!(on_disk(&path).contains("hour = 14.26"));
    }

    #[test]
    fn an_hour_that_is_already_there_writes_nothing() {
        let (editor, _) = sky_scene("edit-hour-same", DAY);
        assert!(set_hour(loaded(&editor), 9.004).unwrap().is_empty());
    }

    #[test]
    fn the_hour_is_refused_without_a_daylight_sun_or_from_params() {
        let (editor, _) = sky_scene("edit-hour-authored", SKY);
        assert_eq!(
            set_hour(loaded(&editor), 10.0).unwrap_err(),
            "the authored sun has no hour of its own; it follows the hour of the shot"
        );
        let none = "manfred = 1\nname = \"none\"\n";
        let (editor, _) = sky_scene("edit-hour-none", none);
        assert_eq!(
            set_hour(loaded(&editor), 10.0).unwrap_err(),
            "the recipe has no [sun] with model = \"daylight\" to give an hour"
        );
        let driven = DAY
            .replace("hour = 9.0", "hour = \"${params.day.hour}\"")
            .replace(
                "name = \"day\"\n",
                "name = \"day\"\nparams = \"day.params.json\"\n",
            );
        let path = scratch("edit-hour-params");
        std::fs::write(&path, &driven).unwrap();
        std::fs::write(
            path.with_file_name("day.params.json"),
            r#"{ "day": { "hour": 9.0 } }"#,
        )
        .unwrap();
        let mut editor = Editor::new(&path);
        assert!(editor.fresh());
        assert_eq!(
            set_hour(loaded(&editor), 10.0).unwrap_err(),
            "sun.hour comes from params.day.hour; change it in the params file"
        );
        let patches = vec![Patch {
            path: vec![Key::Table("sun".into()), Key::Table("hour".into())],
            value: Some(number(10.0)),
        }];
        assert!(editor.gesture(&gesture("hour", patches)).refused);
        assert!(on_disk(&path).contains("${params.day.hour}"));
        assert_eq!(
            set_hour(loaded(&editor), 25.0).unwrap_err(),
            "sun.hour comes from params.day.hour; change it in the params file"
        );
    }

    #[test]
    fn an_empty_queue_takes_nothing() {
        let mut edits = Edits::default();
        edits.push("nothing", Vec::new());
        assert!(edits.is_empty());
        edits.push(
            "move",
            vec![Patch {
                path: object_path("a", "at"),
                value: None,
            }],
        );
        assert!(!edits.is_empty());
        assert_eq!(edits.take().len(), 1);
        assert!(edits.is_empty());
    }

    const OWN: &str = "7k2m9q4xzr";

    fn main_text(scene: bool, include: bool, uses: bool) -> String {
        let mut text = String::from("manfred = 1\nname = \"ids\"\n");
        if include {
            text.push_str("include = [\"rig.toml\"]\n");
        }
        if scene {
            text.push_str("\n[output]\nscene = \"ids.scene.toml\"\nscene_format = 1\n");
        }
        text.push_str(&format!(
            "\n[[object]]\nname = \"top\"          # the slab\nkind = \"box\"\nsize = [1.0, 0.1, 1.0]\nat = [0.0, 0.5, 0.0]\n\n[[object]]\nname = \"ball\"\nid = \"{OWN}\"\nkind = \"sphere\"\nradius = 0.1\n\n[[object]]\nname = \"leg\"\nkind = \"box\"\nsize = [0.1, 0.5, 0.1]\n"
        ));
        if uses {
            text.push_str("\n[[use]]\nfile = \"parts/knob.toml\"\nprefix = \"k_\"\n");
        }
        text
    }

    const RIG: &str =
        "manfred = 1\n\n[[object]]\nname = \"rod\"\nkind = \"box\"\nsize = [0.05, 0.4, 0.05]\n";
    const KNOB: &str =
        "manfred = 1\n\n[[object]]\nname = \"cap\"\nkind = \"box\"\nsize = [0.05, 0.05, 0.05]\n";

    struct Project {
        root: PathBuf,
        recipe: PathBuf,
    }

    fn project(name: &str, main: &str) -> Project {
        let root = scratch(name).parent().unwrap().to_path_buf();
        let recipes = root.join("recipes");
        std::fs::create_dir_all(recipes.join("parts")).unwrap();
        std::fs::write(
            root.join("project.toml"),
            "format = 1\n\n[project]\nname = \"ids\"\n",
        )
        .unwrap();
        std::fs::write(recipes.join("rig.toml"), RIG).unwrap();
        std::fs::write(recipes.join("parts/knob.toml"), KNOB).unwrap();
        let recipe = recipes.join("ids.toml");
        std::fs::write(&recipe, main).unwrap();
        Project { root, recipe }
    }

    fn opened_project(name: &str, main: &str) -> (Editor, Project) {
        let project = project(name, main);
        let mut editor = Editor::new(&project.recipe);
        assert!(editor.fresh());
        (editor, project)
    }

    fn scene_after(editor: &Editor, project: &Project) -> toml_edit::DocumentMut {
        let l = loaded(editor);
        let base = project.root.join("recipes");
        let built = manfred_build::build(&l.recipe, &base).unwrap();
        let out = project.root.join("scenes");
        manfred_build::write(&built, &l.recipe, &base, Some(&out)).unwrap();
        std::fs::read_to_string(out.join("ids.scene.toml"))
            .unwrap()
            .parse()
            .unwrap()
    }

    fn object_ids_of(scene: &toml_edit::DocumentMut) -> Vec<(String, String)> {
        scene["object"]
            .as_array_of_tables()
            .unwrap()
            .iter()
            .map(|t| {
                (
                    t["name"].as_str().unwrap().to_string(),
                    t["id"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    fn mesh_ids_of(scene: &toml_edit::DocumentMut) -> Vec<(String, String)> {
        scene["mesh"]
            .as_table()
            .unwrap()
            .iter()
            .map(|(name, t)| (name.to_string(), t["id"].as_str().unwrap().to_string()))
            .collect()
    }

    fn lift(editor: &mut Editor, name: &str, y: f64) -> Outcome {
        let patches = set_triple(loaded(editor), name, Triple::Location, 1, y).unwrap();
        editor.gesture(&gesture("move", patches))
    }

    #[test]
    fn the_first_save_writes_each_missing_id_and_the_scene_keeps_its_objects_and_ids_across_a_rename()
     {
        let (mut editor, project) = opened_project("ids-first", &main_text(true, false, false));
        let before_text = on_disk(&project.recipe);
        let before = scene_after(&editor, &project);
        assert!(lift(&mut editor, "top", 0.75).changed);
        let after_text = on_disk(&project.recipe);
        assert_eq!(after_text.matches("\nid = ").count(), 3, "{after_text}");
        assert!(after_text.contains(&format!("id = \"{OWN}\"")));
        assert_eq!(after_text.matches(OWN).count(), 1);
        let top = Id::derive(b"recipes/ids.toml\nobject\ntop").to_string();
        let leg = Id::derive(b"recipes/ids.toml\nobject\nleg").to_string();
        assert!(after_text.contains(&format!("name = \"top\"          # the slab\nkind = \"box\"\nsize = [1.0, 0.1, 1.0]\nat = [0.0, 0.75, 0.0]\nid = \"{top}\"\n")), "{after_text}");
        assert!(
            after_text.contains(&format!(
                "name = \"leg\"\nkind = \"box\"\nsize = [0.1, 0.5, 0.1]\nid = \"{leg}\"\n"
            )),
            "{after_text}"
        );
        assert_eq!(
            after_text
                .replace(&format!("id = \"{top}\"\n"), "")
                .replace(&format!("id = \"{leg}\"\n"), ""),
            before_text.replace("0.5, 0.0]", "0.75, 0.0]")
        );

        let saved = scene_after(&editor, &project);
        let was = object_ids_of(&before);
        assert_eq!(object_ids_of(&saved), was);
        assert!(
            was.contains(&("top".to_string(), top.clone()))
                && was.contains(&("leg".to_string(), leg.clone()))
        );
        assert_eq!(saved.to_string().replace("0.75", "0.5"), before.to_string());

        let patches = rename(loaded(&editor), "top", "board").unwrap();
        editor.gesture(&gesture("rename", patches));
        let renamed = scene_after(&editor, &project);
        let ids = object_ids_of(&renamed);
        assert!(ids.contains(&("board".to_string(), top)), "{ids:?}");
        let first = mesh_ids_of(&saved);
        let later = mesh_ids_of(&renamed);
        assert_eq!(
            first.iter().find(|(n, _)| n == "top").unwrap().1,
            later.iter().find(|(n, _)| n == "board").unwrap().1
        );
    }

    #[test]
    fn a_rename_that_is_the_first_save_keeps_the_id_the_old_name_derived() {
        let (mut editor, project) = opened_project("ids-rename", &main_text(true, false, false));
        let before = scene_after(&editor, &project);
        let patches = rename(loaded(&editor), "leg", "post").unwrap();
        editor.gesture(&gesture("rename", patches));
        let after = scene_after(&editor, &project);
        let leg = object_ids_of(&before)
            .into_iter()
            .find(|(n, _)| n == "leg")
            .unwrap()
            .1;
        assert!(object_ids_of(&after).contains(&("post".to_string(), leg)));
    }

    #[test]
    fn an_object_in_an_included_rig_gets_its_id_in_the_rig_file() {
        let (mut editor, project) = opened_project("ids-rig", &main_text(true, true, false));
        let recipes = project.root.join("recipes");
        let main_before = on_disk(&project.recipe);
        lift(&mut editor, "top", 0.75);
        let rig = on_disk(&recipes.join("rig.toml"));
        let rod = Id::derive(b"recipes/ids.toml\nobject\nrod").to_string();
        assert_eq!(rig, format!("{RIG}id = \"{rod}\"\n"));
        let main = on_disk(&project.recipe);
        assert!(!main.contains(&rod));
        assert_eq!(main.matches("\nid = ").count(), 3, "{main}");
        assert_ne!(main, main_before);
        let scene = scene_after(&editor, &project);
        assert!(object_ids_of(&scene).contains(&("rod".to_string(), rod)));
    }

    #[test]
    fn undo_takes_the_ids_away_with_the_edit_and_redo_brings_them_back() {
        let (mut editor, project) = opened_project("ids-undo", &main_text(true, true, false));
        let recipes = project.root.join("recipes");
        let main = on_disk(&project.recipe);
        let rig = on_disk(&recipes.join("rig.toml"));
        lift(&mut editor, "top", 0.75);
        let main_done = on_disk(&project.recipe);
        let rig_done = on_disk(&recipes.join("rig.toml"));
        assert_ne!(rig_done, rig);
        assert_eq!(editor.steps(), (1, 0));
        assert!(editor.undo().changed);
        assert_eq!(on_disk(&project.recipe), main);
        assert_eq!(on_disk(&recipes.join("rig.toml")), rig);
        assert!(editor.redo().changed);
        assert_eq!(on_disk(&project.recipe), main_done);
        assert_eq!(on_disk(&recipes.join("rig.toml")), rig_done);
        assert!(editor.undo().changed);
        assert_eq!(on_disk(&project.recipe), main);
        assert_eq!(on_disk(&recipes.join("rig.toml")), rig);
    }

    #[test]
    fn a_recipe_without_a_scene_gets_no_ids() {
        let (mut editor, project) = opened_project("ids-none", &main_text(false, true, false));
        let before = on_disk(&project.recipe);
        lift(&mut editor, "top", 0.75);
        let after = on_disk(&project.recipe);
        assert_eq!(
            changed_lines(&before, &after),
            vec!["at = [0.0, 0.75, 0.0]".to_string()]
        );
        assert_eq!(on_disk(&project.root.join("recipes/rig.toml")), RIG);
    }

    #[test]
    fn ids_already_there_and_copies_from_a_use_are_left_alone() {
        let (mut editor, project) = opened_project("ids-use", &main_text(true, false, true));
        lift(&mut editor, "top", 0.75);
        let after = on_disk(&project.recipe);
        assert_eq!(after.matches("\nid = ").count(), 3, "{after}");
        assert_eq!(after.matches(OWN).count(), 1);
        assert_eq!(on_disk(&project.root.join("recipes/parts/knob.toml")), KNOB);
        assert!(!after.contains("k_cap"));
        let cap = Id::derive(b"recipes/ids.toml\nobject\nk_cap").to_string();
        assert!(!after.contains(&cap));
        let again = loaded(&editor)
            .recipe
            .objects
            .iter()
            .filter(|o| o.id.is_none())
            .count();
        assert_eq!(again, 1);
        let before = after.clone();
        lift(&mut editor, "top", 0.9);
        assert_eq!(
            changed_lines(&before, &on_disk(&project.recipe)),
            vec!["at = [0.0, 0.9, 0.0]".to_string()]
        );
    }
}
