use crate::Error;
use crate::vector::single::{cross, dot, sub, unit};
use manfred_build::Part;
use manfred_glb::{DMat4, Node};
use manfred_mesh::{Mesh, Slot};
use manfred_recipe::{Recipe, Shadow};
use pfx_geom::mesh::Mesh as Geometry;
use pfx_materials::{Family, Material};

const DEFAULT_MATERIAL: &str = "default";
const DEGENERATE: f32 = 1e-14;

pub struct Piece {
    pub geometry: Geometry,
    pub material: String,
    pub object: usize,
    pub name: String,
    pub shadow: Shadow,
    pub two_sided: bool,
}

pub fn pieces(parts: &[Part]) -> Vec<Piece> {
    let mut out = Vec::new();
    for part in parts.iter().filter(|part| !part.hidden) {
        let node = &part.node;
        let world = if node.keep_transform {
            node.transform
        } else {
            DMat4::IDENTITY
        };
        let normals = world.inverse().transpose();
        let flip = world.determinant() < 0.0;
        let positions: Vec<[f32; 3]> = node
            .mesh
            .positions
            .iter()
            .map(|p| {
                world
                    .transform_point3([p[0], p[1], p[2]].map(f64::from).into())
                    .to_array()
                    .map(|v| v as f32)
            })
            .collect();
        let turned: Vec<[f32; 3]> = node
            .mesh
            .normals
            .iter()
            .map(|n| {
                let n = normals
                    .transform_vector3([n[0], n[1], n[2]].map(f64::from).into())
                    .normalize_or_zero();
                n.to_array().map(|v| v as f32)
            })
            .collect();
        let whole = [Slot {
            material: DEFAULT_MATERIAL.into(),
            start: 0,
            count: node.mesh.indices.len() as u32,
        }];
        let slots = if node.mesh.slots.is_empty() {
            &whole[..]
        } else {
            &node.mesh.slots[..]
        };
        let uvs = node.mesh.uvs.as_deref().unwrap_or_default();
        for slot in slots {
            let start = (slot.start as usize).min(node.mesh.indices.len());
            let end = (start + slot.count as usize).min(node.mesh.indices.len());
            let indices = kept(&positions, &node.mesh.indices[start..end], flip);
            if indices.is_empty() {
                continue;
            }
            let geometry = if turned.len() == positions.len() {
                compact(&positions, &turned, uvs, &indices)
            } else {
                flat(&positions, uvs, &indices)
            };
            out.push(Piece {
                geometry,
                material: slot.material.clone(),
                object: part.object,
                name: node.name.clone(),
                shadow: part.shadow,
                two_sided: part.two_sided,
            });
        }
    }
    out
}

fn kept(positions: &[[f32; 3]], indices: &[u32], flip: bool) -> Vec<u32> {
    let mut out = Vec::with_capacity(indices.len());
    for tri in indices.chunks_exact(3) {
        if tri.iter().any(|&i| i as usize >= positions.len()) {
            continue;
        }
        let p: [[f32; 3]; 3] = std::array::from_fn(|k| positions[tri[k] as usize]);
        let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
        if dot(c, c).sqrt() < DEGENERATE {
            continue;
        }
        if flip {
            out.extend_from_slice(&[tri[0], tri[2], tri[1]]);
        } else {
            out.extend_from_slice(tri);
        }
    }
    out
}

fn compact(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    uvs: &[[f32; 2]],
    indices: &[u32],
) -> Geometry {
    let mut map = vec![u32::MAX; positions.len()];
    let mut p = Vec::new();
    let mut n = Vec::new();
    let mut t = Vec::new();
    let mut out = Vec::with_capacity(indices.len());
    for &index in indices {
        let slot = &mut map[index as usize];
        if *slot == u32::MAX {
            *slot = p.len() as u32;
            p.push(positions[index as usize]);
            n.push(normals[index as usize]);
            t.push(uvs.get(index as usize).copied().unwrap_or([0.0; 2]));
        }
        out.push(*slot);
    }
    Geometry::new(p, n, Vec::new(), t, out)
}

