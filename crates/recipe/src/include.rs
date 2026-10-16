use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use toml_edit::{ArrayOfTables, Document, DocumentMut, Item, Table, Value};

use crate::read::Source;
use crate::{Error, OLD_HEADER, Output, Problem, Recipe, SCHEMA, Use, Variant};

pub(crate) const ARRAYS: [(&str, &str); 7] = [
    ("object", "name"),
    ("camera", "name"),
    ("light", "name"),
    ("particles", "name"),
    ("mover", "group"),
    ("use", "prefix"),
    ("route", "name"),
];
const SINGLE: [&str; 6] = ["sun", "sky", "look", "flare", "motion_blur", "haze"];
const INCLUDED: [&str; 16] = [
    "manfred",
    "include",
    "use",
    "object",
    "camera",
    "light",
    "particles",
    "mover",
    "sun",
    "sky",
    "look",
    "flare",
    "motion_blur",
    "haze",
    "materials",
    "route",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Main,
    Include,
    Part,
}

#[derive(Clone, Debug)]
pub(crate) struct File {
    pub name: String,
    pub path: Option<PathBuf>,
    pub text: String,
    pub role: Role,
    pub at_line: usize,
    pub pad: usize,
}

#[derive(Clone, Debug)]
pub(crate) enum Edit {
    Variant {
        name: String,
        recipe: String,
        output: Box<Output>,
        variant: Box<Variant>,
    },
    Set(Vec<String>),
}

#[derive(Clone, Debug, Default)]
pub struct Files {
    pub(crate) list: Vec<File>,
    pub(crate) edits: Vec<Edit>,
    pub(crate) origins: BTreeMap<String, (usize, usize)>,
    pub(crate) homes: BTreeMap<(String, String), String>,
    pub(crate) changed: BTreeMap<String, String>,
}

impl PartialEq for Files {
    fn eq(&self, _: &Files) -> bool {
        true
    }
}

impl Files {
    pub fn paths(&self) -> Vec<PathBuf> {
        self.list.iter().filter_map(|f| f.path.clone()).collect()
    }

    pub fn names(&self) -> Vec<&str> {
        self.list.iter().map(|f| f.name.as_str()).collect()
    }

    pub fn included(&self) -> bool {
        self.list.iter().any(|f| f.role == Role::Include)
    }

    pub fn main_path(&self) -> Option<&Path> {
        self.list.first().and_then(|f| f.path.as_deref())
    }

    pub fn variant(&self) -> Option<&str> {
        self.edits.iter().rev().find_map(|edit| match edit {
            Edit::Variant { name, .. } => Some(name.as_str()),
            Edit::Set(_) => None,
        })
    }

    pub(crate) fn main_name(&self) -> &str {
        self.list.first().map_or("recipe", |f| f.name.as_str())
    }

    pub(crate) fn main_text(&self) -> Option<&str> {
        self.list.first().map(|f| f.text.as_str())
    }

    pub(crate) fn memory(&self) -> BTreeMap<String, String> {
        self.list
            .iter()
            .skip(1)
            .map(|f| (f.name.clone(), f.text.clone()))
            .collect()
    }

    pub(crate) fn text_of(&self, name: &str) -> Option<&str> {
        self.list
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.text.as_str())
    }
}

impl Recipe {
    pub fn origin_of(&self, ipath: &str) -> Option<(&str, usize)> {
        let mut key = ipath;
        loop {
            if let Some(&(file, line)) = self.files.origins.get(key) {
                return Some((self.files.list[file].name.as_str(), line));
            }
            if self.lines.0.contains_key(key) {
                return None;
            }
            let cut = key.rfind('.')?;
            key = &key[..cut];
        }
    }

    pub fn problem(
        &self,
        path: impl Into<String>,
        ipath: &str,
        text: impl Into<String>,
    ) -> Problem {
        let path = path.into();
        Problem {
            path: match self.origin_of(ipath) {
                Some((file, line)) => format!("{file}:{line}: {path}"),
                None => path,
            },
            line: self.line_of(ipath),
            text: text.into(),
        }
    }

    pub fn copy_of(&self, object: &str) -> Option<&Use> {
        self.uses
            .iter()
            .find(|u| u.objects.iter().any(|name| name == object))
    }
}

pub(crate) fn normal(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if out.last().is_some_and(|last| *last != "..") => {
                out.pop();
            }
            _ => out.push(part),
        }
    }
    out.join("/")
}

