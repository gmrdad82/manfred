use crate::scene::{Camera, Grade, Lamp, Motion, MoverSpec, Room, Sky, Sun};
use crate::{Error, Options};
use pfx_load::Mesh;
use serde_json::Value;

const SENSOR_MM: f64 = 36.0;
const FRAME_ASPECT: f64 = 1586.0 / 992.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Sidecar {
    pub camera: Camera,
    pub sun: Sun,
    pub sky: Sky,
    pub room: Room,
    pub exposure: f32,
    pub grade: Grade,
    pub plate: Option<(f32, f32)>,
    pub motion: Option<Motion>,
    pub movers: Vec<MoverSpec>,
}

pub trait Reader {
    fn read(&self, json: &Value, options: &Options) -> Result<Sidecar, Error>;
}

impl<F> Reader for F
where
    F: Fn(&Value, &Options) -> Result<Sidecar, Error>,
{
    fn read(&self, json: &Value, options: &Options) -> Result<Sidecar, Error> {
        self(json, options)
    }
}

fn vec3(v: &Value, fallback: [f32; 3]) -> [f32; 3] {
    match v.as_array() {
        Some(a) if a.len() == 3 => [
            a[0].as_f64().unwrap_or(0.0) as f32,
            a[1].as_f64().unwrap_or(0.0) as f32,
            a[2].as_f64().unwrap_or(0.0) as f32,
        ],
        _ => fallback,
    }
}

fn plane(v: &Value) -> [f32; 4] {
    v.as_array()
        .map(|a| [0, 1, 2, 3].map(|i| a.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32))
        .unwrap_or([0.0; 4])
}

fn num(v: &Value, fallback: f32) -> f32 {
    v.as_f64().map(|x| x as f32).unwrap_or(fallback)
}

pub(crate) fn parse(json: &str) -> Result<Value, Error> {
    if json.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(json).map_err(|e| Error::Sidecar(e.to_string()))
}

pub(crate) fn fill(value: Option<&str>) -> f32 {
    value
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|f| f.is_finite() && *f >= 0.0)
        .unwrap_or(0.25)
}

pub fn facts(meta: &Value, options: &Options) -> Result<Sidecar, Error> {
    let plate = meta["plate"]
        .as_object()
        .map(|p| (num(&p["width"], 1586.0), num(&p["height"], 992.0)));
    Ok(Sidecar {
        camera: camera(&meta["cameras"][0], plate, options),
        sun: sun(meta),
        sky: sky(meta, options),
        room: room(&meta["sky"]["room"]),
        exposure: num(&meta["look"]["exposure"], 1.0),
        grade: Grade {
            vignette: num(&meta["look"]["grade"]["vignette"], 0.0),
            saturation: num(&meta["look"]["grade"]["saturation"], 1.0),
            warmth: num(&meta["look"]["grade"]["warmth"], 0.0),
            bloom: num(&meta["look"]["grade"]["bloom"], 0.0),
            haze: num(&meta["look"]["grade"]["haze"], 0.15),
        },
        plate,
        motion: meta["motion"].as_object().map(|_| {
            let m = &meta["motion"];
            let lo = vec3(&m["box_lo"], [0.0; 3]);
            let hi = vec3(&m["box_hi"], [0.0; 3]);
            Motion {
                cup: vec3(&m["cup"], [0, 1, 2].map(|k| (lo[k] + hi[k]) * 0.5)),
                radius: num(&m["radius"], 0.02),
                lo,
                hi,
                density: num(&m["density"], 1.0),
                anisotropy: num(&m["anisotropy"], 0.35),
                dust: num(&m["dust"], 1.0),
                mote: num(&m["mote"], 0.0006),
                ambient: num(&m["ambient"], 0.05),
            }
        }),
        movers: listed(&meta["movers"]),
    })
}

fn mover(name: &str, m: &Value) -> MoverSpec {
    MoverSpec {
        name: name.to_string(),
        pivot: vec3(&m["pivot"], [0.0; 3]),
        axis: vec3(&m["axis"], [0.0, 0.0, -1.0]),
        travel: num(&m["travel"], 0.0),
        clip: plane(&m["clip"]),
        clip2: plane(&m["clip2"]),
    }
}

fn listed(value: &Value) -> Vec<MoverSpec> {
    value
        .as_object()
        .map(|all| {
            let mut list: Vec<MoverSpec> = all.iter().map(|(name, m)| mover(name, m)).collect();
            list.sort_by(|a, b| a.name.cmp(&b.name));
            list
        })
        .unwrap_or_default()
}

fn extras_movers(mesh: &Mesh) -> Result<Vec<MoverSpec>, Error> {
    let mut list: Vec<MoverSpec> = Vec::new();
    for node in &mesh.nodes {
        if node.extra_object("manf").is_some() {
            return Err(Error::Glb(format!(
                "node `{}` carries `extras.manf`, the old key; Manfred reads `extras.manfred`, so rebuild the GLB with `manfred build`",
                node.name
            )));
        }
        let Some(manfred) = node.extra_object("manfred") else {
            continue;
        };
        if !manfred.contains_key("pivot") {
            continue;
        }
        let Some(group) = manfred.get("group").and_then(Value::as_str) else {
            return Err(Error::Glb(format!(
                "node `{}` carries a mover's pivot in its extras without a group",
                node.name
            )));
        };
        if list.iter().any(|m| m.name == group) {
            return Err(Error::Glb(format!(
                "two nodes carry the mover `{group}` in their extras"
            )));
        }
        list.push(mover(group, &Value::Object(manfred.clone())));
    }
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(list)
}

