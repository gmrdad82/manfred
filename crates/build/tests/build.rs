mod common;

use common::centre;
use glam::{DMat4, DVec3};
use manfred_build::{Built, build, rotation, world};
use manfred_glb::{Node, Order, summary};
use manfred_mesh::{Mesh, bounds};
use manfred_recipe::{Recipe, apply_variant, load, parse};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../recipe/tests/fixtures")
}

fn recipe(text: &str) -> Recipe {
    parse(&format!("manfred = 1\nname = \"t\"\n{text}")).unwrap()
}

fn built(text: &str) -> Built {
    build(&recipe(text), Path::new(".")).unwrap()
}

fn error(text: &str) -> String {
    build(&recipe(text), Path::new("."))
        .unwrap_err()
        .to_string()
}

fn node<'a>(b: &'a Built, name: &str) -> &'a Node {
    &b.parts
        .iter()
        .find(|p| p.node.name == name)
        .unwrap_or_else(|| panic!("no part {name}"))
        .node
}

fn near(a: DVec3, b: DVec3) -> bool {
    (a - b).abs().max_element() < 1e-6
}

fn names(b: &Built) -> Vec<String> {
    let bytes = manfred_glb::write(&b.nodes(), b.order, "t");
    summary(&bytes)
        .unwrap()
        .nodes
        .into_iter()
        .map(|n| n.name)
        .collect()
}

const SLAB: &str = r#"
[[object]]
name = "slab"
kind = "outline"
front = 0.0
back = 0.02
outline = { rect = [-0.1, -0.05, 0.1, 0.05], radius = 0.01 }
profile = { kind = "round", width = 0.002, segments = 2 }
"#;

const LEFT: &str = r#"
[[object]]
name = "left"
kind = "pocket"
into = "slab"
outline = { rect = [-0.08, -0.03, -0.02, 0.03], radius = 0.005 }
depth = 0.01
"#;

const RIGHT: &str = r#"
[[object]]
name = "right"
kind = "pocket"
into = "slab"
outline = { circle = [0.05, 0.0, 0.02], segments = 32 }
depth = 0.008
"#;

const NESTED: &str = r#"
[[object]]
name = "base"
kind = "box"
size = [0.2, 0.1, 0.2]
at = [1.0, 0.0, 0.0]
rotate = [0.0, 90.0, 0.0]
keep_transform = true

[[object]]
name = "mv_lid"
kind = "box"
size = [0.2, 0.02, 0.2]
at = [0.0, 0.06, 0.0]
parent = "base"
keep_transform = true

[[object]]
name = "knob"
kind = "sphere"
radius = 0.01
at = [0.0, 0.02, 0.0]
parent = "mv_lid"

[[object]]
name = "hinge"
kind = "box"
size = [0.01, 0.01, 0.01]
at = [0.0, 0.05, 0.1]
parent = "base"
keep_transform = true
hidden = true

[[object]]
name = "pin"
kind = "box"
size = [0.01, 0.01, 0.01]
at = [0.01, 0.0, 0.0]
parent = "hinge"
keep_transform = true

[[object]]
name = "stand"
kind = "box"
size = [0.3, 0.01, 0.3]
at = [0.0, 0.0, 2.0]

[[object]]
name = "arm"
kind = "box"
size = [0.01, 0.1, 0.01]
at = [0.0, 1.0, 0.0]
parent = "stand"
keep_transform = true

[[mover]]
group = "lid"
pivot = [0.0, 0.07, -0.1]
axis = [1.0, 0.0, 0.0]
travel = 1.0
"#;
fn nested(hierarchy: bool) -> Built {
    built(&format!("[output]\nhierarchy = {hierarchy}\n{NESTED}"))
}

struct Tree {
    names: Vec<String>,
    children: Vec<Vec<usize>>,
    roots: Vec<usize>,
    matrices: Vec<[[f32; 4]; 4]>,
}

