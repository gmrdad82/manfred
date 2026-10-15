use crate::Mesh;
use glam::{DMat4, Vec3};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Morph {
    pub names: Vec<String>,
    pub weights: Vec<f32>,
    pub positions: Vec<Vec<[f32; 3]>>,
    pub normals: Vec<Vec<[f32; 3]>>,
}

impl Morph {
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn remap(&mut self, sources: &[u32]) {
        for list in self.positions.iter_mut().chain(self.normals.iter_mut()) {
            *list = sources
                .iter()
                .map(|&s| list.get(s as usize).copied().unwrap_or([0.0; 3]))
                .collect();
        }
    }

    pub fn transform(&mut self, matrix: DMat4) {
        let turn = |list: &mut Vec<[f32; 3]>, m: DMat4| {
            for d in list {
                *d = m
                    .transform_vector3(Vec3::from(*d).as_dvec3())
                    .as_vec3()
                    .to_array();
            }
        };
        for list in &mut self.positions {
            turn(list, matrix);
        }
        let normal = matrix.inverse().transpose();
        for list in &mut self.normals {
            turn(list, normal);
        }
    }

    pub fn pose(&self, m: &mut Mesh) {
        if self.weights.iter().all(|&w| w == 0.0) {
            return;
        }
        let mut turned = false;
        for (t, &w) in self.weights.iter().enumerate() {
            if w == 0.0 {
                continue;
            }
            if let Some(list) = self.positions.get(t) {
                for (p, d) in m.positions.iter_mut().zip(list) {
                    for k in 0..3 {
                        p[k] += w * d[k];
                    }
                }
            }
            if let Some(list) = self.normals.get(t) {
                turned = true;
                for (n, d) in m.normals.iter_mut().zip(list) {
                    for k in 0..3 {
                        n[k] += w * d[k];
                    }
                }
            }
        }
        if turned {
            for n in &mut m.normals {
                *n = Vec3::from(*n).normalize_or_zero().to_array();
            }
        }
    }
}
