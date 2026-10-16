use manfred_build::Registry;
use manfred_build::gltf_io::{self, GLTF_PATH};
use serde_json::{Value, json};
use std::path::PathBuf;

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
        if let Some(t) = target {
            view["target"] = json!(t);
        }
        self.data.extend_from_slice(bytes);
        self.views.push(view);
        self.views.len() - 1
    }

    fn floats(
        &mut self,
        values: &[f32],
        kind: &str,
        width: usize,
        target: Option<u32>,
        bounds: bool,
    ) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes, target);
        let mut accessor = json!({
            "bufferView": view,
            "componentType": 5126,
            "count": values.len() / width,
            "type": kind,
        });
        if bounds {
            let column = |i: usize| values.iter().skip(i).step_by(width).copied();
            accessor["min"] = json!(
                (0..width)
                    .map(|i| column(i).fold(f32::INFINITY, f32::min))
                    .collect::<Vec<_>>()
            );
            accessor["max"] = json!(
                (0..width)
                    .map(|i| column(i).fold(f32::NEG_INFINITY, f32::max))
                    .collect::<Vec<_>>()
            );
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

fn corners(lo: [f32; 3], hi: [f32; 3]) -> Vec<f32> {
    (0..8)
        .flat_map(|i| {
            [
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            ]
        })
        .collect()
}

fn box_indices() -> Vec<u16> {
    [
        [0, 2, 6, 4],
        [1, 5, 7, 3],
        [0, 4, 5, 1],
        [2, 3, 7, 6],
        [0, 1, 3, 2],
        [4, 6, 7, 5],
    ]
    .iter()
    .flat_map(|[a, b, c, d]| [*a, *b, *c, *a, *c, *d])
    .collect()
}

fn png() -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer
        .write_image_data(&[
            200, 16, 46, 255, 180, 10, 40, 255, 180, 10, 40, 255, 200, 16, 46, 255,
        ])
        .unwrap();
    writer.finish().unwrap();
    out
}

