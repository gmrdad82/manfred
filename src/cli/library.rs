use std::path::{Path, PathBuf};
use std::process::ExitCode;

use manfred_build::Registry;
use manfred_build::library::{self, Entry, Problem};
use serde_json::{Value, json};

use super::refuse;

#[derive(clap::Args)]
pub(super) struct Args {
    #[command(subcommand)]
    command: LibraryCommand,
}

#[derive(clap::Args)]
pub(super) struct Common {
    #[arg(
        long,
        value_name = "dir",
        help = "The library folder; without it, the nearest library/ holding an index.json from here up"
    )]
    library: Option<PathBuf>,
    #[arg(long, help = "One JSON object on stdout")]
    json: bool,
}

#[derive(clap::Args)]
pub(super) struct List {
    #[arg(long, value_name = "kind", help = "Only the objects of this kind")]
    kind: Option<String>,
    #[command(flatten)]
    common: Common,
}

#[derive(clap::Subcommand)]
enum LibraryCommand {
    #[command(about = "List the library's objects from its index.json")]
    List(List),
    #[command(
        about = "Rewrite index.json from the folders: kinds, names, tags, sizes, triangles and sha256s"
    )]
    Index(Common),
    #[command(
        about = "Validate the sidecars, the index, the sha256s and the licence line, and each model with gltf_validator when it is on the PATH"
    )]
    Check(Common),
    #[command(about = "Build a recipe's object into library/<kind>/<name>")]
    Bake(super::library_bake::Args),
    #[command(about = "Bring a glTF 2.0 binary file into library/<kind>/<name>")]
    Add(super::library_add::Args),
}

const COMMAND: &str = "library";

fn root(given: Option<&Path>, needs_index: bool) -> Result<PathBuf, String> {
    if let Some(dir) = given {
        return if dir.is_dir() {
            Ok(dir.to_path_buf())
        } else {
            Err(format!("{} is not a folder", dir.display()))
        };
    }
    let here = std::env::current_dir().map_err(|e| format!("no current folder: {e}"))?;
    if let Some(found) = library::find_root(&here) {
        return Ok(found);
    }
    let below = here.join(library::FOLDER);
    let named = here
        .file_name()
        .is_some_and(|n| n == library::FOLDER)
        .then(|| here.clone());
    match named.or_else(|| below.is_dir().then_some(below)) {
        Some(dir) if !needs_index => Ok(dir),
        _ => Err(format!(
            "no library/ with an index.json from here up; pass --library <dir>{}",
            if needs_index {
                ", or run `manfred library index` in it"
            } else {
                ""
            }
        )),
    }
}

fn line(entry: &Entry) -> String {
    let [x, y, z] = entry.size_m;
    let tags = if entry.tags.is_empty() {
        String::new()
    } else {
        format!("  [{}]", entry.tags.join(", "))
    };
    format!(
        "{}/{}  {} triangles  {x:.2} x {y:.2} x {z:.2} m{tags}",
        entry.kind, entry.name, entry.triangles
    )
}

fn list(args: &List) -> ExitCode {
    let dir = match root(args.common.library.as_deref(), true) {
        Ok(dir) => dir,
        Err(text) => return refuse(COMMAND, &text),
    };
    let entries = match library::read_index(&dir) {
        Ok(entries) => entries,
        Err(e) => return refuse(COMMAND, &format!("{}: {}", e.path, e.text)),
    };
    let chosen: Vec<&Entry> = entries
        .iter()
        .filter(|e| args.kind.as_ref().is_none_or(|k| &e.kind == k))
        .collect();
    if args.common.json {
        let objects: Vec<Value> = chosen.iter().map(|e| e.json()).collect();
        println!(
            "{}",
            json!({ "library": dir.display().to_string(), "objects": objects })
        );
    } else {
        for entry in &chosen {
            println!("{}", line(entry));
        }
    }
    ExitCode::SUCCESS
}

fn index(args: &Common) -> ExitCode {
    let dir = match root(args.library.as_deref(), false) {
        Ok(dir) => dir,
        Err(text) => return refuse(COMMAND, &text),
    };
    let written = library::write_index(&dir)
        .and_then(|path| library::read_index(&dir).map(|entries| (path, entries.len())));
    match written {
        Ok((path, count)) => {
            if args.json {
                println!(
                    "{}",
                    json!({ "index": path.display().to_string(), "objects": count })
                );
            } else {
                println!("wrote {} ({count} objects)", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => refuse(COMMAND, &format!("{}: {}", e.path, e.text)),
    }
}

fn validator(dir: &Path, problems: &mut Vec<Problem>) -> Option<usize> {
    let mut checked = 0;
    let entries = library::index(dir).ok()?;
    for entry in entries {
        let model = dir.join(&entry.model);
        let run = std::process::Command::new("gltf_validator")
            .arg("-o")
            .arg(&model)
            .output();
        let output = match run {
            Ok(output) => output,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => {
                problems.push(Problem {
                    path: entry.model.clone(),
                    text: format!("gltf_validator did not run: {e}"),
                });
                continue;
            }
        };
        checked += 1;
        let report: Option<Value> = serde_json::from_slice(&output.stdout).ok();
        let errors = report
            .as_ref()
            .and_then(|r| r.get("issues"))
            .and_then(|i| i.get("numErrors"))
            .and_then(Value::as_u64);
        match errors {
            Some(0) => {}
            Some(n) => problems.push(Problem {
                path: entry.model.clone(),
                text: format!("gltf_validator reports {n} errors"),
            }),
            None if output.status.success() => {}
            None => problems.push(Problem {
                path: entry.model.clone(),
                text: format!(
                    "gltf_validator failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            }),
        }
    }
    Some(checked)
}

fn check(args: &Common) -> ExitCode {
    let dir = match root(args.library.as_deref(), false) {
        Ok(dir) => dir,
        Err(text) => return refuse(COMMAND, &text),
    };
    let mut problems = library::check(&dir);
    let validated = validator(&dir, &mut problems);
    let objects = library::index(&dir).map_or(0, |e| e.len());
    if args.json {
        let list: Vec<Value> = problems
            .iter()
            .map(|p| json!({ "path": p.path, "text": p.text }))
            .collect();
        println!(
            "{}",
            json!({
                "library": dir.display().to_string(),
                "objects": objects,
                "validator": validated.is_some(),
                "problems": list,
            })
        );
    } else {
        for p in &problems {
            println!("{}: {}", p.path, p.text);
        }
        if validated.is_none() {
            eprintln!(
                "manfred library: gltf_validator is not on the PATH, so the models were not validated"
            );
        }
        if problems.is_empty() {
            println!("{objects} objects, no problems");
        }
    }
    if problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

pub fn run(args: &Args, registry: &Registry) -> ExitCode {
    match &args.command {
        LibraryCommand::List(list_args) => list(list_args),
        LibraryCommand::Index(common) => index(common),
        LibraryCommand::Check(common) => check(common),
        LibraryCommand::Bake(parsed) => super::library_bake::run(parsed, registry),
        LibraryCommand::Add(parsed) => super::library_add::run(parsed, registry),
    }
}