pub(crate) fn folder(name: &str) -> &str {
    name.rfind('/').map_or("", |at| &name[..at])
}

pub(crate) fn joined(folder: &str, rel: &str) -> String {
    if folder.is_empty() {
        normal(rel)
    } else {
        normal(&format!("{folder}/{rel}"))
    }
}

pub(crate) struct Seg {
    pad: usize,
    src: Source,
    name: String,
    at_line: usize,
}

pub(crate) struct Sources {
    segs: Vec<Seg>,
}

impl Sources {
    pub(crate) fn of(list: &[File]) -> Sources {
        Sources {
            segs: list
                .iter()
                .map(|f| Seg {
                    pad: f.pad,
                    src: Source::new(&f.text),
                    name: f.name.clone(),
                    at_line: f.at_line,
                })
                .collect(),
        }
    }

    pub(crate) fn line(&self, offset: usize) -> usize {
        let at = self.segs.partition_point(|s| s.pad <= offset).max(1) - 1;
        let seg = &self.segs[at];
        seg.pad + seg.src.line(offset - seg.pad)
    }

    pub(crate) fn locate(&self, line: usize) -> (usize, usize) {
        let at = self.segs.partition_point(|s| s.pad < line).max(1) - 1;
        (at, line - self.segs[at].pad)
    }

    pub(crate) fn folder_at(&self, line: usize) -> &str {
        let (at, _) = self.locate(line);
        folder(&self.segs[at].name)
    }

    pub(crate) fn error(&self, mut e: Error) -> Error {
        let Some(line) = e.line else {
            return e;
        };
        let (at, local) = self.locate(line);
        if at == 0 {
            e.line = Some(local);
            return e;
        }
        let seg = &self.segs[at];
        e.path = if e.path.is_empty() {
            format!("{}:{local}", seg.name)
        } else {
            format!("{}:{local}: {}", seg.name, e.path)
        };
        e.line = Some(seg.at_line);
        e
    }

    pub(crate) fn problem(&self, p: Problem) -> Problem {
        let e = self.error(Error {
            path: p.path,
            line: p.line,
            text: p.text,
        });
        Problem {
            path: e.path,
            line: e.line,
            text: e.text,
        }
    }

    pub(crate) fn lines(
        &self,
        lines: BTreeMap<String, usize>,
    ) -> (BTreeMap<String, usize>, BTreeMap<String, (usize, usize)>) {
        let mut main = BTreeMap::new();
        let mut origins = BTreeMap::new();
        for (key, line) in lines {
            let (at, local) = self.locate(line);
            if at == 0 {
                main.insert(key, local);
            } else {
                main.insert(key.clone(), self.segs[at].at_line);
                origins.insert(key, (at, local));
            }
        }
        (main, origins)
    }
}

pub(crate) type Reader<'a> = &'a dyn Fn(&str) -> Result<String, String>;
type Prov = BTreeMap<Vec<String>, usize>;

#[derive(Clone, Default)]
struct Part {
    table: Table,
    prov: Prov,
    order: BTreeMap<String, Vec<usize>>,
    copies: BTreeMap<String, Vec<(Table, bool)>>,
    uses: Vec<(String, Vec<String>)>,
}

pub(crate) struct Assembly {
    pub list: Vec<File>,
    pub table: Table,
    pub copies: BTreeMap<String, Vec<(Table, bool)>>,
    pub uses: Vec<(String, Vec<String>)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Read,
    View,
}

struct Asm<'a> {
    read: Reader<'a>,
    mode: Mode,
    list: Vec<File>,
    tables: Vec<Table>,
    docs: Vec<DocumentMut>,
    stack: Vec<usize>,
    srcs: Vec<Source>,
    included_by: BTreeMap<(usize, usize), usize>,
    tree: usize,
    parts: BTreeMap<usize, Part>,
}

fn label(t: &Table, id: &str) -> Option<String> {
    t.get(id).and_then(Item::as_str).map(str::to_string)
}

fn path_of(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_string()).collect()
}

