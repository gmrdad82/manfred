use crate::{Error, holes, rotation};
use glam::{DMat4, DVec3};
use manfred_mesh::outline::{self, Outline as Ring, Shape};
use manfred_mesh::{self as mesh, Bend, Mesh, Pocket, Profile, ProfileKind, Sheet, Slot, UvMode};
use manfred_recipe::{self as recipe, BendForm, Caps, Kind, Object, Outline, Uv};
use std::path::Path;

const CAPSULE_SEGMENTS: u32 = 32;
const CAPSULE_RINGS: u32 = 8;
const CAP_NORMAL: f64 = std::f64::consts::FRAC_1_SQRT_2;

pub(crate) fn fail(object: &Object, key: &str, text: impl Into<String>) -> Error {
    Error::new(format!("object[{}].{key}", object.name), text)
}

const EDGE: f64 = 1e-6;

const HOLE: &str = "the outline has a hole, and a sweep or a pocket follows one contour";

fn profile(p: &recipe::Profile) -> Profile {
    Profile {
        kind: match p.kind {
            recipe::ProfileKind::Round => ProfileKind::Round,
            recipe::ProfileKind::Chamfer => ProfileKind::Chamfer,
            recipe::ProfileKind::None => ProfileKind::None,
        },
        width: p.width,
        segments: p.segments,
    }
}

pub(crate) fn shapes(o: &Outline) -> Vec<Shape> {
    match o {
        Outline::Rect {
            rect,
            radius,
            turned,
            segments,
        } => {
            let ring = if *turned == 0.0 {
                outline::rounded_rect(*rect, *radius, *segments)
            } else {
                outline::turned(*rect, *radius, *turned, *segments)
            };
            vec![Shape::new(ring)]
        }
        Outline::Circle {
            centre,
            radius,
            segments,
        } => vec![Shape::new(outline::circle(*centre, *radius, *segments))],
        Outline::Points { points, segments } => {
            vec![Shape::new(outline::fillet(points, *segments))]
        }
        Outline::Union(parts) => {
            let parts: Vec<Vec<Shape>> = parts.iter().map(shapes).collect();
            let outers: Vec<Ring> = parts.iter().flatten().map(|s| s.outer.clone()).collect();
            let mut out = outline::union(&outers);
            for (i, part) in parts.iter().enumerate() {
                if part.iter().all(|s| s.holes.is_empty()) {
                    continue;
                }
                let others: Vec<Ring> = parts
                    .iter()
                    .enumerate()
                    .filter(|&(j, _)| j != i)
                    .flat_map(|(_, p)| p.iter().map(|s| s.outer.clone()))
                    .collect();
                for hole in part.iter().flat_map(|s| s.holes.iter()) {
                    let open = outline::difference(&Shape::new(hole.clone()), &others);
                    if open.iter().any(|s| !s.holes.is_empty()) {
                        let every: Vec<Shape> = parts.iter().flatten().cloned().collect();
                        return outline::merge(&every);
                    }
                    let open: Vec<Ring> = open.into_iter().map(|s| s.outer).collect();
                    if !open.is_empty() {
                        out = out
                            .iter()
                            .flat_map(|s| outline::difference(s, &open))
                            .collect();
                    }
                }
            }
            out
        }
        Outline::Difference(a, b) => {
            let cut: Vec<Ring> = shapes(b).into_iter().map(|s| s.outer).collect();
            shapes(a)
                .iter()
                .flat_map(|s| outline::difference(s, &cut))
                .collect()
        }
    }
}

fn single(object: &Object, o: &Outline) -> Result<Ring, Error> {
    let shape = one(object, shapes(o))?;
    if !shape.holes.is_empty() {
        return Err(fail(object, "outline", HOLE));
    }
    Ok(shape.outer)
}

fn one(object: &Object, found: Vec<Shape>) -> Result<Shape, Error> {
    let [shape] = <[Shape; 1]>::try_from(found).map_err(|found| {
        fail(
            object,
            "outline",
            format!(
                "the outline makes {} separate shapes, expected one",
                found.len()
            ),
        )
    })?;
    Ok(shape)
}

