use crate::{
    ANIMATION, Animation, Error, Image, Lens, Lod, Look, Looks, Map, Mime, Node, Order, Sampler,
    Track, Trs,
};
use gltf_json as json;
use json::accessor::{ComponentType, GenericComponentType, Type};
use json::buffer::Target;
use json::mesh::MorphTarget;
use json::mesh::{Mode, Primitive, Semantic};
use json::validation::{Checked, USize64};
use json::{Index, Root};
use manfred_mesh::{Mesh, Morph, natural_cmp};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

const BASE_COLOR: [f32; 4] = [0.8, 0.8, 0.8, 1.0];
const ROUGHNESS: f32 = 0.5;
const DEFAULT_MATERIAL: &str = "default";
const TRANSFORM: &str = "KHR_texture_transform";
const DETAIL_NORMAL: &str = "detailNormalTexture";
const MSFT_LOD: &str = "MSFT_lod";
const SHEEN: &str = "KHR_materials_sheen";
const ANISOTROPY: &str = "KHR_materials_anisotropy";

struct Builder<'a> {
    root: Root,
    bin: Vec<u8>,
    materials: Vec<String>,
    looks: &'a Looks,
    images: Vec<Image>,
    samplers: Vec<Sampler>,
    textures: Vec<(usize, Option<usize>)>,
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

