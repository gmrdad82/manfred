mod common;

use common::{golden, scratch};
use manfred_build::{Error, build, write};
use manfred_recipe::{Recipe, load, parse, validate};
use manfred_scene::{Eye, Lighting};
use pfx_live::scene::{lens, room_haze};
use pfx_live::sky::SkySource;
use pfx_load::scene::{Environment, Projection, Scene, Shadow};
use pfx_post::{Bloom, Pass, Saturation, Tone, Vignette, Warmth};
use std::path::{Path, PathBuf};

fn recipe(set: &str, camera: Option<&str>) -> (Recipe, PathBuf) {
    let folder = golden(set);
    let mut r = load(&folder.join(format!("{set}.toml"))).unwrap().recipe;
    r.output.scene = Some(format!("{set}.scene.toml"));
    r.output.scene_camera = camera.map(str::to_string);
    assert_eq!(validate(&r), []);
    (r, folder)
}

fn emitted(r: &Recipe, folder: &Path, name: &str) -> Result<(PathBuf, Vec<PathBuf>), Error> {
    let out = scratch(name);
    let built = build(r, folder)?;
    let written = write(&built, r, folder, Some(&out))?;
    Ok((out, written))
}

fn opened(set: &str, camera: Option<&str>, name: &str) -> Scene {
    let (r, folder) = recipe(set, camera);
    assert_eq!(r.output.scene_format, 1);
    let (out, written) = emitted(&r, &folder, name).unwrap();
    let file = out.join(format!("{set}.scene.toml"));
    assert!(written.contains(&file), "{written:?}");
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .starts_with("format = 1\n")
    );
    let scene = Scene::open(file).unwrap();
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    scene
}

fn format_0_matches_golden(set: &str, camera: Option<&str>) -> Scene {
    let (mut r, folder) = recipe(set, camera);
    r.output.scene_format = 0;
    assert_eq!(validate(&r), []);
    let (out, written) = emitted(&r, &folder, &format!("scene-{set}-format0")).unwrap();
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scene");
    for name in [format!("{set}.scene.toml"), format!("{set}.materials.toml")] {
        assert!(written.contains(&out.join(&name)), "{name}: {written:?}");
        let made = std::fs::read_to_string(out.join(&name)).unwrap();
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::write(here.join(&name), &made).unwrap();
        }
        let want = std::fs::read_to_string(here.join(&name)).unwrap();
        assert!(
            made == want,
            "{name} differs from its format 0 golden; UPDATE_GOLDEN=1 regenerates it\n{made}"
        );
    }
    Scene::open(out.join(format!("{set}.scene.toml"))).unwrap()
}

#[test]
fn format_0_still_writes_its_goldens_and_the_engine_migrates_them_to_the_format_1_scene() {
    for (set, camera) in [("table", None), ("toybox", Some("high"))] {
        let old = format_0_matches_golden(set, camera);
        assert!(!old.warnings.is_empty(), "{set}");
        let new = opened(set, camera, &format!("scene-{set}-format1"));
        assert_eq!(old.objects, new.objects, "{set}");
        assert_eq!(
            old.meshes.keys().collect::<Vec<_>>(),
            new.meshes.keys().collect::<Vec<_>>(),
            "{set}"
        );
        assert_eq!(old.library, new.library, "{set}");
        assert_eq!(old.sun, new.sun, "{set}");
        assert_eq!(old.sky, new.sky, "{set}");
        assert_eq!(old.camera, new.camera, "{set}");
        assert_eq!(
            format!("{:?}", old.finish.map(|f| f.chain)),
            format!("{:?}", new.finish.map(|f| f.chain)),
            "{set}"
        );
        assert_eq!(old.lights, new.lights, "{set}");
        assert_eq!(old.trace, new.trace, "{set}");
    }
}

