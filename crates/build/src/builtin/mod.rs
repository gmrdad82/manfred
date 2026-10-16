use crate::Registry;

pub mod buildings;
pub mod furniture;
pub mod road;

pub fn builtins(registry: &mut Registry) {
    registry.register_with_facts("manfred/road", road::generate);
    registry.register_with_facts(buildings::NAME, buildings::generate);
    registry.register_with_facts(furniture::NAME, furniture::generate);
}
