use super::shared::{Params, Tris, curve_param, flag, known, material, pairs, piece, whole};
use crate::{Call, Generated};
use glam::{DMat4, DVec2, DVec3};
use manfred_mesh::outline::{self, Outline, Shape};
use manfred_mesh::{self as mesh, Mesh};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

pub const NAME: &str = "manfred/sweep-profile";

pub(super) const PARAMS: [&str; 6] = ["profile", "path", "closed", "repeat", "step", "material"];

const TRIM: &str = "sweep_trim";

pub(super) fn trim() -> Value {
    json!({ "base": [0.018, 0.018, 0.02], "roughness": 0.55 })
}

pub fn generate(call: &Call) -> Result<Generated, String> {
    known(call.params, &PARAMS, "sweep")?;
    let mut made = sweep(&call.object.name, call.params, "sweep", (TRIM, trim()))?;
    let mut all = Mesh::default();
    for piece in &made.pieces {
        all.append(&piece.mesh);
    }
    if let Some(bounds) = mesh::bounds(&all) {
        made.facts.insert("bounds".into(), json!(bounds));
    }
    made.facts = Map::from_iter([(
        "sweep".to_string(),
        Value::Object(std::mem::take(&mut made.facts)),
    )]);
    Ok(made)
}

pub(super) fn sweep(
    name: &str,
    p: &Params,
    part: &str,
    (trim, look): (&str, Value),
) -> Result<Generated, String> {
    let profile = curve_param(p, "profile")?
        .ok_or_else(|| "params.profile: missing, expected a curve [[a, b], ...]".to_string())?;
    let path: Vec<DVec3> = match p.get("path") {
        None => return Err("params.path: missing, expected a polyline [[x, y, z], ...]".into()),
        Some(v) => pairs::<3>(v, "params.path", 2)?
            .into_iter()
            .map(DVec3::from)
            .collect(),
    };
    let closed = flag(p, "closed", false)?;
    let repeat = whole(p, "repeat", 1, 1, 64)?;
    let step = match p.get("step") {
        None => DVec3::new(0.1, 0.0, 0.0),
        Some(v) => DVec3::from(
            pairs::<3>(&json!([v]), "params.step", 1)
                .map_err(|_| "params.step: expected [x, y, z]".to_string())?[0],
        ),
    };
    let paint = material(p, trim)?;
    let one = swept(&profile, &path, closed)
        .ok_or_else(|| "params.path: expected at least 2 distinct points".to_string())?;
    let length = path.windows(2).map(|w| w[0].distance(w[1])).sum::<f64>();
    let mut all = Mesh::default();
    for k in 0..repeat {
        let mut copy = one.clone();
        let shift = step * (k as f64 - (repeat - 1) as f64 * 0.5);
        mesh::transform(&mut copy, DMat4::from_translation(shift));
        all.append(&copy);
    }
    all.set_material(&paint);
    let mut materials = BTreeMap::new();
    if paint == trim {
        materials.insert(trim.to_string(), look);
    }
    let mut facts = Map::new();
    facts.insert("length".into(), json!(length));
    facts.insert("repeat".into(), json!(repeat));
    Ok(Generated {
        pieces: vec![piece(name, part, all)],
        facts,
        materials,
        lights: Vec::new(),
        ..Generated::default()
    })
}

