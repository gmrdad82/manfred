use std::path::{Path, PathBuf};
use std::process::ExitCode;

use manfred_build::Registry;
use manfred_recipe::toml_edit::Value as Toml;
use manfred_recipe::{Change, Loaded, json_of, load, save, set_values, tidy, value_of};
use serde_json::{Value, json};

use super::{check, pair, read_failure, refuse};

#[derive(clap::Args)]
pub(super) struct SetArgs {
    #[arg(value_name = "recipe", help = "The recipe to write")]
    recipe: String,
    #[arg(
        value_name = "key=value",
        required = true,
        num_args = 1..,
        value_parser = pair,
        help = "A dotted key, as --set takes it, and its new value as TOML of the kind it replaces"
    )]
    sets: Vec<String>,
    #[arg(long, help = "One JSON object on stdout")]
    json: bool,
}

#[derive(clap::Args)]
pub(super) struct GetArgs {
    #[arg(value_name = "recipe", help = "The recipe to read")]
    recipe: String,
    #[arg(
        value_name = "key",
        required = true,
        num_args = 1..,
        help = "A dotted key, as --set takes it"
    )]
    keys: Vec<String>,
    #[arg(long, help = "One JSON object on stdout")]
    json: bool,
}

fn file_of(recipe: &str, name: &str) -> String {
    let main = Path::new(recipe);
    if main.file_name().is_some_and(|n| n == name) {
        return recipe.to_string();
    }
    main.parent()
        .map_or_else(|| PathBuf::from(name), |p| p.join(name))
        .display()
        .to_string()
}

type Saved = Vec<(PathBuf, Option<Vec<u8>>)>;

fn originals(recipe: &str, loaded: &Loaded) -> Saved {
    let mut paths = vec![PathBuf::from(recipe)];
    paths.extend(loaded.recipe.files.paths().into_iter().skip(1));
    paths
        .into_iter()
        .map(|p| {
            let bytes = std::fs::read(&p).ok();
            (p, bytes)
        })
        .collect()
}

fn restore(saved: &Saved) -> Result<(), String> {
    for (path, bytes) in saved {
        if std::fs::read(path).ok() == *bytes {
            continue;
        }
        match bytes {
            Some(bytes) => std::fs::write(path, bytes),
            None => std::fs::remove_file(path),
        }
        .map_err(|e| format!("cannot restore {}: {e}", path.display()))?;
    }
    Ok(())
}

fn rechecked(file: &str, registry: &Registry) -> Result<(), String> {
    let loaded = load(Path::new(file)).map_err(|e| read_failure(file, &e))?;
    let problems = check::problems(&loaded.recipe, file, registry);
    if problems.is_empty() {
        return Ok(());
    }
    let lines: Vec<String> = problems
        .iter()
        .map(|p| format!("{file}:{}: {}: {}", p.line.unwrap_or(1), p.path, p.text))
        .collect();
    Err(lines.join("; "))
}

fn shown(value: Option<&Toml>) -> String {
    value.map_or_else(|| "absent".to_string(), |v| tidy(v).to_string())
}

fn described(recipe: &str, change: &Change) -> Value {
    json!({
        "key": change.key,
        "old": change.old.as_ref().map(json_of),
        "new": json_of(&change.new),
        "file": file_of(recipe, &change.file),
    })
}

pub fn run_set(args: &SetArgs, registry: &Registry) -> ExitCode {
    match set(args, registry) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => refuse("set", &message),
    }
}

fn set(args: &SetArgs, registry: &Registry) -> Result<(), String> {
    let file = args.recipe.as_str();
    let mut loaded = load(Path::new(file)).map_err(|e| read_failure(file, &e))?;
    let saved = originals(file, &loaded);
    let changes = set_values(&mut loaded, &args.sets).map_err(|e| read_failure(file, &e))?;
    let written = save(&loaded)
        .map_err(|e| read_failure(file, &e))
        .and_then(|()| rechecked(file, registry));
    if let Err(problem) = written {
        let restored = match restore(&saved) {
            Ok(()) => "the recipe is left as it was".to_string(),
            Err(e) => e,
        };
        return Err(format!("{problem}; {restored}"));
    }
    if args.json {
        let set: Vec<Value> = changes.iter().map(|c| described(file, c)).collect();
        println!("{}", json!({ "recipe": file, "set": set, "check": "ok" }));
        return Ok(());
    }
    for change in &changes {
        println!(
            "{}: {} = {}  (was {})",
            file_of(file, &change.file),
            change.key,
            tidy(&change.new),
            shown(change.old.as_ref())
        );
    }
    println!("check: ok");
    Ok(())
}

pub fn run_get(args: &GetArgs, _: &Registry) -> ExitCode {
    match get(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => refuse("get", &message),
    }
}

fn get(args: &GetArgs) -> Result<(), String> {
    let file = args.recipe.as_str();
    let loaded = load(Path::new(file)).map_err(|e| read_failure(file, &e))?;
    let values = args
        .keys
        .iter()
        .map(|key| {
            let key = key.trim();
            value_of(&loaded, key)
                .map(|value| (key, value))
                .map_err(|e| read_failure(file, &e))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if args.json {
        let list: Vec<Value> = values
            .iter()
            .map(|(key, value)| json!({ "key": key, "value": json_of(value) }))
            .collect();
        println!("{}", json!({ "recipe": file, "values": list }));
    } else {
        for (key, value) in &values {
            println!("{key} = {}", tidy(value));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECIPE: &str = "manfred = 1\nname = \"t\"\n\n# the grade\n[look]\nexposure = 3.0 # bright\n\n[[object]]\nname = \"box\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n\n[materials.default]\nbase = [0.5, 0.5, 0.5]\n";

    fn written(name: &str) -> String {
        let dir = "tmp/set-tests";
        std::fs::create_dir_all(dir).unwrap();
        let file = format!("{dir}/{name}");
        std::fs::write(&file, RECIPE).unwrap();
        file
    }

    fn set_args(file: &str, sets: &[&str]) -> SetArgs {
        SetArgs {
            recipe: file.to_string(),
            sets: sets.iter().map(|s| s.to_string()).collect(),
            json: true,
        }
    }

    #[test]
    fn a_set_that_breaks_the_recipe_is_refused_and_leaves_the_file_as_it_was() {
        let file = written("broken.toml");
        let e = set(&set_args(&file, &["look.exposure=-1"]), &Registry::new()).unwrap_err();
        assert!(
            e.contains("look.exposure: expected a number above 0"),
            "{e}"
        );
        assert!(e.ends_with("the recipe is left as it was"), "{e}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), RECIPE);
        let e = set(&set_args(&file, &["look.exposure=dark"]), &Registry::new()).unwrap_err();
        assert!(e.contains("look.exposure"), "{e}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), RECIPE);
    }

    #[test]
    fn a_set_writes_the_file_and_get_reads_it_back() {
        let file = written("kept.toml");
        set(&set_args(&file, &["look.exposure=2.5"]), &Registry::new()).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            text,
            RECIPE.replace("exposure = 3.0 # bright", "exposure = 2.5 # bright")
        );
        let loaded = load(Path::new(&file)).unwrap();
        assert_eq!(
            tidy(&value_of(&loaded, "object.box.size").unwrap()).to_string(),
            "[1.0, 1.0, 1.0]"
        );
        assert_eq!(
            json_of(&value_of(&loaded, "look").unwrap()),
            json!({ "exposure": 2.5 })
        );
        assert!(value_of(&loaded, "look.view").is_err());
    }
}
