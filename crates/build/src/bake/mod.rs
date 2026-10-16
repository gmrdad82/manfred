use crate::{Built, Error, scene};
use manfred_recipe::{
    Bake, CAP_LUMINANCE, CAP_MEAN, CAPPED, Kind, PlateView, Plates, Projection, Recipe, Shadow,
    Sky, SkyAmbient, SkyKind, bake_product,
};
use pfx_materials::{Library, Material};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::{Component, Path, PathBuf};

pub(crate) mod follow;
pub(crate) mod rig;

pub(crate) const BAKERS: &[(&str, crate::Baker)] = &[("follow", follow::bake), ("rig", rig::bake)];

pub(crate) const ANIMATES: &[crate::Animates] = &[follow::animates];

pub(crate) const LIBRARY: &str = "bake-materials.toml";

pub(crate) struct Written {
    pub text: String,
    pub library: Option<String>,
}

pub(crate) fn text(
    built: &Built,
    recipe: &Recipe,
    glb: &Path,
    bake_file: &Path,
    scene: Option<&Path>,
    base: &Path,
) -> Result<Written, Error> {
    let Some(bake) = &recipe.bake else {
        return Err(Error::new(
            "output.bake",
            "a bake recipe needs its anchors, samples and volumes, expected a `[bake]` table",
        ));
    };
    let folder = folder(bake_file)?;
    let app = app(bake)?;
    let own = built.recipe_without_catalogue(recipe);
    if let Some(name) = own.materials.keys().next() {
        return Err(Error::new(
            format!("materials.{name}"),
            format!(
                "the bake reads its materials from the {app} app's catalogue and has no key for a recipe's own material"
            ),
        ));
    }
    let library = library(built, app)?;
    let mut out = String::new();
    let mut top = Section::new(&mut out, "", "bake");
    top.put("scene", &string(&inside(&folder, glb, "output.glb")?));
    top.put("app", &string(app));
    if library.is_some() {
        top.put("materials", &string(LIBRARY));
    }
    top.list("anchors", &bake.anchors)?;
    top.put("samples", &bake.samples.to_string());
    top.put("seed", &(recipe.seed & u64::from(u32::MAX)).to_string());
    if let Some(daylight) = &recipe.daylight {
        top.at = "daylight".into();
        top.num("day", f64::from(daylight.day))?;
        top.num("latitude", daylight.latitude)?;
        top.num("heading", daylight.heading)?;
    }
    let shadow_only = shadow_only(recipe)?;
    if !shadow_only.is_empty() {
        let names: Vec<String> = shadow_only.iter().map(|name| string(name)).collect();
        top.put("shadow_only_nodes", &format!("[{}]", names.join(", ")));
    }
    if let Some(sky) = &recipe.sky {
        self::sky(&mut out, sky, recipe.sun.is_some(), bake, &folder, base)?;
    }
    if let Some(sun) = &recipe.sun {
        let mut s = Section::new(&mut out, "[sun]", "sun");
        s.put("model", &string(AUTHORED));
        s.list("toward", &sun.toward)?;
        s.list("color", &sun.color)?;
        s.num("irradiance", sun.irradiance)?;
    }
    for volume in &bake.volumes {
        let mut s = Section::new(
            &mut out,
            "[[volumes]]",
            format!("bake.volume[{}]", volume.name),
        );
        s.put("name", &string(&volume.name));
        s.list("min", &volume.min)?;
        s.list("max", &volume.max)?;
        s.num("spacing", volume.spacing)?;
    }
    for light in recipe.lights.iter().filter(|light| light.parent.is_none()) {
        let at = format!("light[{}]", light.name);
        if light.radius <= 0.0 {
            return Err(Error::new(
                format!("{at}.radius"),
                "the bake draws a light as a sphere emitter, expected a radius above 0",
            ));
        }
        let mut s = Section::new(&mut out, "[[emitter]]", at);
        s.list("position", &light.position)?;
        s.num("radius", light.radius)?;
        s.list("color", &light.color)?;
        s.num("intensity", light.intensity)?;
    }
    if let Some(plates) = &bake.plates {
        let Some(scene) = scene else {
            return Err(Error::new(
                "bake.plates",
                "a plate traces the static part of the scene Manfred writes, expected `scene` in `[output]`",
            ));
        };
        self::plates(&mut out, built, recipe, plates, &folder, scene)?;
    }
    Ok(Written { text: out, library })
}

