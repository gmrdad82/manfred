use std::path::{Path, PathBuf};

use manfred_recipe::{
    Key, Kind, Loaded, Origin, Outline, Patch, Problem, Projection, Recipe, Shadow, apply,
    apply_variant, load, parse, save, save_to, validate, write,
};
use toml_edit::{Item, Value};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture(name: &str) -> PathBuf {
    fixtures().join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp/recipe-tests")
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn load_fixture(name: &str) -> Loaded {
    load(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn item(text: &str) -> Item {
    Item::Value(text.parse::<Value>().unwrap())
}

fn object_at(name: &str) -> Vec<Key> {
    vec![
        Key::Table("object".into()),
        Key::Object(name.into()),
        Key::Table("at".into()),
    ]
}

fn object<'a>(r: &'a Recipe, name: &str) -> &'a manfred_recipe::Object {
    r.objects.iter().find(|o| o.name == name).unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn problems(name: &str) -> Vec<String> {
    let l = load_fixture(name);
    validate(&l.recipe).iter().map(Problem::to_string).collect()
}

#[test]
fn the_full_example_covers_every_kind_and_validates_clean() {
    let l = load_fixture("full.toml");
    let r = &l.recipe;
    let mut kinds: Vec<&str> = r.objects.iter().map(|o| o.kind.name()).collect();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(
        kinds,
        [
            "box",
            "capsule",
            "card",
            "cylinder",
            "disc",
            "generator",
            "lathe",
            "mesh",
            "outline",
            "plane",
            "pocket",
            "sphere",
            "sweep",
            "text",
            "torus"
        ]
    );
    assert_eq!(validate(r), Vec::new());
    assert_eq!(r.manfred, 1);
    assert_eq!(r.name, "everything");
    assert_eq!(r.seed, 7);
    assert_eq!(r.output.sidecar_dialect, "facts");
    assert!(r.output.hierarchy);
    assert_eq!(r.cameras.len(), 3);
    assert_eq!(r.cameras[2].kind, Projection::Orthographic);
    assert_eq!(r.cameras[1].focal_mm, Some(35.0));
    assert_eq!(r.lights.len(), 2);
    assert_eq!(r.movers.len(), 2);
    assert_eq!(r.movers[1].parts, ["ball"]);
    assert!(r.movers[1].clip2.is_some());
    assert!(r.sun.is_some() && r.daylight.is_none());
    let sky = r.sky.as_ref().unwrap();
    assert_eq!(sky.shade.len(), 3);
    assert_eq!(sky.shade[&20], [0.8, 0.6, 0.4]);
    let room = sky.room.as_ref().unwrap();
    assert_eq!(room.width, 1024);
    assert_eq!(room.lights[0].name, "key");
    assert_eq!(room.lights[0].slats, 0.0);
    assert_eq!(r.look.as_ref().unwrap().grade.bloom, 0.1);
    assert_eq!(r.materials["oak"]["roughness"], 0.7);
    assert_eq!(r.variants["bare"].drop, ["mv_*", "note"]);
}

#[test]
fn objects_keep_their_common_keys_and_defaults() {
    let r = load_fixture("full.toml").recipe;
    let crate_box = object(&r, "crate");
    assert_eq!(crate_box.scale, [1.5, 1.5, 1.5]);
    assert_eq!(crate_box.smooth_angle, Some(30.0));
    assert!(crate_box.flat_faces);
    assert!(!object(&r, "drum").flat_faces);
    let bevel = crate_box.bevel.as_ref().unwrap();
    assert_eq!((bevel.segments, bevel.angle, bevel.clamp), (2, 30.0, true));
    let drum = object(&r, "drum");
    assert_eq!(drum.parent.as_deref(), Some("crate"));
    assert_eq!(drum.materials, ["steel", "rubber"]);
    assert!(drum.smooth && drum.two_sided);
    assert_eq!(object(&r, "ball").shadow, Shadow::Only);
    assert!(object(&r, "button").hidden);
    assert!(object(&r, "ring").keep_transform);
    assert_eq!(object(&r, "crate").shadow, Shadow::Cast);
    let Kind::Sphere {
        segments, rings, ..
    } = object(&r, "ball").kind
    else {
        panic!("ball is a sphere");
    };
    assert_eq!((segments, rings), (96, 48));
    let Kind::Generator { params, .. } = &object(&r, "birds").kind else {
        panic!("birds is a generator");
    };
    assert_eq!(params["count"], 12);
    assert_eq!(params["tag"], "flock");
}

#[test]
fn outlines_nest_and_pixels_become_metres() {
    let r = load_fixture("full.toml").recipe;
    let Kind::Outline {
        outline, profile, ..
    } = &object(&r, "slab").kind
    else {
        panic!("slab is an outline");
    };
    assert_eq!(profile.segments, 3);
    let Outline::Union(parts) = outline else {
        panic!("slab is a union");
    };
    assert_eq!(parts.len(), 4);
    assert!(matches!(parts[0], Outline::Rect { turned, .. } if turned == 10.0));
    assert!(matches!(&parts[2], Outline::Points { points, .. } if points[1] == [0.05, 0.0, 0.01]));
    assert!(matches!(parts[3], Outline::Difference(..)));
    let Kind::Pocket { outline, .. } = &object(&r, "slot").kind else {
        panic!("slot is a pocket");
    };
    let Outline::Rect { rect, radius, .. } = outline else {
        panic!("slot is a rect");
    };
    let m = 0.000378310;
    assert!(near(rect[0], 100.0 * m) && near(rect[2], 200.0 * m));
    assert!(near(rect[1], -80.0 * m) && near(rect[3], -40.0 * m));
    assert!(near(*radius, 8.0 * m));
    let photo = object(&r, "photo");
    assert!(near(photo.at[0], -300.0 * m) && near(photo.at[1], -120.0 * m));
}

#[test]
fn a_plate_recipe_reads_the_plate_from_the_top_left() {
    let r = load_fixture("plate.toml").recipe;
    assert_eq!(validate(&r), Vec::new());
    let plate = r.plate.as_ref().unwrap();
    assert_eq!(plate.origin, Origin::TopLeft);
    let m = 0.00037831;
    let Kind::Outline { outline, .. } = &object(&r, "body").kind else {
        panic!("body is an outline");
    };
    let Outline::Rect { rect, radius, .. } = outline else {
        panic!("body is a rect");
    };
    assert!(near(rect[0], -793.0 * m) && near(rect[2], 793.0 * m));
    assert!(near(rect[1], -496.0 * m) && near(rect[3], 496.0 * m));
    assert!(near(*radius, 40.0 * m));
    let Kind::Outline { outline, .. } = &object(&r, "mv_knob").kind else {
        panic!("knob is an outline");
    };
    let Outline::Circle {
        centre,
        radius,
        segments,
    } = outline
    else {
        panic!("knob is a circle");
    };
    assert!(near(centre[0], 507.0 * m) && near(centre[1], 96.0 * m));
    assert!(near(*radius, 60.0 * m));
    assert_eq!(*segments, 48);
    let cap = object(&r, "mv_knob__cap");
    assert!(near(cap.at[0], 507.0 * m) && near(cap.at[1], 96.0 * m) && cap.at[2] == 0.0);
    let label = object(&r, "label");
    assert_eq!(label.at[2], 0.031);
    let Kind::Card { size, .. } = &label.kind else {
        panic!("label is a card");
    };
    assert!(near(size[0], 400.0 * m) && near(size[1], 60.0 * m));
    assert_eq!(r.cameras[0].shift, [0.0, -0.6395]);
}

#[test]
fn a_driven_recipe_takes_its_numbers_from_the_params_file() {
    let r = load_fixture("driven.toml").recipe;
    assert_eq!(validate(&r), Vec::new());
    assert_eq!(object(&r, "spiral").at, [0.0, 0.12, 0.0]);
    assert_eq!(object(&r, "table").at, [0.0, -0.025, -0.1]);
    let Kind::Cylinder {
        radius, segments, ..
    } = object(&r, "post").kind
    else {
        panic!("post is a cylinder");
    };
    assert_eq!((radius, segments), (0.015, 24));
    assert_eq!(r.cameras[0].focal_mm, Some(50.0));
    assert_eq!(r.daylight.as_ref().unwrap().hour, 15.5);
    assert_eq!(r.output.sidecar_dialect, "facts");
}

#[test]
fn a_toml_params_file_works_too() {
    let dir = scratch("toml_params");
    std::fs::write(
        dir.join("a.toml"),
        "manfred = 1\nname = \"t\"\nparams = \"p.toml\"\n\n[[object]]\nname = \"s\"\nkind = \"sphere\"\nradius = \"${params.scroll.height}\"\n",
    )
    .unwrap();
    std::fs::copy(fixture("params.toml"), dir.join("p.toml")).unwrap();
    let l = load(&dir.join("a.toml")).unwrap();
    let Kind::Sphere { radius, .. } = l.recipe.objects[0].kind else {
        panic!("a sphere");
    };
    assert_eq!(radius, 0.2);
}

#[test]
fn a_flat_recipe_has_an_orthographic_camera_and_cards() {
    let r = load_fixture("flat.toml").recipe;
    assert_eq!(validate(&r), Vec::new());
    assert_eq!(r.cameras[0].kind, Projection::Orthographic);
    assert_eq!(r.cameras[0].ortho_height, Some(2.0));
    let Kind::Card {
        slot, face_camera, ..
    } = &object(&r, "back").kind
    else {
        panic!("back is a card");
    };
    assert_eq!(slot.as_deref(), Some("background"));
    assert!(!face_camera);
    assert_eq!(object(&r, "back").shadow, Shadow::None);
    assert_eq!(r.output.sidecar_dialect, "none");
}

#[test]
fn parsing_twice_gives_the_same_recipe() {
    let text = std::fs::read_to_string(fixture("full.toml")).unwrap();
    assert_eq!(parse(&text).unwrap(), parse(&text).unwrap());
}

fn parse_error(name: &str) -> String {
    match load(&fixture(&format!("errors/{name}.toml"))) {
        Ok(_) => panic!("{name} should not load"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn parse_errors_name_the_key_the_line_and_what_was_expected() {
    let cases: &[(&str, &str)] = &[
        (
            "unknown_key",
            "line 8: object[a].colour: unknown key `colour` in `object[a]`, expected one of: name, id, kind, at, at_px, rotate, scale, keep_transform, parent, material, materials, smooth_angle, smooth, flat_faces, bevel, uv, detail_uv, shadow, two_sided, hidden, dynamic, proxy, lod, size, size_px",
        ),
        (
            "wrong_kind_key",
            "line 8: object[a].radius: unknown key `radius` in `object[a]`, expected one of: name, id, kind, at, at_px, rotate, scale, keep_transform, parent, material, materials, smooth_angle, smooth, flat_faces, bevel, uv, detail_uv, shadow, two_sided, hidden, dynamic, proxy, lod, size, size_px",
        ),
        (
            "unknown_top",
            "line 3: sky_colour: unknown key `sky_colour` in the top level, expected one of: manfred, include, name, seed, params, plate, output, object, use, camera, sun, sky, light, mover, route, follow, stage, marker, variant, sidecar, look, trace, bake, timeline, materials, particles, flare, motion_blur, haze, sound",
        ),
        (
            "missing_manfred",
            "line 1: manfred: missing, expected the schema version, `manfred = 1`",
        ),
        (
            "old_header",
            "line 1: manf: `manf = 1` is the old header; Manfred recipes start with `manfred = 1`",
        ),
        (
            "newer_schema",
            "line 1: manfred: schema 2 is newer than the 1 this Manfred reads, update Manfred",
        ),
        (
            "wrong_type",
            "line 7: object[a].radius: expected a number, found a string",
        ),
        (
            "bad_segments",
            "line 8: object[a].segments: expected a whole number, found a float",
        ),
        (
            "missing_key",
            "line 4: object[a].depth: missing, expected a number",
        ),
        (
            "unknown_kind",
            "line 6: object[a].kind: expected one of \"box\", \"cylinder\", \"sphere\", \"plane\", \"disc\", \"torus\", \"capsule\", \"outline\", \"pocket\", \"lathe\", \"sweep\", \"mesh\", \"generator\", \"card\", \"text\", found \"cube\"",
        ),
        (
            "bad_choice",
            "line 8: object[a].shadow: expected \"cast\", \"only\" or \"none\", found \"dark\"",
        ),
        (
            "both_twins",
            "line 14: object[a].at_px: `at` and `at_px` are both set, expected only one",
        ),
        (
            "outline_shape",
            "line 9: object[a].outline: expected exactly one of rect, circle, points, superellipse, union or difference",
        ),
        (
            "toml_syntax",
            "line 3: not valid TOML: extra `=`, expected nothing",
        ),
        (
            "params_missing",
            "line 8: object[a].radius: params.post.nope is missing from ../driven.params.json",
        ),
        (
            "params_not_number",
            "line 8: object[a].radius: params.spiral in ../driven.params.json is not a number",
        ),
        (
            "params_no_file",
            "line 3: params: cannot read nothing.json: no such file",
        ),
        (
            "params_unset",
            "line 7: object[a].radius: \"${params.lamp.radius}\" needs a `params` file named at the top of the recipe",
        ),
    ];
    for (name, expected) in cases {
        assert_eq!(&parse_error(name), expected, "{name}");
    }
}

#[test]
fn an_absurd_mesh_count_is_refused_before_anything_is_built() {
    let text = "manfred = 1\nname = \"t\"\n\n[[object]]\nname = \"a\"\nkind = \"outline\"\nfront = 0.1\nback = 0.0\noutline = { superellipse = { half = [1, 1], exponent = 2, segments = 4000000000 } }\n";
    assert_eq!(
        parse(text).unwrap_err().to_string(),
        "line 9: object[a].outline.superellipse.segments: expected a whole number from 0 to 4096, found 4000000000"
    );
}

#[test]
fn parse_without_a_file_cannot_read_params() {
    let text = std::fs::read_to_string(fixture("driven.toml")).unwrap();
    let e = parse(&text).unwrap_err();
    assert_eq!(
        e.to_string(),
        "line 15: object[spiral].at[1]: \"${params.spiral.height}\" needs `driven.params.json`, which only a recipe loaded from a file can read"
    );
}

#[test]
fn a_missing_recipe_file_is_an_error_naming_it() {
    let e = load(Path::new("nowhere/none.toml")).unwrap_err();
    assert_eq!(
        e.to_string(),
        "nowhere/none.toml: cannot read the recipe: no such file"
    );
}

#[test]
fn validation_problems_name_the_key_and_the_line() {
    let cases: &[(&str, &[&str])] = &[
        (
            "v_duplicate_name",
            &[
                "line 15: object[a].name: duplicate name, first used on line 5, expected a name no other object uses",
            ],
        ),
        (
            "v_bad_parent",
            &[
                "line 8: object[a].parent: no object named `nobody`, expected the name of another object",
            ],
        ),
        (
            "v_parent_loop",
            &[
                "line 8: object[a].parent: the parents form a loop",
                "line 14: object[b].parent: the parents form a loop",
            ],
        ),
        (
            "v_bad_into",
            &[
                "line 7: object[hole].into: no object named `wall`, expected the object the pocket cuts into",
            ],
        ),
        ("v_no_camera", &[]),
        (
            "v_sun_reference_hour",
            &[
                "line 9: sun.reference_hour: Manfred lights every scene against its reference hour, expected `reference_hour = 16.0` or the key removed",
            ],
        ),
        (
            "v_twin_without_plate",
            &[
                "line 8: object[a].at_px: `at_px` reads plate pixels, so the recipe needs a `[plate]`; `at` takes metres",
            ],
        ),
        (
            "v_shift_on_ortho",
            &["line 10: camera[flat].shift: `shift` is only for perspective cameras"],
        ),
        (
            "v_five_lights",
            &["line 24: light[l4]: the engine takes 4 point lights at most, found 5"],
        ),
        (
            "v_drop_no_match",
            &[
                "line 10: variant.bare.drop: `slat*` matches no object, expected a name or a glob that matches at least one",
            ],
        ),
        (
            "v_mover_no_part",
            &[
                "line 10: mover[knob].group: no object named `mv_knob` or `mv_knob__<part>`, and no `parts`, so the mover moves nothing",
            ],
        ),
        (
            "v_mover_bad_part",
            &["line 14: mover[knob].parts: no object named `ghost`, expected the name of a part"],
        ),
        (
            "v_uv_plate_no_plate",
            &[
                "line 8: object[a].uv: uv \"plate\" projects onto the plate, so the recipe needs a `[plate]`",
            ],
        ),
        (
            "v_variant_set_missing",
            &["line 9: variant.odd.set.sun.irradiance: no key `sun`"],
        ),
        (
            "v_variant_remove_missing",
            &["line 4: variant.odd.remove: `motion`: no key `motion` to remove"],
        ),
        (
            "v_card_no_content",
            &["line 4: object[c].image: a card needs `image` (a file) or `slot` (a name)"],
        ),
        (
            "v_sidecar_no_dialect",
            &[
                "line 11: output.sidecar_dialect: `sidecar` is set but `sidecar_dialect` is none, expected the name of a registered sidecar writer",
            ],
        ),
        (
            "v_sky_no_source",
            &[
                "line 6: sky.source: every HDR has a `source` line naming where it came from and its licence",
            ],
        ),
        (
            "v_flat_faces_alone",
            &["line 8: object[a].flat_faces: `flat_faces` needs `smooth_angle`, expected both"],
        ),
        (
            "v_material_both",
            &[
                "line 9: object[a].materials: `material` and `materials` are both set, expected only one",
            ],
        ),
        (
            "v_material_key",
            &[
                "line 6: materials.lamp.emissive: unknown field `emissive`, expected one of `family`, `base`, `roughness`, `metalness`, `specular`, `clearcoat`, `clearcoat_roughness`, `sheen`, `transmission`, `ior`, `dispersion`, `thickness`, `subsurface`, `subsurface_tint`, `absorption`, `thin_film`, `thin_film_ior`, `thin_film_amount`, `emission`, `fresnel_power`, `normal`, `maps`, `content`, `content_layer`, `layers`, `ageing`",
            ],
        ),
        (
            "v_camera_lens",
            &[
                "line 4: camera[m].fov_y_deg: a perspective camera needs `fov_y_deg`, or `focal_mm` with `sensor_mm`",
            ],
        ),
    ];
    for (name, expected) in cases {
        assert_eq!(
            problems(&format!("errors/{name}.toml")),
            *expected,
            "{name}"
        );
    }
}

#[test]
fn a_patch_changes_one_line_and_its_inverse_restores_every_byte() {
    let original = std::fs::read_to_string(fixture("commented.toml")).unwrap();
    let mut l = load_fixture("commented.toml");
    assert_eq!(write(&l), original);
    let patch = Patch {
        path: object_at("crate"),
        value: Some(item("[0.5, 0.25, 0.0]")),
    };
    let inverse = apply(&mut l, &patch).unwrap();
    let edited = write(&l);
    assert_eq!(object(&l.recipe, "crate").at, [0.5, 0.25, 0.0]);
    let changed: Vec<(&str, &str)> = original
        .lines()
        .zip(edited.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(original.lines().count(), edited.lines().count());
    assert_eq!(
        changed,
        [(
            "at = [0.0, 0.05, 0.0]   # sits on the floor",
            "at = [0.5, 0.25, 0.0]   # sits on the floor"
        )]
    );
    apply(&mut l, &inverse).unwrap();
    assert_eq!(write(&l), original);
    assert_eq!(object(&l.recipe, "crate").at, [0.0, 0.05, 0.0]);
}

#[test]
fn every_kind_of_edit_round_trips_byte_for_byte() {
    let original = std::fs::read_to_string(fixture("commented.toml")).unwrap();
    let t = |s: &str| Key::Table(s.into());
    let o = |s: &str| Key::Object(s.into());
    let edits: Vec<(&str, Vec<Key>, Option<Item>)> = vec![
        ("top-level number", vec![t("seed")], Some(item("9"))),
        (
            "a number inside an inline table",
            vec![t("object"), o("crate"), t("bevel"), t("width")],
            Some(item("0.01")),
        ),
        (
            "a table key",
            vec![t("plate"), t("height")],
            Some(item("75")),
        ),
        (
            "one array element",
            vec![t("object"), o("crate"), t("size"), Key::Index(1)],
            Some(item("0.3")),
        ),
        (
            "a camera by name",
            vec![t("camera"), o("main"), t("fov_y_deg")],
            Some(item("35.0")),
        ),
        (
            "a string",
            vec![t("plate"), t("origin")],
            Some(item("\"top_left\"")),
        ),
        (
            "a new key",
            vec![t("object"), o("lid"), t("hidden")],
            Some(item("true")),
        ),
        (
            "a new key in an inline table",
            vec![t("object"), o("crate"), t("bevel"), t("angle")],
            Some(item("45.0")),
        ),
        (
            "a new array element",
            vec![t("object"), o("lid"), t("materials"), Key::Index(2)],
            Some(item("\"oak\"")),
        ),
    ];
    for (what, path, value) in edits {
        let mut l = load_fixture("commented.toml");
        let inverse =
            apply(&mut l, &Patch { path, value }).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_ne!(write(&l), original, "{what} changed nothing");
        apply(&mut l, &inverse).unwrap_or_else(|e| panic!("{what}, inverse: {e}"));
        assert_eq!(write(&l), original, "{what}");
    }
}

#[test]
fn a_new_array_element_cannot_leave_a_hole() {
    let mut l = load_fixture("commented.toml");
    let e = apply(
        &mut l,
        &Patch {
            path: vec![
                Key::Table("object".into()),
                Key::Object("lid".into()),
                Key::Table("materials".into()),
                Key::Index(7),
            ],
            value: Some(item("\"oak\"")),
        },
    )
    .unwrap_err();
    assert_eq!(
        e.to_string(),
        "object[lid].materials[7]: no element 7, the array has 2"
    );
}

#[test]
fn a_patch_lands_on_the_right_object_after_the_objects_are_reordered() {
    let patch = Patch {
        path: object_at("mv_knob"),
        value: Some(item("[0.7, 0.8, 0.9]")),
    };
    let mut straight = load_fixture("commented.toml");
    let mut shuffled = load_fixture("reordered.toml");
    let names: Vec<&str> = shuffled
        .recipe
        .objects
        .iter()
        .map(|o| o.name.as_str())
        .collect();
    assert_eq!(names, ["lid", "mv_knob", "crate"]);
    apply(&mut straight, &patch).unwrap();
    apply(&mut shuffled, &patch).unwrap();
    for l in [&straight, &shuffled] {
        assert_eq!(object(&l.recipe, "mv_knob").at, [0.7, 0.8, 0.9]);
        assert_eq!(object(&l.recipe, "crate").at, [0.0, 0.05, 0.0]);
        assert_eq!(object(&l.recipe, "lid").at, [0.0, 0.1, 0.0]);
    }
    let text = write(&shuffled);
    assert!(text.contains("at = [0.7, 0.8, 0.9]    # knob\n# trailing note"));
    let by_name = Patch {
        path: object_at("lid"),
        value: Some(item("[1.0, 1.0, 1.0]")),
    };
    apply(&mut shuffled, &by_name).unwrap();
    assert_eq!(object(&shuffled.recipe, "lid").at, [1.0, 1.0, 1.0]);
    assert_eq!(object(&shuffled.recipe, "mv_knob").at, [0.7, 0.8, 0.9]);
}

#[test]
fn a_patch_by_index_after_a_reorder_would_have_hit_the_wrong_object() {
    let mut shuffled = load_fixture("reordered.toml");
    let path = vec![
        Key::Table("object".into()),
        Key::Index(0),
        Key::Table("hidden".into()),
    ];
    apply(
        &mut shuffled,
        &Patch {
            path,
            value: Some(item("true")),
        },
    )
    .unwrap();
    assert!(object(&shuffled.recipe, "lid").hidden);
    assert!(!object(&shuffled.recipe, "crate").hidden);
}

#[test]
fn renaming_an_object_gives_an_inverse_that_still_finds_it() {
    let original = std::fs::read_to_string(fixture("commented.toml")).unwrap();
    let mut l = load_fixture("commented.toml");
    let name = |n: &str| {
        vec![
            Key::Table("object".into()),
            Key::Object(n.into()),
            Key::Table("name".into()),
        ]
    };
    let inverse = apply(
        &mut l,
        &Patch {
            path: name("lid"),
            value: Some(item("\"cover\"")),
        },
    )
    .unwrap();
    assert_eq!(object(&l.recipe, "cover").name, "cover");
    assert_eq!(inverse.path, name("cover"));
    apply(&mut l, &inverse).unwrap();
    assert_eq!(write(&l), original);
}

#[test]
fn removing_a_key_returns_the_value_and_the_inverse_puts_it_back() {
    let mut l = load_fixture("commented.toml");
    let before = l.recipe.objects.clone();
    let path = object_at("lid");
    let inverse = apply(
        &mut l,
        &Patch {
            path: path.clone(),
            value: None,
        },
    )
    .unwrap();
    assert_eq!(object(&l.recipe, "lid").at, [0.0, 0.0, 0.0]);
    assert_eq!(inverse.path, path);
    apply(&mut l, &inverse).unwrap();
    assert_eq!(l.recipe.objects, before);
}

#[test]
fn removing_an_object_by_name_is_undone_in_its_old_place() {
    let original = std::fs::read_to_string(fixture("commented.toml")).unwrap();
    let mut l = load_fixture("commented.toml");
    let path = vec![Key::Table("object".into()), Key::Object("mv_knob".into())];
    let inverse = apply(&mut l, &Patch { path, value: None }).unwrap();
    let names: Vec<&str> = l.recipe.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["crate", "lid"]);
    apply(&mut l, &inverse).unwrap();
    let names: Vec<&str> = l.recipe.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["crate", "mv_knob", "lid"]);
    assert_eq!(write(&l), original);
}

#[test]
fn adding_an_object_by_name_is_undone_by_removing_it() {
    let original = std::fs::read_to_string(fixture("commented.toml")).unwrap();
    let mut l = load_fixture("commented.toml");
    let table = "name = \"peg\"\nkind = \"sphere\"\nradius = 0.01\n"
        .parse::<toml_edit::DocumentMut>()
        .unwrap()
        .as_table()
        .clone();
    let path = vec![Key::Table("object".into()), Key::Object("peg".into())];
    let inverse = apply(
        &mut l,
        &Patch {
            path,
            value: Some(Item::Table(table)),
        },
    )
    .unwrap();
    assert_eq!(l.recipe.objects.last().unwrap().name, "peg");
    apply(&mut l, &inverse).unwrap();
    assert_eq!(l.recipe.objects.len(), 3);
    assert_eq!(write(&l).trim_end(), original.trim_end());
}

#[test]
fn a_patch_that_would_break_the_recipe_is_refused_and_changes_nothing() {
    let original = std::fs::read_to_string(fixture("commented.toml")).unwrap();
    let mut l = load_fixture("commented.toml");
    let e = apply(
        &mut l,
        &Patch {
            path: vec![
                Key::Table("object".into()),
                Key::Object("crate".into()),
                Key::Table("hidden".into()),
            ],
            value: Some(item("\"yes\"")),
        },
    )
    .unwrap_err();
    assert_eq!(
        e.to_string(),
        "line 22: object[crate].hidden: expected true or false, found a string"
    );
    assert_eq!(write(&l), original);
    let e = apply(
        &mut l,
        &Patch {
            path: object_at("ghost"),
            value: Some(item("[0.0, 0.0, 0.0]")),
        },
    )
    .unwrap_err();
    assert_eq!(e.to_string(), "object[ghost].at: no entry named `ghost`");
    let e = apply(
        &mut l,
        &Patch {
            path: vec![
                Key::Table("object".into()),
                Key::Object("lid".into()),
                Key::Table("colour".into()),
            ],
            value: Some(item("1")),
        },
    )
    .unwrap_err();
    assert!(
        e.to_string()
            .starts_with("line 37: object[lid].colour: unknown key `colour` in `object[lid]`")
    );
    assert_eq!(write(&l), original);
}

#[test]
fn a_patch_keeps_the_params_string_until_the_value_is_replaced() {
    let mut l = load_fixture("driven.toml");
    let path = vec![
        Key::Table("object".into()),
        Key::Object("post".into()),
        Key::Table("radius".into()),
    ];
    let inverse = apply(
        &mut l,
        &Patch {
            path,
            value: Some(item("0.02")),
        },
    )
    .unwrap();
    assert!(write(&l).contains("radius = 0.02\n"));
    apply(&mut l, &inverse).unwrap();
    assert!(write(&l).contains("radius = \"${params.post.radius}\"\n"));
    let Kind::Cylinder { radius, .. } = object(&l.recipe, "post").kind else {
        panic!("a cylinder");
    };
    assert_eq!(radius, 0.015);
}

#[test]
fn a_variant_drops_objects_by_glob() {
    let r = load_fixture("full.toml").recipe;
    let bare = apply_variant(&r, "bare").unwrap();
    let names: Vec<&str> = bare.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(r.objects.len(), 17);
    assert_eq!(bare.objects.len(), 14);
    assert!(
        !names.contains(&"mv_knob")
            && !names.contains(&"mv_strip__card0")
            && !names.contains(&"note")
    );
    assert!(names.contains(&"crate") && names.contains(&"photo"));
    assert!(bare.variants.is_empty());
}

#[test]
fn a_variant_sets_nested_keys_and_removes_tables() {
    let r = load_fixture("full.toml").recipe;
    let bare = apply_variant(&r, "bare").unwrap();
    assert_eq!(bare.sun.as_ref().unwrap().irradiance, 0.0);
    assert_eq!(r.sun.as_ref().unwrap().irradiance, 3.0);
    assert_eq!(bare.cameras[0].fov_y_deg, Some(20.0));
    assert_eq!(bare.cameras[1].focal_mm, Some(35.0));
    assert!(bare.look.is_none());
    assert!(r.look.is_some());
}

#[test]
fn a_variant_can_set_a_key_that_is_not_there_yet_and_drop_nothing() {
    let r = load_fixture("plate.toml").recipe;
    let bare = apply_variant(&r, "bare").unwrap();
    assert_eq!(bare.sun.as_ref().unwrap().irradiance, 0.0);
    let names: Vec<&str> = bare.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["body", "screen_well", "mv_knob", "mv_knob__cap"]);
    assert_eq!(validate(&bare), Vec::new());
}

#[test]
fn an_unknown_variant_lists_the_ones_there_are() {
    let r = load_fixture("full.toml").recipe;
    assert_eq!(
        apply_variant(&r, "dusk").unwrap_err().to_string(),
        "variant.dusk: no such variant, expected one of: bare"
    );
    let r = load_fixture("flat.toml").recipe;
    assert_eq!(
        apply_variant(&r, "dusk").unwrap_err().to_string(),
        "variant.dusk: no such variant, the recipe has no variants"
    );
}

#[test]
fn saving_writes_the_text_through_a_temp_file_and_leaves_none_behind() {
    let dir = scratch("save");
    let path = dir.join("copy.toml");
    std::fs::copy(fixture("commented.toml"), &path).unwrap();
    let mut l = load(&path).unwrap();
    apply(
        &mut l,
        &Patch {
            path: object_at("lid"),
            value: Some(item("[0.0, 0.2, 0.0]")),
        },
    )
    .unwrap();
    save(&l).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), write(&l));
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("at = [0.0, 0.2, 0.0]")
    );
    let other = dir.join("other.toml");
    save_to(&l, &other).unwrap();
    assert_eq!(std::fs::read_to_string(&other).unwrap(), write(&l));
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["copy.toml", "other.toml"]);
    let again = load(&path).unwrap();
    assert_eq!(object(&again.recipe, "lid").at, [0.0, 0.2, 0.0]);
}

