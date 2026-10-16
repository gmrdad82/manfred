use std::collections::BTreeMap;
use std::path::Path;

use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, TableLike, Value};

use crate::include::{self, View, identity, is_array};
use crate::read::{Main, params_ref, parse_doc, read_main};
use crate::{Error, Key, Loaded, Patch, Recipe};

pub fn write(l: &Loaded) -> String {
    if l.recipe.files.included() {
        l.recipe.source.clone()
    } else {
        l.doc.to_string()
    }
}

pub fn save(l: &Loaded) -> Result<(), Error> {
    save_to(l, &l.path)
}

fn replace(path: &Path, text: &str) -> Result<(), Error> {
    crate::files::replace(path, text.replace("\r\n", "\n").as_bytes()).map_err(|e| {
        Error::new(
            path.display().to_string(),
            None,
            format!("cannot replace the recipe: {e}"),
        )
    })
}

pub fn save_to(l: &Loaded, path: &Path) -> Result<(), Error> {
    replace(path, &write(l))?;
    let Some(base) = l.recipe.base.as_deref() else {
        return Ok(());
    };
    for name in l.recipe.files.changed.keys() {
        let Some(text) = l.recipe.files.text_of(name) else {
            continue;
        };
        let target = base.join(name);
        if crate::files::read_to_string(&target).ok().as_deref() != Some(text) {
            replace(&target, text)?;
        }
    }
    Ok(())
}

pub fn path_text(path: &[Key]) -> String {
    let mut out = String::new();
    for key in path {
        match key {
            Key::Table(name) if out.is_empty() => out.push_str(name),
            Key::Table(name) => {
                out.push('.');
                out.push_str(name);
            }
            Key::Object(name) => {
                out.push('[');
                out.push_str(name);
                out.push(']');
            }
            Key::Index(i) => {
                out.push('[');
                out.push_str(&i.to_string());
                out.push(']');
            }
        }
    }
    out
}

pub fn apply(l: &mut Loaded, p: &Patch) -> Result<Patch, Error> {
    let mut inverses = apply_all(l, std::slice::from_ref(p))?;
    Ok(inverses.remove(0))
}

fn reparse(l: &Loaded, text: &str, memory: &BTreeMap<String, String>) -> Result<Recipe, Error> {
    read_main(Main {
        text,
        base: l.recipe.base.as_deref(),
        name: l.recipe.files.main_name(),
        memory,
        edits: &[],
    })
}

fn entry_in(doc: &Table, key: &str, name: &str) -> bool {
    let Some(id) = identity(key) else {
        return false;
    };
    doc.get(key)
        .and_then(Item::as_array_of_tables)
        .is_some_and(|list| {
            list.iter()
                .any(|t| t.get(id).and_then(Item::as_str) == Some(name))
        })
}

fn refuse_copy(l: &Loaded, p: &Patch) -> Result<(), Error> {
    let [Key::Table(key), Key::Object(name) | Key::Table(name), ..] = p.path.as_slice() else {
        return Ok(());
    };
    if key != "object" || entry_in(l.doc.as_table(), key, name) {
        return Ok(());
    }
    match l.recipe.copy_of(name) {
        Some(u) => Err(Error::new(
            path_text(&p.path),
            None,
            format!(
                "`{name}` is a copy that {} places from {}; change the part in {}, or move every copy of it by the use's at, rotate and scale",
                if u.prefix.is_empty() {
                    "a use".to_string()
                } else {
                    format!("use[{}]", u.prefix)
                },
                u.file,
                u.file
            ),
        )),
        None => Ok(()),
    }
}

pub fn apply_all(l: &mut Loaded, patches: &[Patch]) -> Result<Vec<Patch>, Error> {
    for p in patches {
        refuse_copy(l, p)?;
    }
    if l.recipe.files.included() {
        return routed(l, patches);
    }
    let mut doc = l.doc.clone();
    let mut inverses = Vec::with_capacity(patches.len());
    for p in patches {
        inverses.push(edit(&mut doc, p)?);
    }
    let recipe = reparse(l, &doc.to_string(), &BTreeMap::new())?;
    l.doc = doc;
    l.recipe = recipe;
    inverses.reverse();
    Ok(inverses)
}

