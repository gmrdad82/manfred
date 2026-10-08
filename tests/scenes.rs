use std::path::{Path, PathBuf};

use manfred::build::{Built, Call, Generated, Piece, Registry, build_with, write};
use manfred::recipe::{Recipe, apply_variant, load, parse, validate};
use manfred_render::{Request, Staging, View, render};
use serde_json::{Value, json};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn golden(set: &str) -> PathBuf {
    root().join("tests/golden").join(set)
}

fn scratch(name: &str) -> PathBuf {
    let dir = root().join("tmp/scenes").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn recipe(set: &str) -> Recipe {
    load(&golden(set).join(format!("{set}.toml")))
        .unwrap()
        .recipe
}

fn built(recipe: &Recipe, folder: &Path) -> Built {
    build_with(recipe, folder, &Registry::new()).unwrap()
}

fn matches_golden(recipe: &Recipe, set: &str, files: [&str; 2]) {
    assert_eq!(validate(recipe), []);
    let folder = golden(set);
    let made = built(recipe, &folder);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        write(&made, recipe, &folder, None).unwrap();
    }
    let out = scratch(&format!("golden-{}", files[0]));
    write(&made, recipe, &folder, Some(&out)).unwrap();
    for name in files {
        assert!(
            std::fs::read(out.join(name)).unwrap() == std::fs::read(folder.join(name)).unwrap(),
            "{name} differs from its golden; UPDATE_GOLDEN=1 regenerates it"
        );
    }
}

fn manfred_extras(made: &Built, node: &str) -> Option<Value> {
    let summary = manfred::glb::summary(&made.glb()).unwrap();
    let found = summary.nodes.into_iter().find(|n| n.name == node)?;
    found.extras.map(|extras| extras["manfred"].clone())
}

#[test]
fn the_flatslab_scene_matches_its_golden_glb_and_sidecar() {
    matches_golden(
        &recipe("flatslab"),
        "flatslab",
        ["flatslab.glb", "flatslab.json"],
    );
}

#[test]
fn flat_faces_keep_a_large_face_to_its_own_normal_and_nothing_else_changes() {
    let r = recipe("flatslab");
    let made = built(&r, &golden("flatslab"));
    let mesh = |name: &str| {
        &made
            .parts
            .iter()
            .find(|p| p.node.name == name)
            .unwrap()
            .node
            .mesh
    };
    let tilt = |name: &str| {
        let m = mesh(name);
        let mut worst = 0.0_f64;
        for t in m.indices.chunks_exact(3) {
            let face = m.face_normal(t).to_array();
            if face.iter().all(|c| c.abs() < 1.0 - 1e-6) {
                continue;
            }
            for &i in t {
                let n = m.normals[i as usize];
                let dot: f64 = (0..3).map(|k| f64::from(n[k]) * face[k]).sum();
                worst = worst.max(dot.clamp(-1.0, 1.0).acos().to_degrees());
            }
        }
        worst
    };
    for name in ["strip", "plate", "block"] {
        assert!(tilt(name) < 1e-4, "{name}");
    }
    assert!(tilt("strip_smoothed") > 4.0);
    let plain = mesh("strip_smoothed");
    let flat = mesh("strip");
    assert!(plain.positions.len() < flat.positions.len());
}

#[test]
fn the_cards_scene_matches_its_golden_glb_and_sidecar() {
    matches_golden(&recipe("cards"), "cards", ["cards.glb", "cards.json"]);
}

#[test]
fn the_cards_scene_writes_its_cards_glass_and_text_as_nodes() {
    let r = recipe("cards");
    let made = built(&r, &golden("cards"));
    let bytes = made.glb();
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let json: Value = serde_json::from_slice(&bytes[20..20 + length]).unwrap();
    let nodes: Vec<(&str, bool)> = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| (n["name"].as_str().unwrap(), n.get("mesh").is_some()))
        .collect();
    assert_eq!(
        nodes,
        [
            ("board", true),
            ("badge", true),
            ("slot", true),
            ("sign", true),
            ("pane", true),
            ("shade", true),
            ("name_plate", false),
            ("note", false),
            ("corner", false),
        ]
    );
    let summary = manfred::glb::summary(&bytes).unwrap();
    for card in ["badge", "slot", "sign"] {
        let node = summary.nodes.iter().find(|n| n.name == card).unwrap();
        assert_eq!(node.triangles, 2, "{card}");
        assert!(node.uvs, "{card}");
    }
    assert_eq!(
        manfred_extras(&made, "pane"),
        Some(json!({"shadow": "none"}))
    );
    assert_eq!(
        manfred_extras(&made, "shade"),
        Some(json!({"shadow": "only"}))
    );
    assert_eq!(manfred_extras(&made, "board"), None);
}