fn cut_ring(pocket: &Object, cut: &Outline, into: &str, target: &Shape) -> Result<Ring, Error> {
    let ring = single(pocket, cut)?;
    let area = ring.area().abs();
    let mine = Shape::new(ring.clone());
    let mut outside: f64 = outline::difference(&mine, std::slice::from_ref(&target.outer))
        .iter()
        .map(Shape::area)
        .sum();
    for hole in &target.holes {
        outside += outline::intersection(&mine, &Shape::new(hole.clone()))
            .iter()
            .map(Shape::area)
            .sum::<f64>();
    }
    if outside > area * EDGE {
        return Err(fail(
            pocket,
            "outline",
            format!(
                "the pocket reaches past the edge of `{into}`, with {:.1}% of its area outside it",
                100.0 * outside / area
            ),
        ));
    }
    Ok(ring)
}

pub(crate) fn refusals(object: &Object, pockets: &[&Object]) -> Vec<Error> {
    match &object.kind {
        Kind::Outline {
            outline: o,
            holes,
            profile: p,
            ..
        } => {
            let found = match holes::solid(object, o, holes, p) {
                Ok(found) => found,
                Err(e) => return vec![e],
            };
            if pockets.is_empty() {
                return Vec::new();
            }
            match one(object, found) {
                Err(e) => vec![e],
                Ok(shape) => pockets
                    .iter()
                    .filter_map(|p| match &p.kind {
                        Kind::Pocket { outline: cut, .. } => {
                            cut_ring(p, cut, &object.name, &shape).err()
                        }
                        _ => None,
                    })
                    .collect(),
            }
        }
        Kind::Sweep {
            along: Some(along), ..
        } => single(object, &along.outline).err().into_iter().collect(),
        _ => Vec::new(),
    }
}

fn uv_mode(object: &Object) -> UvMode {
    match object.uv {
        Some(Uv::None) => UvMode::None,
        _ => UvMode::Local,
    }
}

fn card(size: [f64; 2]) -> Mesh {
    let mut m = mesh::grid(size, [1, 1], None);
    if let Some(uvs) = &mut m.uvs {
        for uv in uvs {
            uv[1] = 1.0 - uv[1];
        }
    }
    mesh::transform(&mut m, rotation([90.0, 0.0, 0.0]));
    m
}

fn imported(object: &Object, base: &Path, path: &str, node: Option<&str>) -> Result<Mesh, Error> {
    let file = base.join(path);
    let bytes = std::fs::read(&file)
        .map_err(|e| fail(object, "path", format!("cannot read {path}: {e}")))?;
    let parsed = gltf::Gltf::from_slice(&bytes)
        .map_err(|e| fail(object, "path", format!("{path} is not glTF: {e}")))?;
    let folder = file.parent().unwrap_or(base);
    let buffers = gltf::import_buffers(&parsed.document, Some(folder), parsed.blob.clone())
        .map_err(|e| fail(object, "path", format!("{path}: buffers: {e}")))?;
    let document = &parsed.document;
    let mut out = Mesh::default();
    let mut found = false;
    let roots: Vec<gltf::Node> = match document
        .default_scene()
        .or_else(|| document.scenes().next())
    {
        Some(scene) => scene.nodes().collect(),
        None => document.nodes().collect(),
    };
    let mut stack: Vec<(gltf::Node, DMat4, bool)> = roots
        .into_iter()
        .rev()
        .map(|n| (n, DMat4::IDENTITY, false))
        .collect();
    let mut visited = vec![false; document.nodes().len()];
    while let Some((n, parent, inside)) = stack.pop() {
        if visited
            .get_mut(n.index())
            .is_none_or(|done| std::mem::replace(done, true))
        {
            continue;
        }
        let local = DMat4::from_cols_array_2d(&n.transform().matrix().map(|c| c.map(f64::from)));
        let chosen = inside || node.is_none_or(|want| n.name() == Some(want));
        let matrix = if node.is_some() && !inside && chosen {
            DMat4::IDENTITY
        } else {
            parent * local
        };
        if chosen && let Some(m) = n.mesh() {
            found = true;
            for primitive in m.primitives() {
                if primitive.mode() != gltf::mesh::Mode::Triangles {
                    continue;
                }
                let reader = primitive.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
                let Some(positions) = reader.read_positions() else {
                    continue;
                };
                let positions: Vec<[f32; 3]> = positions.collect();
                let count = positions.len();
                let indices: Vec<u32> = match reader.read_indices() {
                    Some(i) => i
                        .into_u32()
                        .collect::<Vec<u32>>()
                        .chunks_exact(3)
                        .filter(|t| t.iter().all(|&v| (v as usize) < count))
                        .flatten()
                        .copied()
                        .collect(),
                    None => (0..count as u32).collect(),
                };
                let uvs = reader
                    .read_tex_coords(0)
                    .map(|t| t.into_f32().collect::<Vec<[f32; 2]>>())
                    .filter(|uvs| uvs.len() == count);
                let mut part = Mesh {
                    normals: vec![[0.0; 3]; positions.len()],
                    positions,
                    uvs,
                    uvs1: None,
                    indices,
                    slots: Vec::new(),
                };
                part.set_material(primitive.material().name().unwrap_or(""));
                mesh::transform(&mut part, matrix);
                out.append(&part);
            }
        }
        for child in n.children().collect::<Vec<_>>().into_iter().rev() {
            stack.push((child, matrix, chosen));
        }
    }
    match node {
        Some(want) if !found => Err(fail(
            object,
            "node",
            format!("{path} has no node `{want}` with a mesh"),
        )),
        _ => Ok(out),
    }
}