impl Asm<'_> {
    fn line_of(&self, span: Option<std::ops::Range<usize>>) -> Option<usize> {
        if self.mode == Mode::View {
            return None;
        }
        let offset = span?.start;
        let at = self.list.partition_point(|f| f.pad <= offset).max(1) - 1;
        let pad = self.list[at].pad;
        Some(pad + self.srcs[at].line(offset - pad))
    }

    fn item_line(&self, item: &Item) -> Option<usize> {
        self.line_of(item.span())
    }

    fn name(&self, file: usize) -> &str {
        &self.list[file].name
    }

    fn add(&mut self, file: File) -> Result<usize, Error> {
        let at = self.list.len();
        let pad = file.pad;
        match self.mode {
            Mode::Read => {
                let padded = format!("{}{}", " ".repeat(pad), file.text);
                let src = Source::new(&file.text);
                let doc = Document::parse(padded).map_err(|e| {
                    let line = e
                        .span()
                        .map(|s| pad + src.line(s.start.saturating_sub(pad)));
                    Error::new("", line, format!("not valid TOML: {}", e.message()))
                });
                self.srcs.push(src);
                self.list.push(file);
                let doc = doc?;
                self.tables.push(doc.as_table().clone());
                self.docs.push(DocumentMut::new());
            }
            Mode::View => {
                let doc: DocumentMut = file.text.parse().map_err(|e: toml_edit::TomlError| {
                    Error::new(
                        file.name.clone(),
                        None,
                        format!("not valid TOML: {}", e.message()),
                    )
                })?;
                self.srcs.push(Source::new(""));
                self.list.push(file);
                self.tables.push(doc.as_table().clone());
                self.docs.push(doc);
            }
        }
        Ok(at)
    }

    fn open(
        &mut self,
        name: String,
        role: Role,
        from: usize,
        line: Option<usize>,
        key: &str,
    ) -> Result<usize, Error> {
        if let Some(at) = self.list.iter().position(|f| f.name == name) {
            if self.stack.contains(&at) {
                let start = self.stack.iter().position(|&k| k == at).unwrap_or(0);
                let mut chain: Vec<&str> = self.stack[start..]
                    .iter()
                    .map(|&k| self.list[k].name.as_str())
                    .collect();
                chain.push(&self.list[at].name);
                return Err(Error::new(
                    key,
                    line,
                    format!(
                        "{} is a cycle; a file never includes or uses itself",
                        chain.join(" → ")
                    ),
                ));
            }
            if role == Role::Include
                && let Some(&first) = self.included_by.get(&(self.tree, at))
            {
                return Err(Error::new(
                    key,
                    line,
                    format!(
                        "{name} is included by {} already; a file is included once",
                        self.list[first].name
                    ),
                ));
            }
            if role == Role::Include {
                self.included_by.insert((self.tree, at), from);
            }
            return Ok(at);
        }
        let text = (self.read)(&name)
            .map_err(|why| Error::new(key, line, format!("cannot read {name}: {why}")))?;
        let at_line = if from == 0 {
            line.unwrap_or(1)
        } else {
            self.list[from].at_line
        };
        let pad = self.list.last().map_or(0, |f| f.pad + f.text.len() + 1);
        let at = self.add(File {
            name,
            path: None,
            text,
            role,
            at_line,
            pad,
        })?;
        if role == Role::Include {
            self.included_by.insert((self.tree, at), from);
        }
        Ok(at)
    }

    fn nested(&mut self, at: usize, expand: bool) -> Result<Part, Error> {
        self.stack.push(at);
        let part = self.file(at, expand, !expand);
        self.stack.pop();
        part
    }

    fn file(&mut self, k: usize, expand: bool, included: bool) -> Result<Part, Error> {
        let own = self.tables[k].clone();
        if included {
            self.check_included(k, &own)?;
        }
        let mut acc = Part::default();
        if let Some(item) = own.get("include") {
            for (entry, line) in self.include_list(item)? {
                let name = joined(folder(self.name(k)), &entry);
                let at = self.open(name, Role::Include, k, line, "include")?;
                let sub = self.nested(at, false)?;
                self.join(&mut acc, sub)?;
            }
        }
        self.overlay(&mut acc, &own, k)?;
        if expand && self.mode == Mode::Read {
            self.expand(&mut acc, k == 0)?;
        }
        Ok(acc)
    }

    fn check_included(&self, k: usize, own: &Table) -> Result<(), Error> {
        for (key, item) in own.iter() {
            let line = own
                .key(key)
                .and_then(|k| self.line_of(k.span()))
                .or_else(|| self.item_line(item));
            if key == "manf" {
                return Err(Error::new(key, line, OLD_HEADER));
            }
            if !INCLUDED.contains(&key) {
                return Err(Error::new(
                    key,
                    line,
                    format!(
                        "an included file holds only {}; `{key}` belongs to the recipe that includes {}",
                        INCLUDED[1..].join(", "),
                        self.name(k)
                    ),
                ));
            }
            if key == "manfred" {
                let fits = item
                    .as_integer()
                    .is_some_and(|v| (1..=i64::from(SCHEMA)).contains(&v));
                if !fits {
                    return Err(Error::new(
                        "manfred",
                        line,
                        format!("expected the schema version, from 1 to {SCHEMA}"),
                    ));
                }
            }
        }
        Ok(())
    }

    fn include_list(&self, item: &Item) -> Result<Vec<(String, Option<usize>)>, Error> {
        let line = self.item_line(item);
        let Some(list) = item.as_array() else {
            return Err(Error::new(
                "include",
                line,
                "expected an array of file names, as include = [\"rig.toml\"]",
            ));
        };
        let mut out = Vec::new();
        for (i, value) in list.iter().enumerate() {
            let at = self.line_of(value.span()).or(line);
            let Some(text) = value.as_str() else {
                return Err(Error::new(
                    format!("include[{i}]"),
                    at,
                    "expected a file name, a string",
                ));
            };
            refuse_absolute(text, &format!("include[{i}]"), at)?;
            out.push((text.to_string(), at));
        }
        Ok(out)
    }

    fn entries(&self, item: &Item, key: &str) -> Result<Vec<Table>, Error> {
        match item {
            Item::ArrayOfTables(list) => Ok(list.iter().cloned().collect()),
            Item::Value(Value::Array(list)) => list
                .iter()
                .enumerate()
                .map(|(i, v)| match v.as_inline_table() {
                    Some(t) => Ok(t.clone().into_table()),
                    None => Err(Error::new(
                        format!("{key}[{i}]"),
                        self.line_of(v.span()),
                        "expected a table",
                    )),
                })
                .collect(),
            _ => Err(Error::new(
                key,
                self.item_line(item),
                "expected an array of tables",
            )),
        }
    }

    fn push(
        &self,
        acc: &mut Part,
        key: &str,
        id: &str,
        table: Table,
        file: usize,
    ) -> Result<(), Error> {
        if let Some(name) = label(&table, id).filter(|name| !name.is_empty()) {
            let at = vec![key.to_string(), name.clone()];
            if let Some(&other) = acc.prov.get(&at)
                && other != file
            {
                return Err(Error::new(
                    format!("{key}[{name}]"),
                    self.line_of(table.span()),
                    format!(
                        "`{name}` is defined in {} and in {}; a name is defined once across a recipe and the files it includes and uses",
                        self.name(other),
                        self.name(file)
                    ),
                ));
            }
            acc.prov.entry(at).or_insert(file);
        }
        if !acc.table.contains_key(key) {
            acc.table
                .insert(key, Item::ArrayOfTables(ArrayOfTables::new()));
        }
        if let Some(list) = acc
            .table
            .get_mut(key)
            .and_then(Item::as_array_of_tables_mut)
        {
            list.push(table);
        }
        acc.order.entry(key.to_string()).or_default().push(file);
        Ok(())
    }

    fn join(&self, acc: &mut Part, sub: Part) -> Result<(), Error> {
        for (key, item) in sub.table.iter() {
            if let Some((_, id)) = ARRAYS.iter().find(|(k, _)| *k == key) {
                let files = sub.order.get(key).cloned().unwrap_or_default();
                for (i, table) in self.entries(item, key)?.into_iter().enumerate() {
                    let file = files.get(i).copied().unwrap_or(0);
                    self.push(acc, key, id, table, file)?;
                }
            } else if key == "materials" {
                self.materials(acc, item, &sub.prov)?;
            } else if let Some(existing) = acc.table.get(key) {
                let first = acc.prov.get(&path_of(&[key])).copied().unwrap_or(0);
                let second = sub.prov.get(&path_of(&[key])).copied().unwrap_or(0);
                let _ = existing;
                return Err(Error::new(
                    key,
                    self.item_line(item),
                    format!(
                        "[{key}] is in {} and in {}; one included file sets it, and the file that includes them overrides its keys",
                        self.name(first),
                        self.name(second)
                    ),
                ));
            } else if let Some(k) = sub.table.key(key) {
                acc.table.insert_formatted(k, item.clone());
            }
        }
        for (path, file) in sub.prov {
            acc.prov.entry(path).or_insert(file);
        }
        acc.copies.extend(sub.copies);
        acc.uses.extend(sub.uses);
        Ok(())
    }

    fn materials(&self, acc: &mut Part, item: &Item, prov: &Prov) -> Result<(), Error> {
        let Some(from) = item.as_table() else {
            return Err(Error::new(
                "materials",
                self.item_line(item),
                "expected a table of materials",
            ));
        };
        if !acc.table.contains_key("materials") {
            let mut table = Table::new();
            table.set_implicit(true);
            acc.table.insert("materials", Item::Table(table));
        }
        for (name, entry) in from.iter() {
            let at = path_of(&["materials", name]);
            let file = prov.get(&at).copied().unwrap_or(0);
            if let Some(&other) = acc.prov.get(&at)
                && other != file
            {
                return Err(Error::new(
                    format!("materials.{name}"),
                    self.item_line(entry),
                    format!(
                        "material `{name}` is defined in {} and in {}; a name is defined once across a recipe and the files it includes",
                        self.name(other),
                        self.name(file)
                    ),
                ));
            }
            acc.prov.entry(at).or_insert(file);
            if let (Some(key), Some(into)) = (
                from.key(name),
                acc.table.get_mut("materials").and_then(Item::as_table_mut),
            ) {
                into.insert_formatted(key, entry.clone());
            }
        }
        Ok(())
    }

    fn overlay(&self, acc: &mut Part, own: &Table, k: usize) -> Result<(), Error> {
        let main = k == 0;
        for (key, item) in own.iter() {
            if !main && (key == "include" || key == "manfred") {
                continue;
            }
            if let Some((_, id)) = ARRAYS.iter().find(|(name, _)| *name == key) {
                for table in self.entries(item, key)? {
                    self.push(acc, key, id, table, k)?;
                }
                continue;
            }
            if key == "materials" && item.is_table() {
                let mut prov = Prov::new();
                if let Some(t) = item.as_table() {
                    for (name, _) in t.iter() {
                        prov.insert(path_of(&["materials", name]), k);
                    }
                }
                self.materials(acc, item, &prov)?;
                continue;
            }
            let Some(name) = own.key(key) else {
                continue;
            };
            let at = path_of(&[key]);
            match (acc.table.get_mut(key), item) {
                (Some(Item::Table(into)), Item::Table(from)) if SINGLE.contains(&key) => {
                    deep(into, from, &at, &mut acc.prov, k);
                }
                _ => {
                    acc.table.insert_formatted(name, item.clone());
                    acc.prov.retain(|p, _| !p.starts_with(&at));
                    acc.prov.insert(at, k);
                }
            }
        }
        Ok(())
    }

    fn expand(&mut self, acc: &mut Part, top: bool) -> Result<(), Error> {
        let Some(item) = acc.table.get("use") else {
            return Ok(());
        };
        let uses = self.entries(item, "use")?;
        let holders = acc.order.get("use").cloned().unwrap_or_default();
        for (i, u) in uses.into_iter().enumerate() {
            let holder = holders.get(i).copied().unwrap_or(0);
            let line = self.line_of(u.span());
            let text = |key: &str| -> Result<Option<(String, Option<usize>)>, Error> {
                match u.get(key) {
                    None => Ok(None),
                    Some(v) => match v.as_str() {
                        Some(s) => Ok(Some((s.to_string(), self.item_line(v).or(line)))),
                        None => Err(Error::new(
                            format!("use[{i}].{key}"),
                            self.item_line(v).or(line),
                            "expected a string",
                        )),
                    },
                }
            };
            let prefix = text("prefix")?.map(|(p, _)| p).unwrap_or_default();
            let at = use_path(&prefix, i);
            let Some((file, file_line)) = text("file")? else {
                return Err(Error::new(
                    format!("{at}.file"),
                    line,
                    "missing, expected the recipe file whose objects this places",
                ));
            };
            refuse_absolute(&file, &format!("{at}.file"), file_line)?;
            let parent = text("parent")?.map(|(p, _)| p);
            let name = joined(folder(self.name(holder)), &file);
            let part_at = self.open(name, Role::Part, holder, file_line, &format!("{at}.file"))?;
            let part = match self.parts.get(&part_at) {
                Some(part) => part.clone(),
                None => {
                    let tree = std::mem::replace(&mut self.tree, part_at);
                    let part = self.nested(part_at, true);
                    self.tree = tree;
                    let part = part?;
                    self.parts.insert(part_at, part.clone());
                    part
                }
            };
            let objects = match part.table.get("object") {
                Some(item) => self.entries(item, "object")?,
                None => Vec::new(),
            };
            if objects.is_empty() {
                return Err(Error::new(
                    format!("{at}.file"),
                    file_line,
                    format!("{} has no [[object]] to place", self.name(part_at)),
                ));
            }
            let mut names = Vec::new();
            for mut t in objects {
                let Some(old) = label(&t, "name") else {
                    self.push(acc, "object", "name", t, part_at)?;
                    continue;
                };
                let new = format!("{prefix}{old}");
                rename(&mut t, "name", &new);
                let root = match label(&t, "parent") {
                    Some(p) => {
                        rename(&mut t, "parent", &format!("{prefix}{p}"));
                        false
                    }
                    None => {
                        if let Some(p) = &parent {
                            t.insert("parent", Item::Value(Value::from(p.as_str())));
                        }
                        true
                    }
                };
                for key in ["into", "on"] {
                    if let Some(target) = label(&t, key) {
                        rename(&mut t, key, &format!("{prefix}{target}"));
                    }
                }
                let mut chain = part.copies.get(&old).cloned().unwrap_or_default();
                if root {
                    chain.push((u.clone(), top));
                }
                let at_name = vec!["object".to_string(), new.clone()];
                if let Some(&other) = acc.prov.get(&at_name) {
                    return Err(Error::new(
                        format!("object[{new}]"),
                        self.line_of(t.span()),
                        format!(
                            "`{new}`, which {at} places from {}, is defined in {} too",
                            self.name(part_at),
                            self.name(other)
                        ),
                    ));
                }
                self.push(acc, "object", "name", t, part_at)?;
                if !chain.is_empty() {
                    acc.copies.insert(new.clone(), chain);
                }
                names.push(new);
            }
            acc.uses.push((prefix, names));
        }
        Ok(())
    }
}