fn plates(
    out: &mut String,
    built: &Built,
    recipe: &Recipe,
    plates: &Plates,
    folder: &Path,
    scene: &Path,
) -> Result<(), Error> {
    let bake = recipe.bake.as_ref();
    let mut s = Section::new(out, "[plates]", "bake.plates");
    s.put("scene", &string(&inside(folder, scene, "output.scene")?));
    s.put("size", &format!("[{}, {}]", plates.size[0], plates.size[1]));
    if plates.overscan != Plates::OVERSCAN {
        s.num("overscan", plates.overscan)?;
    }
    if plates.scale != Plates::SCALE {
        s.num("scale", plates.scale)?;
    }
    if let Some(anchors) = &plates.anchors
        && Some(anchors) != bake.map(|bake| &bake.anchors)
    {
        s.list("anchors", anchors)?;
    }
    if !plates.all_layers() {
        let names: Vec<String> = plates.layers.iter().map(|name| string(name)).collect();
        s.put("layers", &format!("[{}]", names.join(", ")));
    }
    if plates.threshold != Plates::THRESHOLD {
        s.num("threshold", plates.threshold)?;
    }
    if plates.min_samples != Plates::MIN_SAMPLES {
        s.put("min_samples", &plates.min_samples.to_string());
    }
    if plates.max_samples != Plates::MAX_SAMPLES {
        s.put("max_samples", &plates.max_samples.to_string());
    }
    if plates.growth != Plates::GROWTH {
        s.num("growth", plates.growth)?;
    }
    if let Some(seed) = plates.seed
        && u64::from(seed) != recipe.seed & u64::from(u32::MAX)
    {
        s.put("seed", &seed.to_string());
    }
    if let Some(key) = &plates.key {
        s.put("key", &string(key));
    }
    if let Some(clamp) = plates.clamp_indirect {
        s.num("clamp_indirect", clamp)?;
    }
    if let Some(filter) = plates.filter_glossy {
        s.num("filter_glossy", filter)?;
    }
    for view in &plates.views {
        self::view(out, built, recipe, plates, view)?;
    }
    Ok(())
}

fn view(
    out: &mut String,
    built: &Built,
    recipe: &Recipe,
    plates: &Plates,
    view: &PlateView,
) -> Result<(), Error> {
    let at = format!("bake.plates.view[{}]", view.name);
    let mut keys = view.clone();
    if let Some(name) = &view.camera {
        let Some(camera) = recipe.cameras.iter().find(|c| &c.name == name) else {
            return Err(Error::new(
                format!("{at}.camera"),
                format!("no camera is named `{name}`"),
            ));
        };
        if camera.kind != Projection::Perspective {
            return Err(Error::new(
                format!("{at}.camera"),
                format!(
                    "a plate is perspective and `{name}` is orthographic, expected a perspective camera"
                ),
            ));
        }
        let aspect = f64::from(plates.size[0]) / f64::from(plates.size[1].max(1));
        let (near, far) = scene::depth_range(camera, scene::bounds(built));
        keys.at = keys.at.or(Some(camera.position));
        keys.look_at = keys.look_at.or(Some(camera.target));
        keys.up = keys.up.or(Some(camera.up));
        if keys.fov.is_none() && keys.focal.is_none() {
            keys.fov = Some(camera.fov_y_deg(aspect));
        }
        if keys.shift.is_none() && camera.shift != [0.0, 0.0] {
            keys.shift = Some(camera.shift);
        }
        keys.near = keys.near.or(Some(near));
        keys.far = keys.far.or(Some(far));
    }
    let mut s = Section::new(out, "[[plates.view]]", at);
    s.put("name", &string(&keys.name));
    for (key, value) in [("at", keys.at), ("look_at", keys.look_at), ("up", keys.up)] {
        if let Some(value) = value {
            s.list(key, &value)?;
        }
    }
    for (key, value) in [
        ("fov", keys.fov),
        ("focal", keys.focal),
        ("sensor", keys.sensor),
    ] {
        if let Some(value) = value {
            s.num(key, value)?;
        }
    }
    if let Some(shift) = keys.shift {
        s.list("shift", &shift)?;
    }
    for (key, value) in [("near", keys.near), ("far", keys.far)] {
        if let Some(value) = value {
            s.num(key, value)?;
        }
    }
    Ok(())
}

