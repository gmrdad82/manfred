use crate::scene::{Room, Scene, SkyChoice};
use pfx_load::Sky;

pub const CAP_LUMINANCE: f32 = 60.0;
pub const CAP_MEAN: f32 = 0.3;

pub fn pixels(scene: &Scene, daylight: Option<&[u8]>) -> Result<Sky, String> {
    if let (Some(_), Some(bytes)) = (&scene.sky.hdr_file, &scene.sky.hdr) {
        return decode(bytes);
    }
    let balance = scene.sky.balance;
    match scene.sky.choice {
        SkyChoice::Room => Ok(room(&scene.room)),
        SkyChoice::Hdr => daylight_sky(need(daylight)?, balance),
        SkyChoice::Mix => mix(need(daylight)?, &scene.room, balance, scene.sky.fill),
    }
}

fn need(daylight: Option<&[u8]>) -> Result<&[u8], String> {
    daylight.ok_or_else(|| "the daylight sky needs the caller's HDR".to_string())
}

pub fn room(room: &Room) -> Sky {
    Sky::room(room)
}

pub fn daylight_sky(hdr: &[u8], balance: Option<[f32; 3]>) -> Result<Sky, String> {
    let sky = decode(hdr)?.capped_luminance(CAP_LUMINANCE);
    Ok(match balance {
        Some(target) => sky.balanced_to(target),
        None => sky,
    }
    .scaled_to_mean_luminance(CAP_MEAN))
}

pub fn mix(hdr: &[u8], spec: &Room, balance: Option<[f32; 3]>, fill: f32) -> Result<Sky, String> {
    let mut sky = daylight_sky(hdr, balance)?;
    let bare = Room {
        width: sky.width,
        floor: [0.0; 3],
        wall: [0.0; 3],
        ceiling: [0.0; 3],
        lights: spec.lights.clone(),
    };
    let lamps = room(&bare);
    if lamps.width != sky.width || lamps.height != sky.height {
        return Ok(sky);
    }
    let have = mean_luminance(&lamps);
    if have <= 1e-9 {
        return Ok(sky);
    }
    let gain = fill * mean_luminance(&sky) / have;
    for (t, l) in sky.texels.iter_mut().zip(&lamps.texels) {
        for k in 0..3 {
            t[k] += l[k] * gain;
        }
    }
    Ok(sky)
}

pub fn decode(bytes: &[u8]) -> Result<Sky, String> {
    Sky::parse(bytes)
}

fn mean_luminance(sky: &Sky) -> f32 {
    let sum: f64 = sky
        .texels
        .iter()
        .map(|t| f64::from(0.2126 * t[0] + 0.7152 * t[1] + 0.0722 * t[2]))
        .sum();
    (sum / sky.texels.len().max(1) as f64) as f32
}
