use glam::DVec3;
use manfred_build::{Registry, build_with, pose};
use manfred_recipe::{Recipe, parse, validate};
use std::path::Path;

const SCENE: &str = r#"manfred = 1
name = "t"

[timeline]
fps = 10
frames = 20

[[object]]
name = "car"
kind = "box"
size = [2.0, 1.0, 4.0]
at = [5.0, 5.0, 5.0]
keys = { at = [[0.5, [0.0, 0.0, 0.0]], [1.5, [0.0, 0.0, 10.0]]], rotate = [[0.0, [0.0, 0.0, 0.0]], [1.0, [0.0, 90.0, 0.0]]] }

[[object]]
name = "wing"
kind = "box"
size = [1.0, 0.1, 1.0]
parent = "car"
at = [0.0, 1.0, 0.0]

[[object]]
name = "kerb"
kind = "box"
size = [1.0, 1.0, 1.0]
at = [0.0, 0.0, 5.0]

[[camera]]
name = "eye"
position = [0.0, 2.0, -6.0]
target = [0.0, 1.0, 0.0]
fov_y_deg = 50.0
keys = { position = [[0.0, [0.0, 2.0, -6.0]], [2.0, [3.0, 2.0, 6.0]]], fov_y_deg = [[0.0, 50.0], [2.0, 30.0]] }

[[light]]
name = "lamp"
position = [0.0, 3.0, 0.0]
intensity = 2.0
keys = { intensity = [[0.0, 2.0], [1.0, 8.0]] }
interp = { intensity = "step" }
"#;

fn recipe() -> Recipe {
    parse(SCENE).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn a_keyed_object_and_its_child_are_keyed_in_the_glb_and_held_past_the_ends() {
    let r = recipe();
    assert_eq!(validate(&r), []);
    let built = build_with(&r, Path::new("."), &Registry::new()).unwrap();
    let names: Vec<&str> = built.tracks.iter().map(|t| t.node.as_str()).collect();
    assert_eq!(names, ["car", "wing", "camera.eye"]);
    assert!(built.tracks.iter().all(|t| t.keys.len() == 21));
    let at = |node: &str, frame: usize| {
        let track = built.tracks.iter().find(|t| t.node == node).unwrap();
        DVec3::from(track.keys[frame].translation)
    };
    for (frame, z) in [(0, 0.0), (5, 0.0), (10, 5.0), (15, 10.0), (20, 10.0)] {
        assert!(
            (at("car", frame) - DVec3::new(0.0, 0.0, z)).length() < 1e-9,
            "{frame}"
        );
    }
    assert!((at("wing", 15) - DVec3::new(0.0, 1.0, 10.0)).length() < 1e-9);
    let car = built.parts.iter().find(|p| p.node.name == "car").unwrap();
    assert!(car.node.keep_transform);
    assert!(
        !built
            .parts
            .iter()
            .find(|p| p.node.name == "kerb")
            .unwrap()
            .node
            .keep_transform
    );

    let (doc, _, _) = gltf::import_slice(built.glb()).unwrap();
    let animation = doc.animations().next().unwrap();
    assert_eq!(animation.name(), Some("timeline"));
    let keyed: Vec<&str> = animation
        .channels()
        .filter_map(|c| c.target().node().name())
        .collect();
    assert!(keyed.contains(&"car") && keyed.contains(&"wing") && keyed.contains(&"camera.eye"));

    let (posed, moved) = pose(&built.recipe(&r), &built, 2.0).unwrap();
    let car = moved.parts.iter().find(|p| p.node.name == "car").unwrap();
    let place = car.node.transform.transform_point3(DVec3::ZERO);
    assert!(
        (place - DVec3::new(0.0, 0.0, 10.0)).length() < 1e-9,
        "{place}"
    );
    let eye = &posed.cameras[0];
    assert!((eye.position[2] - 6.0).abs() < 1e-9);
    assert_eq!(eye.fov_y_deg, Some(30.0));
    let (posed, _) = pose(&built.recipe(&r), &built, 0.95).unwrap();
    assert_eq!(posed.lights[0].intensity, 2.0);
    let (posed, _) = pose(&built.recipe(&r), &built, 1.0).unwrap();
    assert_eq!(posed.lights[0].intensity, 8.0);
}

#[test]
fn a_followed_object_takes_no_keys() {
    let text = format!(
        "{SCENE}\n[[route]]\nname = \"main\"\npoints = [[0.0, 0.0, 0.0], [0.0, 0.0, 20.0]]\n\n[[follow]]\nobject = \"car\"\nroute = \"main\"\nspeed = [[0.0, 10.0]]\n"
    );
    let problems: Vec<String> = validate(&parse(&text).unwrap())
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(
        problems.iter().any(|p| p.contains("object[car].keys")),
        "{problems:?}"
    );
}