const AUTHORED: &str = "authored";

fn library(built: &Built, app: &str) -> Result<Option<String>, Error> {
    let mut library = Library::new();
    for part in built.parts.iter().filter(|part| !part.hidden) {
        for slot in &part.node.mesh.slots {
            let name = slot.material.as_str();
            if library.contains(name) {
                continue;
            }
            let Some(value) = built.catalogue.get(name) else {
                return Err(Error::new(
                    format!("materials.{name}"),
                    format!(
                        "the bake reads its materials from the {app} app's catalogue, which has no \"{name}\", expected the app to register it"
                    ),
                ));
            };
            let material = serde_json::from_value::<Material>(value.clone())
                .map_err(|e| Error::new(format!("materials.{name}"), e.to_string()))?;
            library
                .insert(name, material)
                .map_err(|e| Error::new(format!("materials.{name}"), e))?;
        }
    }
    if library.names().next().is_none() {
        return Ok(None);
    }
    library
        .to_toml()
        .map(Some)
        .map_err(|e| Error::new("materials", e))
}

fn sky(
    out: &mut String,
    sky: &Sky,
    sun: bool,
    bake: &Bake,
    folder: &Path,
    base: &Path,
) -> Result<(), Error> {
    let mut s = Section::new(out, "[sky]", "sky");
    let mut capped = false;
    match sky.kind {
        SkyKind::Mix => {
            return Err(Error::new(
                "sky.kind",
                "a mix is never baked: the bake's emitters are glTF nodes or spheres, not lamps at infinity, expected kind \"hdr\" or \"room\"",
            ));
        }
        SkyKind::Analytic => {
            if sky.intensity != 1.0 {
                return Err(Error::new(
                    "sky.intensity",
                    "the bake's analytic sky has no intensity, expected 1.0 or no key",
                ));
            }
            if sky.rotation_deg != 0.0 {
                return Err(Error::new(
                    "sky.rotation_deg",
                    "the bake's analytic sky has no rotation, expected 0.0 or no key",
                ));
            }
            s.put("kind", &string("daylight"));
        }
        SkyKind::Hdr => {
            let Some(path) = &sky.path else {
                return Err(Error::new(
                    "sky.path",
                    "kind \"hdr\" needs `path`, the .hdr file",
                ));
            };
            s.put("kind", &string("hdr"));
            s.put(
                "path",
                &string(&inside(folder, &base.join(path), "sky.path")?),
            );
            match sky.prepare.as_deref() {
                None => {}
                Some(CAPPED) => capped = true,
                Some(other) => {
                    return Err(Error::new(
                        "sky.prepare",
                        format!("the bake has no preparation \"{other}\", expected \"{CAPPED}\""),
                    ));
                }
            }
            s.num("rotation_deg", sky.rotation_deg)?;
            s.num("intensity", sky.intensity)?;
            if sun {
                s.put("ambient", &string(AUTHORED));
            }
        }
        SkyKind::Room => {
            s.put("kind", &string("room"));
            s.num("rotation_deg", sky.rotation_deg)?;
            s.num("intensity", sky.intensity)?;
            if sky.ambient == Some(SkyAmbient::Authored) {
                if !sun {
                    return Err(Error::new(
                        "sky.ambient",
                        "the bake scales a room by the authored sun's ambient only under that sun, expected a `[sun]`",
                    ));
                }
                s.put("ambient", &string(AUTHORED));
            }
        }
    }
    if sky.kind == SkyKind::Hdr && (capped || sky.balance.is_some()) {
        let mut s = Section::new(out, "[sky.prepare]", "sky.prepare");
        if capped {
            s.num("cap", CAP_LUMINANCE)?;
        }
        if let Some(balance) = &sky.balance {
            s.list("balance", balance)?;
        }
        if capped {
            s.num("mean", CAP_MEAN)?;
        }
    }
    if !sky.shade.is_empty() {
        let mut s = Section::new(out, "[sky.shade]", "sky.shade");
        for hour in &bake.anchors {
            let key = string(&number(*hour, "bake.anchors")?);
            let tint = shade_at(&sky.shade, *hour);
            s.put(&key, &list(&tint, "sky.shade.hours")?);
        }
    }
    if sky.kind == SkyKind::Room {
        let Some(room) = &sky.room else {
            return Err(Error::new(
                "sky.room",
                "kind \"room\" draws the lamp room, expected a `[sky.room]` table",
            ));
        };
        let mut s = Section::new(out, "[sky.room]", "sky.room");
        s.put("width", &room.width.to_string());
        s.list("floor", &room.floor)?;
        s.list("wall", &room.wall)?;
        s.list("ceiling", &room.ceiling)?;
        for (i, lamp) in room.lights.iter().enumerate() {
            let at = if lamp.name.is_empty() {
                format!("sky.room.lights[{i}]")
            } else {
                format!("sky.room.lights[{}]", lamp.name)
            };
            let mut s = Section::new(out, "[[sky.room.lights]]", at);
            if !lamp.name.is_empty() {
                s.put("name", &string(&lamp.name));
            }
            for (key, value) in [("az", lamp.az), ("el", lamp.el), ("power", lamp.power)] {
                s.num(key, value)?;
            }
            s.list("color", &lamp.color)?;
            for (key, value) in [
                ("width", lamp.width),
                ("height", lamp.height),
                ("soft", lamp.soft),
                ("slats", lamp.slats),
                ("open", lamp.open),
            ] {
                s.num(key, value)?;
            }
        }
    }
    Ok(())
}

