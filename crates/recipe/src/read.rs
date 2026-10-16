use std::collections::BTreeMap;
use std::path::Path;

use toml_edit::{Document, DocumentMut, Item, Table, TableLike, Value};

use crate::include::{self, Assembly, Edit, File, Files, Role, Sources};

pub(crate) mod effects;

use crate::{
    Along, Bake, Bend, BendForm, Bevel, CAP_LUMINANCE, CAP_MEAN, CAPPED, Camera, CameraPath, Caps,
    Casters, Cup, Curl, Daylight, DetailUv, Drift, Ease, Error, Follow, Grade, Kind, Lines, Loaded,
    Lod, LodTarget, Look, Mover, NO_SIDECAR, NORMAL_SIZE, OLD_HEADER, Object, Orbit, Order, Origin,
    Ortho, Outline, Output, PathKey, PathKind, Plate, PlateView, Plates, Problem, Profile,
    ProfileKind, Projection, Proxy, REFERENCE_HOUR, RELEASE_EASE, RIG_LOOK, RIG_OFFSET, Recipe,
    Reference, Rig, RigFocus, RigShake, RigTracks, Room, RouteSpec, SCHEMA, Shadow, Sky,
    SkyAmbient, SkyKind, SkyLight, Span, Spring, Stage, Sun, TEXTURE_KEYS, TextSpace, Textures,
    Timeline, Trace, Use, Uv, Variant, VariantOutput, View, Volume,
};

pub(crate) struct Source {
    starts: Vec<usize>,
}

impl Source {
    pub(crate) fn new(text: &str) -> Source {
        let mut starts = vec![0];
        for (at, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(at + 1);
            }
        }
        Source { starts }
    }

    pub(crate) fn line(&self, offset: usize) -> usize {
        self.starts.partition_point(|&start| start <= offset)
    }
}

enum ParamsState {
    Absent,
    Unloaded(String),
    Loaded(String, serde_json::Value),
}

pub(crate) struct Cx {
    src: Sources,
    params: ParamsState,
    lines: BTreeMap<String, usize>,
    twins: Vec<Problem>,
    plate: Option<Plate>,
    copies: BTreeMap<String, Vec<(Table, bool)>>,
}

impl Cx {
    fn line(&self, span: Option<std::ops::Range<usize>>) -> usize {
        span.map_or(1, |s| self.src.line(s.start))
    }

    fn item_line(&self, item: &Item) -> usize {
        self.line(item.span())
    }

    fn value_line(&self, value: &Value) -> usize {
        self.line(value.span())
    }
}

fn describe(item: &Item) -> &'static str {
    match item {
        Item::None => "nothing",
        Item::Value(value) => describe_value(value),
        Item::Table(_) => "a table",
        Item::ArrayOfTables(_) => "an array of tables",
    }
}

fn describe_value(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "a string",
        Value::Integer(_) => "an integer",
        Value::Float(_) => "a float",
        Value::Boolean(_) => "a boolean",
        Value::Datetime(_) => "a date",
        Value::Array(_) => "an array",
        Value::InlineTable(_) => "a table",
    }
}

fn bad_item(cx: &Cx, path: &str, item: &Item, expected: &str) -> Error {
    Error::new(
        path,
        Some(cx.item_line(item)),
        format!("expected {expected}, found {}", describe(item)),
    )
}

fn bad_value(cx: &Cx, path: &str, value: &Value, expected: &str) -> Error {
    Error::new(
        path,
        Some(cx.value_line(value)),
        format!("expected {expected}, found {}", describe_value(value)),
    )
}

pub(crate) struct Rd<T>(&'static str, fn(&mut Cx, &str, &Item) -> Result<T, Error>);

pub(crate) fn params_ref(text: &str) -> Option<&str> {
    text.strip_prefix("${params.")?.strip_suffix('}')
}

fn param_value(cx: &Cx, path: &str, line: usize, reference: &str) -> Result<f64, Error> {
    let (file, root) = match &cx.params {
        ParamsState::Absent => {
            return Err(Error::new(
                path,
                Some(line),
                format!(
                    "\"${{params.{reference}}}\" needs a `params` file named at the top of the recipe"
                ),
            ));
        }
        ParamsState::Unloaded(file) => {
            return Err(Error::new(
                path,
                Some(line),
                format!(
                    "\"${{params.{reference}}}\" needs `{file}`, which only a recipe loaded from a file can read"
                ),
            ));
        }
        ParamsState::Loaded(file, root) => (file, root),
    };
    let mut at = root;
    for segment in reference.split('.') {
        let next = match at {
            serde_json::Value::Object(map) => map.get(segment),
            serde_json::Value::Array(list) => {
                segment.parse::<usize>().ok().and_then(|i| list.get(i))
            }
            _ => None,
        };
        at = next.ok_or_else(|| {
            Error::new(
                path,
                Some(line),
                format!("params.{reference} is missing from {file}"),
            )
        })?;
    }
    match at.as_f64() {
        Some(number) if number.is_finite() => Ok(number),
        _ => Err(Error::new(
            path,
            Some(line),
            format!("params.{reference} in {file} is not a number"),
        )),
    }
}

fn num_value(cx: &Cx, path: &str, value: &Value) -> Result<f64, Error> {
    match value {
        Value::Float(f) if f.value().is_finite() => Ok(*f.value()),
        Value::Float(_) => Err(Error::new(
            path,
            Some(cx.value_line(value)),
            "expected a finite number, found nan or inf",
        )),
        Value::Integer(i) => Ok(*i.value() as f64),
        Value::String(s) => match params_ref(s.value()) {
            Some(reference) => param_value(cx, path, cx.value_line(value), reference),
            None => Err(bad_value(cx, path, value, "a number")),
        },
        _ => Err(bad_value(cx, path, value, "a number")),
    }
}

const COUNT_MAX: u32 = 4096;

fn int_value(cx: &Cx, path: &str, value: &Value) -> Result<u32, Error> {
    int_upto(cx, path, value, u32::MAX)
}

fn int_upto(cx: &Cx, path: &str, value: &Value, max: u32) -> Result<u32, Error> {
    let line = cx.value_line(value);
    let whole = match value {
        Value::Integer(i) => *i.value() as f64,
        Value::String(s) if params_ref(s.value()).is_some() => num_value(cx, path, value)?,
        _ => return Err(bad_value(cx, path, value, "a whole number")),
    };
    if whole.fract() != 0.0 || whole < 0.0 || whole > f64::from(max) {
        let message = if max == u32::MAX {
            "expected a whole number from 0 up".to_string()
        } else {
            format!("expected a whole number from 0 to {max}, found {whole}")
        };
        return Err(Error::new(path, Some(line), message));
    }
    Ok(whole as u32)
}

fn as_num(cx: &mut Cx, path: &str, item: &Item) -> Result<f64, Error> {
    match item.as_value() {
        Some(value) => num_value(cx, path, value),
        None => Err(bad_item(cx, path, item, "a number")),
    }
}

fn as_u32(cx: &mut Cx, path: &str, item: &Item) -> Result<u32, Error> {
    match item.as_value() {
        Some(value) => int_value(cx, path, value),
        None => Err(bad_item(cx, path, item, "a whole number")),
    }
}

fn as_count(cx: &mut Cx, path: &str, item: &Item) -> Result<u32, Error> {
    match item.as_value() {
        Some(value) => int_upto(cx, path, value, COUNT_MAX),
        None => Err(bad_item(cx, path, item, "a whole number")),
    }
}

fn as_u64(cx: &mut Cx, path: &str, item: &Item) -> Result<u64, Error> {
    match item.as_integer() {
        Some(i) if i >= 0 => Ok(i as u64),
        Some(_) => Err(Error::new(
            path,
            Some(cx.item_line(item)),
            "expected a whole number from 0 up",
        )),
        None => Err(bad_item(cx, path, item, "a whole number")),
    }
}

fn as_str(cx: &mut Cx, path: &str, item: &Item) -> Result<String, Error> {
    match item.as_str() {
        Some(s) => Ok(s.to_string()),
        None => Err(bad_item(cx, path, item, "a string")),
    }
}

fn as_bool(cx: &mut Cx, path: &str, item: &Item) -> Result<bool, Error> {
    match item.as_bool() {
        Some(b) => Ok(b),
        None => Err(bad_item(cx, path, item, "true or false")),
    }
}

fn nums_value<const N: usize>(
    cx: &Cx,
    path: &str,
    value: &Value,
    what: &str,
) -> Result<[f64; N], Error> {
    let Some(list) = value.as_array() else {
        return Err(bad_value(cx, path, value, what));
    };
    if list.len() != N {
        return Err(Error::new(
            path,
            Some(cx.value_line(value)),
            format!("expected {what}, found an array of {}", list.len()),
        ));
    }
    let mut out = [0.0; N];
    for (i, element) in list.iter().enumerate() {
        out[i] = num_value(cx, &format!("{path}[{i}]"), element)?;
    }
    Ok(out)
}

fn as_n<const N: usize>(cx: &mut Cx, path: &str, item: &Item) -> Result<[f64; N], Error> {
    let what = match N {
        2 => "an array of 2 numbers",
        3 => "an array of 3 numbers",
        4 => "an array of 4 numbers",
        _ => "an array of numbers",
    };
    match item.as_value() {
        Some(value) => nums_value::<N>(cx, path, value, what),
        None => Err(bad_item(cx, path, item, what)),
    }
}

fn as_u2(cx: &mut Cx, path: &str, item: &Item) -> Result<[u32; 2], Error> {
    pair_upto(cx, path, item, u32::MAX)
}

fn as_grid(cx: &mut Cx, path: &str, item: &Item) -> Result<[u32; 2], Error> {
    pair_upto(cx, path, item, COUNT_MAX)
}

fn pair_upto(cx: &mut Cx, path: &str, item: &Item, max: u32) -> Result<[u32; 2], Error> {
    let what = "an array of 2 whole numbers";
    let Some(list) = item.as_array() else {
        return Err(bad_item(cx, path, item, what));
    };
    if list.len() != 2 {
        return Err(Error::new(
            path,
            Some(cx.item_line(item)),
            format!("expected {what}, found an array of {}", list.len()),
        ));
    }
    let a = int_upto(cx, &format!("{path}[0]"), list.get(0).expect("two"), max)?;
    let b = int_upto(cx, &format!("{path}[1]"), list.get(1).expect("two"), max)?;
    Ok([a, b])
}

fn as_strings(cx: &mut Cx, path: &str, item: &Item) -> Result<Vec<String>, Error> {
    let what = "an array of strings";
    let Some(list) = item.as_array() else {
        return Err(bad_item(cx, path, item, what));
    };
    list.iter()
        .enumerate()
        .map(|(i, v)| match v.as_str() {
            Some(s) => Ok(s.to_string()),
            None => Err(bad_value(cx, &format!("{path}[{i}]"), v, "a string")),
        })
        .collect()
}

fn as_points<const N: usize>(cx: &mut Cx, path: &str, item: &Item) -> Result<Vec<[f64; N]>, Error> {
    let what = match N {
        2 => "an array of [x, y] pairs",
        _ => "an array of [x, y, z] triples",
    };
    let Some(list) = item.as_array() else {
        return Err(bad_item(cx, path, item, what));
    };
    list.iter()
        .enumerate()
        .map(|(i, v)| {
            nums_value::<N>(
                cx,
                &format!("{path}[{i}]"),
                v,
                if N == 2 {
                    "an array of 2 numbers"
                } else {
                    "an array of 3 numbers"
                },
            )
        })
        .collect()
}

fn as_nums(cx: &mut Cx, path: &str, item: &Item) -> Result<Vec<f64>, Error> {
    let what = "an array of numbers";
    let Some(list) = item.as_array() else {
        return Err(bad_item(cx, path, item, what));
    };
    list.iter()
        .enumerate()
        .map(|(i, v)| num_value(cx, &format!("{path}[{i}]"), v))
        .collect()
}

const NUM: Rd<f64> = Rd("a number", as_num);
const UINT: Rd<u32> = Rd("a whole number", as_u32);
const STR: Rd<String> = Rd("a string", as_str);
const BOOL: Rd<bool> = Rd("true or false", as_bool);
const V2: Rd<[f64; 2]> = Rd("an array of 2 numbers", as_n::<2>);
const V3: Rd<[f64; 3]> = Rd("an array of 3 numbers", as_n::<3>);
const V4: Rd<[f64; 4]> = Rd("an array of 4 numbers", as_n::<4>);
const U2: Rd<[u32; 2]> = Rd("an array of 2 whole numbers", as_u2);
const COUNT: Rd<u32> = Rd("a whole number", as_count);
const GRID: Rd<[u32; 2]> = Rd("an array of 2 whole numbers", as_grid);
const STRS: Rd<Vec<String>> = Rd("an array of strings", as_strings);
const NUMS: Rd<Vec<f64>> = Rd("an array of numbers", as_nums);
const V3S: Rd<Vec<[f64; 3]>> = Rd("an array of [x, y, z] points", as_points::<3>);
const AWAY: [f64; 2] = [-1.0, -1.0];

pub(crate) struct Tab<'a> {
    t: &'a dyn TableLike,
    path: String,
    ipath: String,
    line: usize,
    accepted: Vec<&'static str>,
}

