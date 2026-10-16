use super::imprint::{self, Facing, Frame, Key, View, Work, key};
use glam::{DVec2, DVec3};
use manfred_mesh::{Mesh, triangle_normal};
use serde_json::Value;
use std::collections::BTreeMap;

const FIELDS: [&str; 6] = ["name", "view", "curve", "width", "depth", "profile"];
const V_RINGS: [f64; 1] = [1.0];
const U_RINGS: [f64; 4] = [0.5, 0.8, 0.95, 1.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    V,
    U,
}

impl Profile {
    fn amount(self, r: f64) -> f64 {
        match self {
            Profile::V => 1.0 - r,
            Profile::U => (1.0 - r * r).max(0.0).sqrt(),
        }
    }

    fn rings(self) -> &'static [f64] {
        match self {
            Profile::V => &V_RINGS,
            Profile::U => &U_RINGS,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Groove {
    pub name: String,
    pub view: View,
    pub curve: Vec<DVec2>,
    pub width: f64,
    pub depth: f64,
    pub profile: Profile,
}

impl Groove {
    pub fn closed(&self) -> bool {
        self.curve.len() > 2 && self.curve.first() == self.curve.last()
    }

    pub fn line(&self) -> Vec<DVec2> {
        if self.closed() {
            imprint::polygon(&self.curve)
        } else {
            self.curve.clone()
        }
    }
}

pub fn parse(value: Option<&Value>, at: &str) -> Result<Vec<Groove>, String> {
    let grooves = imprint::list(value, at)?
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            let at = format!("{at}.{i}");
            imprint::known(t, &at, &FIELDS)?;
            let profile = match t.get("profile").map(Value::as_str) {
                None | Some(Some("v")) => Profile::V,
                Some(Some("u")) => Profile::U,
                _ => return Err(format!("{at}.profile: expected one of: v, u")),
            };
            let width = imprint::number(t, &at, "width", None, 0.0)?;
            if width == 0.0 {
                return Err(format!("{at}.width: expected a number above 0"));
            }
            Ok(Groove {
                name: imprint::name(t, &at)?,
                view: imprint::view(t, &at)?,
                curve: imprint::curve(t, &at, 2)?,
                width,
                depth: imprint::number(t, &at, "depth", None, 0.0)?,
                profile,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    imprint::unique(grooves.iter().map(|g| g.name.as_str()), at)?;
    Ok(grooves)
}

pub fn apply(mesh: &mut Mesh, grooves: &[Groove], frame: Frame) {
    if grooves.is_empty() {
        return;
    }
    let mut w = Work::from_mesh(mesh);
    for g in grooves {
        groove(&mut w, g, frame);
    }
    *mesh = w.to_mesh(&vec![true; w.tris.len()]);
}

fn offset(line: &[DVec2], closed: bool, by: f64) -> Vec<DVec2> {
    let n = line.len();
    let normal = |a: usize, b: usize| (line[b] - line[a]).normalize_or_zero().perp();
    (0..n)
        .map(|i| {
            let prev = (i > 0 || closed).then(|| normal((i + n - 1) % n, i));
            let next = (i + 1 < n || closed).then(|| normal(i, (i + 1) % n));
            let m = match (prev, next) {
                (Some(a), Some(b)) => {
                    let s = (a + b).normalize_or_zero();
                    s / s.dot(a).max(0.5)
                }
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => DVec2::ZERO,
            };
            line[i] + m * by
        })
        .collect()
}

fn groove(w: &mut Work, g: &Groove, frame: Frame) {
    let facing = Facing::all(g.view, frame);
    let closed = g.closed();
    let line = g.line();
    let half = g.width * 0.5;
    let mut lines = vec![line.clone()];
    for &r in g.profile.rings() {
        lines.push(offset(&line, closed, half * r));
        lines.push(offset(&line, closed, -half * r));
    }
    let refs: Vec<(&[DVec2], bool)> = lines.iter().map(|l| (l.as_slice(), closed)).collect();
    imprint::imprint(w, facing, &refs);
    let mut sink: BTreeMap<Key, DVec3> = BTreeMap::new();
    for t in 0..w.tris.len() {
        let n = w.face(t);
        if !facing.faces(n) {
            continue;
        }
        for v in w.tris[t] {
            let p = w.pos[v as usize];
            sink.entry(key(p)).or_insert_with(|| {
                let d = imprint::distance(&line, closed, facing.project(p));
                if d < half {
                    facing.inward(n) * g.depth * g.profile.amount(d / half)
                } else {
                    DVec3::ZERO
                }
            });
        }
    }
    sink.retain(|_, a| *a != DVec3::ZERO);
    if sink.is_empty() {
        return;
    }
    let band: Vec<usize> = (0..w.tris.len())
        .filter(|&t| {
            w.tris[t]
                .iter()
                .any(|&v| sink.contains_key(&key(w.pos[v as usize])))
        })
        .collect();
    for p in w.pos.iter_mut() {
        if let Some(&a) = sink.get(&key(*p)) {
            *p += a;
        }
    }
    match g.profile {
        Profile::V => {
            for &t in &band {
                let n = w.face(t);
                let tri = w.tris[t];
                w.tris[t] = tri.map(|v| w.push_vertex(&[(v, 1.0)], w.pos[v as usize], Some(n)));
            }
        }
        Profile::U => {
            let mut smooth: BTreeMap<Key, DVec3> = BTreeMap::new();
            for &t in &band {
                let c = w.corners(t);
                let n = triangle_normal(&c) * (c[1] - c[0]).cross(c[2] - c[0]).length();
                for p in c {
                    *smooth.entry(key(p)).or_default() += n;
                }
            }
            let mut made: BTreeMap<u32, u32> = BTreeMap::new();
            for &t in &band {
                let tri = w.tris[t];
                w.tris[t] = tri.map(|v| match made.get(&v) {
                    Some(&m) => m,
                    None => {
                        let p = w.pos[v as usize];
                        let n = smooth[&key(p)].normalize_or_zero();
                        let m = w.push_vertex(&[(v, 1.0)], p, Some(n));
                        made.insert(v, m);
                        m
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::imprint::test::{body, open_edges, point};
    use super::*;
    use serde_json::json;

    #[test]
    fn a_groove_sinks_its_curve_by_its_depth_over_its_width() {
        for profile in ["v", "u"] {
            let gs = parse(
                Some(&json!([{ "name": "line", "view": "side", "width": 0.04,
                "depth": 0.006, "profile": profile,
                "curve": [[-1.2, 0.0], [0.0, 0.1], [1.2, -0.05]] }])),
                "grooves",
            )
            .unwrap();
            let mut mesh = body();
            apply(&mut mesh, &gs, Frame::WORLD);
            let line = gs[0].line();
            let mut bottom = 0;
            for i in 0..mesh.positions.len() as u32 {
                let p = point(&mesh, i);
                if p.x.abs() < 0.9 || p.y.abs() > 0.45 || p.z.abs() > 1.85 {
                    continue;
                }
                let d = imprint::distance(&line, false, Frame::WORLD.project(View::Side, p));
                let sunk = 1.0 - p.x.abs();
                if d < 1e-6 {
                    assert!((sunk - 0.006).abs() < 1e-6, "{profile}: {sunk}");
                    bottom += 1;
                } else if d >= 0.02 {
                    assert!(sunk.abs() < 1e-6);
                } else {
                    assert!(sunk > -1e-6 && sunk < 0.006 + 1e-6);
                }
            }
            assert!(bottom >= 8, "{profile}: {bottom}");
            assert!(open_edges(&[&mesh]).is_empty());
        }
    }
}