pub(crate) struct Section<'a> {
    out: &'a mut String,
    pub(crate) at: String,
}

impl Section<'_> {
    pub(crate) fn new<'a>(out: &'a mut String, header: &str, at: impl Into<String>) -> Section<'a> {
        if !header.is_empty() {
            let _ = write!(out, "\n{header}\n");
        }
        Section { out, at: at.into() }
    }

    pub(crate) fn put(&mut self, key: &str, value: &str) {
        let _ = writeln!(self.out, "{key} = {value}");
    }

    pub(crate) fn num(&mut self, key: &str, value: f64) -> Result<(), Error> {
        let value = number(value, &format!("{}.{key}", self.at))?;
        self.put(key, &value);
        Ok(())
    }

    pub(crate) fn list(&mut self, key: &str, values: &[f64]) -> Result<(), Error> {
        let value = list(values, &format!("{}.{key}", self.at))?;
        self.put(key, &value);
        Ok(())
    }
}

fn app(bake: &Bake) -> Result<&str, Error> {
    bake_product(bake).map_err(|text| Error::new("bake.product", text))
}

fn shadow_only(recipe: &Recipe) -> Result<Vec<&str>, Error> {
    let mut names = Vec::new();
    for object in &recipe.objects {
        if object.hidden || matches!(object.kind, Kind::Pocket { .. }) {
            continue;
        }
        match object.shadow {
            Shadow::Cast => {}
            Shadow::Only => names.push(object.name.as_str()),
            Shadow::None => {
                return Err(Error::new(
                    format!("object[{}].shadow", object.name),
                    "the bake has no key for a part that casts no shadow, expected \"cast\" or \"only\"",
                ));
            }
        }
    }
    Ok(names)
}