impl<'a> Tab<'a> {
    fn new(cx: &mut Cx, t: &'a dyn TableLike, path: String, ipath: String, line: usize) -> Tab<'a> {
        cx.lines.insert(ipath.clone(), line);
        Tab {
            t,
            path,
            ipath,
            line,
            accepted: Vec::new(),
        }
    }

    fn kpath(&self, key: &str) -> String {
        dotted(&self.path, key)
    }

    fn kipath(&self, key: &str) -> String {
        dotted(&self.ipath, key)
    }

    fn missing(&self, key: &str, what: &str) -> Error {
        Error::new(
            self.kpath(key),
            Some(self.line),
            format!("missing, expected {what}"),
        )
    }

    fn peek(&mut self, cx: &mut Cx, key: &'static str) -> Option<&'a Item> {
        self.accepted.push(key);
        let (_, item) = self.t.get_key_value(key)?;
        cx.lines.insert(self.kipath(key), cx.item_line(item));
        Some(item)
    }

    fn retired(&self, key: &str) -> Option<&'a Item> {
        self.t.get_key_value(key).map(|(_, item)| item)
    }

    fn opt<T>(&mut self, cx: &mut Cx, key: &'static str, rd: Rd<T>) -> Result<Option<T>, Error> {
        match self.peek(cx, key) {
            Some(item) => (rd.1)(cx, &self.kpath(key), item).map(Some),
            None => Ok(None),
        }
    }

    fn req<T>(&mut self, cx: &mut Cx, key: &'static str, rd: Rd<T>) -> Result<T, Error> {
        match self.peek(cx, key) {
            Some(item) => (rd.1)(cx, &self.kpath(key), item),
            None => Err(self.missing(key, rd.0)),
        }
    }

    fn or<T>(&mut self, cx: &mut Cx, key: &'static str, default: T, rd: Rd<T>) -> Result<T, Error> {
        Ok(self.opt(cx, key, rd)?.unwrap_or(default))
    }

    fn sub(&mut self, cx: &mut Cx, key: &'static str) -> Result<Option<Tab<'a>>, Error> {
        let Some(item) = self.peek(cx, key) else {
            return Ok(None);
        };
        let path = self.kpath(key);
        let Some(table) = item.as_table_like() else {
            return Err(bad_item(cx, &path, item, "a table"));
        };
        let line = cx.item_line(item);
        let ipath = self.kipath(key);
        Ok(Some(Tab::new(cx, table, path, ipath, line)))
    }

    fn finish(self, cx: &Cx) -> Result<(), Error> {
        for (key, _) in self.t.iter() {
            if self.accepted.contains(&key) {
                continue;
            }
            let line = self.t.key(key).map_or(self.line, |k| cx.line(k.span()));
            let table = if self.path.is_empty() {
                "the top level".to_string()
            } else {
                format!("`{}`", self.path)
            };
            let mut names: Vec<&str> = Vec::new();
            for name in &self.accepted {
                if !names.contains(name) {
                    names.push(name);
                }
            }
            return Err(Error::new(
                self.kpath(key),
                Some(line),
                format!(
                    "unknown key `{key}` in {table}, expected one of: {}",
                    names.join(", ")
                ),
            ));
        }
        Ok(())
    }
}

fn dotted(base: &str, key: &str) -> String {
    if base.is_empty() {
        key.to_string()
    } else {
        format!("{base}.{key}")
    }
}

fn tables<T>(
    cx: &mut Cx,
    tab: &mut Tab<'_>,
    key: &'static str,
    read: fn(&mut Cx, &dyn TableLike, usize, usize) -> Result<T, Error>,
) -> Result<Vec<T>, Error> {
    let mut out = Vec::new();
    if let Some(item) = tab.peek(cx, key) {
        for (i, (t, line)) in table_list(cx, &tab.kpath(key), item)?
            .into_iter()
            .enumerate()
        {
            out.push(read(cx, t, i, line)?);
        }
    }
    Ok(out)
}

fn table_list<'a>(
    cx: &Cx,
    path: &str,
    item: &'a Item,
) -> Result<Vec<(&'a dyn TableLike, usize)>, Error> {
    match item {
        Item::ArrayOfTables(list) => Ok(list
            .iter()
            .map(|t| (t as &dyn TableLike, cx.line(t.span())))
            .collect()),
        Item::Value(Value::Array(list)) => list
            .iter()
            .enumerate()
            .map(|(i, v)| match v.as_inline_table() {
                Some(t) => Ok((t as &dyn TableLike, cx.value_line(v))),
                None => Err(bad_value(cx, &format!("{path}[{i}]"), v, "a table")),
            })
            .collect(),
        _ => Err(bad_item(cx, path, item, "an array of tables")),
    }
}

fn label(t: &dyn TableLike, key: &str, index: usize) -> String {
    t.get(key)
        .and_then(|item| item.as_str())
        .map_or_else(|| index.to_string(), str::to_string)
}

fn choice<T: Copy>(cx: &Cx, path: &str, item: &Item, options: &[(&str, T)]) -> Result<T, Error> {
    let Some(text) = item.as_str() else {
        return Err(bad_item(cx, path, item, &expected_list(options)));
    };
    options
        .iter()
        .find(|(name, _)| *name == text)
        .map(|(_, value)| *value)
        .ok_or_else(|| {
            Error::new(
                path,
                Some(cx.item_line(item)),
                format!("expected {}, found \"{text}\"", expected_list(options)),
            )
        })
}

fn expected_list<T>(options: &[(&str, T)]) -> String {
    let quoted: Vec<String> = options.iter().map(|(n, _)| format!("\"{n}\"")).collect();
    match quoted.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        Some((last, _)) => last.clone(),
        None => String::new(),
    }
}

fn pick<T: Copy>(
    tab: &mut Tab<'_>,
    cx: &mut Cx,
    key: &'static str,
    default: T,
    options: &[(&str, T)],
) -> Result<T, Error> {
    match tab.peek(cx, key) {
        Some(item) => choice(cx, &tab.kpath(key), item, options),
        None => Ok(default),
    }
}

fn pick_req<T: Copy>(
    tab: &mut Tab<'_>,
    cx: &mut Cx,
    key: &'static str,
    options: &[(&str, T)],
) -> Result<T, Error> {
    match tab.peek(cx, key) {
        Some(item) => choice(cx, &tab.kpath(key), item, options),
        None => Err(tab.missing(key, &expected_list(options))),
    }
}

fn json_of(value: &Value) -> serde_json::Value {
    match value {
        Value::String(s) => serde_json::Value::String(s.value().clone()),
        Value::Integer(i) => serde_json::Value::from(*i.value()),
        Value::Float(f) => serde_json::Number::from_f64(*f.value())
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::Boolean(b) => serde_json::Value::Bool(*b.value()),
        Value::Datetime(d) => serde_json::Value::String(d.value().to_string()),
        Value::Array(list) => serde_json::Value::Array(list.iter().map(json_of).collect()),
        Value::InlineTable(t) => {
            serde_json::Value::Object(t.iter().map(|(k, v)| (k.to_string(), json_of(v))).collect())
        }
    }
}

pub(crate) fn json_of_item(item: &Item) -> serde_json::Value {
    match item {
        Item::None => serde_json::Value::Null,
        Item::Value(v) => json_of(v),
        Item::Table(t) => serde_json::Value::Object(json_map(t)),
        Item::ArrayOfTables(list) => serde_json::Value::Array(
            list.iter()
                .map(|t| serde_json::Value::Object(json_map(t)))
                .collect(),
        ),
    }
}

fn read_textures(cx: &Cx, name: &str, keys: &dyn TableLike) -> Result<Option<Textures>, Error> {
    let path = |key: &str| format!("materials.{name}.{key}");
    let file = |key: &str| -> Result<Option<String>, Error> {
        let Some(item) = keys.get(key) else {
            return Ok(None);
        };
        match item.as_str() {
            Some(text) if !text.trim().is_empty() => Ok(Some(text.to_string())),
            _ => Err(bad_item(
                cx,
                &path(key),
                item,
                "a PNG path relative to the recipe",
            )),
        }
    };
    let number = |key: &str, range: Option<[f64; 2]>| -> Result<Option<f64>, Error> {
        let Some(item) = keys.get(key) else {
            return Ok(None);
        };
        let found = item
            .as_float()
            .or_else(|| item.as_integer().map(|v| v as f64))
            .filter(|v| v.is_finite());
        match (found, range) {
            (Some(v), Some([lo, hi])) if !(lo..=hi).contains(&v) => Err(Error::new(
                path(key),
                Some(cx.item_line(item)),
                format!("expected a number from {lo} to {hi}, found {v}"),
            )),
            (Some(v), _) => Ok(Some(v)),
            (None, Some([lo, hi])) => Err(bad_item(
                cx,
                &path(key),
                item,
                &format!("a number from {lo} to {hi}"),
            )),
            (None, None) => Err(bad_item(cx, &path(key), item, "a number")),
        }
    };
    let found = Textures {
        base_map: file("base_map")?,
        normal_map: file("normal_map")?,
        normal_scale: number("normal_scale", None)?,
        roughness_map: file("roughness_map")?,
        occlusion_map: file("occlusion_map")?,
        occlusion_strength: number("occlusion_strength", Some([0.0, 1.0]))?,
        detail_normal_map: file("detail_normal_map")?,
        detail_normal_scale: number("detail_normal_scale", None)?,
    };
    Ok((found != Textures::default()).then_some(found))
}

fn json_map<C: FromIterator<(String, serde_json::Value)>>(t: &dyn TableLike) -> C {
    t.iter()
        .map(|(k, v)| (k.to_string(), json_of_item(v)))
        .collect()
}

fn twin_used(cx: &mut Cx, tab: &Tab<'_>, key: &str, twin: &str) {
    let path = tab.kpath(twin);
    let line = cx.lines.get(&tab.kipath(twin)).copied().unwrap_or(tab.line);
    if cx.plate.is_none() {
        cx.twins.push(Problem {
            path,
            line: Some(line),
            text: format!(
                "`{twin}` reads plate pixels, so the recipe needs a `[plate]`; `{key}` takes metres"
            ),
        });
    }
}

fn both_set(tab: &Tab<'_>, cx: &Cx, key: &str, twin: &str) -> Error {
    let line = cx.lines.get(&tab.kipath(twin)).copied().unwrap_or(tab.line);
    Error::new(
        tab.kpath(twin),
        Some(line),
        format!("`{key}` and `{twin}` are both set, expected only one"),
    )
}

fn twin<M, P>(
    tab: &mut Tab<'_>,
    cx: &mut Cx,
    (key, twin): (&'static str, &'static str),
    metres: Rd<M>,
    pixels: Rd<P>,
    px_before_both: bool,
    scale: impl FnOnce(Option<&Plate>, P) -> M,
) -> Result<Option<M>, Error> {
    let metres = tab.opt(cx, key, metres)?;
    let Some(item) = tab.peek(cx, twin) else {
        return Ok(metres);
    };
    if metres.is_some() && !px_before_both {
        return Err(both_set(tab, cx, key, twin));
    }
    let px = (pixels.1)(cx, &tab.kpath(twin), item)?;
    if metres.is_some() {
        return Err(both_set(tab, cx, key, twin));
    }
    twin_used(cx, tab, key, twin);
    Ok(Some(scale(cx.plate.as_ref(), px)))
}

fn per_pixel(plate: Option<&Plate>) -> f64 {
    plate.map_or(1.0, |p| p.metres_per_pixel)
}

fn as_xy_z(cx: &mut Cx, path: &str, item: &Item) -> Result<Vec<f64>, Error> {
    let list = as_nums(cx, path, item)?;
    if list.len() != 2 && list.len() != 3 {
        return Err(Error::new(
            path,
            Some(cx.item_line(item)),
            format!(
                "expected an array of 2 or 3 numbers, found an array of {}",
                list.len()
            ),
        ));
    }
    Ok(list)
}

fn vec3_or_px(
    tab: &mut Tab<'_>,
    cx: &mut Cx,
    key: &'static str,
    twin_key: &'static str,
) -> Result<Option<[f64; 3]>, Error> {
    let xy_z = Rd("an array of 2 or 3 numbers", as_xy_z);
    twin(tab, cx, (key, twin_key), V3, xy_z, false, |plate, list| {
        let z = list.get(2).copied().unwrap_or(0.0);
        match plate {
            Some(plate) => {
                let [x, y] = plate.to_metres([list[0], list[1]]);
                [x, y, z]
            }
            None => [list[0], list[1], z],
        }
    })
}

fn length_or_px(
    tab: &mut Tab<'_>,
    cx: &mut Cx,
    key: &'static str,
    twin_key: &'static str,
) -> Result<Option<f64>, Error> {
    twin(tab, cx, (key, twin_key), NUM, NUM, true, |plate, px| {
        plate.map_or(px, |p| px * p.metres_per_pixel)
    })
}

fn length_req(
    tab: &mut Tab<'_>,
    cx: &mut Cx,
    key: &'static str,
    twin_key: &'static str,
) -> Result<f64, Error> {
    length_or_px(tab, cx, key, twin_key)?.ok_or_else(|| tab.missing(key, NUM.0))
}

fn read_plate(cx: &mut Cx, root: &mut Tab<'_>) -> Result<Option<Plate>, Error> {
    let Some(mut tab) = root.sub(cx, "plate")? else {
        return Ok(None);
    };
    let width = tab.req(cx, "width", NUM)?;
    let height = tab.req(cx, "height", NUM)?;
    let metres_per_pixel = tab.req(cx, "metres_per_pixel", NUM)?;
    let origin = pick(
        &mut tab,
        cx,
        "origin",
        Origin::Centre,
        &[("centre", Origin::Centre), ("top_left", Origin::TopLeft)],
    )?;
    let camera = tab.opt(cx, "camera", STR)?;
    tab.finish(cx)?;
    Ok(Some(Plate {
        width,
        height,
        metres_per_pixel,
        origin,
        camera,
    }))
}

fn read_output(cx: &mut Cx, root: &mut Tab<'_>) -> Result<Output, Error> {
    let Some(mut tab) = root.sub(cx, "output")? else {
        return Ok(Output {
            glb: None,
            sidecar: None,
            sidecar_dialect: NO_SIDECAR.to_string(),
            order: Order::File,
            bake: None,
            hierarchy: false,
            scene: None,
            scene_camera: None,
            scene_format: 1,
        });
    };
    let glb = tab.opt(cx, "glb", STR)?;
    let sidecar = tab.opt(cx, "sidecar", STR)?;
    let sidecar_dialect = tab
        .opt(cx, "sidecar_dialect", STR)?
        .unwrap_or_else(|| NO_SIDECAR.to_string());
    let order = pick(
        &mut tab,
        cx,
        "order",
        Order::File,
        &[("file", Order::File), ("natural", Order::Natural)],
    )?;
    let bake = tab.opt(cx, "bake", STR)?;
    let hierarchy = tab.or(cx, "hierarchy", false, BOOL)?;
    let scene = tab.opt(cx, "scene", STR)?;
    let scene_camera = tab.opt(cx, "scene_camera", STR)?;
    let scene_format = tab.or(cx, "scene_format", 1, UINT)?;
    tab.finish(cx)?;
    Ok(Output {
        glb,
        sidecar,
        sidecar_dialect,
        order,
        bake,
        hierarchy,
        scene,
        scene_camera,
        scene_format,
    })
}

