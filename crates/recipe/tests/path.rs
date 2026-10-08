use manfred_recipe::{
    CameraPath, Ease, Orbit, PathKey, PathKind, Problem, Recipe, apply_set, parse, validate,
};

const HEAD: &str = r#"
manfred = 1
name = "paths"

[[object]]
name = "box"
kind = "box"
size = [1.0, 1.0, 1.0]
material = "paint"

[[camera]]
name = "main"
position = [0.0, 1.0, 2.0]
target = [0.0, 1.0, 0.0]
focal_mm = 50.0
sensor_mm = 36.0
"#;

fn recipe(path: &str) -> Recipe {
    let text = format!("{HEAD}{path}");
    parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"))
}

fn problems(path: &str) -> Vec<String> {
    validate(&recipe(path))
        .iter()
        .map(Problem::to_string)
        .collect()
}

fn refused(path: &str) -> String {
    parse(&format!("{HEAD}{path}")).unwrap_err().to_string()
}

#[test]
fn an_orbit_reads_with_its_defaults() {
    let r = recipe(
        r#"
[camera.path]
kind = "orbit"
target = [0.0, 1.0, 0.0]
turns = 1
"#,
    );
    assert_eq!(
        r.cameras[0].path,
        Some(CameraPath {
            kind: PathKind::Orbit(Orbit {
                target: [0.0, 1.0, 0.0],
                turns: 1.0,
                rise_deg: 0.0,
                radius_scale: 1.0,
            }),
            ease: Ease::Linear,
        })
    );
    assert_eq!(validate(&r), Vec::new());
    let full = recipe(
        r#"
[camera.path]
kind = "orbit"
target = [0.0, 0.5, 0.0]
turns = -0.5
rise_deg = 20.0
radius_scale = 1.5
ease = "smooth"
"#,
    );
    let Some(CameraPath {
        kind: PathKind::Orbit(orbit),
        ease,
    }) = &full.cameras[0].path
    else {
        panic!("an orbit");
    };
    assert_eq!(*ease, Ease::Smooth);
    assert_eq!(orbit.turns, -0.5);
    assert_eq!(orbit.rise_deg, 20.0);
    assert_eq!(orbit.radius_scale, 1.5);
    assert_eq!(validate(&full), Vec::new());
}

#[test]
fn keys_read_as_an_array_of_tables() {
    let r = recipe(
        r#"
[camera.path]
kind = "keys"
ease = "smooth"

[[camera.path.keys]]
t = 0.5
position = [1.0, 1.0, 2.0]
target = [0.0, 1.0, 0.0]
focal_mm = 35.0

[[camera.path.keys]]
t = 1.0
position = [2.0, 1.5, 1.0]
target = [0.0, 0.5, 0.0]
"#,
    );
    assert_eq!(
        r.cameras[0].path,
        Some(CameraPath {
            kind: PathKind::Keys(vec![
                PathKey {
                    t: 0.5,
                    position: [1.0, 1.0, 2.0],
                    target: [0.0, 1.0, 0.0],
                    focal_mm: Some(35.0),
                },
                PathKey {
                    t: 1.0,
                    position: [2.0, 1.5, 1.0],
                    target: [0.0, 0.5, 0.0],
                    focal_mm: None,
                },
            ]),
            ease: Ease::Smooth,
        })
    );
    assert_eq!(validate(&r), Vec::new());
}

#[test]
fn a_path_after_other_tables_still_belongs_to_the_last_camera() {
    let r = recipe(
        r#"
[sun]
model = "authored"
toward = [0.3, 0.8, 0.5]
irradiance = 2.0

[camera.path]
kind = "orbit"
target = [0.0, 1.0, 0.0]
turns = 1
"#,
    );
    assert!(r.cameras[0].path.is_some());
}

