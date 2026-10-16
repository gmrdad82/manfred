mod common;

use common::{cube, golden};
use manfred_build::{Built, Cache, Call, Generated, Registry, build_cached, build_with};
use manfred_recipe::{Kind, Recipe, load, parse};
use serde_json::json;
use std::path::Path;

fn stack(call: &Call) -> Result<Generated, String> {
    let mut piece = cube("stack_top", "wood");
    let lift = (call.seed % 1000) as f32 / 1000.0;
    for p in &mut piece.mesh.positions {
        p[1] += lift;
    }
    let mut facts = serde_json::Map::new();
    facts.insert("stack".into(), json!({ "lift": lift }));
    Ok(Generated {
        pieces: vec![piece],
        facts,
        materials: [("wood".to_string(), json!({ "base_color": [0.5, 0.3, 0.1] }))].into(),
        ..Generated::default()
    })
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.register_with_facts("test/stack", stack);
    registry
}

const SCENE: &str = r#"manfred = 1
name = "cache"
seed = 7

[output]
sidecar_dialect = "facts"

[plate]
width = 200
height = 100
metres_per_pixel = 0.01
origin = "top_left"

[[camera]]
name = "main"
position = [0.0, 0.0, 4.0]
target = [0.0, 0.0, 0.0]
fov_y_deg = 60.0

[[object]]
name = "base"
kind = "box"
size = [1.0, 0.2, 1.0]

[[object]]
name = "lid"
kind = "box"
size = [0.5, 0.1, 0.5]
parent = "base"
at = [0.0, 0.2, 0.0]

[[object]]
name = "slab"
kind = "outline"
front = 0.0
back = 0.02
outline = { rect = [-0.1, -0.05, 0.1, 0.05], radius = 0.01 }

[[object]]
name = "hole"
kind = "pocket"
into = "slab"
outline = { circle = [0.05, 0.0, 0.02], segments = 32 }
depth = 0.01

[[object]]
name = "stack"
kind = "generator"
generator = "test/stack"
at = [2.0, 0.0, 0.0]

[[object]]
name = "screen"
kind = "box"
size = [0.4, 0.3, 0.01]
uv = "plate"
at = [-1.0, 0.0, 0.0]
"#;

fn same(cached: &Built, full: &Built) {
    assert!(
        cached.glb() == full.glb(),
        "the GLB differs from a full build"
    );
    assert_eq!(cached.sidecar(), full.sidecar());
    assert_eq!(format!("{cached:?}"), format!("{full:?}"));
}

fn step(recipe: &Recipe, base: &Path, registry: &Registry, cache: &mut Cache) -> Vec<usize> {
    let cached = build_cached(recipe, base, registry, cache).unwrap();
    same(&cached, &build_with(recipe, base, registry).unwrap());
    cache.rebuilt().to_vec()
}

fn index(recipe: &Recipe, name: &str) -> usize {
    recipe.objects.iter().position(|o| o.name == name).unwrap()
}

#[test]
fn a_one_object_edit_rebuilds_that_object_and_matches_a_full_build() {
    let registry = registry();
    let base = Path::new("");
    let mut recipe = parse(SCENE).unwrap();
    let mut cache = Cache::default();
    assert_eq!(step(&recipe, base, &registry, &mut cache), [0, 1, 2, 4, 5]);
    assert_eq!(cache.len(), 5);
    assert_eq!(step(&recipe, base, &registry, &mut cache), [] as [usize; 0]);

    let screen = index(&recipe, "screen");
    recipe.objects[screen].at[0] = -1.25;
    assert_eq!(step(&recipe, base, &registry, &mut cache), [screen]);
    assert_eq!(cache.len(), 5);

    let lid = index(&recipe, "lid");
    recipe.objects[lid].at[1] = 0.25;
    assert_eq!(step(&recipe, base, &registry, &mut cache), [lid]);

    let parent = index(&recipe, "base");
    recipe.objects[parent].rotate[1] = 30.0;
    assert_eq!(step(&recipe, base, &registry, &mut cache), [parent, lid]);

    let hole = index(&recipe, "hole");
    if let Kind::Pocket { depth, .. } = &mut recipe.objects[hole].kind {
        *depth = 0.015;
    }
    assert_eq!(
        step(&recipe, base, &registry, &mut cache),
        [index(&recipe, "slab")]
    );

    recipe.cameras[0].position[2] = 5.0;
    assert_eq!(step(&recipe, base, &registry, &mut cache), [screen]);

    recipe.seed = 8;
    assert_eq!(
        step(&recipe, base, &registry, &mut cache),
        [index(&recipe, "stack")]
    );

    let stack = index(&recipe, "stack");
    recipe.objects.swap(lid, stack);
    assert_eq!(step(&recipe, base, &registry, &mut cache), [lid]);
}