impl Builder<'_> {
    fn pad(&mut self) {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
    }

    fn view(&mut self, bytes: &[u8], target: Option<Target>) -> Index<json::buffer::View> {
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
            target: target.map(Checked::Valid),
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
        let view = self.view(&bytes, Some(Target::ArrayBuffer));
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
        let view = self.view(&bytes, Some(Target::ElementArrayBuffer));
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

    fn keyed<const N: usize>(&mut self, data: &[[f32; N]]) -> Index<json::accessor::Accessor> {
        let kind = match N {
            1 => Type::Scalar,
            3 => Type::Vec3,
            _ => Type::Vec4,
        };
        let mut bytes = Vec::with_capacity(data.len() * N * 4);
        for item in data {
            for value in item {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        let view = self.view(&bytes, None);
        let range = (N == 1).then(|| bounds(data)).flatten().map(|(lo, hi)| {
            (
                lo.iter().map(|v| short(*v)).collect(),
                hi.iter().map(|v| short(*v)).collect(),
            )
        });
        self.accessor(view, data.len(), ComponentType::F32, kind, range)
    }

    fn image(&mut self, image: &Image) -> usize {
        if let Some(at) = self
            .images
            .iter()
            .position(|known| Arc::ptr_eq(&known.bytes, &image.bytes) || known.bytes == image.bytes)
        {
            return at;
        }
        self.pad();
        let offset = self.bin.len();
        self.bin.extend_from_slice(&image.bytes);
        self.root.buffer_views.push(json::buffer::View {
            buffer: Index::new(0),
            byte_length: USize64::from(image.bytes.len()),
            byte_offset: Some(USize64::from(offset)),
            byte_stride: None,
            extensions: None,
            extras: Default::default(),
            name: None,
            target: None,
        });
        self.root.images.push(json::Image {
            buffer_view: Some(Index::new(self.root.buffer_views.len() as u32 - 1)),
            mime_type: Some(json::image::MimeType(image.mime.text().to_string())),
            name: None,
            uri: None,
            extensions: None,
            extras: Default::default(),
        });
        self.images.push(image.clone());
        self.images.len() - 1
    }

    fn sampler(&mut self, sampler: Sampler) -> usize {
        if let Some(at) = self.samplers.iter().position(|known| *known == sampler) {
            return at;
        }
        let mut fields = serde_json::Map::new();
        if let Some(mag) = sampler.mag {
            fields.insert("magFilter".into(), Value::from(mag));
        }
        if let Some(min) = sampler.min {
            fields.insert("minFilter".into(), Value::from(min));
        }
        fields.insert("wrapS".into(), Value::from(sampler.wrap[0]));
        fields.insert("wrapT".into(), Value::from(sampler.wrap[1]));
        let json = serde_json::from_value::<json::texture::Sampler>(Value::Object(fields))
            .unwrap_or_default();
        self.root.samplers.push(json);
        self.samplers.push(sampler);
        self.samplers.len() - 1
    }

    fn texture(&mut self, map: &Map) -> Index<json::Texture> {
        let image = self.image(&map.image);
        let sampler = map.sampler.map(|s| self.sampler(s));
        let at = match self.textures.iter().position(|t| *t == (image, sampler)) {
            Some(at) => at,
            None => {
                self.root.textures.push(json::Texture {
                    name: None,
                    sampler: sampler.map(|s| Index::new(s as u32)),
                    source: Index::new(image as u32),
                    extensions: None,
                    extras: Default::default(),
                });
                self.textures.push((image, sampler));
                self.textures.len() - 1
            }
        };
        Index::new(at as u32)
    }

    fn info(&mut self, map: &Map) -> json::texture::Info {
        let index = self.texture(map);
        let extensions = map.transform.map(|t| {
            self.uses(TRANSFORM);
            json::extensions::texture::Info {
                texture_transform: Some(json::extensions::texture::TextureTransform {
                    offset: json::extensions::texture::TextureTransformOffset(t.offset),
                    rotation: json::extensions::texture::TextureTransformRotation(t.rotation),
                    scale: json::extensions::texture::TextureTransformScale(t.scale),
                    tex_coord: t.tex_coord,
                    extras: Default::default(),
                }),
                others: Default::default(),
            }
        });
        json::texture::Info {
            index,
            tex_coord: map.tex_coord,
            extensions,
            extras: Default::default(),
        }
    }

    fn uses(&mut self, extension: &str) {
        if !self.root.extensions_used.iter().any(|e| e == extension) {
            self.root.extensions_used.push(extension.to_string());
        }
    }

    fn lobes(&mut self, look: &Look) -> Option<json::extensions::material::Material> {
        let mut others = serde_json::Map::new();
        if let Some(sheen) = look.sheen {
            self.uses(SHEEN);
            others.insert(
                SHEEN.to_string(),
                serde_json::json!({
                    "sheenColorFactor": sheen.color.map(short),
                    "sheenRoughnessFactor": short(sheen.roughness),
                }),
            );
        }
        if let Some(anisotropy) = look.anisotropy {
            self.uses(ANISOTROPY);
            others.insert(
                ANISOTROPY.to_string(),
                serde_json::json!({
                    "anisotropyStrength": short(anisotropy.strength),
                    "anisotropyRotation": short(anisotropy.rotation),
                }),
            );
        }
        (!others.is_empty()).then_some(json::extensions::material::Material { others })
    }

    fn textured(&mut self, name: &str, look: &Look) -> json::Material {
        let base = look.base.as_ref().map(|m| self.info(m));
        let metal_roughness = look.metal_roughness.as_ref().map(|m| self.info(m));
        let normal = look.normal.as_ref().map(|m| json::material::NormalTexture {
            index: self.texture(m),
            scale: m.scale,
            tex_coord: m.tex_coord,
            extensions: None,
            extras: Default::default(),
        });
        let occlusion = look
            .occlusion
            .as_ref()
            .map(|m| json::material::OcclusionTexture {
                index: self.texture(m),
                strength: json::material::StrengthFactor(m.scale),
                tex_coord: m.tex_coord,
                extensions: None,
                extras: Default::default(),
            });
        let extras = look.detail_normal.as_ref().and_then(|m| {
            let index = self.texture(m).value();
            raw(Some(&serde_json::json!({
                DETAIL_NORMAL: { "index": index, "texCoord": m.tex_coord, "scale": short(m.scale) }
            })))
        });
        let mapped = metal_roughness.is_some();
        let base_color =
            look.base_color
                .unwrap_or(if base.is_some() { [1.0; 4] } else { BASE_COLOR });
        let fallback = |mapped_value: f32, plain: f32| if mapped { mapped_value } else { plain };
        json::Material {
            double_sided: true,
            name: Some(name.to_string()),
            pbr_metallic_roughness: json::material::PbrMetallicRoughness {
                base_color_factor: json::material::PbrBaseColorFactor(base_color),
                base_color_texture: base,
                metallic_factor: json::material::StrengthFactor(
                    look.metallic.unwrap_or(fallback(1.0, 0.0)),
                ),
                roughness_factor: json::material::StrengthFactor(
                    look.roughness.unwrap_or(fallback(1.0, ROUGHNESS)),
                ),
                metallic_roughness_texture: metal_roughness,
                ..Default::default()
            },
            normal_texture: normal,
            occlusion_texture: occlusion,
            emissive_factor: json::material::EmissiveFactor(look.emission.unwrap_or([0.0; 3])),
            extensions: self.lobes(look),
            extras,
            ..Default::default()
        }
    }

    fn material(&mut self, name: &str) -> Index<json::material::Material> {
        let at = match self.materials.iter().position(|m| m == name) {
            Some(at) => at,
            None => {
                self.materials.push(name.to_string());
                if let Some(look) = self.looks.get(name).filter(|look| look.carried()) {
                    let material = self.textured(name, look);
                    self.root.materials.push(material);
                    return Index::new(self.materials.len() as u32 - 1);
                }
                self.root.materials.push(plain_material(name));
                self.materials.len() - 1
            }
        };
        Index::new(at as u32)
    }

    fn png_texture(&mut self, png: &[u8]) -> Index<json::Texture> {
        let image = Image {
            bytes: Arc::from(png),
            mime: Mime::Png,
            size: [0, 0],
        };
        let image = self.image(&image);
        let at = match self.textures.iter().position(|t| *t == (image, None)) {
            Some(at) => at,
            None => {
                self.root.textures.push(json::Texture {
                    name: None,
                    sampler: None,
                    source: Index::new(image as u32),
                    extensions: None,
                    extras: Default::default(),
                });
                self.textures.push((image, None));
                self.textures.len() - 1
            }
        };
        Index::new(at as u32)
    }

    fn normal_material(
        &mut self,
        name: &str,
        texture: Index<json::Texture>,
    ) -> Index<json::material::Material> {
        let key = format!("{name}\n{}", texture.value());
        if let Some(at) = self.materials.iter().position(|m| *m == key) {
            return Index::new(at as u32);
        }
        let mut material = match self.looks.get(name).filter(|look| look.carried()) {
            Some(look) => self.textured(name, look),
            None => plain_material(name),
        };
        material.normal_texture = Some(json::material::NormalTexture {
            index: texture,
            scale: 1.0,
            tex_coord: 0,
            extensions: None,
            extras: Default::default(),
        });
        self.materials.push(key);
        self.root.materials.push(material);
        Index::new(self.materials.len() as u32 - 1)
    }

    fn targets(&mut self, morph: &Morph, kept: &[u32]) -> Vec<MorphTarget> {
        let pick = |list: &Vec<[f32; 3]>| -> Vec<[f32; 3]> {
            kept.iter()
                .map(|&old| list.get(old as usize).copied().unwrap_or([0.0; 3]))
                .collect()
        };
        let mut out = Vec::with_capacity(morph.names.len());
        for t in 0..morph.names.len() {
            let deltas = morph.positions.get(t).map(pick).unwrap_or_default();
            let positions = Some(self.vec_accessor(&deltas, Type::Vec3));
            let normals = morph
                .normals
                .get(t)
                .map(pick)
                .map(|n| self.vec_accessor(&n, Type::Vec3));
            out.push(MorphTarget {
                positions,
                normals,
                tangents: None,
            });
        }
        out
    }

    fn primitive(
        &mut self,
        mesh: &Mesh,
        morph: Option<&Morph>,
        material: &str,
        indices: &[u32],
        surface: &Surface,
    ) -> Option<Primitive> {
        let vertices = mesh.positions.len();
        let normals = mesh.normals.len() == vertices;
        let uvs = mesh.uvs.as_ref().filter(|uvs| uvs.len() == vertices);
        let uvs1 = mesh.uvs1.as_ref().filter(|uvs| uvs.len() == vertices);
        let tangents = surface.tangents.filter(|t| t.len() == vertices && normals);
        let mut remap = vec![u32::MAX; vertices];
        let mut positions = Vec::new();
        let mut normal_data = Vec::new();
        let mut uv_data = Vec::new();
        let mut uv1_data = Vec::new();
        let mut tangent_data = Vec::new();
        let mut out = Vec::new();
        let mut kept = Vec::new();
        for triangle in indices.chunks_exact(3) {
            if triangle.iter().any(|&i| i as usize >= vertices) {
                continue;
            }
            for &old in triangle {
                let slot = &mut remap[old as usize];
                if *slot == u32::MAX {
                    *slot = positions.len() as u32;
                    if morph.is_some() {
                        kept.push(old);
                    }
                    positions.push(mesh.positions[old as usize]);
                    if normals {
                        normal_data.push(unit(
                            mesh.normals[old as usize],
                            &mesh.positions,
                            triangle,
                        ));
                    }
                    if let Some(uvs) = uvs {
                        uv_data.push(uvs[old as usize]);
                    }
                    if let Some(uvs1) = uvs1 {
                        uv1_data.push(uvs1[old as usize]);
                    }
                    if let Some(tangents) = tangents {
                        tangent_data.push(tangents[old as usize]);
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
        if tangents.is_some() {
            attributes.insert(
                Checked::Valid(Semantic::Tangents),
                self.vec_accessor(&tangent_data, Type::Vec4),
            );
        }
        let indices = self.index_accessor(&out);
        let targets = morph.map(|morph| self.targets(morph, &kept));
        let material = match surface.normal {
            Some(texture) => self.normal_material(material, texture),
            None => self.material(material),
        };
        Some(Primitive {
            attributes,
            extensions: None,
            extras: Default::default(),
            indices: Some(indices),
            material: Some(material),
            mode: Checked::Valid(Mode::Triangles),
            targets,
        })
    }

    fn primitives(
        &mut self,
        mesh: &Mesh,
        morph: Option<&Morph>,
        surface: &Surface,
    ) -> Vec<Primitive> {
        let mut out = Vec::new();
        if mesh.slots.is_empty() {
            out.extend(self.primitive(mesh, morph, DEFAULT_MATERIAL, &mesh.indices, surface));
            return out;
        }
        for slot in &mesh.slots {
            out.extend(self.primitive(mesh, morph, &slot.material, slice(mesh, slot), surface));
        }
        out
    }

    fn level_primitives(&mut self, base: &Node, level: &Node, surface: &Surface) -> Vec<Primitive> {
        let (mesh, morph) = source(level);
        let (whole, whole_morph) = source(base);
        if whole.slots.is_empty() || mesh.slots.is_empty() {
            return self.primitives(mesh, morph, surface);
        }
        let mut out = Vec::new();
        let mut next = 0;
        for slot in &whole.slots {
            let vertices = whole.positions.len();
            let full = slice(whole, slot);
            if !full
                .chunks_exact(3)
                .any(|t| t.iter().all(|&i| (i as usize) < vertices))
            {
                continue;
            }
            let own = mesh.slots[next.min(mesh.slots.len())..]
                .iter()
                .position(|s| s.material == slot.material)
                .map(|at| next + at);
            let made = own.and_then(|at| {
                next = at + 1;
                self.primitive(
                    mesh,
                    morph,
                    &slot.material,
                    slice(mesh, &mesh.slots[at]),
                    surface,
                )
            });
            let made = made.or_else(|| {
                let piece = smallest(whole, full)?;
                let plain = Surface {
                    tangents: None,
                    normal: surface.normal,
                };
                self.primitive(whole, whole_morph, &slot.material, &piece, &plain)
            });
            out.extend(made);
        }
        out
    }

    fn mesh(
        &mut self,
        name: &str,
        primitives: Vec<Primitive>,
        morph: Option<&Morph>,
    ) -> Option<Index<json::Mesh>> {
        if primitives.is_empty() {
            return None;
        }
        self.root.meshes.push(json::Mesh {
            extensions: None,
            extras: morph.and_then(|m| raw(Some(&serde_json::json!({ "targetNames": m.names })))),
            name: Some(name.to_string()),
            primitives,
            weights: morph.map(|m| m.weights.clone()),
        });
        Some(Index::new(self.root.meshes.len() as u32 - 1))
    }
}

fn unit(normal: [f32; 3], positions: &[[f32; 3]], triangle: &[u32]) -> [f32; 3] {
    let length = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let scaled = |v: [f32; 3], l: f32| [v[0] / l, v[1] / l, v[2] / l];
    let size = length(normal);
    if (size - 1.0).abs() <= 5e-3 {
        return normal;
    }
    if size.is_finite() && size > 1e-12 {
        return scaled(normal, size);
    }
    let [a, b, c] = [0, 1, 2].map(|k| positions[triangle[k] as usize]);
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let face = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let area = length(face);
    if area.is_finite() && area > 1e-20 {
        scaled(face, area)
    } else {
        [0.0, 1.0, 0.0]
    }
}

fn source(node: &Node) -> (&Mesh, Option<&Morph>) {
    match &node.targets {
        Some(targets) if !targets.morph.is_empty() => (&targets.base, Some(&targets.morph)),
        _ => (&node.mesh, None),
    }
}

fn slice<'m>(mesh: &'m Mesh, slot: &manfred_mesh::Slot) -> &'m [u32] {
    let start = (slot.start as usize).min(mesh.indices.len());
    let end = (start + slot.count as usize).min(mesh.indices.len());
    &mesh.indices[start..end]
}

fn smallest(mesh: &Mesh, indices: &[u32]) -> Option<[u32; 3]> {
    let vertices = mesh.positions.len();
    indices
        .chunks_exact(3)
        .filter(|t| t.iter().all(|&i| (i as usize) < vertices))
        .map(|t| {
            let p = |i: u32| glam::Vec3::from(mesh.positions[i as usize]);
            let area = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length();
            (area, [t[0], t[1], t[2]])
        })
        .filter(|(area, _)| *area > 0.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, t)| t)
}

fn plain_material(name: &str) -> json::Material {
    json::Material {
        double_sided: true,
        name: Some(name.to_string()),
        pbr_metallic_roughness: json::material::PbrMetallicRoughness {
            base_color_factor: json::material::PbrBaseColorFactor(BASE_COLOR),
            metallic_factor: json::material::StrengthFactor(0.0),
            roughness_factor: json::material::StrengthFactor(ROUGHNESS),
            ..Default::default()
        },
        ..Default::default()
    }
}

struct Surface<'a> {
    tangents: Option<&'a [[f32; 4]]>,
    normal: Option<Index<json::Texture>>,
}

const PLAIN: Surface<'static> = Surface {
    tangents: None,
    normal: None,
};

fn levels(builder: &mut Builder, nodes: &[Node], lods: &[Lod], placed: &[usize]) {
    let mut ids: BTreeMap<usize, Vec<Value>> = BTreeMap::new();
    for lod in lods {
        let (Some(&master), Some(base)) = (placed.get(lod.of), nodes.get(lod.of)) else {
            continue;
        };
        let normal = lod
            .normal_map
            .as_deref()
            .map(|png| builder.png_texture(png));
        let surface = Surface {
            tangents: lod.tangents.as_deref(),
            normal,
        };
        let primitives = builder.level_primitives(base, &lod.node, &surface);
        let (_, morph) = source(&lod.node);
        let mesh = builder.mesh(&lod.node.name, primitives, morph);
        let owner = &builder.root.nodes[master];
        let node = json::Node {
            matrix: owner.matrix,
            translation: owner.translation,
            rotation: owner.rotation,
            scale: owner.scale,
            mesh,
            name: Some(lod.node.name.clone()),
            extras: raw(Some(&lod.extras)),
            ..Default::default()
        };
        builder.root.nodes.push(node);
        ids.entry(master)
            .or_default()
            .push(Value::from(builder.root.nodes.len() - 1));
    }
    if ids.is_empty() {
        return;
    }
    for (master, list) in ids {
        let mut others = serde_json::Map::new();
        others.insert(MSFT_LOD.to_string(), serde_json::json!({ "ids": list }));
        builder.root.nodes[master].extensions = Some(json::extensions::scene::Node { others });
    }
    builder.root.extensions_used.push(MSFT_LOD.to_string());
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
    write_tree(nodes, &[], extras, &[], order, generator).unwrap_or_default()
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
    lods: &[Lod],
    order: Order,
    generator: &str,
) -> Result<Vec<u8>, Error> {
    let content = Content {
        lods,
        ..Content::default()
    };
    write_animated(nodes, parents, extras, content, order, generator, None)
}

#[derive(Clone, Copy)]
pub struct Content<'a> {
    pub looks: &'a Looks,
    pub lods: &'a [Lod],
}

