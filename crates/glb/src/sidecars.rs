use crate::{
    CameraFacts, DaylightFacts, GradeFacts, LookFacts, MoverFacts, RoomFacts, SceneFacts, SkyFacts,
    SunFacts,
};
use serde_json::{Map, Value, json};

pub const FACTS: &str = "facts";

pub type SidecarWriter = fn(&SceneFacts) -> Result<Value, String>;

pub fn mover_json(m: &MoverFacts) -> Map<String, Value> {
    let mut one = Map::new();
    one.insert("pivot".into(), json!(m.pivot));
    one.insert("axis".into(), json!(m.axis));
    one.insert("travel".into(), json!(m.travel));
    if let Some(clip) = m.clip {
        one.insert("clip".into(), json!(clip));
    }
    if let Some(clip2) = m.clip2 {
        one.insert("clip2".into(), json!(clip2));
    }
    one
}

pub fn merge(into: &mut Map<String, Value>, from: &Map<String, Value>) {
    for (key, value) in from {
        match (into.get_mut(key), value) {
            (Some(Value::Object(a)), Value::Object(b)) => merge(a, b),
            _ => {
                into.insert(key.clone(), value.clone());
            }
        }
    }
}

fn camera(c: &CameraFacts) -> Value {
    let mut one = Map::new();
    one.insert("name".into(), json!(c.name));
    one.insert("orthographic".into(), json!(c.orthographic));
    one.insert("position".into(), json!(c.position));
    one.insert("target".into(), json!(c.target));
    one.insert("up".into(), json!(c.up));
    for (key, value) in [
        ("fov_y_deg", c.fov_y_deg),
        ("focal_mm", c.focal_mm),
        ("sensor_mm", c.sensor_mm),
        ("focus_m", c.focus_m),
        ("ortho_height", c.ortho_height),
    ] {
        if let Some(value) = value {
            one.insert(key.into(), json!(value));
        }
    }
    one.insert("shift".into(), json!(c.shift));
    one.insert("fstop".into(), json!(c.fstop));
    Value::Object(one)
}

fn sun(s: &SunFacts) -> Value {
    json!({
        "toward": s.toward,
        "radius_deg": s.radius_deg,
        "color": s.color,
        "irradiance": s.irradiance,
    })
}

fn daylight(d: &DaylightFacts) -> Value {
    json!({
        "hour": d.hour,
        "day": d.day,
        "latitude": d.latitude,
        "heading": d.heading,
    })
}

pub fn room_json(room: &RoomFacts) -> Value {
    let lights: Vec<Value> = room
        .lights
        .iter()
        .map(|lamp| {
            let mut one = Map::new();
            if !lamp.name.is_empty() {
                one.insert("name".into(), json!(lamp.name));
            }
            one.insert("az".into(), json!(lamp.az));
            one.insert("el".into(), json!(lamp.el));
            one.insert("width".into(), json!(lamp.width));
            one.insert("height".into(), json!(lamp.height));
            one.insert("power".into(), json!(lamp.power));
            one.insert("color".into(), json!(lamp.color));
            one.insert("soft".into(), json!(lamp.soft));
            if lamp.slats != 0.0 {
                one.insert("slats".into(), json!(lamp.slats));
            }
            if lamp.open != 1.0 {
                one.insert("open".into(), json!(lamp.open));
            }
            Value::Object(one)
        })
        .collect();
    json!({
        "width": room.width,
        "floor": room.floor,
        "wall": room.wall,
        "ceiling": room.ceiling,
        "lights": lights,
    })
}

fn sky(s: &SkyFacts) -> Value {
    let mut one = Map::new();
    one.insert("intensity".into(), json!(s.intensity));
    one.insert("rotation_deg".into(), json!(s.rotation_deg));
    if let Some(balance) = s.balance {
        one.insert("balance".into(), json!(balance));
    }
    if let Some(room) = &s.room {
        one.insert("room".into(), room_json(room));
    }
    Value::Object(one)
}

pub fn grade_json(g: &GradeFacts) -> Value {
    json!({
        "vignette": g.vignette,
        "saturation": g.saturation,
        "warmth": g.warmth,
        "bloom": g.bloom,
        "haze": g.haze,
    })
}

fn look(l: &LookFacts) -> Value {
    json!({"exposure": l.exposure, "grade": grade_json(&l.grade)})
}

pub fn facts(f: &SceneFacts) -> Result<Value, String> {
    let mut root = Map::new();
    if !f.cameras.is_empty() {
        root.insert(
            "cameras".into(),
            Value::Array(f.cameras.iter().map(camera).collect()),
        );
    }
    if let Some(s) = &f.sun {
        root.insert("sun".into(), sun(s));
    }
    if let Some(d) = &f.daylight {
        root.insert("daylight".into(), daylight(d));
    }
    if let Some(s) = &f.sky {
        root.insert("sky".into(), sky(s));
    }
    if let Some([width, height]) = f.plate {
        root.insert("plate".into(), json!({"width": width, "height": height}));
    }
    if !f.movers.is_empty() {
        let all: Map<String, Value> = f
            .movers
            .iter()
            .map(|m| (m.group.clone(), Value::Object(mover_json(m))))
            .collect();
        root.insert("movers".into(), Value::Object(all));
    }
    if let Some(l) = &f.look {
        root.insert("look".into(), look(l));
    }
    Ok(Value::Object(root))
}

pub fn sidecar_text(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text
}
