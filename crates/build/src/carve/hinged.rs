use super::grooves::Groove;
use super::imprint::{self, Facing, Frame, View, Work};
use crate::Piece;
use glam::DVec3;
use manfred_mesh::Mesh;
use serde_json::Value;

const FIELDS: [&str; 3] = ["name", "gap", "hinge"];

#[derive(Clone, Debug, PartialEq)]
pub struct Hinged {
    pub name: String,
    pub gap: String,
    pub hinge: [DVec3; 2],
}

impl Hinged {
    pub fn pivot(&self) -> DVec3 {
        (self.hinge[0] + self.hinge[1]) * 0.5
    }

    pub fn axis(&self) -> DVec3 {
        (self.hinge[1] - self.hinge[0]).normalize()
    }
}

fn point(v: &Value) -> Option<DVec3> {
    let p = v.as_array().filter(|p| p.len() == 3)?;
    let c = p
        .iter()
        .map(|x| x.as_f64().filter(|x| x.is_finite()))
        .collect::<Option<Vec<f64>>>()?;
    Some(DVec3::new(c[0], c[1], c[2]))
}

pub fn parse(value: Option<&Value>, at: &str, grooves: &[Groove]) -> Result<Vec<Hinged>, String> {
    let panels = imprint::list(value, at)?
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            let at = format!("{at}.{i}");
            imprint::known(t, &at, &FIELDS)?;
            let bad = || format!("{at}.hinge: expected two distinct points [[x, y, z], [x, y, z]]");
            let ends = t
                .get("hinge")
                .and_then(Value::as_array)
                .filter(|h| h.len() == 2)
                .ok_or_else(bad)?;
            let (a, b) = (
                point(&ends[0]).ok_or_else(bad)?,
                point(&ends[1]).ok_or_else(bad)?,
            );
            if a.distance(b) < 1e-6 {
                return Err(bad());
            }
            let gap = imprint::text(t, &at, "gap")?.ok_or_else(|| format!("{at}.gap: required"))?;
            match grooves.iter().find(|g| g.name == gap) {
                None => {
                    let names: Vec<&str> = grooves.iter().map(|g| g.name.as_str()).collect();
                    return Err(format!(
                        "{at}.gap: no groove `{gap}`, expected one of: {}",
                        names.join(", ")
                    ));
                }
                Some(g) if imprint::polygon(&g.curve).len() < 3 => {
                    return Err(format!(
                        "{at}.gap: groove `{gap}` has fewer than 3 points to enclose a panel"
                    ));
                }
                Some(_) => {}
            }
            Ok(Hinged {
                name: imprint::name(t, &at)?,
                gap,
                hinge: [a, b],
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    imprint::unique(panels.iter().map(|d| d.name.as_str()), at)?;
    Ok(panels)
}

pub fn split(
    mesh: &mut Mesh,
    panels: &[Hinged],
    grooves: &[Groove],
    frame: Frame,
) -> Result<Vec<(Piece, DVec3)>, String> {
    if panels.is_empty() {
        return Ok(Vec::new());
    }
    let centre = manfred_mesh::bounds(mesh).map_or(DVec3::ZERO, |[lo, hi]| {
        (DVec3::from(lo) + DVec3::from(hi)) * 0.5
    });
    let mut w = Work::from_mesh(mesh);
    let mut out = Vec::new();
    for d in panels {
        let g = grooves
            .iter()
            .find(|g| g.name == d.gap)
            .ok_or_else(|| format!("hinged panel `{}`: no groove `{}`", d.name, d.gap))?;
        let outline = imprint::polygon(&g.curve);
        let pivot = d.pivot();
        let mut facing = Facing::all(g.view, frame);
        if g.view == View::Side {
            let side = facing.depth(pivot) - facing.depth(centre);
            facing.flank = if side.abs() < 1e-9 {
                0.0
            } else {
                side.signum()
            };
        }
        imprint::imprint(&mut w, facing, &[(&outline, true)]);
        let picked = w.select(facing, &outline);
        if !picked.contains(&true) {
            return Err(format!(
                "hinged panel `{}`: groove `{}` encloses none of the mesh",
                d.name, d.gap
            ));
        }
        out.push((
            Piece {
                name: d.name.clone(),
                mesh: w.to_mesh(&picked),
            },
            pivot,
        ));
        let keep: Vec<bool> = picked.iter().map(|p| !p).collect();
        w.retain(&keep);
    }
    *mesh = w.to_mesh(&vec![true; w.tris.len()]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::grooves;
    use super::super::imprint::test::{body, open_edges, point};
    use super::*;
    use serde_json::json;

    #[test]
    fn a_hinged_panel_splits_along_its_gap_with_its_pivot_on_the_hinge() {
        let gs = grooves::parse(
            Some(
                &json!([{ "name": "shut", "view": "side", "width": 0.01, "depth": 0.005,
                "curve": [[-0.6, -0.4], [0.8, -0.4], [0.8, 0.4], [-0.6, 0.4], [-0.6, -0.4]] }]),
            ),
            "grooves",
        )
        .unwrap();
        let ds = parse(
            Some(&json!([{ "name": "door", "gap": "shut",
                "hinge": [[1.0, -0.4, 0.8], [1.0, 0.4, 0.8]] }])),
            "hinged",
            &gs,
        )
        .unwrap();
        let mut mesh = body();
        grooves::apply(&mut mesh, &gs, Frame::WORLD);
        let before = mesh.triangle_count();
        let panels = split(&mut mesh, &ds, &gs, Frame::WORLD).unwrap();
        assert_eq!(panels.len(), 1);
        let (panel, pivot) = &panels[0];
        assert_eq!(panel.name, "door");
        let [a, b] = ds[0].hinge;
        assert!((*pivot - a).cross(b - a).length() / (b - a).length() < 1e-9);
        assert!(panel.mesh.triangle_count() > 0);
        assert!(mesh.triangle_count() + panel.mesh.triangle_count() >= before);
        for i in 0..panel.mesh.positions.len() as u32 {
            let p = point(&panel.mesh, i);
            assert!(p.x > 0.9 && p.z > -0.6 - 1e-6 && p.z < 0.8 + 1e-6);
            assert!(p.y > -0.4 - 1e-6 && p.y < 0.4 + 1e-6);
        }
        assert!(open_edges(&[&mesh, &panel.mesh]).is_empty());
        assert!(!open_edges(&[&panel.mesh]).is_empty());
        let other = mesh
            .indices
            .chunks_exact(3)
            .map(|t| (point(&mesh, t[0]) + point(&mesh, t[1]) + point(&mesh, t[2])) / 3.0)
            .filter(|c| c.x < -0.9 && c.z > -0.6 && c.z < 0.8 && c.y.abs() < 0.4)
            .count();
        assert!(other > 0);
        let wrong =
            json!([{ "name": "door", "gap": "none", "hinge": [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0]] }]);
        assert!(parse(Some(&wrong), "hinged", &gs).is_err());
    }
}