fn read_profile(
    cx: &mut Cx,
    owner: &mut Tab<'_>,
    key: &'static str,
    named_kind: bool,
) -> Result<Option<Profile>, Error> {
    let Some(mut tab) = owner.sub(cx, key)? else {
        return Ok(None);
    };
    let kind = if named_kind {
        pick(
            &mut tab,
            cx,
            "kind",
            ProfileKind::Round,
            &[
                ("round", ProfileKind::Round),
                ("chamfer", ProfileKind::Chamfer),
                ("none", ProfileKind::None),
            ],
        )?
    } else {
        ProfileKind::Round
    };
    let width = if kind == ProfileKind::None {
        tab.or(cx, "width", 0.0, NUM)?
    } else {
        tab.req(cx, "width", NUM)?
    };
    let default_segments = if kind == ProfileKind::Chamfer { 1 } else { 4 };
    let segments = tab.or(cx, "segments", default_segments, COUNT)?;
    tab.finish(cx)?;
    Ok(Some(Profile {
        kind,
        width,
        segments,
    }))
}

fn read_bevel(cx: &mut Cx, tab: &mut Tab<'_>) -> Result<Option<Bevel>, Error> {
    let Some(mut sub) = tab.sub(cx, "bevel")? else {
        return Ok(None);
    };
    let width = sub.req(cx, "width", NUM)?;
    let segments = sub.or(cx, "segments", 1, COUNT)?;
    let angle = sub.or(cx, "angle", 30.0, NUM)?;
    let clamp = sub.or(cx, "clamp", true, BOOL)?;
    sub.finish(cx)?;
    Ok(Some(Bevel {
        width,
        segments,
        angle,
        clamp,
    }))
}

fn read_detail_uv(cx: &mut Cx, tab: &mut Tab<'_>) -> Result<Option<DetailUv>, Error> {
    let Some(mut sub) = tab.sub(cx, "detail_uv")? else {
        return Ok(None);
    };
    let texels_per_m = sub.req(cx, "texels_per_m", NUM)?;
    let gutter = sub.or(cx, "gutter", 16, COUNT)?;
    sub.finish(cx)?;
    Ok(Some(DetailUv {
        texels_per_m,
        gutter,
    }))
}

fn read_uv(cx: &mut Cx, tab: &mut Tab<'_>) -> Result<Option<Uv>, Error> {
    let Some(item) = tab.peek(cx, "uv") else {
        return Ok(None);
    };
    let path = tab.kpath("uv");
    if item.is_str() {
        return choice(cx, &path, item, &[("plate", 0), ("local", 1), ("none", 2)]).map(|n| {
            Some(match n {
                0 => Uv::Plate,
                1 => Uv::Local,
                _ => Uv::None,
            })
        });
    }
    let Some(table) = item.as_table_like() else {
        return Err(bad_item(
            cx,
            &path,
            item,
            "\"plate\", \"local\", \"none\" or a table with kind = \"cylinder\" or \"rect\"",
        ));
    };
    let line = cx.item_line(item);
    let mut sub = Tab::new(cx, table, path.clone(), tab.kipath("uv"), line);
    let kind = sub.req(cx, "kind", STR)?;
    let uv = match kind.as_str() {
        "cylinder" => {
            let degrees = sub.or(cx, "degrees", 360.0, NUM)?;
            let atlas = sub.or(cx, "atlas", [0.0, 0.0, 1.0, 1.0], V4)?;
            let band = sub.opt(cx, "band", V2)?;
            let wall = sub.opt(cx, "wall", V2)?;
            let away = sub.or(cx, "away", AWAY, V2)?;
            Uv::Cylinder {
                degrees,
                atlas,
                band,
                wall,
                away,
            }
        }
        "rect" => {
            let present = ["rect", "rect_px"]
                .iter()
                .filter(|key| table.contains_key(key))
                .count();
            if present != 1 {
                return Err(match present {
                    0 => Error::new(
                        format!("{path}.rect"),
                        Some(line),
                        "missing, expected `rect` or `rect_px`",
                    ),
                    _ => {
                        sub.peek(cx, "rect_px");
                        both_set(&sub, cx, "rect", "rect_px")
                    }
                });
            }
            let [x0, y0, x1, y1] = rect_of(&mut sub, cx)?;
            let rect = [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)];
            let atlas = sub.or(cx, "atlas", [0.0, 0.0, 1.0, 1.0], V4)?;
            let facing = sub.or(cx, "facing", 0.5, NUM)?;
            let clamp = sub.or(cx, "clamp", false, BOOL)?;
            let away = sub.or(cx, "away", AWAY, V2)?;
            Uv::Rect {
                rect,
                atlas,
                facing,
                clamp,
                away,
            }
        }
        _ => {
            return Err(Error::new(
                format!("{path}.kind"),
                Some(line),
                format!("expected \"cylinder\" or \"rect\", found \"{kind}\""),
            ));
        }
    };
    sub.finish(cx)?;
    Ok(Some(uv))
}

fn read_outline(cx: &mut Cx, path: &str, ipath: &str, item: &Item) -> Result<Outline, Error> {
    let Some(table) = item.as_table_like() else {
        return Err(bad_item(cx, path, item, "a table"));
    };
    outline_table(cx, path, ipath, table, cx.item_line(item))
}

fn outline_table(
    cx: &mut Cx,
    path: &str,
    ipath: &str,
    table: &dyn TableLike,
    line: usize,
) -> Result<Outline, Error> {
    let shapes = [
        "rect",
        "rect_px",
        "circle",
        "circle_px",
        "points",
        "points_px",
        "superellipse",
        "union",
        "difference",
    ];
    let present: Vec<&str> = shapes
        .iter()
        .copied()
        .filter(|name| table.contains_key(name))
        .collect();
    let mut tab = Tab::new(cx, table, path.to_string(), ipath.to_string(), line);
    if present.len() != 1 {
        return Err(Error::new(
            path,
            Some(line),
            "expected exactly one of rect, circle, points, superellipse, union or difference",
        ));
    }
    let outline = match present[0] {
        "rect" | "rect_px" => {
            let rect = rect_of(&mut tab, cx)?;
            let radius = length_or_px(&mut tab, cx, "radius", "radius_px")?.unwrap_or(0.0);
            let turned = tab.or(cx, "turned", 0.0, NUM)?;
            let segments = tab.or(cx, "segments", 8, COUNT)?;
            Outline::Rect {
                rect,
                radius,
                turned,
                segments,
            }
        }
        "circle" | "circle_px" => {
            let (centre, radius) = circle_of(&mut tab, cx)?;
            let segments = tab.or(cx, "segments", 64, COUNT)?;
            Outline::Circle {
                centre,
                radius,
                segments,
            }
        }
        "points" | "points_px" => {
            let points = points_of(&mut tab, cx)?;
            let segments = tab.or(cx, "segments", 8, COUNT)?;
            Outline::Points { points, segments }
        }
        "superellipse" => superellipse_of(&mut tab, cx)?,
        "union" => {
            let item = tab.peek(cx, "union").expect("present");
            let parts = outline_list(cx, &tab, "union", item)?;
            if parts.is_empty() {
                return Err(Error::new(
                    tab.kpath("union"),
                    Some(cx.item_line(item)),
                    "expected at least one outline",
                ));
            }
            Outline::Union(parts)
        }
        _ => {
            let item = tab.peek(cx, "difference").expect("present");
            let mut parts = outline_list(cx, &tab, "difference", item)?;
            if parts.len() != 2 {
                return Err(Error::new(
                    tab.kpath("difference"),
                    Some(cx.item_line(item)),
                    format!("expected exactly 2 outlines, found {}", parts.len()),
                ));
            }
            let b = parts.pop().expect("two");
            let a = parts.pop().expect("two");
            Outline::Difference(Box::new(a), Box::new(b))
        }
    };
    tab.finish(cx)?;
    Ok(outline)
}

fn outline_list(cx: &mut Cx, tab: &Tab<'_>, key: &str, item: &Item) -> Result<Vec<Outline>, Error> {
    let path = tab.kpath(key);
    let ipath = tab.kipath(key);
    let tables = table_list(cx, &path, item)?;
    tables
        .into_iter()
        .enumerate()
        .map(|(i, (table, line))| {
            outline_table(
                cx,
                &format!("{path}[{i}]"),
                &format!("{ipath}#{i}"),
                table,
                line,
            )
        })
        .collect()
}

fn superellipse_of(tab: &mut Tab<'_>, cx: &mut Cx) -> Result<Outline, Error> {
    let mut sub = tab.sub(cx, "superellipse")?.expect("present");
    let half = sub.req(cx, "half", V2)?;
    let exponent = sub.req(cx, "exponent", NUM)?;
    let segments = sub.or(cx, "segments", 64, COUNT)?;
    let centre = sub.or(cx, "centre", [0.0, 0.0], V2)?;
    let line = |sub: &Tab<'_>, cx: &Cx, key: &str| {
        cx.lines.get(&sub.kipath(key)).copied().or(Some(sub.line))
    };
    if !(half[0] > 0.0 && half[1] > 0.0) {
        return Err(Error::new(
            sub.kpath("half"),
            line(&sub, cx, "half"),
            "expected two half sizes above 0",
        ));
    }
    if exponent <= 0.0 {
        return Err(Error::new(
            sub.kpath("exponent"),
            line(&sub, cx, "exponent"),
            "expected an exponent above 0: 2 is an ellipse, 4 a squircle",
        ));
    }
    if segments < 8 || segments % 4 != 0 {
        return Err(Error::new(
            sub.kpath("segments"),
            line(&sub, cx, "segments"),
            format!(
                "expected a multiple of 4 from 8 up, so the shape meets both axes, found {segments}"
            ),
        ));
    }
    sub.finish(cx)?;
    let ring = manfred_mesh::outline::superellipse(centre, half, exponent, segments);
    Ok(Outline::Points {
        points: ring.0.into_iter().map(|[x, y]| [x, y, 0.0]).collect(),
        segments: 8,
    })
}

fn rect_of(tab: &mut Tab<'_>, cx: &mut Cx) -> Result<[f64; 4], Error> {
    if let Some(rect) = tab.opt(cx, "rect", V4)? {
        return Ok(rect);
    }
    let px = tab.req(cx, "rect_px", V4)?;
    twin_used(cx, tab, "rect", "rect_px");
    let Some(plate) = cx.plate.clone() else {
        return Ok(px);
    };
    let [x0, y0] = plate.to_metres([px[0], px[1]]);
    let [x1, y1] = plate.to_metres([px[2], px[3]]);
    Ok([x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)])
}

fn circle_of(tab: &mut Tab<'_>, cx: &mut Cx) -> Result<([f64; 2], f64), Error> {
    if let Some(c) = tab.opt(cx, "circle", V3)? {
        return Ok(([c[0], c[1]], c[2]));
    }
    let px = tab.req(cx, "circle_px", V3)?;
    twin_used(cx, tab, "circle", "circle_px");
    match cx.plate.clone() {
        Some(plate) => Ok((
            plate.to_metres([px[0], px[1]]),
            px[2] * plate.metres_per_pixel,
        )),
        None => Ok(([px[0], px[1]], px[2])),
    }
}

fn as_round_points(cx: &mut Cx, path: &str, item: &Item) -> Result<Vec<[f64; 3]>, Error> {
    let Some(list) = item.as_array() else {
        return Err(bad_item(cx, path, item, POINTS.0));
    };
    let mut out = Vec::new();
    for (i, v) in list.iter().enumerate() {
        let at = format!("{path}[{i}]");
        let Some(point) = v.as_array() else {
            return Err(bad_value(cx, &at, v, "an array of 2 or 3 numbers"));
        };
        if point.len() != 2 && point.len() != 3 {
            return Err(Error::new(
                at,
                Some(cx.value_line(v)),
                format!(
                    "expected an array of 2 or 3 numbers, found an array of {}",
                    point.len()
                ),
            ));
        }
        let mut p = [0.0; 3];
        for (j, n) in point.iter().enumerate() {
            p[j] = num_value(cx, &format!("{at}[{j}]"), n)?;
        }
        out.push(p);
    }
    Ok(out)
}

const POINTS: Rd<Vec<[f64; 3]>> = Rd(
    "an array of [x, y] or [x, y, radius] points",
    as_round_points,
);

fn points_of(tab: &mut Tab<'_>, cx: &mut Cx) -> Result<Vec<[f64; 3]>, Error> {
    twin(
        tab,
        cx,
        ("points", "points_px"),
        POINTS,
        POINTS,
        true,
        |plate, px| {
            let Some(plate) = plate else {
                return px;
            };
            px.into_iter()
                .map(|p| {
                    let [x, y] = plate.to_metres([p[0], p[1]]);
                    [x, y, p[2] * plate.metres_per_pixel]
                })
                .collect()
        },
    )?
    .ok_or_else(|| tab.missing("points", POINTS.0))
}

fn read_bend(cx: &mut Cx, tab: &mut Tab<'_>) -> Result<Option<Bend>, Error> {
    let Some(mut sub) = tab.sub(cx, "bend")? else {
        return Ok(None);
    };
    let radius = sub.req(cx, "radius", NUM)?;
    let form = pick(
        &mut sub,
        cx,
        "form",
        BendForm::Wrap,
        &[("wrap", BendForm::Wrap), ("lift", BendForm::Lift)],
    )?;
    sub.finish(cx)?;
    Ok(Some(Bend { radius, form }))
}

fn read_curl(cx: &mut Cx, tab: &mut Tab<'_>) -> Result<Option<Curl>, Error> {
    let Some(mut sub) = tab.sub(cx, "curl")? else {
        return Ok(None);
    };
    let width = length_req(&mut sub, cx, "width", "width_px")?;
    let depth = sub.req(cx, "depth", NUM)?;
    sub.finish(cx)?;
    Ok(Some(Curl { width, depth }))
}

