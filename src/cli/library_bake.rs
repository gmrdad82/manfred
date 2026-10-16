use std::path::{Path, PathBuf};
use std::process::ExitCode;

use glam::DVec3;
use manfred_build::{Built, Registry, build_with, library};
use manfred_recipe::{Camera, Kind, Object, Projection, Recipe, load};

use super::refuse;

pub const THUMB: [u32; 2] = [512, 512];
const THUMB_CAMERA: &str = "library_thumb";
const GROUND: &str = "library_thumb_ground";
const FOV_DEG: f64 = 30.0;

#[derive(clap::Args, Debug, PartialEq)]
pub(super) struct Args {
    #[arg(value_name = "recipe", help = "The recipe that builds the object")]
    recipe: PathBuf,
    #[arg(value_name = "object", help = "The recipe's object to bake")]
    object: String,
    #[arg(
        long = "as",
        value_name = "kind/name",
        value_parser = entry_name,
        help = "Where it goes in the library: library/<kind>/<name>"
    )]
    entry: String,
    #[arg(
        long,
        help = "Also render a 512x512 thumbnail, thumb.png, on the live renderer"
    )]
    thumb: bool,
    #[arg(
        long,
        value_name = "a,b",
        value_delimiter = ',',
        help = "Tags for its object.toml and the index"
    )]
    tags: Vec<String>,
    #[arg(
        long,
        value_name = "dir",
        help = "The library folder; by default the nearest folder named library holding the recipe, else library/ beside it"
    )]
    library: Option<PathBuf>,
    #[arg(long, help = "Answer with one JSON object")]
    json: bool,
}

pub fn word(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub fn entry_name(text: &str) -> Result<String, String> {
    match text.split_once('/') {
        Some((kind, name)) if word(kind) && word(name) => Ok(text.to_string()),
        _ => Err("expected <kind>/<name>, each of a-z, 0-9, - and _".into()),
    }
}

pub fn root_of(recipe: &Path) -> PathBuf {
    let folder = recipe.parent().unwrap_or(Path::new(""));
    folder
        .ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "library"))
        .map_or_else(|| folder.join("library"), Path::to_path_buf)
}

pub struct Subject {
    pub recipe: Recipe,
    pub built: Built,
    pub index: usize,
    pub base: PathBuf,
}

pub struct Baked {
    pub dir: PathBuf,
    pub model: PathBuf,
    pub sidecar: PathBuf,
    pub model_bytes: usize,
    pub subject: Subject,
}

