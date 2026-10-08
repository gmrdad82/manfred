mod common;

use common::scratch;
use manfred_build::{Error, Registry, build_with, write};
use manfred_recipe::{Recipe, load, parse, validate};
use pfx_bake::plate::parse_plates;
use pfx_load::scene::{PlateCasters, ProxyShape, Scene};
use pfx_scene::{Id, Project};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const DESK: &str = include_str!("plates/desk.toml");
const CATALOGUE: &str = "[materials.wood]\nbase = [0.4, 0.25, 0.12]\nroughness = 0.6\n";

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.register_materials_toml(CATALOGUE).unwrap();
    registry
}

fn desk(edit: &[(&str, &str)]) -> Recipe {
    let mut text = DESK.to_string();
    for (from, to) in edit {
        assert!(text.contains(from), "{from}");
        text = text.replacen(from, to, 1);
    }
    let recipe = parse(&text).unwrap();
    assert_eq!(validate(&recipe), []);
    recipe
}

fn rooted(name: &str) -> PathBuf {
    let base = scratch(name);
    std::fs::write(
        base.join("project.toml"),
        "format = 1\n\n[project]\nname = \"desk\"\n",
    )
    .unwrap();
    base
}

fn written(recipe: &Recipe, name: &str) -> Result<(PathBuf, Vec<PathBuf>), Error> {
    let base = rooted(name);
    let built = build_with(recipe, &base, &registry())?;
    let files = write(&built, recipe, &base, None)?;
    Ok((base, files))
}

fn checked(root: &Path) {
    let diagnostics = Project::open(root).unwrap().check();
    let lines: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
    assert!(lines.is_empty(), "{}", lines.join("\n"));
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn table<'a>(text: &'a str, header: &str) -> &'a str {
    let start = text
        .find(header)
        .unwrap_or_else(|| panic!("{header}\n{text}"));
    let rest = &text[start..];
    rest.find("\n\n").map_or(rest, |end| &rest[..=end])
}

#[test]
fn a_recipe_with_plates_writes_a_bake_that_pfx_reads() {
    let (base, files) = written(&desk(&[]), "plates-bake").unwrap();
    let bake = read(&base.join("bake.toml"));
    for (file, name) in [
        (&files[0], "scene/desk.glb"),
        (&files[2], "bake.toml"),
        (&files[5], "scene/desk.proxies.toml"),
        (&files[6], "scene/desk.scene.toml"),
    ] {
        assert_eq!(*file, base.join(name), "{files:?}");
    }
    assert_eq!(
        table(&bake, "[plates]\n"),
        "[plates]\nscene = \"scene/desk.scene.toml\"\nsize = [1920, 1080]\noverscan = 0.2\nscale = 1.25\nanchors = [12.0, 16.0]\nlayers = [\"color\", \"depth\", \"normal\", \"id\", \"sun\"]\nmax_samples = 256\nkey = \"desk-v1\"\n"
    );
    pfx_bake::Recipe::parse(bake.as_bytes()).unwrap();
    let plates = parse_plates(bake.as_bytes()).unwrap().unwrap();
    assert_eq!(plates.scene, "scene/desk.scene.toml");
    assert_eq!(plates.size, [1920, 1080]);
    assert_eq!((plates.overscan, plates.scale), (0.2, 1.25));
    assert_eq!(plates.anchors, Some(vec![12.0, 16.0]));
    assert_eq!(
        plates.layers.names(),
        ["color", "depth", "normal", "id", "sun"]
    );
    assert_eq!(plates.adaptive.max_samples, 256);
    assert_eq!(plates.adaptive.min_samples, 16);
    assert_eq!(plates.seed, None);
    assert_eq!(plates.key.as_deref(), Some("desk-v1"));
    let names: Vec<&str> = plates.views.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["desk", "side"]);
    let main = &plates.views[0].keys;
    assert_eq!(main.at, Some([0.0, 0.4, 1.0]));
    assert_eq!(main.look_at, Some([0.0, 0.0, 0.0]));
    assert_eq!(main.fov, Some(30.0));
    assert_eq!(main.shift, Some([0.0, -0.1]));
    let (near, far) = (main.near.unwrap(), main.far.unwrap());
    assert!(near > 0.0 && far > near, "{near} {far}");
    let side = &plates.views[1].keys;
    assert_eq!(side.fov, Some(40.0));
    assert_eq!((side.near, side.far, side.up), (None, None, None));
}

