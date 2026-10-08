mod common;

use common::cube;
use manfred_build::{Built, Call, Generated, Registry, build_with};
use manfred_recipe::{Recipe, parse};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

fn offer(call: &Call, glow: Value) -> Result<Generated, String> {
    let mut materials = BTreeMap::new();
    materials.insert("glow".to_string(), glow);
    if let Some(Value::String(extra)) = call.params.get("extra") {
        materials.insert(extra.clone(), json!({ "base": [0.1, 0.2, 0.3] }));
    }
    Ok(Generated {
        pieces: vec![cube(&call.object.name, "glow")],
        materials,
        ..Generated::default()
    })
}

fn red(call: &Call) -> Result<Generated, String> {
    offer(call, json!({ "base": [1.0, 0.0, 0.0], "roughness": 0.5 }))
}

fn blue(call: &Call) -> Result<Generated, String> {
    offer(call, json!({ "base": [0.0, 0.0, 1.0], "roughness": 0.5 }))
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.register_with_facts("test/red", red);
    registry.register_with_facts("test/red_again", red);
    registry.register_with_facts("test/blue", blue);
    registry
}

fn object(name: &str, generator: &str, params: &str) -> String {
    format!(
        "[[object]]\nname = \"{name}\"\nkind = \"generator\"\ngenerator = \"{generator}\"\nparams = {{ {params} }}\n"
    )
}

fn recipe(text: &str) -> Recipe {
    parse(&format!("manfred = 1\nname = \"t\"\n{text}")).unwrap()
}

fn built(recipe: &Recipe) -> Result<Built, String> {
    build_with(recipe, Path::new("."), &registry()).map_err(|e| e.to_string())
}

#[test]
fn a_generators_material_joins_the_recipes_materials() {
    let r = recipe(&object("lamp", "test/red", "extra = \"rim\""));
    assert!(r.materials.is_empty());
    let b = built(&r).unwrap();
    assert_eq!(b.materials.len(), 2);
    let merged = b.recipe(&r);
    assert_eq!(merged.materials["glow"]["base"], json!([1.0, 0.0, 0.0]));
    assert_eq!(merged.materials["rim"]["base"], json!([0.1, 0.2, 0.3]));
    assert!(r.materials.is_empty());
    assert!(b.notes.is_empty());
}

#[test]
fn a_recipe_entry_of_the_same_name_wins_and_the_build_says_so() {
    let r = recipe(&format!(
        "{}\n[materials.glow]\nbase = [0.5, 0.5, 0.5]\n",
        object("lamp", "test/red", "")
    ));
    let b = built(&r).unwrap();
    assert!(b.materials.is_empty());
    assert_eq!(
        b.notes,
        ["materials.glow: the recipe's entry replaces the one the object `lamp` returned"]
    );
    assert_eq!(
        b.recipe(&r).materials["glow"]["base"],
        json!([0.5, 0.5, 0.5])
    );
}

#[test]
fn a_recipe_entry_equal_to_the_generators_is_not_a_replacement() {
    let r = recipe(&format!(
        "{}\n[materials.glow]\nbase = [1.0, 0.0, 0.0]\nroughness = 0.5\n",
        object("lamp", "test/red", "")
    ));
    let b = built(&r).unwrap();
    assert!(b.notes.is_empty());
    assert_eq!(b.recipe(&r).materials, r.materials);
}

#[test]
fn two_generators_returning_one_name_with_one_value_agree() {
    let r = recipe(&format!(
        "{}{}",
        object("a", "test/red", ""),
        object("b", "test/red_again", "")
    ));
    let b = built(&r).unwrap();
    assert_eq!(b.materials.len(), 1);
    assert!(b.notes.is_empty());
}

#[test]
fn two_generators_returning_one_name_with_two_values_clash_naming_both() {
    let r = recipe(&format!(
        "{}{}",
        object("a", "test/red", ""),
        object("b", "test/blue", "")
    ));
    assert_eq!(
        built(&r).unwrap_err(),
        "object[b].generator: the material `glow` differs from the one the object `a` returned"
    );
}

#[test]
fn the_pieces_keep_their_material_names_in_the_glb() {
    let r = recipe(&object("lamp", "test/red", ""));
    let b = built(&r).unwrap();
    let summary = manfred_glb::summary(&b.glb()).unwrap();
    assert_eq!(summary.materials, ["glow"]);
}