fn swept(profile: &Outline, path: &[DVec3], closed: bool) -> Option<Mesh> {
    let mut pts: Vec<DVec3> = Vec::new();
    for &q in path {
        if pts.last().is_none_or(|l| l.distance(q) > 1e-9) {
            pts.push(q);
        }
    }
    let closed = closed && pts.len() > 2;
    if closed
        && pts
            .first()
            .zip(pts.last())
            .is_some_and(|(a, b)| a.distance(*b) < 1e-9)
    {
        pts.pop();
    }
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let ring: Vec<DVec2> = profile.0.iter().map(|&q| DVec2::from(q)).collect();
    let m = ring.len();
    let dir = |i: usize| (pts[(i + 1) % n] - pts[i]).normalize();
    let mut b = Tris::default();
    let mut rows: Vec<Vec<u32>> = Vec::new();
    let mut walked = 0.0;
    let count = if closed { n + 1 } else { n };
    for k in 0..count {
        let i = k % n;
        if k > 0 {
            walked += pts[i].distance(pts[k - 1]);
        }
        let t_in = if i > 0 || closed {
            dir((i + n - 1) % n)
        } else {
            dir(0)
        };
        let t_out = if i + 1 < n || closed {
            dir(i)
        } else {
            dir(n - 2)
        };
        let avg = (t_in + t_out).normalize_or(t_out);
        let side = avg.cross(DVec3::Y);
        let right = if side.length() < 1e-6 {
            avg.cross(DVec3::Z)
        } else {
            side
        }
        .normalize();
        let up = right.cross(avg);
        let cos = t_in.dot(avg).clamp(0.25, 1.0);
        let bend = {
            let k = t_out - t_in;
            let k = k - avg * k.dot(avg);
            (k.length() > 1e-9).then(|| k.normalize())
        };
        let mut around = 0.0;
        let mut row = Vec::with_capacity(m + 1);
        for j in 0..=m {
            if j > 0 {
                around += ring[j - 1].distance(ring[j % m]);
            }
            let q = ring[j % m];
            let mut o = right * q.x + up * q.y;
            if let Some(k) = bend {
                o += k * o.dot(k) * (1.0 / cos - 1.0);
            }
            row.push(b.vert(pts[i] + o, DVec2::new(walked, around)));
        }
        rows.push(row);
    }
    for r in 0..rows.len() - 1 {
        for j in 0..m {
            b.quad(
                rows[r][j],
                rows[r + 1][j],
                rows[r + 1][j + 1],
                rows[r][j + 1],
            );
        }
    }
    if !closed {
        let tris = outline::triangulate(&Shape::new(profile.clone()));
        for (row, start) in [(&rows[0], true), (&rows[n - 1], false)] {
            let cap: Vec<u32> = (0..m)
                .map(|j| {
                    let v = b.pos[row[j] as usize];
                    b.vert(v, ring[j])
                })
                .collect();
            for t in &tris {
                let [p0, p1, p2] = t.map(|i| ring[i as usize]);
                let ccw = (p1 - p0).perp_dot(p2 - p0) > 0.0;
                let [x, y, z] = t.map(|i| cap[i as usize]);
                if ccw == start {
                    b.tri(x, y, z);
                } else {
                    b.tri(x, z, y);
                }
            }
        }
    }
    b.outward();
    Some(b.mesh("", 30.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(v: Value) -> Params {
        let Value::Object(m) = v else { unreachable!() };
        m.into_iter().collect()
    }

    #[test]
    fn a_profile_sweeps_open_with_caps_and_closed_without() {
        let rail = params(json!({
            "profile": [[-0.02, 0.0], [0.02, 0.0], [0.02, 0.05], [-0.02, 0.05]],
            "path": [[0.0, 1.0, 0.0], [2.0, 1.0, 0.0], [2.0, 1.0, 2.0]],
        }));
        let open = sweep("rail", &rail, "sweep", (TRIM, trim())).unwrap();
        let m = &open.pieces[0].mesh;
        assert_eq!(open.pieces[0].name, "rail.sweep");
        let [lo, hi] = mesh::bounds(m).unwrap();
        assert!(
            lo[0] > -0.05 && hi[0] < 2.05 && lo[1] > 0.99 && hi[1] < 1.06,
            "{lo:?} {hi:?}"
        );
        assert!(lo[2] > -0.05 && hi[2] < 2.01, "{lo:?} {hi:?}");
        assert_eq!(open.facts["length"], 4.0);
        let mut ring = rail.clone();
        ring.insert("closed".into(), json!(true));
        let closed = sweep("rail", &ring, "sweep", (TRIM, trim())).unwrap();
        assert!(closed.pieces[0].mesh.triangle_count() > m.triangle_count());
        assert_eq!(
            sweep("rail", &rail, "sweep", (TRIM, trim()))
                .unwrap()
                .pieces[0]
                .mesh,
            *m
        );
    }
}
