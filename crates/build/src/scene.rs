use crate::bake::{Section, absolute, list, number, shade_at, string};
use crate::{Built, Error, GeneratedLight, Part, local_matrix, world};
use glam::{DMat4, DVec3};
use manfred_recipe::toml_edit::{DocumentMut, Item, Value};
use manfred_recipe::{
    CAP_LUMINANCE, CAP_MEAN, CAPPED, Camera, Casters, Kind, LightKind, MONO, Object, Projection,
    Recipe, RigFocus, Room, SCENE_SUFFIX, Shadow, SkyAmbient, SkyKind, TextSpace, Trace, View,
};
use pfx_core::daylight::{Authored, Daylight};
use pfx_load::{RoomLamp, RoomSky, Sky};
use pfx_materials::{Blend, ContentLayer, Library, Material};
use pfx_scene::types::{FORMAT, MaterialsFile, SceneFile};
use pfx_scene::{Id, Project};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};

mod effects;
mod proxies;

pub(crate) const REFERENCE_HOUR: f64 = 16.0;
const SKY_FILL: f64 = 1.6;
const AUTHORED: Authored = Authored {
    toward: [0.450_789_6, 0.621_087_9, 0.641_122_9],
    colour: [1.0, 0.86, 0.66],
    sky_fill: SKY_FILL,
};
const DAYLIGHT_RADIUS_DEG: f64 = 0.27;
const MAX_SUN_RADIUS_DEG: f64 = 10.0;
const SENSOR_MM: f64 = 36.0;
const HAZE_LO: [f64; 3] = [-1.0, -0.02, -0.7];
const HAZE_HI: [f64; 3] = [1.0, 0.95, 0.95];
const HAZE_PHASE: f64 = 0.35;
const HAZE_BACK: f64 = 0.7;
const LIGHT_FLOOR: f64 = 0.01;
const MAX_LIGHTS: usize = 4 + 1024;
const CONTENT_SLOTS: usize = 4;
const STILL: [f64; 2] = [1586.0, 992.0];
const LIFT: f64 = 0.0005;
const BOLD_FROM: u32 = 501;
const REGULAR_FONT: &str = "JetBrainsMonoNerdFont-Regular.ttf";
const BOLD_FONT: &str = "JetBrainsMonoNerdFont-Bold.ttf";
static REGULAR: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");
static BOLD: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMonoNerdFont-Bold.ttf");
const VIGNETTE: [f64; 3] = [2.4, 0.8, 1.28];
const MIX_FILL: f64 = 0.25;
const MIX_FLOOR: f64 = 1e-9;

