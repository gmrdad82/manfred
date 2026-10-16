use manfred_build::{Built, build};
use manfred_recipe::parse;
use std::path::Path;

fn built(params: &str) -> Built {
    let text = format!(
        "manfred = 1\nname = \"t\"\nseed = 3\n\n[[object]]\nname = \"hero\"\nkind = \"generator\"\ngenerator = \"manfred/car\"\nparams = {{ {params} }}\n"
    );
    build(&parse(&text).unwrap(), Path::new(".")).unwrap()
}

#[test]
fn a_car_is_the_same_bytes_for_the_same_params_and_changes_with_its_seed() {
    for style in ["sports", "sedan", "hatch", "van"] {
        let params = format!("style = \"{style}\", seed = 11");
        assert_eq!(built(&params).glb(), built(&params).glb(), "{style}");
    }
    assert_ne!(
        built("style = \"sedan\", seed = 11").glb(),
        built("style = \"sedan\", seed = 12").glb()
    );
}

#[test]
fn a_car_names_its_pieces_and_its_wheels_facts() {
    let b = built("paint = \"#c8102e\"");
    let names: Vec<&str> = b.parts.iter().map(|p| p.node.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "hero.body",
            "hero.glass",
            "hero.wheel_fl",
            "hero.wheel_fr",
            "hero.wheel_rl",
            "hero.wheel_rr",
            "hero.headlights",
            "hero.taillights",
        ]
    );
    let car = &b.facts.extra;
    assert_eq!(car["style"], "sports");
    assert_eq!(car["length"], 4.4);
    for wheel in ["fl", "fr", "rl", "rr"] {
        let w = &car["wheels"][wheel];
        assert_eq!(w["radius"], 0.34);
        assert_eq!(w["axle"], serde_json::json!([1.0, 0.0, 0.0]));
        assert_eq!(w["centre"][1], 0.34);
    }
    assert!(car["wheels"]["fl"]["centre"][0].as_f64().unwrap() > 0.0);
    assert!(car["wheels"]["fl"]["centre"][2].as_f64().unwrap() > 0.0);
    for name in [
        "car_paint_c8102e",
        "car_glass",
        "car_tyre",
        "car_rim",
        "car_head",
        "car_tail",
    ] {
        assert!(b.materials.contains_key(name), "{name}");
    }
    let triangles: usize = b.parts.iter().map(|p| p.node.mesh.triangle_count()).sum();
    assert!(triangles < 30_000, "{triangles}");
}
