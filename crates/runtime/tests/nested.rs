use manfred_build::build;
use manfred_recipe::parse;
use manfred_runtime::{Scene, load};
use std::path::Path;

const NESTED: &str = r#"
[[object]]
name = "base"
kind = "box"
size = [0.2, 0.1, 0.2]
at = [1.0, 0.0, 0.0]
rotate = [0.0, 90.0, 0.0]
keep_transform = true

[[object]]
name = "mv_lid"
kind = "box"
size = [0.2, 0.02, 0.2]
at = [0.0, 0.06, 0.0]
parent = "base"
keep_transform = true

[[object]]
name = "knob"
kind = "sphere"
radius = 0.01
at = [0.0, 0.02, 0.0]
parent = "mv_lid"

[[object]]
name = "hinge"
kind = "box"
size = [0.01, 0.01, 0.01]
at = [0.0, 0.05, 0.1]
parent = "base"
keep_transform = true
hidden = true

[[object]]
name = "pin"
kind = "box"
size = [0.01, 0.01, 0.01]
at = [0.01, 0.0, 0.0]
parent = "hinge"
keep_transform = true

[[object]]
name = "stand"
kind = "box"
size = [0.3, 0.01, 0.3]
at = [0.0, 0.0, 2.0]

[[object]]
name = "arm"
kind = "box"
size = [0.01, 0.1, 0.01]
at = [0.0, 1.0, 0.0]
parent = "stand"
keep_transform = true

[[mover]]
group = "lid"
pivot = [0.0, 0.07, -0.1]
axis = [1.0, 0.0, 0.0]
travel = 1.0
"#;

fn scene(hierarchy: bool) -> Scene {
    let text = format!("manfred = 1\nname = \"t\"\n[output]\nhierarchy = {hierarchy}\n{NESTED}");
    let built = build(&parse(&text).unwrap(), Path::new(".")).unwrap();
    load(&built.glb(), "").unwrap()
}

fn close(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3], within: f32) -> bool {
    a.iter()
        .flatten()
        .zip(b.iter().flatten())
        .all(|(x, y)| (x - y).abs() < within)
}

#[test]
fn a_nested_glb_loads_into_the_scene_the_flat_one_gives() {
    let flat = scene(false);
    let deep = scene(true);
    assert_eq!(deep.objects, flat.objects);
    assert_eq!(deep.groups, flat.groups);
    assert_eq!(deep.materials, flat.materials);
    assert_eq!(deep.movers.len(), 1);
    assert_eq!(deep.movers, flat.movers);
    assert_eq!(deep.tris.len(), flat.tris.len());
    for (a, b) in deep.tris.iter().zip(&flat.tris) {
        assert_eq!((a.object, a.material), (b.object, b.material));
        assert!(close(&a.p, &b.p, 1e-5), "{:?} {:?}", a.p, b.p);
        assert!(close(&a.n, &b.n, 1e-4), "{:?} {:?}", a.n, b.n);
    }
}