pub(crate) fn use_path(prefix: &str, index: usize) -> String {
    if prefix.is_empty() {
        format!("use[{index}]")
    } else {
        format!("use[{prefix}]")
    }
}

fn rename(t: &mut Table, key: &str, to: &str) {
    if let Some(Item::Value(v)) = t.get_mut(key) {
        let decor = v.decor().clone();
        *v = Value::from(to);
        *v.decor_mut() = decor;
    }
}

fn refuse_absolute(text: &str, key: &str, line: Option<usize>) -> Result<(), Error> {
    if Path::new(text).is_absolute() {
        return Err(Error::new(
            key,
            line,
            format!("`{text}` is absolute; a path is relative to the file that names it"),
        ));
    }
    Ok(())
}

fn deep(into: &mut Table, from: &Table, at: &[String], prov: &mut Prov, k: usize) {
    for (name, item) in from.iter() {
        let mut path = at.to_vec();
        path.push(name.to_string());
        match (into.get_mut(name), item) {
            (Some(Item::Table(a)), Item::Table(b)) => deep(a, b, &path, prov, k),
            _ => {
                if let Some(key) = from.key(name) {
                    into.insert_formatted(key, item.clone());
                }
                prov.retain(|p, _| !p.starts_with(&path));
                prov.insert(path, k);
            }
        }
    }
}

