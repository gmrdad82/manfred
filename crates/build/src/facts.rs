use manfred_glb::{
    CameraFacts, DaylightFacts, GradeFacts, LampFacts, LookFacts, MoverFacts, RoomFacts,
    SceneFacts, SkyFacts, SunFacts, merge,
};
use manfred_recipe::{Projection, Recipe, Room};
use serde_json::{Map, Value};

pub(crate) fn facts(r: &Recipe, generated: &Map<String, Value>) -> SceneFacts {
    let cameras = r
        .cameras
        .iter()
        .map(|c| CameraFacts {
            name: c.name.clone(),
            orthographic: c.kind == Projection::Orthographic,
            position: c.position,
            target: c.target,
            up: c.up,
            fov_y_deg: c.fov_y_deg,
            focal_mm: c.focal_mm,
            sensor_mm: c.sensor_mm,
            shift: c.shift,
            focus_m: c.focus_m,
            fstop: c.fstop,
            ortho_height: c.ortho_height,
        })
        .collect();
    let sun = r.sun.as_ref().map(|s| SunFacts {
        toward: s.toward,
        radius_deg: s.radius_deg,
        color: s.color,
        irradiance: s.irradiance,
    });
    let daylight = r.daylight.as_ref().map(|d| DaylightFacts {
        hour: d.hour,
        day: d.day,
        latitude: d.latitude,
        heading: d.heading,
    });
    let sky = r.sky.as_ref().map(|s| SkyFacts {
        intensity: s.intensity,
        rotation_deg: s.rotation_deg,
        balance: s.balance,
        room: s.room.as_ref().map(room),
    });
    let movers = r
        .movers
        .iter()
        .map(|m| MoverFacts {
            group: m.group.clone(),
            pivot: m.pivot,
            axis: m.axis,
            travel: m.travel,
            clip: m.clip,
            clip2: m.clip2,
        })
        .collect();
    let mut extra = Map::new();
    merge(&mut extra, generated);
    merge(&mut extra, &r.sidecar);
    SceneFacts {
        cameras,
        sun,
        daylight,
        sky,
        plate: r.plate.as_ref().map(|p| [p.width, p.height]),
        movers,
        look: r.look.as_ref().map(|l| LookFacts {
            exposure: l.exposure,
            grade: GradeFacts {
                vignette: l.grade.vignette,
                saturation: l.grade.saturation,
                warmth: l.grade.warmth,
                bloom: l.grade.bloom,
                haze: l.grade.haze,
            },
        }),
        extra,
    }
}

fn room(room: &Room) -> RoomFacts {
    RoomFacts {
        width: room.width,
        floor: room.floor,
        wall: room.wall,
        ceiling: room.ceiling,
        lights: room
            .lights
            .iter()
            .map(|lamp| LampFacts {
                name: lamp.name.clone(),
                az: lamp.az,
                el: lamp.el,
                power: lamp.power,
                color: lamp.color,
                width: lamp.width,
                height: lamp.height,
                soft: lamp.soft,
                slats: lamp.slats,
                open: lamp.open,
            })
            .collect(),
    }
}
