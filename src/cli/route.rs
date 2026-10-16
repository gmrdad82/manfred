use std::path::Path;
use std::process::ExitCode;

use manfred_build::Registry;
use manfred_recipe::{PointEdit, RouteSpec, apply, load, route_patch, save};
use serde_json::{Value, json};

use super::{read_failure, refuse};

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Action {
    List,
    Set,
    Add,
    Remove,
}

fn xyz(text: &str) -> Result<[f64; 3], String> {
    let parts: Vec<f64> = text
        .split(',')
        .map(|c| c.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| "not x,y,z in metres".to_string())?;
    match parts.as_slice() {
        [x, y, z] if parts.iter().all(|c| c.is_finite()) => Ok([*x, *y, *z]),
        _ => Err("not x,y,z in metres".to_string()),
    }
}

#[derive(clap::Args)]
pub(super) struct Args {
    #[arg(
        value_name = "recipe",
        help = "The recipe whose routes to list or edit"
    )]
    pub(super) recipe: String,
    #[arg(
        value_enum,
        value_name = "action",
        help = "list: the points and the length; set: move point --at to --point; add: insert --point at --at, or append; remove: delete point --at"
    )]
    action: Action,
    #[arg(
        long,
        value_name = "name",
        required_if_eq_any = [("action", "set"), ("action", "add"), ("action", "remove")],
        help = "The route, by name; list shows every route without it"
    )]
    route: Option<String>,
    #[arg(long, value_name = "i", required_if_eq_any = [("action", "set"), ("action", "remove")], help = "The point's index, from 0")]
    at: Option<usize>,
    #[arg(
        long,
        value_name = "x,y,z",
        value_parser = xyz,
        allow_hyphen_values = true,
        required_if_eq_any = [("action", "set"), ("action", "add")],
        help = "The point, in metres, Y up"
    )]
    point: Option<[f64; 3]>,
    #[arg(long, help = "One JSON object on stdout")]
    json: bool,
}

fn described(route: &RouteSpec) -> Value {
    let length = manfred_motion::Route::new(&route.points, route.closed);
    let mut out = json!({
        "name": route.name,
        "closed": route.closed,
        "points": route.points,
        "length": length.as_ref().ok().map(manfred_motion::Route::length),
    });
    if let Err(problem) = length {
        out["problem"] = json!(problem);
    }
    out
}

fn printed(route: &Value) {
    let points = route["points"].as_array().map_or(0, Vec::len);
    let length = route["length"]
        .as_f64()
        .map_or_else(|| "no length".to_string(), |l| format!("{l:.3} m"));
    let shape = if route["closed"] == true {
        "closed"
    } else {
        "open"
    };
    println!(
        "{}  {points} points  {shape}  {length}",
        route["name"].as_str().unwrap_or_default()
    );
    for (i, p) in route["points"].as_array().into_iter().flatten().enumerate() {
        let c: Vec<String> = p
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| c.as_f64().unwrap_or_default().to_string())
            .collect();
        println!("  {i}  {}", c.join(", "));
    }
    if let Some(problem) = route["problem"].as_str() {
        println!("  {problem}");
    }
}

pub fn run(args: &Args, _: &Registry) -> ExitCode {
    let file = args.recipe.as_str();
    let mut loaded = match load(Path::new(file)) {
        Ok(loaded) => loaded,
        Err(e) => return refuse("route", &read_failure(file, &e)),
    };
    let edit = match (args.action, args.at, args.point) {
        (Action::List, _, _) => None,
        (Action::Set, Some(i), Some(p)) => Some(PointEdit::Set(i, p)),
        (Action::Add, i, Some(p)) => Some(PointEdit::Add(i, p)),
        (Action::Remove, Some(i), _) => Some(PointEdit::Remove(i)),
        _ => {
            return refuse(
                "route",
                "set and add take --point, set and remove take --at",
            );
        }
    };
    let Some(edit) = edit else {
        let chosen: Vec<&RouteSpec> = match &args.route {
            Some(name) => match loaded.recipe.route_named(name) {
                Ok(route) => vec![route],
                Err(e) => return refuse("route", &read_failure(file, &e)),
            },
            None => loaded.recipe.routes.iter().collect(),
        };
        let routes: Vec<Value> = chosen.into_iter().map(described).collect();
        if args.json {
            println!("{}", json!({ "recipe": file, "routes": routes }));
        } else {
            routes.iter().for_each(printed);
        }
        return ExitCode::SUCCESS;
    };
    let name = args.route.as_deref().unwrap_or_default();
    let done = route_patch(&loaded, name, edit)
        .and_then(|patch| apply(&mut loaded, &patch))
        .and_then(|_| save(&loaded));
    if let Err(e) = done {
        return refuse("route", &read_failure(file, &e));
    }
    let Some(route) = loaded.recipe.route(name).map(described) else {
        return refuse(
            "route",
            &format!("{file}: route[{name}]: gone after the edit"),
        );
    };
    if args.json {
        println!("{}", json!({ "recipe": file, "route": route }));
    } else {
        printed(&route);
    }
    ExitCode::SUCCESS
}