#[test]
fn bad_keys_are_refused_while_reading() {
    for (path, expected) in [
        ("[camera.path]\ntarget = [0.0, 0.0, 0.0]\n", "kind"),
        (
            "[camera.path]\nkind = \"spiral\"\n",
            "expected \"orbit\" or \"keys\"",
        ),
        (
            "[camera.path]\nkind = \"orbit\"\nturns = 1\n",
            "camera[main].path.target",
        ),
        (
            "[camera.path]\nkind = \"orbit\"\ntarget = [0.0, 0.0, 0.0]\n",
            "camera[main].path.turns",
        ),
        (
            "[camera.path]\nkind = \"orbit\"\ntarget = [0.0, 0.0, 0.0]\nturns = 1\nspeed = 2\n",
            "unknown key `speed`",
        ),
        (
            "[camera.path]\nkind = \"orbit\"\ntarget = [0.0, 0.0, 0.0]\nturns = 1\nease = \"bouncy\"\n",
            "expected \"linear\" or \"smooth\"",
        ),
        (
            "[camera.path]\nkind = \"orbit\"\ntarget = [0.0, 0.0, 0.0]\nturns = 1\nkeys = []\n",
            "unknown key `keys`",
        ),
        ("[camera.path]\nkind = \"keys\"\n", "camera[main].path.keys"),
        (
            "[camera.path]\nkind = \"keys\"\nturns = 1\n[[camera.path.keys]]\nt = 1.0\nposition = [1.0, 1.0, 1.0]\ntarget = [0.0, 0.0, 0.0]\n",
            "unknown key `turns`",
        ),
        (
            "[camera.path]\nkind = \"keys\"\n[[camera.path.keys]]\nt = 1.0\nposition = [1.0, 1.0, 1.0]\n",
            "camera[main].path.keys[0].target",
        ),
        (
            "[camera.path]\nkind = \"keys\"\n[[camera.path.keys]]\nt = 1.0\nposition = [1.0, 1.0, 1.0]\ntarget = [0.0, 0.0, 0.0]\nroll = 3\n",
            "unknown key `roll`",
        ),
        ("path = 3\n", "camera[main].path"),
        (
            "[camera.path]\nkind = \"orbit\"\ntarget = [0.0, 0.0, 0.0]\nturns = nan\n",
            "camera[main].path.turns",
        ),
    ] {
        let message = refused(path);
        assert!(message.contains(expected), "{path}\n{message}");
    }
}

#[test]
fn bad_values_are_problems() {
    for (path, expected) in [
        (
            "[camera.path]\nkind = \"orbit\"\ntarget = [0.0, 0.0, 0.0]\nturns = 1\nradius_scale = 0\n",
            "camera[main].path.radius_scale: expected a number above 0",
        ),
        (
            "[camera.path]\nkind = \"orbit\"\ntarget = [0.0, 0.0, 0.0]\nturns = 1\nrise_deg = 70\n",
            "camera[main].path.rise_deg: the camera starts 26.6° above the target and would end 96.6° above it",
        ),
        (
            "[camera.path]\nkind = \"orbit\"\ntarget = [0.0, -3.0, 2.0]\nturns = 1\n",
            "camera[main].path.target: the camera is straight above or below the orbit's target",
        ),
        (
            "[camera.path]\nkind = \"keys\"\nkeys = []\n",
            "camera[main].path.keys: expected at least one key",
        ),
        (
            "[camera.path]\nkind = \"keys\"\n[[camera.path.keys]]\nt = 1.5\nposition = [1.0, 1.0, 1.0]\ntarget = [0.0, 0.0, 0.0]\n",
            "camera[main].path.keys[0].t: expected a time from 0 to 1",
        ),
        (
            "[camera.path]\nkind = \"keys\"\n[[camera.path.keys]]\nt = 0.5\nposition = [1.0, 1.0, 1.0]\ntarget = [0.0, 0.0, 0.0]\n[[camera.path.keys]]\nt = 0.5\nposition = [2.0, 1.0, 1.0]\ntarget = [0.0, 0.0, 0.0]\n",
            "camera[main].path.keys[1].t: expected a time after the key before it",
        ),
        (
            "[camera.path]\nkind = \"keys\"\n[[camera.path.keys]]\nt = 1.0\nposition = [1.0, 1.0, 1.0]\ntarget = [1.0, 1.0, 1.0]\n",
            "camera[main].path.keys[0].target: the target is the key's own position",
        ),
        (
            "[camera.path]\nkind = \"keys\"\n[[camera.path.keys]]\nt = 1.0\nposition = [1.0, 1.0, 1.0]\ntarget = [0.0, 0.0, 0.0]\nfocal_mm = 0\n",
            "camera[main].path.keys[0].focal_mm: expected millimetres above 0",
        ),
    ] {
        let found = problems(path);
        assert!(
            found.iter().any(|p| p.contains(expected)),
            "{path}\n{found:?}"
        );
    }
}

