use glam::{DQuat, DVec3};
use manfred_build::{Built, Track, build};
use manfred_recipe::{Recipe, parse};
use std::path::Path;

const HEAD: &str = r#"
manfred = 1
name = "chase"
seed = 4

[timeline]
fps = 30
frames = 45

[[route]]
name = "main"
points = [[0.0, 0.0, 0.0], [0.0, 0.0, 30.0], [-12.0, 0.0, 55.0], [-30.0, 0.0, 70.0]]

[[object]]
name = "hero"
kind = "generator"
generator = "manfred/car"
params = { style = "sports", seed = 1 }
"#;

const SUGAR: &str = r#"
[[follow]]
object = "hero"
route = "main"
lane = 1.5
start = 2.0
speed = [[0.0, 14.0], [1.5, 24.0]]
drift = { amount = 0.6, period = 1.5, slip_deg = 12.0 }

[[camera]]
name = "chase"
fov_y_deg = 60.0
fstop = 2.8

[camera.rig]
follow = "hero"
lag = 0.25
look_ahead = 0.2
yaw_lag = 0.3
bank = 4.0
shake = { translation = 0.02, roll = 0.4, frequency = 6.0, seed = 2 }
focus = "follow"
bank_spring = { frequency = 1.2, damping = 0.3 }
release = 1.2

[camera.rig.tracks]
distance = [[0.0, 4.0], [1.0, 9.0]]
lateral = [[0.0, 0.0], [1.0, -2.0]]
"#;

const EXPLICIT: &str = r#"
[[object.constraint]]
type = "follow_path"
route = "main"
lane = 1.5
start = 2.0
speed = [[0.0, 14.0], [1.5, 24.0]]
drift = { amount = 0.6, period = 1.5, slip_deg = 12.0 }

[[camera]]
name = "chase"
fov_y_deg = 60.0
fstop = 2.8

[[camera.constraint]]
type = "rig"
follow = "hero"
lag = 0.25
look_ahead = 0.2
yaw_lag = 0.3
bank = 4.0
shake = { translation = 0.02, roll = 0.4, frequency = 6.0, seed = 2 }
focus = "follow"
bank_spring = { frequency = 1.2, damping = 0.3 }
release = 1.2

[camera.constraint.tracks]
distance = [[0.0, 4.0], [1.0, 9.0]]
lateral = [[0.0, 0.0], [1.0, -2.0]]
"#;

fn recipe(tail: &str) -> Recipe {
    let text = format!("{HEAD}{tail}");
    parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"))
}

fn built(r: &Recipe) -> Built {
    build(r, Path::new(".")).unwrap_or_else(|e| panic!("{e}"))
}

fn track<'a>(built: &'a Built, node: &str) -> &'a Track {
    built
        .tracks
        .iter()
        .find(|t| t.node == node)
        .unwrap_or_else(|| panic!("no track of `{node}`"))
}

fn at(track: &Track, n: usize) -> DVec3 {
    DVec3::from(track.keys[n].translation)
}

fn turn(track: &Track, n: usize) -> DQuat {
    DQuat::from_array(track.keys[n].rotation)
}

#[test]
fn the_follow_and_rig_sugar_builds_the_bytes_of_the_constraint_form() {
    let sugar = built(&recipe(SUGAR));
    let explicit = built(&recipe(EXPLICIT));
    assert!(sugar.tracks.len() > 4);
    assert!(sugar.tracks.iter().any(|t| t.node == "hero.wheel_fl"));
    assert!(track(&sugar, "camera.chase").focus.is_some());
    assert_eq!(sugar.tracks, explicit.tracks);
    assert_eq!(sugar.glb(), explicit.glb());
}

const AIMED: &str = r#"
[[follow]]
object = "hero"
route = "main"
speed = [[0.0, 20.0]]

[[object]]
name = "post"
kind = "box"
size = [0.2, 3.0, 0.2]
at = [6.0, 0.0, 20.0]

[[object.constraint]]
type = "look_at"
target = "hero"
offset = [0.0, 1.0, 0.0]

[[object]]
name = "drone"
kind = "box"
size = [0.4, 0.1, 0.4]

[[object.constraint]]
type = "copy"
target = "hero"
offset = [0.5, 2.5, -1.0]
rotate = [0.0, 90.0, 0.0]

[[camera]]
name = "eye"
position = [-8.0, 3.0, 10.0]
target = [0.0, 0.0, 0.0]
fov_y_deg = 50.0

[[camera.constraint]]
type = "look_at"
target = "drone"

[[light]]
name = "search"
kind = "spot"
position = [4.0, 8.0, 15.0]
intensity = 30.0

[[light.constraint]]
type = "look_at"
target = "hero"
"#;

#[test]
fn look_at_aims_its_axis_at_the_target_and_copy_rides_it_with_an_offset() {
    let r = recipe(AIMED);
    let built = built(&r);
    let hero = track(&built, "hero.body");
    let post = track(&built, "post");
    let drone = track(&built, "drone");
    let eye = track(&built, "camera.eye");
    let search = track(&built, "light.search");
    let quarter = DQuat::from_rotation_y(90f64.to_radians());
    for n in [0, 20, 45] {
        let car = at(hero, n);
        let car_turn = turn(hero, n);
        assert!(at(post, n).distance(DVec3::new(6.0, 0.0, 20.0)) < 1e-9);
        let toward = (car + car_turn * DVec3::Y - at(post, n)).normalize();
        assert!((turn(post, n) * DVec3::Z).dot(toward) > 1.0 - 1e-9, "{n}");
        let ride = car + car_turn * DVec3::new(0.5, 2.5, -1.0);
        assert!(at(drone, n).distance(ride) < 1e-6, "{n}");
        assert!(
            turn(drone, n).angle_between(car_turn * quarter) < 1e-6,
            "{n}"
        );
        let sight = (at(drone, n) - at(eye, n)).normalize();
        assert!((turn(eye, n) * DVec3::NEG_Z).dot(sight) > 1.0 - 1e-9, "{n}");
        assert!(at(eye, n).distance(DVec3::new(-8.0, 3.0, 10.0)) < 1e-9);
        let beam = (car - at(search, n)).normalize();
        assert!(
            (turn(search, n) * DVec3::NEG_Z).dot(beam) > 1.0 - 1e-9,
            "{n}"
        );
    }
    assert_ne!(turn(post, 0), turn(post, 45));
    let (doc, _, _) = gltf::import_slice(built.glb()).unwrap();
    let light = doc
        .nodes()
        .find(|n| n.name() == Some("light.search"))
        .expect("the light's node");
    assert!(light.mesh().is_none() && light.camera().is_none());
}
