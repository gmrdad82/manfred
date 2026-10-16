use manfred_recipe::{Problem, Recipe, Rig, RigFocus, RigShake, apply_set, parse, validate};

const HEAD: &str = r#"
manfred = 1
name = "chase"

[[object]]
name = "hero"
kind = "box"
size = [1.8, 1.2, 4.4]

[[camera]]
name = "chase"
fov_y_deg = 38.0
"#;

fn recipe(tail: &str) -> Recipe {
    let text = format!("{HEAD}{tail}");
    parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"))
}

fn problems(tail: &str) -> Vec<String> {
    validate(&recipe(tail))
        .iter()
        .map(Problem::to_string)
        .collect()
}

#[test]
fn a_rig_reads_every_key_and_stands_in_for_position_and_target() {
    let r = recipe(
        "fstop = 2.8\n\n[camera.rig]\nfollow = \"hero\"\noffset = [0.5, 1.4, -7.0]\nlook = [0.0, 0.5, 1.0]\nlag = 0.25\nlook_ahead = 0.3\nshake = { translation = 0.03, roll = 0.4, frequency = 2.5, seed = 9 }\nfocus = \"follow\"\n",
    );
    let camera = &r.cameras[0];
    assert_eq!(
        camera.rig,
        Some(Rig {
            follow: "hero".into(),
            offset: [0.5, 1.4, -7.0],
            look: [0.0, 0.5, 1.0],
            lag: 0.25,
            look_ahead: 0.3,
            shake: Some(RigShake {
                translation: 0.03,
                roll: 0.4,
                frequency: 2.5,
                seed: Some(9),
            }),
            focus: Some(RigFocus::Follow),
        })
    );
    assert_eq!(camera.position, [0.5, 1.4, -7.0]);
    assert_eq!(camera.target, [0.0, 0.5, 1.0]);
    let found: Vec<String> = validate(&r).iter().map(Problem::to_string).collect();
    assert!(!found.iter().any(|p| p.contains("camera")), "{found:?}");
    let set = apply_set(
        &r,
        &[
            "camera.chase.rig.lag=0.5".into(),
            "camera.chase.rig.shake.seed=3".into(),
        ],
    )
    .unwrap();
    let rig = set.cameras[0].rig.as_ref().unwrap();
    assert_eq!(rig.lag, 0.5);
    assert_eq!(rig.shake.unwrap().seed, Some(3));
}

#[test]
fn a_rig_refuses_what_it_cannot_shoot() {
    let found = problems(
        "\n[camera.rig]\nfollow = \"ghost\"\nlag = -1\nshake = { frequency = 0 }\nfocus = \"follow\"\n",
    );
    for want in [
        "camera[chase].rig.follow",
        "camera[chase].rig.lag",
        "camera[chase].rig.shake.frequency",
        "camera[chase].rig.focus",
    ] {
        assert!(found.iter().any(|p| p.contains(want)), "{want}: {found:?}");
    }
    let bad = parse(&format!(
        "{HEAD}\n[camera.rig]\nfollow = \"hero\"\nfocus = \"near\"\n"
    ))
    .unwrap_err()
    .to_string();
    assert!(bad.contains("\"follow\" or a distance"), "{bad}");
}