fn tree(bytes: &[u8]) -> Tree {
    let parsed = gltf::Gltf::from_slice(bytes).unwrap();
    let document = &parsed.document;
    let names = document
        .nodes()
        .map(|n| n.name().unwrap().to_string())
        .collect();
    let children = document
        .nodes()
        .map(|n| n.children().map(|c| c.index()).collect())
        .collect();
    let roots = document
        .default_scene()
        .unwrap()
        .nodes()
        .map(|n| n.index())
        .collect();
    let matrices = document.nodes().map(|n| n.transform().matrix()).collect();
    Tree {
        names,
        children,
        roots,
        matrices,
    }
}

fn moved(matrix: [[f32; 4]; 4]) -> DVec3 {
    DVec3::new(
        matrix[3][0].into(),
        matrix[3][1].into(),
        matrix[3][2].into(),
    )
}

#[test]
fn hierarchy_nests_the_glb_by_parent_with_local_matrices() {
    let b = nested(true);
    let Tree {
        names,
        children,
        roots,
        matrices,
    } = tree(&b.glb());
    assert_eq!(names, ["base", "mv_lid", "knob", "pin", "stand", "arm"]);
    assert_eq!(roots, [0, 4]);
    assert_eq!(
        children,
        [vec![1, 3], vec![2], vec![], vec![], vec![5], vec![]]
    );
    let identity = DMat4::IDENTITY
        .to_cols_array_2d()
        .map(|c| c.map(|v| v as f32));
    assert!(near(moved(matrices[0]), DVec3::new(1.0, 0.0, 0.0)));
    assert!(near(moved(matrices[1]), DVec3::new(0.0, 0.06, 0.0)));
    assert_eq!(matrices[1][0], [1.0, 0.0, 0.0, 0.0]);
    assert_ne!(matrices[2], identity);
    assert!(near(moved(matrices[3]), DVec3::new(0.01, 0.05, 0.1)));
    assert_eq!(matrices[4], identity);
    assert!(near(moved(matrices[5]), DVec3::new(0.0, 1.0, 2.0)));
}

#[test]
fn a_nested_glb_puts_every_node_where_the_flat_one_does() {
    let flat = summary(&nested(false).glb()).unwrap();
    let deep = summary(&nested(true).glb()).unwrap();
    let report = manfred_glb::diff(&flat, &deep);
    assert!(!report.differs(), "{report}");
    let extras =
        |s: &manfred_glb::Summary| -> Vec<_> { s.nodes.iter().map(|n| n.extras.clone()).collect() };
    assert_eq!(extras(&flat), extras(&deep));
    assert!(flat.nodes[1].extras.as_ref().unwrap()["manfred"]["pivot"].is_array());
}

#[test]
fn without_hierarchy_the_glb_stays_flat() {
    let b = nested(false);
    let Tree {
        children, roots, ..
    } = tree(&b.glb());
    assert_eq!(roots, [0, 1, 2, 3, 4, 5]);
    assert!(children.iter().all(Vec::is_empty));
    let extras: Vec<_> = b
        .parts
        .iter()
        .zip(&b.extras)
        .filter(|(part, _)| !part.hidden)
        .map(|(_, extras)| extras.clone())
        .collect();
    assert_eq!(
        b.glb(),
        manfred_glb::write_with_extras(&b.nodes(), &extras, b.order, manfred_build::GENERATOR)
    );
}

