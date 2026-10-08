mod common;

use common::{golden, scratch};
use manfred_build::{Error, build, object_ids, write};
use manfred_recipe::{Recipe, apply_variant, load, validate};
use pfx_scene::types::SceneFile;
use pfx_scene::{Id, Project};
use std::path::{Path, PathBuf};

const TURN: &str = "face_camera = true>face_camera = true\nkeep_transform = true";
const NOTE: &str = "{ text = \"a lit note \", size = 0.025 }, >";

fn edited(text: &str, edit: &str) -> String {
    let mut out = text.to_string();
    for (from, to) in edit.split(';').filter_map(|pair| pair.split_once('>')) {
        assert!(out.contains(from), "{from}");
        out = out.replace(from, to);
    }
    out
}

fn project(set: &str, name: &str, edit: &str, assets: &[&str]) -> (PathBuf, Recipe) {
    let root = scratch(name);
    std::fs::write(
        root.join("project.toml"),
        format!("format = 1\n\n[project]\nname = \"{set}\"\n"),
    )
    .unwrap();
    let recipes = root.join("recipes");
    std::fs::create_dir_all(&recipes).unwrap();
    let text = std::fs::read_to_string(golden(set).join(format!("{set}.toml"))).unwrap();
    let output = format!("[output]>[output]\nscene = \"{set}.scene.toml\"\nscene_format = 1");
    let text = edited(&edited(&text, &output), edit);
    std::fs::write(recipes.join(format!("{set}.toml")), text).unwrap();
    for asset in assets {
        std::fs::copy(golden(set).join(asset), recipes.join(asset)).unwrap();
    }
    let r = load(&recipes.join(format!("{set}.toml"))).unwrap().recipe;
    assert_eq!(r.output.scene_format, 1);
    (root, r)
}

fn emit(root: &Path, r: &Recipe) -> Result<Vec<PathBuf>, Error> {
    let base = root.join("recipes");
    let built = build(r, &base)?;
    write(&built, r, &base, Some(&root.join("scenes")))
}

fn checked(root: &Path) {
    let diagnostics = Project::open(root).unwrap().check();
    let lines: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
    assert!(lines.is_empty(), "{}", lines.join("\n"));
}

fn scene_of(root: &Path, set: &str) -> SceneFile {
    let text = std::fs::read_to_string(root.join(format!("scenes/{set}.scene.toml"))).unwrap();
    toml::from_str(&text).unwrap()
}

fn matches_golden(set: &str, camera: Option<&str>) -> PathBuf {
    let (root, mut r) = project(set, &format!("format1-{set}"), "", &[]);
    r.output.scene_camera = camera.map(str::to_string);
    assert_eq!(validate(&r), []);
    emit(&root, &r).unwrap();
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scene/format1");
    for name in [format!("{set}.scene.toml"), format!("{set}.materials.toml")] {
        let made = std::fs::read_to_string(root.join("scenes").join(&name)).unwrap();
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all(&here).unwrap();
            std::fs::write(here.join(&name), &made).unwrap();
        }
        let want = std::fs::read_to_string(here.join(&name)).unwrap();
        assert!(
            made == want,
            "{name} differs from its format 1 golden; UPDATE_GOLDEN=1 regenerates it\n{made}"
        );
        assert!(made.starts_with("format = 1\n"), "{made}");
    }
    checked(&root);
    root
}

#[test]
fn the_table_writes_its_format_1_golden_and_pfx_scene_finds_nothing_wrong() {
    let root = matches_golden("table", None);
    let scene = scene_of(&root, "table");
    assert_eq!(scene.format, Some(1));
    assert_eq!(scene.materials, ["scenes/table.materials.toml"]);
    assert!(scene.mesh.values().all(|m| m.file == "scenes/table.glb"));
    let picks: Vec<u32> = scene.object.iter().filter_map(|o| o.pick).collect();
    assert_eq!(picks, (1..=11).collect::<Vec<u32>>());
    let authoring = scene.authoring.unwrap();
    assert_eq!(authoring["recipe"].as_str(), Some("recipes/table.toml"));
    assert!(!authoring.contains_key("variant"));
    let ball = scene.object.iter().find(|o| o.name == "ball").unwrap();
    assert_eq!(
        ball.id,
        Some(Id::derive(b"recipes/table.toml\nobject\nball"))
    );
}

#[test]
fn the_toybox_writes_its_format_1_golden_and_pfx_scene_finds_nothing_wrong() {
    matches_golden("toybox", Some("high"));
}

#[test]
fn a_variant_is_named_in_the_authoring_table() {
    let (root, r) = project("toybox", "format1-variant", "", &[]);
    let bare = apply_variant(&r, "bare").unwrap();
    emit(&root, &bare).unwrap();
    let scene = scene_of(&root, "toybox-bare");
    let authoring = scene.authoring.unwrap();
    assert_eq!(authoring["recipe"].as_str(), Some("recipes/toybox.toml"));
    assert_eq!(authoring["variant"].as_str(), Some("bare"));
    checked(&root);
}

#[test]
fn format_1_writes_the_same_bytes_from_the_same_recipe() {
    let mut texts = Vec::new();
    for name in ["format1-twice-a", "format1-twice-b"] {
        let (root, r) = project("toybox", name, "", &[]);
        emit(&root, &r).unwrap();
        texts.push(
            ["toybox.scene.toml", "toybox.materials.toml"]
                .map(|file| std::fs::read(root.join("scenes").join(file)).unwrap()),
        );
    }
    assert!(texts[0] == texts[1]);
}