#[test]
fn the_cards_scene_stages_its_images_cards_and_text_for_the_tracer() {
    let r = recipe("cards");
    let made = built(&r, &golden("cards"));
    let mut placed = r.clone();
    placed.base = Some(golden("cards"));
    let staging = Staging::of(&made.recipe(&placed), &made.parts).unwrap();
    let cards = staging.cards.iter().filter(|c| c.is_some()).count();
    assert_eq!(cards, 3);
    assert_eq!(staging.images.len(), 2);
    assert_eq!(staging.text.items.len(), 3);
    assert!(staging.names.iter().any(|n| n == "glass"));
}

#[test]
fn every_golden_scene_builds_with_manfred_alone() {
    for set in ["cards", "flatslab", "table", "toybox"] {
        let r = recipe(set);
        assert_eq!(validate(&r), [], "{set}");
        let made = built(&r, &golden(set));
        assert!(!made.parts.is_empty(), "{set}");
        assert!(made.notes.is_empty(), "{set}: {:?}", made.notes);
        let sidecar: Value = serde_json::from_str(&made.sidecar().unwrap()).unwrap();
        assert!(sidecar["cameras"].is_array(), "{set}");
        for material in made.recipe(&r).materials.keys() {
            assert!(r.materials.contains_key(material), "{set}: {material}");
        }
    }
}

const BOXES: &str = "manfred = 1\nname = \"boxes\"\n\n[[camera]]\nname = \"eye\"\nposition = [0.0, 0.6, 1.6]\ntarget = [0.0, 0.1, 0.0]\nfov_y_deg = 35.0\n\n[sun]\nmodel = \"authored\"\ntoward = [0.3, 0.8, 0.5]\nirradiance = 3.0\n\n[[object]]\nname = \"floor\"\nkind = \"plane\"\nsize = [2.0, 2.0]\nmaterial = \"made_floor\"\n\n[[object]]\nname = \"pair\"\nkind = \"generator\"\ngenerator = \"test/pair\"\n";

const MADE: &str = "\n[materials.made_floor]\nbase = [0.5, 0.5, 0.5]\nroughness = 0.8\n\n[materials.made_glass]\nfamily = \"glass\"\nbase = [0.9, 0.95, 1.0]\nroughness = 0.02\ntransmission = 1.0\nior = 1.45\nthickness = 0.004\n\n[materials.made_lacquer]\nfamily = \"lacquer\"\nbase = [0.6, 0.1, 0.05]\nroughness = 0.3\nclearcoat = 0.8\n";

fn pair_pieces() -> Vec<Piece> {
    let mut glass = manfred::mesh::rounded_box([0.3, 0.3, 0.02], 0.0, 1);
    glass.set_material("made_glass");
    manfred::mesh::transform(
        &mut glass,
        manfred::glb::DMat4::from_translation([-0.25, 0.15, 0.0].into()),
    );
    let mut block = manfred::mesh::rounded_box([0.2, 0.2, 0.2], 0.02, 3);
    block.set_material("made_lacquer");
    manfred::mesh::transform(
        &mut block,
        manfred::glb::DMat4::from_translation([0.25, 0.1, 0.0].into()),
    );
    vec![
        Piece {
            name: "pane".into(),
            mesh: glass,
        },
        Piece {
            name: "block".into(),
            mesh: block,
        },
    ]
}

fn with_materials(_: &Call) -> Result<Generated, String> {
    let table = manfred::recipe::materials(MADE).map_err(|e| e.to_string())?;
    Ok(Generated {
        pieces: pair_pieces(),
        materials: table
            .into_iter()
            .filter(|(name, _)| name != "made_floor")
            .collect(),
        ..Generated::default()
    })
}

fn without_materials(_: &Call) -> Result<Vec<Piece>, String> {
    Ok(pair_pieces())
}

fn generating() -> Registry {
    let mut registry = Registry::new();
    registry.register_with_facts("test/pair", with_materials);
    registry
}

fn hand_written() -> Registry {
    let mut registry = Registry::new();
    registry.register("test/pair", without_materials);
    registry
}

fn pair_files() -> (PathBuf, PathBuf) {
    let dir = scratch("pair");
    let floor = "\n[materials.made_floor]\nbase = [0.5, 0.5, 0.5]\nroughness = 0.8\n";
    let own = dir.join("own.toml");
    std::fs::write(&own, format!("{BOXES}{floor}")).unwrap();
    let written = dir.join("written.toml");
    std::fs::write(&written, format!("{BOXES}{MADE}")).unwrap();
    (own, written)
}