#[test]
fn every_kind_in_the_full_fixture_builds() {
    let loaded = load(&fixtures().join("full.toml")).unwrap();
    let mut r = loaded.recipe;
    r.objects
        .retain(|o| !["sheet", "imported", "birds"].contains(&o.name.as_str()));
    let b = build(&r, &fixtures()).unwrap();
    let parts: Vec<&str> = b.parts.iter().map(|p| p.node.name.as_str()).collect();
    assert_eq!(
        parts,
        [
            "crate",
            "drum",
            "ball",
            "button",
            "ring",
            "pill",
            "slab",
            "vase",
            "wire",
            "photo",
            "note",
            "mv_knob",
            "mv_strip__card0"
        ]
    );
    for part in &b.parts {
        let count = part.node.mesh.triangle_count();
        if part.node.name == "note" {
            assert_eq!(count, 0);
        } else {
            assert!(count > 0, "{} has no triangles", part.node.name);
            assert_eq!(part.node.mesh.normals.len(), part.node.mesh.positions.len());
        }
        assert_eq!(
            part.object,
            r.objects
                .iter()
                .position(|o| o.name == part.node.name)
                .unwrap()
        );
    }
    let drum: Vec<&str> = node(&b, "drum")
        .mesh
        .slots
        .iter()
        .map(|s| s.material.as_str())
        .collect();
    assert_eq!(drum, ["steel", "rubber"]);
    assert!(node(&b, "slab").mesh.uvs.is_some());
    assert!(
        b.parts
            .iter()
            .find(|p| p.node.name == "button")
            .unwrap()
            .hidden
    );
    assert!(
        b.parts
            .iter()
            .find(|p| p.node.name == "drum")
            .unwrap()
            .two_sided
    );
    assert_eq!(b.order, Order::Natural);
    assert_eq!(b.facts.cameras.len(), 3);
    assert_eq!(b.facts.movers.len(), 2);
    assert_eq!(b.facts.plate, Some([1586.0, 992.0]));
}

#[test]
fn plate_uvs_build_and_imports_without_a_file_and_generators_are_refused_by_name() {
    let r = load(&fixtures().join("full.toml")).unwrap().recipe;
    let mut sheet = r.clone();
    sheet.objects.retain(|o| o.name == "sheet");
    let b = build(&sheet, &fixtures()).unwrap();
    let uvs = b.parts[0].node.mesh.uvs.clone().unwrap();
    let sentinel = [0.0005, 1.0 - 0.9995];
    assert!(uvs.contains(&sentinel));
    assert!(uvs.iter().any(|uv| *uv != sentinel));
    let only = |name: &str| {
        let mut r = r.clone();
        r.objects.retain(|o| o.name == name);
        build(&r, &fixtures()).unwrap_err().to_string()
    };
    assert!(only("imported").starts_with("object[imported].path: cannot read parts/handle.glb"));
    assert!(only("birds").starts_with(
        "object[birds].generator: `app/birds` is built by the app's own build binary"
    ));
}

#[test]
fn a_pocket_cuts_its_slab_and_makes_no_node() {
    let bare = built(SLAB);
    let cut = built(&format!("{SLAB}{LEFT}"));
    assert_eq!(names(&cut), ["slab"]);
    assert_eq!(cut.parts.len(), 1);
    let (before, after) = (
        node(&bare, "slab").mesh.triangle_count(),
        node(&cut, "slab").mesh.triangle_count(),
    );
    assert_ne!(before, after);
    let [lo, _] = bounds(&node(&cut, "slab").mesh).unwrap();
    assert!((lo[2] + 0.02).abs() < 1e-6);
}

#[test]
fn two_pockets_into_one_target_are_both_cut() {
    let left = node(&built(&format!("{SLAB}{LEFT}")), "slab")
        .mesh
        .triangle_count();
    let right = node(&built(&format!("{SLAB}{RIGHT}")), "slab")
        .mesh
        .triangle_count();
    let both = built(&format!("{SLAB}{LEFT}{RIGHT}"));
    let count = node(&both, "slab").mesh.triangle_count();
    assert!(count > left && count > right, "{count} {left} {right}");
    let floors = |depth: f32| {
        let m = &node(&both, "slab").mesh;
        m.positions
            .iter()
            .filter(|p| (p[2] + depth).abs() < 1e-6)
            .count()
    };
    assert!(floors(0.01) > 0);
    assert!(floors(0.008) > 0);
}

#[test]
fn a_pocket_into_something_else_is_refused() {
    let text = r#"
[[object]]
name = "block"
kind = "box"
size = [0.1, 0.1, 0.1]

[[object]]
name = "hole"
kind = "pocket"
into = "block"
outline = { circle = [0.0, 0.0, 0.01] }
depth = 0.01
"#;
    assert_eq!(
        error(text),
        "object[hole].into: a pocket cuts only into an outline, `block` is a box"
    );
}