pub(crate) fn has_more(root: &Table) -> bool {
    root.contains_key("include") || root.contains_key("use")
}

pub(crate) fn assemble(main: File, read: Reader<'_>) -> Result<Assembly, Error> {
    let mut asm = Asm {
        read,
        mode: Mode::Read,
        list: Vec::new(),
        tables: Vec::new(),
        docs: Vec::new(),
        stack: vec![0],
        srcs: Vec::new(),
        included_by: BTreeMap::new(),
        tree: 0,
        parts: BTreeMap::new(),
    };
    asm.add(main)?;
    if !has_more(&asm.tables[0]) {
        let table = asm.tables.pop().unwrap_or_default();
        return Ok(Assembly {
            list: asm.list,
            table,
            copies: BTreeMap::new(),
            uses: Vec::new(),
        });
    }
    let fail = |asm: &Asm<'_>, e: Error| Sources::of(&asm.list).error(e);
    match asm.file(0, true, false) {
        Ok(part) => Ok(Assembly {
            list: asm.list,
            table: part.table,
            copies: part.copies,
            uses: part.uses,
        }),
        Err(e) => Err(fail(&asm, e)),
    }
}

pub(crate) struct View {
    pub doc: DocumentMut,
    pub names: Vec<String>,
    pub docs: Vec<DocumentMut>,
    prov: Prov,
    order: BTreeMap<String, Vec<usize>>,
}

