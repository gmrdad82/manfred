use crate::{Error, NodeSummary, Summary};
use glam::{DMat4, DVec3};
use gltf::mesh::Mode;

const DEFAULT_MATERIAL: &str = "default";

fn world(parent: DMat4, node: &gltf::Node) -> DMat4 {
    parent * DMat4::from_cols_array_2d(&node.transform().matrix().map(|c| c.map(f64::from)))
}

fn triangles(mode: Mode, indices: &[u32]) -> Vec<[u32; 3]> {
    match mode {
        Mode::Triangles => indices
            .chunks_exact(3)
            .map(|t| [t[0], t[1], t[2]])
            .collect(),
        Mode::TriangleStrip => (2..indices.len())
            .map(|i| {
                if i % 2 == 0 {
                    [indices[i - 2], indices[i - 1], indices[i]]
                } else {
                    [indices[i - 1], indices[i - 2], indices[i]]
                }
            })
            .collect(),
        Mode::TriangleFan => (2..indices.len())
            .map(|i| [indices[0], indices[i - 1], indices[i]])
            .collect(),
        _ => Vec::new(),
    }
}

fn uv_area(triangles: &[[u32; 3]], uvs: &[[f32; 2]]) -> f64 {
    triangles
        .iter()
        .filter_map(|t| {
            let [a, b, c] = t.map(|i| uvs.get(i as usize).map(|p| p.map(f64::from)));
            let (a, b, c) = (a?, b?, c?);
            Some(0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs())
        })
        .sum()
}

fn visit(
    node: &gltf::Node,
    parent: DMat4,
    buffers: &[gltf::buffer::Data],
    seen: &mut [bool],
    out: &mut Vec<NodeSummary>,
) {
    if seen
        .get_mut(node.index())
        .is_none_or(|done| std::mem::replace(done, true))
    {
        return;
    }
    let matrix = world(parent, node);
    if let Some(mesh) = node.mesh() {
        let mut summary = NodeSummary {
            name: node.name().or(mesh.name()).unwrap_or("unnamed").to_string(),
            index: node.index(),
            triangles: 0,
            vertices: 0,
            bounds: [[0.0; 3]; 2],
            area: 0.0,
            slots: Vec::new(),
            uvs: false,
            uvs1: false,
            uv1_area: 0.0,
            extras: node
                .extras()
                .as_ref()
                .and_then(|raw| serde_json::from_str(raw.get()).ok()),
        };
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = DVec3::splat(f64::NEG_INFINITY);
        for primitive in mesh.primitives() {
            summary.slots.push(
                primitive
                    .material()
                    .name()
                    .unwrap_or(DEFAULT_MATERIAL)
                    .to_string(),
            );
            let reader = primitive.reader(|b| Some(&buffers[b.index()]));
            let Some(positions) = reader.read_positions() else {
                continue;
            };
            let points: Vec<DVec3> = positions
                .map(|p| matrix.transform_point3(DVec3::new(p[0].into(), p[1].into(), p[2].into())))
                .collect();
            summary.vertices += points.len();
            summary.uvs |= reader.read_tex_coords(0).is_some();
            let uv1: Option<Vec<[f32; 2]>> =
                reader.read_tex_coords(1).map(|t| t.into_f32().collect());
            summary.uvs1 |= uv1.is_some();
            for p in &points {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            let indices: Vec<u32> = match reader.read_indices() {
                Some(indices) => indices.into_u32().collect(),
                None => (0..points.len() as u32).collect(),
            };
            let triangles = triangles(primitive.mode(), &indices);
            for &[a, b, c] in &triangles {
                let (Some(a), Some(b), Some(c)) = (
                    points.get(a as usize),
                    points.get(b as usize),
                    points.get(c as usize),
                ) else {
                    continue;
                };
                summary.triangles += 1;
                summary.area += 0.5 * (*b - *a).cross(*c - *a).length();
            }
            if let Some(uv1) = &uv1 {
                summary.uv1_area += uv_area(&triangles, uv1);
            }
        }
        if lo.x.is_finite() {
            summary.bounds = [lo.to_array(), hi.to_array()];
        }
        out.push(summary);
    }
    for child in node.children() {
        visit(&child, matrix, buffers, seen, out);
    }
}

pub fn summary(bytes: &[u8]) -> Result<Summary, Error> {
    let mut parsed = gltf::Gltf::from_slice(bytes).map_err(|e| Error(format!("not a GLB: {e}")))?;
    let buffers = gltf::import_buffers(&parsed.document, None, parsed.blob.take())
        .map_err(|e| Error(format!("buffers: {e}")))?;
    let document = &parsed.document;
    let mut nodes = Vec::new();
    let mut seen = vec![false; document.nodes().len()];
    match document
        .default_scene()
        .or_else(|| document.scenes().next())
    {
        Some(scene) => {
            for root in scene.nodes() {
                visit(&root, DMat4::IDENTITY, &buffers, &mut seen, &mut nodes);
            }
        }
        None => {
            let mut children = vec![false; document.nodes().len()];
            for node in document.nodes() {
                for child in node.children() {
                    children[child.index()] = true;
                }
            }
            for node in document.nodes().filter(|n| !children[n.index()]) {
                visit(&node, DMat4::IDENTITY, &buffers, &mut seen, &mut nodes);
            }
        }
    }
    Ok(Summary {
        generator: document.as_json().asset.generator.clone(),
        nodes,
        materials: document
            .materials()
            .map(|m| m.name().unwrap_or(DEFAULT_MATERIAL).to_string())
            .collect(),
    })
}