fn flat(positions: &[[f32; 3]], uvs: &[[f32; 2]], indices: &[u32]) -> Geometry {
    let mut p = Vec::with_capacity(indices.len());
    let mut n = Vec::with_capacity(indices.len());
    let mut t = Vec::with_capacity(indices.len());
    for tri in indices.chunks_exact(3) {
        let corners: [[f32; 3]; 3] = std::array::from_fn(|k| positions[tri[k] as usize]);
        let normal = unit(cross(
            sub(corners[1], corners[0]),
            sub(corners[2], corners[0]),
        ));
        for (corner, &index) in corners.iter().zip(tri) {
            p.push(*corner);
            n.push(normal);
            t.push(uvs.get(index as usize).copied().unwrap_or([0.0; 2]));
        }
    }
    let out = (0..p.len() as u32).collect();
    Geometry::new(p, n, Vec::new(), t, out)
}

pub fn material(recipe: &Recipe, name: &str) -> Result<Option<Material>, Error> {
    recipe
        .materials
        .get(name)
        .map(|value| {
            serde_json::from_value::<Material>(value.clone())
                .map_err(|e| Error(format!("materials.{name}: {e}")))
        })
        .transpose()
}

fn missing(recipe: &Recipe, piece: &Piece) -> Error {
    Error(format!(
        "object {}: {}",
        piece.name,
        manfred_recipe::missing_material_text(
            recipe.materials.keys().map(String::as_str),
            &piece.material
        )
    ))
}

pub struct Palette {
    pub names: Vec<String>,
    pub materials: Vec<Material>,
    pub slots: Vec<u32>,
}

pub fn palette(
    recipe: &Recipe,
    pieces: &[Piece],
    carded: impl IntoIterator<Item = bool>,
) -> Result<Palette, Error> {
    let mut keys: Vec<(&str, bool)> = Vec::new();
    let mut names = Vec::new();
    let mut materials = Vec::new();
    let mut slots = Vec::with_capacity(pieces.len());
    for (piece, card) in pieces.iter().zip(carded) {
        let key = (piece.material.as_str(), card);
        let index = match keys.iter().position(|known| *known == key) {
            Some(index) => index,
            None => {
                let Some(material) = material(recipe, &piece.material)? else {
                    return Err(missing(recipe, piece));
                };
                keys.push(key);
                names.push(piece.material.clone());
                materials.push(if card {
                    crate::cards::layered(material)
                } else {
                    material
                });
                keys.len() - 1
            }
        };
        slots.push(index as u32);
    }
    Ok(Palette {
        names,
        materials,
        slots,
    })
}

pub fn glassy(material: &Material) -> bool {
    material.transmission > 0.0 || matches!(material.family, Family::Glass | Family::Liquid)
}