#[test]
fn the_table_writes_a_format_1_scene_and_the_engine_opens_it() {
    let scene = opened("table", None, "scene-table");
    let names: Vec<&str> = scene.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "top", "wall", "crate", "dial", "ball", "note", "rod", "tile_a", "tile_b", "lamp",
            "tray"
        ]
    );
    let ids: Vec<u32> = scene.objects.iter().map(|o| o.id).collect();
    assert_eq!(ids, (1..=11).collect::<Vec<u32>>());
    assert_eq!(scene.meshes.len(), 11);
    assert!(scene.library.contains("oak") && scene.library.contains("glow"));
    let sun = scene.sun.unwrap();
    assert_eq!(sun.hour, 16.0);
    assert!((sun.intensity - 2.2).abs() < 1e-6);
    let Some(Environment::Analytic(sky)) = scene.sky.as_ref().map(|s| s.environment.clone()) else {
        panic!("the table's sky is analytic");
    };
    assert!((sky.ambient - 0.9).abs() < 1e-6);
    let camera = scene.camera.clone().unwrap();
    assert_eq!(camera.projection, Projection::Perspective { fov: 32.0 });
    assert_eq!(camera.at, [0.0, 0.34, 0.62]);
    assert!(camera.near > 0.0 && camera.far > camera.near);
    assert_eq!(scene.draws().items.len(), 11);
}

#[test]
fn the_toybox_writes_a_format_1_scene_through_a_named_camera() {
    let scene = opened("toybox", Some("high"), "scene-toybox");
    assert!(scene.object("guide").is_none());
    let shade = scene.object("shade").unwrap();
    assert_eq!(shade.shadow, Shadow::Only);
    let lid = scene
        .objects
        .iter()
        .find(|o| o.name.starts_with("mv_lid"))
        .unwrap();
    assert_eq!(lid.clip.len(), 2);
    assert!(lid.clip[0][1] == 1.0 && lid.clip[1][1] == -1.0);
    let roller = scene.object("mv_roller").unwrap();
    assert_eq!(roller.clip.len(), 1);
    let fov = match scene.camera.unwrap().projection {
        Projection::Perspective { fov } => fov,
        Projection::Orthographic { .. } => panic!("high is a perspective camera"),
    };
    let want = 2.0 * (36.0_f64 / 100.0 / 1.6).atan().to_degrees();
    assert!((f64::from(fov) - want).abs() < 1e-4, "{fov} {want}");
    assert!(scene.finish.is_some());
}

#[test]
fn the_same_recipe_writes_the_same_bytes() {
    let (r, folder) = recipe("toybox", Some("flat"));
    let (a, _) = emitted(&r, &folder, "scene-twice-a").unwrap();
    let (b, _) = emitted(&r, &folder, "scene-twice-b").unwrap();
    for name in ["toybox.scene.toml", "toybox.materials.toml"] {
        assert_eq!(
            std::fs::read(a.join(name)).unwrap(),
            std::fs::read(b.join(name)).unwrap()
        );
    }
}

fn graded(exposure: f32, bloom: f32, vignette: f32, saturation: f32, warmth: f32) -> Vec<Pass> {
    vec![
        Pass::Exposure(exposure),
        Pass::Bloom(Bloom {
            knee: 0.7,
            knee_width: 0.6,
            soft: 0.15,
            ..Bloom::tent(bloom, 0.9, 12, 50.0, 2)
        }),
        Pass::Vignette(Vignette::power(vignette, 2.4, 0.8, 1.28)),
        Pass::Saturation(Saturation::rec709(saturation)),
        Pass::Warmth(Warmth::linear(warmth)),
        Pass::Tone(Tone::aces()),
    ]
}

fn finish_of(r: &Recipe, folder: &Path, name: &str) -> Vec<Pass> {
    let (out, _) = emitted(r, folder, name).unwrap();
    let scene = Scene::open(out.join(r.output.scene.as_deref().unwrap())).unwrap();
    scene.finish.unwrap().chain.passes
}

fn written_text(r: &Recipe, folder: &Path, name: &str) -> String {
    let (out, _) = emitted(r, folder, name).unwrap();
    std::fs::read_to_string(out.join(r.output.scene.as_deref().unwrap())).unwrap()
}

#[test]
fn the_look_becomes_the_graded_finish_in_manfreds_own_order_with_warmth_before_the_tone() {
    let (r, folder) = recipe("toybox", Some("high"));
    let text = written_text(&r, &folder, "scene-finish");
    assert_eq!(text.matches("\n[[finish.pass]]\n").count(), 6, "{text}");
    assert!(!text.contains("style ="), "{text}");
    let want = graded(1.0, 0.08, 0.2, 1.0, 0.05);
    assert!(matches!(want[4], Pass::Warmth(_)) && matches!(want[5], Pass::Tone(_)));
    let passes = finish_of(&r, &folder, "scene-finish-chain");
    assert_eq!(format!("{passes:?}"), format!("{want:?}"));
}

