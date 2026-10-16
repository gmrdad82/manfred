use super::shared::{
    Params, Tris, centre, choice, colour, curve, curve_param, half, known, linear, material,
    nested, piece, placed, ranged, scaled, slab, whole,
};
use super::sweep_profile;
use crate::{Call, Generated, GeneratedLight};
use glam::{DMat4, DQuat, DVec2, DVec3};
use manfred_mesh::math::Real;
use manfred_mesh::outline::{self, Outline, Shape};
use manfred_mesh::{self as mesh, Mesh, Profile, UvMode};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_2;

pub const NAME: &str = "manfred/vehicle-parts";

const KINDS: [&str; 5] = ["lamp", "mirror", "wiper", "exhaust", "extrusion"];
const LAMP: [&str; 10] = [
    "kind",
    "outline",
    "depth",
    "lens",
    "bars",
    "bar_depth",
    "role",
    "color",
    "strength",
    "light",
];
const LIGHT: [&str; 6] = ["kind", "intensity", "cone", "color", "range", "shadow"];
const MIRROR: [&str; 9] = [
    "kind", "side", "profile", "depth", "reach", "lift", "stalk", "rim", "material",
];
const WIPER: [&str; 5] = ["kind", "length", "angle", "arm", "width"];
const EXHAUST: [&str; 10] = [
    "kind", "count", "shape", "pattern", "diameter", "spacing", "split", "length", "aspect", "wall",
];
const PATTERNS: [&str; 4] = ["single", "twin", "quad", "split-quad"];
const EXTRUSION: [&str; 7] = [
    "kind",
    sweep_profile::PARAMS[0],
    sweep_profile::PARAMS[1],
    sweep_profile::PARAMS[2],
    sweep_profile::PARAMS[3],
    sweep_profile::PARAMS[4],
    sweep_profile::PARAMS[5],
];

const HOUSING: &str = "vehicle_housing";
const LENS: &str = "vehicle_lens";
const TRIM: &str = "vehicle_trim";
const MIRROR_GLASS: &str = "vehicle_mirror";
const ARM: &str = "vehicle_wiper";
const RUBBER: &str = "vehicle_rubber";
const TIP: &str = "vehicle_tip";
const BORE: &str = "vehicle_bore";

pub fn generate(call: &Call) -> Result<Generated, String> {
    part(&call.object.name, call.params)
}

fn part(name: &str, params: &Params) -> Result<Generated, String> {
    let kind = match params.get("kind") {
        None => {
            return Err(format!(
                "params.kind: missing, expected one of: {}",
                KINDS.join(", ")
            ));
        }
        Some(v) => v
            .as_str()
            .filter(|k| KINDS.contains(k))
            .ok_or_else(|| format!("params.kind: expected one of: {}", KINDS.join(", ")))?,
    };
    let mut made = match kind {
        "lamp" => lamp(name, params)?,
        "mirror" => mirror(name, params)?,
        "wiper" => wiper(name, params)?,
        "exhaust" => exhaust(name, params)?,
        _ => extrusion(name, params)?,
    };
    let mut all = Mesh::default();
    for piece in &made.pieces {
        all.append(&piece.mesh);
    }
    let mut facts = Map::new();
    facts.insert("kind".into(), json!(kind));
    if let Some(bounds) = mesh::bounds(&all) {
        facts.insert("bounds".into(), json!(bounds));
    }
    for (key, value) in std::mem::take(&mut made.facts) {
        facts.insert(key, value);
    }
    made.facts = Map::from_iter([("part".to_string(), Value::Object(facts))]);
    Ok(made)
}

