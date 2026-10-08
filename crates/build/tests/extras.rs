mod common;

use manfred_build::{Built, build};
use manfred_glb::{NodeSummary, summary};
use manfred_recipe::parse;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn golden() -> PathBuf {
    common::golden("toybox")
}

fn built(text: &str) -> Built {
    let recipe = parse(&format!("manfred = 1\nname = \"t\"\n{text}")).unwrap();
    build(&recipe, Path::new(".")).unwrap()
}

fn objects(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| {
            format!("[[object]]\nname = \"{name}\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n\n")
        })
        .collect()
}

fn nodes(b: &Built) -> Vec<NodeSummary> {
    summary(&b.glb()).unwrap().nodes
}

fn manfred(node: &NodeSummary) -> Option<serde_json::Map<String, Value>> {
    node.extras.as_ref()?.get("manfred")?.as_object().cloned()
}

const KNOB: &str = "[[mover]]\ngroup = \"knob\"\npivot = [0.1, 0.0, 0.2]\naxis = [0.0, 1.0, 0.0]\ntravel = 0.003\nclip = [0.0, 1.0, 0.0, 0.5]\nclip2 = [0.0, -1.0, 0.0, 0.25]\n";

#[test]
fn the_first_part_of_a_mover_is_its_root_and_the_rest_carry_the_group() {
    let text = format!(
        "{}[[object]]\nname = \"extra\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n\n{KNOB}parts = [\"extra\"]\n",
        objects(&["plate", "mv_knob__cap", "mv_knob__ring"]),
    );
    let b = built(&text);
    let loaded = nodes(&b);
    let by = |name: &str| loaded.iter().find(|n| n.name == name).unwrap();
    assert!(by("plate").extras.is_none());
    let root = manfred(by("mv_knob__cap")).unwrap();
    assert_eq!(root["group"], "knob");
    assert_eq!(root["pivot"], json!([0.1, 0.0, 0.2]));
    assert_eq!(root["axis"], json!([0.0, 1.0, 0.0]));
    assert_eq!(root["travel"], json!(0.003));
    assert_eq!(root["clip"], json!([0.0, 1.0, 0.0, 0.5]));
    assert_eq!(root["clip2"], json!([0.0, -1.0, 0.0, 0.25]));
    for part in ["mv_knob__ring", "extra"] {
        assert_eq!(
            manfred(by(part)).unwrap(),
            json!({"group": "knob"}).as_object().unwrap().clone(),
            "{part}"
        );
    }
}

#[test]
fn a_hidden_first_part_leaves_the_root_to_the_next() {
    let text = format!(
        "[[object]]\nname = \"mv_knob__a\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\nhidden = true\n\n{}{KNOB}",
        objects(&["mv_knob__b"])
    );
    let loaded = nodes(&built(&text));
    assert_eq!(loaded.len(), 1);
    assert_eq!(
        manfred(&loaded[0]).unwrap()["pivot"],
        json!([0.1, 0.0, 0.2])
    );
}

#[test]
fn a_shadow_flag_that_is_not_the_default_travels_and_nothing_else_does() {
    let text = "[[object]]\nname = \"sheet\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\nshadow = \"only\"\n\n[[object]]\nname = \"ghost\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\nshadow = \"none\"\ntwo_sided = true\nkeep_transform = true\n\n[[object]]\nname = \"plain\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\ntwo_sided = true\n";
    let loaded = nodes(&built(text));
    let by = |name: &str| loaded.iter().find(|n| n.name == name).unwrap();
    assert_eq!(
        by("sheet").extras,
        Some(json!({"manfred": {"shadow": "only"}}))
    );
    assert_eq!(
        by("ghost").extras,
        Some(json!({"manfred": {"shadow": "none"}}))
    );
    assert!(by("plain").extras.is_none());
}

#[test]
fn a_recipe_with_nothing_to_say_writes_the_bytes_it_always_did() {
    let b = built(&objects(&["a", "b"]));
    assert!(b.extras.iter().all(Option::is_none));
    assert_eq!(
        b.glb(),
        manfred_glb::write(&b.nodes(), b.order, manfred_build::GENERATOR)
    );
}

#[test]
fn the_toyboxs_roots_carry_the_sidecars_numbers_and_every_part_its_group() {
    let recipe = manfred_recipe::load(&golden().join("toybox.toml"))
        .unwrap()
        .recipe;
    let b = build(&recipe, &golden()).unwrap();
    let sidecar: Value = serde_json::from_str(&b.sidecar().unwrap()).unwrap();
    let all = sidecar["movers"].as_object().unwrap();
    let loaded = nodes(&b);
    let mut roots = 0;
    let mut parts = 0;
    for node in &loaded {
        let Some(mover) = group(&recipe, &node.name) else {
            let shadow = node.extras.as_ref().and_then(|_| manfred(node));
            match node.name.as_str() {
                "shade" => assert_eq!(shadow.unwrap()["shadow"], "only"),
                _ => assert!(node.extras.is_none(), "{}", node.name),
            }
            continue;
        };
        let manfred = manfred(node).unwrap();
        assert_eq!(manfred["group"], mover, "{}", node.name);
        parts += 1;
        if manfred.contains_key("pivot") {
            roots += 1;
            let want = &all[mover];
            for key in ["pivot", "axis", "travel", "clip", "clip2"] {
                assert_eq!(manfred.get(key), want.get(key), "{mover} {key}");
            }
        } else {
            assert_eq!(manfred.len(), 1);
        }
    }
    assert_eq!(roots, all.len());
    assert_eq!(roots, 4);
    assert_eq!(parts, 7);
}

fn group<'a>(recipe: &'a manfred_recipe::Recipe, name: &str) -> Option<&'a str> {
    recipe
        .movers
        .iter()
        .find(|m| m.owns(name))
        .map(|m| m.group.as_str())
}