#[test]
fn no_look_is_the_neutral_graded_finish_and_standard_is_exposure_alone() {
    let (mut r, folder) = recipe("table", None);
    r.look = None;
    let passes = finish_of(&r, &folder, "scene-no-look");
    let want = graded(1.0, 0.0, 0.0, 1.0, 0.0);
    assert_eq!(format!("{passes:?}"), format!("{want:?}"));
    let (r, folder) = recipe("table", None);
    let mut standard = parse(&r.source.replace(
        "[look]\nexposure = 1.0",
        "[look]\nview = \"standard\"\nexposure = 0.8",
    ))
    .unwrap();
    standard.output.scene = Some("table.scene.toml".into());
    let text = written_text(&standard, &folder, "scene-standard-text");
    assert!(text.contains("\n[finish]\nexposure = 0.8\n"), "{text}");
    let passes = finish_of(&standard, &folder, "scene-standard");
    assert_eq!(format!("{passes:?}"), "[Exposure(0.8)]");
}

fn refused(edit: impl FnOnce(&mut Recipe), name: &str) -> Error {
    let (mut r, folder) = recipe("table", None);
    edit(&mut r);
    emitted(&r, &folder, name).unwrap_err()
}

const ROOM: &str = "[sky.room]\nwidth = 64\nfloor = [0.22, 0.135, 0.068]\n[[sky.room.lights]]\nname = \"window\"\naz = -64.0\nel = 14.0\nwidth = 37.5\nheight = 29.0\npower = 10.5\ncolor = [1.0, 0.86, 0.66]\nsoft = 0.05\nslats = 6.0\nopen = 0.7\n";

fn rooted(set: &str, name: &str, assets: &[(&str, &str)]) -> PathBuf {
    let root = scratch(name);
    std::fs::write(
        root.join("project.toml"),
        format!("format = 1\n\n[project]\nname = \"{set}\"\n"),
    )
    .unwrap();
    for (from, to) in assets {
        std::fs::copy(golden(set).join(from), root.join(to)).unwrap();
    }
    root
}

fn inside(r: &Recipe, root: &Path) -> Result<(PathBuf, Vec<PathBuf>), Error> {
    let out = root.join("out");
    let built = build(r, root)?;
    let written = write(&built, r, root, Some(&out))?;
    Ok((out, written))
}

fn mixed(sky: &str, name: &str) -> (Recipe, PathBuf) {
    let root = rooted(
        "table",
        name,
        &[("../../fixtures/sky/window.hdr", "window.hdr")],
    );
    let text = std::fs::read_to_string(golden("table").join("table.toml"))
        .unwrap()
        .replace(
            "[sky]\nkind = \"analytic\"\nintensity = 0.9\nrotation_deg = 0.0",
            &format!("[sky]\n{sky}"),
        );
    let mut r = parse(&text).unwrap();
    r.base = Some(root.clone());
    r.output.scene = Some("table.scene.toml".into());
    (r, root)
}

const MIX: &str = "kind = \"mix\"\npath = \"window.hdr\"\nsource = \"Manfred's own test sky\"\nrotation_deg = 30.0\nintensity = 1.4\nfill = 0.4\n[sky.prepare]\ncap = 60.0\nbalance = [1.0, 0.92, 0.8]\nmean = 0.3\n";

fn close(a: &[[f32; 4]], b: &[[f32; 4]]) {
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(b) {
        for k in 0..3 {
            assert!(
                (x[k] - y[k]).abs() <= 1e-3 * y[k].abs().max(1.0),
                "{x:?} {y:?}"
            );
        }
    }
}