pub(crate) fn shade_at(shade: &BTreeMap<u32, [f64; 3]>, hour: f64) -> [f64; 3] {
    let mut below = None;
    let mut above = None;
    for (&at, &tint) in shade {
        let at = f64::from(at);
        if at <= hour {
            below = Some((at, tint));
        }
        if at >= hour && above.is_none() {
            above = Some((at, tint));
        }
    }
    match (below, above) {
        (None, None) => [1.0; 3],
        (Some((_, tint)), None) | (None, Some((_, tint))) => tint,
        (Some((a, low)), Some((b, high))) => {
            let t = if b > a { (hour - a) / (b - a) } else { 0.0 };
            std::array::from_fn(|k| low[k] + (high[k] - low[k]) * t)
        }
    }
}

fn folder(bake_file: &Path) -> Result<PathBuf, Error> {
    let absolute = absolute(bake_file, "output.bake")?;
    match absolute.parent() {
        Some(parent) => Ok(parent.to_path_buf()),
        None => Err(Error::new("output.bake", "expected a file name")),
    }
}

fn inside(folder: &Path, file: &Path, key: &str) -> Result<String, Error> {
    let file = absolute(file, key)?;
    let Ok(rest) = file.strip_prefix(folder) else {
        return Err(Error::new(
            key,
            format!(
                "{} is outside the bake file's folder, and pfx reads nothing outside it, expected `output.bake` in a folder that holds it",
                file.display()
            ),
        ));
    };
    let parts: Vec<String> = rest
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.is_empty() {
        return Err(Error::new(
            key,
            "expected a file, found the bake file's folder",
        ));
    }
    Ok(parts.join("/"))
}