type Beside = Vec<(PathBuf, Cow<'static, [u8]>)>;

pub(crate) struct Written {
    pub text: String,
    pub library: Option<(PathBuf, String)>,
    pub files: Beside,
}

pub(crate) fn proxies_file(scene: &Path) -> PathBuf {
    let name = scene
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = name
        .strip_suffix(SCENE_SUFFIX)
        .filter(|stem| !stem.is_empty())
        .or_else(|| name.strip_suffix(".toml"))
        .unwrap_or(&name);
    scene.with_file_name(format!("{stem}.proxies.toml"))
}

pub(crate) fn library_file(scene: &Path) -> PathBuf {
    let name = scene
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = name
        .strip_suffix(SCENE_SUFFIX)
        .filter(|stem| !stem.is_empty())
        .or_else(|| name.strip_suffix(".toml"))
        .unwrap_or(&name);
    scene.with_file_name(format!("{stem}.materials.toml"))
}

pub(crate) fn left_out(recipe: &Recipe) -> Option<String> {
    recipe.output.scene.as_ref()?;
    let mut kept = Vec::new();
    if !recipe.movers.is_empty() {
        let groups: Vec<&str> = recipe.movers.iter().map(|m| m.group.as_str()).collect();
        kept.push(format!(
            "movers ({}), which say where a part can go in the app's own axes and units with no kind, unit or motion, so the scene's preview [[mover]] has nothing to take; their clip planes are written on their objects",
            groups.join(", ")
        ));
    }
    if !recipe.variants.is_empty() {
        kept.push("variants".to_string());
    }
    if !recipe.sidecar.is_empty() {
        let keys: Vec<&str> = recipe.sidecar.keys().map(String::as_str).collect();
        kept.push(format!("[sidecar] ({})", keys.join(", ")));
    }
    if recipe.cameras.iter().any(|c| c.path.is_some()) {
        kept.push("camera paths".to_string());
    }
    let names: Vec<&str> = recipe
        .objects
        .iter()
        .filter(|o| o.hidden)
        .map(|o| o.name.as_str())
        .collect();
    if !names.is_empty() {
        kept.push(format!(
            "hidden objects ({}), which are built and not exported, so the GLB has no node for them",
            names.join(", ")
        ));
    }
    let slots: Vec<&str> = recipe
        .objects
        .iter()
        .filter(|o| matches!(&o.kind, Kind::Card { slot: Some(_), .. }))
        .map(|o| o.name.as_str())
        .collect();
    if !slots.is_empty() {
        kept.push(format!("live content slots ({})", slots.join(", ")));
    }
    (!kept.is_empty()).then(|| {
        format!(
            "output.scene: the scene has no key for {}; they stay in the recipe",
            kept.join(", ")
        )
    })
}

pub(crate) fn text(
    built: &Built,
    recipe: &Recipe,
    scene: &Path,
    glb: &Path,
    base: &Path,
) -> Result<Written, Error> {
    let format = recipe.output.scene_format;
    if format > 1 {
        return Err(Error::new(
            "output.scene_format",
            format!("Manfred writes scene format 0 or 1, not {format}"),
        ));
    }
    if format == 0
        && let Some(key) = effects::format_1_key(recipe)
    {
        return Err(Error::new(
            key,
            "this key is scene format 1's, expected `[output] scene_format = 1` or the key removed",
        ));
    }
    let paths = Paths::of(scene, format)?;
    let mut out = String::new();
    let library_path = library_file(scene);
    let (library, cards) = library(built, recipe, base)?;
    let library = match library {
        Some(text) => {
            let name = paths.relative(&library_path, "output.scene")?;
            let _ = writeln!(out, "materials = [{}]", string(&name));
            Some((library_path, text))
        }
        None => None,
    };
    if let Some(sound) = &recipe.effects.sound {
        let sound = effects::file(&paths, base, sound, "sound")?;
        let _ = writeln!(out, "sound = {}", string(&sound));
    }
    let light = Light::of(recipe);
    sun(&mut out, recipe, &light)?;
    sky(&mut out, recipe, &light, &paths, base)?;
    camera(&mut out, recipe, built)?;
    finish(&mut out, recipe, &paths, base)?;
    effects::tables(&mut out, recipe)?;
    haze(&mut out, recipe)?;
    trace(&mut out, recipe)?;
    let stands = match plates(&mut out, recipe, scene, &paths, base)? {
        Some(file) => Some((file, proxies::of(built, recipe)?)),
        None => None,
    };
    let whole = effects::whole(recipe, built, format, glb);
    lights(&mut out, recipe, built, whole.as_deref())?;
    effects::particles(&mut out, recipe, built, whole.as_deref())?;
    let glb_name = paths.relative(glb, "output.glb")?;
    match &whole {
        Some(name) => animated(&mut out, built, recipe, &glb_name, name, paths.pick())?,
        None => objects(&mut out, built, recipe, &glb_name, &cards, paths.pick())?,
    }
    contents(&mut out, &cards, &paths, base)?;
    let fonts = texts(&mut out, built, recipe, scene, &paths, base)?;
    let mut written = Written {
        text: out,
        library,
        files: fonts,
    };
    match format {
        0 => {
            if let Some((file, stands)) = stands {
                let text = proxies::text(&stands, &[], "", None)?;
                written.files.push((file, Cow::Owned(text.into_bytes())));
            }
            Ok(written)
        }
        _ => one(written, built, recipe, &paths, stands),
    }
}

fn plates(
    out: &mut String,
    recipe: &Recipe,
    scene: &Path,
    paths: &Paths,
    base: &Path,
) -> Result<Option<PathBuf>, Error> {
    let Some(plates) = recipe.bake.as_ref().and_then(|bake| bake.plates.as_ref()) else {
        return Ok(None);
    };
    let Some(dir) = &plates.dir else {
        return Ok(None);
    };
    let file = proxies_file(scene);
    let mut s = Section::new(out, "[plates]", "bake.plates");
    s.put(
        "dir",
        &string(&paths.relative(&base.join(dir), "bake.plates.dir")?),
    );
    s.put("proxies", &string(&paths.relative(&file, "output.scene")?));
    match plates.casters {
        Casters::Meshes => {}
        Casters::Proxies => s.put("casters", &string("proxies")),
        Casters::None => s.put("casters", &string("none")),
    }
    Ok(Some(file))
}

fn dynamic_key(recipe: &Recipe, index: usize) -> Option<bool> {
    let object = &recipe.objects[index];
    if let Some(dynamic) = object.dynamic {
        return Some(dynamic);
    }
    let turns = matches!(
        object.kind,
        Kind::Card {
            face_camera: true,
            ..
        }
    );
    (!turns && recipe.dynamic(index)).then_some(true)
}

struct Cards {
    slots: Vec<(String, String)>,
    shown: BTreeMap<usize, String>,
}

fn library(built: &Built, recipe: &Recipe, base: &Path) -> Result<(Option<String>, Cards), Error> {
    let merged = built.recipe(recipe);
    let mut library = Library::new();
    let mut cards = Cards {
        slots: Vec::new(),
        shown: BTreeMap::new(),
    };
    let add = |library: &mut Library, name: &str, layer: Option<usize>| {
        let Some(value) = merged.materials.get(name) else {
            return Err(Error::new(
                format!("materials.{name}"),
                format!(
                    "no catalogue entry or `[materials.{name}]` is named \"{name}\", expected the scene's library to hold it"
                ),
            ));
        };
        let material = serde_json::from_value::<Material>(value.clone())
            .map_err(|e| Error::new(format!("materials.{name}"), e.to_string()))?;
        let (name, material) = match layer {
            None => (name.to_string(), material),
            Some(slot) => (
                layered(name, slot),
                Material {
                    content_layer: ContentLayer {
                        slot: slot as i32,
                        blend: Blend::Over,
                        ..ContentLayer::default()
                    },
                    ..material
                },
            ),
        };
        if !library.contains(&name) {
            library
                .insert(&name, material)
                .map_err(|e| Error::new(format!("materials.{name}"), e))?;
        }
        Ok(())
    };
    for part in built.parts.iter().filter(|part| shown(part)) {
        let object = &recipe.objects[part.object];
        let layer = card(&mut cards, object, part.object, base)?;
        for slot in &part.node.mesh.slots {
            add(&mut library, &slot.material, layer)?;
        }
    }
    if library.names().next().is_none() {
        return Ok((None, cards));
    }
    let text = library.to_toml().map_err(|e| Error::new("materials", e))?;
    Ok((Some(text), cards))
}

fn layered(material: &str, slot: usize) -> String {
    format!("{material}+content{slot}")
}

fn card(
    cards: &mut Cards,
    object: &Object,
    index: usize,
    base: &Path,
) -> Result<Option<usize>, Error> {
    let Kind::Card {
        image, face_camera, ..
    } = &object.kind
    else {
        return Ok(None);
    };
    let at = format!("object[{}]", object.name);
    if *face_camera && !object.keep_transform {
        return Err(Error::new(
            format!("{at}.keep_transform"),
            "the scene turns a card about its object's origin, and Manfred bakes a card's placement into its mesh, so it would turn about the scene's origin, expected keep_transform = true on a card that faces the camera",
        ));
    }
    let Some(image) = image else {
        return Ok(None);
    };
    let file = base.join(image);
    let bytes = std::fs::read(&file)
        .map_err(|e| Error::new(format!("{at}.image"), format!("{}: {e}", file.display())))?;
    png_header(&bytes).map_err(|e| {
        Error::new(
            format!("{at}.image"),
            format!("{}: the scene's content is a PNG: {e}", file.display()),
        )
    })?;
    let slot = match cards.slots.iter().position(|(_, path)| path == image) {
        Some(slot) => slot,
        None => {
            if cards.slots.len() == CONTENT_SLOTS {
                return Err(Error::new(
                    format!("{at}.image"),
                    format!(
                        "the scene shows content through {CONTENT_SLOTS} slots and this is a fifth image, expected at most {CONTENT_SLOTS} images"
                    ),
                ));
            }
            cards.slots.push((object.name.clone(), image.clone()));
            cards.slots.len() - 1
        }
    };
    cards.shown.insert(index, cards.slots[slot].0.clone());
    Ok(Some(slot))
}

fn shown(part: &Part) -> bool {
    !part.hidden && !part.node.mesh.indices.is_empty()
}

#[derive(Clone, Copy)]
enum Source {
    Authored,
    Daylight(Daylight),
}

struct Light {
    source: Source,
    hour: f64,
    ambient: f64,
}

impl Light {
    fn of(recipe: &Recipe) -> Light {
        match (&recipe.sun, &recipe.daylight) {
            (Some(_), _) => Light {
                source: Source::Authored,
                hour: REFERENCE_HOUR,
                ambient: 1.0,
            },
            (None, daylight) => {
                let daylight = match daylight {
                    Some(d) => Daylight {
                        hour: d.hour,
                        day: f64::from(d.day),
                        latitude: d.latitude,
                        heading: d.heading,
                    },
                    None => Daylight {
                        hour: REFERENCE_HOUR,
                        day: Daylight::DAY,
                        latitude: Daylight::LATITUDE,
                        heading: Daylight::HEADING,
                    },
                };
                Light {
                    source: Source::Daylight(daylight),
                    hour: daylight.hour,
                    ambient: daylight.light(REFERENCE_HOUR).ambient,
                }
            }
        }
    }
}

fn sun(out: &mut String, recipe: &Recipe, light: &Light) -> Result<(), Error> {
    let mut s = Section::new(out, "[sun]", "sun");
    match (&recipe.sun, light.source) {
        (Some(sun), _) => {
            s.put("model", &string("authored"));
            s.list("toward", &sun.toward)?;
            s.list("color", &sun.color)?;
            s.num("irradiance", sun.irradiance)?;
            s.num("hour", REFERENCE_HOUR)?;
            s.num("reference_hour", REFERENCE_HOUR)?;
            if !(0.0..=MAX_SUN_RADIUS_DEG).contains(&sun.radius_deg) {
                return Err(Error::new(
                    "sun.radius_deg",
                    format!(
                        "the scene's sun radius is 0 to {MAX_SUN_RADIUS_DEG} degrees, expected a radius in that range"
                    ),
                ));
            }
            s.num("radius", sun.radius_deg)?;
        }
        (None, Source::Daylight(daylight)) => {
            s.at = "daylight".into();
            s.put("model", &string("daylight"));
            s.num("hour", daylight.hour)?;
            s.num("day", daylight.day)?;
            s.num("latitude", daylight.latitude)?;
            s.num("heading", daylight.heading)?;
            s.num("reference_hour", REFERENCE_HOUR)?;
            s.num("radius", DAYLIGHT_RADIUS_DEG)?;
        }
        (None, Source::Authored) => {}
    }
    Ok(())
}

fn sky(
    out: &mut String,
    recipe: &Recipe,
    light: &Light,
    paths: &Paths,
    base: &Path,
) -> Result<(), Error> {
    let Some(sky) = &recipe.sky else {
        let mut s = Section::new(out, "[sky]", "sky");
        s.put("kind", &string("analytic"));
        return analytic_ambient(&mut s, light, 1.0);
    };
    if sky.kind == SkyKind::Mix {
        return mix(out, sky, light, paths, base);
    }
    let mut s = Section::new(out, "[sky]", "sky");
    let scale = match sky.kind {
        SkyKind::Mix => {
            return Err(Error::new(
                "sky.kind",
                "a mix is written as its layers, expected kind \"hdr\", \"room\" or \"analytic\" here",
            ));
        }
        SkyKind::Analytic => {
            s.put("kind", &string("analytic"));
            return analytic_ambient(&mut s, light, sky.intensity);
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
                &string(&paths.relative(&base.join(path), "sky.path")?),
            );
            light.ambient
        }
        SkyKind::Room => {
            s.put("kind", &string("room"));
            match (sky.ambient, light.source) {
                (None, _) => 1.0,
                (Some(SkyAmbient::Authored), Source::Authored) => light.ambient,
                (Some(SkyAmbient::Authored), Source::Daylight(daylight)) => {
                    Daylight {
                        hour: light.hour,
                        ..daylight
                    }
                    .authored(&AUTHORED, REFERENCE_HOUR)
                    .ambient
                }
            }
        }
    };
    let tint = grey_tint(sky, light)?;
    s.num("rotation_deg", sky.rotation_deg)?;
    s.num("intensity", sky.intensity * tint * scale)?;
    if sky.kind == SkyKind::Hdr {
        prepare(out, "[sky.prepare]", "sky.prepare", sky)?;
    }
    if sky.kind == SkyKind::Room {
        let Some(room) = &sky.room else {
            return Err(Error::new(
                "sky.room",
                "kind \"room\" draws the lamp room, expected a `[sky.room]` table",
            ));
        };
        room_tables(out, "sky.room", room, room.width, false)?;
    }
    Ok(())
}