fn read_cup(cx: &mut Cx, tab: &mut Tab<'_>) -> Result<Option<Cup>, Error> {
    let Some(mut sub) = tab.sub(cx, "cup")? else {
        return Ok(None);
    };
    let from = sub.req(cx, "from", NUM)?;
    let depth = sub.req(cx, "depth", NUM)?;
    sub.finish(cx)?;
    Ok(Some(Cup { from, depth }))
}

fn read_along(cx: &mut Cx, tab: &mut Tab<'_>) -> Result<Option<Along>, Error> {
    let Some(mut sub) = tab.sub(cx, "along")? else {
        return Ok(None);
    };
    let outline = read_required_outline(cx, &mut sub)?;
    let offset = sub.or(cx, "offset", 0.0, NUM)?;
    let depth = sub.or(cx, "depth", 0.0, NUM)?;
    sub.finish(cx)?;
    Ok(Some(Along {
        outline,
        offset,
        depth,
    }))
}

fn size2_or_px(tab: &mut Tab<'_>, cx: &mut Cx) -> Result<[f64; 2], Error> {
    twin(tab, cx, ("size", "size_px"), V2, V2, true, |plate, px| {
        let m = per_pixel(plate);
        [px[0] * m, px[1] * m]
    })?
    .ok_or_else(|| tab.missing("size", V2.0))
}

fn size3_or_px(tab: &mut Tab<'_>, cx: &mut Cx) -> Result<[f64; 3], Error> {
    twin(tab, cx, ("size", "size_px"), V3, V3, true, |plate, px| {
        let m = per_pixel(plate);
        [px[0] * m, px[1] * m, px[2]]
    })?
    .ok_or_else(|| tab.missing("size", V3.0))
}

const KINDS: &[&str] = &[
    "box",
    "cylinder",
    "sphere",
    "plane",
    "disc",
    "torus",
    "capsule",
    "outline",
    "pocket",
    "lathe",
    "sweep",
    "mesh",
    "generator",
    "library",
    "card",
    "text",
];

