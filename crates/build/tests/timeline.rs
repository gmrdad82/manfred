use glam::{DMat4, DVec3};
use manfred_build::{Bake, Registry, Track, Trs, build_with, camera_track, pose, trs_from_matrix};
use manfred_recipe::{Recipe, apply_set, parse};
use std::path::Path;

const SCENE: &str = "manfred = 1\nname = \"t\"\nseed = 5\n[[object]]\nname = \"car\"\nkind = \"box\"\nsize = [2.0, 1.0, 4.0]\nat = [3.0, 0.5, 0.0]\n[[object]]\nname = \"kerb\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\nat = [0.0, 0.0, 5.0]\n[[camera]]\nname = \"chase\"\nposition = [0.0, 2.0, -6.0]\ntarget = [0.0, 1.0, 0.0]\nfov_y_deg = 50.0\n";

fn recipe(timeline: &str) -> Recipe {
    parse(&format!("{SCENE}{timeline}")).unwrap_or_else(|e| panic!("{e}"))
}

fn animates(_: &Recipe) -> Vec<String> {
    vec!["car".into()]
}

fn drive(baking: &Bake) -> Result<Vec<Track>, String> {
    let start = baking
        .nodes
        .iter()
        .find(|n| n.name == "car")
        .map(|n| n.transform)
        .ok_or("the car is not built")?;
    assert!(baking.tracks.is_empty());
    let frames = baking.timeline.frames();
    let keys: Vec<Trs> = (0..=frames)
        .map(|n| {
            let ahead = DVec3::new(0.0, 0.0, 10.0 * baking.timeline.time(n));
            trs_from_matrix(DMat4::from_translation(ahead) * start)
        })
        .collect();
    let camera = &baking.recipe.cameras[0];
    let lens: Vec<Trs> = (0..=frames)
        .map(|n| Trs {
            translation: [0.0, 2.0, 10.0 * baking.timeline.time(n) - 6.0],
            ..Trs::IDENTITY
        })
        .collect();
    Ok(vec![
        Track {
            node: "car".into(),
            keys,
            camera: None,
            focus: None,
            fov: None,
        },
        camera_track(camera, baking.timeline, lens),
    ])
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.register_baker("test/drive", drive, animates);
    registry
}

#[test]
fn the_timeline_reads_its_keys_and_defaults() {
    let t = recipe("[timeline]\nseconds = 2.0\n").timeline.unwrap();
    assert_eq!(
        (t.fps, t.frames(), t.size, t.seed),
        (60, 120, [1080, 1920], 5)
    );
    assert_eq!(t.time(30), 0.5);
    let t = recipe("[timeline]\nfps = 24\nframes = 10\nsize = [640, 360]\nseed = 9\n")
        .timeline
        .unwrap();
    assert_eq!((t.fps, t.frames(), t.size, t.seed), (24, 10, [640, 360], 9));
    assert!(recipe("").timeline.is_none());
    for (body, wanted) in [
        ("seconds = 1.0\nframes = 3\n", "both give the length"),
        ("fps = 30\n", "missing"),
        ("seconds = 0.0\n", "above 0"),
        ("frames = 0\n", "from 1 up"),
        ("fps = 0\nframes = 3\n", "from 1 to 240"),
        ("frames = 3\nsize = [0, 10]\n", "each side"),
        ("frames = 3\nspeed = 2\n", "unknown key"),
    ] {
        let error = parse(&format!("{SCENE}[timeline]\n{body}"))
            .unwrap_err()
            .to_string();
        assert!(error.contains(wanted), "{body}: {error}");
    }
    let set = apply_set(
        &recipe("[timeline]\nfps = 30\nframes = 10\n"),
        &["timeline.fps=12".into(), "timeline.frames=4".into()],
    )
    .unwrap();
    assert_eq!(set.timeline.map(|t| (t.fps, t.frames)), Some((12, 4)));
}

#[test]
fn a_baker_keys_its_object_in_its_own_frame_and_the_glb_carries_the_timeline() {
    let r = recipe("[timeline]\nfps = 10\nframes = 20\nsize = [900, 1600]\n");
    let built = build_with(&r, Path::new("."), &registry()).unwrap();
    let car = built.parts.iter().find(|p| p.node.name == "car").unwrap();
    assert!(car.node.keep_transform);
    assert_eq!(
        car.node.transform,
        DMat4::from_translation(DVec3::new(3.0, 0.5, 0.0))
    );
    let kerb = built.parts.iter().find(|p| p.node.name == "kerb").unwrap();
    assert!(!kerb.node.keep_transform);
    assert_eq!(built.tracks.len(), 2);
    assert!(built.tracks.iter().all(|t| t.keys.len() == 21));

    let (doc, _, _) = gltf::import_slice(built.glb()).unwrap();
    let names: Vec<&str> = doc.nodes().filter_map(|n| n.name()).collect();
    assert_eq!(names, ["car", "kerb", "camera.chase"]);
    let animation = doc.animations().next().unwrap();
    assert_eq!(animation.name(), Some("timeline"));
    assert_eq!(animation.channels().count(), 2);
    let camera = doc.nodes().nth(2).unwrap().camera().unwrap();
    let gltf::camera::Projection::Perspective(lens) = camera.projection() else {
        panic!("a perspective camera");
    };
    assert_eq!(lens.yfov(), 50f32.to_radians());
    assert_eq!(lens.aspect_ratio(), Some(0.5625));

    let (posed, at) = pose(&built.recipe(&r), &built, 1.25).unwrap();
    let car = at.parts.iter().find(|p| p.node.name == "car").unwrap();
    let moved = car.node.transform.transform_point3(DVec3::ZERO);
    assert!(
        (moved - DVec3::new(3.0, 0.5, 12.5)).length() < 1e-9,
        "{moved}"
    );
    let chase = &posed.cameras[0];
    assert!((chase.position[2] - 6.5).abs() < 1e-9);
    assert!(chase.target[2] < chase.position[2]);
    assert_eq!(chase.up, [0.0, 1.0, 0.0]);
    for refused in [-0.1, 2.5, f64::NAN] {
        assert!(pose(&r, &built, refused).is_err(), "{refused}");
    }
}

#[test]
fn without_a_timeline_nothing_is_baked_or_kept() {
    let r = recipe("");
    let built = build_with(&r, Path::new("."), &registry()).unwrap();
    assert!(built.tracks.is_empty());
    assert!(built.parts.iter().all(|p| !p.node.keep_transform));
    let plain = build_with(&r, Path::new("."), &Registry::new()).unwrap();
    assert_eq!(built.glb(), plain.glb());
    let error = pose(&r, &built, 0.0).unwrap_err().to_string();
    assert!(error.contains("[timeline]"), "{error}");
}