#[test]
fn a_mix_sky_is_two_layers_the_engine_bakes_into_manfreds_own_mix() {
    let (r, root) = mixed(&format!("{MIX}{ROOM}"), "scene-mix");
    let (out, _) = inside(&r, &root).unwrap();
    let text = std::fs::read_to_string(out.join("table.scene.toml")).unwrap();
    assert!(text.contains("\n[sky]\nkind = \"mix\"\n"), "{text}");
    assert_eq!(text.matches("\n[[sky.layer]]\n").count(), 2, "{text}");
    assert!(
        text.contains("\n[sky.layer.prepare]\ncap = 60.0\n"),
        "{text}"
    );
    assert!(text.contains("\n[sky.layer.room]\nwidth = "), "{text}");
    assert!(
        text.contains("\n[[sky.layer.room.lights]]\nname = \"window\"\n"),
        "{text}"
    );
    let scene = Scene::open(out.join("table.scene.toml")).unwrap();
    let sky = scene.sky.unwrap();
    assert_eq!(sky.kind, "mix");
    let Environment::Hdr(made) = sky.environment else {
        panic!("a mix is baked into one HDR");
    };
    let SkySource::Hdr(want) = Lighting::of(&r).unwrap().at(None).sky else {
        panic!("a mix is texels");
    };
    assert_eq!((made.width, made.height), (want.width, want.height));
    close(&made.texels, &want.texels);
}

#[test]
fn a_mix_with_no_lamps_is_its_hdr_alone_and_a_coloured_shade_is_still_refused() {
    let (r, root) = mixed(&format!("{MIX}[sky.room]\nwidth = 64\n"), "scene-mix-dark");
    let (out, _) = inside(&r, &root).unwrap();
    let text = std::fs::read_to_string(out.join("table.scene.toml")).unwrap();
    assert!(text.contains("\nweight = 0.0\n"), "{text}");
    let scene = Scene::open(out.join("table.scene.toml")).unwrap();
    let Environment::Hdr(made) = scene.sky.unwrap().environment else {
        panic!("a mix is baked into one HDR");
    };
    let SkySource::Hdr(want) = Lighting::of(&r).unwrap().at(None).sky else {
        panic!("a mix is texels");
    };
    close(&made.texels, &want.texels);
    let (r, root) = mixed(
        &format!("{MIX}{ROOM}[sky.shade]\nhours = {{ 16 = [0.5, 0.6, 0.7] }}\n"),
        "scene-mix-shade",
    );
    let error = inside(&r, &root).unwrap_err();
    assert_eq!(error.path, "sky.shade");
}

fn high(r: &mut Recipe) -> &mut manfred_recipe::Camera {
    r.cameras.iter_mut().find(|c| c.name == "high").unwrap()
}

fn aspect(r: &Recipe) -> f64 {
    let plate = r.plate.as_ref().unwrap();
    plate.width / plate.height
}

fn opened_scene(r: &Recipe, folder: &Path, name: &str) -> Scene {
    let (out, _) = emitted(r, folder, name).unwrap();
    Scene::open(out.join(r.output.scene.as_deref().unwrap())).unwrap()
}

#[test]
fn the_suns_radius_is_written_for_an_authored_and_a_daylight_sun() {
    let (mut r, folder) = recipe("table", None);
    r.sun.as_mut().unwrap().radius_deg = 1.5;
    let text = written_text(&r, &folder, "scene-radius");
    assert!(
        text.contains("reference_hour = 16.0\n") && text.contains("\nradius = 1.5\n"),
        "{text}"
    );
    assert_eq!(
        opened_scene(&r, &folder, "scene-radius-open")
            .sun
            .unwrap()
            .radius,
        1.5
    );
}

#[test]
fn a_sun_radius_beyond_the_scenes_range_is_refused() {
    let error = refused(
        |r| r.sun.as_mut().unwrap().radius_deg = 11.0,
        "scene-radius-big",
    );
    assert_eq!(error.path, "sun.radius_deg");
}

fn daylight_recipe(hour: f64) -> (Recipe, PathBuf) {
    let (mut r, folder) = recipe("table", None);
    r.sun = None;
    r.daylight = Some(manfred_recipe::Daylight {
        hour,
        day: 120,
        latitude: 51.5,
        heading: 210.0,
        reference_hour: 16.0,
    });
    (r, folder)
}

#[test]
fn a_daylight_sun_and_the_scene_agree_with_the_stage_at_two_hours() {
    for hour in [9.5, 16.0] {
        let (r, folder) = daylight_recipe(hour);
        let sun = opened_scene(&r, &folder, "scene-daylight").sun.unwrap();
        let shine = Lighting::of(&r).unwrap().shine(None);
        assert_eq!(sun.radius, shine.radius_deg);
        for k in 0..3 {
            assert!(
                (sun.direction[k] - shine.sun.direction[k]).abs() < 1e-5,
                "{hour}"
            );
            assert!((sun.color[k] - shine.sun.colour[k]).abs() < 1e-5, "{hour}");
        }
        assert!((sun.intensity - shine.sun.intensity).abs() < 1e-5, "{hour}");
    }
}

