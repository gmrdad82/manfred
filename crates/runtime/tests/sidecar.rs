mod common;

use common::{golden, n, scratch, v3, v4};
use manfred_glb::{CameraFacts, SceneFacts, SkyFacts, SunFacts, facts};
use manfred_runtime::{
    Options, Scene, SkyChoice, load, load_dir, load_files, load_with, mover_group,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

fn toybox() -> (Scene, Value) {
    let dir = golden("toybox");
    let json = std::fs::read_to_string(dir.join("toybox.json")).unwrap();
    (
        load_dir(&dir, "toybox").unwrap(),
        serde_json::from_str(&json).unwrap(),
    )
}

fn built(text: &str) -> (Vec<u8>, String) {
    let recipe = manfred_recipe::parse(text).unwrap();
    let built = manfred_build::build(&recipe, Path::new(".")).unwrap();
    (built.glb(), built.sidecar().unwrap())
}

#[test]
fn the_toybox_sidecar_loads_value_for_value() {
    let (scene, meta) = toybox();
    let c = &meta["cameras"][0];
    assert_eq!(c["name"], "main");
    assert_eq!(scene.camera.position, v3(&c["position"]));
    assert_eq!(scene.camera.target, v3(&c["target"]));
    assert_eq!(scene.camera.up, v3(&c["up"]));
    assert_eq!(scene.camera.fov_y, n(&c["fov_y_deg"]));
    assert_eq!(scene.camera.shift, [n(&c["shift"][0]), n(&c["shift"][1])]);
    assert_eq!(scene.camera.shift, [0.0, -0.05]);
    assert_eq!(scene.camera.aperture, 0.0);
    assert_eq!(scene.camera.focus, n(&c["focus_m"]));
    assert!(!scene.camera.orthographic);
    assert_eq!(scene.plate, Some((1600.0, 1000.0)));

    let s = &meta["sun"];
    assert_eq!(scene.sun.toward, v3(&s["toward"]));
    assert_eq!(scene.sun.radius_deg, n(&s["radius_deg"]));
    assert_eq!(scene.sun.color, v3(&s["color"]));
    assert_eq!(scene.sun.irradiance, n(&s["irradiance"]));

    let k = &meta["sky"];
    assert_eq!(scene.sky.balance, None);
    assert_eq!(scene.sky.intensity, n(&k["intensity"]));
    assert_eq!(scene.sky.rotation_deg, n(&k["rotation_deg"]));
    assert_eq!(scene.sky.rotation(), n(&k["rotation_deg"]).to_radians());
    assert_eq!(scene.sky.hdr_file, None);
    assert_eq!(scene.exposure, n(&meta["look"]["exposure"]));

    let g = &meta["look"]["grade"];
    assert_eq!(scene.grade.vignette, n(&g["vignette"]));
    assert_eq!(scene.grade.saturation, n(&g["saturation"]));
    assert_eq!(scene.grade.warmth, n(&g["warmth"]));
    assert_eq!(scene.grade.bloom, n(&g["bloom"]));
    assert_eq!(scene.grade.haze, n(&g["haze"]));

    let m = scene.motion.as_ref().unwrap();
    let w = &meta["motion"];
    assert_eq!(m.cup, v3(&w["cup"]));
    assert_eq!(m.radius, n(&w["radius"]));
    assert_eq!(m.lo, v3(&w["box_lo"]));
    assert_eq!(m.hi, v3(&w["box_hi"]));
    assert_eq!(m.density, n(&w["density"]));
    assert_eq!(
        (m.anisotropy, m.dust, m.mote, m.ambient),
        (0.35, 1.0, 0.0006, 0.05)
    );

    let all = meta["movers"].as_object().unwrap();
    let names: Vec<&str> = scene.movers.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["drawer", "knob", "lid", "roller"]);
    for mover in &scene.movers {
        let one = &all[&mover.name];
        assert_eq!(mover.pivot, v3(&one["pivot"]), "{}", mover.name);
        assert_eq!(mover.axis, v3(&one["axis"]), "{}", mover.name);
        assert_eq!(mover.travel, n(&one["travel"]), "{}", mover.name);
        let clip = one.get("clip").map_or([0.0; 4], v4);
        let clip2 = one.get("clip2").map_or([0.0; 4], v4);
        assert_eq!(mover.clip, clip, "{}", mover.name);
        assert_eq!(mover.clip2, clip2, "{}", mover.name);
    }
}

