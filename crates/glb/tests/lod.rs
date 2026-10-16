use glam::{DMat4, DVec3};
use manfred_glb::{Lod, Node, Order, write_tree};
use manfred_mesh::{Mesh, rounded_box, torus};
use serde_json::{Value, json};

fn two_slots() -> Mesh {
    let mut ring = torus(1.0, 0.3, 24, 12);
    ring.set_material("paint");
    let mut cap = rounded_box([0.2, 0.2, 0.2], 0.0, 1);
    cap.set_material("chrome");
    let mut tail = torus(0.5, 0.1, 12, 6);
    tail.set_material("rubber");
    ring.append(&cap);
    ring.append(&tail);
    ring
}

fn json_of(glb: &[u8]) -> Value {
    let length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&glb[20..20 + length]).unwrap()
}

#[test]
fn a_level_has_its_bases_transform_and_one_primitive_a_slot_in_the_same_order() {
    let base = Node {
        name: "car".into(),
        mesh: two_slots(),
        transform: DMat4::from_translation(DVec3::new(1.0, 2.0, 3.0)),
        keep_transform: true,
        targets: None,
    };
    let mut level = base.mesh.clone();
    let chrome = level.slots.remove(1);
    let gone = chrome.start as usize..(chrome.start + chrome.count) as usize;
    level.indices.drain(gone);
    level.slots[1].start -= chrome.count;
    let lod = Lod {
        of: 0,
        node: Node {
            name: "car.lod1".into(),
            mesh: level,
            ..base.clone()
        },
        tangents: None,
        normal_map: None,
        extras: json!({ "lod": { "of": "car", "level": 1 } }),
    };
    let glb = write_tree(&[base], &[], &[], &[lod], Order::File, "t").unwrap();
    let doc = json_of(&glb);
    let nodes = doc["nodes"].as_array().unwrap();
    assert_eq!(nodes[1]["matrix"], nodes[0]["matrix"]);
    let materials = |node: &Value| -> Vec<String> {
        let mesh = &doc["meshes"][node["mesh"].as_u64().unwrap() as usize];
        mesh["primitives"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let at = p["material"].as_u64().unwrap() as usize;
                doc["materials"][at]["name"].as_str().unwrap().to_string()
            })
            .collect()
    };
    assert_eq!(materials(&nodes[0]), ["paint", "chrome", "rubber"]);
    assert_eq!(materials(&nodes[1]), materials(&nodes[0]));
    let chrome = &doc["meshes"][nodes[1]["mesh"].as_u64().unwrap() as usize]["primitives"][1];
    let indices = &doc["accessors"][chrome["indices"].as_u64().unwrap() as usize];
    assert_eq!(indices["count"], 3);
}
