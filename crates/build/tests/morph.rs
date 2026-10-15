mod common;

use glam::DMat4;
use manfred_build::build;
use manfred_glb::{Node, Order, Targets};
use manfred_mesh::{Mesh, Morph};
use manfred_recipe::parse;
use std::path::Path;

fn head(dir: &Path) {
    let mut positions = Vec::new();
    for y in [-1.0f32, 0.0, 1.0] {
        for x in [-1.0f32, 0.0, 1.0] {
            positions.push([x, y, 0.0]);
        }
    }
    let mut indices = Vec::new();
    for j in 0..2u32 {
        for i in 0..2u32 {
            let a = j * 3 + i;
            indices.extend([a, a + 1, a + 4, a, a + 4, a + 3]);
        }
    }
    let up = positions
        .iter()
        .map(|p| {
            if p[0] == 0.0 && p[1] == 0.0 {
                [0.0, 0.0, 0.5]
            } else {
                [0.0; 3]
            }
        })
        .collect();
    let wide = positions.iter().map(|p| [p[0] * 0.5, 0.0, 0.0]).collect();
    let mesh = Mesh {
        normals: vec![[0.0, 0.0, 1.0]; positions.len()],
        positions,
        uvs: None,
        uvs1: None,
        indices,
        slots: Vec::new(),
    };
    let node = Node {
        name: "head".into(),
        mesh: mesh.clone(),
        transform: DMat4::IDENTITY,
        keep_transform: false,
        targets: Some(Box::new(Targets {
            base: mesh,
            morph: Morph {
                names: vec!["up".into(), "wide".into()],
                weights: vec![0.0, 0.5],
                positions: vec![up, wide],
                normals: Vec::new(),
            },
        })),
    };
    std::fs::write(
        dir.join("head.glb"),
        manfred_glb::write(&[node], Order::File, "t"),
    )
    .unwrap();
}

fn recipe(morph: &str) -> manfred_recipe::Recipe {
    parse(&format!(
        "manfred = 1\nname = \"t\"\n\n[[object]]\nname = \"face\"\nkind = \"mesh\"\npath = \"head.glb\"\nat = [0.0, 2.0, 0.0]\nsmooth = true\n{morph}"
    ))
    .unwrap()
}

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-5)
}

#[test]
fn a_mesh_keeps_its_morph_targets_and_the_recipe_weights_pose_what_is_staged() {
    let dir = common::scratch("morph");
    head(&dir);
    let b = build(&recipe("morph = { up = 1.0 }\n"), &dir).unwrap();
    let glb = b.glb();
    pfx_load::Mesh::parse(&glb).unwrap();
    let parsed = gltf::Gltf::from_slice(&glb).unwrap();
    let blob = parsed.blob.clone().unwrap();
    let mesh = parsed.document.meshes().next().unwrap();
    assert_eq!(mesh.weights(), Some(&[1.0f32, 0.5][..]));
    let extras: serde_json::Value =
        serde_json::from_str(mesh.extras().as_ref().unwrap().get()).unwrap();
    assert_eq!(extras["targetNames"], serde_json::json!(["up", "wide"]));
    for primitive in mesh.primitives() {
        let targets: Vec<_> = primitive.morph_targets().collect();
        assert_eq!(targets.len(), 2);
        for target in &targets {
            let positions = target.positions().unwrap();
            assert!(positions.min().is_some() && positions.max().is_some());
            assert!(target.normals().is_some());
        }
        let up = targets[0].positions().unwrap();
        assert_eq!(up.max().unwrap(), serde_json::json!([0.0, 0.0, 0.5]));
        let reader = primitive.reader(|_| Some(&blob[..]));
        let base: Vec<[f32; 3]> = reader.read_positions().unwrap().collect();
        let deltas: Vec<Vec<[f32; 3]>> = reader
            .read_morph_targets()
            .map(|(p, _, _)| p.unwrap().collect())
            .collect();
        assert!(base.len() >= 9);
        for (v, p) in base.iter().enumerate() {
            assert!(p[2] == 0.0 && (p[1] - 2.0).abs() <= 1.0, "{p:?} is morphed");
            let centre = p[0] == 0.0 && p[1] == 2.0;
            let up = if centre { [0.0, 0.0, 0.5] } else { [0.0; 3] };
            assert!(near(deltas[0][v], up), "{v}: {:?}", deltas[0][v]);
            assert!(near(deltas[1][v], [p[0] * 0.5, 0.0, 0.0]), "{v}");
        }
    }
    let staged: Vec<[f32; 3]> = manfred_scene::pieces(&b.parts)
        .into_iter()
        .flat_map(|piece| piece.geometry.positions)
        .collect();
    assert!(staged.iter().any(|p| near(*p, [0.0, 2.0, 0.5])));
    assert!(staged.iter().any(|p| near(*p, [1.25, 3.0, 0.0])));
    assert!(!staged.iter().any(|p| near(*p, [0.0, 2.0, 0.0])));
    assert!(!staged.iter().any(|p| near(*p, [1.0, 3.0, 0.0])));
}

#[test]
fn a_morph_target_the_file_lacks_is_refused_by_name() {
    let dir = common::scratch("morph-unknown");
    head(&dir);
    assert_eq!(
        build(&recipe("morph = { smile = 1.0 }\n"), &dir)
            .unwrap_err()
            .to_string(),
        "object[face].morph.smile: head.glb has no morph target `smile`, expected one of: up, wide"
    );
}