fn grey_tint(sky: &manfred_recipe::Sky, light: &Light) -> Result<f64, Error> {
    let tint = shade_at(&sky.shade, light.hour);
    if tint[0] != tint[1] || tint[1] != tint[2] {
        return Err(Error::new(
            "sky.shade",
            format!(
                "the shade at hour {} is {:?}, a colour, and the scene's sky has one intensity, expected a grey shade at that hour",
                number(light.hour, "sky.shade")?,
                tint
            ),
        ));
    }
    Ok(tint[0])
}

fn capped(sky: &manfred_recipe::Sky) -> Result<bool, Error> {
    match sky.prepare.as_deref() {
        None => Ok(false),
        Some(CAPPED) => Ok(true),
        Some(other) => Err(Error::new(
            "sky.prepare",
            format!("there is no preparation named \"{other}\", expected \"{CAPPED}\""),
        )),
    }
}

fn prepare(
    out: &mut String,
    header: &str,
    at: &str,
    sky: &manfred_recipe::Sky,
) -> Result<(), Error> {
    let capped = capped(sky)?;
    if capped || sky.balance.is_some() {
        let mut s = Section::new(out, header, at);
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
    Ok(())
}

fn room_tables(
    out: &mut String,
    table: &str,
    room: &Room,
    width: u32,
    bare: bool,
) -> Result<(), Error> {
    let mut s = Section::new(out, &format!("[{table}]"), table);
    s.put("width", &width.to_string());
    if bare {
        for key in ["floor", "wall", "ceiling"] {
            s.list(key, &[0.0; 3])?;
        }
    } else {
        s.list("floor", &room.floor)?;
        s.list("wall", &room.wall)?;
        s.list("ceiling", &room.ceiling)?;
    }
    for (i, lamp) in room.lights.iter().enumerate() {
        let at = if lamp.name.is_empty() {
            format!("{table}.lights[{i}]")
        } else {
            format!("{table}.lights[{}]", lamp.name)
        };
        let mut s = Section::new(out, &format!("[[{table}.lights]]"), at);
        s.put("name", &string(&lamp.name));
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
    Ok(())
}

fn mix(
    out: &mut String,
    sky: &manfred_recipe::Sky,
    light: &Light,
    paths: &Paths,
    base: &Path,
) -> Result<(), Error> {
    let Some(path) = &sky.path else {
        return Err(Error::new(
            "sky.path",
            "kind \"mix\" needs `path`, the .hdr file",
        ));
    };
    let Some(room) = &sky.room else {
        return Err(Error::new(
            "sky.room",
            "kind \"mix\" draws the room's lamps, expected a `[sky.room]` table",
        ));
    };
    let capped = capped(sky)?;
    let tint = grey_tint(sky, light)?;
    let file = base.join(path);
    let bytes = std::fs::read(&file)
        .map_err(|e| Error::new("sky.path", format!("{}: {e}", file.display())))?;
    let mut daylight = Sky::parse(&bytes)
        .map_err(|e| Error::new("sky.path", format!("{}: {e}", file.display())))?;
    let balance = sky.balance.map(|b| b.map(|v| v as f32));
    if capped {
        daylight = daylight.capped_luminance(CAP_LUMINANCE as f32);
    }
    if let Some(target) = balance {
        daylight = daylight.balanced_to(target);
    }
    if capped {
        daylight = daylight.scaled_to_mean_luminance(CAP_MEAN as f32);
    }
    let lamps = Sky::room(&RoomSky {
        width: daylight.width,
        floor: [0.0; 3],
        wall: [0.0; 3],
        ceiling: [0.0; 3],
        lights: room
            .lights
            .iter()
            .map(|lamp| RoomLamp {
                name: lamp.name.clone(),
                az: lamp.az as f32,
                el: lamp.el as f32,
                power: lamp.power as f32,
                color: lamp.color.map(|v| v as f32),
                width: lamp.width as f32,
                height: lamp.height as f32,
                soft: lamp.soft as f32,
                slats: lamp.slats as f32,
                open: lamp.open as f32,
            })
            .collect(),
    });
    let have = f64::from(lamps.mean_luminance());
    let fill = sky.fill.unwrap_or(MIX_FILL);
    let gain = if have <= MIX_FLOOR {
        0.0
    } else {
        f64::from(fill as f32 * daylight.mean_luminance() / lamps.mean_luminance())
    };
    let level = sky.intensity * tint * light.ambient;
    Section::new(out, "[sky]", "sky").put("kind", &string("mix"));
    let mut s = Section::new(out, "[[sky.layer]]", "sky");
    s.put("kind", &string("hdr"));
    s.put("path", &string(&paths.relative(&file, "sky.path")?));
    s.num("rotation_deg", sky.rotation_deg)?;
    s.num("intensity", level)?;
    prepare(out, "[sky.layer.prepare]", "sky.prepare", sky)?;
    let mut s = Section::new(out, "[[sky.layer]]", "sky.room");
    s.put("kind", &string("room"));
    s.num("weight", gain)?;
    s.num("rotation_deg", sky.rotation_deg)?;
    s.num("intensity", level)?;
    room_tables(out, "sky.layer.room", room, daylight.width, true)?;
    Ok(())
}

fn analytic_ambient(s: &mut Section<'_>, light: &Light, intensity: f64) -> Result<(), Error> {
    match light.source {
        Source::Authored if intensity != 1.0 => s.num("ambient", intensity),
        Source::Daylight(_) if intensity != 1.0 => {
            let ambient = light.ambient as f32 * intensity as f32;
            s.num("ambient", f64::from(ambient))
        }
        _ => Ok(()),
    }
}

fn chosen(recipe: &Recipe) -> Option<&Camera> {
    match &recipe.output.scene_camera {
        Some(name) => recipe.cameras.iter().find(|c| c.name == *name),
        None => recipe.cameras.first(),
    }
}

fn camera(out: &mut String, recipe: &Recipe, built: &Built) -> Result<(), Error> {
    let Some(camera) = chosen(recipe) else {
        if let Some(name) = &recipe.output.scene_camera {
            return Err(Error::new(
                "output.scene_camera",
                format!("no camera is named `{name}`"),
            ));
        }
        return Ok(());
    };
    let at = format!("camera[{}]", camera.name);
    let mut s = Section::new(out, "[camera]", at);
    match camera.kind {
        Projection::Perspective => {
            s.put("projection", &string("perspective"));
        }
        Projection::Orthographic => {
            s.put("projection", &string("orthographic"));
        }
    }
    s.list("at", &camera.position)?;
    s.list("look_at", &camera.target)?;
    s.list("up", &camera.up)?;
    match camera.kind {
        Projection::Perspective => {
            let aspect = match &recipe.plate {
                Some(plate) => plate.width / plate.height,
                None => STILL[0] / STILL[1],
            };
            let fov = camera.fov_y_deg(aspect);
            if camera.fstop.is_finite() && camera.fstop > 0.0 {
                let tan_y = (fov.to_radians() * 0.5).tan();
                let focal = camera
                    .focal_mm
                    .filter(|focal| *focal > 0.0)
                    .unwrap_or_else(|| {
                        camera.sensor_mm.unwrap_or(SENSOR_MM) / (2.0 * tan_y * aspect.max(1.0))
                    });
                s.num("focal", focal)?;
                s.num("sensor", 2.0 * focal * tan_y)?;
                s.num("fstop", camera.fstop)?;
                let fixed = match camera.rig.as_ref().and_then(|rig| rig.focus) {
                    Some(RigFocus::Distance(metres)) => Some(metres),
                    _ => camera.focus_m,
                };
                if let Some(keys) = focus_keys(recipe, built, camera) {
                    s.put("focus_keys", &keys?);
                }
                if let Some(focus) = fixed {
                    if focus <= focal / 1000.0 {
                        return Err(Error::new(
                            format!("{}.focus_m", s.at),
                            "the scene's focus is beyond the focal length, expected a focus_m above focal_mm / 1000",
                        ));
                    }
                    s.num("focus", focus)?;
                }
            } else {
                s.num("fov", fov)?;
            }
        }
        Projection::Orthographic => {
            s.num("height", camera.ortho_height.unwrap_or(1.0))?;
        }
    }
    let (near, far) = depth_range(camera, bounds(built));
    s.num("near", near)?;
    s.num("far", far)?;
    if camera.shift != [0.0, 0.0] {
        s.list("shift", &camera.shift)?;
    }
    Ok(())
}

fn focus_keys(recipe: &Recipe, built: &Built, camera: &Camera) -> Option<Result<String, Error>> {
    let node = format!("camera.{}", camera.name);
    let focus = built
        .tracks
        .iter()
        .find(|t| t.node == node)?
        .focus
        .as_ref()?;
    let fps = f64::from(recipe.timeline.map_or(1, |t| t.fps).max(1));
    let at = format!("camera[{}].rig.focus", camera.name);
    let keys: Result<Vec<String>, Error> = focus
        .iter()
        .enumerate()
        .map(|(n, metres)| list(&[n as f64 / fps, *metres], &at))
        .collect();
    Some(keys.map(|keys| format!("[{}]", keys.join(", "))))
}

fn haze(out: &mut String, recipe: &Recipe) -> Result<(), Error> {
    let room = recipe
        .look
        .as_ref()
        .filter(|look| look.view != View::Standard && look.grade.haze > 0.0)
        .map(|look| look.grade.haze);
    let open = recipe.effects.haze.as_ref();
    let has = |key: &str| open.is_some_and(|keys| keys.iter().any(|(k, _)| *k == key));
    if room.is_some() && has("amount") {
        return Err(Error::new(
            "haze.amount",
            "look.grade.haze already sets the haze's amount, expected one of the two",
        ));
    }
    if room.is_none() && open.is_some() && !has("height") && !has("falloff") {
        return Err(Error::new(
            "haze",
            "an outdoor haze needs `height` or `falloff`; a room haze comes from look.grade.haze",
        ));
    }
    if room.is_none() && open.is_none() {
        return Ok(());
    }
    let mut s = Section::new(out, "[haze]", "haze");
    if let Some(amount) = room {
        s.at = "look.grade.haze".to_string();
        s.list("lo", &HAZE_LO)?;
        s.list("hi", &HAZE_HI)?;
        s.num("amount", amount.min(1.0))?;
        s.num("phase", HAZE_PHASE)?;
        s.num("back", HAZE_BACK)?;
        s.at = "haze".to_string();
    }
    match open {
        Some(settings) => effects::put(&mut s, settings),
        None => Ok(()),
    }
}

pub(crate) fn bounds(built: &Built) -> Option<[[f64; 3]; 2]> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for part in built.parts.iter().filter(|part| !part.hidden) {
        let node = &part.node;
        for p in &node.mesh.positions {
            let p = DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
            let p = if node.keep_transform {
                node.transform.transform_point3(p)
            } else {
                p
            };
            for (k, v) in p.to_array().into_iter().enumerate() {
                lo[k] = lo[k].min(v);
                hi[k] = hi[k].max(v);
            }
        }
    }
    lo[0].is_finite().then_some([lo, hi])
}

pub(crate) fn depth_range(camera: &Camera, bounds: Option<[[f64; 3]; 2]>) -> (f64, f64) {
    let Some(bounds) = bounds else {
        return (0.01, 100.0);
    };
    let eye = DVec3::from(camera.position);
    let forward = (DVec3::from(camera.target) - eye)
        .try_normalize()
        .unwrap_or(DVec3::NEG_Z);
    let mut nearest = f64::INFINITY;
    let mut farthest = f64::NEG_INFINITY;
    for x in [bounds[0][0], bounds[1][0]] {
        for y in [bounds[0][1], bounds[1][1]] {
            for z in [bounds[0][2], bounds[1][2]] {
                let along = (DVec3::new(x, y, z) - eye).dot(forward);
                nearest = nearest.min(along);
                farthest = farthest.max(along);
            }
        }
    }
    let span = (farthest - nearest).max(1e-3);
    let far = farthest + span * 0.05 + 1e-3;
    let near = (nearest - span * 0.05).max(far * 1e-4);
    (near, far.max(near * 2.0))
}

fn finish(out: &mut String, recipe: &Recipe, paths: &Paths, base: &Path) -> Result<(), Error> {
    let lut = match recipe.look.as_ref().and_then(|look| look.lut.as_ref()) {
        Some(lut) => Some(effects::file(paths, base, lut, "look.lut")?),
        None => None,
    };
    let amount = recipe.look.as_ref().and_then(|look| look.lut_amount);
    standard_or_graded(out, recipe)?;
    if let Some(lut) = lut {
        let mut s = Section::new(out, "[[finish.pass]]", "look.lut");
        s.put("lut", &string(&lut));
        if let Some(amount) = amount {
            s.num("lut_amount", amount)?;
        }
    }
    Ok(())
}

fn standard_or_graded(out: &mut String, recipe: &Recipe) -> Result<(), Error> {
    let (exposure, grade) = match &recipe.look {
        None => (1.0, None),
        Some(look) => (look.exposure, Some(look)),
    };
    let mut s = Section::new(out, "[finish]", "look");
    if let Some(look) = grade
        && look.view == View::Standard
    {
        s.num("exposure", exposure)?;
        return Ok(());
    }
    let (bloom, vignette, saturation, warmth) = match grade {
        Some(look) => {
            let g = &look.grade;
            (g.bloom, g.vignette, g.saturation, g.warmth)
        }
        None => (0.0, 0.0, 1.0, 0.0),
    };
    let passes = [
        ("exposure", number(exposure, "look.exposure")?),
        (
            "bloom",
            format!(
                "{{ kind = \"box\", strength = {}, threshold = 0.9, knee = 0.7, knee_width = 0.6, soft = 0.15, radius = 12, sigma = 50.0, down = 2 }}",
                number(bloom, "look.grade.bloom")?
            ),
        ),
        (
            "vignette",
            format!(
                "{{ kind = \"power\", strength = {}, power = {}, aspect = {}, scale = {} }}",
                number(vignette, "look.grade.vignette")?,
                number(VIGNETTE[0], "look")?,
                number(VIGNETTE[1], "look")?,
                number(VIGNETTE[2], "look")?
            ),
        ),
        ("saturation", number(saturation, "look.grade.saturation")?),
        (
            "warmth",
            format!(
                "{{ kind = \"linear\", amount = {} }}",
                number(warmth, "look.grade.warmth")?
            ),
        ),
        ("tone", string("aces")),
    ];
    for (key, value) in passes {
        let mut s = Section::new(out, "[[finish.pass]]", "look");
        s.put(key, &value);
    }
    Ok(())
}

fn trace(out: &mut String, recipe: &Recipe) -> Result<(), Error> {
    let trace = recipe.trace;
    if !trace.transmissive_shadows && trace.clamp_indirect == 0.0 && trace.filter_glossy == 0.0 {
        return Ok(());
    }
    let mut s = Section::new(out, "[trace]", "trace");
    if trace.transmissive_shadows {
        s.put("transmissive_shadows", "true");
    }
    for (key, value, max) in [
        (
            "clamp_indirect",
            trace.clamp_indirect,
            Trace::CLAMP_INDIRECT_MAX,
        ),
        (
            "filter_glossy",
            trace.filter_glossy,
            Trace::FILTER_GLOSSY_MAX,
        ),
    ] {
        if !(0.0..=max).contains(&value) {
            return Err(Error::new(
                format!("trace.{key}"),
                format!(
                    "{key} = {value} is outside 0 to {max}, expected a value in that range, or 0 for off"
                ),
            ));
        }
        if value > 0.0 {
            s.num(key, value)?;
        }
    }
    Ok(())
}

fn lights(
    out: &mut String,
    recipe: &Recipe,
    built: &Built,
    whole: Option<&str>,
) -> Result<(), Error> {
    if recipe.lights.len() > MAX_LIGHTS {
        return Err(Error::new(
            "light",
            format!(
                "the engine draws {MAX_LIGHTS} local lights at most, the recipe has {}",
                recipe.lights.len()
            ),
        ));
    }
    let room = MAX_LIGHTS.saturating_sub(recipe.lights.len());
    let generated: Vec<manfred_recipe::Light> =
        ordered(&built.lights).take(room).map(generated).collect();
    for light in recipe.lights.iter().chain(&generated) {
        let at = format!("light[{}]", light.name);
        let parent = match &light.parent {
            Some((name, place)) => Some((
                effects::parent(built, recipe, whole, name, &format!("{at}.parent"))?,
                place,
            )),
            None => None,
        };
        let mut s = Section::new(out, "[[light]]", at);
        s.put("name", &string(&light.name));
        if light.kind == LightKind::Spot {
            s.put("kind", &string("spot"));
        }
        match parent {
            Some((parent, place)) => {
                parent.put(&mut s);
                s.list("at", place)?;
            }
            None => s.list("position", &light.position)?,
        }
        if let Some(direction) = &light.direction {
            s.list("direction", direction)?;
        }
        if let Some(cone) = &light.cone {
            s.list("cone", cone)?;
        }
        s.list("color", &light.color)?;
        s.num("intensity", light.intensity)?;
        s.num("radius", light.radius)?;
        let range = if light.range > 0.0 {
            light.range
        } else {
            (light.intensity.max(0.0) / LIGHT_FLOOR).sqrt()
        };
        s.num("range", range)?;
        s.put("shadow", &light.shadow.to_string());
    }
    Ok(())
}

fn generated(light: &GeneratedLight) -> manfred_recipe::Light {
    manfred_recipe::Light {
        name: light.name.clone(),
        kind: LightKind::Spot,
        position: light.position,
        parent: light
            .parent
            .as_ref()
            .map(|node| (node.clone(), light.position)),
        direction: Some(light.direction),
        cone: Some(light.cone),
        color: light.color,
        intensity: light.intensity,
        radius: 0.0,
        range: light.range,
        shadow: light.shadow,
    }
}

fn ordered(lights: &[GeneratedLight]) -> impl Iterator<Item = &GeneratedLight> {
    let riding = lights.iter().filter(|light| light.parent.is_some());
    riding.chain(lights.iter().filter(|light| light.parent.is_none()))
}

pub(crate) fn lights_left_out(recipe: &Recipe, generated: &[GeneratedLight]) -> Option<String> {
    recipe.output.scene.as_ref()?;
    let room = MAX_LIGHTS.saturating_sub(recipe.lights.len());
    let left: Vec<&str> = ordered(generated)
        .skip(room)
        .map(|light| light.name.as_str())
        .collect();
    (!left.is_empty()).then(|| {
        format!(
            "output.scene: the engine draws {MAX_LIGHTS} local lights at most, so {} generated lights are left out ({}{})",
            left.len(),
            left[..left.len().min(4)].join(", "),
            if left.len() > 4 { ", …" } else { "" }
        )
    })
}

fn objects(
    out: &mut String,
    built: &Built,
    recipe: &Recipe,
    glb: &str,
    cards: &Cards,
    pick: &str,
) -> Result<(), Error> {
    let below = descendants(built);
    for (at, part) in built.parts.iter().enumerate() {
        if !shown(part) {
            continue;
        }
        let object = &recipe.objects[part.object];
        let name = &part.node.name;
        let ip = format!("object[{}]", object.name);
        let mesh = format!("mesh.{}", key(name));
        let mut s = Section::new(out, &format!("[{mesh}]"), &ip);
        s.put("file", &string(glb));
        s.put("node", &string(name));
        for child in below.get(&at).into_iter().flatten() {
            let mut s = Section::new(out, &format!("[{mesh}.nodes.{}]", key(child)), &ip);
            s.put("hidden", "true");
        }
        let mut s = Section::new(out, "[[object]]", &ip);
        s.put("name", &string(name));
        s.put("mesh", &string(name));
        s.put(pick, &(part.object + 1).to_string());
        if part.node.keep_transform {
            placement(&mut s, recipe, part.object, part.node.transform)?;
        }
        let names: BTreeSet<&str> = part
            .node
            .mesh
            .slots
            .iter()
            .map(|slot| slot.material.as_str())
            .collect();
        let content = cards.shown.get(&part.object);
        if let (1, Some(one)) = (names.len(), names.first()) {
            let material = match cards.slots.iter().position(|(n, _)| Some(n) == content) {
                Some(slot) => layered(one, slot),
                None => (*one).to_string(),
            };
            s.put("material", &string(&material));
        }
        match part.shadow {
            Shadow::Cast => {}
            Shadow::Only => s.put("shadow", &string("only")),
            Shadow::None => s.put("shadow", &string("none")),
        }
        if part.two_sided {
            s.put("two_sided", "true");
        }
        let planes = clips(recipe, &object.name);
        if !planes.is_empty() {
            let items: Result<Vec<String>, Error> = planes
                .iter()
                .map(|p| list(p, &format!("{ip}.clip")))
                .collect();
            s.put("clip", &format!("[{}]", items?.join(", ")));
        }
        if let Some(content) = content {
            s.put("content", &string(content));
        }
        if let Kind::Card {
            image,
            face_camera,
            alpha_cutoff,
            ..
        } = &object.kind
        {
            if image.is_some() {
                s.num("alpha_cutoff", alpha_cutoff.clamp(0.0, 1.0))?;
            }
            if *face_camera {
                s.put("face_camera", "true");
            }
        }
        if let Some(dynamic) = dynamic_key(recipe, part.object) {
            s.put("dynamic", &dynamic.to_string());
        }
    }
    Ok(())
}

fn animated(
    out: &mut String,
    built: &Built,
    recipe: &Recipe,
    glb: &str,
    name: &str,
    pick: &str,
) -> Result<(), Error> {
    let ip = format!("object[{name}]");
    let mesh = format!("mesh.{}", key(name));
    let mut s = Section::new(out, &format!("[{mesh}]"), &ip);
    s.put("file", &string(glb));
    for part in built.parts.iter().filter(|part| !part.hidden) {
        let names: BTreeSet<&str> = part
            .node
            .mesh
            .slots
            .iter()
            .map(|slot| slot.material.as_str())
            .collect();
        let material = match (names.len(), names.first()) {
            (1, Some(one)) => Some(*one),
            _ => None,
        };
        let shadow = match part.shadow {
            Shadow::Cast => None,
            Shadow::Only => Some("only"),
            Shadow::None => Some("none"),
        };
        if material.is_none() && shadow.is_none() && !part.two_sided {
            continue;
        }
        let at = format!("object[{}]", recipe.objects[part.object].name);
        let mut s = Section::new(out, &format!("[{mesh}.nodes.{}]", key(&part.node.name)), at);
        if let Some(material) = material {
            s.put("material", &string(material));
        }
        if let Some(shadow) = shadow {
            s.put("shadow", &string(shadow));
        }
        if part.two_sided {
            s.put("two_sided", "true");
        }
    }
    let mut s = Section::new(out, "[[object]]", &ip);
    s.put("name", &string(name));
    s.put("mesh", &string(name));
    s.put(pick, "1");
    let mut s = Section::new(out, "[object.animation]", &ip);
    s.put("clip", &string(manfred_glb::ANIMATION));
    Ok(())
}

fn descendants(built: &Built) -> BTreeMap<usize, Vec<String>> {
    let mut out: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (at, part) in built.parts.iter().enumerate() {
        if part.hidden {
            continue;
        }
        let mut next = built.nest.get(at).copied().flatten();
        for _ in 0..built.parts.len() {
            let Some((parent, _)) = next else {
                break;
            };
            out.entry(parent).or_default().push(part.node.name.clone());
            next = built.nest.get(parent).copied().flatten();
        }
    }
    out
}

fn placement(
    s: &mut Section<'_>,
    recipe: &Recipe,
    index: usize,
    matrix: DMat4,
) -> Result<(), Error> {
    let object = &recipe.objects[index];
    let ip = format!("object[{}]", object.name);
    let (at, rotate, scale) = if object.parent.is_none() {
        (object.at, object.rotate, object.scale)
    } else {
        decompose(matrix).ok_or_else(|| {
            Error::new(
                format!("{ip}.parent"),
                "the parents' transforms compose to a shear, which the scene's at, rotate and scale cannot hold",
            )
        })?
    };
    s.list("at", &at)?;
    s.list("rotate", &rotate)?;
    s.list("scale", &scale)
}

fn decompose(matrix: DMat4) -> Option<([f64; 3], [f64; 3], [f64; 3])> {
    let columns = [matrix.x_axis, matrix.y_axis, matrix.z_axis].map(|c| c.truncate());
    let mut scale = columns.map(|c| c.length());
    if scale.iter().any(|s| *s <= 1e-12 || !s.is_finite()) {
        return None;
    }
    if matrix.determinant() < 0.0 {
        scale[0] = -scale[0];
    }
    let [c0, c1, c2] = [0, 1, 2].map(|k| columns[k] / scale[k]);
    let sy = (-c0.z).clamp(-1.0, 1.0);
    let y = sy.asin();
    let (x, z) = if y.cos() > 1e-9 {
        (c1.z.atan2(c2.z), c0.y.atan2(c0.x))
    } else {
        (0.0, (-c1.x).atan2(c1.y))
    };
    let at = matrix.w_axis.truncate().to_array();
    let rotate = [x, y, z].map(f64::to_degrees);
    let rebuilt = DMat4::from_translation(DVec3::from(at))
        * crate::rotation(rotate)
        * DMat4::from_scale(DVec3::from(scale));
    let wanted = matrix.to_cols_array();
    let tolerance = 1e-9 * (1.0 + wanted.iter().fold(0.0_f64, |m, v| m.max(v.abs())));
    let close = rebuilt
        .to_cols_array()
        .iter()
        .zip(wanted)
        .all(|(a, b)| (a - b).abs() <= tolerance);
    close.then_some((at, rotate, scale))
}

fn clips(recipe: &Recipe, name: &str) -> Vec<[f64; 4]> {
    recipe
        .movers
        .iter()
        .find(|mover| mover.owns(name))
        .map(|mover| mover.clip.into_iter().chain(mover.clip2).collect())
        .unwrap_or_default()
}

fn contents(out: &mut String, cards: &Cards, paths: &Paths, base: &Path) -> Result<(), Error> {
    for (name, image) in &cards.slots {
        let at = format!("object[{name}].image");
        let mut s = Section::new(out, &format!("[content.{}]", key(name)), &at);
        s.put("image", &string(&paths.relative(&base.join(image), &at)?));
    }
    Ok(())
}

fn texts(
    out: &mut String,
    built: &Built,
    recipe: &Recipe,
    scene: &Path,
    paths: &Paths,
    base: &Path,
) -> Result<Beside, Error> {
    let mut fonts: Beside = Vec::new();
    for (index, object) in recipe.objects.iter().enumerate() {
        let Kind::Text {
            spans, space, on, ..
        } = &object.kind
        else {
            continue;
        };
        if object.hidden {
            continue;
        }
        let ip = format!("object[{}]", object.name);
        if *space == TextSpace::Overlay {
            return Err(Error::new(
                format!("{ip}.space"),
                "overlay text is in pixels of the still, and the scene's text is in scene units, expected \"surface\" or \"lit\"",
            ));
        }
        let [span] = spans.as_slice() else {
            return Err(Error::new(
                format!("{ip}.spans"),
                format!(
                    "the scene's text has one font, size and colour, and this text has {} spans, expected one",
                    spans.len()
                ),
            ));
        };
        let frame = frame(built, recipe, index, on.as_deref())?;
        let (at, rotate, scale) = decompose(frame).ok_or_else(|| {
            Error::new(
                format!("{ip}.scale"),
                "the text's placement is sheared, which the scene's at and rotate cannot hold",
            )
        })?;
        let uniform = (scale[0] - scale[1]).abs() <= 1e-9 * scale[0].abs()
            && (scale[1] - scale[2]).abs() <= 1e-9 * scale[0].abs()
            && scale[0] > 0.0;
        if !uniform {
            return Err(Error::new(
                format!("{ip}.scale"),
                "the scene's text has a size and no scale, expected a uniform positive scale",
            ));
        }
        let font = match span.family.as_deref() {
            None | Some(MONO) => {
                let (name, bytes) = if span.weight.unwrap_or(400) >= BOLD_FROM {
                    (BOLD_FONT, BOLD)
                } else {
                    (REGULAR_FONT, REGULAR)
                };
                let path = scene.with_file_name(name);
                let font = paths.relative(&path, &format!("{ip}.spans"))?;
                if !fonts.iter().any(|(p, _)| *p == path) {
                    fonts.push((path, Cow::Borrowed(bytes)));
                }
                font
            }
            Some(family) => paths.relative(&base.join(family), &format!("{ip}.spans"))?,
        };
        let mut s = Section::new(out, &format!("[text.{}]", key(&object.name)), &ip);
        s.put("text", &string(&span.text));
        s.put("font", &string(&font));
        s.num("size", span.size * scale[0])?;
        s.list("color", &[span.color[0], span.color[1], span.color[2], 1.0])?;
        s.list("at", &at)?;
        s.list("rotate", &rotate)?;
        if recipe.dynamic(index) {
            s.put("dynamic", "true");
        }
    }
    Ok(fonts)
}

fn frame(built: &Built, recipe: &Recipe, index: usize, on: Option<&str>) -> Result<DMat4, Error> {
    let object = &recipe.objects[index];
    let Some(on) = on else {
        return world(recipe, index);
    };
    let Some(found) = recipe.objects.iter().position(|o| o.name == on) else {
        return Err(Error::new(
            format!("object[{}].on", object.name),
            format!("no object is named `{on}`"),
        ));
    };
    let placed = world(recipe, found)?;
    Ok(placed
        * face(&recipe.objects[found], &built.parts, found, placed)
        * DMat4::from_translation(DVec3::new(0.0, 0.0, LIFT))
        * local_matrix(object))
}

fn face(target: &Object, parts: &[Part], index: usize, world: DMat4) -> DMat4 {
    let back = world.inverse();
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for part in parts.iter().filter(|part| part.object == index) {
        let node = &part.node;
        for p in &node.mesh.positions {
            let p = DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
            let local = if node.keep_transform {
                p
            } else {
                back.transform_point3(p)
            };
            for (axis, v) in local.to_array().into_iter().enumerate() {
                low[axis] = low[axis].min(v);
                high[axis] = high[axis].max(v);
            }
        }
    }
    if low[0] > high[0] {
        return DMat4::IDENTITY;
    }
    let mid = |axis: usize| (low[axis] + high[axis]) * 0.5;
    match target.kind {
        Kind::Plane { .. } | Kind::Disc { .. } => DMat4::from_cols(
            [1.0, 0.0, 0.0, 0.0].into(),
            [0.0, 0.0, -1.0, 0.0].into(),
            [0.0, 1.0, 0.0, 0.0].into(),
            [mid(0), high[1], mid(2), 1.0].into(),
        ),
        _ => DMat4::from_translation([mid(0), mid(1), high[2]].into()),
    }
}

fn key(name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare { name.to_string() } else { string(name) }
}

struct Paths {
    folder: PathBuf,
    root: PathBuf,
    format: u32,
}

impl Paths {
    fn of(scene: &Path, format: u32) -> Result<Paths, Error> {
        let root = Project::root_of(&absolute(scene, "output.scene")?);
        let folder = if format == 0 {
            folder(scene)?
        } else {
            root.clone()
        };
        Ok(Paths {
            folder,
            root,
            format,
        })
    }

    fn pick(&self) -> &'static str {
        if self.format == 0 { "id" } else { "pick" }
    }

    fn relative(&self, file: &Path, key: &str) -> Result<String, Error> {
        let path = relative(&self.folder, file, key)?;
        if relative(&self.root, file, key)?.split('/').next() == Some("..") {
            return Err(Error::new(
                key,
                format!(
                    "{} is outside the project root {}, and the engine reads every path of a scene inside its root, expected a file under the root, or a project.toml in a folder above both",
                    absolute(file, key)?.display(),
                    self.root.display()
                ),
            ));
        }
        Ok(path)
    }
}

