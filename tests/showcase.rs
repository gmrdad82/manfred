use std::fs;
use std::path::{Path, PathBuf};

use manfred::build::{Registry, build_with, missing_materials};
use manfred::recipe::{load, validate};
use manfred_scene::{palette, pieces};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn recipes() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(root().join("showcase"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    found.sort();
    found
}

fn name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn the_showcase_folder_holds_the_scenes() {
    let names: Vec<String> = recipes().iter().map(|path| name(path)).collect();
    for scene in ["gallery.toml", "workshop.toml"] {
        assert!(
            names.iter().any(|found| found == scene),
            "{scene}: {names:?}"
        );
    }
}

#[test]
fn every_showcase_scene_checks_clean() {
    for path in recipes() {
        let loaded = load(&path).unwrap_or_else(|e| panic!("{}: {e:?}", name(&path)));
        let mut problems = validate(&loaded.recipe);
        problems.extend(missing_materials(
            &loaded.recipe,
            path.parent().unwrap(),
            &Registry::new(),
        ));
        assert!(problems.is_empty(), "{}: {problems:?}", name(&path));
    }
}

#[test]
fn every_showcase_scene_builds_and_dresses_every_piece() {
    for path in recipes() {
        let loaded = load(&path).unwrap();
        let folder = path.parent().unwrap();
        let built = build_with(&loaded.recipe, folder, &Registry::new())
            .unwrap_or_else(|e| panic!("{}: {e}", name(&path)));
        let recipe = built.recipe(&loaded.recipe);
        let cut = pieces(&built.parts);
        assert!(!cut.is_empty(), "{}", name(&path));
        palette(&recipe, &cut, std::iter::repeat(false))
            .unwrap_or_else(|e| panic!("{}: {e}", name(&path)));
    }
}

#[test]
fn every_showcase_scene_has_its_cameras() {
    let wanted = [
        ("workshop.toml", &["hero", "top", "detail", "orbit"][..]),
        ("gallery.toml", &["flat", "angle"][..]),
    ];
    for (file, cameras) in wanted {
        let loaded = load(&root().join("showcase").join(file)).unwrap();
        for camera in cameras {
            assert!(
                loaded.recipe.cameras.iter().any(|c| c.name == *camera),
                "{file}: no camera {camera}"
            );
        }
    }
}

#[test]
fn every_showcase_scene_uses_only_its_own_materials() {
    for path in recipes() {
        let loaded = load(&path).unwrap();
        let used: Vec<&str> = loaded
            .recipe
            .objects
            .iter()
            .flat_map(|o| o.material.iter().chain(&o.materials))
            .map(String::as_str)
            .collect();
        assert!(!used.is_empty(), "{}", name(&path));
        for material in used {
            assert!(
                loaded.recipe.materials.contains_key(material),
                "{}: {material} is not the scene's own",
                name(&path)
            );
        }
    }
}

#[test]
fn the_readme_commands_name_existing_flags() {
    let usage = fs::read_to_string(root().join("README.md")).unwrap();
    let usage = usage
        .split("## Commands")
        .nth(1)
        .and_then(|rest| rest.split("```").nth(1))
        .unwrap();
    let readme = fs::read_to_string(root().join("showcase/README.md")).unwrap();
    let mut fenced = false;
    let mut checked = 0;
    for line in readme.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced || !line.starts_with("manfred ") {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let command = format!("manfred {} ", words[1]);
        let mine: Vec<&str> = usage
            .lines()
            .filter(|known| known.trim_start().starts_with(&command))
            .collect();
        assert!(!mine.is_empty(), "no command: {line}");
        for word in words.iter().filter(|word| word.starts_with("--")) {
            assert!(
                mine.iter().any(|known| known.contains(word)),
                "{word} is not a flag of `{}`",
                words[1]
            );
        }
        for word in words.iter().filter(|word| word.ends_with(".toml")) {
            assert!(root().join(word).exists(), "{word} does not exist");
        }
        checked += 1;
    }
    assert!(checked >= 4, "{checked} commands checked");
}