pub fn glb_parts(bytes: &[u8]) -> Result<Vec<Part>, Error> {
    let mesh = pfx_load::Mesh::parse(bytes).map_err(Error)?;
    let mut objects: Vec<String> = Vec::new();
    let mut parts = Vec::new();
    let mut stack: Vec<(u32, DMat4)> = mesh
        .all_roots()
        .into_iter()
        .rev()
        .map(|root| (root, DMat4::IDENTITY))
        .collect();
    let mut visits = 0usize;
    while let Some((index, parent)) = stack.pop() {
        visits += 1;
        if visits > mesh.nodes.len() * 4 + 16 {
            return Err(Error("glb: the node tree has a cycle".into()));
        }
        let Some(node) = mesh.nodes.get(index as usize) else {
            return Err(Error(format!("glb: node {index} does not exist")));
        };
        let local = DMat4::from_cols_array_2d(&node.transform.map(|c| c.map(f64::from)));
        let world = parent * local;
        if let Some(group) = node.group.and_then(|g| mesh.groups.get(g as usize)) {
            let name = if !node.name.is_empty() {
                node.name.clone()
            } else if !group.name.is_empty() {
                group.name.clone()
            } else {
                "object".to_string()
            };
            let object = match objects.iter().position(|o| *o == name) {
                Some(found) => found,
                None => {
                    objects.push(name.clone());
                    objects.len() - 1
                }
            };
            let mut built = Mesh {
                positions: Vec::new(),
                normals: Vec::new(),
                uvs: Some(Vec::new()),
                uvs1: None,
                indices: Vec::new(),
                slots: Vec::new(),
            };
            let mut smooth = true;
            for primitive in &group.primitives {
                let material = primitive
                    .material
                    .and_then(|m| mesh.materials.get(m as usize))
                    .map(|m| m.name.as_str())
                    .filter(|n| !n.is_empty())
                    .unwrap_or(DEFAULT_MATERIAL)
                    .to_string();
                let base = built.positions.len() as u32;
                let start = built.indices.len() as u32;
                smooth &= primitive.normals.len() == primitive.positions.len();
                built.positions.extend(&primitive.positions);
                built.normals.extend(&primitive.normals);
                if let Some(uvs) = &mut built.uvs {
                    uvs.extend(
                        (0..primitive.positions.len())
                            .map(|i| primitive.uvs.get(i).copied().unwrap_or([0.0; 2])),
                    );
                }
                built
                    .indices
                    .extend(primitive.indices.iter().map(|&i| i + base));
                built.slots.push(Slot {
                    material,
                    start,
                    count: primitive.indices.len() as u32,
                });
            }
            if !smooth {
                built.normals.clear();
            }
            parts.push(Part {
                node: Node {
                    name,
                    mesh: built,
                    transform: world,
                    keep_transform: true,
                },
                object,
                shadow: Shadow::Cast,
                two_sided: false,
                hidden: false,
            });
        }
        for &child in node.children.iter().rev() {
            stack.push((child, world));
        }
    }
    Ok(parts)
}

