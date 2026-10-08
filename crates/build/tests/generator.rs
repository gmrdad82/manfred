mod common;

use common::{centre, cube};
use glam::DVec3;
use manfred_build::{Built, Call, Piece, Registry, build, build_with, seed};
use manfred_mesh::{Mesh, bounds};
use manfred_recipe::{Recipe, Shadow, parse};
use std::path::Path;

fn param(call: &Call, key: &str) -> String {
    call.params
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(&call.object.name)
        .to_string()
}

fn one(call: &Call) -> Result<Vec<Piece>, String> {
    Ok(vec![cube(&param(call, "name"), "fake")])
}

fn several(_: &Call) -> Result<Vec<Piece>, String> {
    Ok(vec![
        cube("body", "skin"),
        cube("wing_l", "feather"),
        cube("wing_r", "feather"),
    ])
}

fn seeded(call: &Call) -> Result<Vec<Piece>, String> {
    Ok(vec![cube(
        &format!("{}_{}", call.object.name, call.seed),
        "fake",
    )])
}

fn broken(_: &Call) -> Result<Vec<Piece>, String> {
    Err("the wings fell off".into())
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.register("test/one", one);
    registry.register("test/several", several);
    registry.register("test/seeded", seeded);
    registry.register("test/broken", broken);
    registry
}

fn recipe(text: &str) -> Recipe {
    parse(&format!("manfred = 1\nname = \"t\"\nseed = 7\n{text}")).unwrap()
}

fn built(text: &str) -> Built {
    build_with(&recipe(text), Path::new("."), &registry()).unwrap()
}

fn error(text: &str) -> String {
    build_with(&recipe(text), Path::new("."), &registry())
        .unwrap_err()
        .to_string()
}

fn materials(m: &Mesh) -> Vec<&str> {
    m.slots.iter().map(|s| s.material.as_str()).collect()
}

const ONE: &str = "[[object]]\nname = \"thing\"\nkind = \"generator\"\ngenerator = \"test/one\"\nat = [1.0, 2.0, 3.0]\nshadow = \"only\"\ntwo_sided = true\n";

const SEVERAL: &str = "[[object]]\nname = \"bird\"\nkind = \"generator\"\ngenerator = \"test/several\"\nat = [0.0, 0.0, -4.0]\nscale = [2.0, 2.0, 2.0]\nshadow = \"none\"\n";

#[test]
fn one_piece_becomes_one_part_where_the_object_sits() {
    let b = built(ONE);
    assert_eq!(b.parts.len(), 1);
    let part = &b.parts[0];
    assert_eq!(part.node.name, "thing");
    assert_eq!(part.object, 0);
    assert_eq!(part.shadow, Shadow::Only);
    assert!(part.two_sided);
    assert_eq!(materials(&part.node.mesh), ["fake"]);
    assert!((centre(&part.node.mesh) - DVec3::new(1.0, 2.0, 3.0)).length() < 1e-6);
    assert!(part.node.mesh.uvs.is_none());
}

#[test]
fn a_piece_is_named_as_the_generator_says() {
    let b = built(
        "[[object]]\nname = \"thing\"\nkind = \"generator\"\ngenerator = \"test/one\"\nparams = { name = \"crate\" }\n",
    );
    assert_eq!(b.parts[0].node.name, "crate");
}

#[test]
fn several_pieces_share_the_transform_shadow_and_object() {
    let b = built(SEVERAL);
    let names: Vec<&str> = b.parts.iter().map(|p| p.node.name.as_str()).collect();
    assert_eq!(names, ["body", "wing_l", "wing_r"]);
    for part in &b.parts {
        assert_eq!(part.object, 0);
        assert_eq!(part.shadow, Shadow::None);
        assert!(!part.two_sided);
        let [lo, hi] = bounds(&part.node.mesh).unwrap();
        assert!((hi[0] - lo[0] - 2.0).abs() < 1e-6, "{}", part.node.name);
        assert!((centre(&part.node.mesh) - DVec3::new(0.0, 0.0, -4.0)).length() < 1e-6);
    }
    assert_eq!(materials(&b.parts[0].node.mesh), ["skin"]);
    assert_eq!(materials(&b.parts[1].node.mesh), ["feather"]);
}

