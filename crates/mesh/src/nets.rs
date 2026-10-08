use crate::Mesh;
use glam::DVec3;

pub trait Field {
    fn value(&self, p: DVec3) -> f64;
    fn normal(&self, p: DVec3) -> DVec3;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lattice {
    pub min: DVec3,
    pub step: f64,
    pub cells: [usize; 3],
}

impl Lattice {
    pub fn around(min: DVec3, max: DVec3, step: f64) -> Lattice {
        let pad = 2.0 * step;
        let size = max - min;
        let cells = [size.x, size.y, size.z].map(|s| (s / step).ceil() as usize + 4);
        Lattice {
            min: min - DVec3::splat(pad),
            step,
            cells,
        }
    }

    fn corner(&self, i: usize, j: usize, k: usize) -> DVec3 {
        self.min + DVec3::new(i as f64, j as f64, k as f64) * self.step
    }
}

const CORNERS: [[usize; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [0, 1, 0],
    [1, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [0, 1, 1],
    [1, 1, 1],
];

const EDGES: [[usize; 2]; 12] = [
    [0, 1],
    [2, 3],
    [4, 5],
    [6, 7],
    [0, 2],
    [1, 3],
    [4, 6],
    [5, 7],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
];

pub fn surface_nets(field: &impl Field, lattice: &Lattice) -> Mesh {
    let [cx, cy, cz] = lattice.cells;
    let (nx, ny, nz) = (cx + 1, cy + 1, cz + 1);
    let at = |i: usize, j: usize, k: usize| (k * ny + j) * nx + i;
    let mut values = vec![0.0f64; nx * ny * nz];
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                values[at(i, j, k)] = field.value(lattice.corner(i, j, k));
            }
        }
    }
    let cell_at = |i: usize, j: usize, k: usize| (k * cy + j) * cx + i;
    let mut vertex = vec![u32::MAX; cx * cy * cz];
    let mut mesh = Mesh::default();
    for k in 0..cz {
        for j in 0..cy {
            for i in 0..cx {
                let v = CORNERS.map(|c| values[at(i + c[0], j + c[1], k + c[2])]);
                let inside = v.iter().filter(|x| **x < 0.0).count();
                if inside == 0 || inside == 8 {
                    continue;
                }
                let mut sum = DVec3::ZERO;
                let mut count = 0.0;
                for [a, b] in EDGES {
                    if (v[a] < 0.0) == (v[b] < 0.0) {
                        continue;
                    }
                    let t = v[a] / (v[a] - v[b]);
                    let pa =
                        lattice.corner(i + CORNERS[a][0], j + CORNERS[a][1], k + CORNERS[a][2]);
                    let pb =
                        lattice.corner(i + CORNERS[b][0], j + CORNERS[b][1], k + CORNERS[b][2]);
                    sum += pa + (pb - pa) * t;
                    count += 1.0;
                }
                let p = settle(field, sum / count, lattice.step);
                vertex[cell_at(i, j, k)] = mesh.positions.len() as u32;
                mesh.positions.push(p.as_vec3().to_array());
                mesh.normals.push(field.normal(p).as_vec3().to_array());
            }
        }
    }
    let mut indices = Vec::new();
    let mut quad = |q: [u32; 4], outward: bool| {
        let [a, b, c, d] = if outward { q } else { [q[3], q[2], q[1], q[0]] };
        let ac = mesh.point(a).distance_squared(mesh.point(c));
        if ac <= mesh.point(b).distance_squared(mesh.point(d)) {
            indices.extend([a, b, c, a, c, d]);
        } else {
            indices.extend([a, b, d, b, c, d]);
        }
    };
    for axis in 0..3 {
        let from = |a: usize| usize::from(a != axis);
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for k in from(2)..cz {
            for j in from(1)..cy {
                for i in from(0)..cx {
                    let mut next = [i, j, k];
                    next[axis] += 1;
                    let (s, e) = (values[at(i, j, k)], values[at(next[0], next[1], next[2])]);
                    if (s < 0.0) != (e < 0.0) {
                        let cell = |du: usize, dv: usize| {
                            let mut c = [i, j, k];
                            c[u] -= du;
                            c[v] -= dv;
                            vertex[cell_at(c[0], c[1], c[2])]
                        };
                        quad([cell(1, 1), cell(0, 1), cell(0, 0), cell(1, 0)], s < 0.0);
                    }
                }
            }
        }
    }
    mesh.indices = indices;
    mesh
}

fn settle(field: &impl Field, p: DVec3, step: f64) -> DVec3 {
    let n = field.normal(p);
    if n == DVec3::ZERO {
        return p;
    }
    for reach in [step, 2.0 * step] {
        let (mut low, mut high) = (p - n * reach, p + n * reach);
        if field.value(low) >= 0.0 || field.value(high) < 0.0 {
            continue;
        }
        for _ in 0..60 {
            let mid = (low + high) * 0.5;
            let side = if field.value(mid) < 0.0 {
                &mut low
            } else {
                &mut high
            };
            if *side == mid {
                break;
            }
            *side = mid;
        }
        return (low + high) * 0.5;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Ball(f64);

    impl Field for Ball {
        fn value(&self, p: DVec3) -> f64 {
            p.length() - self.0
        }

        fn normal(&self, p: DVec3) -> DVec3 {
            p.normalize_or_zero()
        }
    }

    fn balanced(mesh: &Mesh) -> bool {
        let mut edges: BTreeMap<(u32, u32), i32> = BTreeMap::new();
        for t in mesh.indices.chunks_exact(3) {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *edges.entry((a.min(b), a.max(b))).or_default() += if a < b { 1 } else { -1 };
            }
        }
        edges.values().all(|n| *n == 0)
    }

    #[test]
    fn a_ball_meshes_closed_on_its_surface_facing_out() {
        let ball = Ball(0.5);
        let lattice = Lattice::around(DVec3::splat(-0.5), DVec3::splat(0.5), 0.05);
        let mesh = surface_nets(&ball, &lattice);
        assert!(mesh.triangle_count() > 1000);
        assert!(balanced(&mesh));
        for p in &mesh.positions {
            assert!((DVec3::from(p.map(f64::from)).length() - 0.5).abs() < 1e-6);
        }
        for t in mesh.indices.chunks_exact(3) {
            let centre = (mesh.point(t[0]) + mesh.point(t[1]) + mesh.point(t[2])) / 3.0;
            assert!(mesh.face_normal(t).dot(centre) > 0.0);
        }
    }

    #[test]
    fn the_same_field_gives_the_same_mesh() {
        let lattice = Lattice::around(DVec3::splat(-0.3), DVec3::splat(0.3), 0.04);
        assert_eq!(
            surface_nets(&Ball(0.3), &lattice),
            surface_nets(&Ball(0.3), &lattice)
        );
    }
}