fn start_of(l: &Loaded, name: &str) -> Option<String> {
    let base = l.recipe.base.as_deref()?;
    let disk = crate::files::read_to_string(&base.join(name)).ok();
    let memory = l.recipe.files.text_of(name);
    match (l.recipe.files.changed.get(name), disk) {
        (Some(was), Some(disk)) if *was == disk => memory.map(str::to_string),
        (_, Some(disk)) => Some(disk),
        (_, None) => memory.map(str::to_string),
    }
}

fn routed(l: &mut Loaded, patches: &[Patch]) -> Result<Vec<Patch>, Error> {
    let base = l.recipe.base.clone();
    let main_name = l.recipe.files.main_name().to_string();
    let mut main: DocumentMut = l.recipe.source.parse().map_err(|e: toml_edit::TomlError| {
        Error::new("", None, format!("not valid TOML: {}", e.message()))
    })?;
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    for name in l.recipe.files.names().into_iter().skip(1) {
        if let Some(text) = start_of(l, name) {
            texts.insert(name.to_string(), text);
        }
    }
    let mut changed = BTreeMap::new();
    for (name, was) in &l.recipe.files.changed {
        let disk = base
            .as_deref()
            .and_then(|b| crate::files::read_to_string(&b.join(name)).ok());
        let kept = match &disk {
            Some(disk) if disk == was => was.clone(),
            Some(disk) => disk.clone(),
            None => was.clone(),
        };
        changed.insert(name.clone(), kept);
    }
    let mut homes = l.recipe.files.homes.clone();
    let mut docs: BTreeMap<String, DocumentMut> = BTreeMap::new();
    let view_of = |main: &DocumentMut, docs: &BTreeMap<String, DocumentMut>| {
        let mut memory = texts.clone();
        for (name, doc) in docs {
            memory.insert(name.clone(), doc.to_string());
        }
        let read = |name: &str| include::disk_reader(base.as_deref(), &memory, name);
        include::view(&main_name, main, &read)
    };
    let mut before = view_of(&main, &docs)?;
    let mut inverses = Vec::with_capacity(patches.len());
    for p in patches {
        remember(&mut homes, &before);
        let (file, local) = route(&before, &homes, p);
        let name = before.names[file].clone();
        let doc = if file == 0 {
            &mut main
        } else {
            if !changed.contains_key(&name) {
                changed.insert(name.clone(), texts.get(&name).cloned().unwrap_or_default());
            }
            docs.entry(name.clone())
                .or_insert_with(|| before.docs[file].clone())
        };
        let first = first_entry(doc, &local);
        let mut inverse = edit(doc, first.as_ref().unwrap_or(&local))?;
        let after = view_of(&main, &docs)?;
        if let Some(key) = first.as_ref().and_then(|f| match f.path.first() {
            Some(Key::Table(key)) => Some(key.clone()),
            _ => None,
        }) {
            inverse = Patch {
                path: vec![
                    Key::Table(key.clone()),
                    Key::Index(after.merged(&key, file, 0)),
                ],
                value: None,
            };
        } else if let [Key::Table(key), Key::Index(at), ..] = inverse.path.as_slice()
            && is_array(key)
        {
            let merged = if p.value.is_none() {
                before.merged(key, file, *at)
            } else {
                after.merged(key, file, *at)
            };
            inverse.path[1] = Key::Index(merged);
        }
        inverses.push(inverse);
        before = after;
    }
    remember(&mut homes, &before);
    let mut memory = texts;
    for (name, doc) in &docs {
        memory.insert(name.clone(), doc.to_string());
    }
    let mut recipe = reparse(l, &main.to_string(), &memory)?;
    recipe.files.changed = changed;
    recipe.files.homes = homes;
    l.doc = before.doc;
    l.recipe = recipe;
    inverses.reverse();
    Ok(inverses)
}