#[test]
fn a_pocket_over_its_slab_edge_is_refused_by_name() {
    let text = r#"
[[object]]
name = "notch"
kind = "pocket"
into = "slab"
outline = { rect = [0.08, -0.02, 0.12, 0.02] }
depth = 0.01
"#;
    assert_eq!(
        error(&format!("{SLAB}{text}")),
        "object[notch].outline: the pocket reaches past the edge of `slab`, with 50.0% of its area outside it"
    );
}

#[test]
fn a_pocket_inside_its_slab_and_across_the_bevel_builds() {
    let text = r#"
[[object]]
name = "notch"
kind = "pocket"
into = "slab"
outline = { rect = [0.06, -0.02, 0.0995, 0.02] }
depth = 0.01
"#;
    let b = built(&format!("{SLAB}{text}"));
    assert_eq!(names(&b), ["slab"]);
    assert_ne!(
        node(&b, "slab").mesh.triangle_count(),
        node(&built(SLAB), "slab").mesh.triangle_count()
    );
}

#[test]
fn a_parent_transform_composes() {
    let b = built(
        r#"
[[object]]
name = "child"
kind = "box"
parent = "base"
at = [1.0, 0.0, 0.0]
size = [0.1, 0.1, 0.1]

[[object]]
name = "base"
kind = "box"
at = [0.0, 0.0, 2.0]
rotate = [0.0, 0.0, 90.0]
scale = 2.0
size = [0.1, 0.1, 0.1]
"#,
    );
    assert!(near(
        centre(&node(&b, "child").mesh),
        DVec3::new(0.0, 2.0, 2.0)
    ));
    let [lo, hi] = bounds(&node(&b, "child").mesh).unwrap();
    assert!((hi[0] - lo[0] - 0.2).abs() < 1e-6);
    assert!(near(
        centre(&node(&b, "base").mesh),
        DVec3::new(0.0, 0.0, 2.0)
    ));
}

#[test]
fn a_parent_named_twice_is_the_last_object_of_that_name() {
    let text = r#"
[[object]]
name = "base"
kind = "box"
at = [5.0, 0.0, 0.0]
size = [0.1, 0.1, 0.1]

[[object]]
name = "child"
kind = "box"
parent = "base"
at = [1.0, 0.0, 0.0]
size = [0.1, 0.1, 0.1]

[[object]]
name = "base"
kind = "box"
at = [0.0, 0.0, 2.0]
size = [0.1, 0.1, 0.1]
"#;
    let r = recipe(text);
    let placed = world(&r, 1).unwrap();
    assert!(near(
        placed.transform_point3(DVec3::ZERO),
        DVec3::new(1.0, 0.0, 2.0)
    ));
    assert!(near(
        centre(&node(&built(text), "child").mesh),
        DVec3::new(1.0, 0.0, 2.0)
    ));
}

#[test]
fn keep_transform_keeps_the_matrix_on_the_node() {
    let b = built(
        r#"
[[object]]
name = "ring"
kind = "torus"
major = 0.05
minor = 0.01
at = [0.5, 0.25, 0.0]
rotate = [90.0, 0.0, 0.0]
keep_transform = true
"#,
    );
    let n = node(&b, "ring");
    assert!(n.keep_transform);
    let want = DMat4::from_translation(DVec3::new(0.5, 0.25, 0.0)) * rotation([90.0, 0.0, 0.0]);
    assert!(n.transform.abs_diff_eq(want, 1e-12));
    assert!(near(centre(&n.mesh), DVec3::ZERO));
    let [lo, hi] = bounds(&n.mesh).unwrap();
    assert!((hi[1] - lo[1] - 0.02).abs() < 1e-6);
    let baked = built(
        r#"
[[object]]
name = "ring"
kind = "torus"
major = 0.05
minor = 0.01
at = [0.5, 0.25, 0.0]
"#,
    );
    assert_eq!(node(&baked, "ring").transform, DMat4::IDENTITY);
    assert!(near(
        centre(&node(&baked, "ring").mesh),
        DVec3::new(0.5, 0.25, 0.0)
    ));
}

