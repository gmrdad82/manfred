use super::{shown, string};
use crate::{Built, Error};
use glam::DVec3;
use manfred_recipe::{Proxy, Recipe};
use pfx_scene::Id;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

pub(crate) const THINNEST: f64 = 0.001;
const FLAT: f64 = 1e-6;

pub(crate) struct Stand {
    pub object: String,
    pub shape: Shape,
}

pub(crate) enum Shape {
    Box {
        at: [f32; 3],
        size: [f32; 3],
    },
    Hull {
        points: Vec<[f32; 3]>,
        triangles: Vec<[u32; 3]>,
    },
}

pub(crate) fn of(built: &Built, recipe: &Recipe) -> Result<Vec<Stand>, Error> {
    let mut stands = Vec::new();
    for part in built.parts.iter().filter(|part| shown(part)) {
        if recipe.dynamic(part.object) {
            continue;
        }
        let object = &recipe.objects[part.object];
        let node = &part.node;
        let points: Vec<DVec3> = node
            .mesh
            .positions
            .iter()
            .map(|p| {
                let p = DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
                if node.keep_transform {
                    node.transform.transform_point3(p)
                } else {
                    p
                }
            })
            .collect();
        let shape = match object.proxy.unwrap_or(Proxy::Box) {
            Proxy::Box => boxed(&points),
            Proxy::Hull => hull(&points).ok_or_else(|| {
                Error::new(
                    format!("object[{}].proxy", object.name),
                    format!(
                        "`{}` is flat, so its convex hull holds no volume, expected proxy = \"box\"",
                        node.name
                    ),
                )
            })?,
        };
        stands.push(Stand {
            object: node.name.clone(),
            shape,
        });
    }
    Ok(stands)
}

fn boxed(points: &[DVec3]) -> Shape {
    let low = points.iter().fold(DVec3::INFINITY, |m, p| m.min(*p));
    let high = points.iter().fold(DVec3::NEG_INFINITY, |m, p| m.max(*p));
    let at = (low + high) * 0.5;
    let size = (high - low).max(DVec3::splat(THINNEST));
    Shape::Box {
        at: at.as_vec3().to_array(),
        size: size.as_vec3().to_array(),
    }
}

struct Face {
    corners: [usize; 3],
    normal: DVec3,
    offset: f64,
}

impl Face {
    fn new(points: &[DVec3], corners: [usize; 3]) -> Face {
        let [a, b, c] = corners.map(|k| points[k]);
        let normal = (b - a).cross(c - a).normalize_or_zero();
        Face {
            corners,
            normal,
            offset: normal.dot(a),
        }
    }

    fn above(&self, p: DVec3) -> f64 {
        self.normal.dot(p) - self.offset
    }
}

pub(crate) fn hull(all: &[DVec3]) -> Option<Shape> {
    let mut seen = BTreeSet::new();
    let points: Vec<DVec3> = all
        .iter()
        .filter(|p| seen.insert(p.to_array().map(f64::to_bits)))
        .copied()
        .collect();
    let low = points.iter().fold(DVec3::INFINITY, |m, p| m.min(*p));
    let high = points.iter().fold(DVec3::NEG_INFINITY, |m, p| m.max(*p));
    let reach = (high - low).length();
    if points.len() < 4 || !reach.is_finite() || reach <= 0.0 {
        return None;
    }
    let eps = reach * FLAT;
    let start = simplex(&points, eps)?;
    let inside = start.iter().map(|&k| points[k]).sum::<DVec3>() / 4.0;
    let [a, b, c, d] = start;
    let mut faces: Vec<Face> = [[a, b, c], [a, d, b], [a, c, d], [b, d, c]]
        .into_iter()
        .map(|corners| {
            let face = Face::new(&points, corners);
            if face.above(inside) > 0.0 {
                Face::new(&points, [corners[0], corners[2], corners[1]])
            } else {
                face
            }
        })
        .collect();
    for (k, &p) in points.iter().enumerate() {
        if start.contains(&k) {
            continue;
        }
        let (lit, kept): (Vec<Face>, Vec<Face>) =
            faces.into_iter().partition(|face| face.above(p) > eps);
        faces = kept;
        if lit.is_empty() {
            continue;
        }
        let edges: BTreeSet<(usize, usize)> = lit
            .iter()
            .flat_map(|face| {
                let [a, b, c] = face.corners;
                [(a, b), (b, c), (c, a)]
            })
            .collect();
        for &(a, b) in &edges {
            if !edges.contains(&(b, a)) {
                faces.push(Face::new(&points, [a, b, k]));
            }
        }
    }
    let mut used: BTreeMap<usize, u32> = faces
        .iter()
        .flat_map(|face| face.corners)
        .map(|k| (k, 0))
        .collect();
    for (place, slot) in used.values_mut().enumerate() {
        *slot = place as u32;
    }
    let triangles: Vec<[u32; 3]> = faces
        .iter()
        .map(|face| face.corners.map(|k| used[&k]))
        .collect();
    let points: Vec<[f32; 3]> = used
        .keys()
        .map(|&k| points[k].as_vec3().to_array())
        .collect();
    (points.len() >= 4 && triangles.len() >= 4).then_some(Shape::Hull { points, triangles })
}

