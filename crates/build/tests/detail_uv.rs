use glam::{DVec2, DVec3, Vec2, Vec3};
use manfred_build::detail_uv::{CHART_DEGREES, LARGEST, SMALLEST, layout};
use manfred_build::{Built, build};
use manfred_mesh::Mesh;
use manfred_recipe::{load, parse};
use std::path::{Path, PathBuf};

const TEXELS_PER_M: f64 = 2000.0;
const GUTTER: u32 = 16;

const PARTS: &str = r#"manfred = 1
name = "parts"

[[object]]
name = "box"
kind = "box"
size = [0.1, 0.05, 0.08]
bevel = { width = 0.01, segments = 3 }
material = "m"
smooth_angle = 35.0
detail_uv = { texels_per_m = 2000 }

[[object]]
name = "can"
kind = "cylinder"
radius = 0.03
depth = 0.1
segments = 32
material = "m"
smooth_angle = 35.0
detail_uv = { texels_per_m = 2000 }

[[object]]
name = "ball"
kind = "sphere"
radius = 0.04
segments = 32
rings = 16
material = "m"
smooth = true
detail_uv = { texels_per_m = 2000 }

[[object]]
name = "slab"
kind = "outline"
outline = { rect = [0.0, 0.0, 0.12, 0.06], radius = 0.01 }
front = 0.0
back = 0.02
profile = { kind = "round", width = 0.004, segments = 3 }
material = "m"
smooth_angle = 35.0
detail_uv = { texels_per_m = 2000 }

[[object]]
name = "mirror"
kind = "box"
size = [0.1, 0.1, 0.1]
at = [1.0, 2.0, 3.0]
rotate = [10.0, 20.0, 30.0]
scale = [-1.0, 2.0, 0.5]
material = "m"
detail_uv = { texels_per_m = 2000 }

[[object]]
name = "ring"
kind = "torus"
major = 0.05
minor = 0.01
segments = 48
rings = 12
material = "m"
smooth = true
detail_uv = { texels_per_m = 2000 }

[[object]]
name = "plain"
kind = "box"
size = [0.1, 0.1, 0.1]
material = "m"
"#;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn parts() -> Built {
    build(&parse(PARTS).unwrap(), Path::new(".")).unwrap()
}

fn mesh<'a>(b: &'a Built, name: &str) -> &'a Mesh {
    &b.parts
        .iter()
        .find(|p| p.node.name == name)
        .unwrap()
        .node
        .mesh
}

fn world(m: &Mesh) -> Vec<[DVec3; 3]> {
    m.indices
        .chunks_exact(3)
        .map(|t| [0, 1, 2].map(|k| Vec3::from(m.positions[t[k] as usize]).as_dvec3()))
        .collect()
}

fn flat(m: &Mesh) -> Vec<[DVec2; 3]> {
    let uvs1 = m.uvs1.as_ref().unwrap();
    m.indices
        .chunks_exact(3)
        .map(|t| [0, 1, 2].map(|k| Vec2::from(uvs1[t[k] as usize]).as_dvec2()))
        .collect()
}

fn side(m: &Mesh, texels_per_m: f64) -> u32 {
    let mut ratio = 0.0f64;
    for (p, q) in world(m).iter().zip(flat(m)) {
        for k in 0..3 {
            let metres = (p[(k + 1) % 3] - p[k]).length();
            if metres > 1e-4 {
                ratio = ratio.max((q[(k + 1) % 3] - q[k]).length() / metres);
            }
        }
    }
    let size = texels_per_m / ratio;
    let side = size.log2().round().exp2() as u32;
    assert!((size / f64::from(side) - 1.0).abs() < 1e-3, "{size}");
    side
}

fn inside(tri: &[DVec2; 3], p: DVec2, margin: f64) -> Option<[f64; 3]> {
    let area = (tri[1] - tri[0]).perp_dot(tri[2] - tri[0]);
    if area.abs() < 1e-18 {
        return None;
    }
    let w0 = (tri[2] - tri[1]).perp_dot(p - tri[1]) / area;
    let w1 = (tri[0] - tri[2]).perp_dot(p - tri[2]) / area;
    let w2 = 1.0 - w0 - w1;
    (w0 > margin && w1 > margin && w2 > margin).then_some([w0, w1, w2])
}

struct Raster {
    size: u32,
    count: Vec<u8>,
    first: Vec<u32>,
}

fn pixel(p: DVec2, s: f64) -> DVec2 {
    DVec2::new(p.x * s, (1.0 - p.y) * s)
}