#[test]
fn an_authored_sun_written_at_another_hour_follows_it_as_the_stage_does() {
    let (r, folder) = recipe("table", None);
    let lighting = Lighting::of(&r).unwrap();
    let text = written_text(&r, &folder, "scene-authored-hour");
    assert!(text.contains("hour = 16.0\nreference_hour"), "{text}");
    let out = scratch("scene-authored-hour-edit");
    let (made, _) = emitted(&r, &folder, "scene-authored-hour-made").unwrap();
    for name in ["table.scene.toml", "table.materials.toml", "table.glb"] {
        if let Ok(bytes) = std::fs::read(made.join(name)) {
            std::fs::write(out.join(name), bytes).unwrap();
        }
    }
    for hour in [16.0, 9.5, 19.0] {
        let edited = text.replace(
            "hour = 16.0\nreference_hour",
            &format!("hour = {hour}\nreference_hour"),
        );
        std::fs::write(out.join("table.scene.toml"), edited).unwrap();
        let sun = Scene::open(out.join("table.scene.toml"))
            .unwrap()
            .sun
            .unwrap();
        let shine = lighting.shine(Some(hour));
        for k in 0..3 {
            assert!(
                (sun.direction[k] - shine.sun.direction[k]).abs() < 1e-4,
                "{hour} {k}"
            );
            assert!(
                (sun.color[k] - shine.sun.colour[k]).abs() < 1e-4,
                "{hour} {k}"
            );
        }
        assert!((sun.intensity - shine.sun.intensity).abs() < 1e-3, "{hour}");
    }
}

fn lensed(name: &str, edit: impl FnOnce(&mut Recipe)) -> (Recipe, Scene) {
    let (mut r, folder) = recipe("toybox", Some("high"));
    edit(&mut r);
    let scene = opened_scene(&r, &folder, name);
    (r, scene)
}

#[test]
fn the_cameras_lens_round_trips_through_the_scene_and_matches_the_stages() {
    let (r, scene) = lensed("scene-lens-focus", |r| high(r).focus_m = Some(1.1));
    let camera = r.cameras.iter().find(|c| c.name == "high").unwrap();
    let scene_camera = scene.camera.unwrap();
    let depth = scene_camera.depth_of_field.clone().unwrap();
    assert_eq!((depth.fstop, depth.distance), (4.0, 1.1));
    assert!((depth.focal - 0.05).abs() < 1e-7, "{depth:?}");
    let eye = Eye::from_camera(camera, aspect(&r));
    let live = eye.lens(camera, aspect(&r)).unwrap();
    let engine = lens(&scene_camera, aspect(&r) as f32).unwrap();
    assert_eq!(engine.fstop, live.fstop);
    assert!((engine.focal_m - live.focal_m).abs() < 1e-7);
    assert!((engine.focus_m - live.focus_m).abs() < 1e-6);
    assert!(
        (engine.sensor_m - live.sensor_m).abs() < 1e-5,
        "{engine:?} {live:?}"
    );
    let Projection::Perspective { fov } = scene_camera.projection else {
        panic!("high is a perspective camera");
    };
    assert!((f64::from(fov) - camera.fov_y_deg(aspect(&r))).abs() < 1e-4);
}

#[test]
fn without_a_focus_the_scene_focuses_on_the_target_as_the_stage_does() {
    let (r, scene) = lensed("scene-lens-target", |_| {});
    let camera = r.cameras.iter().find(|c| c.name == "high").unwrap();
    let engine = lens(&scene.camera.unwrap(), aspect(&r) as f32).unwrap();
    let live = Eye::from_camera(camera, aspect(&r))
        .lens(camera, aspect(&r))
        .unwrap();
    assert!(
        (engine.focus_m - live.focus_m).abs() < 1e-5,
        "{engine:?} {live:?}"
    );
}

#[test]
fn a_pinhole_camera_writes_its_fov_and_no_lens() {
    let (_, scene) = lensed("scene-lens-pinhole", |r| high(r).fstop = 0.0);
    let camera = scene.camera.unwrap();
    assert!(camera.depth_of_field.is_none());
    assert!(matches!(camera.projection, Projection::Perspective { .. }));
}

