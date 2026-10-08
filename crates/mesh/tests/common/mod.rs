use glam::Vec3;
use manfred_mesh::Mesh;
use std::collections::BTreeMap;

pub fn ids(m: &Mesh) -> Vec<u32> {
    let mut seen = BTreeMap::<[u32; 3], u32>::new();
    m.positions
        .iter()
        .map(|p| {
            let n = seen.len() as u32;
            *seen.entry(p.map(|c| (c + 0.0).to_bits())).or_insert(n)
        })
        .collect()
}

pub fn assert_closed(m: &Mesh) {
    let id = ids(m);
    let mut directed = BTreeMap::<(u32, u32), i32>::new();
    for t in m.indices.chunks_exact(3) {
        for e in 0..3 {
            let (a, b) = (id[t[e] as usize], id[t[(e + 1) % 3] as usize]);
            assert_ne!(a, b, "collapsed edge");
            *directed.entry((a, b)).or_default() += 1;
        }
    }
    for (&(a, b), &n) in &directed {
        assert_eq!(n, 1, "edge {a}-{b} used {n} times in one direction");
        assert_eq!(
            directed.get(&(b, a)).copied().unwrap_or(0),
            1,
            "edge {a}-{b} has no opposite half at {:?}",
            m.point(id.iter().position(|&x| x == a).unwrap() as u32)
        );
    }
}

pub fn volume(m: &Mesh) -> f64 {
    m.indices
        .chunks_exact(3)
        .map(|t| m.point(t[0]).dot(m.point(t[1]).cross(m.point(t[2]))))
        .sum::<f64>()
        / 6.0
}

pub fn min_area(m: &Mesh) -> f64 {
    m.indices
        .chunks_exact(3)
        .map(|t| {
            let (a, b, c) = (m.point(t[0]), m.point(t[1]), m.point(t[2]));
            (b - a).cross(c - a).length() * 0.5
        })
        .fold(f64::INFINITY, f64::min)
}

pub fn assert_outward(m: &Mesh) {
    assert!(volume(m) > 0.0, "signed volume {}", volume(m));
}

pub fn assert_sane(m: &Mesh) {
    assert_eq!(m.positions.len(), m.normals.len());
    if let Some(u) = &m.uvs {
        assert_eq!(u.len(), m.positions.len());
    }
    assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
    assert!(
        m.normals
            .iter()
            .all(|n| (Vec3::from(*n).length() - 1.0).abs() < 1e-3)
    );
    let total: u32 = m.slots.iter().map(|s| s.count).sum();
    assert_eq!(total as usize, m.indices.len());
}

pub fn bytes(m: &Mesh) -> Vec<u8> {
    let mut out = Vec::new();
    for p in &m.positions {
        for c in p {
            out.extend(c.to_le_bytes());
        }
    }
    for n in &m.normals {
        for c in n {
            out.extend(c.to_le_bytes());
        }
    }
    if let Some(u) = &m.uvs {
        for p in u {
            for c in p {
                out.extend(c.to_le_bytes());
            }
        }
    }
    for i in &m.indices {
        out.extend(i.to_le_bytes());
    }
    for s in &m.slots {
        out.extend(s.material.as_bytes());
        out.extend(s.start.to_le_bytes());
        out.extend(s.count.to_le_bytes());
    }
    out
}