#[test]
fn a_keys_focal_needs_the_cameras_own_focal() {
    let text = HEAD.replace("focal_mm = 50.0\nsensor_mm = 36.0\n", "fov_y_deg = 40.0\n");
    let r = parse(&format!(
        "{text}[camera.path]\nkind = \"keys\"\n[[camera.path.keys]]\nt = 1.0\nposition = [1.0, 1.0, 1.0]\ntarget = [0.0, 0.0, 0.0]\nfocal_mm = 35\n"
    ))
    .unwrap();
    let found: Vec<String> = validate(&r).iter().map(Problem::to_string).collect();
    assert!(
        found.iter().any(
            |p| p.contains("a key's focal_mm needs a perspective camera with its own focal_mm")
        ),
        "{found:?}"
    );
}

#[test]
fn problems_carry_the_line_of_the_key() {
    let r = recipe(
        "[camera.path]\nkind = \"keys\"\n[[camera.path.keys]]\nt = 0.5\nposition = [1.0, 1.0, 1.0]\ntarget = [0.0, 0.0, 0.0]\n[[camera.path.keys]]\nt = 0.25\nposition = [2.0, 1.0, 1.0]\ntarget = [0.0, 0.0, 0.0]\n",
    );
    let found = validate(&r);
    let line = HEAD.lines().count() + 8;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].line, Some(line), "{found:?}");
}

#[test]
fn set_reaches_into_a_path() {
    let r = recipe("[camera.path]\nkind = \"orbit\"\ntarget = [0.0, 1.0, 0.0]\nturns = 1\n");
    let turned = apply_set(&r, &["camera.main.path.turns=2".into()]).unwrap();
    let Some(CameraPath {
        kind: PathKind::Orbit(orbit),
        ..
    }) = &turned.cameras[0].path
    else {
        panic!("an orbit");
    };
    assert_eq!(orbit.turns, 2.0);
}

#[test]
fn the_camera_at_a_time_follows_its_path() {
    let r = recipe("[camera.path]\nkind = \"orbit\"\ntarget = [0.0, 1.0, 0.0]\nturns = 1\n");
    let camera = &r.cameras[0];
    let path = camera.path.as_ref().unwrap();
    let first = camera.at(path.time(0, 24));
    assert_eq!(first.position, camera.position);
    assert_eq!(first.target, camera.target);
    let quarter = camera.at(path.time(6, 24)).position;
    for (got, want) in quarter.iter().zip([2.0, 1.0, 0.0]) {
        assert!((got - want).abs() < 1e-12, "{quarter:?}");
    }
    let last = camera.at(path.time(23, 24)).position;
    let angle = std::f64::consts::TAU * 23.0 / 24.0;
    for (got, want) in last.iter().zip([2.0 * angle.sin(), 1.0, 2.0 * angle.cos()]) {
        assert!((got - want).abs() < 1e-12, "{last:?}");
    }
}