#[test]
fn defaults_are_left_out_of_the_bakes_plates() {
    let recipe = desk(&[
        ("proxy = \"hull\"\n", ""),
        (
            "overscan = 0.2\nscale = 1.25\nanchors = [12.0, 16.0]\nlayers = [\"color\", \"depth\", \"normal\", \"id\", \"sun\"]\nmax_samples = 256\nkey = \"desk-v1\"\ndir = \"plates/desk\"\ncasters = \"proxies\"\n",
            "overscan = 0.1\nanchors = [8.0, 12.0, 16.0]\nlayers = [\"transfer\", \"color\", \"depth\", \"normal\", \"id\", \"sun\"]\nseed = 11\n",
        ),
    ]);
    let (base, files) = written(&recipe, "plates-defaults").unwrap();
    let bake = read(&base.join("bake.toml"));
    assert_eq!(
        table(&bake, "[plates]\n"),
        "[plates]\nscene = \"scene/desk.scene.toml\"\nsize = [1920, 1080]\n"
    );
    let plates = parse_plates(bake.as_bytes()).unwrap().unwrap();
    assert_eq!(plates.overscan, 0.1);
    assert_eq!(plates.anchors, None);
    assert_eq!(plates.layers.names().len(), 6);
    let scene = read(&base.join("scene/desk.scene.toml"));
    assert!(!scene.contains("[plates]"), "{scene}");
    assert!(!files.contains(&base.join("scene/desk.proxies.toml")));
}

#[test]
fn the_scene_and_its_proxies_load_through_scene_open() {
    let (base, _) = written(&desk(&[]), "plates-scene").unwrap();
    let text = read(&base.join("scene/desk.scene.toml"));
    assert!(text.starts_with("format = 1\n"), "{text}");
    assert_eq!(
        table(&text, "[plates]\n"),
        "[plates]\ndir = \"plates/desk\"\nproxies = \"scene/desk.proxies.toml\"\ncasters = \"proxies\"\n"
    );
    checked(&base);
    let scene = Scene::open(base.join("scene/desk.scene.toml")).unwrap();
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    stands(&scene);
}

#[test]
fn format_0_still_writes_the_plates_and_the_engine_migrates_them() {
    let recipe = desk(&[(
        "scene = \"scene/desk.scene.toml\"",
        "scene = \"scene/desk.scene.toml\"\nscene_format = 0",
    )]);
    let (base, _) = written(&recipe, "plates-format0").unwrap();
    let text = read(&base.join("scene/desk.scene.toml"));
    assert_eq!(
        table(&text, "[plates]\n"),
        "[plates]\ndir = \"../plates/desk\"\nproxies = \"desk.proxies.toml\"\ncasters = \"proxies\"\n"
    );
    let scene = Scene::open(base.join("scene/desk.scene.toml")).unwrap();
    assert!(!scene.warnings.is_empty());
    stands(&scene);
}

fn stands(scene: &Scene) {
    let plates = scene.plates.as_ref().unwrap();
    assert!(plates.dir.ends_with("plates/desk"), "{:?}", plates.dir);
    assert_eq!(plates.manifest, None);
    assert_eq!(plates.casters, PlateCasters::Proxies);
    let proxies = plates.proxies.as_ref().unwrap();
    let stood: Vec<&str> = proxies.items.iter().map(|p| p.object.as_str()).collect();
    assert_eq!(stood, ["top", "ball", "pinned"]);
    let ProxyShape::Box { at, size, .. } = proxies.items[0].shape else {
        panic!("the top stands as a box");
    };
    assert_eq!(at, [0.0, 0.0, 0.0]);
    assert_eq!(size, [0.6, 0.04, 0.4]);
    assert_eq!(proxies.items[0].id, 1);
    let ProxyShape::Hull { points, triangles } = &proxies.items[1].shape else {
        panic!("the ball stands as a hull");
    };
    assert_eq!(points.len(), 16 * 7 + 2);
    assert_eq!(triangles.len(), 2 * points.len() - 4);
    let (id, distance) = proxies.pick([0.101, 0.5, 0.001], [0.0, -1.0, 0.0]).unwrap();
    assert_eq!(id, 2);
    assert!((distance - 0.38).abs() < 1e-3, "{distance}");
}