fn texels(tris: &[[DVec2; 3]], size: u32, margin: f64) -> Raster {
    let s = f64::from(size);
    let mut raster = Raster {
        size,
        count: vec![0; (size * size) as usize],
        first: vec![u32::MAX; (size * size) as usize],
    };
    for (index, tri) in tris.iter().enumerate() {
        let tri = tri.map(|p| pixel(p, s));
        let lo = tri[0].min(tri[1]).min(tri[2]).floor().max(DVec2::ZERO);
        let hi = tri[0].max(tri[1]).max(tri[2]).ceil().min(DVec2::splat(s));
        for y in lo.y as u32..hi.y as u32 {
            for x in lo.x as u32..hi.x as u32 {
                let centre = DVec2::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
                if inside(&tri, centre, margin).is_some() {
                    let at = (y * size + x) as usize;
                    raster.count[at] = raster.count[at].saturating_add(1);
                    if raster.first[at] == u32::MAX {
                        raster.first[at] = index as u32;
                    }
                }
            }
        }
    }
    raster
}

fn surface(
    raster: &Raster,
    tris: &[[DVec2; 3]],
    corners: &[[DVec3; 3]],
    at: usize,
) -> Option<(usize, DVec3)> {
    let index = *raster.first.get(at).filter(|&&i| i != u32::MAX)? as usize;
    let s = f64::from(raster.size);
    let size = raster.size as usize;
    let centre = DVec2::new((at % size) as f64 + 0.5, (at / size) as f64 + 0.5);
    let w = inside(&tris[index].map(|p| pixel(p, s)), centre, -1e-6)?;
    let t = corners[index];
    Some((index, t[0] * w[0] + t[1] * w[1] + t[2] * w[2]))
}

fn check(m: &Mesh, name: &str, texels_per_m: f64, gutter: u32) -> u32 {
    let uvs1 = m
        .uvs1
        .as_ref()
        .unwrap_or_else(|| panic!("{name} has no TEXCOORD_1"));
    assert_eq!(uvs1.len(), m.positions.len(), "{name}");
    assert!(m.uvs.is_some(), "{name} lost its TEXCOORD_0");
    let size = side(m, texels_per_m);
    assert!((SMALLEST..=LARGEST).contains(&size), "{name}: {size}");
    let edge = f64::from(gutter) / f64::from(size) - 1e-6;
    for uv in uvs1 {
        assert!(
            uv.iter()
                .all(|&c| f64::from(c) >= edge && f64::from(c) <= 1.0 - edge),
            "{name}: {uv:?} is outside the gutter"
        );
    }
    let ask = texels_per_m / f64::from(size);
    let floor = CHART_DEGREES.to_radians().cos();
    assert!(floor > 0.99);
    for (p, q) in world(m).iter().zip(flat(m)) {
        for k in 0..3 {
            let metres = (p[(k + 1) % 3] - p[k]).length();
            if metres * texels_per_m < 2.0 {
                continue;
            }
            let scale = (q[(k + 1) % 3] - q[k]).length() / metres / ask;
            assert!(
                (0.99..=1.0001).contains(&scale),
                "{name}: an edge of {metres} m is at {scale} of the asked texels per metre"
            );
        }
    }
    let tris = flat(m);
    let raster = texels(&tris, size, 1e-9);
    let overlaps = raster.count.iter().filter(|&&c| c > 1).count();
    assert_eq!(overlaps, 0, "{name}: {overlaps} texels are covered twice");
    let corners = world(m);
    let coarse = texels(&tris, size.min(1024), 1e-9);
    let mut points: Vec<[i64; 3]> = (0..coarse.count.len())
        .filter(|&at| coarse.count[at] == 1)
        .filter_map(|at| surface(&coarse, &tris, &corners, at))
        .map(|(_, p)| (p * 1e7).round().as_i64vec3().to_array())
        .collect();
    let covered = points.len();
    assert!(covered > 0, "{name}: nothing is covered");
    points.sort_unstable();
    points.dedup();
    assert_eq!(points.len(), covered, "{name}: two texels share a point");
    size
}

#[test]
fn every_marked_part_has_a_unique_layout_at_its_texels_per_metre() {
    let b = parts();
    for name in ["box", "can", "ball", "slab", "mirror", "ring"] {
        check(mesh(&b, name), name, TEXELS_PER_M, GUTTER);
    }
    assert!(mesh(&b, "plain").uvs1.is_none());
}

