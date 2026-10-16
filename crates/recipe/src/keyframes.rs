use std::collections::BTreeMap;

use toml_edit::{Array, InlineTable, Item, Value};

use crate::patch::{item_exists, names_at, value_at};
use crate::read::effects::{Colour, Setting};
use crate::{Error, Key, LightKind, Loaded, Patch, Projection, Recipe};

pub use manfred_motion::{Channel, Interp};

pub type Keyed = BTreeMap<String, Channel>;

pub const SAME_TIME: f64 = 1e-6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rule {
    Any,
    AtLeast(f64),
    Above(f64),
    Within(f64, f64),
    Open(f64, f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    Light,
    Camera,
    Particles,
    Finish,
    Sun,
    Haze,
}

impl Table {
    pub fn header(self) -> &'static str {
        match self {
            Table::Light => "[[light]]",
            Table::Camera => "[camera]",
            Table::Particles => "[[particles]]",
            Table::Finish => "[finish]",
            Table::Sun => "[sun]",
            Table::Haze => "[haze]",
        }
    }

    pub fn keyed(self) -> &'static [(&'static str, KeyShape)] {
        match self {
            Table::Light => &[
                ("intensity", KeyShape::Number),
                ("color", KeyShape::Triple),
                ("range", KeyShape::Number),
                ("cone", KeyShape::Pair),
            ],
            Table::Camera => &[
                ("fov", KeyShape::Number),
                ("fstop", KeyShape::Number),
                ("focus", KeyShape::Number),
            ],
            Table::Particles => &[
                ("rate", KeyShape::Number),
                ("speed", KeyShape::Pair),
                ("size", KeyShape::Pair),
                ("color", KeyShape::Triple),
                ("alpha", KeyShape::Number),
            ],
            Table::Finish => &[
                ("exposure", KeyShape::Number),
                ("lut_amount", KeyShape::Number),
            ],
            Table::Sun => &[("hour", KeyShape::Number)],
            Table::Haze => &[("amount", KeyShape::Number), ("color", KeyShape::Triple)],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyShape {
    Number,
    Pair,
    Triple,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goes {
    Glb,
    Views,
    Scene(Table, &'static str),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Property {
    pub name: &'static str,
    pub size: usize,
    pub rule: Rule,
    pub goes: Goes,
}

impl Property {
    pub fn dims(&self) -> usize {
        match self.goes {
            Goes::Scene(table, name) => table
                .keyed()
                .iter()
                .find(|(keyed, _)| *keyed == name)
                .map_or(self.size, |(_, shape)| match shape {
                    KeyShape::Number => 1,
                    KeyShape::Pair => 2,
                    KeyShape::Triple => 3,
                }),
            _ => self.size,
        }
    }

    pub fn fits(&self, value: f64) -> bool {
        value.is_finite()
            && match self.rule {
                Rule::Any => true,
                Rule::AtLeast(low) => value >= low,
                Rule::Above(low) => value > low,
                Rule::Within(low, high) => (low..=high).contains(&value),
                Rule::Open(low, high) => value > low && value < high,
            }
    }

    pub fn rule_text(&self) -> String {
        match self.rule {
            Rule::Any => "finite numbers".to_string(),
            Rule::AtLeast(low) => format!("numbers of at least {low}"),
            Rule::Above(low) => format!("numbers above {low}"),
            Rule::Within(low, high) => format!("numbers from {low} to {high}"),
            Rule::Open(low, high) => format!("numbers above {low} and below {high}"),
        }
    }
}

const fn own(name: &'static str, size: usize, rule: Rule, goes: Goes) -> Property {
    Property {
        name,
        size,
        rule,
        goes,
    }
}

const fn sent(
    name: &'static str,
    size: usize,
    rule: Rule,
    table: Table,
    to: &'static str,
) -> Property {
    Property {
        name,
        size,
        rule,
        goes: Goes::Scene(table, to),
    }
}

const ZERO: Rule = Rule::AtLeast(0.0);

const OBJECT: &[Property] = &[
    own("at", 3, Rule::Any, Goes::Glb),
    own("rotate", 3, Rule::Any, Goes::Glb),
    own("scale", 3, Rule::Any, Goes::Glb),
];

const CAMERA: &[Property] = &[
    own("position", 3, Rule::Any, Goes::Glb),
    own("target", 3, Rule::Any, Goes::Glb),
    sent("fov_y_deg", 1, Rule::Open(0.0, 180.0), Table::Camera, "fov"),
    sent("focal_mm", 1, Rule::Above(0.0), Table::Camera, "fov"),
    sent("fstop", 1, Rule::Above(0.0), Table::Camera, "fstop"),
    sent("focus_m", 1, Rule::Above(0.0), Table::Camera, "focus"),
    sent("exposure", 1, ZERO, Table::Finish, "exposure"),
];

const LIGHT: &[Property] = &[
    sent("intensity", 1, ZERO, Table::Light, "intensity"),
    sent("color", 3, ZERO, Table::Light, "color"),
    sent("range", 1, ZERO, Table::Light, "range"),
    sent("cone", 2, Rule::Within(0.0, 180.0), Table::Light, "cone"),
    own("position", 3, Rule::Any, Goes::Views),
    own("at", 3, Rule::Any, Goes::Views),
];

const PARTICLES: &[Property] = &[
    sent("rate", 1, ZERO, Table::Particles, "rate"),
    sent("speed", 2, ZERO, Table::Particles, "speed"),
    sent("size", 2, ZERO, Table::Particles, "size"),
    sent("color", 3, ZERO, Table::Particles, "color"),
    sent(
        "alpha",
        1,
        Rule::Within(0.0, 1.0),
        Table::Particles,
        "alpha",
    ),
];

const LOOK: &[Property] = &[
    sent("exposure", 1, ZERO, Table::Finish, "exposure"),
    sent(
        "lut_amount",
        1,
        Rule::Within(0.0, 1.0),
        Table::Finish,
        "lut_amount",
    ),
];

const SUN: &[Property] = &[sent("hour", 1, Rule::Within(0.0, 24.0), Table::Sun, "hour")];

const HAZE: &[Property] = &[
    sent("amount", 1, Rule::Within(0.0, 1.0), Table::Haze, "amount"),
    sent("color", 3, ZERO, Table::Haze, "color"),
];

pub const SECTIONS: [&str; 7] = [
    "object",
    "camera",
    "light",
    "particles",
    "look",
    "sun",
    "haze",
];

pub fn properties(section: &str) -> &'static [Property] {
    match section {
        "object" => OBJECT,
        "camera" => CAMERA,
        "light" => LIGHT,
        "particles" => PARTICLES,
        "look" => LOOK,
        "sun" => SUN,
        "haze" => HAZE,
        _ => &[],
    }
}

pub fn property(section: &str, name: &str) -> Option<&'static Property> {
    properties(section).iter().find(|p| p.name == name)
}

pub fn named(section: &str) -> bool {
    matches!(section, "object" | "camera" | "light" | "particles")
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Context {
    pub spot: bool,
    pub parented: bool,
    pub ortho: bool,
    pub authored: bool,
}

pub(crate) fn refusal(section: &str, name: &str, cx: Context) -> Option<&'static str> {
    match (section, name) {
        ("light", "cone") if !cx.spot => {
            Some("`cone` belongs to a spot light, expected `kind = \"spot\"` or the key removed")
        }
        ("light", "position") if cx.parented => Some(
            "a light with a `parent` is placed by `at` in the parent's frame, expected `at` keyed in place of `position`",
        ),
        ("light", "at") if !cx.parented => Some(
            "`at` places a light in its parent's frame, expected `position` keyed on a light without a `parent`",
        ),
        ("camera", "fov_y_deg" | "focal_mm" | "fstop" | "focus_m") if cx.ortho => Some(
            "an orthographic camera has no lens to key, expected `position`, `target` or `exposure`",
        ),
        ("sun", "hour") if cx.authored => {
            Some("an authored sun takes no hour, expected `model = \"daylight\"` for a keyed hour")
        }
        _ => None,
    }
}

pub fn entry_of(section: &str, name: Option<&str>) -> String {
    match name {
        Some(name) => format!("{section}.{name}"),
        None => section.to_string(),
    }
}

pub struct Place<'a> {
    pub section: &'a str,
    pub name: Option<&'a str>,
    pub property: &'a str,
}

impl Place<'_> {
    pub fn entry(&self) -> String {
        entry_of(self.section, self.name)
    }

    fn keys_path(&self, key: &str) -> Vec<Key> {
        let mut path = vec![Key::Table(self.section.to_string())];
        if let Some(name) = self.name {
            path.push(Key::Object(name.to_string()));
        }
        path.push(Key::Table(key.to_string()));
        path
    }
}

