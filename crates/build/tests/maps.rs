mod common;

use glam::DMat4;
use manfred_build::{Look, Looks, Map, build, image};
use manfred_glb::{Node, Order};
use manfred_mesh::{Mesh, Slot};
use manfred_recipe::parse;
use std::path::Path;

fn png(texel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut texels = Vec::new();
    for y in 0..4 {
        for x in 0..4 {
            texels.extend(texel(x, y));
        }
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 4, 4);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&texels).unwrap();
    writer.finish().unwrap();
    out
}

fn face() -> Vec<u8> {
    png(|x, y| [x as u8 * 60, y as u8 * 60, 90, 255])
}

fn bumps() -> Vec<u8> {
    png(|x, _| [128 + x as u8 * 10, 128, 250, 255])
}

fn head(dir: &Path) {
    let mesh = Mesh {
        positions: vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
        normals: vec![[0.0, 0.0, 1.0]; 4],
        uvs: Some(vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]),
        uvs1: None,
        indices: vec![0, 1, 2, 0, 2, 3],
        slots: vec![Slot {
            material: "skin".into(),
            start: 0,
            count: 6,
        }],
    };
    let node = Node {
        name: "head".into(),
        mesh,
        transform: DMat4::IDENTITY,
        keep_transform: false,
        targets: None,
    };
    let mut looks = Looks::new();
    looks.insert(
        "skin".into(),
        Look {
            base_color: Some([1.0, 0.9, 0.8, 1.0]),
            metallic: Some(0.0),
            roughness: Some(0.7),
            base: Some(Map::new(image(face()).unwrap())),
            normal: Some(Map {
                scale: 0.5,
                ..Map::new(image(bumps()).unwrap())
            }),
            ..Look::default()
        },
    );
    let glb = manfred_glb::write_looks(&[node], &[], &[], &looks, Order::File, "t").unwrap();
    std::fs::write(dir.join("head.glb"), glb).unwrap();
    std::fs::create_dir_all(dir.join("tex")).unwrap();
    std::fs::write(dir.join("tex/face.png"), face()).unwrap();
    std::fs::write(dir.join("tex/bumps.png"), bumps()).unwrap();
    std::fs::write(dir.join("tex/other.png"), png(|_, _| [200, 10, 10, 255])).unwrap();
}

fn recipe(materials: &str) -> manfred_recipe::Recipe {
    parse(&format!(
        "manfred = 1\nname = \"t\"\n\n[[object]]\nname = \"face\"\nkind = \"mesh\"\npath = \"head.glb\"\n\n[materials.skin]\nbase = [1.0, 1.0, 1.0]\n{materials}"
    ))
    .unwrap()
}

fn bytes<'a>(texture: &gltf::Texture, blob: &'a [u8]) -> &'a [u8] {
    match texture.source().source() {
        gltf::image::Source::View { view, mime_type } => {
            assert_eq!(mime_type, "image/png");
            &blob[view.offset()..view.offset() + view.length()]
        }
        gltf::image::Source::Uri { .. } => panic!("the image is not embedded"),
    }
}