#[test]
fn saving_into_a_missing_folder_is_an_error_that_names_the_file() {
    let l = load_fixture("flat.toml");
    let dir = scratch("save_missing");
    let target = dir.join("gone/x.toml");
    let e = save_to(&l, &target).unwrap_err();
    assert!(
        e.to_string()
            .starts_with(&format!("{}: cannot write the recipe: ", target.display()))
    );
    assert!(!target.exists());
}

#[test]
fn schema_versions_below_one_and_of_the_wrong_type_are_refused() {
    assert_eq!(
        parse("manfred = 0\nname = \"t\"\n")
            .unwrap_err()
            .to_string(),
        "line 1: manfred: expected a schema version from 1 up"
    );
    assert_eq!(
        parse("manfred = \"1\"\nname = \"t\"\n")
            .unwrap_err()
            .to_string(),
        "line 1: manfred: expected a whole number, found a string"
    );
    assert_eq!(parse("manfred = 1\nname = \"t\"\n").unwrap().manfred, 1);
}

#[test]
fn a_recipe_knows_the_line_of_every_key_it_read() {
    let r = load_fixture("commented.toml").recipe;
    assert_eq!(r.line_of("object#0"), Some(15));
    assert_eq!(r.line_of("object#0.at"), Some(19));
    assert_eq!(r.line_of("object#0.nothing"), Some(15));
    assert_eq!(r.line_of("camera#0.fov_y_deg"), Some(42));
}
