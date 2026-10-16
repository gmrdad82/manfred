use super::imprint::{self, Facing, Frame, View, Work};
use crate::Piece;
use glam::DVec2;
use manfred_mesh::Mesh;
use serde_json::Value;

pub const LIP: f64 = 0.02;

const FIELDS: [&str; 6] = ["name", "view", "curve", "material", "mode", "lip"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Surface,
    Cut,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub name: String,
    pub view: View,
    pub curve: Vec<DVec2>,
    pub material: Option<String>,
    pub mode: Mode,
    pub lip: f64,
}

pub fn parse(value: Option<&Value>, at: &str) -> Result<Vec<Region>, String> {
    let regions = imprint::list(value, at)?
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            let at = format!("{at}.{i}");
            imprint::known(t, &at, &FIELDS)?;
            let mode = match t.get("mode").map(Value::as_str) {
                None | Some(Some("surface")) => Mode::Surface,
                Some(Some("cut")) => Mode::Cut,
                _ => return Err(format!("{at}.mode: expected one of: surface, cut")),
            };
            Ok(Region {
                name: imprint::name(t, &at)?,
                view: imprint::view(t, &at)?,
                curve: imprint::polygon(&imprint::curve(t, &at, 3)?),
                material: imprint::text(t, &at, "material")?,
                mode,
                lip: imprint::number(t, &at, "lip", Some(LIP), 0.0)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    imprint::unique(regions.iter().map(|r| r.name.as_str()), at)?;
    Ok(regions)
}

pub fn apply(mesh: &mut Mesh, regions: &[Region], frame: Frame) -> Vec<Piece> {
    if regions.is_empty() {
        return Vec::new();
    }
    let mut w = Work::from_mesh(mesh);
    let mut pieces = Vec::new();
    for r in regions {
        let facing = Facing::all(r.view, frame);
        imprint::imprint(&mut w, facing, &[(&r.curve, true)]);
        let picked = w.select(facing, &r.curve);
        match r.mode {
            Mode::Surface => {
                let mut part = w.to_mesh(&picked);
                if let Some(m) = &r.material {
                    part.set_material(m);
                }
                pieces.push(Piece {
                    name: r.name.clone(),
                    mesh: part,
                });
                let keep: Vec<bool> = picked.iter().map(|p| !p).collect();
                w.retain(&keep);
            }
            Mode::Cut => w.cut(&picked, r.lip),
        }
    }
    *mesh = w.to_mesh(&vec![true; w.tris.len()]);
    pieces
}

#[cfg(test)]
mod tests {
    use super::super::imprint::test::{area, body, open_edges, point};
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn regions(v: Value) -> Vec<Region> {
        parse(Some(&v), "regions").unwrap()
    }

    fn shoelace(p: &[[f64; 2]]) -> f64 {
        let n = p.len();
        0.5 * (0..n)
            .map(|i| p[i][0] * p[(i + 1) % n][1] - p[(i + 1) % n][0] * p[i][1])
            .sum::<f64>()
            .abs()
    }

    #[test]
    fn a_surface_region_splits_off_exactly_with_its_own_material() {
        let original = body();
        let outline = [[-1.0, -0.2], [0.7, -0.2], [1.0, 0.3], [-0.6, 0.35]];
        let rs = regions(
            json!([{ "name": "window", "view": "side", "material": "glass",
            "curve": outline }]),
        );
        let mut mesh = original.clone();
        let pieces = apply(&mut mesh, &rs, Frame::WORLD);
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].name, "window");
        let window = &pieces[0].mesh;
        assert!(!window.slots.is_empty());
        assert!(window.slots.iter().all(|s| s.material == "glass"));
        assert!(mesh.slots.iter().all(|s| s.material != "glass"));
        assert!((area(window) - 2.0 * shoelace(&outline)).abs() < 1e-5);
        assert!((area(&mesh) + area(window) - area(&original)).abs() < 1e-5);
        for i in 0..window.positions.len() as u32 {
            assert!((point(window, i).x.abs() - 1.0).abs() < 1e-6);
        }
        assert!(open_edges(&[&mesh, window]).is_empty());
        let mut again = original.clone();
        assert_eq!(apply(&mut again, &rs, Frame::WORLD)[0].mesh, *window);
        assert_eq!(again, mesh);
        let mut turned = original.clone();
        manfred_mesh::transform(
            &mut turned,
            glam::DMat4::from_rotation_y(std::f64::consts::FRAC_PI_2),
        );
        let frame = Frame::along(glam::DVec3::X).unwrap();
        let along = apply(&mut turned, &rs, frame);
        assert!((area(&along[0].mesh) - area(window)).abs() < 1e-5);
    }

    #[test]
    fn a_cut_region_leaves_a_closed_lip_turned_in() {
        let original = body();
        assert!(open_edges(&[&original]).is_empty());
        let rs = regions(
            json!([{ "name": "vent", "view": "side", "mode": "cut", "lip": 0.05,
            "curve": [[0.0, -0.3], [0.5, -0.1], [0.4, 0.3], [-0.2, 0.25], [-0.4, 0.0]] }]),
        );
        let mut mesh = original.clone();
        assert!(apply(&mut mesh, &rs, Frame::WORLD).is_empty());
        let open = open_edges(&[&mesh]);
        assert!(!open.is_empty());
        let mut out: BTreeMap<_, i32> = BTreeMap::new();
        let mut inn: BTreeMap<_, i32> = BTreeMap::new();
        for (a, b) in &open {
            *out.entry(*a).or_default() += 1;
            *inn.entry(*b).or_default() += 1;
        }
        assert_eq!(out.len(), inn.len());
        assert!(out.iter().all(|(k, c)| *c == 1 && inn.get(k) == Some(&1)));
        for k in out.keys() {
            let x = f64::from_bits(k[0]).abs();
            assert!(x > 0.95 - 1e-6 && x < 0.952, "{x}");
        }
        assert!(area(&mesh) < area(&original));
    }
}
