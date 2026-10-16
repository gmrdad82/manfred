use manfred_recipe::{
    Problem, Recipe, Rig, RigFocus, RigShake, RigTracks, Spring, apply_set, parse, validate,
};

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
            yaw_lag: 0.0,
            bank: 0.0,
            shake: Some(RigShake {
                translation: 0.03,
                roll: 0.4,
                frequency: 2.5,
                seed: Some(9),
            }),
            focus: Some(RigFocus::Follow),
            tracks: RigTracks::default(),
            bank_spring: None,
            release: None,
            release_ease: 1.5,
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

#[test]
fn a_rig_reads_its_tracks_spring_and_release_and_set_reaches_them() {
    let r = recipe(
        "\n[camera.rig]\nfollow = \"hero\"\nbank_spring = { frequency = 1.4, damping = 0.35 }\nrelease = 8.2\nrelease_ease = 1.2\n\n[camera.rig.tracks]\ndistance = [[0.0, 4.2], [0.6, 8.0]]\nlateral = [[0.0, -3.0], [5.0, 3.0]]\nshake = [[0.0, 1.0]]\n",
    );
    let rig = r.cameras[0].rig.as_ref().unwrap();
    assert_eq!(
        rig.tracks,
        RigTracks {
            distance: Some(vec![[0.0, 4.2], [0.6, 8.0]]),
            lateral: Some(vec![[0.0, -3.0], [5.0, 3.0]]),
            shake: Some(vec![[0.0, 1.0]]),
            ..RigTracks::default()
        }
    );
    assert_eq!(
        rig.bank_spring,
        Some(Spring {
            frequency: 1.4,
            damping: 0.35
        })
    );
    assert_eq!((rig.release, rig.release_ease), (Some(8.2), 1.2));
    assert!(validate(&r).is_empty(), "{:?}", validate(&r));
    let set = apply_set(
        &r,
        &["camera.chase.rig.tracks.distance=[[0, 3], [1, 15]]".into()],
    )
    .unwrap();
    assert_eq!(
        set.cameras[0].rig.as_ref().unwrap().tracks.distance,
        Some(vec![[0.0, 3.0], [1.0, 15.0]])
    );
    let found = problems(
        "\n[camera.rig]\nfollow = \"hero\"\nrelease_ease = 0\nbank_spring = { frequency = 0 }\n\n[camera.rig.tracks]\ndistance = [[1.0, 4.0], [0.5, 8.0]]\nlag = [[0.0, -1.0]]\n",
    );
    for want in [
        "camera[chase].rig.tracks.distance",
        "camera[chase].rig.tracks.lag",
        "camera[chase].rig.bank_spring.frequency",
        "camera[chase].rig.release_ease",
    ] {
        assert!(found.iter().any(|p| p.contains(want)), "{want}: {found:?}");
    }
}