static NO_LOOKS: Looks = Looks::new();

impl Default for Content<'_> {
    fn default() -> Self {
        Content {
            looks: &NO_LOOKS,
            lods: &[],
        }
    }
}

pub fn write_looks(
    nodes: &[Node],
    parents: &[Option<usize>],
    extras: &[Option<Value>],
    looks: &Looks,
    order: Order,
    generator: &str,
) -> Result<Vec<u8>, Error> {
    let content = Content { looks, lods: &[] };
    write_animated(nodes, parents, extras, content, order, generator, None)
}

fn pose(trs: &Trs) -> ([f32; 3], json::scene::UnitQuaternion, [f32; 3]) {
    (
        trs.translation.map(|v| v as f32),
        json::scene::UnitQuaternion(trs.rotation.map(|v| v as f32)),
        trs.scale.map(|v| v as f32),
    )
}

fn lens(builder: &mut Builder, name: &str, lens: &Lens) -> Index<json::camera::Camera> {
    builder.root.cameras.push(json::camera::Camera {
        name: Some(name.to_string()),
        orthographic: None,
        perspective: Some(json::camera::Perspective {
            aspect_ratio: Some(lens.aspect as f32),
            yfov: lens.yfov as f32,
            zfar: None,
            znear: lens.znear as f32,
            extensions: None,
            extras: Default::default(),
        }),
        type_: Checked::Valid(json::camera::Type::Perspective),
        extensions: None,
        extras: Default::default(),
    });
    Index::new(builder.root.cameras.len() as u32 - 1)
}