#[test]
fn the_cameras_shift_is_written_and_the_projection_matches_the_stages() {
    let (r, scene) = lensed("scene-lens-shift", |r| {
        high(r).shift = [0.1, -0.05];
        high(r).fstop = 0.0;
    });
    let camera = r.cameras.iter().find(|c| c.name == "high").unwrap();
    let scene_camera = scene.camera.unwrap();
    assert_eq!(scene_camera.shift, [0.1, -0.05]);
    let eye = Eye::from_camera(camera, aspect(&r));
    let ours = eye.projection(
        [1600, 1000],
        scene_camera.near.into(),
        scene_camera.far.into(),
    );
    let theirs = scene_camera.projection(aspect(&r) as f32);
    for (a, b) in ours.iter().flatten().zip(theirs.iter().flatten()) {
        assert!(
            (a - b).abs() < 1e-4 * (1.0 + a.abs()),
            "{ours:?} {theirs:?}"
        );
    }
}

#[test]
fn an_orthographic_camera_writes_its_shift_and_no_lens() {
    let (mut r, folder) = recipe("toybox", Some("flat"));
    let flat = r.cameras.iter_mut().find(|c| c.name == "flat").unwrap();
    flat.shift = [0.25, 0.0];
    flat.fstop = 3.0;
    let scene = opened_scene(&r, &folder, "scene-ortho-shift");
    let camera = scene.camera.unwrap();
    assert_eq!(camera.shift, [0.25, 0.0]);
    assert!(camera.depth_of_field.is_none());
}

#[test]
fn a_focus_inside_the_focal_length_is_refused_naming_the_key() {
    let (mut r, folder) = recipe("toybox", Some("high"));
    high(&mut r).focus_m = Some(0.01);
    let error = emitted(&r, &folder, "scene-focus-near").unwrap_err();
    assert_eq!(error.path, "camera[high].focus_m");
}

#[test]
fn the_haze_is_the_engines_room_haze_and_matches_the_stages() {
    let (mut r, folder) = recipe("table", None);
    r.look.as_mut().unwrap().grade.haze = 0.3;
    let text = written_text(&r, &folder, "scene-haze");
    assert!(
        text.contains("\n[haze]\nlo = [-1.0, -0.02, -0.7]\n"),
        "{text}"
    );
    let scene = opened_scene(&r, &folder, "scene-haze-open");
    let engine = room_haze(scene.haze.as_ref().unwrap());
    let stage = Lighting::of(&r).unwrap().haze.unwrap();
    assert_eq!(engine, stage);
    let (none, folder) = recipe("table", None);
    assert!(opened_scene(&none, &folder, "scene-no-haze").haze.is_none());
}

#[test]
fn a_recipe_with_every_key_gets_no_refusal_and_no_left_out_note_for_them() {
    let (mut r, folder) = recipe("toybox", Some("high"));
    high(&mut r).shift = [0.0, -0.05];
    high(&mut r).focus_m = Some(1.2);
    r.sun.as_mut().unwrap().radius_deg = 0.8;
    r.look.as_mut().unwrap().grade.haze = 0.2;
    let built = build(&r, &folder).unwrap();
    let note = built
        .notes
        .iter()
        .find(|n| n.starts_with("output.scene"))
        .unwrap();
    for gone in ["lens", "sun's radius", "haze", "shift"] {
        assert!(!note.contains(gone), "{gone}: {note}");
    }
    write(&built, &r, &folder, Some(&scratch("scene-every-key"))).unwrap();
}

#[test]
fn a_coloured_shade_at_the_hour_is_refused_and_a_grey_one_scales_the_sky() {
    let text = format!(
        "{}\n[sky.shade]\nhours = {{ 6 = [0.6, 0.6, 0.7], 20 = [0.5, 0.5, 0.5] }}\n",
        std::fs::read_to_string(golden("table").join("table.toml"))
            .unwrap()
            .replace(
                "[sky]\nkind = \"analytic\"\nintensity = 0.9\nrotation_deg = 0.0",
                "[sky]\nkind = \"room\"\nintensity = 0.9\n[sky.room]\nwidth = 64"
            )
    );
    let folder = golden("table");
    let mut r = parse(&text).unwrap();
    r.output.scene = Some("table.scene.toml".into());
    let error = emitted(&r, &folder, "scene-shade").unwrap_err();
    assert_eq!(error.path, "sky.shade");
    let grey = text.replace("6 = [0.6, 0.6, 0.7]", "6 = [0.6, 0.6, 0.6]");
    let mut r = parse(&grey).unwrap();
    r.output.scene = Some("table.scene.toml".into());
    let written = written_text(&r, &folder, "scene-shade-grey");
    assert!(
        written.contains("kind = \"room\"\nrotation_deg = 0.0\nintensity = 0.4757"),
        "{written}"
    );
}

