mod common;

use common::{golden, n, scratch, v3, v4, without_extras};
use manfred_runtime::{
    Camera, Error, Grade, Lamp, Motion, MoverSpec, Options, Room, Scene, Sidecar, Sky, SkyChoice,
    Sun, facts, load, load_dir, load_dir_read, load_dir_read_with, load_dir_with, load_files_read,
    load_files_read_with, load_read, load_read_with, load_with,
};
use serde_json::{Value, json};

fn v2(v: &Value) -> [f32; 2] {
    [n(&v[0]), n(&v[1])]
}

fn need<'a>(json: &'a Value, key: &str) -> Result<&'a Value, Error> {
    json.get(key).ok_or_else(|| Error::wanted(key))
}

fn dialect_of(scene: &Scene) -> Value {
    let camera = &scene.camera;
    let mut eye = json!({
        "at": camera.position,
        "to": camera.target,
        "up": camera.up,
        "fov": camera.fov_y,
        "shift": camera.shift,
        "aperture": camera.aperture,
        "focus": camera.focus,
        "flat": camera.orthographic,
    });
    if let Some(height) = camera.ortho_height {
        eye["span"] = json!(height);
    }
    let mut heavens = json!({
        "gain": scene.sky.intensity,
        "turn": scene.sky.rotation_deg,
    });
    if let Some(tint) = scene.sky.balance {
        heavens["tint"] = json!(tint);
    }
    if let Some(file) = &scene.sky.hdr_file {
        heavens["file"] = json!(file);
    }
    let lamps: Vec<Value> = scene
        .room
        .lights
        .iter()
        .map(|lamp| {
            json!({
                "name": lamp.name,
                "az": lamp.az,
                "el": lamp.el,
                "power": lamp.power,
                "color": lamp.color,
                "width": lamp.width,
                "height": lamp.height,
                "soft": lamp.soft,
                "slats": lamp.slats,
                "open": lamp.open,
            })
        })
        .collect();
    let slides: Vec<Value> = scene
        .movers
        .iter()
        .map(|mover| {
            json!({
                "name": mover.name,
                "pivot": mover.pivot,
                "axis": mover.axis,
                "travel": mover.travel,
                "clip": mover.clip,
                "clip2": mover.clip2,
            })
        })
        .collect();
    let mut root = json!({
        "eye": eye,
        "lamp": {
            "toward": scene.sun.toward,
            "radius": scene.sun.radius_deg,
            "color": scene.sun.color,
            "power": scene.sun.irradiance,
        },
        "heavens": heavens,
        "chamber": {
            "width": scene.room.width,
            "floor": scene.room.floor,
            "wall": scene.room.wall,
            "ceiling": scene.room.ceiling,
            "lamps": lamps,
        },
        "stop": scene.exposure,
        "tone": [
            scene.grade.vignette,
            scene.grade.saturation,
            scene.grade.warmth,
            scene.grade.bloom,
            scene.grade.haze,
        ],
        "slides": slides,
    });
    if let Some((width, height)) = scene.plate {
        root["frame"] = json!([width, height]);
    }
    if let Some(motion) = &scene.motion {
        root["steam"] = json!({
            "cup": motion.cup,
            "radius": motion.radius,
            "lo": motion.lo,
            "hi": motion.hi,
            "density": motion.density,
            "anisotropy": motion.anisotropy,
            "dust": motion.dust,
            "mote": motion.mote,
            "ambient": motion.ambient,
        });
    }
    root
}