fn lamp(name: &str, p: &Params) -> Result<Generated, String> {
    known(p, &LAMP, "lamp")?;
    let outline = curve_param(p, "outline")?
        .unwrap_or_else(|| outline::superellipse([0.0, 0.0], [0.16, 0.05], 4.0, 32));
    let depth = ranged(p, "depth", 0.08, 0.01, 1.0)?;
    let lens = ranged(p, "lens", 0.006, 0.001, 0.05)?;
    let bar_depth = ranged(p, "bar_depth", 0.004, 0.001, 0.05)?;
    let recess = lens + bar_depth + 0.002;
    if depth < recess + 0.004 {
        return Err(format!(
            "params.depth: {depth} is too shallow for the lens and bars, expected at least {}",
            recess + 0.004
        ));
    }
    let tail = choice(p, "role", "head", &["head", "tail"])? == "tail";
    let rgb = match p.get("color") {
        None if tail => [255, 26, 10],
        None => [255, 242, 216],
        Some(v) => colour(v, "params.color")?,
    };
    let strength = ranged(p, "strength", if tail { 3.2 } else { 5.0 }, 0.0, 100.0)?;
    let mid = centre(&outline);
    let bars = match p.get("bars") {
        None => vec![scaled(&outline, mid, 0.7)],
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(i, v)| curve(v, &format!("params.bars[{i}]")))
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err("params.bars: expected an array of curves [[x, y], ...]".into()),
    };
    let base = linear(rgb);
    let bar = format!(
        "vehicle_bar_{:02x}{:02x}{:02x}_{}",
        rgb[0],
        rgb[1],
        rgb[2],
        (strength * 100.0).round() as u64
    );
    let wall = (half(&outline).min_element() * 0.2).min(0.004);
    let mut housing = slab(&outline, -depth, -recess, HOUSING);
    if let Some(inner) = outline::offset(&Shape::new(outline.clone()), -wall)
        .into_iter()
        .next()
    {
        let mut ring = mesh::extrude_shape(
            &Shape {
                outer: outline.clone(),
                holes: vec![inner.outer],
            },
            lens,
            recess,
            &Profile::NONE,
            UvMode::Local,
        );
        ring.set_material(HOUSING);
        housing.append(&ring);
    }
    let mut lit = Mesh::default();
    for b in &bars {
        lit.append(&slab(b, -recess, -recess + bar_depth, &bar));
    }
    let mut pieces = vec![
        piece(name, "housing", housing),
        piece(name, "lens", slab(&outline, -lens, 0.0, LENS)),
    ];
    if !lit.indices.is_empty() {
        pieces.push(piece(name, "bars", lit));
    }
    let at = [mid.x, mid.y, 0.01];
    let light =
        lamp_light(p, tail, base)?.map(|(intensity, cone, color, range, shadow)| GeneratedLight {
            name: format!("{name}.light"),
            position: at,
            direction: [0.0, 0.0, 1.0],
            cone,
            range,
            color,
            intensity,
            shadow,
            parent: Some(format!("{name}.housing")),
        });
    let mut facts = Map::new();
    facts.insert("role".into(), json!(if tail { "tail" } else { "head" }));
    if light.is_some() {
        facts.insert(
            "light".into(),
            json!({ "at": at, "direction": [0.0, 0.0, 1.0] }),
        );
    }
    let materials = BTreeMap::from([
        (
            HOUSING.to_string(),
            json!({ "base": [0.012, 0.012, 0.014], "roughness": 0.45 }),
        ),
        (
            LENS.to_string(),
            json!({ "family": "glass", "base": [0.96, 0.96, 0.96], "roughness": 0.01, "transmission": 0.95, "ior": 1.49, "thickness": lens }),
        ),
        (
            bar,
            json!({ "family": "emissive", "base": base, "emission": base.map(|c| c * strength) }),
        ),
    ]);
    Ok(Generated {
        pieces,
        facts,
        materials,
        lights: light.into_iter().collect(),
        ..Generated::default()
    })
}

type LampLight = (f64, [f64; 2], [f64; 3], f64, bool);

