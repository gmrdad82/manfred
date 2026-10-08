mod common;

use common::cube;
use manfred_build::{Call, Generated, Piece, Registry, build_with};
use manfred_recipe::parse;
use serde_json::{Map, Value, json};
use std::path::Path;

fn facts(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

fn sheet(call: &Call) -> Result<Generated, String> {
    Ok(Generated {
        pieces: vec![cube(&call.object.name, "fake")],
        facts: facts(json!({
            "materials": ["fake"],
            "sheet": {"end": 0.5, "plan": [[0.0, 1.0]]},
            "sun": {"extra": 1},
        })),
        ..Generated::default()
    })
}

fn edges(_: &Call) -> Result<Generated, String> {
    Ok(Generated {
        pieces: vec![cube("rim", "fake")],
        facts: facts(json!({"edges": {"rims": [[0.0, 0.1]]}, "sheet": {"end": 0.75}})),
        ..Generated::default()
    })
}

fn plain(call: &Call) -> Result<Vec<Piece>, String> {
    Ok(vec![cube(&call.object.name, "fake")])
}

fn broken(_: &Call) -> Result<Generated, String> {
    Err("the sheet tore".into())
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.register_with_facts("test/sheet", sheet);
    registry.register_with_facts("test/edges", edges);
    registry.register_with_facts("test/broken", broken);
    registry.register("test/plain", plain);
    registry
}

const HEAD: &str = "manfred = 1\nname = \"t\"\n\n[output]\nglb = \"t.glb\"\nsidecar = \"t.json\"\nsidecar_dialect = \"facts\"\n\n[sun]\nmodel = \"authored\"\ntoward = [0.0, 1.0, 0.0]\nradius_deg = 2.5\nirradiance = 3.0\n";

fn object(name: &str, generator: &str) -> String {
    format!("\n[[object]]\nname = \"{name}\"\nkind = \"generator\"\ngenerator = \"{generator}\"\n")
}

fn sidecar(text: &str) -> Value {
    let recipe = parse(&format!("{HEAD}{text}")).unwrap();
    let built = build_with(&recipe, Path::new("."), &registry()).unwrap();
    serde_json::from_str(&built.sidecar().unwrap()).unwrap()
}

#[test]
fn a_generators_facts_land_in_the_sidecar_beside_the_dialects_keys() {
    let written = sidecar(&object("scroll", "test/sheet"));
    assert_eq!(written["materials"], json!(["fake"]));
    assert_eq!(written["sheet"], json!({"end": 0.5, "plan": [[0.0, 1.0]]}));
    assert_eq!(
        written["sun"],
        json!({"toward": [0.0, 1.0, 0.0], "radius_deg": 2.5, "color": [1.0, 1.0, 1.0], "irradiance": 3.0, "extra": 1})
    );
}

#[test]
fn facts_of_several_generators_merge_deeply_in_file_order() {
    let written = sidecar(&format!(
        "{}{}",
        object("scroll", "test/sheet"),
        object("rims", "test/edges")
    ));
    assert_eq!(written["sheet"], json!({"end": 0.75, "plan": [[0.0, 1.0]]}));
    assert_eq!(written["edges"], json!({"rims": [[0.0, 0.1]]}));
}

#[test]
fn the_recipes_sidecar_table_wins_over_a_generators_facts() {
    let written = sidecar(&format!(
        "{}\n[sidecar.sheet]\nend = 1.25\n",
        object("scroll", "test/sheet")
    ));
    assert_eq!(written["sheet"], json!({"end": 1.25, "plan": [[0.0, 1.0]]}));
}

#[test]
fn plain_generators_add_no_facts_and_both_kinds_are_registered() {
    let written = sidecar(&object("box", "test/plain"));
    assert!(written.get("sheet").is_none());
    assert!(written.get("materials").is_none());
    let registry = registry();
    assert!(registry.get("test/plain").is_some());
    assert!(registry.get("test/sheet").is_none());
    assert!(registry.get_with_facts("test/sheet").is_some());
    assert!(registry.contains("test/sheet") && registry.contains("test/plain"));
    let names: Vec<&str> = registry.names().collect();
    assert_eq!(
        names,
        ["test/broken", "test/edges", "test/plain", "test/sheet"]
    );
}

#[test]
fn a_facts_generators_error_names_the_object_and_the_generator() {
    let recipe = parse(&format!("{HEAD}{}", object("scroll", "test/broken"))).unwrap();
    let text = build_with(&recipe, Path::new("."), &registry())
        .unwrap_err()
        .to_string();
    assert_eq!(
        text,
        "object[scroll].generator: test/broken: the sheet tore"
    );
}