fn simplex(points: &[DVec3], eps: f64) -> Option<[usize; 4]> {
    let mut extremes = Vec::new();
    for axis in 0..3 {
        let by = |k: &usize| points[*k][axis];
        let order = |x: &usize, y: &usize| by(x).total_cmp(&by(y)).then(x.cmp(y));
        extremes.extend((0..points.len()).min_by(order));
        extremes.extend((0..points.len()).max_by(|x, y| order(x, y).then(y.cmp(x))));
    }
    let mut pair = (extremes[0], extremes[0], 0.0);
    for &i in &extremes {
        for &j in &extremes {
            let span = points[i].distance(points[j]);
            if span > pair.2 {
                pair = (i, j, span);
            }
        }
    }
    let (a, b, span) = pair;
    if span <= eps {
        return None;
    }
    let along = (points[b] - points[a]) / span;
    let c = farthest(points, |p| {
        let off = p - points[a];
        (off - along * off.dot(along)).length()
    })?;
    let normal = (points[b] - points[a])
        .cross(points[c] - points[a])
        .normalize_or_zero();
    if normal == DVec3::ZERO {
        return None;
    }
    let d = farthest(points, |p| normal.dot(p - points[a]).abs())?;
    let gaps = [
        (points[c] - points[a]).reject_from(along).length(),
        normal.dot(points[d] - points[a]).abs(),
    ];
    gaps.iter().all(|gap| *gap > eps).then_some([a, b, c, d])
}

fn farthest(points: &[DVec3], measure: impl Fn(DVec3) -> f64) -> Option<usize> {
    (0..points.len()).max_by(|x, y| {
        measure(points[*x])
            .total_cmp(&measure(points[*y]))
            .then(y.cmp(x))
    })
}

pub(crate) fn text(
    stands: &[Stand],
    ids: &[Id],
    head: &str,
    authoring: Option<&toml::Table>,
) -> Result<String, Error> {
    let mut out = String::from(head);
    for (k, stand) in stands.iter().enumerate() {
        let at = format!("object[{}].proxy", stand.object);
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("[[proxy]]\n");
        if let Some(id) = ids.get(k) {
            let _ = writeln!(out, "id = {}", string(&id.to_string()));
        }
        let _ = writeln!(out, "object = {}", string(&stand.object));
        match &stand.shape {
            Shape::Box { at: centre, size } => {
                let _ = writeln!(out, "kind = {}", string("box"));
                let _ = writeln!(out, "at = {}", triple(centre, &at)?);
                let _ = writeln!(out, "size = {}", triple(size, &at)?);
            }
            Shape::Hull { points, triangles } => {
                let _ = writeln!(out, "kind = {}", string("hull"));
                let points: Result<Vec<String>, Error> =
                    points.iter().map(|p| triple(p, &at)).collect();
                let _ = writeln!(out, "points = [{}]", points?.join(", "));
                let triangles: Vec<String> = triangles
                    .iter()
                    .map(|[a, b, c]| format!("[{a}, {b}, {c}]"))
                    .collect();
                let _ = writeln!(out, "triangles = [{}]", triangles.join(", "));
            }
        }
    }
    if let Some(authoring) = authoring {
        out.push_str("\n[authoring]\n");
        for (key, value) in authoring {
            if let Some(value) = value.as_str() {
                let _ = writeln!(out, "{key} = {}", string(value));
            }
        }
    }
    Ok(out)
}

fn triple(values: &[f32; 3], key: &str) -> Result<String, Error> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(format!(
            "[{:?}, {:?}, {:?}]",
            values[0], values[1], values[2]
        ))
    } else {
        Err(Error::new(key, "expected finite world positions"))
    }
}