fn blender_style() -> (Vec<u8>, Vec<u8>) {
    let mut bin = Bin {
        data: Vec::new(),
        views: Vec::new(),
        accessors: Vec::new(),
    };
    let body = bin.floats(
        &corners([-0.8, -0.3, -2.0], [0.8, 0.3, 2.0]),
        "VEC3",
        3,
        Some(34962),
        true,
    );
    let uv: Vec<f32> = (0..8)
        .flat_map(|i| [(i & 1) as f32, ((i >> 2) & 1) as f32])
        .collect();
    let uv = bin.floats(&uv, "VEC2", 2, Some(34962), false);
    let body_indices = bin.indices(&box_indices());
    let lift: Vec<f32> = (0..8)
        .flat_map(|i| [0.0, if i & 2 == 0 { 0.0 } else { 0.1 }, 0.0])
        .collect();
    let lift = bin.floats(&lift, "VEC3", 3, Some(34962), true);
    let wheel = bin.floats(
        &corners([-0.1, -0.3, -0.3], [0.1, 0.3, 0.3]),
        "VEC3",
        3,
        Some(34962),
        true,
    );
    let wheel_indices = bin.indices(&box_indices());
    let times = bin.floats(&[0.0, 1.0], "SCALAR", 1, None, true);
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let turns = bin.floats(
        &[0.0, 0.0, 0.0, 1.0, half, 0.0, 0.0, half],
        "VEC4",
        4,
        None,
        false,
    );
    let image = bin.view(&png(), None);
    while !bin.data.len().is_multiple_of(4) {
        bin.data.push(0);
    }
    let gltf = json!({
        "asset": { "generator": "Khronos glTF Blender I/O v4.2.70", "version": "2.0" },
        "extensionsUsed": ["KHR_materials_clearcoat"],
        "scene": 0,
        "scenes": [{ "name": "Scene", "nodes": [0] }],
        "nodes": [
            { "name": "Coupe", "children": [1, 2, 3], "extras": { "blender_prop": 1 } },
            { "name": "Body", "mesh": 0, "translation": [0.0, 0.6, 0.0] },
            { "name": "wheel_fl", "mesh": 1, "translation": [0.9, 0.3, 1.3] },
            { "name": "Headlight.L", "translation": [0.6, 0.6, 2.0] }
        ],
        "meshes": [
            {
                "name": "Body",
                "primitives": [{
                    "attributes": { "POSITION": body, "TEXCOORD_0": uv },
                    "indices": body_indices,
                    "material": 0,
                    "targets": [{ "POSITION": lift }]
                }],
                "weights": [0.0],
                "extras": { "targetNames": ["Lift"] }
            },
            {
                "name": "Wheel",
                "primitives": [{ "attributes": { "POSITION": wheel }, "indices": wheel_indices }]
            }
        ],
        "materials": [{
            "name": "Paint",
            "pbrMetallicRoughness": {
                "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                "baseColorTexture": { "index": 0 },
                "metallicFactor": 0.0,
                "roughnessFactor": 0.4
            },
            "extensions": { "KHR_materials_clearcoat": { "clearcoatFactor": 1.0 } }
        }],
        "textures": [{ "sampler": 0, "source": 0 }],
        "samplers": [{ "magFilter": 9729, "minFilter": 9987 }],
        "images": [{ "name": "paint", "mimeType": "image/png", "bufferView": image }],
        "animations": [{
            "name": "Spin",
            "channels": [{ "sampler": 0, "target": { "node": 2, "path": "rotation" } }],
            "samplers": [{ "input": times, "output": turns, "interpolation": "LINEAR" }]
        }],
        "accessors": bin.accessors,
        "bufferViews": bin.views,
        "buffers": [{ "byteLength": bin.data.len() }]
    });
    (serde_json::to_vec(&gltf).unwrap(), bin.data)
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("gltf_io");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn a_blender_style_glb_round_trips_through_a_manf_without_extras() {
    let (json_bytes, bin) = blender_style();
    let sample = gltf_io::join(&json_bytes, &bin);
    std::fs::write(scratch("sample.glb"), &sample).unwrap();
    let out = scratch("coupe.manf");
    let imported =
        gltf_io::import_manf(&sample, "vehicle/coupe", &["car".into()], None, &out).unwrap();
    assert!(imported.recipe.contains(&format!("path = \"{GLTF_PATH}\"")));
    assert_eq!(imported.kind, "vehicle");
    assert_eq!(imported.name, "coupe");
    assert_eq!(imported.nodes.get("coupe.wheel_fl"), Some(&2));

    let facts = &imported.facts;
    assert_eq!(facts["size"], json!([1.8, 0.9, 4.0]));
    assert_eq!(facts["licence"], json!(gltf_io::LICENCE));
    assert_eq!(
        facts["wheels"]["fl"],
        json!({ "node": "wheel_fl", "centre": [0.9, 0.3, 1.3], "radius": 0.3, "axle": [1.0, 0.0, 0.0] })
    );
    assert_eq!(facts["anchors"][0]["node"], json!("Headlight.L"));
    assert_eq!(facts["anchors"][0]["kind"], json!("head"));

    let reader = manf::Reader::open(&out).unwrap();
    assert_eq!(
        reader.chunk(manf::GLTF).unwrap().as_deref(),
        Some(&json_bytes[..])
    );
    assert_eq!(reader.chunk(manf::GBIN).unwrap().as_deref(), Some(&bin[..]));
    let fact: toml::Table =
        toml::from_str(std::str::from_utf8(&reader.chunk(manf::FACT).unwrap().unwrap()).unwrap())
            .unwrap();
    assert_eq!(fact["licence"].as_str(), Some(gltf_io::LICENCE));

    let exported = gltf_io::export_glb(&out, &Registry::new(), None).unwrap();
    std::fs::write(scratch("coupe.glb"), &exported).unwrap();
    assert_eq!(gltf_io::extras(&exported).unwrap(), Vec::<String>::new());
    assert_eq!(gltf_io::clean(&exported).unwrap(), exported);
    let parts = gltf_io::split(&exported).unwrap();
    assert_eq!(parts.bin, &bin[..]);
    let mut wanted: Value = serde_json::from_slice(&json_bytes).unwrap();
    wanted["nodes"][0].as_object_mut().unwrap().remove("extras");
    let got: Value = serde_json::from_slice(parts.json).unwrap();
    assert_eq!(got, wanted);
}

#[test]
fn a_recipe_exports_without_manfred_extras() {
    let dir = scratch("recipe");
    std::fs::create_dir_all(&dir).unwrap();
    let recipe = dir.join("pair.toml");
    std::fs::write(
        &recipe,
        "manfred = 1\nname = \"pair\"\n\n[[object]]\nname = \"base\"\nkind = \"box\"\nsize = [1.0, 0.2, 1.0]\nmaterial = \"m\"\n\n[[object]]\nname = \"post\"\nkind = \"box\"\nsize = [0.2, 1.0, 0.2]\nat = [0.0, 0.6, 0.0]\nparent = \"base\"\nmaterial = \"m\"\n\n[materials.m]\nbase = [0.5, 0.5, 0.5]\nroughness = 0.5\n",
    )
    .unwrap();
    let glb = gltf_io::export_glb(&recipe, &Registry::new(), None).unwrap();
    assert_eq!(gltf_io::extras(&glb).unwrap(), Vec::<String>::new());
    let parts = gltf_io::split(&glb).unwrap();
    let json: Value = serde_json::from_slice(parts.json).unwrap();
    let nodes = json["nodes"].as_array().unwrap();
    assert!(nodes.iter().any(|n| n.get("children").is_some()));
}