pub(crate) fn with_extras(
    from_sidecar: Vec<MoverSpec>,
    mesh: &Mesh,
) -> Result<Vec<MoverSpec>, Error> {
    let inside = extras_movers(mesh)?;
    if inside.is_empty() {
        return Ok(from_sidecar);
    }
    if !from_sidecar.is_empty() {
        for one in &inside {
            if from_sidecar.iter().find(|m| m.name == one.name) != Some(one) {
                return Err(Error::Sidecar(format!(
                    "the mover `{}` differs between the sidecar and the GLB's extras",
                    one.name
                )));
            }
        }
        if let Some(missing) = from_sidecar
            .iter()
            .find(|m| !inside.iter().any(|one| one.name == m.name))
        {
            return Err(Error::Sidecar(format!(
                "the mover `{}` is in the sidecar and not in the GLB's extras",
                missing.name
            )));
        }
    }
    Ok(inside)
}

fn sun(meta: &Value) -> Sun {
    Sun {
        toward: vec3(&meta["sun"]["toward"], [-0.5, 0.7, 0.5]),
        radius_deg: num(&meta["sun"]["radius_deg"], 0.27),
        color: vec3(&meta["sun"]["color"], [1.0, 0.93, 0.82]),
        irradiance: num(&meta["sun"]["irradiance"], 0.0),
    }
}

fn sky(meta: &Value, options: &Options) -> Sky {
    let balance = meta["sky"]["balance"]
        .as_array()
        .map(|a| match a.as_slice() {
            [r, g, b] => [r, g, b].map(|v| v.as_f64().unwrap_or(0.0) as f32),
            _ => [1.0; 3],
        });
    Sky {
        choice: options.sky,
        fill: options.fill,
        balance,
        intensity: num(&meta["sky"]["intensity"], 1.0),
        rotation_deg: num(&meta["sky"]["rotation_deg"], 0.0),
        hdr_file: meta["sky"]["hdr"].as_str().map(str::to_string),
        hdr: None,
    }
}

fn room(spec: &Value) -> Room {
    let rgb = |v: &Value, d: [f32; 3]| match v.as_array() {
        Some(a) if a.len() == 3 => [num(&a[0], d[0]), num(&a[1], d[1]), num(&a[2], d[2])],
        _ => d,
    };
    Room {
        width: num(&spec["width"], 1024.0) as u32,
        floor: rgb(&spec["floor"], [0.16, 0.10, 0.06]),
        wall: rgb(&spec["wall"], [0.30, 0.24, 0.18]),
        ceiling: rgb(&spec["ceiling"], [0.55, 0.47, 0.38]),
        lights: spec["lights"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|l| Lamp {
                        name: l["name"].as_str().unwrap_or_default().to_string(),
                        az: num(&l["az"], 0.0),
                        el: num(&l["el"], 30.0),
                        power: num(&l["power"], 4.0),
                        color: rgb(&l["color"], [1.0, 0.9, 0.78]),
                        width: num(&l["width"], 30.0),
                        height: num(&l["height"], 20.0),
                        soft: num(&l["soft"], 0.08),
                        slats: num(&l["slats"], 0.0),
                        open: num(&l["open"], 1.0),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn frame_aspect(plate: Option<(f32, f32)>, options: &Options) -> f64 {
    match plate {
        Some((w, h)) if w > 0.0 && h > 0.0 => f64::from(w) / f64::from(h),
        _ => options.aspect.map_or(FRAME_ASPECT, f64::from),
    }
}

fn camera(c: &Value, plate: Option<(f32, f32)>, options: &Options) -> Camera {
    let position = vec3(&c["position"], [0.0, 0.3, 1.0]);
    let target = vec3(&c["target"], [0.0, 0.0, 0.0]);
    let sensor = c["sensor_mm"].as_f64().unwrap_or(SENSOR_MM);
    let longest = frame_aspect(plate, options).max(1.0);
    let half_width = match (c["fov_y_deg"].as_f64(), c["focal_mm"].as_f64()) {
        (Some(fov), _) => Some((fov.to_radians() / 2.0).tan() * longest),
        (None, Some(focal)) if focal > 0.0 => Some(sensor / (2.0 * focal)),
        _ => None,
    };
    let fov_y = match half_width {
        Some(half) => (2.0 * (half / longest).atan()).to_degrees() as f32,
        None => 30.0,
    };
    let focal = match c["focal_mm"].as_f64() {
        Some(focal) => Some(focal),
        None => half_width.filter(|h| *h > 0.0).map(|h| sensor / (2.0 * h)),
    };
    let fstop = c["fstop"].as_f64().unwrap_or(0.0);
    let aperture = match focal {
        Some(focal) if fstop > 0.0 => (focal / 1000.0 / fstop) as f32,
        _ => 0.0,
    };
    let distance = (0..3)
        .map(|i| (f64::from(target[i]) - f64::from(position[i])).powi(2))
        .sum::<f64>()
        .sqrt();
    let shift = match c["shift"].as_array() {
        Some(a) if a.len() == 2 => [num(&a[0], 0.0), num(&a[1], 0.0)],
        _ => [0.0, 0.0],
    };
    Camera {
        position,
        target,
        up: vec3(&c["up"], [0.0, 1.0, 0.0]),
        fov_y,
        shift,
        aperture,
        focus: match c["focus_m"].as_f64() {
            Some(focus) => focus as f32,
            None if c.is_object() => distance as f32,
            None => 1.0,
        },
        orthographic: c["orthographic"].as_bool().unwrap_or(false),
        ortho_height: c["ortho_height"].as_f64().map(|h| h as f32),
    }
}