pub(crate) fn view(main_name: &str, main: &DocumentMut, read: Reader<'_>) -> Result<View, Error> {
    let mut asm = Asm {
        read,
        mode: Mode::View,
        list: Vec::new(),
        tables: Vec::new(),
        docs: Vec::new(),
        stack: vec![0],
        srcs: Vec::new(),
        included_by: BTreeMap::new(),
        tree: 0,
        parts: BTreeMap::new(),
    };
    asm.list.push(File {
        name: main_name.to_string(),
        path: None,
        text: String::new(),
        role: Role::Main,
        at_line: 1,
        pad: 0,
    });
    asm.srcs.push(Source::new(""));
    asm.tables.push(main.as_table().clone());
    asm.docs.push(main.clone());
    let part = asm.file(0, false, false)?;
    let mut doc = DocumentMut::new();
    *doc.as_table_mut() = part.table;
    Ok(View {
        doc,
        names: asm.list.into_iter().map(|f| f.name).collect(),
        docs: asm.docs,
        prov: part.prov,
        order: part.order,
    })
}

pub(crate) fn is_array(key: &str) -> bool {
    ARRAYS.iter().any(|(k, _)| *k == key)
}

impl View {
    pub fn entry(&self, key: &str, name: &str) -> Option<(usize, usize)> {
        let id = ARRAYS.iter().find(|(k, _)| *k == key)?.1;
        let list = self.doc.get(key)?.as_array_of_tables()?;
        let at = list
            .iter()
            .position(|t| t.get(id).and_then(Item::as_str) == Some(name))?;
        Some((at, self.file_at(key, at)))
    }

