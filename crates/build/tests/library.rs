mod common;

use glam::{DQuat, DVec3};
use manfred_build::library::{self, LICENCE, Sidecar};
use manfred_build::{Built, build, matrix};
use manfred_recipe::{Recipe, parse};
use serde_json::{Value, json};
use std::path::Path;

const CAR: &str = r##"manfred = 1
name = "t"
seed = 3

[[object]]
name = "hero"
kind = "generator"
generator = "manfred/car"
params = { style = "sports", paint = "#c8102e" }
at = [3.0, 0.0, 1.0]
rotate = [0.0, 30.0, 0.0]
"##;

fn car() -> (Recipe, Built) {
    let r = parse(CAR).unwrap();
    let b = build(&r, Path::new(".")).unwrap();
    (r, b)
}

fn put(root: &Path, as_name: &str, exported: &library::Exported) {
    let dir = root.join(as_name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(library::MODEL), &exported.glb).unwrap();
    std::fs::write(dir.join(library::SIDECAR), exported.sidecar.text()).unwrap();
}

fn close(a: DVec3, b: DVec3, tolerance: f64) -> bool {
    (a - b).abs().max_element() < tolerance
}

#[test]
fn an_exported_car_has_its_wheels_at_their_axle_pivots_and_no_manfred_extras() {
    let (r, b) = car();
    let e = library::export(&r, &b, "hero", "car/sports").unwrap();
    assert_eq!(
        library::manfred_extras(&e.glb).unwrap(),
        Vec::<String>::new()
    );
    let gltf = gltf::Gltf::from_slice(&e.glb).unwrap();
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .unwrap();
    let roots: Vec<_> = scene.nodes().collect();
    assert_eq!(roots.len(), 1);
    let root = &roots[0];
    assert_eq!(root.name(), Some("sports"));
    assert!(root.mesh().is_some());
    let children: Vec<_> = root.children().collect();
    let names: Vec<&str> = children.iter().filter_map(|c| c.name()).collect();
    for want in [
        "glass",
        "wheel_fl",
        "wheel_fr",
        "wheel_rl",
        "wheel_rr",
        "headlights",
        "taillights",
    ] {
        assert!(names.contains(&want), "{want} in {names:?}");
    }
    let wheels = &e.sidecar.facts["wheels"];
    for corner in ["fl", "fr", "rl", "rr"] {
        let node = children
            .iter()
            .find(|c| c.name() == Some(&format!("wheel_{corner}")))
            .unwrap();
        let (t, rot, s) = node.transform().decomposed();
        let centre = &wheels[corner]["centre"];
        let want = DVec3::new(
            centre[0].as_f64().unwrap(),
            centre[1].as_f64().unwrap(),
            centre[2].as_f64().unwrap(),
        );
        assert!(close(DVec3::from(t.map(f64::from)), want, 1e-5), "{corner}");
        assert_eq!(rot, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(s, [1.0, 1.0, 1.0]);
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = DVec3::splat(f64::NEG_INFINITY);
        for p in node.mesh().unwrap().primitives() {
            let bb = p.bounding_box();
            lo = lo.min(DVec3::from(bb.min.map(f64::from)));
            hi = hi.max(DVec3::from(bb.max.map(f64::from)));
        }
        let mid = (lo + hi) * 0.5;
        assert!(mid.y.abs() < 1e-3 && mid.z.abs() < 1e-3, "{corner}: {mid}");
    }
    assert!(e.sidecar.facts["size"][2].as_f64().unwrap() > 4.0);
    let (json, _) = library::split_glb(&e.glb).unwrap();
    let glass = json["materials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "car_glass")
        .unwrap();
    assert!(glass["extensions"]["KHR_materials_transmission"].is_object());
    assert_eq!(e.sidecar.licence, LICENCE);
    assert_eq!(Sidecar::parse(&e.sidecar.text()).unwrap(), e.sidecar);
}

#[test]
fn a_library_car_builds_and_its_wheels_spin_about_their_own_nodes_under_follow() {
    let (r, b) = car();
    let exported = library::export(&r, &b, "hero", "car/sports").unwrap();
    let dir = common::scratch("library-follow");
    let root = dir.join("library");
    put(&root, "car/sports", &exported);
    library::write_index(&root).unwrap();
    let text = r#"manfred = 1
name = "drive"

[timeline]
fps = 10
frames = 10

[[route]]
name = "main"
points = [[0.0, 0.0, 0.0], [0.0, 0.0, 100.0]]

[[object]]
name = "hero"
kind = "library"
object = "car/sports"

[[follow]]
object = "hero"
route = "main"
speed = [[0.0, 10.0]]
"#;
    let r = parse(text).unwrap();
    let built = build(&r, &dir).unwrap();
    let names: Vec<&str> = built.parts.iter().map(|p| p.node.name.as_str()).collect();
    assert!(names.contains(&"hero.body"), "{names:?}");
    assert!(names.contains(&"hero.wheel_rl"), "{names:?}");
    let track = |node: &str| built.tracks.iter().find(|t| t.node == node).unwrap();
    let wheel = track("hero.wheel_rl");
    let body = track("hero.body");
    assert_eq!(wheel.keys.len(), 11);
    assert_ne!(wheel.keys[0].rotation, wheel.keys[5].rotation);
    let c = &exported.sidecar.facts["wheels"]["rl"]["centre"];
    let centre = DVec3::new(
        c[0].as_f64().unwrap(),
        c[1].as_f64().unwrap(),
        c[2].as_f64().unwrap(),
    );
    for (w, b) in wheel.keys.iter().zip(&body.keys) {
        let at = matrix(w).transform_point3(centre);
        let want = matrix(b).transform_point3(centre);
        assert!(close(at, want, 1e-6), "{at} {want}");
    }
}

#[test]
fn the_index_is_sorted_and_the_same_bytes_every_time() {
    let (r, b) = car();
    let dir = common::scratch("library-index");
    for name in ["vehicle/zed", "car/sports", "car/alpha"] {
        let e = library::export(&r, &b, "hero", name).unwrap();
        put(&dir, name, &e);
    }
    let path = library::write_index(&dir).unwrap();
    let first = std::fs::read(&path).unwrap();
    library::write_index(&dir).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), first);
    let entries = library::read_index(&dir).unwrap();
    let order: Vec<String> = entries
        .iter()
        .map(|e| format!("{}/{}", e.kind, e.name))
        .collect();
    assert_eq!(order, ["car/alpha", "car/sports", "vehicle/zed"]);
    assert!(entries.iter().all(|e| e.sha256.contains_key("model")));
    assert_eq!(library::check(&dir), []);
    std::fs::write(dir.join("car/alpha").join(library::SIDECAR), "nonsense").unwrap();
    assert!(!library::check(&dir).is_empty());
}