const ROOM: &str = "[sky]\nkind = \"room\"\nrotation_deg = 30.0\nintensity = 1.5\n[sky.room]\nwidth = 256\nfloor = [0.1, 0.1, 0.12]\nwall = [0.2, 0.2, 0.22]\nceiling = [0.5, 0.5, 0.55]\n[[sky.room.lights]]\nname = \"key\"\naz = -40.0\nel = 20.0\nwidth = 30.0\nheight = 20.0\npower = 6.0\ncolor = [1.0, 0.95, 0.9]\nsoft = 0.05\nslats = 4.0\nopen = 0.6\n[[sky.room.lights]]\nname = \"fill\"\naz = 70.0\npower = 1.5\n";

#[test]
fn a_room_sky_travels_in_the_sidecar_and_loads_lamp_for_lamp() {
    let table = std::fs::read_to_string(golden("table").join("table.toml")).unwrap();
    let cut = table.find("[sky]").unwrap();
    let (glb, json) = built(&format!("{}{ROOM}", &table[..cut]));
    let meta: Value = serde_json::from_str(&json).unwrap();
    let scene = load(&glb, &json).unwrap();
    let room = &meta["sky"]["room"];
    assert_eq!(scene.room.width, 256);
    assert_eq!(scene.room.floor, v3(&room["floor"]));
    assert_eq!(scene.room.wall, v3(&room["wall"]));
    assert_eq!(scene.room.ceiling, v3(&room["ceiling"]));
    let names: Vec<&str> = scene.room.lights.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["key", "fill"]);
    let key = &scene.room.lights[0];
    assert_eq!((key.az, key.el, key.power), (-40.0, 20.0, 6.0));
    assert_eq!((key.slats, key.open), (4.0, 0.6));
    let fill = &scene.room.lights[1];
    assert_eq!((fill.az, fill.el, fill.power), (70.0, 30.0, 1.5));
    assert_eq!((fill.slats, fill.open), (0.0, 1.0));
    assert_eq!((scene.sky.intensity, scene.sky.rotation_deg), (1.5, 30.0));
}

#[test]
fn mover_group_names_the_group_before_the_double_underscore() {
    assert_eq!(mover_group("mv_lid__handle"), Some("lid"));
    assert_eq!(mover_group("mv_tray__card0"), Some("tray"));
    assert_eq!(mover_group("mv_roller"), Some("roller"));
    assert_eq!(mover_group("mv_knob"), Some("knob"));
    assert_eq!(mover_group("mv_"), Some(""));
    assert_eq!(mover_group("pane"), None);
    assert_eq!(mover_group("body"), None);
}

#[test]
fn the_toybox_has_its_four_mover_groups_and_every_mover_part_finds_one() {
    let (scene, _) = toybox();
    let named: BTreeSet<&str> = scene.movers.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(named.len(), 4);
    let parts = [
        "drawer_front",
        "drawer_pull",
        "mv_knob",
        "mv_knob__dot",
        "mv_lid",
        "mv_lid__handle",
        "mv_roller",
    ];
    for (index, object) in scene.objects.iter().enumerate() {
        let found = scene.mover_of(index as u32);
        assert_eq!(
            found.is_some(),
            parts.contains(&object.as_str()),
            "{object}"
        );
    }
    let drawer = scene
        .objects
        .iter()
        .position(|o| o == "drawer_pull")
        .unwrap();
    assert_eq!(scene.mover_of(drawer as u32).unwrap().name, "drawer");
    assert_eq!(scene.mover("lid").unwrap().clip[1], 1.0);
    assert!(scene.mover("nothing").is_none());
}

#[test]
fn the_sky_is_the_room_by_default_and_the_hdr_when_asked() {
    let dir = golden("toybox");
    let glb = std::fs::read(dir.join("toybox.glb")).unwrap();
    let json = std::fs::read_to_string(dir.join("toybox.json")).unwrap();
    assert_eq!(load(&glb, &json).unwrap().sky.choice, SkyChoice::Room);
    for (name, choice) in [
        (None, SkyChoice::Room),
        (Some("room"), SkyChoice::Room),
        (Some("sunny"), SkyChoice::Room),
        (Some("daylight"), SkyChoice::Hdr),
        (Some("hdr"), SkyChoice::Hdr),
        (Some("mix"), SkyChoice::Mix),
    ] {
        let options = Options::with_sky(name, Some("0.5"));
        let scene = load_with(&glb, &json, &options).unwrap();
        assert_eq!(scene.sky.choice, choice, "{name:?}");
        assert_eq!(scene.sky.fill, 0.5);
        assert_eq!(scene.sky.intensity, 0.9);
        assert_eq!(scene.sky.rotation_deg, 0.0);
    }
}