struct Ids {
    source: String,
    taken: BTreeSet<Id>,
    bases: BTreeMap<usize, (Id, usize)>,
}

fn plain_key(source: &str, kind: &str, name: &str) -> String {
    format!("{source}\n{kind}\n{name}")
}

fn source_of(recipe: &Recipe, root: &Path) -> Result<Option<String>, Error> {
    match recipe.files.main_path() {
        Some(path) => Ok(Some(relative(root, path, "output.scene")?)),
        None => Ok(None),
    }
}

fn nodes_of(built: &Built) -> BTreeMap<&str, (usize, usize)> {
    let mut count: BTreeMap<usize, usize> = BTreeMap::new();
    let mut nodes: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for part in &built.parts {
        let ordinal = count.entry(part.object).or_default();
        if shown(part) {
            nodes.insert(part.node.name.as_str(), (part.object, *ordinal));
        }
        *ordinal += 1;
    }
    nodes
}

fn unique(taken: &mut BTreeSet<Id>, key: &str) -> Id {
    let mut id = Id::derive(key.as_bytes());
    let mut bump = 0;
    while !taken.insert(id) {
        bump += 1;
        id = Id::derive(format!("{key}\n{bump}").as_bytes());
    }
    id
}

fn bases(
    recipe: &Recipe,
    nodes: &BTreeMap<&str, (usize, usize)>,
    source: &str,
    built: bool,
) -> (BTreeSet<Id>, BTreeMap<usize, (Id, usize)>) {
    let mut taken: BTreeSet<Id> = recipe
        .objects
        .iter()
        .filter_map(|o| o.id.as_deref())
        .filter_map(|id| Id::parse(id).ok())
        .collect();
    let mut bases = BTreeMap::new();
    for (index, object) in recipe.objects.iter().enumerate() {
        let first = nodes
            .iter()
            .filter(|(_, (owner, _))| *owner == index)
            .min_by_key(|(_, (_, ordinal))| *ordinal)
            .map(|(name, (_, ordinal))| ((*name).to_string(), *ordinal));
        let generator = matches!(object.kind, Kind::Generator { .. });
        let (name, ordinal) = match first {
            Some(first) => first,
            None if generator && !built => continue,
            None => (object.name.clone(), 0),
        };
        let own = object.id.as_deref().and_then(|id| Id::parse(id).ok());
        let id = match own {
            Some(id) => id,
            None => unique(&mut taken, &plain_key(source, "object", &name)),
        };
        bases.insert(index, (id, ordinal));
    }
    (taken, bases)
}

