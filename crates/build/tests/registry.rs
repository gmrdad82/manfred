mod common;

use common::cube;
use manfred_build::{
    Call, FACTS, NO_SIDECAR, Piece, Registry, SceneFacts, build_with, facts_sidecar,
};
use manfred_recipe::parse;
use serde_json::{Value, json};
use std::path::Path;

fn tile(call: &Call) -> Result<Vec<Piece>, String> {
    Ok(vec![cube(&call.object.name, "tile")])
}

fn count(facts: &SceneFacts) -> Result<Value, String> {
    if facts.cameras.is_empty() {
        return Err("the count dialect wants a camera".into());
    }
    Ok(json!({"cameras": facts.cameras.len(), "movers": facts.movers.len(), "sun": "writer"}))
}

fn list(_: &SceneFacts) -> Result<Value, String> {
    Ok(json!([1, 2]))
}

const CATALOGUE: &str = "[materials.tile]\nbase = [0.2, 0.4, 0.6]\nroughness = 0.5\n\n[materials.spare]\nbase = [1.0, 1.0, 1.0]\n";

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.register("test/tile", tile);
    registry.register_sidecar("count", count);
    registry.register_sidecar("list", list);
    registry.register_sidecar(NO_SIDECAR, list);
    registry.register_materials_toml(CATALOGUE).unwrap();
    registry
}

fn recipe(dialect: &str, rest: &str) -> String {
    format!(
        "manfred = 1\nname = \"t\"\n\n[output]\nsidecar = \"t.json\"\nsidecar_dialect = \"{dialect}\"\n\n[[object]]\nname = \"floor\"\nkind = \"generator\"\ngenerator = \"test/tile\"\n{rest}"
    )
}

const CAMERA: &str = "\n[[camera]]\nname = \"main\"\nposition = [0.0, 1.0, 2.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 30.0\n";

fn built(text: &str) -> Result<manfred_build::Built, manfred_build::Error> {
    build_with(&parse(text).unwrap(), Path::new("."), &registry())
}

#[test]
fn a_registered_writer_writes_the_sidecar_and_the_recipes_table_merges_over_it() {
    let text = format!(
        "{}{CAMERA}\n[sidecar]\nsun = \"recipe\"\nnote = 1\n",
        recipe("count", "")
    );
    let sidecar: Value = serde_json::from_str(&built(&text).unwrap().sidecar().unwrap()).unwrap();
    assert_eq!(
        sidecar,
        json!({"cameras": 1, "movers": 0, "sun": "recipe", "note": 1})
    );
}

#[test]
fn an_unknown_dialect_is_an_error_naming_the_registered_writers() {
    let error = built(&recipe("nope", CAMERA)).unwrap_err();
    assert_eq!(error.path, "output.sidecar_dialect");
    assert!(
        error
            .text
            .starts_with("no sidecar writer is named `nope`, expected \"none\" or one of: "),
        "{}",
        error.text
    );
    for name in ["count", FACTS, "list"] {
        assert!(error.text.contains(name), "{name}: {}", error.text);
    }
}

#[test]
fn a_writer_may_refuse_a_recipe_and_must_write_an_object() {
    let error = built(&recipe("count", "")).unwrap_err();
    assert_eq!(error.path, "output.sidecar_dialect");
    assert_eq!(error.text, "count: the count dialect wants a camera");
    let error = built(&recipe("list", "")).unwrap_err();
    assert_eq!(error.text, "list: a sidecar writer returns a JSON object");
}

#[test]
fn none_writes_no_sidecar_and_cannot_be_registered() {
    let text = "manfred = 1\nname = \"t\"\n\n[[object]]\nname = \"floor\"\nkind = \"generator\"\ngenerator = \"test/tile\"\n";
    assert_eq!(built(text).unwrap().sidecar(), None);
    assert!(registry().sidecar(NO_SIDECAR).is_none());
    assert!(!registry().sidecar_names().any(|name| name == NO_SIDECAR));
}

#[test]
fn the_facts_dialect_is_manfreds_own_and_always_registered() {
    let text = format!("{}{CAMERA}", recipe(FACTS, ""));
    let b = built(&text).unwrap();
    let sidecar: Value = serde_json::from_str(&b.sidecar().unwrap()).unwrap();
    assert_eq!(sidecar, facts_sidecar(&b.facts).unwrap());
    assert_eq!(sidecar["cameras"][0]["name"], json!("main"));
    assert!(Registry::new().sidecar(FACTS).is_some());
}

