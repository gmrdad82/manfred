use crate::Registry;

pub mod road;

pub fn builtins(registry: &mut Registry) {
    registry.register_with_facts("manfred/road", road::generate);
}