fn lamp_light(p: &Params, tail: bool, base: [f64; 3]) -> Result<Option<LampLight>, String> {
    let l: Params = match p.get("light") {
        Some(Value::Bool(false)) => return Ok(None),
        None | Some(Value::Bool(true)) => Params::new(),
        Some(Value::Object(m)) => m.clone().into_iter().collect(),
        Some(_) => {
            return Err("params.light: expected true, false or a table { intensity, cone, color, range, shadow }".into());
        }
    };
    if let Some(key) = l.keys().find(|k| !LIGHT.contains(&k.as_str())) {
        return Err(format!(
            "params.light.{key}: unknown key, expected one of: {}",
            LIGHT.join(", ")
        ));
    }
    if let Some(kind) = l.get("kind")
        && kind.as_str() != Some("spot")
    {
        return Err("params.light.kind: expected \"spot\"".into());
    }
    let intensity = nested(
        &l,
        "intensity",
        "params.light.intensity",
        if tail { 6.0 } else { 90.0 },
        0.0,
        1.0e6,
    )?;
    let range = nested(
        &l,
        "range",
        "params.light.range",
        if tail { 6.0 } else { 40.0 },
        0.01,
        1.0e4,
    )?;
    let cone = match l.get("cone") {
        None if tail => [15.0, 35.0],
        None => [20.0, 40.0],
        Some(v) => v
            .as_array()
            .filter(|a| a.len() == 2)
            .and_then(|a| Some([a[0].as_f64()?, a[1].as_f64()?]))
            .filter(|c| c[0] >= 0.0 && c[0] <= c[1] && c[1] <= 90.0)
            .ok_or_else(|| {
                "params.light.cone: expected [inner, outer] degrees, 0 <= inner <= outer <= 90"
                    .to_string()
            })?,
    };
    let color = match l.get("color") {
        Some(v) => linear(colour(v, "params.light.color")?),
        None => {
            let top = base.iter().copied().fold(1e-6, f64::max);
            base.map(|c| c / top)
        }
    };
    let shadow = match l.get("shadow") {
        None => false,
        Some(v) => v
            .as_bool()
            .ok_or_else(|| "params.light.shadow: expected true or false".to_string())?,
    };
    Ok(Some((intensity, cone, color, range, shadow)))
}