fn failed(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub fn bake(
    path: &Path,
    object: &str,
    entry: &str,
    tags: &[String],
    root: &Path,
    registry: &Registry,
) -> Result<Baked, String> {
    let recipe = load(path).map_err(failed)?.recipe;
    let index = recipe
        .objects
        .iter()
        .position(|o| o.name == object)
        .ok_or_else(|| format!("{}: no object is named `{object}`", path.display()))?;
    let base = path.parent().unwrap_or(Path::new(""));
    let built = build_with(&recipe, base, registry).map_err(failed)?;
    let merged = built.recipe(&recipe);
    let mut exported = library::export(&merged, &built, object, entry).map_err(failed)?;
    exported.sidecar.tags = tags.to_vec();
    let dir = root.join(entry);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let model = dir.join("model.glb");
    let sidecar = dir.join("object.toml");
    std::fs::write(&model, &exported.glb).map_err(|e| format!("{}: {e}", model.display()))?;
    std::fs::write(&sidecar, exported.sidecar.text())
        .map_err(|e| format!("{}: {e}", sidecar.display()))?;
    Ok(Baked {
        dir,
        model,
        sidecar,
        model_bytes: exported.glb.len(),
        subject: Subject {
            recipe,
            built,
            index,
            base: base.to_path_buf(),
        },
    })
}

fn bounds(built: &Built, index: usize) -> Option<(DVec3, DVec3)> {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for part in built
        .parts
        .iter()
        .filter(|p| p.object == index && !p.hidden)
    {
        for p in &part.node.mesh.positions {
            let at = part.node.transform.transform_point3(DVec3::new(
                p[0].into(),
                p[1].into(),
                p[2].into(),
            ));
            lo = lo.min(at);
            hi = hi.max(at);
        }
    }
    lo.x.is_finite().then_some((lo, hi))
}

fn ground(like: &Object, at: DVec3, side: f64) -> Object {
    Object {
        name: GROUND.into(),
        id: None,
        kind: Kind::Plane {
            size: [side, side],
            grid: None,
            bend: None,
            curl: None,
            cup: None,
            jag: Vec::new(),
        },
        at: at.to_array(),
        rotate: [0.0; 3],
        scale: [1.0; 3],
        keep_transform: false,
        parent: None,
        material: Some(GROUND.into()),
        materials: Vec::new(),
        smooth_angle: None,
        smooth: false,
        flat_faces: false,
        bevel: None,
        uv: None,
        detail_uv: None,
        shadow: like.shadow,
        two_sided: false,
        hidden: false,
        dynamic: None,
        proxy: None,
        lod: Vec::new(),
    }
}

pub fn framing(lo: DVec3, hi: DVec3) -> ([f64; 3], [f64; 3]) {
    let centre = (lo + hi) * 0.5;
    let radius = ((hi - lo).length() * 0.5).max(0.05);
    let distance = radius / (FOV_DEG.to_radians() * 0.5).sin() * 1.02;
    let toward = DVec3::new(1.0, 0.6, 1.0).normalize();
    ((centre + toward * distance).to_array(), centre.to_array())
}

fn thumb_recipe(subject: &Subject) -> Result<Recipe, String> {
    let (lo, hi) = bounds(&subject.built, subject.index).ok_or_else(|| {
        format!(
            "{}: nothing to frame",
            subject.recipe.objects[subject.index].name
        )
    })?;
    let (position, target) = framing(lo, hi);
    let mut recipe = subject.recipe.clone();
    let side = ((hi - lo).length() * 8.0).max(20.0);
    let centre = (lo + hi) * 0.5;
    let floor = DVec3::new(centre.x, lo.y - 0.01, centre.z);
    let like = recipe.objects[subject.index].clone();
    recipe.objects.push(ground(&like, floor, side));
    recipe.materials.insert(
        GROUND.into(),
        serde_json::json!({"base": [0.34, 0.34, 0.33], "roughness": 0.85}),
    );
    recipe.cameras.push(Camera {
        name: THUMB_CAMERA.into(),
        kind: Projection::Perspective,
        position,
        target,
        up: [0.0, 1.0, 0.0],
        fov_y_deg: Some(FOV_DEG),
        focal_mm: None,
        sensor_mm: None,
        shift: [0.0, 0.0],
        focus_m: None,
        fstop: 0.0,
        ortho_height: None,
        path: None,
        rig: None,
    });
    Ok(recipe)
}

pub fn thumbnail(subject: &Subject, dir: &Path, registry: &Registry) -> Result<PathBuf, String> {
    let recipe = thumb_recipe(subject)?;
    let ground = recipe.objects.len() - 1;
    let built = build_with(&recipe, &subject.base, registry).map_err(failed)?;
    let mut merged = built.recipe(&recipe);
    let own = format!("{}.", recipe.objects[subject.index].name);
    let others: Vec<String> = recipe
        .objects
        .iter()
        .enumerate()
        .filter(|&(at, _)| at != subject.index)
        .map(|(_, o)| format!("{}.", o.name))
        .collect();
    merged
        .lights
        .retain(|l| l.name.starts_with(&own) || !others.iter().any(|o| l.name.starts_with(o)));
    let parts: Vec<_> = built
        .parts
        .into_iter()
        .filter(|p| p.object == subject.index || p.object == ground)
        .collect();
    let gpu = manfred_scene::headless().map_err(failed)?;
    let rgba = manfred_scene::shoot(&gpu, &merged, &parts, THUMB_CAMERA, THUMB).map_err(failed)?;
    let out = dir.join("thumb.png");
    manfred_scene::write_png(&out, THUMB, &rgba).map_err(failed)?;
    Ok(out)
}

pub fn quoted(path: &Path) -> serde_json::Value {
    serde_json::Value::String(path.display().to_string())
}

pub fn run(args: &Args, registry: &Registry) -> ExitCode {
    let root = args
        .library
        .clone()
        .unwrap_or_else(|| root_of(&args.recipe));
    let baked = match bake(
        &args.recipe,
        &args.object,
        &args.entry,
        &args.tags,
        &root,
        registry,
    ) {
        Ok(baked) => baked,
        Err(e) => return refuse("library bake", &e),
    };
    let thumb = if args.thumb {
        match thumbnail(&baked.subject, &baked.dir, registry) {
            Ok(path) => Some(path),
            Err(e) => return refuse("library bake", &e),
        }
    } else {
        None
    };
    let index = match library::write_index(&root) {
        Ok(path) => path,
        Err(e) => return refuse("library bake", &e.to_string()),
    };
    if args.json {
        let (kind, name) = args.entry.split_once('/').unwrap_or_default();
        println!(
            "{}",
            serde_json::json!({
                "kind": kind,
                "name": name,
                "dir": quoted(&baked.dir),
                "model": quoted(&baked.model),
                "model_bytes": baked.model_bytes,
                "sidecar": quoted(&baked.sidecar),
                "thumb": thumb.as_deref().map(quoted),
                "index": quoted(&index),
            })
        );
    } else {
        println!("{}", baked.dir.display());
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = root().join("tmp").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_bake_writes_the_same_bytes_twice() {
        let sources = root().join("library/sources.toml");
        let registry = Registry::new();
        let tags = vec!["car".to_string()];
        let mut seen = Vec::new();
        for run in ["a", "b"] {
            let dir = scratch(&format!("library-bake-{run}"));
            let baked = bake(&sources, "coupe", "cars/coupe", &tags, &dir, &registry).unwrap();
            assert_eq!(baked.dir, dir.join("cars/coupe"));
            seen.push((
                std::fs::read(&baked.model).unwrap(),
                std::fs::read(&baked.sidecar).unwrap(),
            ));
        }
        assert!(!seen[0].0.is_empty());
        assert_eq!(seen[0], seen[1]);
    }

    #[test]
    fn the_sources_recipe_checks_and_builds_every_object() {
        let sources = root().join("library/sources.toml");
        let recipe = load(&sources).unwrap().recipe;
        assert_eq!(manfred_recipe::validate(&recipe), Vec::new());
        let built = build_with(&recipe, &root().join("library"), &Registry::new()).unwrap();
        for (index, object) in recipe.objects.iter().enumerate() {
            let (lo, hi) = bounds(&built, index).unwrap_or_else(|| panic!("{}", object.name));
            assert!((hi - lo).max_element() > 0.1, "{}", object.name);
        }
        assert_eq!(root_of(&sources), root().join("library"));
    }
}