#[test]
fn the_left_out_keys_are_a_note_of_the_build() {
    let (r, folder) = recipe("toybox", Some("high"));
    let built = build(&r, &folder).unwrap();
    let note = built
        .notes
        .iter()
        .find(|n| n.starts_with("output.scene"))
        .unwrap();
    assert!(
        note.contains("movers (lid, knob, drawer, roller)"),
        "{note}"
    );
    assert!(note.contains("variants"), "{note}");
    assert!(note.contains("[sidecar] (motion)"), "{note}");
    assert!(!note.contains("lens"), "{note}");
    assert!(
        note.contains("preview [[mover]] has nothing to take; their clip planes are written"),
        "{note}"
    );
    assert!(
        note.contains("hidden objects (guide), which are built and not exported"),
        "{note}"
    );
    let (mut r, folder) = recipe("table", None);
    r.output.scene = None;
    assert!(build(&r, &folder).unwrap().notes.is_empty());
}

fn cards(edit: &str, name: &str) -> (Recipe, PathBuf) {
    let root = rooted("cards", name, &[("badge.png", "badge.png")]);
    let text = std::fs::read_to_string(golden("cards").join("cards.toml")).unwrap();
    let mut r = parse(&edit_cards(&text, edit)).unwrap();
    r.base = Some(root.clone());
    r.output.scene = Some("cards.scene.toml".into());
    (r, root)
}

fn edit_cards(text: &str, edit: &str) -> String {
    let mut out = text.to_string();
    for (from, to) in edit.split(';').filter_map(|pair| pair.split_once('>')) {
        assert!(out.contains(from), "{from}");
        out = out.replace(from, to);
    }
    out
}

const TURN: &str = "face_camera = true>face_camera = true\nkeep_transform = true";
const NOTE: &str = "{ text = \"a lit note \", size = 0.025 }, >";

#[test]
fn a_card_that_turns_needs_its_own_origin_and_text_in_pixels_or_spans_is_refused() {
    let (r, root) = cards("", "scene-cards-refused");
    let error = inside(&r, &root).unwrap_err();
    assert_eq!(error.path, "object[sign].keep_transform");
    let (r, root) = cards(TURN, "scene-cards-spans");
    let error = inside(&r, &root).unwrap_err();
    assert_eq!(error.path, "object[note].spans");
    let (r, root) = cards(&format!("{TURN};{NOTE}"), "scene-cards-overlay");
    let error = inside(&r, &root).unwrap_err();
    assert_eq!(error.path, "object[corner].space");
}

#[test]
fn cards_show_their_image_as_content_and_text_names_its_font() {
    let (r, root) = cards(
        &format!("{TURN};{NOTE};space = \"overlay\">space = \"lit\""),
        "scene-cards",
    );
    let (out, written) = inside(&r, &root).unwrap();
    assert!(written.contains(&out.join("JetBrainsMonoNerdFont-Bold.ttf")));
    assert!(written.contains(&out.join("JetBrainsMonoNerdFont-Regular.ttf")));
    let text = std::fs::read_to_string(out.join("cards.scene.toml")).unwrap();
    assert!(
        text.contains("\n[content.badge]\nid = ") && text.contains("\nimage = \"badge.png\"\n"),
        "{text}"
    );
    let scene = Scene::open(out.join("cards.scene.toml")).unwrap();
    let badge = scene.object("badge").unwrap();
    assert_eq!(badge.alpha_cutoff, 0.5);
    assert!(!badge.face_camera);
    let sign = scene.object("sign").unwrap();
    assert_eq!(sign.alpha_cutoff, 0.0);
    assert!(sign.face_camera);
    assert_eq!(sign.at, [0.3, -0.18, 0.08]);
    assert_eq!(badge.content.as_deref(), Some("badge"));
    assert_eq!(badge.material.as_deref(), Some("card+content0"));
    let layer = scene.library.get("card+content0").unwrap().content_layer;
    assert_eq!(layer.slot, 0);
    assert_eq!(
        scene.object("sign").unwrap().content.as_deref(),
        Some("badge")
    );
    assert!(scene.object("slot").unwrap().content.is_none());
    assert_eq!(
        scene.object("slot").unwrap().material.as_deref(),
        Some("card")
    );
    let plate = &scene.texts["name_plate"];
    assert_eq!(plate.text, "cards");
    assert!(plate.font.ends_with("JetBrainsMonoNerdFont-Bold.ttf"));
    assert!((plate.at[2] + 0.0095).abs() < 1e-6, "{:?}", plate.at);
    assert!((plate.at[1] + 0.33).abs() < 1e-6, "{:?}", plate.at);
    assert_eq!(scene.texts["corner"].size, 18.0);
    let draws = scene.draws();
    assert_eq!(
        draws.items.iter().filter(|d| d.content.is_some()).count(),
        2
    );
}