pub fn bounds(pieces: &[Piece], visible: bool) -> Option<[[f64; 3]; 2]> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for piece in pieces {
        if visible && piece.shadow == Shadow::Only {
            continue;
        }
        for p in &piece.geometry.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(f64::from(p[k]));
                hi[k] = hi[k].max(f64::from(p[k]));
            }
        }
    }
    lo[0].is_finite().then_some([lo, hi])
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_glb::{Order, write};
    use manfred_recipe::parse;

    fn quad(material: &str) -> Mesh {
        Mesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0],
            ],
            normals: vec![[0.0, 0.0, 1.0]; 5],
            uvs1: None,
            uvs: None,
            indices: vec![0, 1, 2, 0, 2, 3, 0, 4, 1],
            slots: vec![Slot {
                material: material.into(),
                start: 0,
                count: 9,
            }],
        }
    }

    fn part(name: &str, object: usize, mesh: Mesh, transform: DMat4, keep: bool) -> Part {
        Part {
            node: Node {
                name: name.into(),
                mesh,
                transform,
                keep_transform: keep,
            },
            object,
            shadow: Shadow::Cast,
            two_sided: false,
            hidden: false,
        }
    }

    #[test]
    fn pieces_drop_degenerate_triangles_and_hidden_parts() {
        let mut hidden = part("cutter", 1, quad("chrome"), DMat4::IDENTITY, false);
        hidden.hidden = true;
        let shown = part("plate", 0, quad("brass"), DMat4::IDENTITY, false);
        let out = pieces(&[shown, hidden]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].material, "brass");
        assert_eq!(out[0].geometry.indices.len(), 6);
        assert_eq!(out[0].geometry.positions.len(), 4);
        assert_eq!(out[0].geometry.tangents.len(), 4);
    }

    #[test]
    fn a_kept_transform_moves_and_turns_the_piece() {
        let mirror = DMat4::from_scale([-1.0, 1.0, 1.0].into());
        let moved = DMat4::from_translation([0.0, 2.0, 0.0].into());
        let out = pieces(&[
            part("a", 0, quad("m"), moved, true),
            part("b", 1, quad("m"), moved, false),
            part("c", 2, quad("m"), mirror, true),
        ]);
        assert_eq!(out[0].geometry.positions[0], [0.0, 2.0, 0.0]);
        assert_eq!(out[1].geometry.positions[0], [0.0, 0.0, 0.0]);
        let mirrored = &out[2].geometry;
        let corner = |k: usize| mirrored.positions[mirrored.indices[k] as usize];
        let face = cross(sub(corner(1), corner(0)), sub(corner(2), corner(0)));
        assert!(face[2] > 0.0, "{face:?}");
        assert_eq!(mirrored.normals[0], [0.0, 0.0, 1.0]);
        assert!(mirrored.positions.contains(&[-1.0, 0.0, 0.0]));
    }

    #[test]
    fn materials_resolve_by_name_through_the_recipe_only() {
        let recipe = parse(
            "manfred = 1\nname = \"t\"\n[materials.mine]\nbase = [0.1, 0.2, 0.3]\nroughness = 0.4\n",
        )
        .unwrap();
        for name in ["lacquer_orange", "chrome"] {
            assert_eq!(material(&recipe, name).unwrap(), None, "{name}");
        }
        let mine = material(&recipe, "mine").unwrap().unwrap();
        assert_eq!(mine.base, [0.1, 0.2, 0.3]);
        assert_eq!(material(&recipe, "no_such_material").unwrap(), None);
    }

    #[test]
    fn a_material_in_neither_the_catalogue_nor_the_recipe_is_a_clear_error() {
        let recipe = parse(
            "manfred = 1\nname = \"t\"\n[materials.mine]\nbase = [0.1, 0.2, 0.3]\nroughness = 0.4\n",
        )
        .unwrap();
        let out = pieces(&[part("a", 0, quad("chrome"), DMat4::IDENTITY, false)]);
        let error = palette(&recipe, &out, [false]).err().unwrap();
        assert!(
            error.0.starts_with("object a: material \"chrome\""),
            "{error}"
        );
        assert!(error.0.contains("(it defines mine)"), "{error}");
        assert!(error.0.contains("[materials.chrome]"), "{error}");
        let bare = parse("manfred = 1\nname = \"t\"\n").unwrap();
        let error = palette(&bare, &out, [false]).err().unwrap();
        assert!(error.0.contains("(it defines none)"), "{error}");
        let mine = pieces(&[part("a", 0, quad("mine"), DMat4::IDENTITY, false)]);
        assert_eq!(palette(&recipe, &mine, [false]).unwrap().names, ["mine"]);
    }

    #[test]
    fn a_glb_becomes_parts_per_object_in_world_space() {
        let nodes = [
            Node {
                name: "shell".into(),
                mesh: quad("lacquer_orange"),
                transform: DMat4::from_translation([0.0, 0.0, 1.0].into()),
                keep_transform: true,
            },
            Node {
                name: "knob".into(),
                mesh: quad("brass"),
                transform: DMat4::IDENTITY,
                keep_transform: false,
            },
        ];
        let bytes = write(&nodes, Order::File, "manfred test");
        let parts = glb_parts(&bytes).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].node.name, "shell");
        assert_eq!(parts[0].object, 0);
        assert_eq!(parts[1].object, 1);
        assert_eq!(parts[0].node.mesh.slots[0].material, "lacquer_orange");
        let out = pieces(&parts);
        assert_eq!(out[0].geometry.positions[0], [0.0, 0.0, 1.0]);
        assert_eq!(out[1].material, "brass");
        assert!(glb_parts(b"not a glb").is_err());
    }
}
