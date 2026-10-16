use std::path::Path;
use std::process::ExitCode;

use manfred_recipe::{
    CurveEdit, Kind, Loaded, Ortho, apply, curve_patch, curve_points, curves_of, load, save,
};
use serde_json::{Value, json};

use super::{read_failure, refuse};

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Action {
    List,
    Set,
    Add,
    Remove,
}

fn ab(text: &str) -> Result<[f64; 2], String> {
    let parts: Vec<f64> = text
        .split(',')
        .map(|c| c.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| "not a,b in metres".to_string())?;
    match parts.as_slice() {
        [a, b] if parts.iter().all(|c| c.is_finite()) => Ok([*a, *b]),
        _ => Err("not a,b in metres".to_string()),
    }
}

#[derive(clap::Args)]
pub(super) struct Args {
    #[arg(value_name = "recipe", help = "The recipe whose curve to list or edit")]
    recipe: String,
    #[arg(value_name = "object", help = "The generator object, by name")]
    object: String,
    #[arg(
        value_name = "param",
        help = "The curve's path in the object's params, as stations.0.curve or silhouettes.side"
    )]
    param: String,
    #[arg(
        value_enum,
        value_name = "action",
        help = "list: the points; set: move point --at to --point; add: insert --point at --at, or append; remove: delete point --at"
    )]
    action: Action,
    #[arg(long, value_name = "i", required_if_eq_any = [("action", "set"), ("action", "remove")], help = "The point's index, from 0")]
    at: Option<usize>,
    #[arg(
        long,
        value_name = "a,b",
        value_parser = ab,
        allow_hyphen_values = true,
        required_if_eq_any = [("action", "set"), ("action", "add")],
        help = "The point, in metres, in the curve's own two coordinates"
    )]
    point: Option<[f64; 2]>,
    #[arg(long, help = "One JSON object on stdout")]
    json: bool,
}

fn view_of(loaded: &Loaded, object: &str, param: &str) -> Option<&'static str> {
    let found = loaded.recipe.objects.iter().find(|o| o.name == object)?;
    let Kind::Generator { params, .. } = &found.kind else {
        return None;
    };
    let wanted = param.replace('[', ".").replace(']', "");
    curves_of(params)
        .into_iter()
        .find(|c| c.path == wanted)
        .and_then(|c| c.view)
        .map(Ortho::word)
}

fn printed(out: &Value) {
    let points = out["points"].as_array().map_or(0, Vec::len);
    let view = out["view"].as_str().unwrap_or("front");
    println!(
        "{}.{}  {points} points  {view}",
        out["object"].as_str().unwrap_or_default(),
        out["param"].as_str().unwrap_or_default()
    );
    for (i, p) in out["points"].as_array().into_iter().flatten().enumerate() {
        let c: Vec<String> = p
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| c.as_f64().unwrap_or_default().to_string())
            .collect();
        println!("  {i}  {}", c.join(", "));
    }
}

pub fn run(args: &Args) -> ExitCode {
    let file = args.recipe.as_str();
    let mut loaded = match load(Path::new(file)) {
        Ok(loaded) => loaded,
        Err(e) => return refuse("curve", &read_failure(file, &e)),
    };
    let edit = match (args.action, args.at, args.point) {
        (Action::List, _, _) => None,
        (Action::Set, Some(i), Some(p)) => Some(CurveEdit::Set(i, p)),
        (Action::Add, i, Some(p)) => Some(CurveEdit::Add(i, p)),
        (Action::Remove, Some(i), _) => Some(CurveEdit::Remove(i)),
        _ => {
            return refuse(
                "curve",
                "set and add take --point, set and remove take --at",
            );
        }
    };
    let (object, param) = (args.object.as_str(), args.param.as_str());
    if let Some(edit) = edit {
        let done = curve_patch(&loaded, object, param, edit)
            .and_then(|patch| apply(&mut loaded, &patch))
            .and_then(|_| save(&loaded));
        if let Err(e) = done {
            return refuse("curve", &read_failure(file, &e));
        }
    }
    let points = match curve_points(&loaded, object, param) {
        Ok(points) => points,
        Err(e) => return refuse("curve", &read_failure(file, &e)),
    };
    let out = json!({
        "recipe": file,
        "object": object,
        "param": param,
        "view": view_of(&loaded, object, param),
        "points": points,
    });
    if args.json {
        println!("{out}");
    } else {
        printed(&out);
    }
    ExitCode::SUCCESS
}