fn ids(scene: &SceneFile, object: &str) -> (Option<Id>, Option<Id>) {
    let entry = scene.object.iter().find(|o| o.name == object).unwrap();
    (entry.id, scene.mesh[object].id)
}

#[test]
fn an_object_with_its_own_id_keeps_its_ids_across_a_rename_and_the_rest_keep_theirs() {
    const OWN: &str = "name = \"ball\">name = \"ball\"\nid = \"7k2m9q4xzr\"";
    let (root, r) = project("table", "format1-ids-a", OWN, &[]);
    emit(&root, &r).unwrap();
    let first = scene_of(&root, "table");
    let (root, r) = project("table", "format1-ids-b", OWN, &[]);
    emit(&root, &r).unwrap();
    assert_eq!(scene_of(&root, "table"), first);
    let (root, r) = project(
        "table",
        "format1-ids-renamed",
        &format!("{OWN};\"ball\">\"orb\""),
        &[],
    );
    emit(&root, &r).unwrap();
    let renamed = scene_of(&root, "table");
    checked(&root);
    let ball = ids(&first, "ball");
    assert!(ball.0.is_some() && ball.1.is_some() && ball.0 != ball.1);
    assert_eq!(ids(&renamed, "orb"), ball);
    assert_eq!(ball.0, Some(Id::parse("7k2m9q4xzr").unwrap()));
    for other in ["top", "wall", "crate", "tray"] {
        assert_eq!(ids(&renamed, other), ids(&first, other), "{other}");
    }
    let (root, r) = project("table", "format1-ids-plain", "\"ball\">\"orb\"", &[]);
    emit(&root, &r).unwrap();
    assert_ne!(ids(&scene_of(&root, "table"), "orb"), ball);
}

#[test]
fn cards_text_and_content_take_ids_and_paths_from_the_root() {
    let (root, r) = project(
        "cards",
        "format1-cards",
        &format!("{TURN};{NOTE};space = \"overlay\">space = \"lit\""),
        &["badge.png"],
    );
    let written = emit(&root, &r).unwrap();
    assert!(written.contains(&root.join("scenes/JetBrainsMonoNerdFont-Bold.ttf")));
    let scene = scene_of(&root, "cards");
    assert_eq!(scene.content["badge"].image, "recipes/badge.png");
    assert_eq!(
        scene.text["name_plate"].font,
        "scenes/JetBrainsMonoNerdFont-Bold.ttf"
    );
    assert!(scene.content.values().all(|c| c.id.is_some()));
    assert!(scene.text.values().all(|t| t.id.is_some()));
    checked(&root);
}

#[test]
fn a_path_outside_the_project_root_is_refused_naming_its_key() {
    let (root, mut r) = project("cards", "format1-outside", TURN, &[]);
    r.base = Some(golden("cards"));
    let built = build(&r, &golden("cards")).unwrap();
    let error = write(&built, &r, &golden("cards"), Some(&root.join("scenes"))).unwrap_err();
    assert_eq!(error.path, "object[badge].image");
    assert!(error.text.contains("badge.png"), "{}", error.text);
    assert!(
        error.text.contains("outside the project root"),
        "{}",
        error.text
    );
    let (root, mut r) = project("table", "format1-outside-glb", "", &[]);
    std::fs::create_dir_all(root.join("recipes/scenes")).unwrap();
    std::fs::rename(
        root.join("project.toml"),
        root.join("recipes/scenes/project.toml"),
    )
    .unwrap();
    r.output.scene = Some("scenes/table.scene.toml".into());
    r.output.glb = Some("table.glb".into());
    let base = root.join("recipes");
    let built = build(&r, &base).unwrap();
    let error = write(&built, &r, &base, None).unwrap_err();
    assert_eq!(error.path, "output.glb");
    assert!(error.text.contains("table.glb"), "{}", error.text);
    assert!(!root.join("recipes/table.glb").exists());
}

#[test]
fn object_ids_gives_the_ids_the_writer_derives_and_skips_what_has_one() {
    let (root, r) = project("table", "format1-object-ids", "", &[]);
    let base = root.join("recipes");
    let out = root.join("scenes");
    let given = object_ids(&r, None, &base, Some(&out)).unwrap();
    assert_eq!(given.len(), r.objects.len());
    emit(&root, &r).unwrap();
    let scene = scene_of(&root, "table");
    for (name, id) in &given {
        let entry = scene.object.iter().find(|o| &o.name == name).unwrap();
        assert_eq!(
            entry.id.map(|id| id.to_string()),
            Some(id.clone()),
            "{name}"
        );
    }
    const OWN: &str = "name = \"ball\">name = \"ball\"\nid = \"7k2m9q4xzr\"";
    let (root, r) = project("table", "format1-object-ids-own", OWN, &[]);
    let rest = object_ids(&r, None, &root.join("recipes"), Some(&root.join("scenes"))).unwrap();
    assert!(rest.iter().all(|(name, _)| name != "ball"));
    assert_eq!(rest.len(), given.len() - 1);
    let mut plain = r.clone();
    plain.output.scene = None;
    assert!(
        object_ids(&plain, None, &root.join("recipes"), None)
            .unwrap()
            .is_empty()
    );
}