pub fn object_ids(
    recipe: &Recipe,
    built: Option<&Built>,
    base: &Path,
    out: Option<&Path>,
) -> Result<Vec<(String, String)>, Error> {
    let Some(file) = &recipe.output.scene else {
        return Ok(Vec::new());
    };
    let scene = crate::target(
        Some(file),
        format!("{}{}", recipe.name, SCENE_SUFFIX),
        base,
        out,
    );
    let paths = Paths::of(&scene, 1)?;
    let source = source_of(recipe, &paths.folder)?.unwrap_or_else(|| recipe.name.clone());
    let nodes = built.map(nodes_of).unwrap_or_default();
    let (_, bases) = bases(recipe, &nodes, &source, built.is_some());
    Ok(recipe
        .objects
        .iter()
        .enumerate()
        .filter(|(_, object)| object.id.is_none() && recipe.copy_of(&object.name).is_none())
        .filter_map(|(index, object)| {
            bases
                .get(&index)
                .map(|(id, _)| (object.name.clone(), id.to_string()))
        })
        .collect())
}

impl Ids {
    fn next(&mut self, owner: Option<(usize, usize)>, kind: &str, name: &str) -> Id {
        if let Some((index, ordinal)) = owner
            && let Some(&(base, first)) = self.bases.get(&index)
        {
            if kind == "object" && ordinal == first {
                return base;
            }
            return unique(&mut self.taken, &format!("{base}\n{kind}\n{ordinal}"));
        }
        unique(&mut self.taken, &plain_key(&self.source, kind, name))
    }
}