fn first_entry(doc: &DocumentMut, p: &Patch) -> Option<Patch> {
    let [Key::Table(key), _] = p.path.as_slice() else {
        return None;
    };
    if !is_array(key) || doc.contains_key(key) {
        return None;
    }
    let table = match p.value.clone()? {
        Item::Table(t) => t,
        Item::Value(Value::InlineTable(t)) => t.into_table(),
        _ => return None,
    };
    let mut list = ArrayOfTables::new();
    list.push(table);
    Some(Patch {
        path: vec![Key::Table(key.clone())],
        value: Some(Item::ArrayOfTables(list)),
    })
}

fn remember(homes: &mut BTreeMap<(String, String), String>, view: &View) {
    for (key, label, file) in view.labels() {
        homes.insert((key, label), view.names[file].clone());
    }
}

fn home(
    view: &View,
    homes: &BTreeMap<(String, String), String>,
    key: &str,
    name: &str,
) -> Option<usize> {
    let file = homes.get(&(key.to_string(), name.to_string()))?;
    view.names.iter().position(|n| n == file)
}

fn route(view: &View, homes: &BTreeMap<(String, String), String>, p: &Patch) -> (usize, Patch) {
    if let [Key::Table(key), second, rest @ ..] = p.path.as_slice()
        && is_array(key)
    {
        match second {
            Key::Object(name) | Key::Table(name) => {
                let file = match view.entry(key, name) {
                    Some((_, file)) => file,
                    None => home(view, homes, key, name).unwrap_or(0),
                };
                return (file, p.clone());
            }
            Key::Index(at) => {
                let len = view
                    .doc
                    .get(key)
                    .and_then(Item::as_array_of_tables)
                    .map_or(0, ArrayOfTables::len);
                let file = if rest.is_empty() && p.value.is_some() {
                    let named = identity(key).and_then(|id| {
                        p.value
                            .as_ref()
                            .and_then(Item::as_table_like)
                            .and_then(|t| t.get(id))
                            .and_then(Item::as_str)
                            .and_then(|name| home(view, homes, key, name))
                    });
                    named
                        .or_else(|| (*at > 0 && *at <= len).then(|| view.file_at(key, at - 1)))
                        .or_else(|| (*at < len).then(|| view.file_at(key, *at)))
                        .unwrap_or(0)
                } else {
                    view.file_at(key, *at)
                };
                let mut path = p.path.clone();
                path[1] = Key::Index(view.local(key, *at, file));
                return (
                    file,
                    Patch {
                        path,
                        value: p.value.clone(),
                    },
                );
            }
        }
    }
    let strings: Vec<String> = p
        .path
        .iter()
        .map(|k| match k {
            Key::Table(name) | Key::Object(name) => name.clone(),
            Key::Index(i) => i.to_string(),
        })
        .collect();
    (view.owner(&strings), p.clone())
}

pub fn edit(doc: &mut DocumentMut, p: &Patch) -> Result<Patch, Error> {
    let done = edit_item(doc.as_item_mut(), &p.path, p.value.clone())
        .map_err(|text| Error::new(path_text(&p.path), None, text))?;
    let mut path = p.path.clone();
    if let Some(last) = done.last
        && let Some(slot) = path.last_mut()
    {
        *slot = last;
    }
    renamed(&mut path, p, &done.old);
    Ok(Patch {
        path,
        value: done.old,
    })
}

pub fn reread(path: &Path, text: &str) -> Result<Loaded, Error> {
    let (recipe, doc) = parse_doc(path, text)?;
    Ok(Loaded {
        recipe,
        doc,
        path: path.to_path_buf(),
    })
}

