use glam::DMat4;
use manfred_glb::{Animation, Content, Lens, Node, Order, Track, Trs, write_animated, write_tree};
use manfred_mesh::Mesh;

fn tri() -> Mesh {
    Mesh {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        uvs: None,
        uvs1: None,
        indices: vec![0, 1, 2],
        slots: Vec::new(),
    }
}

fn node(name: &str) -> Node {
    Node {
        name: name.to_string(),
        mesh: tri(),
        transform: DMat4::from_translation(glam::DVec3::new(9.0, 9.0, 9.0)),
        keep_transform: true,
        targets: None,
    }
}

fn key(x: f64, turn: f64) -> Trs {
    let (s, c) = (turn / 2.0).sin_cos();
    Trs {
        translation: [x, 0.5, 0.0],
        rotation: [0.0, s, 0.0, c],
        scale: [1.0; 3],
    }
}

#[test]
fn a_track_writes_the_timeline_animation_and_a_camera_track_its_camera_node() {
    let nodes = vec![node("hero.body"), node("still")];
    let tracks = vec![
        Track {
            node: "hero.body".into(),
            keys: vec![key(0.0, 0.0), key(1.0, 0.5), key(2.0, 1.0)],
            camera: None,
            focus: None,
        },
        Track {
            node: "camera.chase".into(),
            keys: vec![key(0.0, 0.0); 3],
            camera: Some(Lens {
                yfov: 0.8,
                aspect: 0.5625,
                znear: 0.05,
            }),
            focus: None,
        },
    ];
    let bytes = write_animated(
        &nodes,
        &[],
        &[],
        Content::default(),
        Order::File,
        "manfred test",
        Some(Animation {
            fps: 2,
            tracks: &tracks,
        }),
    )
    .unwrap();
    let (doc, buffers, _) = gltf::import_slice(&bytes).unwrap();
    let names: Vec<&str> = doc.nodes().filter_map(|n| n.name()).collect();
    assert_eq!(names, ["hero.body", "still", "camera.chase"]);
    let roots: Vec<usize> = doc
        .default_scene()
        .unwrap()
        .nodes()
        .map(|n| n.index())
        .collect();
    assert_eq!(roots, [0, 1, 2]);

    let body = doc.nodes().next().unwrap();
    let gltf::scene::Transform::Decomposed { translation, .. } = body.transform() else {
        panic!("an animated node carries translation, rotation and scale");
    };
    assert_eq!(translation, [0.0, 0.5, 0.0]);
    let still = doc.nodes().nth(1).unwrap();
    assert!(matches!(
        still.transform(),
        gltf::scene::Transform::Matrix { .. }
    ));

    let camera = doc.nodes().nth(2).unwrap();
    assert!(camera.mesh().is_none());
    let lens = camera.camera().unwrap();
    let gltf::camera::Projection::Perspective(lens) = lens.projection() else {
        panic!("a perspective camera");
    };
    assert_eq!(lens.yfov(), 0.8);
    assert_eq!(lens.aspect_ratio(), Some(0.5625));
    assert_eq!(lens.znear(), 0.05);
    assert_eq!(lens.zfar(), None);

    let animations: Vec<_> = doc.animations().collect();
    assert_eq!(animations.len(), 1);
    let animation = &animations[0];
    assert_eq!(animation.name(), Some("timeline"));
    let channels: Vec<_> = animation.channels().collect();
    let paths: Vec<(usize, gltf::animation::Property)> = channels
        .iter()
        .map(|c| (c.target().node().index(), c.target().property()))
        .collect();
    assert_eq!(
        paths,
        [
            (0, gltf::animation::Property::Translation),
            (0, gltf::animation::Property::Rotation),
        ]
    );
    for channel in &channels {
        assert_eq!(
            channel.sampler().interpolation(),
            gltf::animation::Interpolation::Linear
        );
        let input = channel.sampler().input();
        assert_eq!(input.min(), Some(serde_json::json!([0.0])));
        assert_eq!(input.max(), Some(serde_json::json!([1.0])));
        let reader = channel.reader(|b| Some(&buffers[b.index()]));
        let times: Vec<f32> = reader.read_inputs().unwrap().collect();
        assert_eq!(times, [0.0, 0.5, 1.0]);
        match reader.read_outputs().unwrap() {
            gltf::animation::util::ReadOutputs::Translations(out) => {
                let out: Vec<[f32; 3]> = out.collect();
                assert_eq!(out, [[0.0, 0.5, 0.0], [1.0, 0.5, 0.0], [2.0, 0.5, 0.0]]);
            }
            gltf::animation::util::ReadOutputs::Rotations(out) => {
                let out: Vec<[f32; 4]> = out.into_f32().collect();
                assert_eq!(out.len(), 3);
                assert_eq!(out[0], [0.0, 0.0, 0.0, 1.0]);
            }
            _ => panic!("only translation and rotation move"),
        }
    }
}

#[test]
fn no_tracks_write_the_same_bytes_as_before() {
    let nodes = vec![node("a"), node("b")];
    let plain = write_tree(&nodes, &[], &[], &[], Order::File, "manfred test").unwrap();
    let none = write_animated(
        &nodes,
        &[],
        &[],
        Content::default(),
        Order::File,
        "manfred test",
        None,
    )
    .unwrap();
    let empty = write_animated(
        &nodes,
        &[],
        &[],
        Content::default(),
        Order::File,
        "manfred test",
        Some(Animation {
            fps: 60,
            tracks: &[],
        }),
    )
    .unwrap();
    assert_eq!(plain, none);
    assert_eq!(plain, empty);
}

#[test]
fn a_track_naming_no_node_is_refused() {
    let tracks = vec![Track {
        node: "ghost".into(),
        keys: vec![key(0.0, 0.0), key(1.0, 0.0)],
        camera: None,
        focus: None,
    }];
    let error = write_animated(
        &[node("a")],
        &[],
        &[],
        Content::default(),
        Order::File,
        "manfred test",
        Some(Animation {
            fps: 60,
            tracks: &tracks,
        }),
    )
    .unwrap_err();
    assert!(error.0.contains("ghost"), "{error}");
}
