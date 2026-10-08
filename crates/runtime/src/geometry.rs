use crate::raster::IDENTITY;
use crate::scene::{Piece, Tri};
use pfx_load::Mesh;

type Matrix = [[f32; 4]; 4];

#[derive(Default)]
pub(crate) struct Geometry {
    pub tris: Vec<Tri>,
    pub pieces: Vec<Piece>,
    pub parts: u32,
    pub materials: Vec<String>,
    pub objects: Vec<String>,
    pub groups: Vec<Option<String>>,
}

pub(crate) fn build(mesh: &Mesh) -> Geometry {
    let mut geometry = Geometry::default();
    for root in mesh.all_roots() {
        visit(mesh, root, IDENTITY, &mut geometry);
    }
    geometry
}

fn mul(a: Matrix, b: Matrix) -> Matrix {
    let mut r = [[0.0; 4]; 4];
    for c in 0..4 {
        for row in 0..4 {
            r[c][row] = (0..4).map(|k| a[k][row] * b[c][k]).sum();
        }
    }
    r
}

fn point(m: &Matrix, p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|row| m[0][row] * p[0] + m[1][row] * p[1] + m[2][row] * p[2] + m[3][row])
}

fn normal_matrix(m: &Matrix) -> [[f32; 3]; 3] {
    let a = [
        [m[0][0], m[1][0], m[2][0]],
        [m[0][1], m[1][1], m[2][1]],
        [m[0][2], m[1][2], m[2][2]],
    ];
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    let inv_det = if det.abs() > 1e-20 { 1.0 / det } else { 0.0 };
    let cof = [
        [
            a[1][1] * a[2][2] - a[1][2] * a[2][1],
            a[1][2] * a[2][0] - a[1][0] * a[2][2],
            a[1][0] * a[2][1] - a[1][1] * a[2][0],
        ],
        [
            a[0][2] * a[2][1] - a[0][1] * a[2][2],
            a[0][0] * a[2][2] - a[0][2] * a[2][0],
            a[0][1] * a[2][0] - a[0][0] * a[2][1],
        ],
        [
            a[0][1] * a[1][2] - a[0][2] * a[1][1],
            a[0][2] * a[1][0] - a[0][0] * a[1][2],
            a[0][0] * a[1][1] - a[0][1] * a[1][0],
        ],
    ];
    cof.map(|row| row.map(|v| v * inv_det))
}

fn apply3(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    let r = [0, 1, 2].map(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2]);
    let len = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt().max(1e-20);
    r.map(|x| x / len)
}

fn intern(list: &mut Vec<String>, name: &str) -> u32 {
    match list.iter().position(|n| n == name) {
        Some(i) => i as u32,
        None => {
            list.push(name.to_string());
            (list.len() - 1) as u32
        }
    }
}

fn visit(mesh: &Mesh, node: u32, parent: Matrix, out: &mut Geometry) {
    let node = &mesh.nodes[node as usize];
    let world = mul(parent, node.transform);
    if let Some(group) = node.group.map(|index| &mesh.groups[index as usize]) {
        let name = if !node.name.is_empty() {
            &node.name
        } else if !group.name.is_empty() {
            &group.name
        } else {
            "object"
        };
        let object = intern(&mut out.objects, name);
        out.groups.resize(out.objects.len(), None);
        if out.groups[object as usize].is_none() {
            out.groups[object as usize] = node
                .extra_object("manfred")
                .and_then(|manfred| manfred.get("group"))
                .and_then(|group| group.as_str())
                .map(str::to_string);
        }
        let nm = normal_matrix(&world);
        for prim in &group.primitives {
            let part = out.parts;
            out.parts += 1;
            let positions: Vec<[f32; 3]> = prim
                .positions
                .iter()
                .map(|&position| point(&world, position))
                .collect();
            let normals = (!prim.normals.is_empty()).then(|| {
                prim.normals
                    .iter()
                    .map(|&normal| apply3(&nm, normal))
                    .collect::<Vec<_>>()
            });
            let uvs = (!prim.uvs.is_empty()).then_some(&prim.uvs);
            let material_name = prim
                .material
                .map(|index| mesh.materials[index as usize].name.as_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("default");
            let material = intern(&mut out.materials, material_name);
            let mut kept = Vec::new();
            let mut unwelded = (Vec::new(), Vec::new(), Vec::new());
            for t in prim.indices.chunks_exact(3) {
                let p = [
                    positions[t[0] as usize],
                    positions[t[1] as usize],
                    positions[t[2] as usize],
                ];
                let e1 = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
                let e2 = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
                let c = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                let len = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
                if len < 1e-14 {
                    continue;
                }
                let flat = c.map(|x| x / len);
                let n = match &normals {
                    Some(ns) => [ns[t[0] as usize], ns[t[1] as usize], ns[t[2] as usize]],
                    None => [flat; 3],
                };
                let uv = match &uvs {
                    Some(us) => [us[t[0] as usize], us[t[1] as usize], us[t[2] as usize]],
                    None => [[0.0; 2]; 3],
                };
                if normals.is_some() {
                    kept.extend_from_slice(t);
                } else {
                    unwelded.0.extend(p);
                    unwelded.1.extend(n);
                    unwelded.2.extend(uv);
                }
                out.tris.push(Tri {
                    p,
                    n,
                    uv,
                    material,
                    object,
                });
            }
            let piece = match normals {
                Some(normals) if !kept.is_empty() => {
                    Some((positions, normals, prim.uvs.clone(), kept))
                }
                None if !unwelded.0.is_empty() => {
                    let count = unwelded.0.len() as u32;
                    Some((unwelded.0, unwelded.1, unwelded.2, (0..count).collect()))
                }
                _ => None,
            };
            if let Some((positions, normals, uvs, indices)) = piece {
                out.pieces.push(Piece {
                    positions,
                    normals,
                    uvs,
                    indices,
                    material,
                    object,
                    part,
                });
            }
        }
    }
    for &child in &node.children {
        visit(mesh, child, world, out);
    }
}