fn kinds() -> String {
    KINDS
        .iter()
        .map(|k| format!("\"{k}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

fn read_kind(cx: &mut Cx, tab: &mut Tab<'_>, kind: &str) -> Result<Kind, Error> {
    Ok(match kind {
        "box" => Kind::Box {
            size: size3_or_px(tab, cx)?,
        },
        "cylinder" => Kind::Cylinder {
            radius: length_req(tab, cx, "radius", "radius_px")?,
            depth: length_req(tab, cx, "depth", "depth_px")?,
            segments: tab.or(cx, "segments", 64, COUNT)?,
            caps: pick(
                tab,
                cx,
                "caps",
                Caps::Ngon,
                &[("ngon", Caps::Ngon), ("none", Caps::None)],
            )?,
        },
        "sphere" => Kind::Sphere {
            radius: tab.req(cx, "radius", NUM)?,
            segments: tab.or(cx, "segments", 96, COUNT)?,
            rings: tab.or(cx, "rings", 48, COUNT)?,
        },
        "plane" => {
            let size = size2_or_px(tab, cx)?;
            let grid = tab.opt(cx, "grid", GRID)?;
            let bend = read_bend(cx, tab)?;
            let curl = read_curl(cx, tab)?;
            let cup = read_cup(cx, tab)?;
            let jag = twin(tab, cx, ("jag", "jag_px"), NUMS, NUMS, true, |plate, px| {
                let m = per_pixel(plate);
                px.into_iter().map(|v| v * m).collect()
            })?
            .unwrap_or_default();
            Kind::Plane {
                size,
                grid,
                bend,
                curl,
                cup,
                jag,
            }
        }
        "disc" => Kind::Disc {
            radius: tab.req(cx, "radius", NUM)?,
            segments: tab.or(cx, "segments", 64, COUNT)?,
        },
        "torus" => Kind::Torus {
            major: tab.req(cx, "major", NUM)?,
            minor: tab.req(cx, "minor", NUM)?,
            segments: tab.or(cx, "segments", 96, COUNT)?,
            rings: tab.or(cx, "rings", 24, COUNT)?,
        },
        "capsule" => Kind::Capsule {
            radius: tab.req(cx, "radius", NUM)?,
            length: tab.req(cx, "length", NUM)?,
        },
        "outline" => {
            let outline = read_required_outline(cx, tab)?;
            let holes = match tab.peek(cx, "holes") {
                Some(item) => outline_list(cx, tab, "holes", item)?,
                None => Vec::new(),
            };
            let front = tab.req(cx, "front", NUM)?;
            let back = tab.req(cx, "back", NUM)?;
            let profile = read_profile(cx, tab, "profile", true)?.unwrap_or(Profile {
                kind: ProfileKind::None,
                width: 0.0,
                segments: 1,
            });
            Kind::Outline {
                outline,
                holes,
                front,
                back,
                profile,
            }
        }
        "pocket" => {
            let into = tab.req(cx, "into", STR)?;
            let outline = read_required_outline(cx, tab)?;
            let depth = tab.req(cx, "depth", NUM)?;
            let rim = read_profile(cx, tab, "rim", false)?;
            let floor = read_profile(cx, tab, "floor", false)?;
            Kind::Pocket {
                into,
                outline,
                depth,
                rim,
                floor,
            }
        }
        "lathe" => {
            let profile = match tab.peek(cx, "profile") {
                Some(item) => as_points::<2>(cx, &tab.kpath("profile"), item)?,
                None => return Err(tab.missing("profile", "an array of [r, y] pairs")),
            };
            Kind::Lathe {
                profile,
                segments: tab.or(cx, "segments", 128, COUNT)?,
                weld: tab.or(cx, "weld", 1e-7, NUM)?,
            }
        }
        "sweep" => {
            let along = read_along(cx, tab)?;
            let path = match tab.peek(cx, "path") {
                Some(item) if along.is_some() => {
                    return Err(Error::new(
                        tab.kpath("path"),
                        Some(cx.item_line(item)),
                        "`path` and `along` are both set, expected only one",
                    ));
                }
                Some(item) => as_points::<3>(cx, &tab.kpath("path"), item)?,
                None if along.is_some() => Vec::new(),
                None => return Err(tab.missing("path", "an array of [x, y, z] triples")),
            };
            let radius = tab.opt(cx, "radius", NUM)?;
            let radii = tab.opt(cx, "radii", NUMS)?;
            let radii = match (radius, radii) {
                (Some(_), Some(_)) => {
                    return Err(Error::new(
                        tab.kpath("radii"),
                        cx.lines.get(&tab.kipath("radii")).copied(),
                        "`radius` and `radii` are both set, expected only one",
                    ));
                }
                (Some(r), None) => vec![r],
                (None, Some(list)) => list,
                (None, None) => {
                    return Err(tab.missing(
                        "radius",
                        "`radius` (a number) or `radii` (an array of numbers)",
                    ));
                }
            };
            let ring = tab.or(cx, "ring", 16, COUNT)?;
            let (closed, caps) = if along.is_some() {
                for key in ["closed", "caps"] {
                    if let Some(item) = tab.peek(cx, key) {
                        return Err(Error::new(
                            tab.kpath(key),
                            Some(cx.item_line(item)),
                            format!(
                                "`{key}` goes with `path`; a sweep along an outline is closed and has no caps"
                            ),
                        ));
                    }
                }
                (true, false)
            } else {
                (
                    tab.or(cx, "closed", false, BOOL)?,
                    tab.or(cx, "caps", true, BOOL)?,
                )
            };
            Kind::Sweep {
                path,
                radii,
                ring,
                closed,
                caps,
                along,
            }
        }
        "mesh" => Kind::Mesh {
            path: tab.req(cx, "path", STR)?,
            node: tab.opt(cx, "node", STR)?,
            morph: read_morph(cx, tab)?,
        },
        "generator" => {
            let generator = tab.req(cx, "generator", STR)?;
            let params = match tab.peek(cx, "params") {
                Some(item) => match item.as_table_like() {
                    Some(t) => json_map(t),
                    None => return Err(bad_item(cx, &tab.kpath("params"), item, "a table")),
                },
                None => BTreeMap::new(),
            };
            Kind::Generator { generator, params }
        }
        "library" => Kind::Library {
            object: tab.req(cx, "object", STR)?,
            clip: tab.opt(cx, "clip", STR)?,
            looped: tab.or(cx, "loop", true, BOOL)?,
        },
        "card" => Kind::Card {
            size: size2_or_px(tab, cx)?,
            image: tab.opt(cx, "image", STR)?,
            slot: tab.opt(cx, "slot", STR)?,
            face_camera: tab.or(cx, "face_camera", false, BOOL)?,
            alpha_cutoff: tab.or(cx, "alpha_cutoff", 0.5, NUM)?,
        },
        "text" => read_text(cx, tab)?,
        other => {
            return Err(Error::new(
                tab.kpath("kind"),
                cx.lines.get(&tab.kipath("kind")).copied(),
                format!("expected one of {}, found \"{other}\"", kinds()),
            ));
        }
    })
}

fn read_morph(cx: &mut Cx, tab: &mut Tab<'_>) -> Result<BTreeMap<String, f64>, Error> {
    let Some(item) = tab.peek(cx, "morph") else {
        return Ok(BTreeMap::new());
    };
    let path = tab.kpath("morph");
    let Some(table) = item.as_table_like() else {
        return Err(bad_item(
            cx,
            &path,
            item,
            "a table of morph target names to weights",
        ));
    };
    let mut out = BTreeMap::new();
    for (name, value) in table.iter() {
        let at = dotted(&path, name);
        let weight = as_num(cx, &at, value)?;
        out.insert(name.to_string(), weight);
    }
    Ok(out)
}

fn read_required_outline(cx: &mut Cx, tab: &mut Tab<'_>) -> Result<Outline, Error> {
    match tab.peek(cx, "outline") {
        Some(item) => read_outline(cx, &tab.kpath("outline"), &tab.kipath("outline"), item),
        None => Err(tab.missing(
            "outline",
            "a table with rect, circle, points, union or difference",
        )),
    }
}

fn read_text(cx: &mut Cx, tab: &mut Tab<'_>) -> Result<Kind, Error> {
    let path = tab.kpath("spans");
    let ipath = tab.kipath("spans");
    let Some(item) = tab.peek(cx, "spans") else {
        return Err(tab.missing("spans", "an array of tables with text and size"));
    };
    let tables = table_list(cx, &path, item)?;
    if tables.is_empty() {
        return Err(Error::new(
            path,
            Some(cx.item_line(item)),
            "expected at least one span",
        ));
    }
    let mut spans = Vec::new();
    for (i, (table, line)) in tables.into_iter().enumerate() {
        let mut sub = Tab::new(
            cx,
            table,
            format!("{path}[{i}]"),
            format!("{ipath}#{i}"),
            line,
        );
        let text = sub.req(cx, "text", STR)?;
        let family = sub.opt(cx, "family", STR)?;
        let size = sub.req(cx, "size", NUM)?;
        let weight = sub.opt(cx, "weight", UINT)?;
        let italic = sub.or(cx, "italic", false, BOOL)?;
        let color = sub.or(cx, "color", [1.0, 1.0, 1.0], V3)?;
        sub.finish(cx)?;
        spans.push(Span {
            text,
            family,
            size,
            weight,
            italic,
            color,
        });
    }
    Ok(Kind::Text {
        spans,
        wrap: tab.opt(cx, "wrap", NUM)?,
        anchor: tab.opt(cx, "anchor", STR)?,
        space: pick(
            tab,
            cx,
            "space",
            TextSpace::Surface,
            &[
                ("surface", TextSpace::Surface),
                ("overlay", TextSpace::Overlay),
                ("lit", TextSpace::Lit),
            ],
        )?,
        on: tab.opt(cx, "on", STR)?,
    })
}

fn read_object(cx: &mut Cx, t: &dyn TableLike, index: usize, line: usize) -> Result<Object, Error> {
    let path = format!("object[{}]", label(t, "name", index));
    let mut tab = Tab::new(cx, t, path, format!("object#{index}"), line);
    let name = tab.req(cx, "name", STR)?;
    let id = tab.opt(cx, "id", STR)?;
    let kind_item = tab.peek(cx, "kind");
    let kind_name = match kind_item {
        Some(item) => as_str(cx, &tab.kpath("kind"), item)?,
        None => return Err(tab.missing("kind", &format!("one of {}", kinds()))),
    };
    let (at, rotate, scale) = placement(cx, &mut tab)?;
    let keep_transform = tab.or(cx, "keep_transform", false, BOOL)?;
    let parent = tab.opt(cx, "parent", STR)?;
    let material = tab.opt(cx, "material", STR)?;
    let materials = tab.or(cx, "materials", Vec::new(), STRS)?;
    let smooth_angle = tab.opt(cx, "smooth_angle", NUM)?;
    let smooth = tab.or(cx, "smooth", false, BOOL)?;
    let flat_faces = tab.or(cx, "flat_faces", false, BOOL)?;
    let bevel = read_bevel(cx, &mut tab)?;
    let uv = read_uv(cx, &mut tab)?;
    let detail_uv = read_detail_uv(cx, &mut tab)?;
    let shadow = pick(
        &mut tab,
        cx,
        "shadow",
        Shadow::Cast,
        &[
            ("cast", Shadow::Cast),
            ("only", Shadow::Only),
            ("none", Shadow::None),
        ],
    )?;
    let two_sided = tab.or(cx, "two_sided", false, BOOL)?;
    let hidden = tab.or(cx, "hidden", false, BOOL)?;
    let dynamic = tab.opt(cx, "dynamic", BOOL)?;
    let proxy = match tab.peek(cx, "proxy") {
        Some(item) => Some(choice(
            cx,
            &tab.kpath("proxy"),
            item,
            &[("box", Proxy::Box), ("hull", Proxy::Hull)],
        )?),
        None => None,
    };
    if kind_name != "mesh"
        && let Some(item) = tab.retired("morph")
    {
        return Err(Error::new(
            tab.kpath("morph"),
            Some(cx.item_line(item)),
            format!(
                "`morph` sets the weights of an imported mesh's morph targets, so it goes with kind = \"mesh\", found \"{kind_name}\""
            ),
        ));
    }
    let lod = read_lod(cx, &mut tab, &kind_name)?;
    let kind = read_kind(cx, &mut tab, &kind_name)?;
    tab.finish(cx)?;
    Ok(Object {
        name,
        id,
        kind,
        at,
        rotate,
        scale,
        keep_transform,
        parent,
        material,
        materials,
        smooth_angle,
        smooth,
        flat_faces,
        bevel,
        uv,
        detail_uv,
        shadow,
        two_sided,
        hidden,
        dynamic,
        proxy,
        lod,
    })
}

fn read_lod(cx: &mut Cx, tab: &mut Tab<'_>, kind: &str) -> Result<Vec<Lod>, Error> {
    let path = tab.kpath("lod");
    let ipath = tab.kipath("lod");
    let Some(item) = tab.peek(cx, "lod") else {
        return Ok(Vec::new());
    };
    if matches!(kind, "pocket" | "text") {
        return Err(Error::new(
            path,
            Some(cx.item_line(item)),
            format!("a {kind} carries no mesh of its own, so it takes no `lod`"),
        ));
    }
    let tables = table_list(cx, &path, item)?;
    if tables.is_empty() {
        return Err(Error::new(
            path,
            Some(cx.item_line(item)),
            "expected at least one level, such as [{ triangles = 5000 }]",
        ));
    }
    let mut levels = Vec::new();
    for (i, (table, line)) in tables.into_iter().enumerate() {
        let at = format!("{path}[{i}]");
        let mut sub = Tab::new(cx, table, at.clone(), format!("{ipath}#{i}"), line);
        let triangles = sub.opt(cx, "triangles", UINT)?;
        let ratio = sub.opt(cx, "ratio", NUM)?;
        let bake_normals = sub.or(cx, "bake_normals", false, BOOL)?;
        let size = sub.opt(cx, "normal_size", UINT)?;
        let bake_distance = sub.opt(cx, "bake_distance", NUM)?;
        sub.finish(cx)?;
        let fail =
            |key: &str, text: &str| Error::new(dotted(&at, key), Some(line), text.to_string());
        let target = match (triangles, ratio) {
            (Some(n), None) if n > 0 => LodTarget::Triangles(n),
            (Some(_), None) => return Err(fail("triangles", "expected a whole number from 1 up")),
            (None, Some(r)) if r > 0.0 && r < 1.0 => LodTarget::Ratio(r),
            (None, Some(_)) => {
                return Err(fail("ratio", "expected a number above 0 and below 1"));
            }
            (Some(_), Some(_)) => {
                return Err(fail(
                    "ratio",
                    "a level takes `triangles` or `ratio`, not both",
                ));
            }
            (None, None) => {
                return Err(Error::new(
                    at,
                    Some(line),
                    "missing, expected `triangles` (a count) or `ratio` (of the object's triangles)",
                ));
            }
        };
        if !bake_normals && (size.is_some() || bake_distance.is_some()) {
            let key = if size.is_some() {
                "normal_size"
            } else {
                "bake_distance"
            };
            return Err(fail(key, "goes with bake_normals = true"));
        }
        let normal_size = size.unwrap_or(NORMAL_SIZE);
        if !(16..=8192).contains(&normal_size) {
            return Err(fail(
                "normal_size",
                "expected a whole number from 16 to 8192",
            ));
        }
        if let Some(d) = bake_distance
            && (d <= 0.0 || !d.is_finite())
        {
            return Err(fail("bake_distance", "expected a distance above 0"));
        }
        levels.push(Lod {
            target,
            bake_normals,
            normal_size,
            bake_distance,
        });
    }
    Ok(levels)
}

type Placement = ([f64; 3], [f64; 3], [f64; 3]);

fn placement(cx: &mut Cx, tab: &mut Tab<'_>) -> Result<Placement, Error> {
    let at = vec3_or_px(tab, cx, "at", "at_px")?.unwrap_or([0.0; 3]);
    let rotate = tab.or(cx, "rotate", [0.0; 3], V3)?;
    let scale = match tab.peek(cx, "scale") {
        Some(item) if item.is_array() => as_n::<3>(cx, &tab.kpath("scale"), item)?,
        Some(item) => {
            let s = as_num(cx, &tab.kpath("scale"), item)?;
            [s, s, s]
        }
        None => [1.0; 3],
    };
    Ok((at, rotate, scale))
}

fn read_use(cx: &mut Cx, t: &dyn TableLike, index: usize, line: usize) -> Result<Use, Error> {
    let prefix_of = t.get("prefix").and_then(Item::as_str).unwrap_or_default();
    let path = include::use_path(prefix_of, index);
    let mut tab = Tab::new(cx, t, path, format!("use#{index}"), line);
    let file = tab.req(cx, "file", STR)?;
    let prefix = tab.or(cx, "prefix", String::new(), STR)?;
    let (at, rotate, scale) = placement(cx, &mut tab)?;
    let parent = tab.opt(cx, "parent", STR)?;
    tab.finish(cx)?;
    Ok(Use {
        prefix,
        file,
        at,
        rotate,
        scale,
        parent,
        objects: Vec::new(),
    })
}

fn sin_cos(degrees: f64) -> (f64, f64) {
    let quarter = degrees / 90.0;
    if quarter.fract() == 0.0 {
        match quarter.rem_euclid(4.0) as u8 {
            0 => (0.0, 1.0),
            1 => (1.0, 0.0),
            2 => (0.0, -1.0),
            _ => (-1.0, 0.0),
        }
    } else {
        degrees.to_radians().sin_cos()
    }
}

type Mat = [[f64; 3]; 3];

fn times(a: &Mat, b: &Mat) -> Mat {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}

fn turn(degrees: [f64; 3]) -> Mat {
    let (sx, cx) = sin_cos(degrees[0]);
    let (sy, cy) = sin_cos(degrees[1]);
    let (sz, cz) = sin_cos(degrees[2]);
    let x = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let y = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let z = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    times(&z, &times(&y, &x))
}

fn degrees_of(m: &Mat) -> [f64; 3] {
    let y = (-m[2][0]).clamp(-1.0, 1.0).asin();
    if y.cos() > 1e-9 {
        [
            m[2][1].atan2(m[2][2]).to_degrees(),
            y.to_degrees(),
            m[1][0].atan2(m[0][0]).to_degrees(),
        ]
    } else {
        [0.0, y.to_degrees(), (-m[0][1]).atan2(m[1][1]).to_degrees()]
    }
}

fn one_axis(r: [f64; 3]) -> Option<usize> {
    let turned: Vec<usize> = (0..3).filter(|&i| r[i] != 0.0).collect();
    (turned.len() == 1).then(|| turned[0])
}

fn turned(outer: [f64; 3], inner: [f64; 3]) -> [f64; 3] {
    if outer == [0.0; 3] {
        return inner;
    }
    if inner == [0.0; 3] {
        return outer;
    }
    match (one_axis(outer), one_axis(inner)) {
        (Some(a), Some(b)) if a == b => {
            let mut sum = [0.0; 3];
            sum[a] = outer[a] + inner[a];
            sum
        }
        _ => degrees_of(&times(&turn(outer), &turn(inner))),
    }
}

pub(crate) fn compose(outer: Placement, inner: Placement) -> Result<Placement, String> {
    let (oa, or, os) = outer;
    let (ia, ir, is) = inner;
    if !(os[0] == os[1] && os[1] == os[2]) && ir != [0.0; 3] {
        return Err(
            "a scale that differs between axes places only unrotated objects, and this copy turns"
                .to_string(),
        );
    }
    let m = turn(or);
    let local = [os[0] * ia[0], os[1] * ia[1], os[2] * ia[2]];
    let at = std::array::from_fn(|r| {
        oa[r]
            + if or == [0.0; 3] {
                local[r]
            } else {
                (0..3).map(|c| m[r][c] * local[c]).sum()
            }
    });
    Ok((at, turned(or, ir), std::array::from_fn(|k| os[k] * is[k])))
}

fn live<'a>(root: &'a Table, stored: &'a Table) -> &'a Table {
    let prefix = stored.get("prefix").and_then(Item::as_str);
    if prefix.is_none_or(str::is_empty) {
        return stored;
    }
    root.get("use")
        .and_then(Item::as_array_of_tables)
        .and_then(|list| {
            list.iter()
                .find(|t| t.get("prefix").and_then(Item::as_str) == prefix)
        })
        .unwrap_or(stored)
}

fn placed(cx: &mut Cx, root: &Table, objects: &mut [Object]) -> Result<(), Error> {
    for (i, object) in objects.iter_mut().enumerate() {
        let Some(chain) = cx.copies.get(&object.name).cloned() else {
            continue;
        };
        let follows = match &object.kind {
            Kind::Pocket { .. } => true,
            Kind::Text { on, space, .. } => on.is_some() || *space == TextSpace::Overlay,
            _ => false,
        };
        if follows {
            continue;
        }
        for (stored, top) in &chain {
            let u = if *top { live(root, stored) } else { stored };
            let line = cx.line(u.span());
            let prefix = u.get("prefix").and_then(Item::as_str).unwrap_or_default();
            let path = if prefix.is_empty() {
                "use".to_string()
            } else {
                format!("use[{prefix}]")
            };
            let mut tab = Tab::new(cx, u, path.clone(), format!("object#{i}.use"), line);
            let outer = placement(cx, &mut tab)?;
            let inner = (object.at, object.rotate, object.scale);
            (object.at, object.rotate, object.scale) = compose(outer, inner).map_err(|text| {
                Error::new(
                    format!("{path}.scale"),
                    Some(line),
                    format!("{text}: `{}`", object.name),
                )
            })?;
        }
    }
    Ok(())
}

fn relocated(cx: &Cx, key: &str, path: &mut String) {
    let Some(&line) = cx.lines.get(key) else {
        return;
    };
    let folder = cx.src.folder_at(line);
    if folder.is_empty() || Path::new(path.as_str()).is_absolute() {
        return;
    }
    *path = include::joined(folder, path);
}

fn relocate(cx: &Cx, objects: &mut [Object], sky: &mut Option<Sky>) {
    for (i, object) in objects.iter_mut().enumerate() {
        match &mut object.kind {
            Kind::Card {
                image: Some(image), ..
            } => relocated(cx, &format!("object#{i}.image"), image),
            Kind::Mesh { path, .. } => relocated(cx, &format!("object#{i}.path"), path),
            Kind::Text { spans, .. } => {
                for (j, span) in spans.iter_mut().enumerate() {
                    if let Some(family) = &mut span.family
                        && family != crate::MONO
                    {
                        relocated(cx, &format!("object#{i}.spans#{j}.family"), family);
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(Sky {
        path: Some(path), ..
    }) = sky
    {
        relocated(cx, "sky.path", path);
    }
}

fn read_camera(cx: &mut Cx, t: &dyn TableLike, index: usize, line: usize) -> Result<Camera, Error> {
    let path = format!("camera[{}]", label(t, "name", index));
    let mut tab = Tab::new(cx, t, path, format!("camera#{index}"), line);
    let name = tab.req(cx, "name", STR)?;
    let kind = pick(
        &mut tab,
        cx,
        "kind",
        Projection::Perspective,
        &[
            ("perspective", Projection::Perspective),
            ("orthographic", Projection::Orthographic),
        ],
    )?;
    let rig = read_rig(cx, &mut tab)?;
    let (position, target) = match &rig {
        Some(rig) => (
            tab.or(cx, "position", rig.offset, V3)?,
            tab.or(cx, "target", rig.look, V3)?,
        ),
        None => (tab.req(cx, "position", V3)?, tab.req(cx, "target", V3)?),
    };
    let up = tab.or(cx, "up", [0.0, 1.0, 0.0], V3)?;
    let fov_y_deg = tab.opt(cx, "fov_y_deg", NUM)?;
    let focal_mm = tab.opt(cx, "focal_mm", NUM)?;
    let sensor_mm = tab.opt(cx, "sensor_mm", NUM)?;
    let shift = tab.or(cx, "shift", [0.0, 0.0], V2)?;
    let focus_m = tab.opt(cx, "focus_m", NUM)?;
    let fstop = tab.or(cx, "fstop", 0.0, NUM)?;
    let ortho_height = tab.opt(cx, "ortho_height", NUM)?;
    let path = read_path(cx, &mut tab)?;
    tab.finish(cx)?;
    Ok(Camera {
        name,
        kind,
        position,
        target,
        up,
        fov_y_deg,
        focal_mm,
        sensor_mm,
        shift,
        focus_m,
        fstop,
        ortho_height,
        path,
        rig,
    })
}

const U64: Rd<u64> = Rd("a whole number", as_u64);
const KEYS: Rd<Vec<[f64; 2]>> = Rd("an array of [t, value] pairs", as_points::<2>);

fn read_rig(cx: &mut Cx, tab: &mut Tab<'_>) -> Result<Option<Rig>, Error> {
    let Some(mut sub) = tab.sub(cx, "rig")? else {
        return Ok(None);
    };
    let follow = sub.req(cx, "follow", STR)?;
    let offset = sub.or(cx, "offset", RIG_OFFSET, V3)?;
    let look = sub.or(cx, "look", RIG_LOOK, V3)?;
    let lag = sub.or(cx, "lag", 0.0, NUM)?;
    let look_ahead = sub.or(cx, "look_ahead", 0.0, NUM)?;
    let yaw_lag = sub.or(cx, "yaw_lag", 0.0, NUM)?;
    let bank = sub.or(cx, "bank", 0.0, NUM)?;
    let shake = match sub.sub(cx, "shake")? {
        Some(mut shake) => {
            let read = RigShake {
                translation: shake.or(cx, "translation", 0.0, NUM)?,
                roll: shake.or(cx, "roll", 0.0, NUM)?,
                frequency: shake.or(cx, "frequency", 2.0, NUM)?,
                seed: shake.opt(cx, "seed", U64)?,
            };
            shake.finish(cx)?;
            Some(read)
        }
        None => None,
    };
    let path = sub.kpath("focus");
    let focus = match sub.peek(cx, "focus") {
        None => None,
        Some(item) if item.as_str() == Some("follow") => Some(RigFocus::Follow),
        Some(item) => match item
            .as_float()
            .or_else(|| item.as_integer().map(|i| i as f64))
        {
            Some(metres) => Some(RigFocus::Distance(metres)),
            None => {
                return Err(bad_item(
                    cx,
                    &path,
                    item,
                    "\"follow\" or a distance in metres",
                ));
            }
        },
    };
    let tracks = match sub.sub(cx, "tracks")? {
        Some(mut keyed) => {
            let mut read = |name: &'static str| keyed.opt(cx, name, KEYS);
            let tracks = RigTracks {
                distance: read("distance")?,
                lateral: read("lateral")?,
                height: read("height")?,
                bank: read("bank")?,
                lag: read("lag")?,
                shake: read("shake")?,
                look_ahead: read("look_ahead")?,
            };
            keyed.finish(cx)?;
            tracks
        }
        None => RigTracks::default(),
    };
    let bank_spring = match sub.sub(cx, "bank_spring")? {
        Some(mut spring) => {
            let read = Spring {
                frequency: spring.or(cx, "frequency", 1.5, NUM)?,
                damping: spring.or(cx, "damping", 0.5, NUM)?,
            };
            spring.finish(cx)?;
            Some(read)
        }
        None => None,
    };
    let release = sub.opt(cx, "release", NUM)?;
    let release_ease = sub.or(cx, "release_ease", RELEASE_EASE, NUM)?;
    sub.finish(cx)?;
    Ok(Some(Rig {
        follow,
        offset,
        look,
        lag,
        look_ahead,
        yaw_lag,
        bank,
        shake,
        focus,
        tracks,
        bank_spring,
        release,
        release_ease,
    }))
}

fn read_path(cx: &mut Cx, tab: &mut Tab<'_>) -> Result<Option<CameraPath>, Error> {
    let Some(mut sub) = tab.sub(cx, "path")? else {
        return Ok(None);
    };
    let orbit = pick_req(&mut sub, cx, "kind", &[("orbit", true), ("keys", false)])?;
    let ease = pick(
        &mut sub,
        cx,
        "ease",
        Ease::Linear,
        &[("linear", Ease::Linear), ("smooth", Ease::Smooth)],
    )?;
    let kind = if orbit {
        PathKind::Orbit(Orbit {
            target: sub.req(cx, "target", V3)?,
            turns: sub.req(cx, "turns", NUM)?,
            rise_deg: sub.or(cx, "rise_deg", 0.0, NUM)?,
            radius_scale: sub.or(cx, "radius_scale", 1.0, NUM)?,
        })
    } else {
        let path = sub.kpath("keys");
        let ipath = sub.kipath("keys");
        let Some(item) = sub.peek(cx, "keys") else {
            return Err(sub.missing("keys", "an array of tables with t, position and target"));
        };
        let mut keys = Vec::new();
        for (i, (table, line)) in table_list(cx, &path, item)?.into_iter().enumerate() {
            let mut key = Tab::new(
                cx,
                table,
                format!("{path}[{i}]"),
                format!("{ipath}#{i}"),
                line,
            );
            keys.push(PathKey {
                t: key.req(cx, "t", NUM)?,
                position: key.req(cx, "position", V3)?,
                target: key.req(cx, "target", V3)?,
                focal_mm: key.opt(cx, "focal_mm", NUM)?,
            });
            key.finish(cx)?;
        }
        PathKind::Keys(keys)
    };
    sub.finish(cx)?;
    Ok(Some(CameraPath { kind, ease }))
}

fn read_sun(cx: &mut Cx, root: &mut Tab<'_>) -> Result<(Option<Sun>, Option<Daylight>), Error> {
    if let Some(item) = root.retired("daylight") {
        return Err(Error::new(
            root.kpath("daylight"),
            Some(cx.item_line(item)),
            "`[daylight]` is part of `[sun]` now, expected `[sun]` with `model = \"daylight\"` and the same keys",
        ));
    }
    let Some(mut tab) = root.sub(cx, "sun")? else {
        return Ok((None, None));
    };
    let Some(item) = tab.peek(cx, "model") else {
        return Err(tab.missing(
            "model",
            "\"daylight\" or \"authored\": `toward`, `color` and `irradiance` are `model = \"authored\"`, `hour`, `day`, `latitude` and `heading` are `model = \"daylight\"`",
        ));
    };
    let model = choice(
        cx,
        &tab.kpath("model"),
        item,
        &[("daylight", true), ("authored", false)],
    )?;
    if model {
        let hour = tab.req(cx, "hour", NUM)?;
        let day = tab.or(cx, "day", 172, UINT)?;
        let latitude = tab.req(cx, "latitude", NUM)?;
        let heading = tab.req(cx, "heading", NUM)?;
        let reference_hour = tab.or(cx, "reference_hour", REFERENCE_HOUR, NUM)?;
        tab.finish(cx)?;
        return Ok((
            None,
            Some(Daylight {
                hour,
                day,
                latitude,
                heading,
                reference_hour,
            }),
        ));
    }
    if let Some(item) = tab.peek(cx, "hour") {
        return Err(Error::new(
            tab.kpath("hour"),
            Some(cx.item_line(item)),
            "an authored sun takes the hour of the shot or the window, expected `hour` removed",
        ));
    }
    let toward = tab.req(cx, "toward", V3)?;
    let radius_deg = tab.or(cx, "radius_deg", 0.27, NUM)?;
    let color = tab.or(cx, "color", [1.0, 1.0, 1.0], V3)?;
    let irradiance = tab.req(cx, "irradiance", NUM)?;
    tab.finish(cx)?;
    Ok((
        Some(Sun {
            toward,
            radius_deg,
            color,
            irradiance,
        }),
        None,
    ))
}

fn read_sky(cx: &mut Cx, root: &mut Tab<'_>) -> Result<Option<Sky>, Error> {
    let Some(mut tab) = root.sub(cx, "sky")? else {
        return Ok(None);
    };
    if let Some(item) = tab.peek(cx, "light") {
        return Err(Error::new(
            tab.kpath("light"),
            Some(cx.item_line(item)),
            "`[[sky.light]]` moved under `[sky.room]`, expected the lamps as `[[sky.room.lights]]`",
        ));
    }
    let kind = pick_req(
        &mut tab,
        cx,
        "kind",
        &[
            ("hdr", SkyKind::Hdr),
            ("analytic", SkyKind::Analytic),
            ("room", SkyKind::Room),
            ("mix", SkyKind::Mix),
        ],
    )?;
    let path = tab.opt(cx, "path", STR)?;
    let source = tab.opt(cx, "source", STR)?;
    if let Some(item) = tab.retired("balance") {
        return Err(Error::new(
            tab.kpath("balance"),
            Some(cx.item_line(item)),
            "`balance` moved into `[sky.prepare]`, expected `[sky.prepare]` with `balance = [r, g, b]`",
        ));
    }
    let (prepare, balance) = read_prepare(cx, &mut tab)?;
    let rotation_deg = tab.or(cx, "rotation_deg", 0.0, NUM)?;
    let intensity = tab.or(cx, "intensity", 1.0, NUM)?;
    let fill = tab.opt(cx, "fill", NUM)?;
    let ambient = pick(
        &mut tab,
        cx,
        "ambient",
        None,
        &[("authored", Some(SkyAmbient::Authored))],
    )?;
    let mut shade = BTreeMap::new();
    if let Some(mut sub) = tab.sub(cx, "shade")? {
        if let Some(item) = sub.peek(cx, "hours") {
            let hours_path = sub.kpath("hours");
            let Some(table) = item.as_table_like() else {
                return Err(bad_item(
                    cx,
                    &hours_path,
                    item,
                    "a table of hour = [r, g, b]",
                ));
            };
            for (key, value) in table.iter() {
                let at = format!("{hours_path}.{key}");
                let line = cx.item_line(value);
                let hour: u32 = key.parse().map_err(|_| {
                    Error::new(&at, Some(line), "expected an hour from 0 to 24 as the key")
                })?;
                if hour > 24 {
                    return Err(Error::new(
                        &at,
                        Some(line),
                        "expected an hour from 0 to 24 as the key",
                    ));
                }
                shade.insert(hour, as_n::<3>(cx, &at, value)?);
            }
        }
        sub.finish(cx)?;
    }
    let room = match tab.sub(cx, "room")? {
        Some(mut sub) => {
            let room = read_room(cx, &mut sub)?;
            sub.finish(cx)?;
            Some(room)
        }
        None => None,
    };
    tab.finish(cx)?;
    Ok(Some(Sky {
        kind,
        path,
        source,
        prepare,
        rotation_deg,
        intensity,
        balance,
        fill,
        ambient,
        shade,
        room,
    }))
}

fn read_prepare(
    cx: &mut Cx,
    tab: &mut Tab<'_>,
) -> Result<(Option<String>, Option<[f64; 3]>), Error> {
    if let Some(item) = tab.retired("prepare")
        && item.as_table_like().is_none()
    {
        return Err(Error::new(
            tab.kpath("prepare"),
            Some(cx.item_line(item)),
            "`prepare` is a table now, expected `[sky.prepare]` with `cap = 60.0` and `mean = 0.3` (and `balance = [r, g, b]` when the sky has one)",
        ));
    }
    let Some(mut sub) = tab.sub(cx, "prepare")? else {
        return Ok((None, None));
    };
    let cap = sub.opt(cx, "cap", NUM)?;
    let balance = sub.opt(cx, "balance", V3)?;
    let mean = sub.opt(cx, "mean", NUM)?;
    if cap.is_none() && balance.is_none() && mean.is_none() {
        return Err(Error::new(
            sub.path.clone(),
            Some(sub.line),
            "an empty table, expected one of `cap`, `balance` and `mean`",
        ));
    }
    let prepare = match (cap, mean) {
        (None, None) => None,
        (Some(cap), Some(mean)) if cap == CAP_LUMINANCE && mean == CAP_MEAN => {
            Some(CAPPED.to_string())
        }
        _ => {
            return Err(Error::new(
                sub.path.clone(),
                Some(sub.line),
                format!(
                    "Manfred prepares an HDR at one cap and mean, expected `cap = {CAP_LUMINANCE:?}` and `mean = {CAP_MEAN:?}` together, or neither"
                ),
            ));
        }
    };
    sub.finish(cx)?;
    Ok((prepare, balance))
}

fn read_room(cx: &mut Cx, tab: &mut Tab<'_>) -> Result<Room, Error> {
    let width = tab.or(cx, "width", 1024, UINT)?;
    let floor = tab.or(cx, "floor", [0.16, 0.10, 0.06], V3)?;
    let wall = tab.or(cx, "wall", [0.30, 0.24, 0.18], V3)?;
    let ceiling = tab.or(cx, "ceiling", [0.55, 0.47, 0.38], V3)?;
    let lights = tables(cx, tab, "lights", read_lamp)?;
    Ok(Room {
        width,
        floor,
        wall,
        ceiling,
        lights,
    })
}

fn read_lamp(cx: &mut Cx, t: &dyn TableLike, index: usize, line: usize) -> Result<SkyLight, Error> {
    let path = format!("sky.room.lights[{}]", label(t, "name", index));
    let mut sub = Tab::new(cx, t, path, format!("sky.room.lights#{index}"), line);
    let lamp = SkyLight {
        name: sub.or(cx, "name", String::new(), STR)?,
        az: sub.or(cx, "az", 0.0, NUM)?,
        el: sub.or(cx, "el", 30.0, NUM)?,
        power: sub.or(cx, "power", 4.0, NUM)?,
        color: sub.or(cx, "color", [1.0, 0.9, 0.78], V3)?,
        width: sub.or(cx, "width", 30.0, NUM)?,
        height: sub.or(cx, "height", 20.0, NUM)?,
        soft: sub.or(cx, "soft", 0.08, NUM)?,
        slats: sub.or(cx, "slats", 0.0, NUM)?,
        open: sub.or(cx, "open", 1.0, NUM)?,
    };
    sub.finish(cx)?;
    Ok(lamp)
}

fn read_route(
    cx: &mut Cx,
    t: &dyn TableLike,
    index: usize,
    line: usize,
) -> Result<RouteSpec, Error> {
    let path = format!("route[{}]", label(t, "name", index));
    let mut tab = Tab::new(cx, t, path, format!("route#{index}"), line);
    let route = RouteSpec {
        name: tab.req(cx, "name", STR)?,
        points: tab.req(cx, "points", V3S)?,
        closed: tab.or(cx, "closed", false, BOOL)?,
    };
    tab.finish(cx)?;
    Ok(route)
}

fn read_stage(cx: &mut Cx, t: &dyn TableLike, index: usize, line: usize) -> Result<Stage, Error> {
    let path = format!("stage[{}]", label(t, "name", index));
    let mut tab = Tab::new(cx, t, path, format!("stage#{index}"), line);
    let stage = Stage {
        name: tab.req(cx, "name", STR)?,
        objects: tab.req(cx, "objects", STRS)?,
        camera: tab.opt(cx, "camera", STR)?,
    };
    tab.finish(cx)?;
    Ok(stage)
}

const ORTHO: [(&str, Ortho); 4] = [
    ("top", Ortho::Top),
    ("front", Ortho::Front),
    ("side", Ortho::Side),
    ("rear", Ortho::Rear),
];

fn read_reference(
    cx: &mut Cx,
    t: &dyn TableLike,
    index: usize,
    line: usize,
) -> Result<Reference, Error> {
    let path = format!("reference[{}]", label(t, "name", index));
    let mut tab = Tab::new(cx, t, path, format!("reference#{index}"), line);
    let name = tab.req(cx, "name", STR)?;
    let image = tab.req(cx, "image", STR)?;
    let view = pick_req(&mut tab, cx, "view", &ORTHO)?;
    let Some(mut scale) = tab.sub(cx, "scale")? else {
        return Err(tab.missing(
            "scale",
            "a table, { from = [[px, py], [px, py]], metres = <m> }",
        ));
    };
    let from = scale.req(cx, "from", PIXEL_PAIR)?;
    let metres = scale.req(cx, "metres", NUM)?;
    scale.finish(cx)?;
    let offset = tab.or(cx, "offset", [0.0, 0.0], V2)?;
    let opacity = tab.or(cx, "opacity", 0.5, NUM)?;
    tab.finish(cx)?;
    Ok(Reference {
        name,
        image,
        view,
        from,
        metres,
        offset,
        opacity,
    })
}

const PIXEL_PAIR: Rd<[[f64; 2]; 2]> = Rd("two [px, py] pixels", as_pixel_pair);

fn as_pixel_pair(cx: &mut Cx, path: &str, item: &Item) -> Result<[[f64; 2]; 2], Error> {
    let points = as_points::<2>(cx, path, item)?;
    match points.as_slice() {
        [a, b] => Ok([*a, *b]),
        _ => Err(Error::new(
            path,
            Some(cx.item_line(item)),
            "expected two [px, py] pixels on the image",
        )),
    }
}

fn missing_pair(tab: &Tab<'_>, key: &str, twin: &str, what: &str) -> Error {
    tab.missing(key, &format!("{what}, or `{twin}` in plate pixels"))
}

fn as_plane_px(cx: &mut Cx, path: &str, item: &Item) -> Result<[f64; 4], Error> {
    let plane = as_n::<4>(cx, path, item)?;
    if plane[0] != 0.0 || plane[2] != 0.0 || plane[1] == 0.0 {
        return Err(Error::new(
            path,
            Some(cx.item_line(item)),
            "expected a normal along the plate's y axis, [0, 1, 0] or [0, -1, 0], then a plate row",
        ));
    }
    Ok(plane)
}

fn plane_or_px(
    tab: &mut Tab<'_>,
    cx: &mut Cx,
    key: &'static str,
    twin_key: &'static str,
) -> Result<Option<[f64; 4]>, Error> {
    let px = Rd("a plane", as_plane_px);
    twin(
        tab,
        cx,
        (key, twin_key),
        V4,
        px,
        false,
        |plate, [nx, ny, nz, row]| {
            let offset = match plate {
                Some(plate) => ny * plate.to_metres([0.0, row])[1],
                None => ny * row,
            };
            [nx, ny, nz, offset]
        },
    )
}

fn read_mover(cx: &mut Cx, t: &dyn TableLike, index: usize, line: usize) -> Result<Mover, Error> {
    let path = format!("mover[{}]", label(t, "group", index));
    let mut tab = Tab::new(cx, t, path, format!("mover#{index}"), line);
    let mover = Mover {
        group: tab.req(cx, "group", STR)?,
        pivot: vec3_or_px(&mut tab, cx, "pivot", "pivot_px")?
            .ok_or_else(|| missing_pair(&tab, "pivot", "pivot_px", "an array of 3 numbers"))?,
        axis: tab.req(cx, "axis", V3)?,
        travel: length_or_px(&mut tab, cx, "travel", "travel_px")?
            .ok_or_else(|| missing_pair(&tab, "travel", "travel_px", "a number"))?,
        clip: plane_or_px(&mut tab, cx, "clip", "clip_px")?,
        clip2: plane_or_px(&mut tab, cx, "clip2", "clip2_px")?,
        parts: tab.or(cx, "parts", Vec::new(), STRS)?,
    };
    tab.finish(cx)?;
    Ok(mover)
}

const SPEED: Rd<Vec<[f64; 2]>> = Rd("an array of [t, m/s] pairs", as_points::<2>);

fn read_follow(cx: &mut Cx, t: &dyn TableLike, index: usize, line: usize) -> Result<Follow, Error> {
    let path = format!("follow[{}]", label(t, "object", index));
    let mut tab = Tab::new(cx, t, path, format!("follow#{index}"), line);
    let object = tab.req(cx, "object", STR)?;
    let route = tab.req(cx, "route", STR)?;
    let lane = tab.or(cx, "lane", 0.0, NUM)?;
    let start = tab.or(cx, "start", 0.0, NUM)?;
    let speed = tab.req(cx, "speed", SPEED)?;
    let drift = match tab.sub(cx, "drift")? {
        Some(mut sub) => {
            let drift = Drift {
                amount: sub.req(cx, "amount", NUM)?,
                period: sub.req(cx, "period", NUM)?,
                slip_deg: sub.or(cx, "slip_deg", 0.0, NUM)?,
            };
            sub.finish(cx)?;
            Some(drift)
        }
        None => None,
    };
    let ride = tab.or(cx, "ride", 0.0, NUM)?;
    tab.finish(cx)?;
    Ok(Follow {
        object,
        route,
        lane,
        start,
        speed,
        drift,
        ride,
    })
}

fn read_sidecar(
    cx: &mut Cx,
    root: &mut Tab<'_>,
) -> Result<serde_json::Map<String, serde_json::Value>, Error> {
    let Some(item) = root.peek(cx, "sidecar") else {
        return Ok(serde_json::Map::new());
    };
    match item.as_table_like() {
        Some(t) => Ok(json_map(t)),
        None => Err(bad_item(cx, "sidecar", item, "a table")),
    }
}

fn read_variants(cx: &mut Cx, root: &mut Tab<'_>) -> Result<BTreeMap<String, Variant>, Error> {
    let mut out = BTreeMap::new();
    let Some(item) = root.peek(cx, "variant") else {
        return Ok(out);
    };
    let Some(table) = item.as_table_like() else {
        return Err(bad_item(cx, "variant", item, "a table of variants"));
    };
    for (name, value) in table.iter() {
        let path = format!("variant.{name}");
        let Some(vt) = value.as_table_like() else {
            return Err(bad_item(cx, &path, value, "a table"));
        };
        let line = cx.item_line(value);
        let mut tab = Tab::new(cx, vt, path.clone(), path.clone(), line);
        let drop = tab.or(cx, "drop", Vec::new(), STRS)?;
        let set = match tab.peek(cx, "set") {
            Some(item) => match item.as_table_like() {
                Some(st) => json_map(st),
                None => {
                    return Err(bad_item(
                        cx,
                        &tab.kpath("set"),
                        item,
                        "a table of dotted keys",
                    ));
                }
            },
            None => BTreeMap::new(),
        };
        let remove = tab.or(cx, "remove", Vec::new(), STRS)?;
        let mut output = VariantOutput::default();
        if let Some(mut sub) = tab.sub(cx, "output")? {
            output.glb = sub.opt(cx, "glb", STR)?;
            output.sidecar = sub.opt(cx, "sidecar", STR)?;
            output.bake = sub.opt(cx, "bake", STR)?;
            output.scene = sub.opt(cx, "scene", STR)?;
            sub.finish(cx)?;
        }
        tab.finish(cx)?;
        out.insert(
            name.to_string(),
            Variant {
                drop,
                set,
                remove,
                output,
            },
        );
    }
    Ok(out)
}

fn read_look(cx: &mut Cx, root: &mut Tab<'_>) -> Result<Option<Look>, Error> {
    if let Some(item) = root.retired("finish") {
        return Err(Error::new(
            root.kpath("finish"),
            Some(cx.item_line(item)),
            "`[finish]` is the scene file's table, expected the recipe's own `[look]` with `view`, `exposure` and `grade`; the scene file's `[finish]` is written from it",
        ));
    }
    let Some(mut tab) = root.sub(cx, "look")? else {
        return Ok(None);
    };
    let view = pick(
        &mut tab,
        cx,
        "view",
        View::Graded,
        &[("graded", View::Graded), ("standard", View::Standard)],
    )?;
    let exposure = tab.or(cx, "exposure", 1.0, NUM)?;
    let lut = tab.opt(cx, "lut", STR)?;
    if let Some(file) = &lut
        && (!file.ends_with(".cube") || file.len() == ".cube".len())
    {
        return Err(Error::new(
            tab.kpath("lut"),
            cx.lines.get(&tab.kipath("lut")).copied(),
            format!("expected a path to a .cube file, found \"{file}\""),
        ));
    }
    let lut_amount = tab.opt(cx, "lut_amount", NUM)?;
    if let Some(amount) = lut_amount {
        let path = tab.kpath("lut_amount");
        let line = cx.lines.get(&tab.kipath("lut_amount")).copied();
        if lut.is_none() {
            return Err(Error::new(
                path,
                line,
                "expected a `lut` beside `lut_amount`",
            ));
        }
        if !(0.0..=1.0).contains(&amount) {
            return Err(Error::new(
                path,
                line,
                format!("expected a number from 0 to 1, found {amount}"),
            ));
        }
    }
    let grade = match tab.sub(cx, "grade")? {
        Some(mut sub) => {
            let grade = Grade {
                vignette: sub.or(cx, "vignette", 0.0, NUM)?,
                saturation: sub.or(cx, "saturation", 1.0, NUM)?,
                warmth: sub.or(cx, "warmth", 0.0, NUM)?,
                bloom: sub.or(cx, "bloom", 0.0, NUM)?,
                haze: sub.or(cx, "haze", 0.0, NUM)?,
            };
            sub.finish(cx)?;
            grade
        }
        None => Grade {
            vignette: 0.0,
            saturation: 1.0,
            warmth: 0.0,
            bloom: 0.0,
            haze: 0.0,
        },
    };
    tab.finish(cx)?;
    Ok(Some(Look {
        view,
        exposure,
        grade,
        lut,
        lut_amount,
    }))
}

fn read_trace(cx: &mut Cx, root: &mut Tab<'_>) -> Result<Trace, Error> {
    let Some(mut tab) = root.sub(cx, "trace")? else {
        return Ok(Trace::default());
    };
    let transmissive_shadows = tab.or(cx, "transmissive_shadows", false, BOOL)?;
    let clamp_indirect = tab.or(cx, "clamp_indirect", 0.0, NUM)?;
    let filter_glossy = tab.or(cx, "filter_glossy", 0.0, NUM)?;
    let spp = tab.opt(cx, "spp", UINT)?;
    let threshold = tab.opt(cx, "threshold", NUM)?;
    let min_samples = tab.opt(cx, "min_samples", UINT)?;
    tab.finish(cx)?;
    Ok(Trace {
        transmissive_shadows,
        clamp_indirect,
        filter_glossy,
        spp,
        threshold,
        min_samples,
    })
}

fn read_timeline(cx: &mut Cx, root: &mut Tab<'_>, seed: u64) -> Result<Option<Timeline>, Error> {
    let Some(mut tab) = root.sub(cx, "timeline")? else {
        return Ok(None);
    };
    let at = |cx: &Cx, tab: &Tab<'_>, key: &str| {
        cx.lines.get(&tab.kipath(key)).copied().or(Some(tab.line))
    };
    let fps = tab.or(cx, "fps", Timeline::FPS, UINT)?;
    if !(1..=Timeline::MAX_FPS).contains(&fps) {
        return Err(Error::new(
            tab.kpath("fps"),
            at(cx, &tab, "fps"),
            format!("expected frames per second from 1 to {}", Timeline::MAX_FPS),
        ));
    }
    let seconds = tab.opt(cx, "seconds", NUM)?;
    let frames = tab.opt(cx, "frames", UINT)?;
    let frames = match (seconds, frames) {
        (Some(_), Some(_)) => {
            return Err(Error::new(
                tab.kpath("frames"),
                at(cx, &tab, "frames"),
                "`seconds` and `frames` both give the length, expected one of them",
            ));
        }
        (None, None) => return Err(tab.missing("seconds", "the length, `seconds` or `frames`")),
        (Some(seconds), None) => {
            if !(seconds.is_finite() && seconds > 0.0) {
                return Err(Error::new(
                    tab.kpath("seconds"),
                    at(cx, &tab, "seconds"),
                    "expected a number above 0",
                ));
            }
            (seconds * f64::from(fps)).round().max(1.0)
        }
        (None, Some(frames)) => {
            if frames == 0 {
                return Err(Error::new(
                    tab.kpath("frames"),
                    at(cx, &tab, "frames"),
                    "expected a whole number from 1 up",
                ));
            }
            f64::from(frames)
        }
    };
    if frames > f64::from(Timeline::MAX_FRAMES) {
        let key = if seconds.is_some() {
            "seconds"
        } else {
            "frames"
        };
        return Err(Error::new(
            tab.kpath(key),
            at(cx, &tab, key),
            format!(
                "comes to {frames} frames, expected at most {}",
                Timeline::MAX_FRAMES
            ),
        ));
    }
    let size = tab.or(cx, "size", Timeline::SIZE, U2)?;
    if size
        .iter()
        .any(|&side| !(1..=Timeline::MAX_SIDE).contains(&side))
    {
        return Err(Error::new(
            tab.kpath("size"),
            at(cx, &tab, "size"),
            format!(
                "expected [width, height] with each side from 1 to {}",
                Timeline::MAX_SIDE
            ),
        ));
    }
    let seed = match tab.peek(cx, "seed") {
        Some(item) => as_u64(cx, &tab.kpath("seed"), item)?,
        None => seed,
    };
    tab.finish(cx)?;
    Ok(Some(Timeline {
        fps,
        frames: frames as u32,
        size,
        seed,
    }))
}

fn read_bake(cx: &mut Cx, root: &mut Tab<'_>) -> Result<Option<Bake>, Error> {
    let Some(mut tab) = root.sub(cx, "bake")? else {
        return Ok(None);
    };
    let product = tab.opt(cx, "product", STR)?;
    let anchors = tab.req(cx, "anchors", NUMS)?;
    let samples = tab.req(cx, "samples", UINT)?;
    let volumes = tables(cx, &mut tab, "volume", read_volume)?;
    let plates = read_plates(cx, &mut tab)?;
    tab.finish(cx)?;
    Ok(Some(Bake {
        product,
        anchors,
        samples,
        volumes,
        plates,
    }))
}

fn read_plates(cx: &mut Cx, bake: &mut Tab<'_>) -> Result<Option<Plates>, Error> {
    let Some(mut tab) = bake.sub(cx, "plates")? else {
        return Ok(None);
    };
    let plates = Plates {
        size: tab.req(cx, "size", U2)?,
        overscan: tab.or(cx, "overscan", Plates::OVERSCAN, NUM)?,
        scale: tab.or(cx, "scale", Plates::SCALE, NUM)?,
        anchors: tab.opt(cx, "anchors", NUMS)?,
        layers: tab.opt(cx, "layers", STRS)?.unwrap_or_else(|| {
            Plates::LAYERS
                .iter()
                .map(|name| (*name).to_string())
                .collect()
        }),
        threshold: tab.or(cx, "threshold", Plates::THRESHOLD, NUM)?,
        min_samples: tab.or(cx, "min_samples", Plates::MIN_SAMPLES, UINT)?,
        max_samples: tab.or(cx, "max_samples", Plates::MAX_SAMPLES, UINT)?,
        growth: tab.or(cx, "growth", Plates::GROWTH, NUM)?,
        seed: tab.opt(cx, "seed", UINT)?,
        key: tab.opt(cx, "key", STR)?,
        dir: tab.opt(cx, "dir", STR)?,
        casters: pick(
            &mut tab,
            cx,
            "casters",
            Casters::Meshes,
            &[
                ("meshes", Casters::Meshes),
                ("proxies", Casters::Proxies),
                ("none", Casters::None),
            ],
        )?,
        clamp_indirect: tab.opt(cx, "clamp_indirect", NUM)?,
        filter_glossy: tab.opt(cx, "filter_glossy", NUM)?,
        views: tables(cx, &mut tab, "view", read_plate_view)?,
    };
    tab.finish(cx)?;
    Ok(Some(plates))
}

fn read_plate_view(
    cx: &mut Cx,
    t: &dyn TableLike,
    index: usize,
    line: usize,
) -> Result<PlateView, Error> {
    let path = format!("bake.plates.view[{}]", label(t, "name", index));
    let mut sub = Tab::new(cx, t, path, format!("bake.plates.view#{index}"), line);
    let view = PlateView {
        name: sub.req(cx, "name", STR)?,
        camera: sub.opt(cx, "camera", STR)?,
        at: sub.opt(cx, "at", V3)?,
        look_at: sub.opt(cx, "look_at", V3)?,
        up: sub.opt(cx, "up", V3)?,
        fov: sub.opt(cx, "fov", NUM)?,
        focal: sub.opt(cx, "focal", NUM)?,
        sensor: sub.opt(cx, "sensor", NUM)?,
        shift: sub.opt(cx, "shift", V2)?,
        near: sub.opt(cx, "near", NUM)?,
        far: sub.opt(cx, "far", NUM)?,
    };
    sub.finish(cx)?;
    Ok(view)
}

fn read_volume(cx: &mut Cx, t: &dyn TableLike, index: usize, line: usize) -> Result<Volume, Error> {
    let path = format!("bake.volume[{}]", label(t, "name", index));
    let mut sub = Tab::new(cx, t, path, format!("bake.volume#{index}"), line);
    let volume = Volume {
        name: sub.req(cx, "name", STR)?,
        min: sub.req(cx, "min", V3)?,
        max: sub.req(cx, "max", V3)?,
        spacing: sub.req(cx, "spacing", NUM)?,
    };
    sub.finish(cx)?;
    Ok(volume)
}

fn load_params(base: &Path, file: &str) -> Result<serde_json::Value, Error> {
    let at = base.join(file);
    let text = std::fs::read_to_string(&at).map_err(|e| {
        Error::new(
            "params",
            None,
            format!("cannot read {file}: {}", io_text(&e)),
        )
    })?;
    if file.ends_with(".json") {
        serde_json::from_str(&text)
            .map_err(|e| Error::new("params", None, format!("{file} is not valid JSON: {e}")))
    } else if file.ends_with(".toml") {
        let doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
            Error::new(
                "params",
                None,
                format!("{file} is not valid TOML: {}", e.message()),
            )
        })?;
        Ok(json_of_item(doc.as_item()))
    } else {
        Err(Error::new("params", None, "expected a .json or .toml file"))
    }
}

fn io_text(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "no such file".to_string(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        _ => e.to_string(),
    }
}

pub fn parse(text: &str) -> Result<Recipe, Error> {
    parse_with(text, None, "recipe")
}

pub fn materials(text: &str) -> Result<BTreeMap<String, serde_json::Value>, Error> {
    let src = Source::new(text);
    let doc = Document::parse(text.to_string()).map_err(|e| {
        let line = e.span().map(|s| src.line(s.start));
        Error::new("", line, format!("not valid TOML: {}", e.message()))
    })?;
    match doc.as_table().get("materials") {
        None => Ok(BTreeMap::new()),
        Some(item) => match item.as_table_like() {
            Some(table) => Ok(json_map(table)),
            None => Err(Error::new(
                "materials",
                None,
                "expected a table of `[materials.<name>]` tables",
            )),
        },
    }
}

pub fn load(path: &Path) -> Result<Loaded, Error> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        Error::new(
            path.display().to_string(),
            None,
            format!("cannot read the recipe: {}", io_text(&e)),
        )
    })?;
    crate::patch::reread(path, &text)
}

pub(crate) struct Main<'a> {
    pub text: &'a str,
    pub base: Option<&'a Path>,
    pub name: &'a str,
    pub memory: &'a BTreeMap<String, String>,
    pub edits: &'a [Edit],
}

pub(crate) fn read_main(m: Main<'_>) -> Result<Recipe, Error> {
    let main = File {
        name: m.name.to_string(),
        path: m.base.map(|b| b.join(m.name)),
        text: m.text.to_string(),
        role: Role::Main,
        at_line: 1,
        pad: 0,
    };
    let read = |name: &str| include::disk_reader(m.base, m.memory, name);
    let mut asm = include::assemble(main, &read)?;
    for edit in m.edits {
        edited(&mut asm.table, edit)?;
    }
    if m.edits.is_empty() || asm.list.len() > 1 {
        return read_recipe(asm, m.text, m.base, m.edits);
    }
    let mut doc: DocumentMut = m.text.parse().map_err(|e: toml_edit::TomlError| {
        Error::new("", None, format!("not valid TOML: {}", e.message()))
    })?;
    for edit in m.edits {
        edited(doc.as_table_mut(), edit)?;
    }
    read_recipe(asm, &doc.to_string(), m.base, m.edits)
}

fn edited(table: &mut Table, edit: &Edit) -> Result<(), Error> {
    match edit {
        Edit::Variant {
            name,
            recipe,
            output,
            variant,
        } => crate::variant::apply_to_table(table, name, recipe, output, variant.as_ref()),
        Edit::Set(sets) => crate::set::set_in(table, sets),
        Edit::Keep(globs) => {
            crate::stage::keep_in(table, globs);
            Ok(())
        }
    }
}

pub(crate) fn parse_with(text: &str, base: Option<&Path>, name: &str) -> Result<Recipe, Error> {
    read_main(Main {
        text,
        base,
        name,
        memory: &BTreeMap::new(),
        edits: &[],
    })
}

pub(crate) fn parse_doc(path: &Path, text: &str) -> Result<(Recipe, DocumentMut), Error> {
    let base = path
        .parent()
        .map_or_else(Default::default, Path::to_path_buf);
    let name = path.file_name().map_or_else(
        || "recipe".to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let recipe = parse_with(text, Some(&base), &name)?;
    let doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        Error::new("", None, format!("not valid TOML: {}", e.message()))
    })?;
    if !recipe.files.included() {
        return Ok((recipe, doc));
    }
    let memory = recipe.files.memory();
    let read = |name: &str| include::disk_reader(Some(&base), &memory, name);
    let view = include::view(&name, &doc, &read)?;
    Ok((recipe, view.doc))
}

