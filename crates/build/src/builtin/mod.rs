use crate::Registry;

pub mod buildings;
pub mod car;
pub mod furniture;
pub mod loft;
pub mod road;

pub fn builtins(registry: &mut Registry) {
    registry.register_with_facts(road::NAME, road::generate);
    registry.register_with_facts(buildings::NAME, buildings::generate);
    registry.register_with_facts(car::NAME, car::generate);
    registry.register_with_facts(furniture::NAME, furniture::generate);
    registry.register_with_facts(loft::NAME, loft::generate);
}
