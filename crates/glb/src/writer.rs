use crate::{Node, Order};
use gltf_json as json;
use json::accessor::{ComponentType, GenericComponentType, Type};
use json::buffer::Target;
use json::mesh::{Mode, Primitive, Semantic};
use json::validation::{Checked, USize64};
use json::{Index, Root};
use manfred_mesh::{Mesh, natural_cmp};
use serde_json::Value;
use std::collections::BTreeMap;

const BASE_COLOR: [f32; 4] = [0.8, 0.8, 0.8, 1.0];
const ROUGHNESS: f32 = 0.5;
const DEFAULT_MATERIAL: &str = "default";

struct Builder {
    root: Root,
    bin: Vec<u8>,
    materials: Vec<String>,
}

fn short(value: f32) -> Value {
    let text = format!("{value}");
    Value::from(text.parse::<f64>().unwrap_or(0.0))
}

fn bounds<const N: usize>(data: &[[f32; N]]) -> Option<([f32; N], [f32; N])> {
    let first = data.first()?;
    let mut lo = *first;
    let mut hi = *first;
    for item in data {
        for axis in 0..N {
            lo[axis] = lo[axis].min(item[axis]);
            hi[axis] = hi[axis].max(item[axis]);
        }
    }
    Some((lo, hi))
}

impl Builder {
    fn pad(&mut self) {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
    }

    fn view(&mut self, bytes: &[u8], target: Target) -> Index<json::buffer::View> {
        self.pad();
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        self.root.buffer_views.push(json::buffer::View {
            buffer: Index::new(0),
            byte_length: USize64::from(bytes.len()),
            byte_offset: Some(USize64::from(offset)),
            byte_stride: None,
            extensions: None,
            extras: Default::default(),
            name: None,
            target: Some(Checked::Valid(target)),
        });
        Index::new(self.root.buffer_views.len() as u32 - 1)
    }

    fn accessor(
        &mut self,
        view: Index<json::buffer::View>,
        count: usize,
        component: ComponentType,
        kind: Type,
        range: Option<(Vec<Value>, Vec<Value>)>,
    ) -> Index<json::accessor::Accessor> {
        let (min, max) = match range {
            Some((lo, hi)) => (Some(Value::Array(lo)), Some(Value::Array(hi))),
            None => (None, None),
        };
        self.root.accessors.push(json::accessor::Accessor {
            buffer_view: Some(view),
            byte_offset: Some(USize64(0)),
            count: USize64::from(count),
            component_type: Checked::Valid(GenericComponentType(component)),
            extensions: None,
            extras: Default::default(),
            type_: Checked::Valid(kind),
            min,
            max,
            name: None,
            normalized: false,
            sparse: None,
        });
        Index::new(self.root.accessors.len() as u32 - 1)
    }