fn resample(o: &Outline, n: usize) -> Vec<DVec2> {
    let p: Vec<DVec2> = o.0.iter().map(|&q| DVec2::from(q)).collect();
    let total = o.perimeter();
    let mut out = Vec::with_capacity(n);
    let (mut seg, mut walked) = (0, 0.0);
    for k in 0..n {
        let want = total * k as f64 / n as f64;
        loop {
            let len = p[seg].distance(p[(seg + 1) % p.len()]);
            if walked + len >= want || seg + 1 == p.len() {
                let f = if len > 0.0 {
                    ((want - walked) / len).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                out.push(p[seg].lerp(p[(seg + 1) % p.len()], f));
                break;
            }
            walked += len;
            seg += 1;
        }
    }
    out
}

fn mirror(name: &str, p: &Params) -> Result<Generated, String> {
    known(p, &MIRROR, "mirror")?;
    let left = choice(p, "side", "left", &["left", "right"])? == "left";
    let profile = curve_param(p, "profile")?
        .unwrap_or_else(|| outline::superellipse([0.0, 0.0], [0.11, 0.065], 3.0, 32));
    let depth = ranged(p, "depth", 0.09, 0.01, 0.5)?;
    let reach = ranged(p, "reach", 0.16, 0.02, 1.0)?;
    let lift = ranged(p, "lift", 0.04, -0.5, 0.5)?;
    let stalk = ranged(p, "stalk", 0.014, 0.003, 0.1)?;
    let size = half(&profile);
    let rim = ranged(p, "rim", 0.006, 0.0, size.min_element() * 0.45)?;
    let paint = material(p, TRIM)?;
    let at = DVec2::new(reach, lift) - centre(&profile);
    let profile = profile.map(|q| (DVec2::from(q) + at).to_array());
    let mid = DVec2::new(reach, lift);
    let ring = resample(&profile, 48);
    let rows = 10;
    let mut shell = Tris::default();
    let apex = shell.vert(DVec3::new(mid.x, mid.y, depth), DVec2::new(0.0, depth));
    let mut grid: Vec<Vec<u32>> = Vec::new();
    for r in 0..rows {
        let a = FRAC_PI_2 * r as f64 / rows as f64;
        let (s, z) = (a.cos_of(), depth * a.sin_of());
        let mut row: Vec<u32> = ring
            .iter()
            .map(|&q| {
                let q = mid + (q - mid) * s;
                shell.vert(DVec3::new(q.x, q.y, z), q)
            })
            .collect();
        row.push(row[0]);
        grid.push(row);
    }
    grid.push(vec![apex; ring.len() + 1]);
    for r in 0..rows {
        for j in 0..ring.len() {
            shell.quad(
                grid[r][j],
                grid[r + 1][j],
                grid[r + 1][j + 1],
                grid[r][j + 1],
            );
        }
    }
    let base = DVec3::new(mid.x, mid.y, 0.0);
    shell.face(0, |n, m| n.dot(m - base) >= 0.0);
    let k = 1.0 - rim / size.min_element();
    let inset = scaled(
        &Outline(ring.iter().map(|q| q.to_array()).collect()),
        mid,
        k,
    );
    let outer = Outline(ring.iter().map(|q| q.to_array()).collect());
    let shape = Shape {
        outer: outer.clone(),
        holes: vec![inset.clone()],
    };
    let cap: Vec<u32> = outer
        .0
        .iter()
        .chain(&inset.0)
        .map(|&q| shell.vert(DVec3::new(q[0], q[1], 0.0), DVec2::from(q)))
        .collect();
    let from = shell.tris.len();
    for t in outline::triangulate(&shape) {
        shell.tri(cap[t[0] as usize], cap[t[1] as usize], cap[t[2] as usize]);
    }
    shell.face(from, |n, _| n.z <= 0.0);
    let mut housing = shell.mesh(&paint, 40.0);
    let mut glass = slab(&inset, 0.003, 0.007, MIRROR_GLASS);
    let end = DVec3::new(mid.x - size.x * 0.4, mid.y, depth * 0.45);
    let mut arm = mesh::sweep(
        &[
            [0.0, 0.0, 0.0],
            [end.x * 0.5, end.y * 0.6, depth * 0.25],
            end.to_array(),
        ],
        &[stalk],
        12,
        false,
        true,
    );
    arm.set_material(TRIM);
    if !left {
        let flip = DMat4::from_scale(DVec3::new(-1.0, 1.0, 1.0));
        for m in [&mut arm, &mut housing, &mut glass] {
            mesh::transform(m, flip);
        }
    }
    let materials = BTreeMap::from([
        (
            TRIM.to_string(),
            json!({ "base": [0.018, 0.018, 0.02], "roughness": 0.55 }),
        ),
        (
            MIRROR_GLASS.to_string(),
            json!({ "family": "metal", "base": [0.92, 0.93, 0.94], "roughness": 0.02, "metalness": 1.0 }),
        ),
    ]);
    let mut facts = Map::new();
    facts.insert("side".into(), json!(if left { "left" } else { "right" }));
    Ok(Generated {
        pieces: vec![
            piece(name, "stalk", arm),
            piece(name, "housing", housing),
            piece(name, "glass", glass),
        ],
        facts,
        materials,
        lights: Vec::new(),
        ..Generated::default()
    })
}

fn wiper(name: &str, p: &Params) -> Result<Generated, String> {
    known(p, &WIPER, "wiper")?;
    let length = ranged(p, "length", 0.55, 0.1, 2.0)?;
    let angle = ranged(p, "angle", 0.0, -360.0, 360.0)?;
    let reach = ranged(p, "arm", length * 0.85, 0.05, 3.0)?;
    let width = ranged(p, "width", 0.02, 0.005, 0.1)?;
    let turn = DQuat::from_rotation_z(angle.to_radians());
    let mut arm = placed(
        mesh::rounded_box([reach, width * 0.9, width * 0.6], width * 0.2, 2),
        ARM,
        turn * DVec3::new(reach * 0.5, 0.0, width * 0.9),
        turn,
    );
    arm.append(&placed(
        mesh::cylinder(width * 1.1, width * 1.5, 16, true),
        ARM,
        DVec3::new(0.0, 0.0, width * 0.75),
        DQuat::from_rotation_x(FRAC_PI_2),
    ));
    let blade = placed(
        mesh::rounded_box([length, width * 0.6, width * 0.8], width * 0.15, 2),
        RUBBER,
        turn * DVec3::new(reach, 0.0, width * 0.4),
        turn,
    );
    let materials = BTreeMap::from([
        (
            ARM.to_string(),
            json!({ "base": [0.02, 0.02, 0.022], "roughness": 0.5 }),
        ),
        (
            RUBBER.to_string(),
            json!({ "base": [0.015, 0.015, 0.016], "roughness": 0.85 }),
        ),
    ]);
    let tip = turn * DVec3::new(reach, 0.0, 0.0);
    let mut facts = Map::new();
    facts.insert("pivot".into(), json!([0.0, 0.0, 0.0]));
    facts.insert("tip".into(), json!(tip.to_array()));
    Ok(Generated {
        pieces: vec![piece(name, "arm", arm), piece(name, "blade", blade)],
        facts,
        materials,
        lights: Vec::new(),
        ..Generated::default()
    })
}

fn exhaust(name: &str, p: &Params) -> Result<Generated, String> {
    known(p, &EXHAUST, "exhaust")?;
    let pattern = choice(p, "pattern", "single", &PATTERNS)?;
    let (groups, count): (&[f64], u64) = match pattern {
        "single" => (&[0.0], 1),
        "twin" => (&[-0.5, 0.5], 1),
        "quad" => (&[0.0], 4),
        _ => (&[-0.5, 0.5], 2),
    };
    let count = whole(p, "count", count, 1, 8)?;
    let oval = choice(p, "shape", "round", &["round", "oval"])? == "oval";
    let diameter = ranged(p, "diameter", 0.09, 0.02, 0.4)?;
    let aspect = ranged(p, "aspect", 0.6, 0.2, 1.0)?;
    let spacing = ranged(p, "spacing", diameter * 1.25, 0.0, 2.0)?;
    let length = ranged(p, "length", 0.12, 0.02, 1.0)?;
    let wall = ranged(p, "wall", 0.004, 0.001, diameter * 0.25)?;
    if count > 1 && spacing < diameter * 1.02 {
        return Err(format!(
            "params.spacing: {spacing} makes the tips overlap, expected at least {}",
            diameter * 1.02
        ));
    }
    let row = spacing * (count - 1) as f64 + diameter;
    let split = ranged(p, "split", 1.0, 0.0, 4.0)?;
    if groups.len() > 1 && split < row + diameter * 0.25 {
        return Err(format!(
            "params.split: {split} makes the two groups overlap, expected at least {}",
            row + diameter * 0.25
        ));
    }
    let r = diameter * 0.5;
    let shape = |c: [f64; 2], r: f64| {
        if oval {
            outline::superellipse(c, [r, r * aspect], 2.5, 32)
        } else {
            outline::circle(c, r, 28)
        }
    };
    let recess = (length * 0.5).min(0.03);
    let mut tips = Mesh::default();
    let mut bores = Mesh::default();
    let mut at = Vec::new();
    for g in groups {
        for i in 0..count {
            let x = g * split + (i as f64 - (count - 1) as f64 * 0.5) * spacing;
            at.push([x, 0.0, -length]);
            let inner = shape([x, 0.0], r - wall);
            let mut tube = mesh::extrude_shape(
                &Shape {
                    outer: shape([x, 0.0], r),
                    holes: vec![inner.clone()],
                },
                0.0,
                length,
                &Profile::NONE,
                UvMode::Local,
            );
            tube.set_material(TIP);
            tips.append(&tube);
            bores.append(&slab(&inner, -length + recess, 0.0, BORE));
        }
    }
    let materials = BTreeMap::from([
        (
            TIP.to_string(),
            json!({ "family": "metal", "base": [0.78, 0.78, 0.8], "roughness": 0.18, "metalness": 1.0 }),
        ),
        (
            BORE.to_string(),
            json!({ "base": [0.008, 0.008, 0.008], "roughness": 0.9 }),
        ),
    ]);
    let mut facts = Map::new();
    facts.insert("pattern".into(), json!(pattern));
    facts.insert("tips".into(), json!(at));
    facts.insert("diameter".into(), json!(diameter));
    Ok(Generated {
        pieces: vec![piece(name, "tips", tips), piece(name, "bores", bores)],
        facts,
        materials,
        lights: Vec::new(),
        ..Generated::default()
    })
}

fn extrusion(name: &str, p: &Params) -> Result<Generated, String> {
    known(p, &EXTRUSION, "extrusion")?;
    sweep_profile::sweep(name, p, "extrusion", (TRIM, sweep_profile::trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(v: Value) -> Params {
        let Value::Object(m) = v else { unreachable!() };
        m.into_iter().collect()
    }

    fn bounds(g: &Generated) -> [[f64; 3]; 2] {
        let mut all = Mesh::default();
        for p in &g.pieces {
            assert!(p.mesh.triangle_count() > 0, "{}", p.name);
            assert!(
                p.mesh.positions.iter().flatten().all(|c| c.is_finite()),
                "{}",
                p.name
            );
            all.append(&p.mesh);
        }
        mesh::bounds(&all).unwrap()
    }

    fn names(g: &Generated) -> Vec<&str> {
        g.pieces.iter().map(|p| p.name.as_str()).collect()
    }

    fn within(b: [[f64; 3]; 2], lo: [f64; 3], hi: [f64; 3]) {
        for k in 0..3 {
            assert!(b[0][k] >= lo[k] - 1e-6 && b[1][k] <= hi[k] + 1e-6, "{b:?}");
            assert!(b[1][k] > b[0][k], "{b:?}");
        }
    }

    #[test]
    fn a_lamp_builds_its_three_nodes_and_emits_its_spot_light() {
        let g = part(
            "head_l",
            &params(json!({
                "kind": "lamp",
                "outline": [[-0.2, -0.04], [0.2, -0.04], [0.18, 0.05], [-0.18, 0.05]],
                "depth": 0.1,
                "bars": [[[-0.15, 0.0], [0.15, 0.0], [0.15, 0.01], [-0.15, 0.01]]],
                "light": { "intensity": 120.0, "cone": [10.0, 30.0] },
            })),
        )
        .unwrap();
        assert_eq!(names(&g), ["head_l.housing", "head_l.lens", "head_l.bars"]);
        within(bounds(&g), [-0.2, -0.04, -0.1], [0.2, 0.05, 0.0]);
        assert_eq!(g.lights.len(), 1);
        let l = &g.lights[0];
        assert_eq!(l.name, "head_l.light");
        assert_eq!(l.direction, [0.0, 0.0, 1.0]);
        assert_eq!(l.intensity, 120.0);
        assert_eq!(l.cone, [10.0, 30.0]);
        assert_eq!(l.parent.as_deref(), Some("head_l.housing"));
        assert!((l.position[1] - 0.005).abs() < 1e-9 && l.position[2] > 0.0);
        assert!(g.materials.keys().any(|k| k.starts_with("vehicle_bar_")));
        let tail = part("t", &params(json!({ "kind": "lamp", "role": "tail" }))).unwrap();
        assert_eq!(tail.lights[0].intensity, 6.0);
        assert!(tail.lights[0].color[0] > tail.lights[0].color[1] * 5.0);
        let dark = part("d", &params(json!({ "kind": "lamp", "light": false }))).unwrap();
        assert!(dark.lights.is_empty());
    }

    #[test]
    fn every_kind_builds_with_sane_bounds_and_the_same_bytes() {
        let kinds = [
            (
                json!({ "kind": "mirror" }),
                [-0.02, -0.1, -0.02],
                [0.35, 0.15, 0.1],
            ),
            (
                json!({ "kind": "mirror", "side": "right" }),
                [-0.35, -0.1, -0.02],
                [0.02, 0.15, 0.1],
            ),
            (
                json!({ "kind": "wiper", "length": 0.6, "angle": 90.0 }),
                [-0.05, -0.05, -0.01],
                [0.05, 0.9, 0.05],
            ),
            (
                json!({ "kind": "exhaust", "pattern": "split-quad", "split": 1.0 }),
                [-0.7, -0.05, -0.12],
                [0.7, 0.05, 0.0],
            ),
            (
                json!({ "kind": "exhaust", "pattern": "twin", "shape": "oval" }),
                [-0.6, -0.03, -0.12],
                [0.6, 0.03, 0.0],
            ),
            (
                json!({ "kind": "extrusion", "profile": [[-0.01, 0.0], [0.01, 0.0], [0.01, 0.12], [-0.01, 0.12]], "path": [[0.0, 0.0, 0.0], [0.0, 0.1, -0.5]], "repeat": 5, "step": [0.15, 0.0, 0.0] }),
                [-0.32, -0.02, -0.52],
                [0.32, 0.23, 0.03],
            ),
            (
                json!({ "kind": "extrusion", "profile": [[0.0, 0.0], [0.08, 0.0], [0.08, 0.02], [0.0, 0.04]], "path": [[0.9, 0.1, 2.0], [0.95, 0.1, 0.0], [0.9, 0.1, -2.0]] }),
                [0.85, 0.09, -2.01],
                [1.04, 0.15, 2.01],
            ),
        ];
        for (p, lo, hi) in kinds {
            let p = params(p);
            let g = part("x", &p).unwrap();
            within(bounds(&g), lo, hi);
            let again = part("x", &p).unwrap();
            assert_eq!(
                g.pieces.iter().map(|x| &x.mesh).collect::<Vec<_>>(),
                again.pieces.iter().map(|x| &x.mesh).collect::<Vec<_>>()
            );
            assert!(g.lights.is_empty());
        }
        let quad = part(
            "q",
            &params(json!({ "kind": "exhaust", "pattern": "quad" })),
        )
        .unwrap();
        assert_eq!(quad.facts["part"]["tips"].as_array().unwrap().len(), 4);
        assert_eq!(names(&quad), ["q.tips", "q.bores"]);
    }

    #[test]
    fn bad_params_name_what_they_expect() {
        for (p, says) in [
            (json!({}), "params.kind"),
            (json!({ "kind": "lamp", "count": 2 }), "params.count"),
            (
                json!({ "kind": "exhaust", "pattern": "quad", "spacing": 0.05 }),
                "params.spacing",
            ),
            (
                json!({ "kind": "extrusion", "profile": [[0.0, 0.0], [1.0, 0.0]] }),
                "params.profile",
            ),
            (
                json!({ "kind": "lamp", "light": { "kind": "point" } }),
                "params.light.kind",
            ),
        ] {
            let e = part("x", &params(p)).err().unwrap();
            assert!(e.starts_with(says), "{e}");
        }
    }
}
