use glam::{DMat4, DVec3};
use manfred_glb::{
    CameraFacts, DaylightFacts, GradeFacts, LampFacts, LookFacts, MoverFacts, Node, Order,
    RoomFacts, SceneFacts, SkyFacts, SunFacts, diff, facts as facts_sidecar, merge, sidecar_text,
    summary, write, write_tree, write_with_extras,
};
use manfred_mesh::{Mesh, Slot};
use pfx_load::Mesh as Loaded;
use serde_json::json;
use std::path::PathBuf;

const GENERATOR: &str = "manfred 0.1.0";

fn quad(x: f32, y: f32, size: f32, uvs: bool, slots: &[(&str, u32, u32)]) -> Mesh {
    Mesh {
        positions: vec![
            [x, y, 0.0],
            [x + size, y, 0.0],
            [x + size, y + size, 0.0],
            [x, y + size, 0.0],
        ],
        normals: vec![[0.0, 0.0, 1.0]; 4],
        uvs: uvs.then(|| vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
        uvs1: None,
        indices: vec![0, 1, 2, 0, 2, 3],
        slots: slots
            .iter()
            .map(|(material, start, count)| Slot {
                material: material.to_string(),
                start: *start,
                count: *count,
            })
            .collect(),
    }
}

fn node(name: &str, mesh: Mesh) -> Node {
    Node {
        name: name.to_string(),
        mesh,
        transform: DMat4::IDENTITY,
        keep_transform: false,
        targets: None,
    }
}

fn two() -> Vec<Node> {
    vec![
        node("plate", quad(0.0, 0.0, 1.0, true, &[("steel", 0, 6)])),
        node(
            "knob",
            quad(2.0, 0.5, 0.25, false, &[("rubber", 0, 3), ("steel", 3, 3)]),
        ),
    ]
}

fn names(bytes: &[u8]) -> Vec<String> {
    summary(bytes)
        .unwrap()
        .nodes
        .into_iter()
        .map(|n| n.name)
        .collect()
}

#[test]
fn the_same_input_gives_the_same_bytes() {
    assert_eq!(
        write(&two(), Order::File, GENERATOR),
        write(&two(), Order::File, GENERATOR)
    );
}

#[test]
fn golden_two_node_scene() {
    let bytes = write(&two(), Order::File, GENERATOR);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/two.glb");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &bytes).unwrap();
    }
    let golden =
        std::fs::read(&path).expect("tests/golden/two.glb; regenerate with UPDATE_GOLDEN=1");
    assert!(
        golden == bytes,
        "the writer's output changed; UPDATE_GOLDEN=1 regenerates the golden"
    );
}

#[test]
fn the_file_has_the_structure_the_apps_expect() {
    let bytes = write(&two(), Order::File, GENERATOR);
    assert_eq!(&bytes[0..4], b"glTF");
    assert_eq!(bytes.len() % 4, 0);
    let declared = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    assert_eq!(declared as usize, bytes.len());
    let parsed = gltf::Gltf::from_slice(&bytes).unwrap();
    let json = parsed.document.as_json();
    assert_eq!(json.asset.version, "2.0");
    assert_eq!(json.asset.generator.as_deref(), Some(GENERATOR));
    assert!(json.cameras.is_empty() && json.animations.is_empty() && json.skins.is_empty());
    assert_eq!(json.scenes.len(), 1);
    let materials: Vec<_> = parsed.document.materials().collect();
    assert_eq!(
        materials
            .iter()
            .map(|m| m.name().unwrap())
            .collect::<Vec<_>>(),
        ["steel", "rubber"]
    );
    for material in &materials {
        let pbr = material.pbr_metallic_roughness();
        assert_eq!(pbr.base_color_factor(), [0.8, 0.8, 0.8, 1.0]);
        assert_eq!(pbr.metallic_factor(), 0.0);
        assert_eq!(pbr.roughness_factor(), 0.5);
        assert!(material.double_sided());
    }
    let plate = parsed.document.nodes().next().unwrap();
    let primitive = plate.mesh().unwrap().primitives().next().unwrap();
    let semantics: Vec<String> = primitive
        .attributes()
        .map(|(semantic, accessor)| {
            assert!(accessor.min().is_some() && accessor.max().is_some());
            format!("{semantic:?}")
        })
        .collect();
    assert_eq!(semantics.len(), 3);
    let knob = parsed.document.nodes().nth(1).unwrap();
    let mesh = knob.mesh().unwrap();
    assert_eq!(mesh.primitives().len(), 2);
    for primitive in mesh.primitives() {
        assert!(primitive.get(&gltf::Semantic::TexCoords(0)).is_none());
        assert_eq!(
            primitive.get(&gltf::Semantic::Positions).unwrap().count(),
            3
        );
    }
    assert_eq!(
        plate.transform().matrix(),
        DMat4::IDENTITY
            .to_cols_array_2d()
            .map(|c| c.map(|v| v as f32))
    );
}