fn rotations(keys: &[Trs]) -> Vec<[f32; 4]> {
    let mut out: Vec<[f32; 4]> = Vec::with_capacity(keys.len());
    for key in keys {
        let mut q = key.rotation.map(|v| v as f32);
        if let Some(last) = out.last() {
            let dot: f32 = (0..4).map(|i| last[i] * q[i]).sum();
            if dot < 0.0 {
                q = q.map(|v| -v);
            }
        }
        out.push(q);
    }
    out
}

fn moves<const N: usize>(data: &[[f32; N]]) -> bool {
    data.windows(2).any(|pair| pair[0] != pair[1])
}

fn animate(
    builder: &mut Builder,
    animation: Animation<'_>,
    at: &BTreeMap<String, u32>,
) -> Result<(), Error> {
    let Some(first) = animation.tracks.first() else {
        return Ok(());
    };
    let count = first.keys.len();
    if count == 0 || animation.fps == 0 {
        return Err(Error(format!(
            "the track of `{}` has no keys or the timeline no frame rate",
            first.node
        )));
    }
    if let Some(odd) = animation.tracks.iter().find(|t| t.keys.len() != count) {
        return Err(Error(format!(
            "the track of `{}` has {} keys, expected {count} like the others",
            odd.node,
            odd.keys.len()
        )));
    }
    let mut channels = Vec::new();
    let mut samplers = Vec::new();
    let mut input = None;
    for track in animation.tracks {
        let Some(&node) = at.get(&track.node) else {
            return Err(Error(format!(
                "the track names `{}`, expected a node of the GLB",
                track.node
            )));
        };
        let translations: Vec<[f32; 3]> = track
            .keys
            .iter()
            .map(|k| k.translation.map(|v| v as f32))
            .collect();
        let turns = rotations(&track.keys);
        let scales: Vec<[f32; 3]> = track
            .keys
            .iter()
            .map(|k| k.scale.map(|v| v as f32))
            .collect();
        let mut channel = |builder: &mut Builder, output, path| {
            let times = *input.get_or_insert_with(|| {
                let times: Vec<[f32; 1]> = (0..count)
                    .map(|n| [(n as f64 / f64::from(animation.fps)) as f32])
                    .collect();
                builder.keyed(&times)
            });
            samplers.push(json::animation::Sampler {
                extensions: None,
                extras: Default::default(),
                input: times,
                interpolation: Checked::Valid(json::animation::Interpolation::Linear),
                output,
            });
            channels.push(json::animation::Channel {
                sampler: Index::new(samplers.len() as u32 - 1),
                target: json::animation::Target {
                    extensions: None,
                    extras: Default::default(),
                    node: Index::new(node),
                    path: Checked::Valid(path),
                },
                extensions: None,
                extras: Default::default(),
            });
        };
        if moves(&translations) {
            let output = builder.keyed(&translations);
            channel(builder, output, json::animation::Property::Translation);
        }
        if moves(&turns) {
            let output = builder.keyed(&turns);
            channel(builder, output, json::animation::Property::Rotation);
        }
        if moves(&scales) {
            let output = builder.keyed(&scales);
            channel(builder, output, json::animation::Property::Scale);
        }
    }
    if !channels.is_empty() {
        let markers: Vec<Value> = animation
            .markers
            .iter()
            .map(|(name, t)| serde_json::json!({ "name": name, "t": t }))
            .collect();
        let extras = (!markers.is_empty()).then(|| serde_json::json!({ "markers": markers }));
        builder.root.animations.push(json::Animation {
            extensions: None,
            extras: raw(extras.as_ref()),
            channels,
            name: Some(ANIMATION.to_string()),
            samplers,
        });
    }
    Ok(())
}