fn png() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ])
            .unwrap();
    }
    out
}

struct Bin {
    data: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Bin {
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.data.len().is_multiple_of(4) {
            self.data.push(0);
        }
        let mut view =
            json!({ "buffer": 0, "byteOffset": self.data.len(), "byteLength": bytes.len() });
        if let Some(target) = target {
            view["target"] = json!(target);
        }
        self.data.extend_from_slice(bytes);
        self.views.push(view);
        self.views.len() - 1
    }

    fn floats(&mut self, values: &[f32], kind: &str, target: Option<u32>, bounds: bool) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes, target);
        let width = match kind {
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            _ => 1,
        };
        let mut accessor = json!({
            "bufferView": view,
            "componentType": 5126,
            "count": values.len() / width,
            "type": kind,
        });
        if bounds {
            let mut lo = vec![f32::INFINITY; width];
            let mut hi = vec![f32::NEG_INFINITY; width];
            for chunk in values.chunks(width) {
                for (i, v) in chunk.iter().enumerate() {
                    lo[i] = lo[i].min(*v);
                    hi[i] = hi[i].max(*v);
                }
            }
            accessor["min"] = json!(lo);
            accessor["max"] = json!(hi);
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    fn indices(&mut self, values: &[u16]) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(34963));
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": 5123,
            "count": values.len(),
            "type": "SCALAR",
        }));
        self.accessors.len() - 1
    }
}