#[test]
fn a_malformed_fill_falls_back_to_a_quarter() {
    for fill in [None, Some("lots"), Some("-1"), Some("NaN")] {
        assert_eq!(Options::with_sky(None, fill).fill, 0.25, "{fill:?}");
    }
    assert_eq!(Options::with_sky(None, Some(" 0.5 ")).fill, 0.5);
    assert_eq!(Options::with_sky(None, Some("0")).fill, 0.0);
    assert_eq!(Options::default().fill, 0.25);
}

#[test]
fn a_sidecar_that_names_an_hdr_reads_it_beside_itself_and_makes_it_the_sky() {
    let dir = scratch("hdr");
    std::fs::copy(golden("table").join("table.glb"), dir.join("lit.glb")).unwrap();
    std::fs::write(
        dir.join("lit.json"),
        r#"{"sky": {"hdr": "sky.hdr", "intensity": 2.0}}"#,
    )
    .unwrap();
    std::fs::write(dir.join("sky.hdr"), b"radiance").unwrap();
    let scene = load_dir(&dir, "lit").unwrap();
    assert_eq!(scene.sky.choice, SkyChoice::Hdr);
    assert_eq!(scene.sky.hdr_file.as_deref(), Some("sky.hdr"));
    assert_eq!(scene.sky.hdr.as_deref(), Some(b"radiance".as_slice()));
    std::fs::remove_file(dir.join("sky.hdr")).unwrap();
    let error = load_dir(&dir, "lit").unwrap_err();
    assert!(error.to_string().contains("sky.hdr"), "{error}");
}

#[test]
fn the_tables_sidecar_loads() {
    let scene = load_dir(&golden("table"), "table").unwrap();
    assert_eq!(scene.camera.position, [0.0, 0.34, 0.62]);
    assert_eq!(scene.camera.target, [0.0, 0.03, -0.02]);
    assert_eq!(scene.camera.fov_y, 32.0);
    assert_eq!(scene.camera.shift, [0.0, 0.0]);
    assert_eq!(scene.camera.focus, 0.7);
    assert_eq!(scene.sun.toward, [-0.45, 0.82, 0.35]);
    assert_eq!(scene.sun.radius_deg, 0.8);
    assert_eq!(scene.sun.color, [1.0, 0.92, 0.82]);
    assert_eq!(scene.sun.irradiance, 2.2);
    assert_eq!(scene.sky.intensity, 0.9);
    assert_eq!(scene.sky.rotation_deg, 0.0);
    assert_eq!(scene.exposure, 1.0);
    assert!(scene.movers.is_empty());
    assert!(scene.motion.is_none());
    assert_eq!(scene.plate, None);
    assert_eq!(scene.materials.len(), 9);
}

#[test]
fn a_lens_in_millimetres_reads_back_as_a_field_of_view_and_an_aperture() {
    let camera = CameraFacts {
        position: [0.1, 0.2, 0.9],
        target: [0.0, 0.1, 0.0],
        up: [0.0, 1.0, 0.0],
        focal_mm: Some(50.0),
        sensor_mm: Some(36.0),
        shift: [0.05, -0.125],
        focus_m: Some(0.8),
        fstop: 4.0,
        ..CameraFacts::default()
    };
    let written = SceneFacts {
        cameras: vec![camera.clone()],
        sun: Some(SunFacts {
            toward: [0.3, 0.8, 0.5],
            radius_deg: 0.8,
            color: [1.0, 0.9, 0.8],
            irradiance: 3.0,
        }),
        sky: Some(SkyFacts {
            intensity: 2.0,
            rotation_deg: 30.0,
            ..SkyFacts::default()
        }),
        plate: Some([1600.0, 1000.0]),
        ..SceneFacts::default()
    };
    let glb = std::fs::read(golden("table").join("table.glb")).unwrap();
    let scene = load(&glb, &facts(&written).unwrap().to_string()).unwrap();
    assert_eq!(scene.camera.position, [0.1, 0.2, 0.9]);
    assert_eq!(scene.camera.focus, 0.8);
    assert_eq!(scene.camera.shift, [0.05, -0.125]);
    assert!((scene.camera.aperture - 0.0125).abs() < 1e-7);
    let tan_y = 36.0 / 100.0 / 1.6_f64;
    let fov = (2.0 * tan_y.atan()).to_degrees() as f32;
    assert!(
        (scene.camera.fov_y - fov).abs() < 1e-4,
        "{}",
        scene.camera.fov_y
    );
    assert_eq!(scene.sun.toward, [0.3, 0.8, 0.5]);
    assert_eq!(scene.sun.radius_deg, 0.8);
    assert_eq!(scene.sun.color, [1.0, 0.9, 0.8]);
    assert_eq!(scene.sun.irradiance, 3.0);
    assert_eq!((scene.sky.intensity, scene.sky.rotation_deg), (2.0, 30.0));

    let unplated = SceneFacts {
        cameras: vec![camera],
        ..SceneFacts::default()
    };
    let text = facts(&unplated).unwrap().to_string();
    let wide = load_with(
        &glb,
        &text,
        &Options {
            aspect: Some(2.0),
            ..Options::default()
        },
    )
    .unwrap();
    let fov = (2.0 * (36.0 / 100.0 / 2.0_f64).atan()).to_degrees() as f32;
    assert!(
        (wide.camera.fov_y - fov).abs() < 1e-4,
        "{}",
        wide.camera.fov_y
    );
}

