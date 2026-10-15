use glam::DMat4;
use manfred_glb::{Node, Order, diff, summary, write};
use manfred_mesh::Mesh;

fn quad(x: f32, detail: Option<[f32; 2]>) -> Mesh {
    let mut m = Mesh {
        positions: vec![
            [x, 0.0, 0.0],
            [x + 1.0, 0.0, 0.0],
            [x + 1.0, 1.0, 0.0],
            [x, 1.0, 0.0],
        ],
        normals: vec![[0.0, 0.0, 1.0]; 4],
        uvs: Some(vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]),
        uvs1: detail.map(|[lo, hi]| vec![[lo, hi], [hi, hi], [hi, lo], [lo, lo]]),
        indices: vec![0, 1, 2, 0, 2, 3],
        slots: Vec::new(),
    };
    m.set_material("wood");
    m
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

fn scene(detail: Option<[f32; 2]>) -> Vec<u8> {
    write(
        &[
            node("desk", quad(0.0, detail)),
            node("wall", quad(2.0, None)),
        ],
        Order::File,
        "manfred test",
    )
}

#[test]
fn the_writer_emits_texcoord_1_only_where_the_mesh_has_it() {
    let bytes = scene(Some([0.25, 0.75]));
    let (document, buffers, _) = gltf::import_slice(&bytes).unwrap();
    let mut sets = Vec::new();
    for node in document.nodes() {
        let primitive = node.mesh().unwrap().primitives().next().unwrap();
        let reader = primitive.reader(|b| Some(&buffers[b.index()]));
        let first: Vec<[f32; 2]> = reader.read_tex_coords(0).unwrap().into_f32().collect();
        let second: Option<Vec<[f32; 2]>> =
            reader.read_tex_coords(1).map(|t| t.into_f32().collect());
        assert_eq!(first, [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
        sets.push((node.name().unwrap().to_string(), second));
    }
    assert_eq!(sets[0].0, "desk");
    assert_eq!(
        sets[0].1.as_deref(),
        Some(&[[0.25, 0.75], [0.75, 0.75], [0.75, 0.25], [0.25, 0.25]][..])
    );
    assert_eq!(sets[1], ("wall".to_string(), None));
}

#[test]
fn a_mesh_without_texcoord_1_writes_the_bytes_it_always_did() {
    let mut plain = quad(0.0, None);
    plain.uvs1 = None;
    let a = write(&[node("desk", plain.clone())], Order::File, "manfred test");
    let b = write(
        &[node("desk", quad(0.0, None))],
        Order::File,
        "manfred test",
    );
    assert_eq!(a, b);
    let text = String::from_utf8_lossy(&a);
    assert!(!text.contains("TEXCOORD_1"));
    assert!(String::from_utf8_lossy(&scene(Some([0.0, 0.5]))).contains("TEXCOORD_1"));
}

#[test]
fn the_summary_reports_texcoord_1_and_its_area_use() {
    let s = summary(&scene(Some([0.25, 0.75]))).unwrap();
    assert!(s.nodes[0].uvs && s.nodes[0].uvs1);
    assert!((s.nodes[0].uv1_area - 0.25).abs() < 1e-9);
    assert!(s.nodes[1].uvs && !s.nodes[1].uvs1);
    assert_eq!(s.nodes[1].uv1_area, 0.0);
}

#[test]
fn manfred_diff_marks_texcoord_1_presence_and_area_use() {
    let without = summary(&scene(None)).unwrap();
    let with = summary(&scene(Some([0.25, 0.75]))).unwrap();
    let report = diff(&without, &with);
    assert!(report.differs());
    assert_eq!(report.nodes.len(), 1);
    assert_eq!(report.nodes[0].name, "desk");
    assert_eq!(
        report.nodes[0].marks,
        ["uvs1 false -> true", "uv1 area use 0.00% -> 25.00%"]
    );
    let wider = summary(&scene(Some([0.0, 1.0]))).unwrap();
    assert_eq!(
        diff(&with, &wider).nodes[0].marks,
        ["uv1 area use 25.00% -> 100.00%"]
    );
    assert!(!diff(&with, &with).differs());
    let text = diff(&without, &with).to_string();
    assert!(text.contains("~ desk\n    uvs1 false -> true\n"), "{text}");
}
