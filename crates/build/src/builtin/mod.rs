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