#[test]
fn what_moves_is_dynamic_and_the_rest_static_as_pfx_derives_it() {
    let (base, _) = written(&desk(&[]), "plates-dynamic").unwrap();
    checked(&base);
    let scene = Scene::open(base.join("scene/desk.scene.toml")).unwrap();
    let dynamic: BTreeSet<String> = ["mv_lid", "cup", "saucer", "tag"].map(String::from).into();
    assert_eq!(scene.dynamic_names(), dynamic);
    let text = read(&base.join("scene/desk.scene.toml"));
    let keyed = |name: &str| {
        let entry = text
            .split("[[object]]\n")
            .find(|entry| entry.contains(&format!("\nname = \"{name}\"\n")))
            .unwrap_or_else(|| panic!("{name}\n{text}"));
        let entry = &entry[..entry.find("\n\n").unwrap_or(entry.len())];
        entry
            .lines()
            .find_map(|line| line.strip_prefix("dynamic = "))
            .map(str::to_string)
    };
    assert_eq!(keyed("top"), None);
    assert_eq!(keyed("ball"), None);
    assert_eq!(keyed("mv_lid").as_deref(), Some("true"));
    assert_eq!(keyed("pinned").as_deref(), Some("false"));
    assert_eq!(keyed("cup").as_deref(), Some("true"));
    assert_eq!(keyed("saucer").as_deref(), Some("true"));
    assert_eq!(keyed("tag"), None);
    let texts: Vec<(&str, bool)> = scene
        .texts
        .iter()
        .map(|(name, text)| (name.as_str(), text.dynamic))
        .collect();
    assert_eq!(texts, [("clock", true), ("label", false), ("price", true)]);
    let subset = scene.static_subset();
    assert_eq!(subset.texts.len(), 1);
    let hidden: Vec<&str> = subset
        .objects
        .iter()
        .filter(|o| o.hidden)
        .map(|o| o.name.as_str())
        .collect();
    assert_eq!(hidden, ["mv_lid", "cup", "saucer", "tag"]);
}

#[test]
fn the_same_recipe_writes_the_same_plates_and_proxies() {
    let recipe = desk(&[]);
    let (a, _) = written(&recipe, "plates-twice-a").unwrap();
    let (b, _) = written(&recipe, "plates-twice-b").unwrap();
    for name in [
        "bake.toml",
        "scene/desk.scene.toml",
        "scene/desk.proxies.toml",
    ] {
        assert_eq!(read(&a.join(name)), read(&b.join(name)), "{name}");
    }
}

#[test]
fn a_flat_object_has_no_hull() {
    let recipe = desk(&[(
        "slot = \"tag\"\nface_camera = true\nkeep_transform = true\n",
        "slot = \"tag\"\nproxy = \"hull\"\n",
    )]);
    let error = written(&recipe, "plates-flat").unwrap_err();
    assert_eq!(error.path, "object[tag].proxy");
    assert!(error.text.contains("flat"), "{error}");
}

#[test]
fn a_scene_outside_the_bake_files_folder_is_refused() {
    let recipe = desk(&[("bake = \"bake.toml\"", "bake = \"bakes/bake.toml\"")]);
    let error = written(&recipe, "plates-outside").unwrap_err();
    assert_eq!(error.path, "output.glb");
    let recipe = desk(&[
        ("bake = \"bake.toml\"", "bake = \"scene/bake.toml\""),
        (
            "scene = \"scene/desk.scene.toml\"",
            "scene = \"desk.scene.toml\"",
        ),
    ]);
    let error = written(&recipe, "plates-outside-scene").unwrap_err();
    assert_eq!(error.path, "output.scene");
}

#[test]
fn with_out_the_bake_names_the_scene_in_that_folder() {
    let recipe = desk(&[]);
    let base = rooted("plates-out");
    let out = base.join("out");
    let built = build_with(&recipe, &base, &registry()).unwrap();
    write(&built, &recipe, &base, Some(&out)).unwrap();
    let bake = read(&out.join("bake.toml"));
    assert!(
        bake.contains("[plates]\nscene = \"desk.scene.toml\"\n"),
        "{bake}"
    );
    let scene = read(&out.join("desk.scene.toml"));
    assert!(
        scene.contains("proxies = \"out/desk.proxies.toml\""),
        "{scene}"
    );
    checked(&base);
    Scene::open(out.join("desk.scene.toml")).unwrap();
}

#[test]
fn with_out_the_folder_of_the_glb_and_the_bake_the_scene_still_lands_there() {
    let recipe = desk(&[
        ("glb = \"scene/desk.glb\"", "glb = \"shipped/desk.glb\""),
        ("bake = \"bake.toml\"", "bake = \"shipped/bake.toml\""),
    ]);
    let base = rooted("plates-out-shared");
    let out = base.join("shipped");
    let built = build_with(&recipe, &base, &registry()).unwrap();
    write(&built, &recipe, &base, Some(&out)).unwrap();
    let bake = read(&out.join("bake.toml"));
    assert!(
        bake.contains("[plates]\nscene = \"desk.scene.toml\"\n"),
        "{bake}"
    );
    Scene::open(out.join("desk.scene.toml")).unwrap();
}