#[derive(Clone, Copy)]
enum At<'a> {
    Table(&'a Table),
    Tables(&'a ArrayOfTables),
    Value(&'a Value),
}

fn at_item(item: &Item) -> Option<At<'_>> {
    match item {
        Item::Table(t) => Some(At::Table(t)),
        Item::ArrayOfTables(a) => Some(At::Tables(a)),
        Item::Value(v) => Some(At::Value(v)),
        Item::None => None,
    }
}

fn step<'a>(at: At<'a>, key: &Key) -> Option<At<'a>> {
    match (at, key) {
        (At::Table(t), Key::Table(k)) => t.get(k).and_then(at_item),
        (At::Value(Value::InlineTable(t)), Key::Table(k)) => t.get(k).map(At::Value),
        (At::Tables(a), Key::Object(n) | Key::Table(n)) => a
            .iter()
            .find(|t| label_of(t) == Some(n.as_str()))
            .or_else(|| n.parse::<usize>().ok().and_then(|i| a.get(i)))
            .map(At::Table),
        (At::Tables(a), Key::Index(i)) => a.get(*i).map(At::Table),
        (At::Value(Value::Array(a)), Key::Index(i)) => a.get(*i).map(At::Value),
        _ => None,
    }
}

fn walk<'a>(doc: &'a DocumentMut, path: &[Key]) -> Option<At<'a>> {
    let mut at = At::Table(doc.as_table());
    for key in path {
        at = step(at, key)?;
    }
    Some(at)
}

pub(crate) fn item_exists(doc: &DocumentMut, path: &[Key]) -> bool {
    walk(doc, path).is_some()
}

pub(crate) fn names_at(doc: &DocumentMut, path: &[Key]) -> Vec<String> {
    match walk(doc, path) {
        Some(At::Table(t)) => t.iter().map(|(k, _)| k.to_string()).collect(),
        Some(At::Value(Value::InlineTable(t))) => t.iter().map(|(k, _)| k.to_string()).collect(),
        _ => Vec::new(),
    }
}

pub fn value_at(doc: &DocumentMut, path: &[Key]) -> Option<Value> {
    value_in(doc.as_table(), path)
}

pub(crate) fn value_in(table: &Table, path: &[Key]) -> Option<Value> {
    let mut at = At::Table(table);
    for key in path {
        at = step(at, key)?;
    }
    match at {
        At::Value(v) => Some(v.clone()),
        At::Table(_) | At::Tables(_) => None,
    }
}

pub(crate) enum Reach {
    Value(Value),
    Item(Item),
    Missing(usize, bool),
    NoEntry(usize),
    Inside(usize),
}

pub(crate) fn reach(table: &Table, path: &[Key]) -> Reach {
    let mut at = At::Table(table);
    for (i, key) in path.iter().enumerate() {
        match step(at, key) {
            Some(next) => at = next,
            None => {
                return match at {
                    At::Table(_) => Reach::Missing(i, false),
                    At::Value(Value::InlineTable(_)) => Reach::Missing(i, true),
                    At::Tables(_) => Reach::NoEntry(i),
                    At::Value(_) => Reach::Inside(i),
                };
            }
        }
    }
    match at {
        At::Value(v) => Reach::Value(v.clone()),
        At::Table(t) => Reach::Item(Item::Table(t.clone())),
        At::Tables(a) => Reach::Item(Item::ArrayOfTables(a.clone())),
    }
}

pub fn params_in(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => params_ref(s.value()).map(|reference| format!("params.{reference}")),
        Value::Array(a) => a.iter().find_map(params_in),
        Value::InlineTable(t) => t.iter().find_map(|(_, v)| params_in(v)),
        _ => None,
    }
}

pub fn number(value: f64) -> Item {
    Item::Value(Value::from(value))
}

pub fn numbers(values: &[f64]) -> Item {
    let mut array = Array::new();
    for value in values {
        array.push(*value);
    }
    Item::Value(Value::Array(array))
}

pub fn string(text: &str) -> Item {
    Item::Value(Value::from(text))
}

