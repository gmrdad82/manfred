use crate::Registry;

pub mod buildings;
pub mod car;
pub mod furniture;
pub mod loft;
pub mod road;
mod shared;
pub mod sweep_profile;
pub mod vehicle_parts;
pub mod wheel;

pub fn builtins(registry: &mut Registry) {
    registry.register_with_facts(road::NAME, road::generate);
    registry.register_with_facts(buildings::NAME, buildings::generate);
    registry.register_with_facts(car::NAME, car::generate);
    registry.register_with_facts(furniture::NAME, furniture::generate);
    registry.register_with_facts(loft::NAME, loft::generate);
    registry.register_with_facts(wheel::NAME, wheel::generate);
    registry.register_with_facts(sweep_profile::NAME, sweep_profile::generate);
    registry.register_with_facts(vehicle_parts::NAME, vehicle_parts::generate);
}

pub(crate) fn tagged(name: &str, value: &serde_json::Value) -> String {
    let text = value.to_string();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{name}_{:08x}", hash >> 32)
}

pub(crate) fn rgb(
    value: &serde_json::Value,
    key: &str,
    high: Option<f64>,
) -> Result<[f64; 3], String> {
    let want = match high {
        Some(high) => format!("params.{key}: expected [r, g, b], each from 0 to {high}"),
        None => format!("params.{key}: expected [r, g, b], each 0 or more"),
    };
    let list = value
        .as_array()
        .filter(|list| list.len() == 3)
        .ok_or(&want)?;
    let mut out = [0.0; 3];
    for (slot, item) in out.iter_mut().zip(list) {
        *slot = item
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 0.0 && high.is_none_or(|high| *v <= high))
            .ok_or(&want)?;
    }
    Ok(out)
}

pub(crate) fn within(
    value: &serde_json::Value,
    key: &str,
    lo: f64,
    hi: f64,
) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && (lo..=hi).contains(v))
        .ok_or_else(|| format!("params.{key}: expected a number from {lo} to {hi}"))
}

pub(crate) fn table<'a>(
    value: &'a serde_json::Value,
    key: &str,
    keys: &[&str],
) -> Result<&'a serde_json::Map<String, serde_json::Value>, String> {
    let map = value
        .as_object()
        .ok_or_else(|| format!("params.{key}: expected a table of {}", keys.join(", ")))?;
    if let Some(unknown) = map.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(format!(
            "params.{key}.{unknown}: unknown key, expected one of: {}",
            keys.join(", ")
        ));
    }
    Ok(map)
}
