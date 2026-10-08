use manfred_mesh::{Mesh, flat, smooth_by_angle, weld};

type Corner = ([f32; 3], [f32; 2], Option<[f32; 2]>);

fn corners(m: &Mesh) -> Vec<Corner> {
    m.indices
        .iter()
        .map(|&i| {
            let i = i as usize;
            (
                m.positions[i],
                m.uvs.as_ref().unwrap()[i],
                m.uvs1.as_ref().map(|u| u[i]),
            )
        })
        .collect()
}

fn fold() -> Mesh {
    Mesh {
        positions: vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 0.0, -1.0],
            [1.0, 1.0, -1.0],
        ],
        normals: vec![[0.0; 3]; 6],
        uvs: Some(vec![
            [0.0, 1.0],
            [0.5, 1.0],
            [0.5, 0.0],
            [0.0, 0.0],
            [1.0, 1.0],
            [1.0, 0.0],
        ]),
        uvs1: Some(vec![
            [0.1, 0.1],
            [0.2, 0.1],
            [0.2, 0.2],
            [0.1, 0.2],
            [0.6, 0.1],
            [0.6, 0.2],
        ]),
        indices: vec![0, 1, 2, 0, 2, 3, 1, 4, 5, 1, 5, 2],
        slots: Vec::new(),
    }
}

#[test]
fn split_normals_keep_every_corners_second_uv() {
    for degrees in [0.01, 30.0, 180.0] {
        let mut m = fold();
        let before = corners(&m);
        smooth_by_angle(&mut m, degrees);
        assert_eq!(corners(&m), before, "{degrees}");
        assert_eq!(m.uvs1.as_ref().unwrap().len(), m.positions.len());
    }
}

#[test]
fn split_normals_part_vertices_where_the_second_uv_differs() {
    let mut m = fold();
    m.uvs1.as_mut().unwrap().push([0.9, 0.9]);
    m.positions.push([1.0, 0.0, 0.0]);
    m.normals.push([0.0; 3]);
    m.uvs.as_mut().unwrap().push([0.5, 1.0]);
    m.indices[6] = 6;
    let before = corners(&m);
    smooth_by_angle(&mut m, 180.0);
    assert_eq!(corners(&m), before);
    let seam = m
        .positions
        .iter()
        .filter(|p| **p == [1.0, 0.0, 0.0])
        .count();
    assert_eq!(seam, 2);
}

#[test]
fn append_fills_a_missing_second_uv_with_zeros() {
    let mut m = fold();
    let mut other = fold();
    other.uvs1 = None;
    m.append(&other);
    let uvs1 = m.uvs1.as_ref().unwrap();
    assert_eq!(uvs1.len(), 12);
    assert!(uvs1[6..].iter().all(|uv| *uv == [0.0, 0.0]));
    let mut plain = fold();
    plain.uvs1 = None;
    let mut joined = plain.clone();
    joined.append(&plain);
    assert_eq!(joined.uvs1, None);
    let mut late = plain.clone();
    late.append(&fold());
    let uvs1 = late.uvs1.as_ref().unwrap();
    assert!(uvs1[..6].iter().all(|uv| *uv == [0.0, 0.0]));
    assert_eq!(uvs1[6..], fold().uvs1.unwrap()[..]);
}

#[test]
fn weld_and_flat_keep_the_second_uv_beside_its_vertex() {
    let mut m = fold();
    m.set_material("m");
    let before = corners(&m);
    weld(&mut m, 1e-9);
    assert_eq!(corners(&m), before);
    flat(&mut m);
    assert_eq!(corners(&m), before);
}