#[test]
fn the_file_loads_through_the_gltf_crate() {
    let bytes = write(&two(), Order::File, GENERATOR);
    let (document, buffers, images) = gltf::import_slice(&bytes).unwrap();
    assert!(images.is_empty());
    let order: Vec<_> = document
        .nodes()
        .map(|n| n.name().unwrap().to_string())
        .collect();
    assert_eq!(order, ["plate", "knob"]);
    let knob = document.nodes().nth(1).unwrap();
    let mut triangles = 0;
    for primitive in knob.mesh().unwrap().primitives() {
        let reader = primitive.reader(|b| Some(&buffers[b.index()]));
        let positions: Vec<[f32; 3]> = reader.read_positions().unwrap().collect();
        let indices: Vec<u32> = reader.read_indices().unwrap().into_u32().collect();
        assert!(indices.iter().all(|&i| (i as usize) < positions.len()));
        triangles += indices.len() / 3;
    }
    assert_eq!(triangles, 2);
}

#[test]
fn natural_and_file_order_differ_on_numbers_and_prefixes() {
    let given = [
        "wall_bottom",
        "slat10",
        "mv_knob__cap",
        "wall",
        "slat2",
        "mv_knob",
    ];
    let nodes: Vec<Node> = given
        .iter()
        .map(|n| node(n, quad(0.0, 0.0, 1.0, false, &[("m", 0, 6)])))
        .collect();
    assert_eq!(names(&write(&nodes, Order::File, GENERATOR)), given);
    assert_eq!(
        names(&write(&nodes, Order::Natural, GENERATOR)),
        [
            "mv_knob",
            "mv_knob__cap",
            "slat2",
            "slat10",
            "wall",
            "wall_bottom"
        ]
    );
}

#[test]
fn materials_are_interned_in_the_order_of_the_nodes() {
    let nodes = vec![
        node("b", quad(0.0, 0.0, 1.0, false, &[("zinc", 0, 6)])),
        node(
            "a",
            quad(0.0, 0.0, 1.0, false, &[("alu", 0, 3), ("zinc", 3, 3)]),
        ),
    ];
    assert_eq!(
        summary(&write(&nodes, Order::File, GENERATOR))
            .unwrap()
            .materials,
        ["zinc", "alu"]
    );
    assert_eq!(
        summary(&write(&nodes, Order::Natural, GENERATOR))
            .unwrap()
            .materials,
        ["alu", "zinc"]
    );
}

#[test]
fn a_kept_transform_is_written_and_a_dropped_one_is_not() {
    let moved = DMat4::from_translation(DVec3::new(0.0, 5.0, 0.0));
    let mut kept = node("kept", quad(0.0, 0.0, 1.0, false, &[("m", 0, 6)]));
    kept.transform = moved;
    kept.keep_transform = true;
    let mut dropped = node("dropped", quad(0.0, 0.0, 1.0, false, &[("m", 0, 6)]));
    dropped.transform = moved;
    let found = summary(&write(&[kept, dropped], Order::File, GENERATOR)).unwrap();
    assert_eq!(found.nodes[0].bounds[0][1], 5.0);
    assert_eq!(found.nodes[1].bounds[0][1], 0.0);
}

