use manfred_recipe::{Pick, Problem, Recipe, apply_set, apply_stage, parse, validate};

const STREET: &str = r#"manfred = 1
name = "street"

[output]
glb = "out/street.glb"
scene = "out/street.scene.toml"
scene_camera = "chase"

[timeline]
seconds = 2.0

[[route]]
name = "main"
points = [[0.0, 0.0, 0.0], [0.0, 0.0, 100.0]]

[[object]]
name = "road"
kind = "box"
size = [1.0, 1.0, 1.0]

[[object]]
name = "hero"
kind = "box"
size = [1.0, 1.0, 1.0]

[[object]]
name = "hero_badge"
kind = "box"
size = [0.1, 0.1, 0.1]
parent = "hero"

[[object]]
name = "traffic1"
kind = "box"
size = [1.0, 1.0, 1.0]

[[follow]]
object = "hero"
route = "main"
speed = [[0.0, 10.0]]

[[follow]]
object = "traffic1"
route = "main"
speed = [[0.0, 5.0]]

[[camera]]
name = "top"
kind = "perspective"
position = [0.0, 100.0, 50.0]
target = [0.0, 0.0, 50.0]
up = [0.0, 0.0, 1.0]
fov_y_deg = 60.0

[[camera]]
name = "chase"
kind = "perspective"
fov_y_deg = 60.0

[camera.rig]
follow = "hero"

[[light]]
name = "tail"
kind = "point"
parent = "hero"
at = [0.0, 0.5, -1.0]
intensity = 2.0

[[stage]]
name = "street"
objects = ["road"]
camera = "top"

[[stage]]
name = "car"
objects = ["hero*"]

[[stage]]
name = "city"
objects = ["traffic*"]

[[stage]]
name = "look"
objects = ["*"]
camera = "chase"
"#;

fn street() -> Recipe {
    parse(STREET).unwrap_or_else(|e| panic!("{e}"))
}

fn names(r: &Recipe) -> Vec<&str> {
    r.objects.iter().map(|o| o.name.as_str()).collect()
}

#[test]
fn a_stage_keeps_the_objects_of_its_stages_and_drops_what_rides_on_the_rest() {
    let r = street();
    assert_eq!(validate(&r), Vec::<Problem>::new());
    let first = apply_stage(&r, "1").unwrap();
    assert_eq!(names(&first), ["road"]);
    assert!(first.follows.is_empty());
    assert_eq!(
        first
            .cameras
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["top"]
    );
    assert!(first.lights.is_empty());
    assert_eq!(first.output.scene_camera, None);
    assert!(first.stages.is_empty());
    assert_eq!(validate(&first), Vec::<Problem>::new());
    let car = apply_stage(&r, "car").unwrap();
    assert_eq!(names(&car), ["road", "hero", "hero_badge"]);
    assert_eq!(car.follows.len(), 1);
    assert_eq!(car.cameras.len(), 2);
    assert_eq!(car.lights.len(), 1);
    assert_eq!(validate(&car), Vec::<Problem>::new());
    assert_eq!(names(&apply_stage(&r, "3").unwrap()), names(&r));
    assert_eq!(
        apply_stage(&r, "city").unwrap(),
        apply_stage(&r, "3").unwrap()
    );
    for wrong in ["0", "5", "nope"] {
        let e = apply_stage(&r, wrong).unwrap_err().to_string();
        assert!(e.contains("street, car, city, look"), "{e}");
    }
}

#[test]
fn only_keeps_objects_and_pieces_and_a_child_goes_with_its_parent() {
    let r = street();
    let pick = |only: &[&str]| {
        Pick {
            stage: None,
            only: only.iter().map(|s| s.to_string()).collect(),
        }
        .apply(&r)
    };
    let badge = pick(&["hero_badge"]).unwrap();
    assert!(badge.objects.is_empty());
    let road = pick(&["road.asphalt"]).unwrap();
    assert_eq!(names(&road), ["road"]);
    assert!(road.keeps("road", "road.asphalt"));
    assert!(!road.keeps("road", "road.markings"));
    assert!(street().keeps("road", "road.markings"));
    assert!(pick(&["nothing*"]).is_err());
}

#[test]
fn stages_are_validated_and_set_reaches_them() {
    let twice = STREET.replace("name = \"city\"", "name = \"car\"");
    let problems: Vec<String> = validate(&parse(&twice).unwrap())
        .iter()
        .map(Problem::to_string)
        .collect();
    assert!(
        problems.iter().any(|p| p.contains("stage[car].name")),
        "{problems:?}"
    );
    let stray = STREET.replace("objects = [\"traffic*\"]", "objects = [\"bus*\"]");
    let problems: Vec<String> = validate(&parse(&stray).unwrap())
        .iter()
        .map(Problem::to_string)
        .collect();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("stage[city].objects") && p.contains("`bus*` matches no object")),
        "{problems:?}"
    );
    let set = apply_set(
        &street(),
        &["stage.street.objects=[\"road\", \"traffic1\"]".into()],
    )
    .unwrap();
    assert_eq!(
        names(&apply_stage(&set, "street").unwrap()),
        ["road", "traffic1"]
    );
}