    fn vec_accessor<const N: usize>(
        &mut self,
        data: &[[f32; N]],
        kind: Type,
    ) -> Index<json::accessor::Accessor> {
        let mut bytes = Vec::with_capacity(data.len() * N * 4);
        for item in data {
            for value in item {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        let view = self.view(&bytes, Target::ArrayBuffer);
        let range = bounds(data).map(|(lo, hi)| {
            (
                lo.iter().map(|v| short(*v)).collect(),
                hi.iter().map(|v| short(*v)).collect(),
            )
        });
        self.accessor(view, data.len(), ComponentType::F32, kind, range)
    }

    fn index_accessor(&mut self, data: &[u32]) -> Index<json::accessor::Accessor> {
        let mut bytes = Vec::with_capacity(data.len() * 4);
        for value in data {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let view = self.view(&bytes, Target::ElementArrayBuffer);
        let lo = data.iter().copied().min().unwrap_or(0);
        let hi = data.iter().copied().max().unwrap_or(0);
        self.accessor(
            view,
            data.len(),
            ComponentType::U32,
            Type::Scalar,
            Some((vec![Value::from(lo)], vec![Value::from(hi)])),
        )
    }

    fn material(&mut self, name: &str) -> Index<json::material::Material> {
        let at = match self.materials.iter().position(|m| m == name) {
            Some(at) => at,
            None => {
                self.materials.push(name.to_string());
                self.root.materials.push(json::Material {
                    double_sided: true,
                    name: Some(name.to_string()),
                    pbr_metallic_roughness: json::material::PbrMetallicRoughness {
                        base_color_factor: json::material::PbrBaseColorFactor(BASE_COLOR),
                        metallic_factor: json::material::StrengthFactor(0.0),
                        roughness_factor: json::material::StrengthFactor(ROUGHNESS),
                        ..Default::default()
                    },
                    ..Default::default()
                });
                self.materials.len() - 1
            }
        };
        Index::new(at as u32)
    }

    fn primitive(&mut self, mesh: &Mesh, material: &str, indices: &[u32]) -> Option<Primitive> {
        let vertices = mesh.positions.len();
        let normals = mesh.normals.len() == vertices;
        let uvs = mesh.uvs.as_ref().filter(|uvs| uvs.len() == vertices);
        let uvs1 = mesh.uvs1.as_ref().filter(|uvs| uvs.len() == vertices);
        let mut remap = vec![u32::MAX; vertices];
        let mut positions = Vec::new();
        let mut normal_data = Vec::new();
        let mut uv_data = Vec::new();
        let mut uv1_data = Vec::new();
        let mut out = Vec::new();
        for triangle in indices.chunks_exact(3) {
            if triangle.iter().any(|&i| i as usize >= vertices) {
                continue;
            }
            for &old in triangle {
                let slot = &mut remap[old as usize];
                if *slot == u32::MAX {
                    *slot = positions.len() as u32;
                    positions.push(mesh.positions[old as usize]);
                    if normals {
                        normal_data.push(mesh.normals[old as usize]);
                    }
                    if let Some(uvs) = uvs {
                        uv_data.push(uvs[old as usize]);
                    }
                    if let Some(uvs1) = uvs1 {
                        uv1_data.push(uvs1[old as usize]);
                    }
                }
                out.push(*slot);
            }
        }
        if out.is_empty() {
            return None;
        }
        let mut attributes = BTreeMap::new();
        attributes.insert(
            Checked::Valid(Semantic::Positions),
            self.vec_accessor(&positions, Type::Vec3),
        );
        if normals {
            attributes.insert(
                Checked::Valid(Semantic::Normals),
                self.vec_accessor(&normal_data, Type::Vec3),
            );
        }
        if uvs.is_some() {
            attributes.insert(
                Checked::Valid(Semantic::TexCoords(0)),
                self.vec_accessor(&uv_data, Type::Vec2),
            );
        }
        if uvs1.is_some() {
            attributes.insert(
                Checked::Valid(Semantic::TexCoords(1)),
                self.vec_accessor(&uv1_data, Type::Vec2),
            );
        }
        let indices = self.index_accessor(&out);
        let material = self.material(material);
        Some(Primitive {
            attributes,
            extensions: None,
            extras: Default::default(),
            indices: Some(indices),
            material: Some(material),
            mode: Checked::Valid(Mode::Triangles),
            targets: None,
        })
    }

    fn primitives(&mut self, mesh: &Mesh) -> Vec<Primitive> {
        let mut out = Vec::new();
        if mesh.slots.is_empty() {
            out.extend(self.primitive(mesh, DEFAULT_MATERIAL, &mesh.indices));
            return out;
        }
        for slot in &mesh.slots {
            let start = (slot.start as usize).min(mesh.indices.len());
            let end = (start + slot.count as usize).min(mesh.indices.len());
            out.extend(self.primitive(mesh, &slot.material, &mesh.indices[start..end]));
        }
        out
    }
}

fn matrix(node: &Node) -> Option<[f32; 16]> {
    if !node.keep_transform || node.transform == glam::DMat4::IDENTITY {
        return None;
    }
    Some(node.transform.to_cols_array().map(|v| v as f32))
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8], fill: u8) {
    let padded = data.len().next_multiple_of(4);
    out.extend_from_slice(&(padded as u32).to_le_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.resize(out.len() + padded - data.len(), fill);
}

fn raw(extras: Option<&Value>) -> json::Extras {
    serde_json::value::to_raw_value(extras?).ok()
}

pub fn write(nodes: &[Node], order: Order, generator: &str) -> Vec<u8> {
    write_with_extras(nodes, &[], order, generator)
}

pub fn write_with_extras(
    nodes: &[Node],
    extras: &[Option<Value>],
    order: Order,
    generator: &str,
) -> Vec<u8> {
    write_tree(nodes, &[], extras, order, generator)
}

fn resolved(parents: &[Option<usize>], count: usize) -> Vec<Option<usize>> {
    let parent = |at: usize| parents.get(at).copied().flatten().filter(|&p| p < count);
    (0..count)
        .map(|node| {
            let mut at = node;
            for _ in 0..=count {
                match parent(at) {
                    None => return parent(node),
                    Some(next) => at = next,
                }
            }
            None
        })
        .collect()
}

pub fn write_tree(
    nodes: &[Node],
    parents: &[Option<usize>],
    extras: &[Option<Value>],
    order: Order,
    generator: &str,
) -> Vec<u8> {
    let parents = resolved(parents, nodes.len());
    let mut sequence: Vec<(usize, &Node, Option<&Value>)> = nodes
        .iter()
        .enumerate()
        .map(|(at, node)| (at, node, extras.get(at).and_then(Option::as_ref)))
        .collect();
    if order == Order::Natural {
        sequence.sort_by(|a, b| natural_cmp(&a.1.name, &b.1.name));
    }
    let mut children: Vec<Vec<Index<json::Node>>> = vec![Vec::new(); nodes.len()];
    for (position, (at, _, _)) in sequence.iter().enumerate() {
        if let Some(parent) = parents[*at] {
            children[parent].push(Index::new(position as u32));
        }
    }
    let mut builder = Builder {
        root: Root::default(),
        bin: Vec::new(),
        materials: Vec::new(),
    };
    builder.root.asset.version = "2.0".to_string();
    builder.root.asset.generator = Some(generator.to_string());
    let mut scene_nodes = Vec::new();
    for &(at, node, extras) in &sequence {
        let primitives = builder.primitives(&node.mesh);
        let mesh = if primitives.is_empty() {
            None
        } else {
            builder.root.meshes.push(json::Mesh {
                extensions: None,
                extras: Default::default(),
                name: Some(node.name.clone()),
                primitives,
                weights: None,
            });
            Some(Index::new(builder.root.meshes.len() as u32 - 1))
        };
        let children = std::mem::take(&mut children[at]);
        builder.root.nodes.push(json::Node {
            matrix: matrix(node),
            mesh,
            name: Some(node.name.clone()),
            extras: raw(extras),
            children: (!children.is_empty()).then_some(children),
            ..Default::default()
        });
        if parents[at].is_none() {
            scene_nodes.push(Index::new(builder.root.nodes.len() as u32 - 1));
        }
    }
    builder.root.scenes.push(json::Scene {
        extensions: None,
        extras: Default::default(),
        name: None,
        nodes: scene_nodes,
    });
    builder.root.scene = Some(Index::new(0));
    builder.pad();
    if !builder.bin.is_empty() {
        builder.root.buffers.push(json::Buffer {
            byte_length: USize64::from(builder.bin.len()),
            extensions: None,
            extras: Default::default(),
            name: None,
            uri: None,
        });
    }
    let text = json::serialize::to_vec(&builder.root).unwrap_or_default();
    let mut body = Vec::new();
    chunk(&mut body, b"JSON", &text, b' ');
    if !builder.bin.is_empty() {
        chunk(&mut body, b"BIN\0", &builder.bin, 0);
    }
    let mut out = Vec::with_capacity(12 + body.len());
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((12 + body.len()) as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}