fn kept(name: &str, x: f64, y: f64) -> Node {
    let mut node = node(name, quad(0.0, 0.0, 1.0, false, &[("m", 0, 6)]));
    node.transform = DMat4::from_translation(DVec3::new(x, y, 0.0));
    node.keep_transform = true;
    node
}

fn tree() -> (Vec<Node>, Vec<Option<usize>>) {
    let mut lid = kept("lid", 0.0, 0.0);
    lid.transform = DMat4::from_translation(DVec3::new(1.0, 0.0, 0.0))
        * DMat4::from_rotation_z(std::f64::consts::FRAC_PI_2);
    (
        vec![
            lid,
            kept("box", 0.0, 5.0),
            node("knob", quad(0.0, 0.0, 0.5, false, &[("m", 0, 6)])),
        ],
        vec![Some(1), None, Some(0)],
    )
}

#[test]
fn summary_ends_a_node_cycle() {
    let text = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"name":"a","children":[1]},{"name":"b","children":[0]}]}"#;
    assert!(summary(text.as_bytes()).unwrap().nodes.is_empty());
}

#[test]
fn a_flat_tree_writes_the_same_bytes_as_a_flat_list() {
    let flat = two();
    for order in [Order::File, Order::Natural] {
        let plain = write(&flat, order, GENERATOR);
        assert_eq!(
            write_tree(&flat, &[], &[], &[], order, GENERATOR).unwrap(),
            plain
        );
        assert_eq!(
            write_tree(&flat, &[None, None], &[], &[], order, GENERATOR).unwrap(),
            plain
        );
    }
}

#[test]
fn nested_nodes_keep_local_matrices_and_read_back_in_world_space() {
    let (nodes, parents) = tree();
    let bytes = write_tree(&nodes, &parents, &[], &[], Order::File, GENERATOR).unwrap();
    let parsed = gltf::Gltf::from_slice(&bytes).unwrap();
    let document = &parsed.document;
    let roots: Vec<usize> = document
        .default_scene()
        .unwrap()
        .nodes()
        .map(|n| n.index())
        .collect();
    assert_eq!(roots, [1]);
    let children = |at: usize| -> Vec<usize> {
        document
            .nodes()
            .nth(at)
            .unwrap()
            .children()
            .map(|n| n.index())
            .collect()
    };
    assert_eq!(children(1), [0]);
    assert_eq!(children(0), [2]);
    assert!(children(2).is_empty());
    let lid = document.nodes().next().unwrap().transform().matrix();
    assert_eq!(lid[3], [1.0, 0.0, 0.0, 1.0]);
    let found = summary(&bytes).unwrap();
    let order: Vec<&str> = found.nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(order, ["box", "lid", "knob"]);
    let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6);
    assert!(near(found.nodes[1].bounds[0], [0.0, 5.0, 0.0]));
    assert!(near(found.nodes[1].bounds[1], [1.0, 6.0, 0.0]));
    assert!(near(found.nodes[2].bounds[0], [0.5, 5.0, 0.0]));
    assert!(near(found.nodes[2].bounds[1], [1.0, 5.5, 0.0]));
    let loaded = Loaded::parse(&bytes).unwrap();
    assert_eq!(loaded.all_roots(), [1]);
    assert_eq!(loaded.nodes[2].parent, Some(0));
    let world = loaded.world(2).unwrap();
    assert!(near(
        world[3].map(f64::from)[..3].try_into().unwrap(),
        [1.0, 5.0, 0.0]
    ));
}

#[test]
fn natural_order_moves_the_children_with_their_nodes() {
    let (nodes, parents) = tree();
    let extras = [None, Some(json!({ "manfred": { "group": "box" } })), None];
    let bytes = write_with_extras(&nodes, &extras, Order::Natural, GENERATOR);
    let nested = write_tree(&nodes, &parents, &extras, &[], Order::Natural, GENERATOR).unwrap();
    assert_eq!(names(&bytes), ["box", "knob", "lid"]);
    let parsed = gltf::Gltf::from_slice(&nested).unwrap();
    let names_of = |list: Vec<gltf::Node>| -> Vec<String> {
        list.iter().map(|n| n.name().unwrap().to_string()).collect()
    };
    let all = names_of(parsed.document.nodes().collect());
    assert_eq!(all, ["box", "knob", "lid"]);
    let at = |name: &str| {
        parsed
            .document
            .nodes()
            .find(|n| n.name() == Some(name))
            .unwrap()
    };
    assert_eq!(names_of(at("box").children().collect()), ["lid"]);
    assert_eq!(names_of(at("lid").children().collect()), ["knob"]);
    assert!(at("lid").extras().is_none());
    assert!(at("box").extras().is_some());
    assert_eq!(names(&nested), ["box", "lid", "knob"]);
}