#[test]
fn a_quarter_turn_about_each_axis_maps_a_known_point() {
    let turn = |r: [f64; 3], p: [f64; 3]| rotation(r).transform_point3(DVec3::from(p));
    assert_eq!(turn([90.0, 0.0, 0.0], [0.0, 1.0, 0.0]), DVec3::Z);
    assert_eq!(turn([0.0, 90.0, 0.0], [0.0, 0.0, 1.0]), DVec3::X);
    assert_eq!(turn([0.0, 0.0, 90.0], [1.0, 0.0, 0.0]), DVec3::Y);
    assert_eq!(turn([90.0, 90.0, 0.0], [0.0, 1.0, 0.0]), DVec3::X);
    assert_eq!(turn([90.0, 0.0, 90.0], [0.0, 0.0, 1.0]), DVec3::X);
    let b = built(
        r#"
[[object]]
name = "dot"
kind = "sphere"
radius = 0.01
segments = 8
rings = 4
at = [0.0, 0.0, 0.0]
rotate = [0.0, 0.0, 90.0]
parent = "arm"

[[object]]
name = "arm"
kind = "box"
size = [0.01, 0.01, 0.01]
rotate = [90.0, 0.0, 0.0]
at = [0.0, 1.0, 0.0]
"#,
    );
    assert!(near(
        centre(&node(&b, "dot").mesh),
        DVec3::new(0.0, 1.0, 0.0)
    ));
}

#[test]
fn hidden_objects_stay_out_of_the_glb() {
    let b = built(
        r#"
[[object]]
name = "seen"
kind = "box"
size = [0.1, 0.1, 0.1]

[[object]]
name = "cutter"
kind = "box"
size = [0.1, 0.1, 0.1]
hidden = true
"#,
    );
    assert_eq!(b.parts.len(), 2);
    assert!(b.parts[1].hidden);
    assert!(b.parts[1].node.mesh.triangle_count() > 0);
    assert_eq!(names(&b), ["seen"]);
}

#[test]
fn natural_order_differs_from_file_order() {
    let slats = r#"
[[object]]
name = "slat10"
kind = "box"
size = [0.1, 0.1, 0.1]

[[object]]
name = "slat2"
kind = "box"
size = [0.1, 0.1, 0.1]
"#;
    assert_eq!(names(&built(slats)), ["slat10", "slat2"]);
    let natural = built(&format!("[output]\norder = \"natural\"\n{slats}"));
    assert_eq!(natural.order, Order::Natural);
    assert_eq!(names(&natural), ["slat2", "slat10"]);
}

#[test]
fn a_variant_drops_its_objects_from_the_build() {
    let r = recipe(
        r#"
[[object]]
name = "slat1"
kind = "box"
size = [0.1, 0.1, 0.1]

[[object]]
name = "mug"
kind = "cylinder"
radius = 0.04
depth = 0.1

[variant.bare]
drop = ["slat*"]
"#,
    );
    let all = build(&r, Path::new(".")).unwrap();
    let bare = build(&apply_variant(&r, "bare").unwrap(), Path::new(".")).unwrap();
    assert_eq!(names(&all), ["slat1", "mug"]);
    assert_eq!(names(&bare), ["mug"]);
}

