mod common;

use common::without_extras;
use manfred_build::build;
use manfred_recipe::parse;
use manfred_runtime::{Scene, load, load_dir};
use serde_json::json;
use std::path::{Path, PathBuf};

fn golden() -> PathBuf {
    common::golden("toybox")
}

fn found(scene: &Scene) -> Vec<(String, Option<String>)> {
    scene
        .objects
        .iter()
        .enumerate()
        .map(|(index, name)| {
            (
                name.clone(),
                scene.mover_of(index as u32).map(|m| m.name.clone()),
            )
        })
        .collect()
}

const RECIPE: &str = "manfred = 1\nname = \"t\"\n\
[[object]]\nname = \"plate\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n\n\
[[object]]\nname = \"mv_knob__cap\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n\n\
[[object]]\nname = \"extra\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n\n\
[[mover]]\ngroup = \"knob\"\npivot = [0.1, 0.0, 0.2]\naxis = [0.0, 1.0, 0.0]\ntravel = 0.003\nparts = [\"extra\"]\n";

fn built() -> Vec<u8> {
    let recipe = parse(RECIPE).unwrap();
    build(&recipe, Path::new(".")).unwrap().glb()
}

fn index(scene: &Scene, name: &str) -> u32 {
    scene.objects.iter().position(|o| o == name).unwrap() as u32
}

#[test]
fn a_part_the_recipe_lists_without_the_mv_prefix_finds_its_mover() {
    let scene = load(&built(), "").unwrap();
    let extra = index(&scene, "extra");
    assert_eq!(scene.group_of(extra), Some("knob"));
    assert_eq!(scene.mover_of(extra).unwrap().name, "knob");
    let cap = index(&scene, "mv_knob__cap");
    assert_eq!(scene.mover_of(cap).unwrap().name, "knob");
    let plate = index(&scene, "plate");
    assert_eq!(scene.group_of(plate), None);
    assert!(scene.mover_of(plate).is_none());
}

#[test]
fn without_extras_the_listed_part_is_not_found_and_the_named_ones_are() {
    let sidecar = json!({"movers": {"knob": {
        "pivot": [0.1, 0.0, 0.2], "axis": [0.0, 1.0, 0.0], "travel": 0.003
    }}});
    let scene = load(&without_extras(&built()), &sidecar.to_string()).unwrap();
    assert!(scene.groups.iter().all(Option::is_none));
    assert!(scene.mover_of(index(&scene, "extra")).is_none());
    assert_eq!(
        scene.mover_of(index(&scene, "mv_knob__cap")).unwrap().name,
        "knob"
    );
    assert!(scene.mover_of(index(&scene, "plate")).is_none());
}

#[test]
fn the_toyboxs_movers_are_found_by_name_without_extras_and_by_group_with_them() {
    let dir = golden();
    let glb = std::fs::read(dir.join("toybox.glb")).unwrap();
    let text = std::fs::read_to_string(dir.join("toybox.json")).unwrap();
    let before = load(&without_extras(&glb), &text).unwrap();
    let after = load_dir(&dir, "toybox").unwrap();
    assert_eq!(before.objects, after.objects);
    let (found_before, found_after) = (found(&before), found(&after));
    for ((name, was), (_, is)) in found_before.iter().zip(&found_after) {
        if name.starts_with("mv_") {
            assert!(was.is_some(), "{name}");
            assert_eq!(was, is, "{name}");
        } else if name.starts_with("drawer_") {
            assert_eq!(
                (was.as_deref(), is.as_deref()),
                (None, Some("drawer")),
                "{name}"
            );
        } else {
            assert_eq!((was, is), (&None, &None), "{name}");
        }
    }
    assert_eq!(found_after.iter().filter(|(_, m)| m.is_some()).count(), 7);
}