pub fn write_animated(
    nodes: &[Node],
    parents: &[Option<usize>],
    extras: &[Option<Value>],
    content: Content<'_>,
    order: Order,
    generator: &str,
    animation: Option<Animation<'_>>,
) -> Result<Vec<u8>, Error> {
    let looks = content.looks;
    let tracks: &[Track] = animation.map_or(&[], |a| a.tracks);
    let keyed: BTreeMap<&str, &Track> = tracks
        .iter()
        .filter(|t| t.camera.is_none())
        .map(|t| (t.node.as_str(), t))
        .collect();
    let mut parents = resolved(parents, nodes.len());
    for (at, node) in nodes.iter().enumerate() {
        if keyed.contains_key(node.name.as_str()) {
            parents[at] = None;
        }
    }
    let mut sequence: Vec<(usize, &Node, Option<&Value>)> = nodes
        .iter()
        .enumerate()
        .map(|(at, node)| (at, node, extras.get(at).and_then(Option::as_ref)))
        .collect();
    if order == Order::Natural {
        sequence.sort_by(|a, b| natural_cmp(&a.1.name, &b.1.name));
    }
    let mut children: Vec<Vec<Index<json::Node>>> = vec![Vec::new(); nodes.len()];
    let mut placed = vec![0; nodes.len()];
    for (position, (at, _, _)) in sequence.iter().enumerate() {
        placed[*at] = position;
        if let Some(parent) = parents[*at] {
            children[parent].push(Index::new(position as u32));
        }
    }
    let mut builder = Builder {
        root: Root::default(),
        bin: Vec::new(),
        materials: Vec::new(),
        looks,
        images: Vec::new(),
        samplers: Vec::new(),
        textures: Vec::new(),
    };
    builder.root.asset.version = "2.0".to_string();
    builder.root.asset.generator = Some(generator.to_string());
    let mut scene_nodes = Vec::new();
    for &(at, node, extras) in &sequence {
        let (mesh, morph) = source(node);
        let primitives = builder.primitives(mesh, morph, &PLAIN);
        let mesh = builder.mesh(&node.name, primitives, morph);
        let children = std::mem::take(&mut children[at]);
        let start = keyed
            .get(node.name.as_str())
            .and_then(|t| t.keys.first())
            .map(pose);
        builder.root.nodes.push(json::Node {
            matrix: if start.is_some() { None } else { matrix(node) },
            mesh,
            name: Some(node.name.clone()),
            extras: raw(extras),
            children: (!children.is_empty()).then_some(children),
            translation: start.map(|p| p.0),
            rotation: start.map(|p| p.1),
            scale: start.map(|p| p.2),
            ..Default::default()
        });
        if parents[at].is_none() {
            scene_nodes.push(Index::new(builder.root.nodes.len() as u32 - 1));
        }
    }
    levels(&mut builder, nodes, content.lods, &placed);
    for track in tracks {
        let camera = match &track.camera {
            Some(lens_of) => Some(lens(&mut builder, &track.node, lens_of)),
            None if track.light_name().is_some()
                && !nodes.iter().any(|node| node.name == track.node) =>
            {
                None
            }
            None => continue,
        };
        let start = track.keys.first().map(pose);
        builder.root.nodes.push(json::Node {
            camera,
            name: Some(track.node.clone()),
            translation: start.map(|p| p.0),
            rotation: start.map(|p| p.1),
            scale: start.map(|p| p.2),
            ..Default::default()
        });
        scene_nodes.push(Index::new(builder.root.nodes.len() as u32 - 1));
    }
    if let Some(animation) = animation {
        let at: BTreeMap<String, u32> = builder
            .root
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| Some((n.name.clone()?, i as u32)))
            .collect();
        animate(&mut builder, animation, &at)?;
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
    let bin = match builder.bin.len() {
        0 => 0,
        len => 8 + len.next_multiple_of(4),
    };
    let total = 20 + text.len().next_multiple_of(4) + bin;
    if u32::try_from(total).is_err() {
        return Err(Error(format!(
            "the GLB comes to {total} bytes, expected at most {} as its lengths are 32-bit",
            u32::MAX
        )));
    }
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
    Ok(out)
}
