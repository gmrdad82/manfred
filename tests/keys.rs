use std::fs;
use std::path::{Path, PathBuf};

use manfred::recipe::{load, validate};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn walk(folder: &Path, found: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "errors" | "target" | "tmp" | ".git") {
                walk(&path, found);
            }
        } else if path.extension().is_some_and(|ext| ext == "toml")
            && fs::read_to_string(&path)
                .unwrap()
                .lines()
                .any(|line| line == "manfred = 1")
        {
            found.push(path);
        }
    }
}

#[test]
fn every_recipe_in_the_tree_reads_and_checks_clean_in_the_scene_files_spelling() {
    let mut found = Vec::new();
    for folder in ["showcase", "tests", "crates", "examples"] {
        walk(&root().join(folder), &mut found);
    }
    assert!(found.len() >= 10, "{found:?}");
    for path in found {
        let shown = path.strip_prefix(root()).unwrap().display().to_string();
        let loaded = load(&path).unwrap_or_else(|e| panic!("{shown}: {e}"));
        let problems = validate(&loaded.recipe);
        assert!(problems.is_empty(), "{shown}: {problems:?}");
    }
}