#[test]
fn a_failed_build_keeps_the_cache_and_the_next_one_reuses_it() {
    let registry = registry();
    let base = Path::new("");
    let mut recipe = parse(SCENE).unwrap();
    let mut cache = Cache::default();
    build_cached(&recipe, base, &registry, &mut cache).unwrap();
    let lid = index(&recipe, "lid");
    recipe.objects[lid].parent = Some("nobody".into());
    let error = build_cached(&recipe, base, &registry, &mut cache).unwrap_err();
    assert_eq!(
        error,
        build_with(&recipe, base, &registry).unwrap_err(),
        "a cached build fails as a full one does"
    );
    assert_eq!(cache.len(), 5);
    recipe.objects[lid].parent = Some("base".into());
    assert_eq!(step(&recipe, base, &registry, &mut cache), [] as [usize; 0]);
}

#[test]
fn a_generator_clash_is_still_refused_when_its_object_is_cached() {
    let registry = registry();
    let base = Path::new("");
    let mut recipe = parse(SCENE).unwrap();
    let mut cache = Cache::default();
    build_cached(&recipe, base, &registry, &mut cache).unwrap();
    let mut other = recipe.objects[index(&recipe, "stack")].clone();
    other.name = "second".into();
    recipe.objects.push(other);
    let error = build_cached(&recipe, base, &registry, &mut cache).unwrap_err();
    assert_eq!(error, build_with(&recipe, base, &registry).unwrap_err());
    assert!(error.text.contains("stack_top"), "{error}");
}

fn under(recipe: &Recipe, mut at: usize, name: &str) -> bool {
    for _ in 0..recipe.objects.len() {
        let Some(parent) = &recipe.objects[at].parent else {
            return false;
        };
        if parent == name {
            return true;
        }
        match recipe.objects.iter().rposition(|o| &o.name == parent) {
            Some(next) => at = next,
            None => return false,
        }
    }
    false
}

#[test]
fn every_object_of_the_golden_scenes_rebuilds_alone() {
    let registry = Registry::new();
    for set in ["table", "toybox", "cards", "flatslab"] {
        let folder = golden(set);
        let mut recipe = load(&folder.join(format!("{set}.toml"))).unwrap().recipe;
        let mut cache = Cache::default();
        build_cached(&recipe, &folder, &registry, &mut cache).unwrap();
        for at in 0..recipe.objects.len() {
            if matches!(recipe.objects[at].kind, Kind::Pocket { .. }) {
                continue;
            }
            recipe.objects[at].at[2] += 0.001;
            let rebuilt = step(&recipe, &folder, &registry, &mut cache);
            let name = recipe.objects[at].name.clone();
            assert!(rebuilt.contains(&at), "{set}: {name}");
            for other in rebuilt {
                assert!(
                    other == at
                        || matches!(recipe.objects[other].kind, Kind::Mesh { .. })
                        || under(&recipe, other, &name),
                    "{set}: an edit of {name} rebuilt {}",
                    recipe.objects[other].name
                );
            }
        }
    }
}