#[test]
fn a_mesh_keeps_its_texture_maps_and_the_build_embeds_them() {
    let dir = common::scratch("maps-kept");
    head(&dir);
    let r = recipe("");
    let b = build(&r, &dir).unwrap();
    let glb = b.glb();
    assert!(glb == build(&r, &dir).unwrap().glb());
    let parsed = gltf::Gltf::from_slice(&glb).unwrap();
    let blob = parsed.blob.clone().unwrap();
    assert_eq!(parsed.document.images().count(), 2);
    let skin = parsed
        .document
        .materials()
        .find(|m| m.name() == Some("skin"))
        .unwrap();
    let pbr = skin.pbr_metallic_roughness();
    assert_eq!(pbr.base_color_factor(), [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(pbr.roughness_factor(), 0.5);
    assert_eq!(pbr.metallic_factor(), 0.0);
    let base = pbr.base_color_texture().unwrap();
    assert_eq!(base.tex_coord(), 0);
    assert_eq!(bytes(&base.texture(), &blob), face());
    let normal = skin.normal_texture().unwrap();
    assert_eq!(normal.scale(), 0.5);
    assert_eq!(bytes(&normal.texture(), &blob), bumps());
    assert!(skin.occlusion_texture().is_none());
    assert!(b.parts[0].looks.contains_key("skin"));
}

#[test]
fn a_recipe_map_replaces_an_imported_one_and_the_rest_is_kept() {
    let dir = common::scratch("maps-override");
    head(&dir);
    let b = build(
        &recipe(
            "base_map = \"tex/other.png\"\nnormal_scale = 0.25\ndetail_normal_map = \"tex/bumps.png\"\ndetail_normal_scale = 2.0\n",
        ),
        &dir,
    )
    .unwrap();
    let glb = b.glb();
    let parsed = gltf::Gltf::from_slice(&glb).unwrap();
    let blob = parsed.blob.clone().unwrap();
    assert_eq!(parsed.document.images().count(), 2);
    let skin = parsed.document.materials().next().unwrap();
    let base = skin.pbr_metallic_roughness().base_color_texture().unwrap();
    assert_eq!(
        bytes(&base.texture(), &blob),
        std::fs::read(dir.join("tex/other.png")).unwrap()
    );
    let normal = skin.normal_texture().unwrap();
    assert_eq!(normal.scale(), 0.25);
    assert_eq!(bytes(&normal.texture(), &blob), bumps());
    let extras: serde_json::Value =
        serde_json::from_str(skin.extras().as_ref().unwrap().get()).unwrap();
    let detail = &extras["detailNormalTexture"];
    assert_eq!(detail["texCoord"], 1);
    assert_eq!(detail["scale"], 2.0);
    let texture = parsed
        .document
        .textures()
        .nth(detail["index"].as_u64().unwrap() as usize)
        .unwrap();
    assert_eq!(bytes(&texture, &blob), bumps());
}

#[test]
fn a_missing_a_foreign_or_an_oversized_map_is_refused() {
    let dir = common::scratch("maps-refused");
    head(&dir);
    std::fs::write(dir.join("tex/photo.jpg"), [0xff, 0xd8, 0xff, 0xe0, 0, 0]).unwrap();
    let mut huge = face();
    huge[16..20].copy_from_slice(&20_000u32.to_be_bytes());
    std::fs::write(dir.join("tex/huge.png"), &huge).unwrap();
    for (key, path, said) in [
        ("base_map", "tex/gone.png", "cannot read tex/gone.png"),
        ("normal_map", "tex/photo.jpg", "tex/photo.jpg is not a PNG"),
        (
            "roughness_map",
            "tex/huge.png",
            "a 20000x4 image does not fit",
        ),
    ] {
        let e = build(&recipe(&format!("{key} = \"{path}\"\n")), &dir).unwrap_err();
        assert_eq!(e.path, format!("materials.skin.{key}"));
        assert!(e.text.contains(said), "{}", e.text);
    }
}

#[test]
fn fabric_and_anisotropy_reach_the_glb_as_khr_extensions() {
    let dir = common::scratch("maps-lobes");
    let r = parse(
        "manfred = 1\nname = \"t\"\n\n[[object]]\nname = \"knit\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\nmaterial = \"knit\"\n\n[[object]]\nname = \"hair\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\nmaterial = \"hair\"\n\n[materials.knit]\nbase = [0.25, 0.05, 0.05]\nfabric = { color = [0.8, 0.8, 0.8], roughness = 0.5 }\n\n[materials.hair.anisotropy]\nstrength = 0.8\nrotation = 0.25\n",
    )
    .unwrap();
    let glb = build(&r, &dir).unwrap().glb();
    let parsed = gltf::Gltf::from_slice(&glb).unwrap();
    let used: Vec<&str> = parsed.document.extensions_used().collect();
    assert!(used.contains(&"KHR_materials_sheen"));
    assert!(used.contains(&"KHR_materials_anisotropy"));
    let length = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
    let root: serde_json::Value = serde_json::from_slice(&glb[20..20 + length]).unwrap();
    let material = |name: &str| {
        root["materials"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"] == name)
            .unwrap()
            .clone()
    };
    let knit = material("knit");
    let sheen = &knit["extensions"]["KHR_materials_sheen"];
    assert_eq!(
        sheen["sheenColorFactor"],
        serde_json::json!([0.8, 0.8, 0.8])
    );
    assert_eq!(sheen["sheenRoughnessFactor"], 0.5);
    assert_eq!(
        knit["pbrMetallicRoughness"]["baseColorFactor"],
        serde_json::json!([0.25, 0.05, 0.05, 1.0])
    );
    let hair = material("hair");
    let anisotropy = &hair["extensions"]["KHR_materials_anisotropy"];
    assert_eq!(anisotropy["anisotropyStrength"], 0.8);
    assert_eq!(anisotropy["anisotropyRotation"], 0.25);
}