#[test]
fn materials_name_the_slots_and_lists_follow_face_groups() {
    let one = built(
        r#"
[[object]]
name = "block"
kind = "box"
size = [0.1, 0.1, 0.1]
materials = ["oak"]
"#,
    );
    assert_eq!(node(&one, "block").mesh.slots.len(), 1);
    assert_eq!(node(&one, "block").mesh.slots[0].material, "oak");
    let none = built(
        r#"
[[object]]
name = "block"
kind = "box"
size = [0.1, 0.1, 0.1]
"#,
    );
    assert!(node(&none, "block").mesh.slots.is_empty());
    let can = built(
        r#"
[[object]]
name = "can"
kind = "cylinder"
radius = 0.04
depth = 0.1
segments = 16
materials = ["tin", "lid"]
"#,
    );
    let slots = &node(&can, "can").mesh.slots;
    assert_eq!((slots[0].count / 3, slots[1].count / 3), (32, 28));
    assert_eq!(
        (slots[0].material.as_str(), slots[1].material.as_str()),
        ("tin", "lid")
    );
    assert_eq!(
        error(
            r#"
[[object]]
name = "block"
kind = "box"
size = [0.1, 0.1, 0.1]
materials = ["a", "b"]
"#
        ),
        "object[block].materials: a box defines no face groups, expected one name or `material`"
    );
}

#[test]
fn uvs_are_local_unless_none() {
    let b = built(
        r#"
[[object]]
name = "with"
kind = "cylinder"
radius = 0.04
depth = 0.1

[[object]]
name = "without"
kind = "sphere"
radius = 0.04
uv = "none"
"#,
    );
    let with = &node(&b, "with").mesh;
    assert_eq!(with.uvs.as_ref().map(Vec::len), Some(with.positions.len()));
    assert!(node(&b, "without").mesh.uvs.is_none());
}

#[test]
fn a_mesh_object_imports_a_glb_node() {
    let dir = common::scratch("import");
    let tri = Mesh {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        uvs: None,
        uvs1: None,
        indices: vec![0, 1, 2],
        slots: Vec::new(),
    };
    let mut other = tri.clone();
    other.positions.push([0.0, 0.0, 1.0]);
    other.indices.extend([0, 1, 3]);
    let nodes = [
        Node {
            name: "tri".into(),
            mesh: tri,
            transform: DMat4::IDENTITY,
            keep_transform: false,
            targets: None,
        },
        Node {
            name: "pair".into(),
            mesh: other,
            transform: DMat4::IDENTITY,
            keep_transform: false,
            targets: None,
        },
    ];
    std::fs::write(
        dir.join("parts.glb"),
        manfred_glb::write(&nodes, Order::File, "t"),
    )
    .unwrap();
    let r = recipe(
        r#"
[[object]]
name = "one"
kind = "mesh"
path = "parts.glb"
node = "pair"
at = [0.0, 2.0, 0.0]
material = "clay"

[[object]]
name = "all"
kind = "mesh"
path = "parts.glb"
"#,
    );
    let b = build(&r, &dir).unwrap();
    assert_eq!(node(&b, "one").mesh.triangle_count(), 2);
    assert_eq!(node(&b, "one").mesh.slots[0].material, "clay");
    assert_eq!(bounds(&node(&b, "one").mesh).unwrap()[0][1], 2.0);
    assert_eq!(node(&b, "all").mesh.triangle_count(), 3);
    let mut r = r.clone();
    r.objects.truncate(1);
    if let manfred_recipe::Kind::Mesh { node, .. } = &mut r.objects[0].kind {
        *node = Some("nobody".into());
    }
    assert_eq!(
        build(&r, &dir).unwrap_err().to_string(),
        "object[one].node: parts.glb has no node `nobody` with a mesh"
    );
}