pub fn flag(value: bool) -> Item {
    Item::Value(Value::from(value))
}

fn renamed(path: &mut [Key], p: &Patch, old: &Option<Item>) {
    let n = path.len();
    if n < 2 {
        return;
    }
    let is_label = matches!(&path[n - 1], Key::Table(k) if k == "name" || k == "group");
    if !is_label {
        return;
    }
    let (Some(Item::Value(new)), Some(Item::Value(was))) = (&p.value, old) else {
        return;
    };
    let (Some(new), Some(was)) = (new.as_str(), was.as_str()) else {
        return;
    };
    let by_name = match &path[n - 2] {
        Key::Object(x) | Key::Table(x) => x == was,
        Key::Index(_) => false,
    };
    if by_name {
        path[n - 2] = Key::Object(new.to_string());
    }
}

pub(crate) struct Done {
    pub old: Option<Item>,
    pub last: Option<Key>,
}

fn done(old: Option<Item>) -> Done {
    Done { old, last: None }
}

pub(crate) fn edit_item(
    item: &mut Item,
    path: &[Key],
    value: Option<Item>,
) -> Result<Done, String> {
    if path.is_empty() {
        return Err("the path is empty".to_string());
    }
    match item {
        Item::Table(t) => edit_table(t as &mut dyn TableLike, path, value),
        Item::ArrayOfTables(a) => edit_tables(a, path, value),
        Item::Value(Value::InlineTable(t)) => edit_table(t as &mut dyn TableLike, path, value),
        Item::Value(Value::Array(a)) => edit_array(a, path, value),
        Item::Value(_) => Err(format!("`{}` has nothing inside it", path_text(&path[..1]))),
        Item::None => Err("nothing is there".to_string()),
    }
}

fn edit_table(t: &mut dyn TableLike, path: &[Key], value: Option<Item>) -> Result<Done, String> {
    let Key::Table(key) = &path[0] else {
        return Err(format!(
            "`{}` is not a key; a table is addressed by key name",
            path_text(&path[..1])
        ));
    };
    if path.len() > 1 {
        let child = t.get_mut(key).ok_or_else(|| format!("no key `{key}`"))?;
        return edit_item(child, &path[1..], value);
    }
    match value {
        Some(mut new) => match t.get_mut(key) {
            Some(slot) => {
                if let (Item::Value(old), Item::Value(fresh)) = (&*slot, &mut new) {
                    *fresh.decor_mut() = old.decor().clone();
                }
                let old = std::mem::replace(slot, new);
                Ok(done(Some(old)))
            }
            None => {
                t.insert(key, new);
                Ok(done(None))
            }
        },
        None => match t.remove(key) {
            Some(old) => Ok(done(Some(old))),
            None => Err(format!("no key `{key}` to remove")),
        },
    }
}

fn label_of(t: &Table) -> Option<&str> {
    t.get("name")
        .or_else(|| t.get("group"))
        .or_else(|| t.get("prefix"))
        .or_else(|| t.get("object"))
        .and_then(Item::as_str)
}

pub(crate) fn morph_room(t: &mut Table, path: &[Key]) -> bool {
    let [
        Key::Table(top),
        Key::Table(name),
        Key::Table(key),
        Key::Table(_),
    ] = path
    else {
        return false;
    };
    if top != "object" || key != "morph" {
        return false;
    }
    let Some(object) = t
        .get_mut("object")
        .and_then(Item::as_array_of_tables_mut)
        .and_then(|list| list.iter_mut().find(|o| label_of(o) == Some(name.as_str())))
    else {
        return false;
    };
    match object.get("morph") {
        None => {
            object.insert(
                "morph",
                Item::Value(Value::InlineTable(toml_edit::InlineTable::new())),
            );
            true
        }
        Some(item) => item.is_table_like(),
    }
}