pub(crate) fn local(object: &Object, pockets: &[&Object], base: &Path) -> Result<Mesh, Error> {
    let uv = uv_mode(object);
    Ok(match &object.kind {
        Kind::Box { size } => match &object.bevel {
            Some(b) => mesh::rounded_box(*size, b.width, b.segments),
            None => mesh::rounded_box(*size, 0.0, 1),
        },
        Kind::Cylinder {
            radius,
            depth,
            segments,
            caps,
        } => match &object.bevel {
            Some(b) => {
                let ring = outline::circle([0.0, 0.0], *radius, *segments);
                let edge = Profile::round(b.width, b.segments);
                let mut m = mesh::extrude(&ring, -depth * 0.5, depth * 0.5, &edge, UvMode::None);
                mesh::transform(&mut m, rotation([-90.0, 0.0, 0.0]));
                m
            }
            None => mesh::cylinder(*radius, *depth, *segments, *caps == Caps::Ngon),
        },
        Kind::Sphere {
            radius,
            segments,
            rings,
        } => mesh::sphere(*radius, *segments, *rings),
        Kind::Plane {
            size,
            grid,
            bend,
            curl,
            cup,
            jag,
        } => mesh::sheet(&Sheet {
            size: *size,
            cells: grid.unwrap_or([1, 1]),
            bend: bend.as_ref().map(|b| match b.form {
                BendForm::Wrap => Bend::Wrap(b.radius),
                BendForm::Lift => Bend::Lift(b.radius),
            }),
            curl: curl.as_ref().map(|c| [c.width, c.depth]),
            cup: cup.as_ref().map(|c| [c.from, c.depth]),
            jag: jag.clone(),
        }),
        Kind::Disc { radius, segments } => mesh::disc(*radius, *segments),
        Kind::Torus {
            major,
            minor,
            segments,
            rings,
        } => mesh::torus(*major, *minor, *segments, *rings),
        Kind::Capsule { radius, length } => {
            mesh::capsule(*radius, *length, CAPSULE_SEGMENTS, CAPSULE_RINGS)
        }
        Kind::Outline {
            outline: o,
            holes,
            front,
            back,
            profile: p,
        } => {
            let edge = profile(p);
            let found = holes::solid(object, o, holes, p)?;
            if pockets.is_empty() {
                let mut m = Mesh::default();
                for shape in &found {
                    m.append(&mesh::extrude_shape(shape, *front, *back, &edge, uv));
                }
                m
            } else {
                let shape = one(object, found)?;
                let mut cuts = Vec::new();
                for p in pockets {
                    let Kind::Pocket {
                        outline: cut,
                        depth,
                        rim,
                        floor,
                        ..
                    } = &p.kind
                    else {
                        continue;
                    };
                    cuts.push(Pocket {
                        outline: cut_ring(p, cut, &object.name, &shape)?,
                        depth: *depth,
                        rim: rim.as_ref().map_or(Profile::NONE, profile),
                        floor: floor.as_ref().map_or(Profile::NONE, profile),
                    });
                }
                mesh::pocket_shape(&shape, *front, *back, &edge, &cuts, uv)
            }
        }
        Kind::Pocket { .. } => Mesh::default(),
        Kind::Lathe {
            profile: p,
            segments,
            weld,
        } => mesh::lathe(p, *segments, *weld),
        Kind::Sweep {
            radii,
            ring,
            along: Some(along),
            ..
        } => {
            let [radius] = radii[..] else {
                return Err(fail(
                    object,
                    "radius",
                    format!(
                        "a sweep along an outline takes one radius, found {}",
                        radii.len()
                    ),
                ));
            };
            let outline = single(object, &along.outline)?;
            mesh::bead(&outline, radius, along.depth, along.offset, *ring)
        }
        Kind::Sweep {
            path,
            radii,
            ring,
            closed,
            caps,
            along: None,
        } => mesh::sweep(path, radii, *ring, *closed, *caps),
        Kind::Mesh { path, node } => imported(object, base, path, node.as_deref())?,
        Kind::Generator { generator, .. } => {
            return Err(fail(
                object,
                "generator",
                format!(
                    "`{generator}` is built by the app's own build binary, which registers it through the Manfred library"
                ),
            ));
        }
        Kind::Card { size, .. } => card(*size),
        Kind::Text { .. } => Mesh::default(),
    })
}

