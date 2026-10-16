use std::path::{Path, PathBuf};
use std::process::ExitCode;

use manfred_build::gltf_io::{self, Imported};
use manfred_build::{Registry, build_with};
use serde_json::{Value, json};

use super::refuse;

const COMMAND: &str = "import";
const THUMB: [u32; 2] = [256, 256];

#[derive(clap::Args)]
#[command(group(clap::ArgGroup::new("into").required(true).args(["as_name", "out"])))]
pub(super) struct Args {
    #[arg(
        value_name = "file.glb",
        help = "A glTF 2.0 binary (a Blender export included)"
    )]
    file: PathBuf,
    #[arg(
        long = "as",
        id = "as_name",
        value_name = "kind/name",
        help = "Write library/<kind>/<name>.manf under the current folder"
    )]
    as_name: Option<String>,
    #[arg(long, value_name = "x.manf", help = "Write the .manf here")]
    out: Option<PathBuf>,
    #[arg(
        long,
        value_name = "tag,tag",
        value_delimiter = ',',
        help = "Tags kept in the document's facts"
    )]
    tags: Vec<String>,
    #[arg(
        long,
        help = "Render a 256x256 thumbnail into the document (GPU, headless)"
    )]
    thumb: bool,
    #[arg(long, help = "One JSON object on stdout")]
    json: bool,
}

fn target(args: &Args) -> Result<(PathBuf, String), String> {
    let stem = || {
        args.file
            .file_stem()
            .map_or_else(|| "model".to_string(), |s| s.to_string_lossy().to_string())
    };
    match (&args.as_name, &args.out) {
        (Some(name), out) => {
            let (kind, base) = name
                .split_once('/')
                .filter(|(k, n)| !k.is_empty() && !n.is_empty() && !n.contains('/'))
                .ok_or_else(|| format!("--as expects <kind>/<name>, found \"{name}\""))?;
            let path = out
                .clone()
                .unwrap_or_else(|| Path::new("library").join(kind).join(format!("{base}.manf")));
            Ok((path, name.clone()))
        }
        (None, Some(out)) => Ok((out.clone(), stem())),
        (None, None) => Err("give --as <kind>/<name> or --out <x.manf>".into()),
    }
}

fn framed(facts: &serde_json::Map<String, Value>) -> ([f64; 3], [f64; 3]) {
    let corner = |key: &str, fallback: f64| -> [f64; 3] {
        facts
            .get(key)
            .and_then(Value::as_array)
            .map_or([fallback; 3], |v| {
                [0, 1, 2].map(|i| v.get(i).and_then(Value::as_f64).unwrap_or(fallback))
            })
    };
    let (lo, hi) = (corner("min", -0.5), corner("max", 0.5));
    let centre = [0, 1, 2].map(|i| (lo[i] + hi[i]) * 0.5);
    let radius = ([0, 1, 2]
        .map(|i| (hi[i] - lo[i]).powi(2))
        .iter()
        .sum::<f64>()
        .sqrt()
        * 0.5)
        .max(0.05);
    let distance = radius / (20.0_f64).to_radians().sin() * 1.05;
    let toward = [1.0, 0.6, 1.0];
    let length = (toward.iter().map(|v| v * v).sum::<f64>()).sqrt();
    let eye = [0, 1, 2].map(|i| centre[i] + toward[i] / length * distance);
    (eye, centre)
}

fn thumbnail(
    file: &Path,
    out: &Path,
    imported: &Imported,
    registry: &Registry,
) -> Result<Vec<u8>, String> {
    let absolute = std::path::absolute(file).map_err(|e| e.to_string())?;
    let (eye, centre) = framed(&imported.facts);
    let text = format!(
        "manfred = 1\nname = \"thumb\"\n\n[[object]]\nname = \"model\"\nkind = \"mesh\"\npath = {}\n\n[[camera]]\nname = \"thumb\"\nposition = {eye:?}\ntarget = {centre:?}\nfov_y_deg = 40.0\n\n[sky]\nkind = \"analytic\"\nintensity = 1.0\n",
        manfred_recipe::toml_edit::Value::from(absolute.to_string_lossy().as_ref())
            .to_string()
            .trim()
    );
    let recipe = manfred_recipe::parse(&text).map_err(|e| e.text)?;
    let base = absolute.parent().unwrap_or(Path::new(""));
    let built = build_with(&recipe, base, registry).map_err(|e| e.to_string())?;
    let merged = built.recipe(&recipe);
    let gpu = manfred_scene::headless().map_err(|e| e.0)?;
    let rgba =
        manfred_scene::shoot(&gpu, &merged, &built.parts, "thumb", THUMB).map_err(|e| e.0)?;
    let png = out.with_extension("thumb.png");
    manfred_scene::write_png(&png, THUMB, &rgba).map_err(|e| e.0)?;
    let bytes = std::fs::read(&png).map_err(|e| format!("{}: {e}", png.display()));
    let _ = std::fs::remove_file(&png);
    bytes
}

pub fn run(args: &Args, registry: &Registry) -> ExitCode {
    let (out, name) = match target(args) {
        Ok(found) => found,
        Err(text) => return refuse(COMMAND, &text),
    };
    let file = args.file.display().to_string();
    let bytes = match std::fs::read(&args.file) {
        Ok(bytes) => bytes,
        Err(e) => return refuse(COMMAND, &format!("{file}: {e}")),
    };
    let validated = match gltf_io::validator(&args.file) {
        None => "absent",
        Some(Ok(())) => "passed",
        Some(Err(text)) => return refuse(COMMAND, &format!("{file}: {text}")),
    };
    let imported = match gltf_io::import(&bytes, &name, &args.tags) {
        Ok(imported) => imported,
        Err(text) => return refuse(COMMAND, &format!("{file}: {text}")),
    };
    let thumb = if args.thumb {
        match thumbnail(&args.file, &out, &imported, registry) {
            Ok(png) => Some(png),
            Err(text) => return refuse(COMMAND, &format!("--thumb: {text}")),
        }
    } else {
        None
    };
    if let Err(text) = imported.write(&out, thumb.as_deref()) {
        return refuse(COMMAND, &text);
    }
    let size = std::fs::metadata(&out).map_or(0, |m| m.len());
    if args.json {
        println!(
            "{}",
            json!({
                "out": out.display().to_string(),
                "kind": imported.kind,
                "name": imported.name,
                "bytes": size,
                "nodes": imported.nodes.len(),
                "thumb": thumb.is_some(),
                "validator": validated,
                "facts": imported.facts,
            })
        );
    } else {
        let wheels = imported
            .facts
            .get("wheels")
            .and_then(Value::as_object)
            .map_or(0, |w| w.len());
        let anchors = imported
            .facts
            .get("anchors")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        println!(
            "{}: {}/{}, {} nodes, {wheels} wheels, {anchors} light anchors, {size} bytes{}",
            out.display(),
            imported.kind,
            imported.name,
            imported.nodes.len(),
            if thumb.is_some() { ", thumbnail" } else { "" }
        );
    }
    ExitCode::SUCCESS
}
