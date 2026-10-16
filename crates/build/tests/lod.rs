mod common;

use glam::DMat4;
use manfred_build::build;
use manfred_glb::{Node, Order, Targets};
use manfred_mesh::{Morph, torus};
use manfred_recipe::parse;
use std::path::Path;

fn head(dir: &Path) {
    let mesh = torus(1.0, 0.4, 64, 32);
    let bulge = mesh
        .positions
        .iter()
        .map(|p| [p[0] * 0.1, 0.0, p[2] * 0.1])
        .collect();
    let node = Node {
        name: "head".into(),
        mesh: mesh.clone(),
        transform: DMat4::IDENTITY,
        keep_transform: false,
        targets: Some(Box::new(Targets {
            base: mesh,
            morph: Morph {
                names: vec!["bulge".into()],
                weights: vec![0.0],
                positions: vec![bulge],
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

fn recipe(lod: &str) -> manfred_recipe::Recipe {
    parse(&format!(
        "manfred = 1\nname = \"t\"\n\n[[object]]\nname = \"head\"\nkind = \"mesh\"\npath = \"head.glb\"\nsmooth = true\nmorph = {{ bulge = 1.0 }}\n{lod}"
    ))
    .unwrap()
}

#[test]
fn levels_of_detail_keep_their_targets_morphs_and_a_baked_normal_map_in_the_glb() {
    let dir = common::scratch("lod");
    head(&dir);
    let r = recipe(
        "lod = [{ triangles = 1000, bake_normals = true, normal_size = 64 }, { ratio = 0.1 }]\n",
    );
    let b = build(&r, &dir).unwrap();
    let master = b.parts[0].node.mesh.triangle_count();
    assert_eq!(b.lods.len(), 2);
    for (lod, target) in b
        .lods
        .iter()
        .zip([1000, (master as f64 * 0.1).round() as usize])
    {
        let got = lod.node.mesh.triangle_count();
        assert!(
            got <= target && got + target / 10 >= target,
            "{got} for {target}"
        );
        let targets = lod.node.targets.as_ref().unwrap();
        let deltas = &targets.morph.positions[0];
        assert_eq!(deltas.len(), targets.base.positions.len());
        for (p, d) in targets.base.positions.iter().zip(deltas) {
            assert!((d[0] - p[0] * 0.1).abs() < 1e-6 && (d[2] - p[2] * 0.1).abs() < 1e-6);
        }
    }
    assert_eq!(b.lods[0].node.name, "head.lod1");
    assert!(b.lods[1].normal_map.is_none() && b.lods[1].tangents.is_none());
    let png = b.lods[0].normal_map.as_ref().unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .unwrap();
    let mut texels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut texels).unwrap();
    assert_eq!((info.width, info.height), (64, 64));
    let glb = b.glb();
    assert_eq!(glb, build(&r, &dir).unwrap().glb(), "the bytes repeat");
    pfx_load::Mesh::parse(&glb).unwrap();
    let parsed = gltf::Gltf::from_slice(&glb).unwrap();
    let doc = &parsed.document;
    assert_eq!(doc.scenes().next().unwrap().nodes().count(), 1);
    assert_eq!(doc.nodes().count(), 3);
    assert!(doc.extensions_used().any(|e| e == "MSFT_lod"));
    let json: serde_json::Value = serde_json::from_slice(
        &glb[20..20 + u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize],
    )
    .unwrap();
    assert_eq!(
        json["nodes"][0]["extensions"]["MSFT_lod"]["ids"],
        serde_json::json!([1, 2])
    );
    assert_eq!(json["nodes"][1]["extras"]["lod"]["of"], "head");
    assert_eq!(json["nodes"][2]["extras"]["lod"]["level"], 2);
    let lod1 = doc.nodes().nth(1).unwrap().mesh().unwrap();
    for primitive in lod1.primitives() {
        assert!(primitive.get(&gltf::Semantic::Tangents).is_some());
        assert_eq!(primitive.morph_targets().count(), 1);
        let normal = primitive.material().normal_texture().unwrap();
        let image = normal.texture().source();
        assert!(matches!(
            image.source(),
            gltf::image::Source::View {
                mime_type: "image/png",
                ..
            }
        ));
    }
}

#[test]
fn a_level_must_have_fewer_triangles_than_its_object() {
    let dir = common::scratch("lod-many");
    head(&dir);
    let e = build(&recipe("lod = [{ triangles = 1000000 }]\n"), &dir).unwrap_err();
    assert_eq!(e.path, "object[head].lod[0].triangles");
}
