use std::collections::BTreeMap;

use toml_edit::{Item, TableLike, Value};

use super::{Cx, NUM, STR, Tab, V3, bad_item, bad_value, choice, num_value, tables};
use crate::keyframes::Context;
use crate::{Error, Light, LightKind};

#[derive(Clone, Debug, PartialEq)]
pub enum Colour {
    Hex(String),
    Rgb([f64; 3]),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Setting {
    Number(f64),
    Whole(u64),
    Flag(bool),
    Word(String),
    Numbers(Vec<f64>),
    Keys(Vec<[f64; 2]>),
    Colour(Colour),
    Colours(Vec<(f64, Colour)>),
}

pub type Settings = Vec<(&'static str, Setting)>;

#[derive(Clone, Debug, PartialEq)]
pub struct Particles {
    pub name: String,
    pub parent: Option<String>,
    pub settings: Settings,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effects {
    pub particles: Vec<Particles>,
    pub flare: Option<Settings>,
    pub motion_blur: Option<Settings>,
    pub haze: Option<Settings>,
    pub wet: BTreeMap<String, Settings>,
    pub sound: Option<String>,
}

impl Effects {
    pub fn first_key(&self) -> Option<String> {
        if let Some(p) = self.particles.first() {
            return Some(format!("particles[{}]", p.name));
        }
        for (key, set) in [
            ("flare", self.flare.is_some()),
            ("motion_blur", self.motion_blur.is_some()),
            ("haze", self.haze.is_some()),
            ("sound", self.sound.is_some()),
        ] {
            if set {
                return Some(key.to_string());
            }
        }
        self.wet
            .keys()
            .next()
            .map(|name| format!("materials.{name}.wet"))
    }
}

#[derive(Clone, Copy)]
enum Rule {
    Any,
    AtLeast(f64),
    Above(f64),
    Within(f64, f64),
}

#[derive(Clone, Copy)]
enum Shape {
    Num(Rule),
    Whole(u64, u64),
    Flag,
    Word(&'static [&'static str]),
    Vec3 { nonzero: bool },
    Span { above: bool },
    Pair(Rule),
    Keys { unit: bool },
    Colour,
    Colours,
    Rgb,
}

const WHOLE: u64 = u32::MAX as u64;

const PARTICLES: &[(&str, Shape)] = &[
    ("at", Shape::Vec3 { nonzero: false }),
    ("rate", Shape::Num(Rule::AtLeast(0.0))),
    ("rate_keys", Shape::Keys { unit: false }),
    ("burst", Shape::Whole(0, WHOLE)),
    ("max", Shape::Whole(1, WHOLE)),
    ("lifetime", Shape::Span { above: true }),
    ("speed", Shape::Span { above: false }),
    ("direction", Shape::Vec3 { nonzero: true }),
    ("cone", Shape::Num(Rule::Within(0.0, 180.0))),
    ("inherit", Shape::Num(Rule::Within(0.0, 1.0))),
    ("gravity", Shape::Vec3 { nonzero: false }),
    ("drag", Shape::Num(Rule::AtLeast(0.0))),
    ("size", Shape::Pair(Rule::AtLeast(0.0))),
    ("alpha", Shape::Keys { unit: true }),
    ("color", Shape::Colour),
    ("colors", Shape::Colours),
    ("spin", Shape::Pair(Rule::Any)),
    ("stretch", Shape::Num(Rule::AtLeast(0.0))),
    ("blend", Shape::Word(&["alpha", "add"])),
    ("lit", Shape::Flag),
    ("seed", Shape::Whole(0, u64::MAX)),
];

const FLARE: &[(&str, Shape)] = &[
    ("amount", Shape::Num(Rule::Within(0.0, 1.0))),
    ("ghosts", Shape::Whole(0, WHOLE)),
    ("ghost_spacing", Shape::Num(Rule::AtLeast(0.0))),
    ("halo", Shape::Num(Rule::AtLeast(0.0))),
    ("halo_radius", Shape::Num(Rule::AtLeast(0.0))),
    ("starburst", Shape::Num(Rule::AtLeast(0.0))),
    ("threshold", Shape::Num(Rule::AtLeast(0.0))),
];

const MOTION_BLUR: &[(&str, Shape)] = &[
    ("shutter", Shape::Num(Rule::Within(0.0, 1.0))),
    ("max_px", Shape::Num(Rule::AtLeast(0.0))),
    ("samples", Shape::Whole(1, WHOLE)),
];

const HAZE: &[(&str, Shape)] = &[
    ("lo", Shape::Vec3 { nonzero: false }),
    ("hi", Shape::Vec3 { nonzero: false }),
    ("amount", Shape::Num(Rule::Within(0.0, 1.0))),
    ("height", Shape::Num(Rule::Any)),
    ("falloff", Shape::Num(Rule::AtLeast(0.0))),
    ("reach", Shape::Num(Rule::AtLeast(0.0))),
    ("shafts", Shape::Num(Rule::Within(0.0, 1.0))),
    ("steps", Shape::Whole(1, WHOLE)),
    ("color", Shape::Colour),
    ("fog", Shape::Num(Rule::AtLeast(0.0))),
    ("smoke", Shape::Num(Rule::AtLeast(0.0))),
    ("mist", Shape::Num(Rule::AtLeast(0.0))),
    ("floor", Shape::Num(Rule::AtLeast(0.0))),
    ("phase", Shape::Num(Rule::Within(-0.9, 0.9))),
    ("back", Shape::Num(Rule::AtLeast(0.0))),
    ("gold", Shape::Rgb),
    ("ambient", Shape::Rgb),
    ("unmapped", Shape::Num(Rule::AtLeast(0.0))),
    ("seed", Shape::Whole(0, WHOLE)),
];

const WET: &[(&str, Shape)] = &[
    ("amount", Shape::Num(Rule::Within(0.0, 1.0))),
    ("puddles", Shape::Num(Rule::Within(0.0, 1.0))),
    ("puddle_scale", Shape::Num(Rule::Above(0.0))),
    ("seed", Shape::Whole(0, u64::MAX)),
];

fn rule(cx: &Cx, path: &str, value: &Value, rule: Rule) -> Result<f64, Error> {
    let v = num_value(cx, path, value)?;
    let (fits, want) = match rule {
        Rule::Any => (true, String::new()),
        Rule::AtLeast(low) => (v >= low, format!("a number of at least {low}")),
        Rule::Above(low) => (v > low, format!("a number above {low}")),
        Rule::Within(low, high) => (
            (low..=high).contains(&v),
            format!("a number from {low} to {high}"),
        ),
    };
    if fits {
        Ok(v)
    } else {
        Err(Error::new(
            path,
            Some(cx.value_line(value)),
            format!("expected {want}, found {v}"),
        ))
    }
}

fn numbers(cx: &Cx, path: &str, value: &Value, n: usize, each: Rule) -> Result<Vec<f64>, Error> {
    let what = format!("an array of {n} numbers");
    let Some(list) = value.as_array() else {
        return Err(bad_value(cx, path, value, &what));
    };
    if list.len() != n {
        return Err(Error::new(
            path,
            Some(cx.value_line(value)),
            format!("expected {what}, found an array of {}", list.len()),
        ));
    }
    list.iter()
        .enumerate()
        .map(|(i, v)| rule(cx, &format!("{path}[{i}]"), v, each))
        .collect()
}

fn colour(cx: &Cx, path: &str, value: &Value) -> Result<Colour, Error> {
    if let Some(text) = value.as_str() {
        let hex = text.strip_prefix('#').unwrap_or("");
        if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(Colour::Hex(text.to_string()));
        }
        return Err(Error::new(
            path,
            Some(cx.value_line(value)),
            format!("expected \"#rrggbb\" or [r, g, b], found \"{text}\""),
        ));
    }
    let rgb = numbers(cx, path, value, 3, Rule::AtLeast(0.0))?;
    Ok(Colour::Rgb([rgb[0], rgb[1], rgb[2]]))
}

fn shaped(cx: &Cx, path: &str, item: &Item, shape: Shape) -> Result<Setting, Error> {
    let Some(value) = item.as_value() else {
        return Err(bad_item(cx, path, item, "a value"));
    };
    let line = Some(cx.value_line(value));
    Ok(match shape {
        Shape::Flag => match value.as_bool() {
            Some(flag) => Setting::Flag(flag),
            None => return Err(bad_value(cx, path, value, "true or false")),
        },
        Shape::Num(r) => Setting::Number(rule(cx, path, value, r)?),
        Shape::Whole(low, high) => {
            let Some(whole) = value.as_integer() else {
                return Err(bad_value(cx, path, value, "a whole number"));
            };
            let fits = whole >= 0 && (low..=high).contains(&(whole as u64));
            if !fits {
                return Err(Error::new(
                    path,
                    line,
                    format!("expected a whole number from {low} to {high}, found {whole}"),
                ));
            }
            Setting::Whole(whole as u64)
        }
        Shape::Word(words) => {
            let options: Vec<(&str, usize)> =
                words.iter().enumerate().map(|(i, w)| (*w, i)).collect();
            let at = choice(cx, path, item, &options)?;
            Setting::Word(words[at].to_string())
        }
        Shape::Vec3 { nonzero } => {
            let v = numbers(cx, path, value, 3, Rule::Any)?;
            if nonzero && v.iter().all(|c| *c == 0.0) {
                return Err(Error::new(
                    path,
                    line,
                    "expected a nonzero direction, found [0, 0, 0]",
                ));
            }
            Setting::Numbers(v)
        }
        Shape::Span { above } => {
            let v = numbers(cx, path, value, 2, Rule::AtLeast(0.0))?;
            if v[0] > v[1] || (above && v[0] <= 0.0) {
                let low = if above { "above 0" } else { "at least 0" };
                return Err(Error::new(
                    path,
                    line,
                    format!(
                        "expected [low, high] with low {low} and at most high, found [{}, {}]",
                        v[0], v[1]
                    ),
                ));
            }
            Setting::Numbers(v)
        }
        Shape::Pair(r) => Setting::Numbers(numbers(cx, path, value, 2, r)?),
        Shape::Keys { unit } => {
            let each = if unit {
                Rule::Within(0.0, 1.0)
            } else {
                Rule::AtLeast(0.0)
            };
            let Some(list) = value.as_array() else {
                return Err(bad_value(cx, path, value, "an array of [t, value] pairs"));
            };
            let mut keys: Vec<[f64; 2]> = Vec::new();
            for (i, key) in list.iter().enumerate() {
                let pair = numbers(cx, &format!("{path}[{i}]"), key, 2, each)?;
                if let Some([t, _]) = keys.last()
                    && pair[0] <= *t
                {
                    return Err(Error::new(
                        format!("{path}[{i}]"),
                        Some(cx.value_line(key)),
                        format!("expected times that increase, found {} after {t}", pair[0]),
                    ));
                }
                keys.push([pair[0], pair[1]]);
            }
            if keys.is_empty() {
                return Err(Error::new(
                    path,
                    line,
                    "expected at least one [t, value] pair",
                ));
            }
            Setting::Keys(keys)
        }
        Shape::Colour => Setting::Colour(colour(cx, path, value)?),
        Shape::Rgb => Setting::Numbers(numbers(cx, path, value, 3, Rule::AtLeast(0.0))?),
        Shape::Colours => {
            let Some(list) = value.as_array() else {
                return Err(bad_value(cx, path, value, "an array of [t, colour] pairs"));
            };
            let mut out = Vec::new();
            for (i, entry) in list.iter().enumerate() {
                let at = format!("{path}[{i}]");
                let pair = entry.as_array().filter(|pair| pair.len() == 2);
                let Some(pair) = pair else {
                    return Err(bad_value(cx, &at, entry, "a [t, colour] pair"));
                };
                let mut items = pair.iter();
                let (Some(t), Some(c)) = (items.next(), items.next()) else {
                    return Err(bad_value(cx, &at, entry, "a [t, colour] pair"));
                };
                let t = rule(cx, &format!("{at}[0]"), t, Rule::Within(0.0, 1.0))?;
                out.push((t, colour(cx, &format!("{at}[1]"), c)?));
            }
            Setting::Colours(out)
        }
    })
}

fn settings(
    cx: &mut Cx,
    tab: &mut Tab<'_>,
    spec: &[(&'static str, Shape)],
) -> Result<Settings, Error> {
    let mut out = Vec::new();
    for (key, shape) in spec {
        if let Some(item) = tab.peek(cx, key) {
            out.push((*key, shaped(cx, &tab.kpath(key), item, *shape)?));
        }
    }
    Ok(out)
}

fn read_particles(
    cx: &mut Cx,
    t: &dyn TableLike,
    index: usize,
    line: usize,
) -> Result<Particles, Error> {
    let path = format!("particles[{}]", super::label(t, "name", index));
    let mut tab = Tab::new(cx, t, path, format!("particles#{index}"), line);
    let name = tab.req(cx, "name", STR)?;
    let parent = tab.opt(cx, "parent", STR)?;
    let settings = settings(cx, &mut tab, PARTICLES)?;
    super::keys::read(
        cx,
        &mut tab,
        "particles",
        Some(name.as_str()),
        Context::default(),
    )?;
    if settings.iter().any(|(k, _)| *k == "rate_keys")
        && cx
            .keyed
            .get(&format!("particles.{name}"))
            .is_some_and(|k| k.contains_key("rate"))
    {
        return Err(Error::new(
            tab.kpath("keys.rate"),
            Some(line),
            "`rate_keys` and a keyed `rate` are one track, expected one of the two",
        ));
    }
    if settings.iter().any(|(k, _)| *k == "color") && settings.iter().any(|(k, _)| *k == "colors") {
        return Err(Error::new(
            tab.kpath("colors"),
            Some(line),
            "expected `color` or `colors`, not both",
        ));
    }
    tab.finish(cx)?;
    Ok(Particles {
        name,
        parent,
        settings,
    })
}

fn table(
    cx: &mut Cx,
    root: &mut Tab<'_>,
    key: &'static str,
    spec: &[(&'static str, Shape)],
) -> Result<Option<Settings>, Error> {
    let Some(mut tab) = root.sub(cx, key)? else {
        return Ok(None);
    };
    let out = settings(cx, &mut tab, spec)?;
    if key == "haze" {
        super::keys::read(cx, &mut tab, "haze", None, Context::default())?;
    }
    tab.finish(cx)?;
    Ok(Some(out))
}

pub(super) fn read_effects(
    cx: &mut Cx,
    root: &mut Tab<'_>,
    wet: BTreeMap<String, Settings>,
) -> Result<Effects, Error> {
    let particles = tables(cx, root, "particles", read_particles)?;
    let flare = table(cx, root, "flare", FLARE)?;
    let motion_blur = table(cx, root, "motion_blur", MOTION_BLUR)?;
    let haze = table(cx, root, "haze", HAZE)?;
    if let Some(settings) = &haze {
        room_box(cx, settings)?;
    }
    let sound = match root.opt(cx, "sound", STR)? {
        Some(file) if !file.ends_with(".sound.toml") || file.len() == ".sound.toml".len() => {
            return Err(Error::new(
                "sound",
                cx.lines.get("sound").copied(),
                format!("expected a path ending in .sound.toml, found \"{file}\""),
            ));
        }
        other => other,
    };
    Ok(Effects {
        particles,
        flare,
        motion_blur,
        haze,
        wet,
        sound,
    })
}

fn room_box(cx: &Cx, settings: &Settings) -> Result<(), Error> {
    let get = |key: &str| {
        settings.iter().find_map(|(k, v)| match v {
            Setting::Numbers(v) if *k == key => Some(v.clone()),
            _ => None,
        })
    };
    let line = |key: &str| cx.lines.get(&format!("haze.{key}")).copied();
    match (get("lo"), get("hi")) {
        (Some(lo), Some(hi)) => {
            if (0..3).any(|k| lo[k] >= hi[k]) {
                return Err(Error::new(
                    "haze.hi",
                    line("hi"),
                    format!(
                        "expected `hi` above `lo` on every axis, found lo [{}, {}, {}] and hi [{}, {}, {}]",
                        lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]
                    ),
                ));
            }
            Ok(())
        }
        (None, None) => Ok(()),
        (lo, _) => {
            let (key, other) = if lo.is_none() {
                ("lo", "hi")
            } else {
                ("hi", "lo")
            };
            Err(Error::new(
                format!("haze.{key}"),
                line(other),
                format!("a room haze needs `lo` and `hi`, expected `{key}` beside `{other}`"),
            ))
        }
    }
}

pub(super) fn read_wet(cx: &mut Cx, name: &str, item: &Item) -> Result<Settings, Error> {
    let path = format!("materials.{name}.wet");
    let Some(t) = item.as_table_like() else {
        return Err(bad_item(cx, &path, item, "a table"));
    };
    let line = cx.item_line(item);
    let mut tab = Tab::new(cx, t, path.clone(), path, line);
    let out = settings(cx, &mut tab, WET)?;
    tab.finish(cx)?;
    Ok(out)
}

pub(super) fn read_light(
    cx: &mut Cx,
    t: &dyn TableLike,
    index: usize,
    line: usize,
) -> Result<Light, Error> {
    let path = format!("light[{}]", super::label(t, "name", index));
    let mut tab = Tab::new(cx, t, path, format!("light#{index}"), line);
    let name = tab.req(cx, "name", STR)?;
    let kind = super::pick(
        &mut tab,
        cx,
        "kind",
        LightKind::Point,
        &[("point", LightKind::Point), ("spot", LightKind::Spot)],
    )?;
    let parent = tab.opt(cx, "parent", STR)?;
    let at = tab.opt(cx, "at", V3)?;
    let position = tab.opt(cx, "position", V3)?;
    let (position, parent) = match (parent, position, at) {
        (None, Some(position), None) => (position, None),
        (None, None, _) => {
            return Err(tab.missing("position", "an array of 3 numbers, or `parent` with `at`"));
        }
        (None, Some(_), Some(_)) => {
            return Err(Error::new(
                tab.kpath("at"),
                Some(line),
                "`at` places a light in its parent's frame, expected a `parent` or `position` alone",
            ));
        }
        (Some(_), Some(_), _) => {
            return Err(Error::new(
                tab.kpath("position"),
                Some(line),
                "a light with a `parent` is placed by `at` in the parent's frame, expected `at` in place of `position`",
            ));
        }
        (Some(parent), None, at) => ([0.0; 3], Some((parent, at.unwrap_or([0.0; 3])))),
    };
    let direction = tab.opt(cx, "direction", V3)?;
    let cone = tab.opt(cx, "cone", super::V2)?;
    if kind == LightKind::Point {
        for (key, set) in [("direction", direction.is_some()), ("cone", cone.is_some())] {
            if set {
                return Err(Error::new(
                    tab.kpath(key),
                    Some(line),
                    format!(
                        "`{key}` belongs to a spot light, expected `kind = \"spot\"` or the key removed"
                    ),
                ));
            }
        }
    }
    if direction.is_some_and(|d| d.iter().all(|c| *c == 0.0)) {
        return Err(Error::new(
            tab.kpath("direction"),
            Some(line),
            "expected a nonzero direction",
        ));
    }
    if let Some([inner, outer]) = cone
        && !(0.0 <= inner && inner <= outer && outer <= 180.0)
    {
        return Err(Error::new(
            tab.kpath("cone"),
            Some(line),
            format!(
                "expected [inner, outer] degrees with 0 <= inner <= outer <= 180, found [{inner}, {outer}]"
            ),
        ));
    }
    let context = Context {
        spot: kind == LightKind::Spot,
        parented: parent.is_some(),
        ..Context::default()
    };
    let light = Light {
        name,
        kind,
        position,
        parent,
        direction,
        cone,
        color: tab.or(cx, "color", [1.0, 1.0, 1.0], V3)?,
        intensity: tab.req(cx, "intensity", NUM)?,
        radius: tab.or(cx, "radius", 0.0, NUM)?,
        range: tab.or(cx, "range", 0.0, NUM)?,
        shadow: tab.or(cx, "shadow", false, super::BOOL)?,
    };
    super::keys::read(cx, &mut tab, "light", Some(light.name.as_str()), context)?;
    let owner = crate::Owner::Light(light.name.clone());
    let list = super::constraint::list(cx, &mut tab, &owner)?;
    super::constraint::keep(cx, owner, list);
    tab.finish(cx)?;
    Ok(light)
}
