use std::path::{Path, PathBuf};
use std::process::ExitCode;

use glam::{DMat4, DVec3};
use manfred_build::{Registry, build_with, library};
use serde_json::{Map, Value, json};

use super::library_bake::{Subject, entry_name, quoted, thumbnail};
use super::refuse;

const VALIDATOR: &str = "gltf_validator";
const LIGHTING: &str = "\n[sun]\nmodel = \"authored\"\ntoward = [-0.5, 0.55, 0.65]\nradius_deg = 0.8\ncolor = [1.0, 0.95, 0.88]\nirradiance = 3.0\n\n[sky]\nkind = \"analytic\"\nintensity = 0.9\n\n[look]\nexposure = 1.0\n";

#[derive(clap::Args, Debug, PartialEq)]
pub(super) struct Args {
    #[arg(value_name = "file.glb", help = "A glTF 2.0 binary file to bring in")]
    file: PathBuf,
    #[arg(
        long = "as",
        value_name = "kind/name",
        value_parser = entry_name,
        help = "Where it goes in the library: library/<kind>/<name>"
    )]
    entry: String,
    #[arg(
        long,
        help = "Also render a 512x512 thumbnail, thumb.png, on the live renderer"
    )]
    thumb: bool,
    #[arg(
        long,
        value_name = "a,b",
        value_delimiter = ',',
        help = "Tags for its object.toml and the index"
    )]
    tags: Vec<String>,
    #[arg(
        long,
        value_name = "dir",
        default_value = "library",
        help = "The library folder"
    )]
    library: PathBuf,
    #[arg(long, help = "Answer with one JSON object")]
    json: bool,
}

pub struct Added {
    pub dir: PathBuf,
    pub model: PathBuf,
    pub sidecar: PathBuf,
    pub facts: Map<String, Value>,
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn basics(bytes: &[u8]) -> Result<gltf::Gltf, String> {
    if bytes.len() < 12 || &bytes[..4] != b"glTF" {
        return Err("not a binary glTF file (no glTF magic)".into());
    }
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version != 2 {
        return Err(format!("binary glTF version {version}, expected 2"));
    }
    let parsed = gltf::Gltf::from_slice(bytes).map_err(|e| format!("does not parse: {e}"))?;
    if !parsed.document.as_json().asset.version.starts_with("2.") {
        return Err(format!(
            "asset.version {}, expected 2.x",
            parsed.document.as_json().asset.version
        ));
    }
    Ok(parsed)
}

fn validated(file: &Path) -> Result<(), String> {
    let output = match std::process::Command::new(VALIDATOR)
        .arg("-o")
        .arg(file)
        .output()
    {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("{VALIDATOR}: {e}")),
    };
    let report: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("{VALIDATOR}: its report does not read: {e}"))?;
    let errors = report
        .pointer("/issues/numErrors")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if errors == 0 {
        return Ok(());
    }
    let first: Vec<String> = report
        .pointer("/issues/messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| m.get("severity").and_then(Value::as_u64) == Some(0))
        .take(3)
        .map(|m| {
            format!(
                "{} {}",
                m.get("code").and_then(Value::as_str).unwrap_or(""),
                m.get("message").and_then(Value::as_str).unwrap_or("")
            )
        })
        .collect();
    Err(format!(
        "{VALIDATOR} found {errors} error(s): {}",
        first.join("; ")
    ))
}

type Bounds = Option<(DVec3, DVec3)>;

fn union(a: Bounds, b: Bounds) -> Bounds {
    match (a, b) {
        (Some((alo, ahi)), Some((blo, bhi))) => Some((alo.min(blo), ahi.max(bhi))),
        (a, None) => a,
        (None, b) => b,
    }
}

fn matrix(node: &gltf::Node) -> DMat4 {
    let m = node.transform().matrix();
    DMat4::from_cols_array_2d(&m.map(|col| col.map(f64::from)))
}