#[test]
fn a_generators_materials_draw_as_the_same_materials_written_by_hand() {
    let (own, written) = pair_files();
    let own_recipe = load(&own).unwrap().recipe;
    let written_recipe = load(&written).unwrap().recipe;
    let a = build_with(&own_recipe, own.parent().unwrap(), &generating()).unwrap();
    let b = build_with(&written_recipe, written.parent().unwrap(), &hand_written()).unwrap();
    assert_eq!(a.glb(), b.glb());
    let (a, b) = (
        Staging::of(&a.recipe(&own_recipe), &a.parts).unwrap(),
        Staging::of(&b.recipe(&written_recipe), &b.parts).unwrap(),
    );
    assert_eq!(a.names, b.names);
    assert_eq!(a.materials, b.materials);
}

fn traced(path: &Path, camera: &str, registry: &Registry, samples: u32) -> manfred_render::Film {
    let request = Request {
        recipe: path,
        camera,
        size: [192, 128],
        samples,
        seed: Some(1),
        view: View::Lens,
        variant: None,
        set: &[],
    };
    let film = render(&request, registry, &mut |_| {}).unwrap().film;
    pfx::adapter::turn();
    film
}

fn same_bits(a: &[[f32; 4]], b: &[[f32; 4]]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(p, q)| p.iter().zip(q).all(|(x, y)| x.to_bits() == y.to_bits()))
}

#[test]
#[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
fn a_generators_materials_render_the_same_bytes_as_hand_written_ones() {
    let (own, written) = pair_files();
    let a = traced(&own, "eye", &generating(), 16);
    let b = traced(&written, "eye", &hand_written(), 16);
    assert_eq!(a.samples, 16);
    assert!(a.color.iter().any(|p| p[0] > 0.0));
    assert!(same_bits(&a.color, &b.color), "color differs");
    assert!(same_bits(&a.albedo, &b.albedo), "albedo differs");
    assert!(same_bits(&a.normal, &b.normal), "normal differs");
    let live_a = manfred_scene::still_with(&own, "eye", [192, 128], None, &generating()).unwrap();
    pfx::adapter::turn();
    let live_b =
        manfred_scene::still_with(&written, "eye", [192, 128], None, &hand_written()).unwrap();
    pfx::adapter::turn();
    assert!(live_a == live_b, "the live stills differ");
}

fn mean(rgba: &[u8]) -> f64 {
    rgba.iter().map(|&v| f64::from(v)).sum::<f64>() / rgba.len() as f64
}

#[test]
#[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
fn the_neutral_scenes_draw_live_and_traced() {
    let out = scratch("drawn");
    for (path, camera) in [
        (golden("cards").join("cards.toml"), "main"),
        (golden("cards").join("cards.toml"), "flat"),
        (golden("toybox").join("toybox.toml"), "main"),
        (golden("toybox").join("toybox.toml"), "high"),
        (golden("table").join("table.toml"), "main"),
        (root().join("showcase/workshop.toml"), "hero"),
        (root().join("showcase/gallery.toml"), "flat"),
    ] {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let size = [480, 300];
        let live = manfred_scene::still_with(&path, camera, size, None, &Registry::new()).unwrap();
        pfx::adapter::turn();
        manfred_scene::write_png(&out.join(format!("{stem}-{camera}-live.png")), size, &live)
            .unwrap();
        let level = mean(&live);
        assert!(
            level > 10.0 && level < 245.0,
            "{stem} {camera}: live mean {level}"
        );
        let film = traced(&path, camera, &Registry::new(), 8);
        let lit = film
            .color
            .iter()
            .filter(|p| p[0] + p[1] + p[2] > 0.0)
            .count();
        assert!(
            lit * 2 > film.color.len(),
            "{stem} {camera}: {lit} lit texels"
        );
    }
}

#[test]
#[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
fn the_bare_toybox_draws_without_its_dropped_objects_and_its_sun() {
    let path = golden("toybox").join("toybox.toml");
    let size = [320, 200];
    let full = manfred_scene::still_with(&path, "main", size, None, &Registry::new()).unwrap();
    pfx::adapter::turn();
    let bare =
        manfred_scene::still_with(&path, "main", size, Some("bare"), &Registry::new()).unwrap();
    pfx::adapter::turn();
    assert!(full != bare);
    assert!(
        mean(&bare) < mean(&full),
        "{} against {}",
        mean(&bare),
        mean(&full)
    );
    let r = apply_variant(&recipe("toybox"), "bare").unwrap();
    assert_eq!(r.sun.unwrap().irradiance, 0.0);
    assert!(parse(&std::fs::read_to_string(&path).unwrap()).is_ok());
}