fn read_recipe(
    asm: Assembly,
    text: &str,
    base: Option<&Path>,
    edits: &[Edit],
) -> Result<Recipe, Error> {
    let mut cx = Cx {
        src: Sources::of(&asm.list),
        params: ParamsState::Absent,
        lines: BTreeMap::new(),
        twins: Vec::new(),
        plate: None,
        copies: asm.copies,
    };
    match read_root(&mut cx, &asm.table, text, base) {
        Ok(mut recipe) => {
            recipe.uses = include::uses_of(&recipe.uses, &asm.uses);
            let (lines, origins) = cx.src.lines(std::mem::take(&mut cx.lines));
            recipe.lines = Lines(lines);
            recipe.twins = std::mem::take(&mut cx.twins)
                .into_iter()
                .map(|p| cx.src.problem(p))
                .collect();
            recipe.keep = edits
                .iter()
                .filter_map(|edit| match edit {
                    Edit::Keep(globs) => Some(globs.clone()),
                    _ => None,
                })
                .collect();
            recipe.files = Files {
                list: asm.list,
                edits: edits.to_vec(),
                origins,
                ..Files::default()
            };
            Ok(recipe)
        }
        Err(e) => Err(cx.src.error(e)),
    }
}

fn read_root(
    cx: &mut Cx,
    root_table: &Table,
    text: &str,
    base: Option<&Path>,
) -> Result<Recipe, Error> {
    let mut root = Tab::new(cx, root_table, String::new(), String::new(), 1);

    if let Some(item) = root.retired("manf") {
        return Err(Error::new("manf", Some(cx.item_line(item)), OLD_HEADER));
    }
    let manfred = match root.peek(cx, "manfred") {
        None => {
            return Err(Error::new(
                "manfred",
                Some(1),
                format!("missing, expected the schema version, `manfred = {SCHEMA}`"),
            ));
        }
        Some(item) => as_u64(cx, "manfred", item)?,
    };
    if manfred == 0 {
        return Err(Error::new(
            "manfred",
            cx.lines.get("manfred").copied(),
            "expected a schema version from 1 up",
        ));
    }
    if manfred > u64::from(SCHEMA) {
        return Err(Error::new(
            "manfred",
            cx.lines.get("manfred").copied(),
            format!(
                "schema {manfred} is newer than the {SCHEMA} this Manfred reads, update Manfred"
            ),
        ));
    }
    if let Some(item) = root.peek(cx, "include")
        && !item.is_array()
    {
        return Err(bad_item(cx, "include", item, "an array of file names"));
    }
    let name = root.req(cx, "name", STR)?;
    let seed = match root.peek(cx, "seed") {
        Some(item) => as_u64(cx, "seed", item)?,
        None => 0,
    };
    let params = root.opt(cx, "params", STR)?;
    let library = root.opt(cx, "library", STR)?;
    cx.params = match (&params, base) {
        (None, _) => ParamsState::Absent,
        (Some(file), Some(base)) => {
            let loaded = load_params(base, file).map_err(|mut e| {
                e.line = cx.lines.get("params").copied();
                e
            })?;
            ParamsState::Loaded(file.clone(), loaded)
        }
        (Some(file), None) => ParamsState::Unloaded(file.clone()),
    };
    let plate = read_plate(cx, &mut root)?;
    cx.plate = plate.clone();
    let output = read_output(cx, &mut root)?;

    let mut objects = tables(cx, &mut root, "object", read_object)?;
    placed(cx, root_table, &mut objects)?;
    let uses = tables(cx, &mut root, "use", read_use)?;
    let cameras = tables(cx, &mut root, "camera", read_camera)?;
    let (sun, daylight) = read_sun(cx, &mut root)?;
    let mut sky = read_sky(cx, &mut root)?;
    relocate(cx, &mut objects, &mut sky);
    let lights = tables(cx, &mut root, "light", effects::read_light)?;
    let movers = tables(cx, &mut root, "mover", read_mover)?;
    let routes = tables(cx, &mut root, "route", read_route)?;
    let mut references = tables(cx, &mut root, "reference", read_reference)?;
    for (i, reference) in references.iter_mut().enumerate() {
        relocated(cx, &format!("reference#{i}.image"), &mut reference.image);
    }
    let follows = tables(cx, &mut root, "follow", read_follow)?;
    let stages = tables(cx, &mut root, "stage", read_stage)?;
    let variants = read_variants(cx, &mut root)?;
    let sidecar = read_sidecar(cx, &mut root)?;
    let look = read_look(cx, &mut root)?;
    let trace = read_trace(cx, &mut root)?;
    let bake = read_bake(cx, &mut root)?;
    let timeline = read_timeline(cx, &mut root, seed)?;

    let mut wet = BTreeMap::new();
    let mut textures = BTreeMap::new();
    let materials = match root.peek(cx, "materials") {
        Some(item) => match item.as_table_like() {
            Some(table) => {
                for (name, entry) in table.iter() {
                    cx.lines
                        .insert(format!("materials.{name}"), cx.item_line(entry));
                    if let Some(keys) = entry.as_table_like() {
                        for (key, value) in keys.iter() {
                            cx.lines
                                .insert(format!("materials.{name}.{key}"), cx.item_line(value));
                        }
                        if let Some(item) = keys.get("wet") {
                            wet.insert(name.to_string(), effects::read_wet(cx, name, item)?);
                        }
                        if let Some(found) = read_textures(cx, name, keys)? {
                            textures.insert(name.to_string(), found);
                        }
                    }
                }
                let mut map: BTreeMap<String, serde_json::Value> = json_map(table);
                map.retain(|_, value| {
                    let Some(keys) = value.as_object_mut() else {
                        return true;
                    };
                    let had = keys.len();
                    keys.remove("wet");
                    for key in TEXTURE_KEYS {
                        keys.remove(key);
                    }
                    !keys.is_empty() || had == 0
                });
                map
            }
            None => return Err(bad_item(cx, "materials", item, "a table of materials")),
        },
        None => BTreeMap::new(),
    };
    let effects = effects::read_effects(cx, &mut root, wet)?;
    root.finish(cx)?;

    Ok(Recipe {
        manfred: manfred as u32,
        name,
        seed,
        params,
        library,
        plate,
        output,
        objects,
        cameras,
        sun,
        daylight,
        sky,
        lights,
        movers,
        routes,
        references,
        follows,
        stages,
        keep: Vec::new(),
        variants,
        sidecar,
        look,
        trace,
        bake,
        timeline,
        materials,
        effects,
        textures,
        uses,
        files: Files::default(),
        source: text.to_string(),
        base: base.map(Path::to_path_buf),
        lines: Lines::default(),
        twins: Vec::new(),
    })
}