fn walk(node: gltf::Node, parent: DMat4, named: &mut Vec<(String, (DVec3, DVec3))>) -> Bounds {
    let world = parent * matrix(&node);
    let mut own: Bounds = None;
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            let b = primitive.bounding_box();
            for k in 0..8 {
                let corner = DVec3::new(
                    f64::from(if k & 1 == 0 { b.min[0] } else { b.max[0] }),
                    f64::from(if k & 2 == 0 { b.min[1] } else { b.max[1] }),
                    f64::from(if k & 4 == 0 { b.min[2] } else { b.max[2] }),
                );
                let at = world.transform_point3(corner);
                own = union(own, Some((at, at)));
            }
        }
    }
    for child in node.children() {
        own = union(own, walk(child, world, named));
    }
    if let (Some(name), Some(b)) = (node.name(), own) {
        named.push((name.to_string(), b));
    }
    own
}

fn rounded(v: DVec3) -> Value {
    json!(v.to_array().map(|x| (x * 1e6).round() / 1e6))
}

pub fn facts(parsed: &gltf::Gltf) -> Result<Map<String, Value>, String> {
    let scene = parsed
        .default_scene()
        .or_else(|| parsed.scenes().next())
        .ok_or("the file has no scene")?;
    let mut named = Vec::new();
    let mut all: Bounds = None;
    for node in scene.nodes() {
        all = union(all, walk(node, DMat4::IDENTITY, &mut named));
    }
    let (lo, hi) = all.ok_or("the file's scene has no mesh")?;
    named.sort_by(|a, b| a.0.cmp(&b.0));
    let mut wheels = Map::new();
    let mut anchors = Map::new();
    for (name, (nlo, nhi)) in named {
        let lower = name.to_ascii_lowercase();
        let centre = (nlo + nhi) * 0.5;
        let extent = nhi - nlo;
        if lower.contains("wheel") {
            let thin = extent.min_position();
            let mut axle = [0.0; 3];
            axle[thin] = 1.0;
            let radius = (0..3)
                .filter(|&k| k != thin)
                .map(|k| extent[k])
                .fold(0.0, f64::max)
                * 0.5;
            let key = lower
                .strip_prefix("wheel_")
                .map_or(name.clone(), str::to_string);
            wheels.insert(
                key,
                json!({"centre": rounded(centre), "radius": (radius * 1e6).round() / 1e6, "axle": axle}),
            );
        } else if ["light", "lamp", "head", "tail"]
            .iter()
            .any(|word| lower.contains(word))
        {
            anchors.insert(
                name,
                json!({"centre": rounded(centre), "size": rounded(extent)}),
            );
        }
    }
    let mut out = Map::new();
    out.insert("size".into(), rounded(hi - lo));
    if !wheels.is_empty() {
        out.insert("wheels".into(), Value::Object(wheels));
    }
    if !anchors.is_empty() {
        out.insert("anchors".into(), Value::Object(anchors));
    }
    Ok(out)
}

pub fn add(file: &Path, entry: &str, tags: &[String], root: &Path) -> Result<Added, String> {
    let shown = file.display();
    let bytes = std::fs::read(file).map_err(|e| format!("{shown}: {e}"))?;
    let parsed = basics(&bytes).map_err(|e| format!("{shown}: {e}"))?;
    validated(file).map_err(|e| format!("{shown}: {e}"))?;
    let facts = facts(&parsed).map_err(|e| format!("{shown}: {e}"))?;
    let (kind, name) = entry.split_once('/').unwrap_or_default();
    let original = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let sidecar = library::Sidecar {
        kind: kind.into(),
        name: name.into(),
        tags: tags.to_vec(),
        licence: library::LICENCE.into(),
        generator: None,
        params: [
            ("file".to_string(), json!(original)),
            ("sha256".to_string(), json!(sha256(&bytes))),
        ]
        .into(),
        manfred: super::VERSION.into(),
        facts: facts.clone(),
    };
    let dir = root.join(entry);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let model = dir.join("model.glb");
    let written = dir.join("object.toml");
    std::fs::write(&model, &bytes).map_err(|e| format!("{}: {e}", model.display()))?;
    std::fs::write(&written, sidecar.text()).map_err(|e| format!("{}: {e}", written.display()))?;
    Ok(Added {
        dir,
        model,
        sidecar: written,
        facts,
    })
}