    pub fn file_at(&self, key: &str, at: usize) -> usize {
        self.order
            .get(key)
            .and_then(|o| o.get(at))
            .copied()
            .unwrap_or(0)
    }

    pub fn local(&self, key: &str, at: usize, file: usize) -> usize {
        self.order
            .get(key)
            .map_or(0, |o| o.iter().take(at).filter(|&&f| f == file).count())
    }

    pub fn merged(&self, key: &str, file: usize, local: usize) -> usize {
        let order = self.order.get(key).cloned().unwrap_or_default();
        let mut seen = 0;
        let mut last = None;
        for (at, &f) in order.iter().enumerate() {
            if f == file {
                if seen == local {
                    return at;
                }
                seen += 1;
                last = Some(at);
            }
        }
        last.map_or(order.len(), |at| at + 1)
    }

    pub fn owner(&self, path: &[String]) -> usize {
        (1..=path.len())
            .rev()
            .find_map(|n| self.prov.get(&path[..n]).copied())
            .unwrap_or(0)
    }

    pub fn labels(&self) -> Vec<(String, String, usize)> {
        let mut out = Vec::new();
        for (key, id) in ARRAYS {
            if let Some(list) = self.doc.get(key).and_then(Item::as_array_of_tables) {
                for (at, t) in list.iter().enumerate() {
                    if let Some(name) = label(t, id) {
                        out.push((key.to_string(), name, self.file_at(key, at)));
                    }
                }
            }
        }
        out
    }
}

pub(crate) fn identity(key: &str) -> Option<&'static str> {
    ARRAYS.iter().find(|(k, _)| *k == key).map(|(_, id)| *id)
}

pub(crate) fn disk_reader(
    base: Option<&Path>,
    memory: &BTreeMap<String, String>,
    name: &str,
) -> Result<String, String> {
    if let Some(text) = memory.get(name) {
        return Ok(text.clone());
    }
    let Some(base) = base else {
        return Err("a recipe read from text has no folder to read it from".to_string());
    };
    std::fs::read_to_string(base.join(name)).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "no such file".to_string(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        _ => e.to_string(),
    })
}

pub(crate) fn uses_of(list: &[Use], by_prefix: &[(String, Vec<String>)]) -> Vec<Use> {
    list.iter()
        .map(|u| Use {
            objects: by_prefix
                .iter()
                .find(|(p, _)| *p == u.prefix)
                .map(|(_, names)| names.clone())
                .unwrap_or_default(),
            ..u.clone()
        })
        .collect()
}