fn one(
    written: Written,
    built: &Built,
    recipe: &Recipe,
    paths: &Paths,
    stands: Option<(PathBuf, Vec<proxies::Stand>)>,
) -> Result<Written, Error> {
    let refused = |key: &str, e: &dyn std::fmt::Display| {
        Error::new(
            key,
            format!("pfx-scene's format 1 types refuse what Manfred wrote: {e}"),
        )
    };
    let mut scene: SceneFile =
        toml::from_str(&written.text).map_err(|e| refused("output.scene", &e))?;
    let source = source_of(recipe, &paths.folder)?;
    let mut authoring = toml::Table::new();
    if let Some(source) = &source {
        authoring.insert("recipe".into(), toml::Value::String(source.clone()));
    }
    if let Some(variant) = recipe.files.variant() {
        authoring.insert("variant".into(), toml::Value::String(variant.to_string()));
    }
    let authoring = (!authoring.is_empty()).then_some(authoring);
    let source = source.unwrap_or_else(|| recipe.name.clone());
    let nodes = nodes_of(built);
    let (taken, bases) = bases(recipe, &nodes, &source, true);
    let mut ids = Ids {
        source,
        taken,
        bases,
    };
    let named = |name: &str| {
        recipe
            .objects
            .iter()
            .position(|o| o.name == name)
            .map(|index| (index, 0))
    };
    let next = |ids: &mut Ids, owner: Option<(usize, usize)>, kind: &str, name: &str| {
        Some(ids.next(owner, kind, name))
    };
    for (name, mesh) in &mut scene.mesh {
        mesh.id = next(&mut ids, nodes.get(name.as_str()).copied(), "mesh", name);
    }
    for object in &mut scene.object {
        let owner = nodes.get(object.name.as_str()).copied();
        object.id = next(&mut ids, owner, "object", &object.name);
    }
    for (name, content) in &mut scene.content {
        content.id = next(&mut ids, named(name), "content", name);
    }
    for (name, text) in &mut scene.text {
        text.id = next(&mut ids, named(name), "text", name);
    }
    scene.format = Some(FORMAT);
    scene.authoring.clone_from(&authoring);
    let library = match written.library {
        Some((path, text)) => {
            let mut library: MaterialsFile =
                toml::from_str(&text).map_err(|e| refused("materials", &e))?;
            for (name, material) in &mut library.materials {
                material.id = next(&mut ids, None, "materials", name);
                if let Some(wet) = recipe.effects.wet.get(name) {
                    material.wet = Some(effects::wet(wet));
                }
            }
            library.format = Some(FORMAT);
            library.authoring.clone_from(&authoring);
            let text = toml::to_string(&library).map_err(|e| refused("materials", &e))?;
            Some((path, spaced(text)))
        }
        None => None,
    };
    for light in &mut scene.light {
        light.id = next(&mut ids, None, "light", &light.name);
    }
    for particles in &mut scene.particles {
        particles.id = next(&mut ids, None, "particles", &particles.name);
    }
    let text = toml::to_string(&scene).map_err(|e| refused("output.scene", &e))?;
    let text = effects::unplaced(text, recipe, built);
    let mut fonts = written.files;
    if let Some((file, stands)) = stands {
        let owned: Vec<Id> = stands
            .iter()
            .map(|stand| {
                let owner = nodes.get(stand.object.as_str()).copied();
                ids.next(owner, "proxy", &stand.object)
            })
            .collect();
        let head = format!("format = {FORMAT}\n");
        let text = proxies::text(&stands, &owned, &head, authoring.as_ref())?;
        fonts.push((file, Cow::Owned(text.into_bytes())));
    }
    Ok(Written {
        text: spaced(inline_passes(text)),
        library,
        files: fonts,
    })
}