#[test]
fn the_apps_catalogue_names_the_materials_the_parts_use_and_wins_over_the_recipe() {
    let b = built(&recipe(FACTS, "")).unwrap();
    assert_eq!(b.catalogue.keys().collect::<Vec<_>>(), ["tile"]);
    assert!(b.notes.is_empty());
    let r = parse(&recipe(FACTS, "")).unwrap();
    let drawn = b.recipe(&r);
    assert_eq!(drawn.materials["tile"]["roughness"], json!(0.5));
    assert!(!drawn.materials.contains_key("spare"));

    let own = recipe(FACTS, "\n[materials.tile]\nbase = [0.9, 0.0, 0.0]\n");
    let b = built(&own).unwrap();
    assert_eq!(
        b.notes,
        ["materials.tile: the app's catalogue entry is used, the recipe's own is not"]
    );
    let drawn = b.recipe(&parse(&own).unwrap());
    assert_eq!(drawn.materials["tile"]["base"], json!([0.2, 0.4, 0.6]));
}

#[test]
fn without_a_catalogue_the_recipes_own_material_stands() {
    let own = recipe(FACTS, "\n[materials.tile]\nbase = [0.9, 0.0, 0.0]\n");
    let mut plain = Registry::new();
    plain.register("test/tile", tile);
    let r = parse(&own).unwrap();
    let b = build_with(&r, Path::new("."), &plain).unwrap();
    assert!(b.catalogue.is_empty());
    assert_eq!(
        b.recipe(&r).materials["tile"]["base"],
        json!([0.9, 0.0, 0.0])
    );
}

#[test]
fn a_catalogue_that_is_not_toml_is_an_error() {
    let mut registry = Registry::new();
    assert!(registry.register_materials_toml("[materials\n").is_err());
    assert!(registry.register_materials_toml("materials = 3\n").is_err());
    registry.register_materials([("one", json!({"base": [1.0, 1.0, 1.0]}))]);
    assert_eq!(registry.material_names().collect::<Vec<_>>(), ["one"]);
}

const BAKED: &str = "manfred = 1\nname = \"t\"\n\n[output]\nglb = \"t.glb\"\nbake = \"bake.toml\"\n\n[[object]]\nname = \"floor\"\nkind = \"generator\"\ngenerator = \"test/tile\"\n\n[bake]\nproduct = \"sample\"\nanchors = [12.0]\nsamples = 4\n[[bake.volume]]\nname = \"all\"\nmin = [0.0, 0.0, 0.0]\nmax = [1.0, 1.0, 1.0]\nspacing = 0.5\n";

#[test]
fn a_bake_writes_the_apps_catalogue_as_its_library_and_refuses_the_recipes_own_materials() {
    let dir = common::scratch("registry-bake");
    let r = parse(BAKED).unwrap();
    let b = build_with(&r, &dir, &registry()).unwrap();
    assert_eq!(b.catalogue.keys().collect::<Vec<_>>(), ["tile"]);
    let files = manfred_build::write(&b, &r, &dir, None).unwrap();
    assert_eq!(
        files,
        [
            dir.join("t.glb"),
            dir.join("bake-materials.toml"),
            dir.join("bake.toml")
        ]
    );
    let bake = std::fs::read_to_string(dir.join("bake.toml")).unwrap();
    assert!(
        bake.starts_with(
            "scene = \"t.glb\"\napp = \"sample\"\nmaterials = \"bake-materials.toml\"\n"
        ),
        "{bake}"
    );
    let library = std::fs::read_to_string(dir.join("bake-materials.toml")).unwrap();
    assert!(library.contains("[materials.tile]"), "{library}");
    assert!(!library.contains("spare"), "{library}");
    let own = parse(&format!(
        "{BAKED}\n[materials.tile]\nbase = [0.9, 0.0, 0.0]\n"
    ))
    .unwrap();
    let b = build_with(&own, &dir, &registry()).unwrap();
    let error = manfred_build::write(&b, &own, &dir, None).unwrap_err();
    assert_eq!(error.path, "materials.tile");
}
