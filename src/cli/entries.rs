use std::path::Path;
use std::process::ExitCode;

use clap::builder::PossibleValuesParser;
use manfred_recipe::{
    Entry, Loaded, Patch, add_patch, duplicate_patch, entries, load, remove_patch, write_checked,
};
use serde_json::{Value, json};

use super::{pair, read_failure, refuse};

const TABLES: [&str; 10] = [
    "object",
    "camera",
    "light",
    "route",
    "stage",
    "particles",
    "follow",
    "use",
    "mover",
    "marker",
];

#[derive(clap::Args)]
pub(super) struct Add {
    #[arg(value_name = "recipe", help = "The recipe to write")]
    recipe: String,
    #[arg(value_name = "table", value_parser = PossibleValuesParser::new(TABLES), help = "The array of tables to append to")]
    table: String,
    #[arg(
        value_name = "name",
        help = "The new entry's name (a follow's object, a use's prefix, a mover's group)"
    )]
    name: String,
    #[arg(value_name = "key=value", value_parser = pair, help = "The entry's keys; a value that is not TOML is a string, a dotted key makes a table, as params.radius=0.1")]
    pairs: Vec<String>,
    #[arg(long, help = "One JSON object on stdout")]
    json: bool,
}

#[derive(clap::Args)]
pub(super) struct Remove {
    #[arg(value_name = "recipe", help = "The recipe to write")]
    recipe: String,
    #[arg(value_name = "table", value_parser = PossibleValuesParser::new(TABLES), help = "The array of tables to remove from")]
    table: String,
    #[arg(value_name = "name", help = "The entry to remove")]
    name: String,
    #[arg(long, help = "One JSON object on stdout")]
    json: bool,
}

#[derive(clap::Args)]
pub(super) struct Duplicate {
    #[arg(value_name = "recipe", help = "The recipe to write")]
    recipe: String,
    #[arg(value_name = "table", value_parser = PossibleValuesParser::new(TABLES), help = "The array of tables the entry is in")]
    table: String,
    #[arg(value_name = "name", help = "The entry to copy")]
    name: String,
    #[arg(value_name = "new-name", help = "The copy's name")]
    new_name: String,
    #[arg(value_name = "key=value", value_parser = pair, help = "Keys to change on the copy, as at=[1,0,0]")]
    pairs: Vec<String>,
    #[arg(long, help = "One JSON object on stdout")]
    json: bool,
}

#[derive(clap::Args)]
pub(super) struct List {
    #[arg(value_name = "recipe", help = "The recipe to read")]
    recipe: String,
    #[arg(value_name = "table", value_parser = PossibleValuesParser::new(TABLES), help = "Only this array of tables")]
    table: Option<String>,
    #[arg(long, help = "One JSON object on stdout")]
    json: bool,
}

fn described(entry: &Entry) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("table".into(), json!(entry.table));
    out.insert("name".into(), json!(entry.name));
    for (key, value) in &entry.detail {
        out.insert(key.clone(), value.clone());
    }
    Value::Object(out)
}

fn line(entry: &Entry) -> String {
    let mut out = format!("{}[{}]", entry.table, entry.name);
    for (key, value) in &entry.detail {
        let text = match value {
            Value::Null => continue,
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        out.push_str(&format!("  {key} {text}"));
    }
    out
}

fn opened(command: &str, file: &str) -> Result<Loaded, ExitCode> {
    load(Path::new(file)).map_err(|e| refuse(command, &read_failure(file, &e)))
}

fn found(l: &Loaded, table: &str, name: &str) -> Option<Entry> {
    entries(l, Some(table))
        .ok()?
        .into_iter()
        .find(|e| e.name == name)
}

fn written(
    command: &str,
    file: &str,
    table: &str,
    name: &str,
    json: bool,
    patch: impl FnOnce(&Loaded) -> Result<Patch, manfred_recipe::Error>,
) -> ExitCode {
    let mut loaded = match opened(command, file) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let done = patch(&loaded).and_then(|p| write_checked(&mut loaded, &[p]));
    if let Err(e) = done {
        return refuse(command, &read_failure(file, &e));
    }
    let entry = found(&loaded, table, name);
    let verb = match command {
        "add" => "added",
        "duplicate" => "duplicated",
        _ => "removed",
    };
    if json {
        println!(
            "{}",
            json!({
                "recipe": file,
                "done": verb,
                "table": table,
                "name": name,
                "entry": entry.as_ref().map(described),
            })
        );
    } else {
        match &entry {
            Some(entry) => println!("{file}: {verb} {}", line(entry)),
            None => println!("{file}: {verb} {table}[{name}]"),
        }
    }
    ExitCode::SUCCESS
}

pub fn add(args: &Add) -> ExitCode {
    written(
        "add",
        &args.recipe,
        &args.table,
        &args.name,
        args.json,
        |l| add_patch(l, &args.table, &args.name, &args.pairs),
    )
}

pub fn remove(args: &Remove) -> ExitCode {
    written(
        "remove",
        &args.recipe,
        &args.table,
        &args.name,
        args.json,
        |l| remove_patch(l, &args.table, &args.name),
    )
}

pub fn duplicate(args: &Duplicate) -> ExitCode {
    written(
        "duplicate",
        &args.recipe,
        &args.table,
        &args.new_name,
        args.json,
        |l| duplicate_patch(l, &args.table, &args.name, &args.new_name, &args.pairs),
    )
}

pub fn list(args: &List) -> ExitCode {
    let file = args.recipe.as_str();
    let loaded = match opened("list", file) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let found = match entries(&loaded, args.table.as_deref()) {
        Ok(found) => found,
        Err(e) => return refuse("list", &read_failure(file, &e)),
    };
    if args.json {
        let list: Vec<Value> = found.iter().map(described).collect();
        println!("{}", json!({ "recipe": file, "entries": list }));
    } else {
        for entry in &found {
            println!("{}", line(entry));
        }
    }
    ExitCode::SUCCESS
}