fn quad(bin: &mut Bin, half: f32) -> (usize, usize, usize, usize) {
    let positions = [
        -half, -half, 0.0, half, -half, 0.0, half, half, 0.0, -half, half, 0.0,
    ];
    let normals = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
    let uvs = [0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0];
    let p = bin.floats(&positions, "VEC3", Some(34962), true);
    let n = bin.floats(&normals, "VEC3", Some(34962), false);
    let t = bin.floats(&uvs, "VEC2", Some(34962), false);
    let i = bin.indices(&[0, 1, 2, 0, 2, 3]);
    (p, n, t, i)
}

fn blender_style() -> Vec<u8> {
    let mut bin = Bin {
        data: Vec::new(),
        views: Vec::new(),
        accessors: Vec::new(),
    };
    let (bp, bn, bt, bi) = quad(&mut bin, 0.5);
    let smile = bin.floats(
        &[0.0, 0.1, 0.0, 0.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        "VEC3",
        Some(34962),
        true,
    );
    let (wp, wn, wt, wi) = quad(&mut bin, 0.3);
    let image = bin.view(&png(), None);
    let half = std::f32::consts::FRAC_PI_4;
    let times = bin.floats(&[0.0, 0.5, 1.0], "SCALAR", None, true);
    let quats = bin.floats(
        &[
            0.0,
            0.0,
            0.0,
            1.0,
            half.sin(),
            0.0,
            0.0,
            half.cos(),
            1.0,
            0.0,
            0.0,
            0.0,
        ],
        "VEC4",
        None,
        false,
    );
    let json = json!({
        "asset": { "version": "2.0", "generator": "Khronos glTF Blender I/O" },
        "extensionsUsed": ["KHR_materials_clearcoat"],
        "scene": 0,
        "scenes": [{ "name": "Scene", "nodes": [0] }],
        "nodes": [
            { "name": "Rig", "children": [1, 2] },
            { "name": "Body", "mesh": 0, "translation": [0.0, 0.5, 0.0] },
            { "name": "Wheel", "mesh": 1, "translation": [0.8, 0.3, 0.0] },
        ],
        "meshes": [
            {
                "name": "Body",
                "primitives": [{
                    "attributes": { "POSITION": bp, "NORMAL": bn, "TEXCOORD_0": bt },
                    "indices": bi,
                    "material": 0,
                    "targets": [{ "POSITION": smile }],
                }],
                "weights": [0.0],
                "extras": { "targetNames": ["Smile"] },
            },
            {
                "name": "Wheel",
                "primitives": [{
                    "attributes": { "POSITION": wp, "NORMAL": wn, "TEXCOORD_0": wt },
                    "indices": wi,
                    "material": 1,
                }],
            },
        ],
        "materials": [
            {
                "name": "Paint",
                "pbrMetallicRoughness": {
                    "baseColorTexture": { "index": 0 },
                    "metallicFactor": 0.0,
                    "roughnessFactor": 0.4,
                },
                "extensions": {
                    "KHR_materials_clearcoat": { "clearcoatFactor": 1.0, "clearcoatRoughnessFactor": 0.05 },
                },
            },
            {
                "name": "Rubber",
                "pbrMetallicRoughness": {
                    "baseColorFactor": [0.02, 0.02, 0.02, 1.0],
                    "metallicFactor": 0.0,
                    "roughnessFactor": 0.9,
                },
            },
        ],
        "textures": [{ "sampler": 0, "source": 0 }],
        "samplers": [{ "magFilter": 9729, "minFilter": 9987 }],
        "images": [{ "name": "paint", "mimeType": "image/png", "bufferView": image }],
        "animations": [{
            "name": "Spin",
            "channels": [{ "sampler": 0, "target": { "node": 2, "path": "rotation" } }],
            "samplers": [{ "input": times, "output": quats, "interpolation": "LINEAR" }],
        }],
        "accessors": bin.accessors,
        "bufferViews": bin.views,
        "buffers": [{ "byteLength": bin.data.len().next_multiple_of(4) }],
    });
    library::join_glb(&json, &bin.data)
}

#[test]
fn a_blender_style_glb_imports_with_its_hierarchy_materials_morphs_and_animation_and_round_trips() {
    let dir = common::scratch("library-blender");
    let root = dir.join("library");
    let folder = root.join("prop/cart");
    std::fs::create_dir_all(&folder).unwrap();
    let source = blender_style();
    assert!(gltf::Gltf::from_slice(&source).is_ok());
    std::fs::write(folder.join(library::MODEL), &source).unwrap();
    let sidecar = Sidecar {
        kind: "prop".into(),
        name: "cart".into(),
        tags: vec!["test".into()],
        licence: LICENCE.into(),
        generator: None,
        params: Default::default(),
        manfred: "1.3.0".into(),
        facts: Default::default(),
    };
    std::fs::write(folder.join(library::SIDECAR), sidecar.text()).unwrap();
    library::write_index(&root).unwrap();
    let text = r#"manfred = 1
name = "cart"

[timeline]
fps = 10
frames = 10

[[object]]
name = "cart"
kind = "library"
object = "prop/cart"
at = [2.0, 0.0, 0.0]
"#;
    let r = parse(text).unwrap();
    let built = build(&r, &dir).unwrap();
    let names: Vec<&str> = built.parts.iter().map(|p| p.node.name.as_str()).collect();
    assert_eq!(names, ["cart.Body", "cart.Wheel"]);
    let body = &built.parts[0].node;
    assert_eq!(
        body.targets.as_ref().map(|t| t.morph.names.clone()),
        Some(vec!["Smile".to_string()])
    );
    assert!(built.looks.get("Paint").is_some_and(|l| l.base.is_some()));
    assert_eq!(built.materials["Paint"]["family"], "lacquer");
    let rough = built.materials["Rubber"]["roughness"].as_f64().unwrap();
    assert!((rough - 0.9).abs() < 1e-6);
    let wheel = built
        .tracks
        .iter()
        .find(|t| t.node == "cart.Wheel")
        .unwrap();
    assert_eq!(wheel.keys.len(), 11);
    let pivot = DVec3::new(2.8, 0.3, 0.0);
    let half = matrix(&wheel.keys[5]);
    assert!(close(half.transform_point3(pivot), pivot, 1e-5));
    let turned = half.transform_vector3(DVec3::Y);
    let want = DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2) * DVec3::Y;
    assert!(close(turned, want, 1e-5), "{turned}");
    assert!(built.tracks.iter().all(|t| t.node != "cart.Body"));
    let glb = built.try_glb().unwrap();
    assert!(gltf::Gltf::from_slice(&glb).is_ok());

    let e = library::export(&r, &built, "cart", "prop/cart2").unwrap();
    assert_eq!(
        library::manfred_extras(&e.glb).unwrap(),
        Vec::<String>::new()
    );
    let out = gltf::Gltf::from_slice(&e.glb).unwrap();
    let scene = out.scenes().next().unwrap();
    let top: Vec<_> = scene.nodes().collect();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].name(), Some("cart2"));
    let kids: Vec<_> = top[0].children().collect();
    let find = |n: &str| kids.iter().find(|k| k.name() == Some(n)).unwrap();
    assert_eq!(find("Body").transform().decomposed().0, [0.0, 0.5, 0.0]);
    assert_eq!(find("Wheel").transform().decomposed().0, [0.8, 0.3, 0.0]);
    let body_mesh = find("Body").mesh().unwrap();
    assert_eq!(
        body_mesh
            .primitives()
            .next()
            .unwrap()
            .morph_targets()
            .count(),
        1
    );
    let (json, _) = library::split_glb(&e.glb).unwrap();
    assert_eq!(
        json["meshes"][body_mesh.index()]["extras"]["targetNames"],
        json!(["Smile"])
    );
    assert_eq!(out.images().count(), 1);
    let paint = json["materials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "Paint")
        .unwrap();
    assert!(paint["extensions"]["KHR_materials_clearcoat"].is_object());
    let animation = out.animations().next().unwrap();
    assert_eq!(animation.name(), Some("Spin"));
    let channel = animation.channels().next().unwrap();
    assert_eq!(channel.target().node().name(), Some("Wheel"));
    assert_eq!(
        channel.target().property(),
        gltf::animation::Property::Rotation
    );
}