#[test]
fn a_mesh_object_keeps_the_glb_s_materials_unless_it_names_one() {
    let dir = common::scratch("import-materials");
    let slot = |material: &str| manfred_mesh::Slot {
        material: material.into(),
        start: 0,
        count: 3,
    };
    let tri = Mesh {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        uvs: None,
        uvs1: None,
        indices: vec![0, 1, 2],
        slots: vec![slot("brass")],
    };
    let mut other = tri.clone();
    other.positions.iter_mut().for_each(|p| p[2] = 1.0);
    other.slots = vec![slot("clay")];
    let nodes = [("tri", tri), ("lid", other)].map(|(name, mesh)| Node {
        name: name.into(),
        mesh,
        transform: DMat4::IDENTITY,
        keep_transform: false,
        targets: None,
    });
    std::fs::write(
        dir.join("parts.glb"),
        manfred_glb::write(&nodes, Order::File, "t"),
    )
    .unwrap();
    let r = recipe(
        r#"
[[object]]
name = "kept"
kind = "mesh"
path = "parts.glb"

[[object]]
name = "named"
kind = "mesh"
path = "parts.glb"
material = "oak"
"#,
    );
    let b = build(&r, &dir).unwrap();
    let names = |name: &str| -> Vec<String> {
        node(&b, name)
            .mesh
            .slots
            .iter()
            .map(|slot| slot.material.clone())
            .collect()
    };
    assert_eq!(names("kept"), ["brass", "clay"]);
    assert_eq!(names("named"), ["oak"]);
}

#[test]
fn a_mesh_object_drops_bad_indices_and_short_uvs_and_ends_a_node_cycle() {
    let dir = common::scratch("import-bad");
    std::fs::write(dir.join("bad.gltf"), BAD_GLTF).unwrap();
    let r = recipe("[[object]]\nname = \"bad\"\nkind = \"mesh\"\npath = \"bad.gltf\"\n");
    let b = build(&r, &dir).unwrap();
    assert_eq!(node(&b, "bad").mesh.triangle_count(), 1);
}

const BAD_GLTF: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
"nodes":[{"mesh":0,"children":[1]},{"children":[0]}],
"meshes":[{"primitives":[{"attributes":{"POSITION":0,"TEXCOORD_0":1},"indices":2}]}],
"buffers":[{"byteLength":64,"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAAAAAAAAAAAAAAAIA/AAAAAAAAAQACAAAAAQAJAA=="}],
"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":16},{"buffer":0,"byteOffset":52,"byteLength":12}],
"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},
{"bufferView":1,"componentType":5126,"count":2,"type":"VEC2"},
{"bufferView":2,"componentType":5123,"count":6,"type":"SCALAR"}]}"#;

#[test]
fn daylight_goes_to_the_sidecar_as_it_is() {
    let b = built(
        r#"
[output]
sidecar = "t.json"
sidecar_dialect = "facts"

[[camera]]
name = "main"
position = [0.0, 0.0, 1.0]
target = [0.0, 0.0, 0.0]
fov_y_deg = 30.0

[sun]
model = "daylight"
hour = 9.5
latitude = 44.4
heading = 0.0
"#,
    );
    let text = b.sidecar().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["daylight"]["hour"], 9.5);
    assert_eq!(json["daylight"]["day"], 172);
    assert!(json.get("sun").is_none());
}

#[test]
fn write_puts_files_where_output_says_or_into_out() {
    let dir = common::scratch("write");
    let r = recipe(
        r#"
[output]
glb = "scene/t.glb"
sidecar = "scene/t.json"
sidecar_dialect = "facts"

[[object]]
name = "block"
kind = "box"
size = [0.1, 0.1, 0.1]

[[camera]]
name = "main"
position = [0.0, 0.0, 1.0]
target = [0.0, 0.0, 0.0]
fov_y_deg = 30.0
"#,
    );
    let b = build(&r, &dir).unwrap();
    let beside = manfred_build::write(&b, &r, &dir, None).unwrap();
    assert_eq!(beside, [dir.join("scene/t.glb"), dir.join("scene/t.json")]);
    let out = dir.join("out");
    let into = manfred_build::write(&b, &r, &dir, Some(&out)).unwrap();
    assert_eq!(into, [out.join("t.glb"), out.join("t.json")]);
    assert_eq!(
        std::fs::read(&beside[0]).unwrap(),
        std::fs::read(&into[0]).unwrap()
    );
    let glb = summary(&std::fs::read(&into[0]).unwrap()).unwrap();
    assert_eq!(glb.generator.as_deref(), Some(manfred_build::GENERATOR));
    assert!(manfred_build::GENERATOR.starts_with("manfred "));
    let mut left: Vec<String> = std::fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["t.glb", "t.json"]);
}