#[test]
fn format_1_carries_the_plate_keys_with_root_paths_and_proxy_ids() {
    let root = rooted("plates-format1");
    let recipes = root.join("recipes");
    std::fs::create_dir_all(&recipes).unwrap();
    let text = DESK.replacen(
        "scene = \"scene/desk.scene.toml\"",
        "scene = \"scene/desk.scene.toml\"\nscene_format = 1",
        1,
    );
    std::fs::write(recipes.join("desk.toml"), text).unwrap();
    let recipe = load(&recipes.join("desk.toml")).unwrap().recipe;
    assert_eq!(validate(&recipe), []);
    let built = build_with(&recipe, &recipes, &registry()).unwrap();
    write(&built, &recipe, &recipes, None).unwrap();
    let scene = read(&recipes.join("scene/desk.scene.toml"));
    assert!(scene.starts_with("format = 1\n"), "{scene}");
    assert!(
        scene.ends_with("\n\n[plates]\ndir = \"recipes/plates/desk\"\nproxies = \"recipes/scene/desk.proxies.toml\"\ncasters = \"proxies\"\n\n[authoring]\nrecipe = \"recipes/desk.toml\"\n"),
        "{scene}"
    );
    let file: toml::Table = toml::from_str(&scene).unwrap();
    let dynamic: Vec<(&str, bool)> = file["object"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|o| Some((o["name"].as_str()?, o.get("dynamic")?.as_bool()?)))
        .collect();
    assert_eq!(
        dynamic,
        [
            ("mv_lid", true),
            ("pinned", false),
            ("cup", true),
            ("saucer", true)
        ]
    );
    let texts = file["text"].as_table().unwrap();
    for (name, live) in [("clock", true), ("label", false), ("price", true)] {
        assert_eq!(
            texts[name].get("dynamic").and_then(|v| v.as_bool()),
            live.then_some(true),
            "{name}"
        );
    }
    let top = file["object"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["name"].as_str() == Some("top"))
        .unwrap();
    let top_id = top["id"].as_str().unwrap();
    let proxies = read(&recipes.join("scene/desk.proxies.toml"));
    assert!(
        proxies.starts_with(&format!(
            "format = 1\n\n[[proxy]]\nid = \"{}\"\nobject = \"top\"\nkind = \"box\"\n",
            Id::derive(format!("{top_id}\nproxy\n0").as_bytes())
        )),
        "{proxies}"
    );
    assert!(
        proxies.ends_with("\n\n[authoring]\nrecipe = \"recipes/desk.toml\"\n"),
        "{proxies}"
    );
    let proxies: toml::Table = toml::from_str(&proxies).unwrap();
    assert_eq!(proxies["proxy"].as_array().unwrap().len(), 3);
    checked(&root);
}

#[test]
fn the_plates_firefly_settings_reach_the_bake_and_replace_the_scenes() {
    let recipe = desk(&[
        (
            "casters = \"proxies\"\n",
            "casters = \"proxies\"\nclamp_indirect = 0\nfilter_glossy = 0.4\n",
        ),
        (
            "[bake.plates]\n",
            "[trace]\nclamp_indirect = 4\nfilter_glossy = 0.25\n\n[bake.plates]\n",
        ),
    ]);
    let (base, _) = written(&recipe, "plates-fireflies").unwrap();
    let bake = read(&base.join("bake.toml"));
    assert!(
        table(&bake, "[plates]\n")
            .ends_with("key = \"desk-v1\"\nclamp_indirect = 0.0\nfilter_glossy = 0.4\n"),
        "{bake}"
    );
    let plates = parse_plates(bake.as_bytes()).unwrap().unwrap();
    assert_eq!(
        (plates.clamp_indirect, plates.filter_glossy),
        (Some(0.0), Some(0.4))
    );
    let scene = Scene::open(base.join("scene/desk.scene.toml")).unwrap();
    assert_eq!(
        (scene.trace.clamp_indirect, scene.trace.filter_glossy),
        (4.0, 0.25)
    );
    let baked = plates.trace(scene.trace);
    assert_eq!((baked.clamp_indirect, baked.filter_glossy), (0.0, 0.4));
    let (base, _) = written(&desk(&[]), "plates-fireflies-off").unwrap();
    let bake = read(&base.join("bake.toml"));
    assert!(
        !bake.contains("clamp_indirect") && !bake.contains("filter_glossy"),
        "{bake}"
    );
}