fn unweld(m: &mut Mesh) {
    let mut positions = Vec::with_capacity(m.indices.len());
    for &i in &m.indices {
        positions.push(m.positions[i as usize]);
    }
    if let Some(uvs1) = &m.uvs1 {
        m.uvs1 = Some(m.indices.iter().map(|&i| uvs1[i as usize]).collect());
    }
    m.normals = vec![[0.0; 3]; positions.len()];
    m.indices = (0..positions.len() as u32).collect();
    m.positions = positions;
}

fn box_uvs(m: &mut Mesh) {
    let Some([lo, hi]) = mesh::bounds(m) else {
        return;
    };
    let (lo, hi) = (DVec3::from(lo), DVec3::from(hi));
    let span = (hi - lo).max(DVec3::splat(1e-12));
    unweld(m);
    let mut uvs = vec![[0.0f32; 2]; m.positions.len()];
    for t in m.indices.chunks_exact(3) {
        let n = m.face_normal(t);
        let a = n.abs();
        let (u, v) = if a.x >= a.y && a.x >= a.z {
            (2, 1)
        } else if a.y >= a.z {
            (0, 2)
        } else {
            (0, 1)
        };
        for &i in t {
            let q = (m.point(i) - lo) / span;
            uvs[i as usize] = [q[u] as f32, (1.0 - q[v]) as f32];
        }
    }
    m.uvs = Some(uvs);
}

pub(crate) fn uvs(
    recipe: &recipe::Recipe,
    object: &Object,
    matrix: DMat4,
    m: &mut Mesh,
) -> Result<(), Error> {
    match &object.uv {
        _ if matches!(object.kind, Kind::Card { .. }) => {}
        Some(Uv::None) => m.uvs = None,
        None if generated(object) => {}
        Some(Uv::Local) | None => {
            if m.uvs.is_none() && !m.indices.is_empty() {
                box_uvs(m);
            }
        }
        Some(_) => crate::uv::apply(recipe, object, matrix, m)?,
    }
    Ok(())
}

fn groups(object: &Object) -> Option<&'static [&'static str]> {
    match object.kind {
        Kind::Cylinder { .. } => Some(&["side", "caps"]),
        _ => None,
    }
}

fn cylinder_slots(m: &mut Mesh, names: &[String]) {
    let mut side = Vec::new();
    let mut caps = Vec::new();
    for t in m.indices.chunks_exact(3) {
        if m.face_normal(t).y.abs() > CAP_NORMAL {
            caps.extend_from_slice(t);
        } else {
            side.extend_from_slice(t);
        }
    }
    let split = side.len() as u32;
    side.extend(caps);
    m.slots = vec![
        Slot {
            material: names[0].clone(),
            start: 0,
            count: split,
        },
        Slot {
            material: names[1].clone(),
            start: split,
            count: side.len() as u32 - split,
        },
    ];
    m.indices = side;
}

