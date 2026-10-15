use glam::DMat4;
use manfred_glb::MoverFacts;
use manfred_glb::{Node, Order, diff, mover_json, summary, write, write_with_extras};
use manfred_mesh::{Mesh, Slot};
use pfx_load::Mesh as Loaded;
use serde_json::{Value, json};

const GENERATOR: &str = "manfred 0.1.0";

fn quad(x: f32) -> Mesh {
    Mesh {
        positions: vec![
            [x, 0.0, 0.0],
            [x + 1.0, 0.0, 0.0],
            [x + 1.0, 1.0, 0.0],
            [x, 1.0, 0.0],
        ],
        normals: vec![[0.0, 0.0, 1.0]; 4],
        uvs: Some(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
        uvs1: None,
        indices: vec![0, 1, 2, 0, 2, 3],
        slots: vec![Slot {
            material: "steel".into(),
            start: 0,
            count: 6,
        }],
    }
}

fn node(name: &str, x: f32) -> Node {
    Node {
        name: name.to_string(),
        mesh: quad(x),
        transform: DMat4::IDENTITY,
        keep_transform: false,
        targets: None,
    }
}

fn nodes() -> Vec<Node> {
    vec![
        node("mv_knob", 0.0),
        node("mv_knob__cap", 2.0),
        node("plate", 4.0),
    ]
}

fn facts(travel: f64) -> MoverFacts {
    MoverFacts {
        group: "knob".into(),
        pivot: [0.1, 0.05, -0.003],
        axis: [0.0, 0.0, -1.0],
        travel,
        clip: Some([0.0, 1.0, 0.0, 0.25]),
        clip2: None,
    }
}

fn extras(travel: f64) -> Vec<Option<Value>> {
    let mut root = mover_json(&facts(travel));
    root.insert("group".into(), json!("knob"));
    vec![
        Some(json!({ "manfred": root })),
        Some(json!({ "manfred": { "group": "knob" } })),
        None,
    ]
}

fn bytes(travel: f64) -> Vec<u8> {
    write_with_extras(&nodes(), &extras(travel), Order::File, GENERATOR)
}

#[test]
fn a_mover_root_and_a_part_carry_extras_and_a_plain_object_none() {
    let loaded = Loaded::parse(&bytes(0.003)).unwrap();
    let [root, part, plain] = &loaded.nodes[..] else {
        panic!("three nodes");
    };
    assert_eq!(root.name, "mv_knob");
    let manfred = root.extra_object("manfred").unwrap();
    assert_eq!(manfred["group"], "knob");
    assert_eq!(manfred["pivot"], json!([0.1, 0.05, -0.003]));
    assert_eq!(manfred["axis"], json!([0.0, 0.0, -1.0]));
    assert_eq!(manfred["travel"], json!(0.003));
    assert_eq!(manfred["clip"], json!([0.0, 1.0, 0.0, 0.25]));
    assert!(!manfred.contains_key("clip2"));
    let numbers: Vec<f64> = manfred["pivot"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    assert_eq!(numbers, [0.1, 0.05, -0.003]);
    assert_eq!(manfred["travel"].as_f64(), Some(0.003));
    assert_eq!(part.name, "mv_knob__cap");
    assert_eq!(
        part.extra_object("manfred").unwrap().get("group"),
        Some(&json!("knob"))
    );
    assert_eq!(part.extra_object("manfred").unwrap().len(), 1);
    assert_eq!(plain.name, "plate");
    assert!(plain.extras.is_none());
    assert!(loaded.groups.iter().all(|group| group.extras.is_none()));
    assert!(
        loaded
            .materials
            .iter()
            .all(|material| material.extras.is_none())
    );
}

#[test]
fn without_extras_the_bytes_are_the_ones_write_gives() {
    assert_eq!(
        write_with_extras(&nodes(), &[], Order::File, GENERATOR),
        write(&nodes(), Order::File, GENERATOR)
    );
    assert_eq!(
        write_with_extras(&nodes(), &[None, None, None], Order::Natural, GENERATOR),
        write(&nodes(), Order::Natural, GENERATOR)
    );
}

#[test]
fn extras_change_nothing_but_the_json_chunk() {
    let plain = write(&nodes(), Order::File, GENERATOR);
    let with = bytes(0.003);
    let bin = |glb: &[u8]| {
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        glb[20 + json_len..].to_vec()
    };
    assert_eq!(bin(&plain), bin(&with));
    assert!(with.len() > plain.len());
    let a = summary(&plain).unwrap();
    let mut b = summary(&with).unwrap();
    assert!(b.nodes.iter().any(|n| n.extras.is_some()));
    for node in &mut b.nodes {
        node.extras = None;
    }
    assert_eq!(a, b);
}

#[test]
fn natural_order_carries_each_nodes_extras_with_it() {
    let out = vec![node("b", 0.0), node("a", 2.0)];
    let extras = vec![Some(json!({ "manfred": { "group": "b" } })), None];
    let loaded =
        Loaded::parse(&write_with_extras(&out, &extras, Order::Natural, GENERATOR)).unwrap();
    assert_eq!(loaded.nodes[0].name, "a");
    assert!(loaded.nodes[0].extras.is_none());
    assert_eq!(loaded.nodes[1].name, "b");
    assert_eq!(
        loaded.nodes[1].extra_object("manfred").unwrap()["group"],
        "b"
    );
}

#[test]
fn extras_keys_are_written_sorted_and_the_bytes_repeat() {
    assert_eq!(bytes(0.003), bytes(0.003));
    let text = String::from_utf8_lossy(&bytes(0.003)).to_string();
    let at = |key: &str| text.find(&format!("\"{key}\":")).unwrap();
    assert!(at("axis") < at("clip"));
    assert!(at("clip") < at("group"));
    assert!(at("group") < at("pivot"));
    assert!(at("pivot") < at("travel"));
}

#[test]
fn the_reader_reads_extras_back_by_node() {
    let found = summary(&bytes(0.003)).unwrap();
    assert_eq!(
        found.nodes[0].extras,
        Some(extras(0.003)[0].clone().unwrap())
    );
    assert_eq!(found.nodes[2].extras, None);
}

#[test]
fn diff_reports_a_changed_travel_by_node_and_key() {
    let a = summary(&bytes(0.003)).unwrap();
    let b = summary(&bytes(0.004)).unwrap();
    let report = diff(&a, &b);
    assert!(report.differs());
    assert_eq!(report.nodes.len(), 1);
    assert_eq!(report.nodes[0].name, "mv_knob");
    assert_eq!(
        report.nodes[0].marks,
        ["extras manfred.travel 0.003 -> 0.004"]
    );
    let text = report.to_string();
    assert!(text.contains("~ mv_knob"));
    assert!(text.contains("extras manfred.travel 0.003 -> 0.004"));
    assert!(!diff(&a, &a).differs());
}

#[test]
fn diff_reports_a_key_only_one_side_has() {
    let with = summary(&bytes(0.003)).unwrap();
    let mut fewer = extras(0.003);
    fewer[1] = None;
    let without = summary(&write_with_extras(&nodes(), &fewer, Order::File, GENERATOR)).unwrap();
    let report = diff(&with, &without);
    let names: Vec<&str> = report.nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["mv_knob__cap"]);
    assert!(
        report.nodes[0]
            .marks
            .contains(&"extras manfred.group only in a".to_string())
    );
    let back = diff(&without, &with);
    assert!(
        back.nodes[0]
            .marks
            .contains(&"extras manfred.group only in b".to_string())
    );
}

#[test]
fn diff_leaves_extras_out_when_one_file_carries_none() {
    let with = summary(&bytes(0.003)).unwrap();
    let blender = summary(&write(&nodes(), Order::File, GENERATOR)).unwrap();
    assert!(!diff(&with, &blender).differs());
    assert!(!diff(&blender, &with).differs());
}