pub fn place(path: &str) -> Result<Place<'_>, Error> {
    let fail = |text: String| Error::new(path, None, text);
    let (section, rest) = path.split_once('.').ok_or_else(|| {
        fail("expected <section>.<name>.<property>, as object.hero.at or light.lamp.intensity, or look.exposure and sun.hour".to_string())
    })?;
    if !SECTIONS.contains(&section) {
        return Err(fail(format!(
            "`{section}` keys nothing, expected one of: {}",
            SECTIONS.join(", ")
        )));
    }
    let (name, prop) = if named(section) {
        let (name, prop) = rest.rsplit_once('.').ok_or_else(|| {
            fail(format!(
                "expected {section}.<name>.<property>, as {section}.<name>.{}",
                properties(section)[0].name
            ))
        })?;
        if name.is_empty() {
            return Err(fail(format!("expected the {section}'s name")));
        }
        (Some(name), prop)
    } else {
        (None, rest)
    };
    if property(section, prop).is_none() {
        let names: Vec<&str> = properties(section).iter().map(|p| p.name).collect();
        return Err(fail(format!(
            "`{prop}` is not keyable on {section}, expected one of: {}",
            names.join(", ")
        )));
    }
    Ok(Place {
        section,
        name,
        property: prop,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keyable {
    pub path: String,
    pub property: &'static str,
    pub dims: usize,
    pub rest: Vec<f64>,
    pub channel: Option<Channel>,
    pub goes: Goes,
}

#[derive(Clone, Debug, PartialEq)]
pub enum KeyEdit {
    Set {
        t: f64,
        value: Vec<f64>,
    },
    Move {
        index: usize,
        t: f64,
        value: Vec<f64>,
    },
    Remove {
        t: f64,
    },
    RemoveAll,
    Interp(Interp),
}

impl Recipe {
    pub fn channel(&self, path: &str) -> Option<&Channel> {
        let place = place(path).ok()?;
        self.keyed.get(&place.entry())?.get(place.property)
    }

    pub fn scene_camera(&self) -> Option<&crate::Camera> {
        match &self.output.scene_camera {
            Some(name) => self.cameras.iter().find(|c| c.name == *name),
            None => self.cameras.first(),
        }
    }

    fn context(&self, section: &str, name: Option<&str>) -> Result<Context, Error> {
        let missing = |what: &str| {
            Error::new(
                entry_of(section, name),
                None,
                format!("the recipe has no {what}"),
            )
        };
        let name = name.unwrap_or_default();
        let mut cx = Context::default();
        match section {
            "object" => self
                .objects
                .iter()
                .find(|o| o.name == name)
                .map(|_| cx)
                .ok_or_else(|| missing(&format!("object named `{name}`"))),
            "camera" => {
                let camera = self
                    .cameras
                    .iter()
                    .find(|c| c.name == name)
                    .ok_or_else(|| missing(&format!("camera named `{name}`")))?;
                cx.ortho = camera.kind == Projection::Orthographic;
                Ok(cx)
            }
            "light" => {
                let light = self
                    .lights
                    .iter()
                    .find(|l| l.name == name)
                    .ok_or_else(|| missing(&format!("light named `{name}`")))?;
                cx.spot = light.kind == LightKind::Spot;
                cx.parented = light.parent.is_some();
                Ok(cx)
            }
            "particles" => self
                .effects
                .particles
                .iter()
                .find(|p| p.name == name)
                .map(|_| cx)
                .ok_or_else(|| missing(&format!("particles named `{name}`"))),
            "look" => self
                .look
                .as_ref()
                .map(|_| cx)
                .ok_or_else(|| missing("[look]")),
            "sun" => match (&self.sun, &self.daylight) {
                (_, Some(_)) => Ok(cx),
                (Some(_), None) => Ok(Context {
                    authored: true,
                    ..cx
                }),
                _ => Err(missing("[sun]")),
            },
            "haze" => self
                .effects
                .haze
                .as_ref()
                .map(|_| cx)
                .ok_or_else(|| missing("[haze]")),
            _ => Err(missing(section)),
        }
    }

    fn rest(&self, section: &str, name: &str, property: &str) -> Vec<f64> {
        let aspect = self.timeline.map_or(16.0 / 9.0, |t| t.aspect());
        let setting = |settings: &crate::Settings, key: &str| -> Vec<f64> {
            match settings.iter().find(|(k, _)| *k == key).map(|(_, s)| s) {
                Some(Setting::Number(v)) => vec![*v],
                Some(Setting::Numbers(v)) => v.clone(),
                Some(Setting::Colour(Colour::Rgb(c))) => c.to_vec(),
                _ => Vec::new(),
            }
        };
        match section {
            "object" => self
                .objects
                .iter()
                .find(|o| o.name == name)
                .map_or_else(Vec::new, |o| match property {
                    "at" => o.at.to_vec(),
                    "rotate" => o.rotate.to_vec(),
                    _ => o.scale.to_vec(),
                }),
            "camera" => self
                .cameras
                .iter()
                .find(|c| c.name == name)
                .map_or_else(Vec::new, |c| match property {
                    "position" => c.position.to_vec(),
                    "target" => c.target.to_vec(),
                    "fov_y_deg" => vec![c.fov_y_deg(aspect)],
                    "focal_mm" => c.focal_mm.map_or_else(Vec::new, |f| vec![f]),
                    "fstop" => vec![c.fstop],
                    "focus_m" => c.focus_m.map_or_else(Vec::new, |f| vec![f]),
                    _ => vec![self.look.as_ref().map_or(1.0, |l| l.exposure)],
                }),
            "light" => self
                .lights
                .iter()
                .find(|l| l.name == name)
                .map_or_else(Vec::new, |l| match property {
                    "intensity" => vec![l.intensity],
                    "color" => l.color.to_vec(),
                    "range" => vec![l.range],
                    "cone" => l.cone.map_or_else(Vec::new, |c| c.to_vec()),
                    "position" => l.position.to_vec(),
                    _ => l
                        .parent
                        .as_ref()
                        .map_or_else(Vec::new, |(_, at)| at.to_vec()),
                }),
            "particles" => self
                .effects
                .particles
                .iter()
                .find(|p| p.name == name)
                .map_or_else(Vec::new, |p| match property {
                    "alpha" => vec![1.0],
                    other => setting(&p.settings, other),
                }),
            "look" => self
                .look
                .as_ref()
                .map_or_else(Vec::new, |l| match property {
                    "exposure" => vec![l.exposure],
                    _ => vec![l.lut_amount.unwrap_or(1.0)],
                }),
            "sun" => self
                .daylight
                .as_ref()
                .map_or_else(Vec::new, |d| vec![d.hour]),
            "haze" => self
                .effects
                .haze
                .as_ref()
                .map_or_else(Vec::new, |h| setting(h, property)),
            _ => Vec::new(),
        }
    }

    pub fn key_entries(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        out.extend(
            self.objects
                .iter()
                .map(|o| entry_of("object", Some(&o.name))),
        );
        out.extend(
            self.cameras
                .iter()
                .map(|c| entry_of("camera", Some(&c.name))),
        );
        out.extend(self.lights.iter().map(|l| entry_of("light", Some(&l.name))));
        out.extend(
            self.effects
                .particles
                .iter()
                .map(|p| entry_of("particles", Some(&p.name))),
        );
        for (section, present) in [
            ("look", self.look.is_some()),
            ("sun", self.sun.is_some() || self.daylight.is_some()),
            ("haze", self.effects.haze.is_some()),
        ] {
            if present {
                out.push(section.to_string());
            }
        }
        out
    }

    pub fn keyable(&self, entry: &str) -> Result<Vec<Keyable>, Error> {
        let (section, name) = match entry.split_once('.') {
            Some((section, name)) => (section, Some(name)),
            None => (entry, None),
        };
        if !SECTIONS.contains(&section) || named(section) != name.is_some() {
            return Err(Error::new(
                entry,
                None,
                "expected an entry as object.<name>, camera.<name>, light.<name>, particles.<name>, look, sun or haze",
            ));
        }
        let cx = self.context(section, name)?;
        let keyed = self.keyed.get(entry);
        Ok(properties(section)
            .iter()
            .filter(|p| refusal(section, p.name, cx).is_none())
            .map(|p| Keyable {
                path: format!("{entry}.{}", p.name),
                property: p.name,
                dims: p.dims(),
                rest: self.rest(section, name.unwrap_or_default(), p.name),
                channel: keyed.and_then(|k| k.get(p.name)).cloned(),
                goes: p.goes,
            })
            .collect())
    }

    pub fn lights_at(&self, time: f64) -> Vec<crate::Light> {
        let mut lights = self.lights.clone();
        for light in &mut lights {
            let Some(keyed) = self.keyed.get(&entry_of("light", Some(&light.name))) else {
                continue;
            };
            let at = |property: &str| keyed.get(property).map(|c| c.at(time));
            if let Some(v) = at("intensity") {
                light.intensity = first(&v);
            }
            if let Some(v) = at("color") {
                light.color = triple(&v);
            }
            if let Some(v) = at("range") {
                light.range = first(&v);
            }
            if let Some(v) = at("cone") {
                light.cone = Some([0, 1].map(|i| v.get(i).copied().unwrap_or(0.0)));
            }
            if let Some(v) = at("position") {
                light.position = triple(&v);
            }
            if let (Some(v), Some((_, place))) = (at("at"), light.parent.as_mut()) {
                *place = triple(&v);
            }
        }
        lights
    }

    pub fn keyed_at(&self, time: f64) -> Recipe {
        let mut r = self.clone();
        if self.keyed.is_empty() {
            return r;
        }
        let at = |entry: &str, property: &str| {
            self.keyed
                .get(entry)
                .and_then(|k| k.get(property))
                .map(|c| c.at(time))
        };
        let three = |v: Vec<f64>| triple(&v);
        let one = |v: Vec<f64>| first(&v);
        let scene_camera = self.scene_camera().map(|c| c.name.clone());
        for camera in &mut r.cameras {
            let entry = entry_of("camera", Some(&camera.name));
            if let Some(v) = at(&entry, "position") {
                camera.position = three(v);
            }
            if let Some(v) = at(&entry, "target") {
                camera.target = three(v);
            }
            if let Some(v) = at(&entry, "fov_y_deg") {
                camera.fov_y_deg = Some(one(v));
            }
            if let Some(v) = at(&entry, "focal_mm") {
                camera.fov_y_deg = None;
                camera.focal_mm = Some(one(v));
            }
            if let Some(v) = at(&entry, "fstop") {
                camera.fstop = one(v);
            }
            if let Some(v) = at(&entry, "focus_m") {
                camera.focus_m = Some(one(v));
            }
            if scene_camera.as_deref() == Some(camera.name.as_str())
                && let (Some(v), Some(look)) = (at(&entry, "exposure"), r.look.as_mut())
            {
                look.exposure = one(v);
            }
        }
        r.lights = self.lights_at(time);
        if let Some(look) = r.look.as_mut() {
            if let Some(v) = at("look", "exposure") {
                look.exposure = one(v);
            }
            if let Some(v) = at("look", "lut_amount") {
                look.lut_amount = Some(one(v));
            }
        }
        if let (Some(daylight), Some(v)) = (r.daylight.as_mut(), at("sun", "hour")) {
            daylight.hour = one(v);
        }
        if let Some(haze) = r.effects.haze.as_mut() {
            for (key, setting) in haze.iter_mut() {
                match (*key, at("haze", key)) {
                    ("amount", Some(v)) => *setting = Setting::Number(one(v)),
                    ("color", Some(v)) => *setting = Setting::Colour(Colour::Rgb(three(v))),
                    _ => {}
                }
            }
        }
        r
    }
}

fn first(v: &[f64]) -> f64 {
    v.first().copied().unwrap_or(0.0)
}

fn triple(v: &[f64]) -> [f64; 3] {
    [0, 1, 2].map(|i| v.get(i).copied().unwrap_or(0.0))
}

fn pair(t: f64, value: &[f64]) -> Value {
    let mut array = Array::new();
    array.push(t);
    match value {
        [one] => array.push(*one),
        many => {
            let mut inner = Array::new();
            for v in many {
                inner.push(*v);
            }
            array.push(Value::Array(inner));
        }
    }
    Value::Array(array)
}

fn fresh(keys: &[(f64, Vec<f64>)]) -> Value {
    let mut array = Array::new();
    for (i, (t, value)) in keys.iter().enumerate() {
        let mut v = pair(*t, value);
        v.decor_mut().set_prefix(if i == 0 { "" } else { " " });
        array.push_formatted(v);
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

fn own_prefix(value: &Value) -> String {
    value
        .decor()
        .prefix()
        .and_then(|p| p.as_str())
        .unwrap_or_default()
        .to_string()
}

fn insert(array: &mut Array, i: usize, mut v: Value) {
    let len = array.len();
    if i < len {
        let was = array.get(i).map(own_prefix);
        v.decor_mut().set_prefix(was.clone().unwrap_or_default());
        array.insert_formatted(i, v);
        if let Some(next) = array.get_mut(i + 1) {
            next.decor_mut().set_prefix(plain(was.as_deref(), false));
        }
    } else {
        let like = len
            .checked_sub(1)
            .and_then(|last| array.get(last).map(own_prefix));
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

fn replace(array: &mut Array, i: usize, mut v: Value) {
    if let Some(old) = array.get(i) {
        *v.decor_mut() = old.decor().clone();
    }
    array.replace(i, v);
}

fn dropped(l: &Loaded, place: &Place<'_>, key: &str) -> Option<Patch> {
    let table = place.keys_path(key);
    if !item_exists(&l.doc, &table) {
        return None;
    }
    let names = names_at(&l.doc, &table);
    if !names.iter().any(|n| n == place.property) {
        return None;
    }
    let mut path = table;
    if names.len() > 1 {
        path.push(Key::Table(place.property.to_string()));
    }
    Some(Patch { path, value: None })
}

pub fn key_patches(l: &Loaded, path: &str, edit: &KeyEdit) -> Result<Vec<Patch>, Error> {
    let place = place(path)?;
    let fail = |text: String| Error::new(path, None, text);
    let cx = l.recipe.context(place.section, place.name)?;
    if let Some(text) = refusal(place.section, place.property, cx) {
        return Err(fail(text.to_string()));
    }
    let Some(spec) = property(place.section, place.property) else {
        return Err(fail("not keyable".to_string()));
    };
    if l.recipe.timeline.is_none() {
        return Err(fail(
            "keys animate over the recipe's [timeline], and the recipe has none (docs/recipe.md, Timeline)"
                .to_string(),
        ));
    }
    let dims = spec.dims();
    let checked = |t: f64, value: &[f64]| -> Result<(), Error> {
        if !(t.is_finite() && t >= 0.0) {
            return Err(fail(format!("the time is seconds of at least 0, not {t}")));
        }
        if value.len() != dims {
            return Err(fail(format!(
                "{} takes {} a key, found {}",
                place.property,
                match dims {
                    1 => "a number".to_string(),
                    n => format!("{n} numbers"),
                },
                value.len()
            )));
        }
        if let Some(bad) = value.iter().find(|v| !spec.fits(**v)) {
            return Err(fail(format!(
                "{} takes {}, not {bad}",
                place.property,
                spec.rule_text()
            )));
        }
        Ok(())
    };
    let entry = place.entry();
    let channel = l
        .recipe
        .keyed
        .get(&entry)
        .and_then(|k| k.get(place.property));
    let keys: Vec<(f64, Vec<f64>)> = channel.map_or_else(Vec::new, |c| c.keys.clone());
    let keys_path = place.keys_path("keys");
    let mut property_path = keys_path.clone();
    property_path.push(Key::Table(place.property.to_string()));
    let unkeyed = || fail(format!("{} has no keys", place.property));
    let mut patches = Vec::new();
    match edit {
        KeyEdit::Interp(interp) => {
            if channel.is_none() {
                return Err(unkeyed());
            }
            match interp {
                Interp::Linear => patches.extend(dropped(l, &place, "interp")),
                other => {
                    let interp_path = place.keys_path("interp");
                    let value = Value::from(other.name());
                    if item_exists(&l.doc, &interp_path) {
                        let mut path = interp_path;
                        path.push(Key::Table(place.property.to_string()));
                        patches.push(Patch {
                            path,
                            value: Some(Item::Value(value)),
                        });
                    } else {
                        let mut table = InlineTable::new();
                        table.insert(place.property, value);
                        patches.push(Patch {
                            path: interp_path,
                            value: Some(Item::Value(Value::InlineTable(table))),
                        });
                    }
                }
            }
            return Ok(patches);
        }
        KeyEdit::RemoveAll => {
            if channel.is_none() {
                return Err(unkeyed());
            }
            patches.extend(dropped(l, &place, "keys"));
            patches.extend(dropped(l, &place, "interp"));
            return Ok(patches);
        }
        KeyEdit::Remove { t } => {
            let Some(i) = keys.iter().position(|k| (k.0 - t).abs() < SAME_TIME) else {
                return match channel {
                    None => Err(unkeyed()),
                    Some(_) => Err(fail(format!(
                        "{} has no key at {t} s; its keys are at {}",
                        place.property,
                        keys.iter()
                            .map(|k| format!("{}", k.0))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))),
                };
            };
            if keys.len() == 1 {
                patches.extend(dropped(l, &place, "keys"));
                patches.extend(dropped(l, &place, "interp"));
                return Ok(patches);
            }
            let Some(Value::Array(mut array)) = value_at(&l.doc, &property_path) else {
                return Err(fail("expected an array of [t, value] keys".to_string()));
            };
            remove(&mut array, i);
            patches.push(Patch {
                path: property_path,
                value: Some(Item::Value(Value::Array(array))),
            });
            return Ok(patches);
        }
        KeyEdit::Set { t, value } | KeyEdit::Move { t, value, .. } => {
            checked(*t, value)?;
            let moved = match edit {
                KeyEdit::Move { index, .. } => {
                    if *index >= keys.len() {
                        return Err(fail(format!(
                            "{} has no key {index}, it has {}",
                            place.property,
                            keys.len()
                        )));
                    }
                    Some(*index)
                }
                _ => None,
            };
            let same = keys.iter().position(|k| (k.0 - t).abs() < SAME_TIME);
            if let (Some(from), Some(other)) = (moved, same)
                && from != other
            {
                return Err(fail(format!(
                    "{} already has a key at {t} s",
                    place.property
                )));
            }
            let mut array = match value_at(&l.doc, &property_path) {
                Some(Value::Array(array)) => Some(array),
                Some(_) => return Err(fail("expected an array of [t, value] keys".to_string())),
                None => None,
            };
            let Some(array) = array.as_mut() else {
                let fresh = fresh(&[(*t, value.clone())]);
                if item_exists(&l.doc, &keys_path) {
                    patches.push(Patch {
                        path: property_path,
                        value: Some(Item::Value(fresh)),
                    });
                } else {
                    let mut table = InlineTable::new();
                    table.insert(place.property, fresh);
                    patches.push(Patch {
                        path: keys_path,
                        value: Some(Item::Value(Value::InlineTable(table))),
                    });
                }
                return Ok(patches);
            };
            let mut times: Vec<f64> = keys.iter().map(|k| k.0).collect();
            let target = moved.or(same);
            match target {
                Some(i)
                    if keys
                        .iter()
                        .enumerate()
                        .all(|(j, k)| j == i || (j < i && k.0 < *t) || (j > i && k.0 > *t)) =>
                {
                    replace(array, i, pair(*t, value));
                }
                found => {
                    if let Some(i) = found {
                        remove(array, i);
                        times.remove(i);
                    }
                    let at = times.partition_point(|k| *k < *t);
                    insert(array, at, pair(*t, value));
                }
            }
            patches.push(Patch {
                path: property_path,
                value: Some(Item::Value(Value::Array(array.clone()))),
            });
        }
    }
    Ok(patches)
}
