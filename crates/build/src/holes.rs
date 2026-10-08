use crate::Error;
use crate::shape::{fail, shapes};
use manfred_mesh::outline::{self, Outline as Ring, Shape};
use manfred_recipe::{Object, Outline, Profile, ProfileKind};

struct Leaf {
    key: String,
    rings: Vec<Ring>,
}

fn contours(found: &[Shape]) -> Vec<Ring> {
    found
        .iter()
        .flat_map(|s| std::iter::once(s.outer.clone()).chain(s.holes.iter().cloned()))
        .collect()
}

fn leaves(o: &Outline, key: String, out: &mut Vec<Leaf>) {
    match o {
        Outline::Union(parts) => {
            for (i, part) in parts.iter().enumerate() {
                leaves(part, format!("{key}.union[{i}]"), out);
            }
        }
        Outline::Difference(a, b) => {
            leaves(a, format!("{key}.difference[0]"), out);
            leaves(b, format!("{key}.difference[1]"), out);
        }
        _ => out.push(Leaf {
            key,
            rings: contours(&shapes(o)),
        }),
    }
}

fn tolerance(rings: &[Ring]) -> f64 {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in rings.iter().flat_map(|r| r.0.iter()) {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    if size.is_finite() && size > 0.0 {
        size * 1e-8
    } else {
        0.0
    }
}

fn touches(a: &[Ring], b: &[Ring], tol: f64) -> bool {
    a.iter()
        .any(|r| b.iter().any(|q| outline::clearance(r, q) <= tol))
}

fn on_ring(p: [f64; 2], r: &Ring, tol: f64) -> bool {
    let n = r.0.len();
    (0..n).any(|i| {
        let (a, b) = (r.0[i], r.0[(i + 1) % n]);
        let e = [b[0] - a[0], b[1] - a[1]];
        let len = e[0] * e[0] + e[1] * e[1];
        let t = if len > 0.0 {
            (((p[0] - a[0]) * e[0] + (p[1] - a[1]) * e[1]) / len).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (dx, dy) = (a[0] + e[0] * t - p[0], a[1] + e[1] * t - p[1]);
        (dx * dx + dy * dy).sqrt() <= tol
    })
}

fn name(ring: &Ring, leaves: &[Leaf], tol: f64) -> String {
    let near = |leaf: &Leaf| {
        ring.0
            .iter()
            .filter(|&&p| leaf.rings.iter().any(|r| on_ring(p, r, tol)))
            .count()
    };
    let mut best: Option<(usize, &Leaf)> = None;
    for leaf in leaves {
        let count = near(leaf);
        if count > 0 && best.is_none_or(|(b, _)| count > b) {
            best = Some((count, leaf));
        }
    }
    best.map_or_else(|| "outline".to_string(), |(_, leaf)| leaf.key.clone())
}

fn at_point(p: [f64; 2], leaves: &[Leaf], tol: f64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for leaf in leaves {
        if leaf.rings.iter().any(|r| on_ring(p, r, tol)) && !out.contains(&leaf.key) {
            out.push(leaf.key.clone());
        }
    }
    out
}

fn metres(v: f64) -> String {
    format!("{}", (v * 1e6).round() / 1e6 + 0.0)
}

fn explicit(object: &Object, o: &Outline, holes: &[Outline], tol: f64) -> Result<Vec<Ring>, Error> {
    let base = shapes(o);
    let outers: Vec<Ring> = base.iter().map(|s| s.outer.clone()).collect();
    let edge = contours(&base);
    let mut all = edge.clone();
    let mut placed: Vec<(String, Vec<Ring>)> = Vec::new();
    for (i, hole) in holes.iter().enumerate() {
        let key = format!("holes[{i}]");
        let rings = contours(&shapes(hole));
        let Some(first) = rings.first().and_then(|r| r.0.first().copied()) else {
            continue;
        };
        if touches(&rings, &edge, tol) {
            return Err(fail(
                object,
                &key,
                "the hole touches or crosses the edge of `outline`; a hole lies inside the outline, clear of its edge",
            ));
        }
        if !outers.iter().any(|r| r.contains(first)) {
            return Err(fail(
                object,
                &key,
                "the hole lies outside `outline`; a hole lies inside the outline, clear of its edge",
            ));
        }
        for (other, theirs) in &placed {
            if touches(&rings, theirs, tol) {
                return Err(fail(
                    object,
                    &key,
                    format!(
                        "the hole touches or crosses `{other}`; holes stay clear of each other, and one inside another makes an island"
                    ),
                ));
            }
        }
        all.extend(rings.iter().cloned());
        placed.push((key, rings));
    }
    Ok(all)
}

pub(crate) fn solid(
    object: &Object,
    o: &Outline,
    holes: &[Outline],
    profile: &Profile,
) -> Result<Vec<Shape>, Error> {
    let mut named = Vec::new();
    leaves(o, "outline".to_string(), &mut named);
    for (i, hole) in holes.iter().enumerate() {
        leaves(hole, format!("holes[{i}]"), &mut named);
    }
    let every: Vec<Ring> = named.iter().flat_map(|l| l.rings.iter().cloned()).collect();
    let tol = tolerance(&every);
    let found = if holes.is_empty() {
        shapes(o)
    } else {
        outline::even_odd(&explicit(object, o, holes, tol)?)
    };
    for s in &found {
        let rings: Vec<&Ring> = std::iter::once(&s.outer).chain(&s.holes).collect();
        for ring in &rings {
            if let Some(p) = outline::pinch(ring, tol) {
                let keys = at_point(p, &named, tol);
                let (key, text) = match keys.as_slice() {
                    [a, b, ..] => (
                        b.clone(),
                        format!(
                            "`{b}` touches `{a}` at [{}, {}]; a hole lies clear of the edge and of other holes",
                            metres(p[0]),
                            metres(p[1])
                        ),
                    ),
                    _ => (
                        keys.first()
                            .cloned()
                            .unwrap_or_else(|| "outline".to_string()),
                        format!(
                            "the edge touches itself at [{}, {}]; a hole lies clear of the edge and of other holes",
                            metres(p[0]),
                            metres(p[1])
                        ),
                    ),
                };
                return Err(fail(object, &key, text));
            }
        }
        for i in 0..rings.len() {
            for j in i + 1..rings.len() {
                if outline::clearance(rings[i], rings[j]) <= tol {
                    let a = name(rings[i], &named, tol);
                    let b = name(rings[j], &named, tol);
                    return Err(fail(
                        object,
                        &b,
                        format!(
                            "`{b}` touches `{a}`; a hole lies clear of the edge and of other holes"
                        ),
                    ));
                }
            }
        }
    }
    let width = match profile.kind {
        ProfileKind::None => 0.0,
        _ => profile.width,
    };
    if width > 0.0 {
        let mut narrowest: Option<(f64, &Ring, &Ring)> = None;
        for s in found.iter().filter(|s| !s.holes.is_empty()) {
            let rings: Vec<&Ring> = std::iter::once(&s.outer).chain(&s.holes).collect();
            for i in 0..rings.len() {
                for j in i + 1..rings.len() {
                    let wall = outline::clearance(rings[i], rings[j]);
                    if narrowest.is_none_or(|(w, _, _)| wall < w) {
                        narrowest = Some((wall, rings[i], rings[j]));
                    }
                }
            }
        }
        if let Some((wall, a, b)) = narrowest
            && 2.0 * width > wall
        {
            let a = name(a, &named, tol);
            let b = name(b, &named, tol);
            return Err(fail(
                object,
                "profile.width",
                format!(
                    "the profile's width {} is wider than half the {} wall between `{a}` and `{b}`, which takes a width up to {}",
                    metres(width),
                    metres(wall),
                    metres(0.5 * wall)
                ),
            ));
        }
    }
    Ok(found)
}