#[test]
fn charts_sit_a_gutter_apart_and_split_where_the_normal_turns() {
    let b = parts();
    for name in ["box", "can", "ball", "slab", "mirror", "ring"] {
        let m = mesh(&b, name);
        let corners = world(m);
        let found = layout(&corners, TEXELS_PER_M, GUTTER).unwrap();
        assert_eq!(found.size, side(m, TEXELS_PER_M), "{name}");
        let s = f64::from(found.size);
        let mut boxes = Vec::new();
        for faces in &found.charts {
            let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
            let normal = |f: usize| {
                let t = corners[f];
                (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero()
            };
            let axis = normal(faces[0]);
            for &f in faces {
                if normal(f) != DVec3::ZERO {
                    assert!(
                        normal(f).dot(axis) >= CHART_DEGREES.to_radians().cos() - 1e-12,
                        "{name}: a chart turns past {CHART_DEGREES} degrees"
                    );
                }
                for k in 0..3 {
                    let uv = found.corners[f * 3 + k];
                    let p = DVec2::new(f64::from(uv[0]), f64::from(uv[1])) * s;
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
            boxes.push((lo, hi));
        }
        let gap = f64::from(GUTTER) - 1e-3;
        for (i, a) in boxes.iter().enumerate() {
            for b in &boxes[i + 1..] {
                let apart = a.1.x + gap <= b.0.x
                    || b.1.x + gap <= a.0.x
                    || a.1.y + gap <= b.0.y
                    || b.1.y + gap <= a.0.y;
                assert!(apart, "{name}: two charts are closer than the gutter");
            }
        }
        let faces: usize = found.charts.iter().map(Vec::len).sum();
        assert_eq!(faces, corners.len(), "{name}");
    }
    let flat_box = layout(&world(mesh(&b, "plain")), 100.0, GUTTER).unwrap();
    assert_eq!(flat_box.charts.len(), 6);
}

#[test]
fn the_same_mesh_gives_the_same_layout() {
    let a = parts();
    let b = parts();
    for (x, y) in a.parts.iter().zip(&b.parts) {
        assert_eq!(x.node.mesh, y.node.mesh);
    }
    assert!(a.glb() == b.glb());
}

#[test]
fn a_layout_too_large_for_the_biggest_map_is_an_error() {
    let text = "manfred = 1\nname = \"t\"\n[[object]]\nname = \"wide\"\nkind = \"plane\"\nsize = [3.0, 1.0]\nmaterial = \"m\"\ndetail_uv = { texels_per_m = 2000 }\n";
    let error = build(&parse(text).unwrap(), Path::new("."))
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("object[wide].detail_uv: "), "{error}");
    assert!(error.contains("6000 texels"), "{error}");
}

#[test]
fn the_glb_carries_two_uv_sets_on_the_marked_nodes_only() {
    let bytes = parts().glb();
    let (document, buffers, _) = gltf::import_slice(&bytes).unwrap();
    for node in document.nodes() {
        let name = node.name().unwrap();
        for primitive in node.mesh().unwrap().primitives() {
            let reader = primitive.reader(|b| Some(&buffers[b.index()]));
            let count = reader.read_positions().unwrap().count();
            let first = reader.read_tex_coords(0).map(|t| t.into_f32().count());
            let second = reader.read_tex_coords(1).map(|t| t.into_f32().count());
            assert_eq!(first, Some(count), "{name}");
            if name == "plain" {
                assert_eq!(second, None);
            } else {
                assert_eq!(second, Some(count), "{name}");
            }
        }
    }
}

fn toybox_text() -> String {
    std::fs::read_to_string(root().join("tests/golden/toybox/toybox.toml")).unwrap()
}

fn toybox(text: &str) -> Built {
    let dir = root().join("tests/golden/toybox");
    let mut r = parse(text).unwrap();
    r.base = Some(dir.clone());
    build(&r, &dir).unwrap()
}

type Streams = (
    String,
    Vec<[f32; 3]>,
    Vec<[f32; 3]>,
    Vec<[f32; 2]>,
    Vec<u32>,
    Option<Vec<[f32; 2]>>,
);

fn streams(bytes: &[u8]) -> Vec<Streams> {
    let (document, buffers, _) = gltf::import_slice(bytes).unwrap();
    let mut out = Vec::new();
    for node in document.nodes() {
        let Some(mesh) = node.mesh() else {
            continue;
        };
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|b| Some(&buffers[b.index()]));
            out.push((
                node.name().unwrap().to_string(),
                reader.read_positions().unwrap().collect(),
                reader.read_normals().unwrap().collect(),
                reader.read_tex_coords(0).unwrap().into_f32().collect(),
                reader.read_indices().unwrap().into_u32().collect(),
                reader.read_tex_coords(1).map(|t| t.into_f32().collect()),
            ));
        }
    }
    out
}