#[test]
fn a_loop_or_a_missing_parent_leaves_the_nodes_at_the_root() {
    let (nodes, _) = tree();
    let flat = write(&nodes, Order::File, GENERATOR);
    for parents in [
        vec![Some(1), Some(0), None],
        vec![Some(0), None, None],
        vec![Some(7), None, Some(9)],
    ] {
        let nested = write_tree(&nodes, &parents, &[], &[], Order::File, GENERATOR).unwrap();
        let roots = gltf::Gltf::from_slice(&nested)
            .unwrap()
            .document
            .default_scene()
            .unwrap()
            .nodes()
            .count();
        assert_eq!(roots, 3, "{parents:?}");
        assert_eq!(nested, flat, "{parents:?}");
    }
}

#[test]
fn summary_reports_counts_bounds_area_and_slots() {
    let found = summary(&write(&two(), Order::File, GENERATOR)).unwrap();
    assert_eq!(found.generator.as_deref(), Some(GENERATOR));
    assert_eq!(found.materials, ["steel", "rubber"]);
    let plate = &found.nodes[0];
    assert_eq!((plate.name.as_str(), plate.index), ("plate", 0));
    assert_eq!((plate.triangles, plate.vertices), (2, 4));
    assert_eq!(plate.bounds, [[0.0, 0.0, 0.0], [1.0, 1.0, 0.0]]);
    assert!((plate.area - 1.0).abs() < 1e-9);
    assert_eq!(plate.slots, ["steel"]);
    assert!(plate.uvs);
    let knob = &found.nodes[1];
    assert_eq!((knob.triangles, knob.vertices), (2, 6));
    assert_eq!(knob.slots, ["rubber", "steel"]);
    assert!(!knob.uvs);
}

#[test]
fn a_file_against_itself_differs_in_nothing() {
    let a = summary(&write(&two(), Order::File, GENERATOR)).unwrap();
    let report = diff(&a, &a);
    assert!(!report.differs());
    assert!(report.nodes.is_empty());
    assert_eq!(report.to_string(), "no differences beyond tolerance\n");
}

#[test]
fn a_moved_node_is_the_only_difference() {
    let a = summary(&write(&two(), Order::File, GENERATOR)).unwrap();
    let mut nodes = two();
    nodes[1].transform = DMat4::from_translation(DVec3::new(0.0, 0.01, 0.0));
    nodes[1].keep_transform = true;
    let b = summary(&write(&nodes, Order::File, GENERATOR)).unwrap();
    let report = diff(&a, &b);
    assert_eq!(report.nodes.len(), 1);
    assert_eq!(report.nodes[0].name, "knob");
    assert_eq!(report.nodes[0].marks.len(), 1);
    assert!(report.nodes[0].marks[0].starts_with("bounds"));
    assert!(report.only_a.is_empty() && report.only_b.is_empty());
    assert!(!report.order_differs && !report.materials_differ);
    assert!(report.differs());
}

#[test]
fn a_move_inside_the_tolerance_is_not_a_difference() {
    let a = summary(&write(&two(), Order::File, GENERATOR)).unwrap();
    let mut nodes = two();
    nodes[1].transform = DMat4::from_translation(DVec3::new(0.0, 0.00002, 0.0));
    nodes[1].keep_transform = true;
    let b = summary(&write(&nodes, Order::File, GENERATOR)).unwrap();
    assert!(!diff(&a, &b).differs());
}