fn subject(added: &Added, registry: &Registry) -> Result<Subject, String> {
    let text = format!(
        "manfred = 1\nname = \"library\"\n\n[[object]]\nname = \"model\"\nkind = \"mesh\"\npath = \"model.glb\"\n{LIGHTING}"
    );
    let recipe = manfred_recipe::parse(&text).map_err(|e| e.to_string())?;
    let built = build_with(&recipe, &added.dir, registry).map_err(|e| e.to_string())?;
    Ok(Subject {
        recipe,
        built,
        index: 0,
        base: added.dir.clone(),
    })
}

pub fn run(args: &Args, registry: &Registry) -> ExitCode {
    let added = match add(&args.file, &args.entry, &args.tags, &args.library) {
        Ok(added) => added,
        Err(e) => return refuse("library add", &e),
    };
    let thumb = if args.thumb {
        match subject(&added, registry).and_then(|s| thumbnail(&s, &added.dir, registry)) {
            Ok(path) => Some(path),
            Err(e) => return refuse("library add", &e),
        }
    } else {
        None
    };
    let index = match library::write_index(&args.library) {
        Ok(path) => path,
        Err(e) => return refuse("library add", &e.to_string()),
    };
    if args.json {
        let (kind, name) = args.entry.split_once('/').unwrap_or_default();
        println!(
            "{}",
            json!({
                "kind": kind,
                "name": name,
                "dir": quoted(&added.dir),
                "model": quoted(&added.model),
                "sidecar": quoted(&added.sidecar),
                "thumb": thumb.as_deref().map(quoted),
                "index": quoted(&index),
                "facts": added.facts,
            })
        );
    } else {
        println!("{}", added.dir.display());
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_glb::{Node, Order};
    use manfred_mesh::{cylinder, rounded_box, transform};

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn node(name: &str, mesh: manfred_mesh::Mesh, at: [f64; 3]) -> Node {
        Node {
            name: name.into(),
            mesh,
            transform: DMat4::from_translation(DVec3::from(at)),
            keep_transform: true,
            targets: None,
        }
    }

    #[test]
    fn an_added_glb_gets_its_size_wheels_and_anchors() {
        let dir = scratch("library-add");
        let mut tyre = cylinder(0.3, 0.2, 16, true);
        transform(
            &mut tyre,
            DMat4::from_rotation_z(std::f64::consts::FRAC_PI_2),
        );
        let glb = manfred_glb::write(
            &[
                node(
                    "Body",
                    rounded_box([1.8, 1.0, 4.0], 0.0, 1),
                    [0.0, 0.8, 0.0],
                ),
                node("wheel_fl", tyre.clone(), [0.8, 0.3, 1.3]),
                node("RearWheel", tyre, [0.8, 0.3, -1.3]),
                node(
                    "headlights",
                    rounded_box([1.2, 0.1, 0.1], 0.0, 1),
                    [0.0, 0.9, 1.9],
                ),
            ],
            Order::File,
            "test",
        );
        let file = dir.join("hand.glb");
        std::fs::write(&file, &glb).unwrap();
        let added = add(&file, "cars/hand", &["car".into()], &dir.join("library")).unwrap();
        assert_eq!(std::fs::read(&added.model).unwrap(), glb);
        let f = &added.facts;
        let near = |v: &Value, want: [f64; 3]| {
            let got: Vec<f64> = v
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap())
                .collect();
            assert!(
                got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-4),
                "{got:?} {want:?}"
            );
        };
        near(&f["size"], [1.8, 1.3, 4.0]);
        near(&f["wheels"]["fl"]["centre"], [0.8, 0.3, 1.3]);
        assert!((f["wheels"]["fl"]["radius"].as_f64().unwrap() - 0.3).abs() < 1e-3);
        assert_eq!(f["wheels"]["fl"]["axle"], json!([1.0, 0.0, 0.0]));
        near(&f["wheels"]["RearWheel"]["centre"], [0.8, 0.3, -1.3]);
        near(&f["anchors"]["headlights"]["centre"], [0.0, 0.9, 1.9]);
        assert!(f["anchors"].get("Body").is_none());
        let text = std::fs::read_to_string(&added.sidecar).unwrap();
        assert!(
            text.contains("hand.glb") && text.contains(&sha256(&glb)),
            "{text}"
        );
        assert!(
            add(
                &dir.join("library/cars/hand/object.toml"),
                "cars/x",
                &[],
                &dir
            )
            .is_err()
        );
    }
}
