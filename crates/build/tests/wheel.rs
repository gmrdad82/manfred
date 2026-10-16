use glam::{DQuat, DVec3};
use manfred_build::{Built, build};
use manfred_recipe::parse;
use std::f64::consts::TAU;
use std::path::Path;

fn built(params: &str) -> Built {
    let text = format!(
        "manfred = 1\nname = \"t\"\nseed = 3\n\n[[object]]\nname = \"w\"\nkind = \"generator\"\ngenerator = \"manfred/wheel\"\nparams = {{ {params} }}\n"
    );
    build(&parse(&text).unwrap(), Path::new(".")).unwrap()
}

fn piece<'a>(b: &'a Built, name: &str) -> &'a manfred_mesh::Mesh {
    &b.parts
        .iter()
        .find(|p| p.node.name == name)
        .unwrap()
        .node
        .mesh
}

#[test]
fn the_tyre_takes_its_outer_radius_and_width_from_the_size_code() {
    for (code, width, aspect, rim) in [
        ("255/40R19", 255.0, 40.0, 19.0),
        ("195/65R15", 195.0, 65.0, 15.0),
    ] {
        let b = built(&format!("tyre = \"{code}\""));
        let names: Vec<&str> = b.parts.iter().map(|p| p.node.name.as_str()).collect();
        assert_eq!(names, ["w", "w.tyre", "w.rim", "w.brake"]);
        let radius = rim * 0.0254 * 0.5 + width / 1000.0 * aspect / 100.0;
        let fact = &b.facts.extra["wheel"];
        assert!(
            (fact["radius"].as_f64().unwrap() - radius).abs() < 1e-12,
            "{code}"
        );
        assert_eq!(fact["width"].as_f64().unwrap(), width / 1000.0);
        assert_eq!(fact["axle"], serde_json::json!([1.0, 0.0, 0.0]));
        assert_eq!(b.facts.extra["brake"]["spin"], false);
        let tyre = piece(&b, "w.tyre");
        let outer = tyre
            .positions
            .iter()
            .map(|p| f64::from(p[1]).hypot(f64::from(p[2])))
            .fold(0.0, f64::max);
        assert!((outer - radius).abs() < 1e-4, "{code}: {outer} vs {radius}");
        let across = tyre
            .positions
            .iter()
            .map(|p| f64::from(p[0]).abs())
            .fold(0.0, f64::max);
        assert!((across - width / 2000.0).abs() < 1e-4, "{code}: {across}");
    }
    assert!(
        build(
            &parse("manfred = 1\nname = \"t\"\n\n[[object]]\nname = \"w\"\nkind = \"generator\"\ngenerator = \"manfred/wheel\"\nparams = { tyre = \"255-40-19\" }\n").unwrap(),
            Path::new(".")
        )
        .is_err()
    );
}

#[test]
fn the_rim_repeats_once_a_spoke_about_the_axle() {
    for (spokes, twin) in [(5, false), (7, true), (10, false)] {
        let b = built(&format!("rim = {{ spokes = {spokes}, twin = {twin} }}"));
        let rim = piece(&b, "w.rim");
        let points: Vec<DVec3> = rim
            .positions
            .iter()
            .map(|p| DVec3::from(p.map(f64::from)))
            .collect();
        let turn = DQuat::from_rotation_x(TAU / f64::from(spokes));
        let half = DQuat::from_rotation_x(TAU / f64::from(spokes) * 0.5);
        let matches = |q: DQuat| {
            points
                .iter()
                .all(|&p| points.iter().any(|&o| (q * p).distance_squared(o) < 1e-10))
        };
        assert!(matches(turn), "{spokes} spokes");
        assert!(!matches(half), "{spokes} spokes");
    }
}

#[test]
fn a_wheel_is_the_same_bytes_for_the_same_params() {
    for params in [
        "",
        "tyre = \"225/45R17\", rim = { spokes = 10, twin = true, concavity = 0.8, metal = \"#202022\" }, brake = { disc = \"drilled\", caliper = \"#f2a900\" }, side = \"right\"",
    ] {
        assert_eq!(built(params).glb(), built(params).glb(), "{params}");
    }
    assert_ne!(
        built("brake = { disc = \"vented\" }").glb(),
        built("brake = { disc = \"drilled\" }").glb()
    );
}

fn followed(points: &str, corner: &str) -> Built {
    let text = format!(
        "manfred = 1\nname = \"t\"\nseed = 3\n\n[timeline]\nfps = 30\nframes = 30\n\n[[route]]\nname = \"main\"\npoints = {points}\n\n[[object]]\nname = \"car\"\nkind = \"box\"\nsize = [1.8, 0.6, 4.0]\nat = [0.0, 0.5, 0.0]\n\n[[object]]\nname = \"car.wheel_{corner}\"\nkind = \"generator\"\ngenerator = \"manfred/wheel\"\nparent = \"car\"\nat = [0.8, -0.16, -1.3]\n\n[[follow]]\nobject = \"car\"\nroute = \"main\"\nspeed = [[0.0, 10.0]]\n"
    );
    build(&parse(&text).unwrap(), Path::new(".")).unwrap()
}

fn rotations(b: &Built, node: &str) -> Vec<DQuat> {
    b.tracks
        .iter()
        .find(|t| t.node == node)
        .unwrap_or_else(|| panic!("no track for {node}"))
        .keys
        .iter()
        .map(|k| DQuat::from_array(k.rotation))
        .collect()
}

#[test]
fn under_a_follow_the_tyre_and_rim_spin_and_the_brake_only_steers() {
    let b = followed("[[0.0, 0.0, 0.0], [0.0, 0.0, 100.0]]", "rl");
    let body = rotations(&b, "car");
    let brake = rotations(&b, "car.wheel_rl.brake");
    let rim = rotations(&b, "car.wheel_rl.rim");
    let tyre = rotations(&b, "car.wheel_rl.tyre");
    for n in 0..body.len() {
        assert!(body[n].angle_between(brake[n]) < 1e-9, "frame {n}");
        assert!(rim[n].angle_between(tyre[n]) < 1e-9, "frame {n}");
    }
    assert!(body[15].angle_between(rim[15]) > 0.1);
    let key = &b
        .tracks
        .iter()
        .find(|t| t.node == "car.wheel_rl.brake")
        .unwrap()
        .keys[0];
    let car = &b.tracks.iter().find(|t| t.node == "car").unwrap().keys[0];
    let offset = DVec3::from(key.translation) - DVec3::from(car.translation);
    assert!(
        (offset - DQuat::from_array(car.rotation) * DVec3::new(0.8, -0.16, -1.3)).length() < 1e-9
    );

    let b = followed(
        "[[0.0, 0.0, 0.0], [0.0, 0.0, 20.0], [15.0, 0.0, 40.0], [30.0, 0.0, 45.0]]",
        "fl",
    );
    let body = rotations(&b, "car");
    let brake = rotations(&b, "car.wheel_fl.brake");
    let steered = (0..body.len()).any(|n| {
        let (axis, angle) = (body[n].inverse() * brake[n]).to_axis_angle();
        angle > 0.01 && axis.y.abs() > 0.999
    });
    assert!(steered);
}