fn inline_passes(text: String) -> String {
    let Ok(mut doc) = text.parse::<DocumentMut>() else {
        return text;
    };
    let Some(passes) = doc
        .get_mut("finish")
        .and_then(|finish| finish.get_mut("pass"))
        .and_then(Item::as_array_of_tables_mut)
    else {
        return text;
    };
    for pass in passes.iter_mut() {
        for (mut key, item) in pass.iter_mut() {
            if let Some(table) = item.as_table() {
                key.fmt();
                let mut inline = table.clone().into_inline_table();
                inline.fmt();
                *item = Item::Value(Value::InlineTable(inline));
            }
        }
    }
    doc.to_string()
}

fn spaced(text: String) -> String {
    let head = format!("format = {FORMAT}\n");
    match text.strip_prefix(&head) {
        Some(rest) if !rest.is_empty() && !rest.starts_with('\n') => format!("{head}\n{rest}"),
        _ => text,
    }
}

fn folder(scene: &Path) -> Result<PathBuf, Error> {
    let absolute = absolute(scene, "output.scene")?;
    match absolute.parent() {
        Some(parent) => Ok(parent.to_path_buf()),
        None => Err(Error::new("output.scene", "expected a file name")),
    }
}

fn relative(folder: &Path, file: &Path, key: &str) -> Result<String, Error> {
    let file = absolute(file, key)?;
    let from: Vec<_> = folder.components().collect();
    let to: Vec<_> = file.components().collect();
    let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_string(); from.len() - shared];
    parts.extend(
        to[shared..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    if parts.is_empty() {
        return Err(Error::new(key, "expected a file, found the scene's folder"));
    }
    Ok(parts.join("/"))
}

fn png_header(bytes: &[u8]) -> Result<(), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let reader = decoder.read_info().map_err(|e| format!("png: {e}"))?;
    let (width, height) = reader.info().size();
    if width == 0 || height == 0 {
        return Err("png: the image is empty".into());
    }
    reader
        .output_buffer_size()
        .map(|_| ())
        .ok_or_else(|| "png: the image is too large".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_scene_holds_as_many_local_lights_as_pfx_live_draws() {
        assert_eq!(
            super::MAX_LIGHTS,
            pfx_live::lights::MAX_LIGHTS + pfx_live::lights::MAX_CLUSTERED
        );
    }
}
