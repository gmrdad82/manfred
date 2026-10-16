use crate::{Error, fail, normal_map};
use manfred_glb::{Lod, Node, Targets};
use manfred_mesh::{Mesh, simplify, tangents};
use manfred_recipe::{LodTarget, Object};
use serde_json::json;

pub const DISTANCE_SHARE: f64 = 0.01;
pub const DISTANCE_ERRORS: f64 = 2.0;

pub(crate) fn levels(object: &Object, master: &Node) -> Result<Vec<Lod>, Error> {
    if object.lod.is_empty() {
        return Ok(Vec::new());
    }
    let base = master.targets.as_ref().map_or(&master.mesh, |t| &t.base);
    let morph = master.targets.as_ref().map(|t| &t.morph);
    let triangles = base.triangle_count();
    let mut out = Vec::with_capacity(object.lod.len());
    for (k, level) in object.lod.iter().enumerate() {
        let at = |key: &str| format!("lod[{k}].{key}");
        let target = match level.target {
            LodTarget::Triangles(n) => n as usize,
            LodTarget::Ratio(r) => ((triangles as f64 * r).round() as usize).max(1),
        };
        if target >= triangles {
            let key = match level.target {
                LodTarget::Triangles(_) => "triangles",
                LodTarget::Ratio(_) => "ratio",
            };
            return Err(fail(
                object,
                &at(key),
                format!("{target} triangles is not below the object's {triangles}, expected fewer"),
            ));
        }
        let reduced = simplify(base, target);
        let mut mesh = reduced.mesh;
        let mut morph = morph.cloned();
        if let Some(m) = morph.as_mut() {
            m.remap(&reduced.sources);
        }
        let mut tangent_list = None;
        let mut png = None;
        if level.bake_normals {
            let Some(spaced) = tangents(&mesh) else {
                return Err(fail(
                    object,
                    &at("bake_normals"),
                    "the level has no TEXCOORD_0 or no normals to bake a normal map on, expected uv other than \"none\" and smooth normals",
                ));
            };
            if let Some(m) = morph.as_mut() {
                m.remap(&spaced.sources);
            }
            let distance = level
                .bake_distance
                .unwrap_or_else(|| default_distance(base, reduced.error));
            let map = normal_map::bake(
                base,
                &spaced.mesh,
                &spaced.tangents,
                level.normal_size,
                distance as f32,
            )
            .map_err(|text| fail(object, &at("bake_normals"), text))?;
            mesh = spaced.mesh;
            tangent_list = Some(spaced.tangents);
            png = Some(map);
        }
        let mut extras = json!({
            "of": master.name,
            "level": k + 1,
            "triangles": mesh.triangle_count(),
            "target": target,
            "error": f64::from(reduced.error),
        });
        if png.is_some() {
            extras["normal_size"] = json!(level.normal_size);
        }
        let targets = morph.map(|morph| {
            let unposed = mesh.clone();
            morph.pose(&mut mesh);
            Box::new(Targets {
                base: unposed,
                morph,
            })
        });
        out.push(Lod {
            of: 0,
            node: Node {
                name: format!("{}.lod{}", master.name, k + 1),
                mesh,
                transform: master.transform,
                keep_transform: master.keep_transform,
                targets,
            },
            tangents: tangent_list,
            normal_map: png,
            extras: json!({ "lod": extras }),
        });
    }
    Ok(out)
}

fn default_distance(mesh: &Mesh, error: f32) -> f64 {
    let diagonal = manfred_mesh::bounds(mesh).map_or(0.0, |[lo, hi]| {
        (glam::DVec3::from(hi) - glam::DVec3::from(lo)).length()
    });
    (diagonal * DISTANCE_SHARE).max(f64::from(error) * DISTANCE_ERRORS)
}