pub(crate) fn absolute(path: &Path, key: &str) -> Result<PathBuf, Error> {
    let full = std::path::absolute(path)
        .map_err(|e| Error::new(key, format!("{}: {e}", path.display())))?;
    let mut out = PathBuf::new();
    for component in full.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

pub(crate) fn number(value: f64, key: &str) -> Result<String, Error> {
    if value.is_finite() {
        Ok(format!("{value:?}"))
    } else {
        Err(Error::new(key, "expected a finite number"))
    }
}

pub(crate) fn list(values: &[f64], key: &str) -> Result<String, Error> {
    let items: Result<Vec<String>, Error> = values.iter().map(|v| number(*v, key)).collect();
    Ok(format!("[{}]", items?.join(", ")))
}

pub(crate) fn string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_recipe::{parse, validate};

    const SMALL: &str = include_str!("../../tests/sky/small.toml");
    const CATALOGUE: &str = "[materials.wood]\nbase = [0.4, 0.25, 0.12]\nroughness = 0.6\n";

    fn registry() -> crate::Registry {
        let mut registry = crate::Registry::new();
        registry.register_materials_toml(CATALOGUE).unwrap();
        registry
    }

    fn small() -> Recipe {
        let recipe = parse(SMALL).unwrap();
        assert_eq!(validate(&recipe), Vec::new());
        recipe
    }

    fn written_with(recipe: &Recipe, registry: &crate::Registry) -> Result<Written, Error> {
        let base = Path::new("/work/app");
        let built = crate::build_with(recipe, base, registry)?;
        text(
            &built,
            recipe,
            &base.join("scene/small.glb"),
            &base.join("bake.toml"),
            None,
            base,
        )
    }

    fn bake_of(recipe: &Recipe) -> Result<String, Error> {
        written_with(recipe, &registry()).map(|written| written.text)
    }

    fn edited(from: &str, to: &str) -> Recipe {
        assert!(SMALL.contains(from), "{from}");
        parse(&SMALL.replace(from, to)).unwrap()
    }

    #[test]
    fn a_small_recipe_writes_the_hand_written_bake() {
        assert_eq!(
            bake_of(&small()).unwrap(),
            include_str!("../../tests/sky/small.bake.toml")
        );
    }

    #[test]
    fn an_hdr_sky_writes_its_preparation_and_leaves_its_room_out() {
        let recipe = edited(
            "kind = \"room\"\nrotation_deg = 30.0",
            "kind = \"hdr\"\npath = \"sky/grey.hdr\"\nsource = \"test\"\nprepare.cap = 60.0\nprepare.balance = [1.0, 0.92, 0.8]\nprepare.mean = 0.3\nrotation_deg = 169.4",
        );
        let text = bake_of(&recipe).unwrap();
        assert!(
            text.contains("[sky]\nkind = \"hdr\"\npath = \"sky/grey.hdr\"\nrotation_deg = 169.4\nintensity = 1.5\nambient = \"authored\"\n\n[sky.prepare]\ncap = 60.0\nbalance = [1.0, 0.92, 0.8]\nmean = 0.3\n"),
            "{text}"
        );
        assert!(!text.contains("[sky.room]"), "{text}");
    }

    #[test]
    fn a_path_outside_the_bake_files_folder_is_refused() {
        let recipe = edited(
            "kind = \"room\"\nrotation_deg = 30.0",
            "kind = \"hdr\"\npath = \"../elsewhere/sky.hdr\"\nsource = \"test\"\nrotation_deg = 30.0",
        );
        let error = bake_of(&recipe).unwrap_err();
        assert_eq!(error.path, "sky.path");
        assert!(
            error.text.contains("outside the bake file's folder"),
            "{error}"
        );
        let base = Path::new("/work/app");
        let recipe = small();
        let built = crate::build_with(&recipe, base, &registry()).unwrap();
        let error = text(
            &built,
            &recipe,
            &base.join("scene/small.glb"),
            &base.join("bakes/bake.toml"),
            None,
            base,
        )
        .err()
        .unwrap();
        assert_eq!(error.path, "output.glb");
    }

    #[test]
    fn what_the_bake_cannot_express_is_an_error_that_names_the_key() {
        let sky = "kind = \"room\"\nrotation_deg = 30.0";
        let cases = [
            (
                sky,
                "kind = \"mix\"\npath = \"sky.hdr\"\nsource = \"test\"\nrotation_deg = 30.0",
                "sky.kind",
            ),
            (
                sky,
                "kind = \"analytic\"\nrotation_deg = 0.0",
                "sky.intensity",
            ),
            ("radius = 0.02", "radius = 0.0", "light[lamp].radius"),
            (
                "shadow = \"only\"",
                "shadow = \"none\"",
                "object[blocker].shadow",
            ),
            (
                "[bake]\n",
                "[materials.oak]\nbase_color = [0.6, 0.4, 0.2]\n[bake]\n",
                "materials.oak",
            ),
        ];
        for (from, to, key) in cases {
            let error = bake_of(&edited(from, to)).unwrap_err();
            assert_eq!(error.path, key, "{error}");
        }
        for product in ["", "App", "-app", "app_one"] {
            let named = SMALL.replace("product = \"sample\"", &format!("product = \"{product}\""));
            let error = bake_of(&parse(&named).unwrap()).unwrap_err();
            assert_eq!(error.path, "bake.product", "{product}: {error}");
        }
        let missing = SMALL.replace("product = \"sample\"\n", "");
        let error = bake_of(&parse(&missing).unwrap()).unwrap_err();
        assert_eq!(error.path, "bake.product", "{error}");
    }

    fn written(name: &str) -> PathBuf {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(name);
        let recipe = small();
        let built = crate::build_with(&recipe, &base, &registry()).unwrap();
        let files = crate::write(&built, &recipe, &base, None).unwrap();
        assert_eq!(files.len(), 4);
        assert_eq!(files[2], base.join(LIBRARY));
        assert_eq!(files[3], base.join("bake.toml"));
        base
    }

    #[test]
    fn manfred_build_writes_the_bake_beside_the_glb() {
        let base = written("sky-bake-write");
        assert_eq!(
            std::fs::read_to_string(base.join("bake.toml")).unwrap(),
            include_str!("../../tests/sky/small.bake.toml")
        );
        assert!(base.join("scene/small.glb").is_file());
    }

    #[test]
    #[ignore = "GPU and pfx on PATH: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn pfx_bakes_the_written_recipe() {
        let base = written("sky-bake-pfx");
        let run = std::process::Command::new("pfx")
            .args(["bake", "--scene"])
            .arg(&base)
            .arg("--force")
            .env("PFX_BAKE_GPU", "1")
            .output();
        let output = run.unwrap_or_else(|error| panic!("pfx must be on PATH to bake: {error}"));
        assert!(
            output.status.success(),
            "pfx bake failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let bakes = base.join("../bakes/sky-bake-pfx");
        for volume in ["desk", "lamp-side"] {
            assert!(
                bakes.join(volume).join("manifest.json").is_file(),
                "{volume}"
            );
        }
    }

    #[test]
    fn the_apps_catalogue_is_written_as_the_bakes_material_library() {
        let base = written("sky-bake-library");
        let library =
            Library::from_toml(&std::fs::read_to_string(base.join(LIBRARY)).unwrap()).unwrap();
        assert_eq!(library.names().collect::<Vec<_>>(), ["wood"]);
        let wood = library.get("wood").unwrap();
        assert_eq!(wood.base, [0.4, 0.25, 0.12]);
        assert_eq!(wood.roughness, 0.6);
        let error = written_with(&small(), &crate::Registry::new())
            .err()
            .unwrap();
        assert_eq!(error.path, "materials.wood", "{error}");
        assert!(error.text.contains("catalogue"), "{error}");
    }

    #[test]
    fn the_sky_takes_the_authored_ambient_only_under_the_authored_sun() {
        let room = edited(
            "intensity = 1.5\n",
            "intensity = 1.5\nambient = \"authored\"\n",
        );
        let text = bake_of(&room).unwrap();
        assert!(
            text.contains("[sky]\nkind = \"room\"\nrotation_deg = 30.0\nintensity = 1.5\nambient = \"authored\"\n"),
            "{text}"
        );
        assert!(
            text.contains("[sun]\nmodel = \"authored\"\ntoward = [0.45, 0.62, 0.64]\n"),
            "{text}"
        );
        let sunless = SMALL
            .replace(
                "intensity = 1.5\n",
                "intensity = 1.5\nambient = \"authored\"\n",
            )
            .replace(
                "[sun]\nmodel = \"authored\"\ntoward = [0.45, 0.62, 0.64]\ncolor = [1.0, 0.86, 0.66]\nirradiance = 4.5\n",
                "",
            );
        let error = bake_of(&parse(&sunless).unwrap()).unwrap_err();
        assert_eq!(error.path, "sky.ambient", "{error}");
        let hdr = SMALL
            .replace(
                "kind = \"room\"\nrotation_deg = 30.0",
                "kind = \"hdr\"\npath = \"sky/grey.hdr\"\nsource = \"test\"\nprepare.balance = [1.0, 0.92, 0.8]\nrotation_deg = 30.0",
            )
            .replace("[sun]\nmodel = \"authored\"\ntoward = [0.45, 0.62, 0.64]\ncolor = [1.0, 0.86, 0.66]\nirradiance = 4.5\n", "");
        let text = bake_of(&parse(&hdr).unwrap()).unwrap();
        assert!(!text.contains("ambient"), "{text}");
        assert!(!text.contains("[sun]"), "{text}");
        assert!(
            text.contains(
                "intensity = 1.5\n\n[sky.prepare]\nbalance = [1.0, 0.92, 0.8]\n\n[sky.shade]"
            ),
            "{text}"
        );
    }

    #[test]
    fn the_shade_is_written_at_each_anchor() {
        let shade = BTreeMap::from([(8, [0.5, 0.5, 0.6]), (16, [1.0, 1.0, 1.0])]);
        assert_eq!(shade_at(&shade, 12.0), [0.75, 0.75, 0.8]);
        assert_eq!(shade_at(&shade, 20.0), [1.0, 1.0, 1.0]);
    }

    #[test]
    fn strings_are_escaped_as_toml_basic_strings() {
        assert_eq!(string("a \"b\" \\ c\n"), "\"a \\\"b\\\" \\\\ c\\u000A\"");
    }
}