#[test]
fn a_blank_sidecar_gives_the_defaults() {
    let glb = std::fs::read(golden("table").join("table.glb")).unwrap();
    let scene = load(&glb, "").unwrap();
    assert_eq!(scene.camera.position, [0.0, 0.3, 1.0]);
    assert_eq!(scene.camera.fov_y, 30.0);
    assert_eq!(scene.camera.focus, 1.0);
    assert_eq!(scene.sun.toward, [-0.5, 0.7, 0.5]);
    assert_eq!(scene.sun.radius_deg, 0.27);
    assert_eq!(scene.sun.irradiance, 0.0);
    assert_eq!(scene.exposure, 1.0);
    assert_eq!(scene.grade.haze, 0.15);
    assert_eq!(scene.room.width, 1024);
    assert_eq!(scene.room.floor, [0.16, 0.10, 0.06]);
    assert!(scene.room.lights.is_empty());
    assert!(!scene.tris.is_empty());
    assert!(
        load(&glb, "{")
            .unwrap_err()
            .to_string()
            .starts_with("scene json: ")
    );
}

#[test]
fn a_room_lamp_without_a_spec_takes_the_defaults() {
    let glb = std::fs::read(golden("table").join("table.glb")).unwrap();
    let scene = load(&glb, r#"{"sky": {"room": {"width": 64, "lights": [{}]}}}"#).unwrap();
    let lamp = &scene.room.lights[0];
    assert_eq!(scene.room.width, 64);
    assert_eq!(lamp.name, "");
    assert_eq!((lamp.az, lamp.el, lamp.power), (0.0, 30.0, 4.0));
    assert_eq!(lamp.color, [1.0, 0.9, 0.78]);
    assert_eq!((lamp.width, lamp.height, lamp.soft), (30.0, 20.0, 0.08));
}

#[test]
fn loading_twice_gives_the_same_scene_by_every_door() {
    let dir = golden("toybox");
    let a = load_dir(&dir, "toybox").unwrap();
    let b = load_dir(&dir, "toybox").unwrap();
    let c = load_files(&dir.join("toybox.glb"), &dir.join("toybox.json")).unwrap();
    let glb = std::fs::read(dir.join("toybox.glb")).unwrap();
    let json = std::fs::read_to_string(dir.join("toybox.json")).unwrap();
    let d = load(&glb, &json).unwrap();
    assert_eq!(a, b);
    assert_eq!(a, c);
    assert_eq!(a, d);
}

#[test]
fn a_missing_file_names_its_path() {
    let error = load_dir(&golden("table"), "nothing").unwrap_err();
    assert!(error.to_string().contains("nothing.glb"), "{error}");
}

#[test]
fn a_motion_table_without_a_cup_puts_the_steam_at_its_box_centre() {
    let glb = std::fs::read(golden("toybox").join("toybox.glb")).unwrap();
    let motion = |cup: &str| {
        let sidecar = format!(
            r#"{{"motion": {{{cup}"radius": 0.03, "box_lo": [-0.5, -0.25, 0.0], "box_hi": [0.25, 0.75, 0.5]}}}}"#
        );
        load(&glb, &sidecar).unwrap().motion.unwrap()
    };
    let centred = motion("");
    assert_eq!(centred.cup, [-0.125, 0.25, 0.25]);
    assert_eq!(
        (centred.lo, centred.hi),
        ([-0.5, -0.25, 0.0], [0.25, 0.75, 0.5])
    );
    assert_eq!(centred.radius, 0.03);
    assert_eq!(motion(r#""cup": [0.1, 0.0, -0.2], "#).cup, [0.1, 0.0, -0.2]);
    assert_eq!(motion(r#""cup": [0.1], "#).cup, [-0.125, 0.25, 0.25]);
}