#[test]
fn the_traces_glass_shadows_and_a_cards_cutoff_round_trip() {
    let (mut r, folder) = recipe("table", None);
    assert!(!r.trace.transmissive_shadows);
    let (out, _) = emitted(&r, &folder, "scene-trace-off").unwrap();
    let text = std::fs::read_to_string(out.join("table.scene.toml")).unwrap();
    assert!(!text.contains("[trace]"), "{text}");
    assert!(
        !Scene::open(out.join("table.scene.toml"))
            .unwrap()
            .trace
            .transmissive_shadows
    );
    r = parse(&format!(
        "{}\n[trace]\ntransmissive_shadows = true\n",
        r.source
    ))
    .unwrap();
    assert!(r.trace.transmissive_shadows);
    r.output.scene = Some("table.scene.toml".into());
    r.base = Some(folder.clone());
    let (out, _) = emitted(&r, &folder, "scene-trace-on").unwrap();
    let text = std::fs::read_to_string(out.join("table.scene.toml")).unwrap();
    assert!(
        text.contains("\n[trace]\ntransmissive_shadows = true\n"),
        "{text}"
    );
    assert!(
        Scene::open(out.join("table.scene.toml"))
            .unwrap()
            .trace
            .transmissive_shadows
    );

    let (r, root) = cards(
        &format!(
            "{TURN};{NOTE};space = \"overlay\">space = \"lit\";alpha_cutoff = 0.5>alpha_cutoff = 0.25"
        ),
        "scene-cutoff",
    );
    let (out, _) = inside(&r, &root).unwrap();
    let scene = Scene::open(out.join("cards.scene.toml")).unwrap();
    assert_eq!(scene.object("badge").unwrap().alpha_cutoff, 0.25);
}

#[test]
fn the_firefly_settings_are_written_in_the_scenes_trace_and_off_writes_none() {
    let (r, folder) = recipe("table", None);
    let text = written_text(&r, &folder, "scene-fireflies-off");
    assert!(!text.contains("[trace]"), "{text}");
    let off = opened_scene(&r, &folder, "scene-fireflies-off-open").trace;
    assert_eq!((off.clamp_indirect, off.filter_glossy), (0.0, 0.0));
    for format in [1, 0] {
        let mut r = parse(&format!(
            "{}\n[trace]\nclamp_indirect = 4\nfilter_glossy = 0.25\n",
            r.source
        ))
        .unwrap();
        r.output.scene = Some("table.scene.toml".into());
        r.output.scene_format = format;
        r.base = Some(folder.clone());
        let name = format!("scene-fireflies-{format}");
        let text = written_text(&r, &folder, &name);
        assert!(
            text.contains("\n[trace]\nclamp_indirect = 4.0\nfilter_glossy = 0.25\n"),
            "{text}"
        );
        let trace = opened_scene(&r, &folder, &format!("{name}-open")).trace;
        assert_eq!((trace.clamp_indirect, trace.filter_glossy), (4.0, 0.25));
    }
    let error = refused(|r| r.trace.filter_glossy = 2.0, "scene-fireflies-far");
    assert_eq!(error.path, "trace.filter_glossy");
    let error = refused(
        |r| r.trace.clamp_indirect = -1.0,
        "scene-fireflies-negative",
    );
    assert_eq!(error.path, "trace.clamp_indirect");
}