pub(crate) fn edit_root(t: &mut Table, path: &[Key], value: Option<Item>) -> Result<Done, String> {
    if path.is_empty() {
        return Err("the path is empty".to_string());
    }
    edit_table(t as &mut dyn TableLike, path, value)
}

fn find(a: &ArrayOfTables, name: &str) -> Result<usize, String> {
    a.iter()
        .position(|t| label_of(t) == Some(name))
        .or_else(|| name.parse::<usize>().ok().filter(|i| *i < a.len()))
        .ok_or_else(|| format!("no entry named `{name}`"))
}

fn table_of(item: Item) -> Result<Table, String> {
    match item {
        Item::Table(t) => Ok(t),
        Item::Value(Value::InlineTable(t)) => Ok(t.into_table()),
        _ => Err("expected a table".to_string()),
    }
}

fn edit_tables(a: &mut ArrayOfTables, path: &[Key], value: Option<Item>) -> Result<Done, String> {
    let named = match &path[0] {
        Key::Object(n) | Key::Table(n) => Some(n.as_str()),
        Key::Index(_) => None,
    };
    let at = match (&path[0], named) {
        (Key::Index(i), _) => *i,
        (_, Some(n)) => match find(a, n) {
            Ok(i) => i,
            Err(_) if path.len() == 1 && value.is_some() => a.len(),
            Err(e) => return Err(e),
        },
        _ => unreachable!(),
    };
    if path.len() > 1 {
        let len = a.len();
        let table = a
            .get_mut(at)
            .ok_or_else(|| format!("no entry {at}, the array has {len}"))?;
        return edit_table(table as &mut dyn TableLike, &path[1..], value);
    }
    let exists = at < a.len();
    match (&path[0], value) {
        (Key::Index(_), Some(item)) => {
            if at > a.len() {
                return Err(format!("no room at {at}, the array has {}", a.len()));
            }
            a.insert(at, table_of(item)?);
            Ok(done(None))
        }
        (Key::Index(_), None) => {
            if !exists {
                return Err(format!("no entry {at}, the array has {}", a.len()));
            }
            Ok(done(Some(Item::Table(a.remove(at)))))
        }
        (_, Some(item)) => {
            let table = table_of(item)?;
            if exists {
                let old = a.replace(at, table);
                Ok(done(Some(Item::Table(old))))
            } else {
                a.push(table);
                Ok(done(None))
            }
        }
        (_, None) => {
            if !exists {
                return Err("no such entry to remove".to_string());
            }
            let old = a.remove(at);
            Ok(Done {
                old: Some(Item::Table(old)),
                last: Some(Key::Index(at)),
            })
        }
    }
}

fn edit_array(a: &mut Array, path: &[Key], value: Option<Item>) -> Result<Done, String> {
    let Key::Index(i) = &path[0] else {
        return Err(format!(
            "`{}` is not an index; an array is addressed by position",
            path_text(&path[..1])
        ));
    };
    let i = *i;
    if path.len() > 1 {
        let len = a.len();
        let element = a
            .get_mut(i)
            .ok_or_else(|| format!("no element {i}, the array has {len}"))?;
        return match element {
            Value::InlineTable(t) => edit_table(t as &mut dyn TableLike, &path[1..], value),
            Value::Array(inner) => edit_array(inner, &path[1..], value),
            _ => Err(format!("element {i} has nothing inside it")),
        };
    }
    match value {
        Some(Item::Value(new)) => {
            if i < a.len() {
                let old = a.replace(i, new);
                Ok(done(Some(Item::Value(old))))
            } else if i == a.len() {
                a.push(new);
                Ok(done(None))
            } else {
                Err(format!("no element {i}, the array has {}", a.len()))
            }
        }
        Some(_) => Err("expected a plain value".to_string()),
        None => {
            if i + 1 != a.len() {
                return Err(format!(
                    "only the last element can be removed, the array has {}",
                    a.len()
                ));
            }
            Ok(done(Some(Item::Value(a.remove(i)))))
        }
    }
}