#[test]
fn triangles_nodes_order_and_materials_are_reported() {
    let a = summary(&write(&two(), Order::File, GENERATOR)).unwrap();
    let mut nodes = two();
    nodes.reverse();
    nodes[0].mesh.indices = vec![0, 1, 2];
    nodes[0].mesh.slots = vec![Slot {
        material: "rubber".into(),
        start: 0,
        count: 3,
    }];
    nodes.push(node(
        "extra",
        quad(0.0, 0.0, 1.0, false, &[("steel", 0, 6)]),
    ));
    let b = summary(&write(&nodes, Order::File, GENERATOR)).unwrap();
    let report = diff(&a, &b);
    assert_eq!(report.only_b, ["extra"]);
    assert!(report.only_a.is_empty());
    assert!(report.order_differs);
    assert!(report.materials_differ);
    let knob = report.nodes.iter().find(|n| n.name == "knob").unwrap();
    assert!(knob.marks.iter().any(|m| m == "triangles 2 -> 1"));
    assert!(knob.marks.iter().any(|m| m.starts_with("slots")));
    assert!(knob.marks.iter().any(|m| m.starts_with("area")));
    let value = report.json();
    assert_eq!(value["only_b"], json!(["extra"]));
    assert_eq!(value["order_differs"], json!(true));
    let text = report.to_string();
    assert!(text.contains("~ knob") && text.contains("+ extra (only in b)"));
    assert!(text.contains("! node order differs") && text.contains("! materials differ"));
}

#[test]
fn a_file_that_is_not_a_glb_is_an_error() {
    assert!(summary(b"not a glb at all").is_err());
}