#[test]
fn keep_transform_leaves_the_matrix_on_every_piece() {
    let b = built(&format!("{SEVERAL}keep_transform = true\n"));
    for part in &b.parts {
        assert!(part.node.keep_transform);
        assert!((centre(&part.node.mesh)).length() < 1e-6);
        assert_eq!(part.node.transform.w_axis.z, -4.0);
    }
}

#[test]
fn material_overrides_every_piece() {
    let b = built(&format!("{SEVERAL}material = \"gold\"\n"));
    for part in &b.parts {
        assert_eq!(materials(&part.node.mesh), ["gold"]);
    }
    let b = built(&format!("{SEVERAL}materials = [\"silver\"]\n"));
    for part in &b.parts {
        assert_eq!(materials(&part.node.mesh), ["silver"]);
    }
}

#[test]
fn seeds_differ_per_object_and_repeat_per_build() {
    let text = "[[object]]\nname = \"a\"\nkind = \"generator\"\ngenerator = \"test/seeded\"\n\n[[object]]\nname = \"b\"\nkind = \"generator\"\ngenerator = \"test/seeded\"\n";
    let names =
        |b: &Built| -> Vec<String> { b.parts.iter().map(|p| p.node.name.clone()).collect() };
    let first = built(text);
    assert_eq!(
        names(&first),
        [format!("a_{}", seed(7, 0)), format!("b_{}", seed(7, 1))]
    );
    assert_ne!(seed(7, 0), seed(7, 1));
    assert_ne!(seed(7, 0), seed(8, 0));
    assert_eq!(names(&first), names(&built(text)));
}

#[test]
fn an_unknown_generator_names_the_object_and_the_registered_names() {
    let text = "[[object]]\nname = \"scroll\"\nkind = \"generator\"\ngenerator = \"app/scroll\"\n";
    assert_eq!(
        error(text),
        "object[scroll].generator: no generator is named `app/scroll`, expected one of: test/broken, test/one, test/seeded, test/several"
    );
    let empty = build(&recipe(text), Path::new("."))
        .unwrap_err()
        .to_string();
    assert!(
        empty.starts_with(
            "object[scroll].generator: `app/scroll` is built by the app's own build binary"
        ),
        "{empty}"
    );
}

#[test]
fn a_generators_own_error_is_wrapped_with_the_object() {
    let text = "[[object]]\nname = \"bird\"\nkind = \"generator\"\ngenerator = \"test/broken\"\n";
    assert_eq!(
        error(text),
        "object[bird].generator: test/broken: the wings fell off"
    );
}

#[test]
fn a_piece_named_as_another_object_is_refused() {
    let text = format!(
        "{SEVERAL}\n[[object]]\nname = \"wing_l\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n"
    );
    assert_eq!(
        error(&text),
        "object[bird].generator: the piece `wing_l` has the name of the object `wing_l`"
    );
}

#[test]
fn a_piece_named_as_another_piece_is_refused() {
    let text = format!(
        "{SEVERAL}\n[[object]]\nname = \"second\"\nkind = \"generator\"\ngenerator = \"test/several\"\n"
    );
    assert_eq!(
        error(&text),
        "object[second].generator: the piece `body` has the name of a piece of the object `bird`"
    );
}

#[test]
fn generator_objects_build_beside_shapes_in_file_order() {
    let text = format!(
        "[[object]]\nname = \"floor\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n\n{SEVERAL}\n[[object]]\nname = \"lid\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n"
    );
    let b = built(&text);
    let parts: Vec<(&str, usize)> = b
        .parts
        .iter()
        .map(|p| (p.node.name.as_str(), p.object))
        .collect();
    assert_eq!(
        parts,
        [
            ("floor", 0),
            ("body", 1),
            ("wing_l", 1),
            ("wing_r", 1),
            ("lid", 2)
        ]
    );
    assert_eq!(b.glb(), built(&text).glb());
}