#[test]
fn the_toyboxs_backdrop_takes_a_detail_layout_and_keeps_its_texcoord_0() {
    let text = toybox_text();
    let line = "detail_uv = { texels_per_m = 400 }\n";
    assert_eq!(text.matches(line).count(), 1);
    let marked = toybox(&text);
    let bare = toybox(&text.replace(line, ""));
    let (with, without) = (streams(&marked.glb()), streams(&bare.glb()));
    assert_eq!(with.len(), without.len());
    for (a, b) in with.iter().zip(&without) {
        assert_eq!(a.0, b.0);
        let bits = |v: &[[f32; 2]]| v.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>();
        assert_eq!(bits(&a.3), bits(&b.3), "{}: TEXCOORD_0 changed", a.0);
        assert_eq!(a.1, b.1, "{}", a.0);
        assert_eq!(a.2, b.2, "{}", a.0);
        assert_eq!(a.4, b.4, "{}", a.0);
        assert_eq!(b.5, None);
        assert_eq!(a.5.is_some(), a.0 == "backdrop", "{}", a.0);
    }
    let backdrop = mesh(&marked, "backdrop");
    assert_eq!(check(backdrop, "backdrop", 400.0, GUTTER), 1024);
    let summary = manfred_glb::summary(&marked.glb()).unwrap();
    let node = summary.nodes.iter().find(|n| n.name == "backdrop").unwrap();
    assert!(node.uvs1);
    let want = 640.0 * 400.0 / (1024.0 * 1024.0);
    assert!((node.uv1_area - want).abs() < 1e-4, "{}", node.uv1_area);
    let report = manfred_glb::diff(&manfred_glb::summary(&bare.glb()).unwrap(), &summary);
    assert_eq!(report.nodes.len(), 1);
    assert_eq!(report.nodes[0].name, "backdrop");
}

#[test]
fn the_toybox_marks_its_wooden_backdrop_only() {
    let r = load(&root().join("tests/golden/toybox/toybox.toml"))
        .unwrap()
        .recipe;
    let marked: Vec<&str> = r
        .objects
        .iter()
        .filter(|o| o.detail_uv.is_some())
        .map(|o| o.name.as_str())
        .collect();
    assert_eq!(marked, ["backdrop"]);
    let wood: Vec<&str> = r
        .objects
        .iter()
        .filter(|o| o.material.as_deref() == Some("wood"))
        .map(|o| o.name.as_str())
        .collect();
    assert_eq!(wood, marked);
}

fn picture(m: &Mesh, size: u32, square: f64, path: &Path) {
    let corners = world(m);
    let found = layout(&corners, 1.0, 0).ok();
    let chart_of = |face: usize| {
        found
            .as_ref()
            .and_then(|l| l.charts.iter().position(|c| c.contains(&face)))
            .unwrap_or(0)
    };
    let mut charts = vec![0usize; corners.len()];
    for (face, chart) in charts.iter_mut().enumerate() {
        *chart = chart_of(face);
    }
    let tris = flat(m);
    let raster = texels(&tris, size, -1e-9);
    let mut rgb = Vec::with_capacity((size * size * 3) as usize);
    for at in 0..raster.count.len() {
        let colour = match surface(&raster, &tris, &corners, at) {
            None => [16, 22, 38],
            Some((index, p)) => {
                let cell = (p / square).floor().as_i64vec3();
                let odd = (cell.x + cell.y + cell.z).rem_euclid(2) == 1;
                let hue = (charts[index] * 47 % 96) as u8;
                if odd {
                    [196, 150u8.saturating_sub(hue / 2), 96 + hue]
                } else {
                    [120, 82u8.saturating_sub(hue / 3), 52 + hue / 2]
                }
            }
        };
        rgb.extend_from_slice(&colour);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size, size);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&rgb).unwrap();
}

#[test]
fn the_backdrop_layout_draws_as_a_checker_over_its_charts() {
    let marked = toybox(&toybox_text());
    let backdrop = mesh(&marked, "backdrop");
    let path = root().join("tmp/detailuv-backdrop.png");
    picture(backdrop, 1024, 0.1, &path);
    assert!(std::fs::metadata(&path).unwrap().len() > 0);
    picture(
        mesh(&parts(), "slab"),
        256,
        0.005,
        &root().join("tmp/detailuv-slab.png"),
    );
}