fn generated(object: &Object) -> bool {
    matches!(object.kind, Kind::Generator { .. })
}

pub(crate) fn materials(object: &Object, m: &mut Mesh) -> Result<(), Error> {
    let names = &object.materials;
    match (&object.material, names.len()) {
        (Some(name), _) => m.set_material(name),
        (None, 0) if generated(object) || matches!(object.kind, Kind::Mesh { .. }) => {}
        (None, 0) => m.slots.clear(),
        (None, 1) => m.set_material(&names[0]),
        (None, count) => match groups(object) {
            Some(groups) if groups.len() == count => cylinder_slots(m, names),
            Some(groups) => {
                return Err(fail(
                    object,
                    "materials",
                    format!(
                        "a {} has {} face groups ({}), expected that many names, found {count}",
                        object.kind.name(),
                        groups.len(),
                        groups.join(", ")
                    ),
                ));
            }
            None => {
                return Err(fail(
                    object,
                    "materials",
                    format!(
                        "a {} defines no face groups, expected one name or `material`",
                        object.kind.name()
                    ),
                ));
            }
        },
    }
    Ok(())
}

pub(crate) fn normals(object: &Object, m: &mut Mesh) {
    if m.indices.is_empty() {
        return;
    }
    match (object.smooth_angle, object.smooth) {
        (Some(angle), _) if object.flat_faces => mesh::smooth_by_angle_flat_faces(m, angle),
        (Some(angle), _) => mesh::smooth_by_angle(m, angle),
        (None, true) => mesh::smooth(m),
        (None, false) if generated(object) && carries_normals(m) => {}
        (None, false) => mesh::flat(m),
    }
}

fn carries_normals(m: &Mesh) -> bool {
    m.normals.len() == m.positions.len() && m.normals.iter().any(|n| *n != [0.0; 3])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card_object(uv: &str) -> Object {
        let text = format!(
            "manfred = 1\nname = \"t\"\n\n[[object]]\nname = \"c\"\nkind = \"card\"\nsize = [2.0, 1.0]\nimage = \"a.png\"\n{uv}"
        );
        recipe::parse(&text).unwrap().objects.remove(0)
    }

    #[test]
    fn a_card_quad_spans_its_image_from_the_top_left() {
        let m = card([2.0, 1.0]);
        let uvs = m.uvs.as_ref().unwrap();
        assert_eq!(m.positions.len(), uvs.len());
        assert_eq!(m.indices.len(), 6);
        for (p, uv) in m.positions.iter().zip(uvs) {
            assert!(uv.iter().all(|&v| v == 0.0 || v == 1.0), "{uv:?}");
            let expected = [(uv[0] - 0.5) * 2.0, 0.5 - uv[1], 0.0];
            for k in 0..3 {
                assert!((p[k] - expected[k]).abs() < 1e-6, "{p:?} at {uv:?}");
            }
        }
        let corners: Vec<[f32; 2]> = uvs.clone();
        for want in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]] {
            assert!(corners.contains(&want), "{want:?}");
        }
        for n in &m.normals {
            assert!((n[2] - 1.0).abs() < 1e-6, "{n:?}");
        }
        let t = &m.indices[..3];
        assert!(m.face_normal(t).z > 0.0);
    }

    #[test]
    fn a_card_keeps_its_own_uvs_whatever_its_uv_key_says() {
        for uv in ["", "uv = \"none\"\n", "uv = \"local\"\n"] {
            let object = card_object(uv);
            let recipe = recipe::parse("manfred = 1\nname = \"t\"\n").unwrap();
            let mut m = local(&object, &[], Path::new("")).unwrap();
            let before = m.uvs.clone();
            uvs(&recipe, &object, DMat4::IDENTITY, &mut m).unwrap();
            assert_eq!(m.uvs, before, "{uv}");
            assert!(m.uvs.is_some());
        }
    }
}