fn read_dialect(json: &Value, options: &Options) -> Result<Sidecar, Error> {
    let eye = need(json, "eye")?;
    let lamp = need(json, "lamp")?;
    let heavens = need(json, "heavens")?;
    let chamber = need(json, "chamber")?;
    let tone = need(json, "tone")?;
    let lamps = need(chamber, "lamps")?
        .as_array()
        .unwrap()
        .iter()
        .map(|lamp| Lamp {
            name: lamp["name"].as_str().unwrap_or_default().to_string(),
            az: n(&lamp["az"]),
            el: n(&lamp["el"]),
            power: n(&lamp["power"]),
            color: v3(&lamp["color"]),
            width: n(&lamp["width"]),
            height: n(&lamp["height"]),
            soft: n(&lamp["soft"]),
            slats: n(&lamp["slats"]),
            open: n(&lamp["open"]),
        })
        .collect();
    let slides = json
        .get("slides")
        .map(|value| {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|mover| MoverSpec {
                    name: mover["name"].as_str().unwrap().to_string(),
                    pivot: v3(&mover["pivot"]),
                    axis: v3(&mover["axis"]),
                    travel: n(&mover["travel"]),
                    clip: v4(&mover["clip"]),
                    clip2: v4(&mover["clip2"]),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Sidecar {
        camera: Camera {
            position: v3(&eye["at"]),
            target: v3(&eye["to"]),
            up: v3(&eye["up"]),
            fov_y: n(&eye["fov"]),
            shift: v2(&eye["shift"]),
            aperture: n(&eye["aperture"]),
            focus: n(&eye["focus"]),
            orthographic: eye["flat"].as_bool().unwrap(),
            ortho_height: eye.get("span").map(n),
        },
        sun: Sun {
            toward: v3(&lamp["toward"]),
            radius_deg: n(&lamp["radius"]),
            color: v3(&lamp["color"]),
            irradiance: n(&lamp["power"]),
        },
        sky: Sky {
            choice: options.sky,
            fill: options.fill,
            balance: heavens.get("tint").map(v3),
            intensity: n(&heavens["gain"]),
            rotation_deg: n(&heavens["turn"]),
            hdr_file: heavens
                .get("file")
                .and_then(Value::as_str)
                .map(str::to_string),
            hdr: None,
        },
        room: Room {
            width: n(&chamber["width"]) as u32,
            floor: v3(&chamber["floor"]),
            wall: v3(&chamber["wall"]),
            ceiling: v3(&chamber["ceiling"]),
            lights: lamps,
        },
        exposure: n(need(json, "stop")?),
        grade: Grade {
            vignette: n(&tone[0]),
            saturation: n(&tone[1]),
            warmth: n(&tone[2]),
            bloom: n(&tone[3]),
            haze: n(&tone[4]),
        },
        plate: json.get("frame").map(|frame| (n(&frame[0]), n(&frame[1]))),
        motion: json.get("steam").map(|motion| Motion {
            cup: v3(&motion["cup"]),
            radius: n(&motion["radius"]),
            lo: v3(&motion["lo"]),
            hi: v3(&motion["hi"]),
            density: n(&motion["density"]),
            anisotropy: n(&motion["anisotropy"]),
            dust: n(&motion["dust"]),
            mote: n(&motion["mote"]),
            ambient: n(&motion["ambient"]),
        }),
        movers: slides,
    })
}

fn refuse(_: &Value, _: &Options) -> Result<Sidecar, Error> {
    Err(Error::wanted("lens"))
}

fn pair(name: &str) -> (Vec<u8>, String) {
    let dir = golden(name);
    (
        std::fs::read(dir.join(format!("{name}.glb"))).unwrap(),
        std::fs::read_to_string(dir.join(format!("{name}.json"))).unwrap(),
    )
}

fn same_scene(name: &str) {
    let (glb, json) = pair(name);
    let options = Options::with_sky(Some("mix"), Some("0.4"));
    let facts_scene = load_with(&glb, &json, &options).unwrap();
    assert_eq!(
        facts_scene,
        load_read_with(&glb, &json, &options, facts).unwrap()
    );
    let dialect = dialect_of(&facts_scene).to_string();
    assert_eq!(
        facts_scene,
        load_read_with(&glb, &dialect, &options, read_dialect).unwrap()
    );
    let plain = load(&glb, &json).unwrap();
    assert_eq!(plain, load_read(&glb, &json, facts).unwrap());
    assert_eq!(
        plain,
        load_read(&glb, &dialect_of(&plain).to_string(), read_dialect).unwrap()
    );
}

#[test]
fn the_facts_reader_and_another_dialect_load_the_same_scene() {
    same_scene("toybox");
    same_scene("table");
    let dir = golden("toybox");
    let loaded = load_dir(&dir, "toybox").unwrap();
    assert_eq!(loaded, load_dir_read(&dir, "toybox", facts).unwrap());
    assert_eq!(
        loaded,
        load_files_read(&dir.join("toybox.glb"), &dir.join("toybox.json"), facts).unwrap()
    );
    let options = Options {
        aspect: Some(2.0),
        ..Options::default()
    };
    let table = golden("table");
    assert_eq!(
        load_dir_with(&table, "table", &options).unwrap(),
        load_dir_read_with(&table, "table", &options, facts).unwrap()
    );
}

#[test]
fn a_dialect_reader_supplies_the_movers_when_the_glb_has_no_extras() {
    let (glb, json) = pair("toybox");
    let plain = without_extras(&glb);
    let scene = load(&plain, &json).unwrap();
    assert_eq!(scene.movers.len(), 4);
    let dialect = dialect_of(&scene).to_string();
    assert_eq!(scene, load_read(&plain, &dialect, read_dialect).unwrap());
}

#[test]
fn a_failing_reader_names_the_key_it_wanted() {
    let dir = golden("table");
    let (glb, json) = pair("table");
    let expect = "scene json: wanted `lens`";
    assert_eq!(
        load_read(&glb, &json, refuse).unwrap_err().to_string(),
        expect
    );
    assert_eq!(
        load_read_with(&glb, &json, &Options::default(), refuse)
            .unwrap_err()
            .to_string(),
        expect
    );
    assert_eq!(
        load_files_read(&dir.join("table.glb"), &dir.join("table.json"), refuse)
            .unwrap_err()
            .to_string(),
        expect
    );
    assert_eq!(
        load_files_read_with(
            &dir.join("table.glb"),
            &dir.join("table.json"),
            &Options::default(),
            refuse
        )
        .unwrap_err()
        .to_string(),
        expect
    );
    assert_eq!(
        load_dir_read(&dir, "table", refuse)
            .unwrap_err()
            .to_string(),
        expect
    );
    assert_eq!(
        load_dir_read_with(&dir, "table", &Options::default(), refuse)
            .unwrap_err()
            .to_string(),
        expect
    );
    assert_eq!(
        load_read(&glb, "{}", read_dialect).unwrap_err().to_string(),
        "scene json: wanted `eye`"
    );
}

#[test]
fn a_readers_hdr_file_is_read_beside_the_sidecar() {
    let scene = load_dir(&golden("table"), "table").unwrap();
    let mut dialect = dialect_of(&scene);
    dialect["heavens"]["file"] = json!("day.hdr");
    let dir = scratch("reader-hdr");
    std::fs::copy(golden("table").join("table.glb"), dir.join("lit.glb")).unwrap();
    std::fs::write(dir.join("lit.json"), dialect.to_string()).unwrap();
    std::fs::write(dir.join("day.hdr"), b"radiance").unwrap();
    let loaded = load_dir_read(&dir, "lit", read_dialect).unwrap();
    assert_eq!(loaded.sky.hdr_file.as_deref(), Some("day.hdr"));
    assert_eq!(loaded.sky.hdr.as_deref(), Some(b"radiance".as_slice()));
    assert_eq!(loaded.sky.choice, SkyChoice::Hdr);
    assert_eq!(loaded.camera.position, scene.camera.position);
    std::fs::remove_file(dir.join("day.hdr")).unwrap();
    let error = load_dir_read(&dir, "lit", read_dialect).unwrap_err();
    assert!(error.to_string().contains("day.hdr"), "{error}");
}