fn facts() -> SceneFacts {
    let mut extra = serde_json::Map::new();
    extra.insert("motion".into(), json!({"density": 1.8}));
    SceneFacts {
        cameras: vec![CameraFacts {
            name: "main".into(),
            position: [0.0, 0.12, 1.6],
            target: [0.0, 0.12, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: Some(30.0),
            shift: [0.0, -0.5],
            focus_m: Some(1.6),
            ..Default::default()
        }],
        sun: Some(SunFacts {
            toward: [0.5, 0.75, 0.25],
            radius_deg: 0.8,
            color: [1.0, 0.86, 0.66],
            irradiance: 4.5,
        }),
        daylight: Some(DaylightFacts {
            hour: 10.0,
            day: 172,
            latitude: 45.0,
            heading: 30.0,
        }),
        sky: Some(SkyFacts {
            intensity: 4.2,
            rotation_deg: 169.5,
            balance: Some([1.0, 0.92, 0.8]),
            room: Some(RoomFacts {
                width: 256,
                floor: [0.1, 0.1, 0.1],
                wall: [0.2, 0.2, 0.2],
                ceiling: [0.5, 0.5, 0.5],
                lights: vec![LampFacts {
                    name: "key".into(),
                    az: 30.0,
                    el: 40.0,
                    power: 4.0,
                    color: [1.0, 1.0, 1.0],
                    width: 30.0,
                    height: 20.0,
                    soft: 0.1,
                    slats: 0.0,
                    open: 1.0,
                }],
            }),
        }),
        plate: Some([1600.0, 1000.0]),
        movers: vec![
            MoverFacts {
                group: "knob".into(),
                pivot: [0.25, 0.5, 0.0],
                axis: [0.0, 0.0, -1.0],
                travel: 0.003,
                clip: None,
                clip2: None,
            },
            MoverFacts {
                group: "lid".into(),
                pivot: [0.0, 0.0, 0.0],
                axis: [0.0, 1.0, 0.0],
                travel: 0.1,
                clip: Some([0.0, 1.0, 0.0, 0.5]),
                clip2: Some([0.0, -1.0, 0.0, 0.5]),
            },
        ],
        look: Some(LookFacts {
            exposure: 0.93,
            grade: GradeFacts {
                vignette: 0.4,
                saturation: 1.0,
                warmth: 0.0,
                bloom: 0.12,
                haze: 0.0,
            },
        }),
        extra,
    }
}

#[test]
fn the_facts_dialect_writes_the_facts_as_they_are() {
    let expected = json!({
        "cameras": [{
            "name": "main",
            "orthographic": false,
            "position": [0.0, 0.12, 1.6],
            "target": [0.0, 0.12, 0.0],
            "up": [0.0, 1.0, 0.0],
            "fov_y_deg": 30.0,
            "focus_m": 1.6,
            "shift": [0.0, -0.5],
            "fstop": 0.0
        }],
        "sun": {
            "toward": [0.5, 0.75, 0.25],
            "radius_deg": 0.8,
            "color": [1.0, 0.86, 0.66],
            "irradiance": 4.5
        },
        "daylight": {"hour": 10.0, "day": 172, "latitude": 45.0, "heading": 30.0},
        "sky": {
            "intensity": 4.2,
            "rotation_deg": 169.5,
            "balance": [1.0, 0.92, 0.8],
            "room": {
                "width": 256,
                "floor": [0.1, 0.1, 0.1],
                "wall": [0.2, 0.2, 0.2],
                "ceiling": [0.5, 0.5, 0.5],
                "lights": [{
                    "name": "key",
                    "az": 30.0,
                    "el": 40.0,
                    "width": 30.0,
                    "height": 20.0,
                    "power": 4.0,
                    "color": [1.0, 1.0, 1.0],
                    "soft": 0.1
                }]
            }
        },
        "plate": {"width": 1600.0, "height": 1000.0},
        "movers": {
            "knob": {"pivot": [0.25, 0.5, 0.0], "axis": [0.0, 0.0, -1.0], "travel": 0.003},
            "lid": {
                "pivot": [0.0, 0.0, 0.0],
                "axis": [0.0, 1.0, 0.0],
                "travel": 0.1,
                "clip": [0.0, 1.0, 0.0, 0.5],
                "clip2": [0.0, -1.0, 0.0, 0.5]
            }
        },
        "look": {
            "exposure": 0.93,
            "grade": {"vignette": 0.4, "saturation": 1.0, "warmth": 0.0, "bloom": 0.12, "haze": 0.0}
        }
    });
    assert_eq!(facts_sidecar(&facts()), Ok(expected));
}

#[test]
fn the_facts_dialect_leaves_out_what_the_recipe_does_not_say() {
    assert_eq!(facts_sidecar(&SceneFacts::default()), Ok(json!({})));
    let mut f = facts();
    f.cameras[0].orthographic = true;
    f.cameras[0].ortho_height = Some(2.0);
    f.cameras[0].fov_y_deg = None;
    let camera = &facts_sidecar(&f).unwrap()["cameras"][0];
    assert_eq!(camera["orthographic"], json!(true));
    assert_eq!(camera["ortho_height"], json!(2.0));
    assert!(camera.get("fov_y_deg").is_none());
}

#[test]
fn merge_is_deep_and_the_later_key_wins() {
    let mut into = json!({"sun": {"toward": [0.0, 1.0, 0.0], "radius_deg": 0.5}, "a": 1})
        .as_object()
        .unwrap()
        .clone();
    let from = json!({"sun": {"radius_deg": 0.8}, "a": {"b": 2}});
    merge(&mut into, from.as_object().unwrap());
    assert_eq!(
        serde_json::Value::Object(into),
        json!({"sun": {"toward": [0.0, 1.0, 0.0], "radius_deg": 0.8}, "a": {"b": 2}})
    );
}

#[test]
fn sidecar_text_is_pretty_sorted_and_round_trips() {
    let value = facts_sidecar(&facts()).unwrap();
    let text = sidecar_text(&value);
    assert!(text.ends_with("}\n"));
    assert!(text.starts_with("{\n  \"cameras\": ["));
    let cameras = text.find("\"cameras\"").unwrap();
    let daylight = text.find("\"daylight\"").unwrap();
    let look = text.find("\"look\"").unwrap();
    let sun = text.find("\"sun\"").unwrap();
    assert!(cameras < daylight && daylight < look && look < sun);
    assert!(text.contains("\"width\": 1600.0"));
    let back: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(back, value);
    assert_eq!(text, sidecar_text(&facts_sidecar(&facts()).unwrap()));
}
