use std::path::{Path, PathBuf};

use manfred_recipe::{
    Key, Kind, Loaded, Outline, Patch, Recipe, apply, apply_all, apply_set, apply_variant, load,
    parse, save, validate, value_at, write,
};
use toml_edit::{Item, Value};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/include")
        .join(name)
}

fn scratch(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp/recipe-tests/include")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    for (file, text) in files {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

fn copied(name: &str) -> PathBuf {
    let files: Vec<(String, String)> = ["rig.toml", "parts/knob.toml", "panel.toml", "flat.toml"]
        .iter()
        .map(|f| (f.to_string(), std::fs::read_to_string(fixture(f)).unwrap()))
        .collect();
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    scratch(name, &refs)
}

fn read(path: &Path) -> Recipe {
    load(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .recipe
}

fn error(path: &Path) -> manfred_recipe::Error {
    match load(path) {
        Ok(_) => panic!("{} read without an error", path.display()),
        Err(e) => e,
    }
}

fn names(r: &Recipe) -> Vec<&str> {
    r.objects.iter().map(|o| o.name.as_str()).collect()
}

fn item(text: &str) -> Item {
    Item::Value(text.parse::<Value>().unwrap())
}

fn keys(path: &[&str]) -> Vec<Key> {
    path.iter()
        .map(|k| match k.strip_prefix('@') {
            Some(name) => Key::Object(name.to_string()),
            None => Key::Table(k.to_string()),
        })
        .collect()
}

fn set(path: &[&str], value: &str) -> Patch {
    Patch {
        path: keys(path),
        value: Some(item(value)),
    }
}

const HEAD: &str = "manfred = 1\nname = \"scene\"\n";

fn object(name: &str) -> String {
    format!("\n[[object]]\nname = \"{name}\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n")
}

#[test]
fn includes_merge_in_order_depth_first_before_the_recipes_own() {
    let c = object("c1");
    let a = format!("include = [\"c.toml\"]\n{}", object("a1"));
    let b = object("b1");
    let main = format!("{HEAD}include = [\"a.toml\", \"b.toml\"]\n{}", object("m1"));
    let dir = scratch(
        "order",
        &[
            ("c.toml", &c),
            ("a.toml", &a),
            ("b.toml", &b),
            ("scene.toml", &main),
        ],
    );
    let r = read(&dir.join("scene.toml"));
    assert_eq!(names(&r), ["c1", "a1", "b1", "m1"]);
    assert_eq!(
        r.files.names(),
        ["scene.toml", "a.toml", "c.toml", "b.toml"]
    );
    assert_eq!(r.files.paths()[0], dir.join("scene.toml"));
    assert!(r.files.included());
    assert_eq!(validate(&r), []);
}

#[test]
fn the_recipes_own_keys_win_over_an_included_table() {
    let r = read(&fixture("panel.toml"));
    let sky = r.sky.as_ref().unwrap();
    assert_eq!(sky.intensity, 1.5);
    assert_eq!(sky.rotation_deg, 10.0);
    assert_eq!(r.look.as_ref().unwrap().exposure, 1.1);
    assert_eq!(r.cameras[0].name, "main");
    assert_eq!(r.lights[0].name, "lamp");
    assert!(r.materials.contains_key("tile"));
    assert_eq!(r.sun.as_ref().unwrap().irradiance, 3.0);
}

#[test]
fn a_name_defined_in_two_files_is_refused_naming_both() {
    let rig = "[[light]]\nname = \"lamp\"\nposition = [0.0, 1.0, 0.0]\nintensity = 1.0\n";
    let main = format!(
        "{HEAD}include = [\"rig.toml\"]\n\n[[light]]\nname = \"lamp\"\nposition = [0.0, 2.0, 0.0]\nintensity = 1.0\n"
    );
    let dir = scratch("twice", &[("rig.toml", rig), ("scene.toml", &main)]);
    let e = error(&dir.join("scene.toml"));
    assert_eq!(e.path, "light[lamp]");
    assert_eq!(e.line, Some(5));
    assert!(
        e.text.contains("rig.toml") && e.text.contains("scene.toml"),
        "{e}"
    );

    let sky = "[sky]\nkind = \"analytic\"\n";
    let main = format!("{HEAD}include = [\"one.toml\", \"two.toml\"]\n");
    let dir = scratch(
        "twice-sky",
        &[("one.toml", sky), ("two.toml", sky), ("scene.toml", &main)],
    );
    let e = error(&dir.join("scene.toml"));
    assert!(e.path.starts_with("two.toml:1: sky"), "{e}");
    assert!(
        e.text.contains("one.toml") && e.text.contains("two.toml"),
        "{e}"
    );

    let main =
        format!("{HEAD}include = [\"rig.toml\"]\n\n[materials.tile]\nbase = [1.0, 1.0, 1.0]\n");
    let rig = "[materials.tile]\nbase = [0.5, 0.5, 0.5]\n";
    let dir = scratch(
        "twice-material",
        &[("rig.toml", rig), ("scene.toml", &main)],
    );
    let e = error(&dir.join("scene.toml"));
    assert_eq!(e.path, "materials.tile");
    assert!(
        e.text.contains("rig.toml") && e.text.contains("scene.toml"),
        "{e}"
    );
}

#[test]
fn a_cycle_is_refused() {
    let a = "include = [\"b.toml\"]\n";
    let b = "include = [\"a.toml\"]\n";
    let main = format!("{HEAD}include = [\"a.toml\"]\n");
    let dir = scratch(
        "cycle",
        &[("a.toml", a), ("b.toml", b), ("scene.toml", &main)],
    );
    let e = error(&dir.join("scene.toml"));
    assert_eq!(e.path, "b.toml:1: include");
    assert_eq!(e.line, Some(3));
    assert!(
        e.text.contains("a.toml → b.toml → a.toml is a cycle"),
        "{e}"
    );

    let back = "include = [\"scene.toml\"]\n";
    let dir = scratch("cycle-main", &[("a.toml", back), ("scene.toml", &main)]);
    let e = error(&dir.join("scene.toml"));
    assert!(e.text.contains("scene.toml → a.toml → scene.toml"), "{e}");
}

#[test]
fn a_missing_file_is_named() {
    let main = format!("{HEAD}include = [\n  \"rig.toml\",\n  \"nope.toml\",\n]\n");
    let dir = scratch("missing", &[("rig.toml", ""), ("scene.toml", &main)]);
    let e = error(&dir.join("scene.toml"));
    assert_eq!(e.path, "include");
    assert_eq!(e.line, Some(5));
    assert_eq!(e.text, "cannot read nope.toml: no such file");
    let e = parse(&format!("{HEAD}include = [\"rig.toml\"]\n")).unwrap_err();
    assert!(e.text.contains("cannot read rig.toml"), "{e}");
}

#[test]
fn a_problem_in_an_included_file_names_its_own_file_and_line() {
    let rig = "manfred = 1\n\n[[light]]\nname = \"lamp\"\nposition = [0.0, 1.0, 0.0]\nintensity = 1.0\ncolour = [1.0, 1.0, 1.0]\n";
    let main = format!("{HEAD}\ninclude = [\"lights/rig.toml\"]\n");
    let dir = scratch(
        "own-line",
        &[("lights/rig.toml", rig), ("scene.toml", &main)],
    );
    let e = error(&dir.join("scene.toml"));
    assert_eq!(e.path, "lights/rig.toml:7: light[lamp].colour");
    assert_eq!(e.line, Some(4));
    assert!(e.text.starts_with("unknown key `colour`"), "{e}");
    assert_eq!(
        e.to_string(),
        format!("line 4: lights/rig.toml:7: light[lamp].colour: {}", e.text)
    );

    let rig = "[[object]]\nname = \"knob\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\nparent = \"nobody\"\n";
    let dir = scratch(
        "own-problem",
        &[("lights/rig.toml", rig), ("scene.toml", &main)],
    );
    let r = read(&dir.join("scene.toml"));
    let problems = validate(&r);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path, "lights/rig.toml:5: object[knob].parent");
    assert_eq!(problems[0].line, Some(4));
    assert_eq!(r.origin_of("object#0.parent"), Some(("lights/rig.toml", 5)));
    assert_eq!(r.line_of("object#0.parent"), Some(4));

    let broken = "[[light]\nname = 1\n";
    let dir = scratch(
        "own-toml",
        &[("lights/rig.toml", broken), ("scene.toml", &main)],
    );
    let e = error(&dir.join("scene.toml"));
    assert_eq!(e.path, "lights/rig.toml:1");
    assert!(e.text.starts_with("not valid TOML"), "{e}");
}

#[test]
fn an_included_file_holds_only_shared_tables() {
    let main = format!("{HEAD}include = [\"rig.toml\"]\n");
    let dir = scratch(
        "only-shared",
        &[("rig.toml", "name = \"rig\"\n"), ("scene.toml", &main)],
    );
    let e = error(&dir.join("scene.toml"));
    assert_eq!(e.path, "rig.toml:1: name");
    assert!(e.text.contains("belongs to the recipe"), "{e}");
    let dir = scratch(
        "only-shared-schema",
        &[("rig.toml", "manfred = 2\n"), ("scene.toml", &main)],
    );
    assert_eq!(error(&dir.join("scene.toml")).path, "rig.toml:1: manfred");
    let dir = scratch(
        "old-header-included",
        &[("rig.toml", "manf = 1\n"), ("scene.toml", &main)],
    );
    let e = error(&dir.join("scene.toml"));
    assert_eq!(e.path, "rig.toml:1: manf");
    assert!(e.text.contains("is the old header"), "{e}");
    let main = format!("{HEAD}include = [\"/abs/rig.toml\"]\n");
    let dir = scratch("absolute", &[("scene.toml", &main)]);
    assert!(error(&dir.join("scene.toml")).text.contains("is absolute"));
}

#[test]
fn paths_in_an_included_or_used_file_are_relative_to_it() {
    let rig = "[sky]\nkind = \"hdr\"\npath = \"sky/window.hdr\"\nsource = \"test\"\n";
    let part = "manfred = 1\nname = \"part\"\n\n[[object]]\nname = \"photo\"\nkind = \"card\"\nsize = [0.1, 0.1]\nimage = \"../art/photo.png\"\n\n[[object]]\nname = \"model\"\nkind = \"mesh\"\npath = \"model.glb\"\n";
    let main = format!(
        "{HEAD}include = [\"shared/rig.toml\"]\n\n[[use]]\nfile = \"parts/part.toml\"\nprefix = \"a_\"\n"
    );
    let dir = scratch(
        "relative",
        &[
            ("shared/rig.toml", rig),
            ("parts/part.toml", part),
            ("scene.toml", &main),
        ],
    );
    let r = read(&dir.join("scene.toml"));
    assert_eq!(
        r.sky.unwrap().path.as_deref(),
        Some("shared/sky/window.hdr")
    );
    let Kind::Card { image, .. } = &r.objects[0].kind else {
        panic!("a card");
    };
    assert_eq!(image.as_deref(), Some("art/photo.png"));
    let Kind::Mesh { path, .. } = &r.objects[1].kind else {
        panic!("a mesh");
    };
    assert_eq!(path, "parts/model.glb");
}

#[test]
fn a_use_places_a_prefixed_copy_with_rewired_parents() {
    let r = read(&fixture("panel.toml"));
    assert_eq!(
        names(&r),
        ["board", "left_base", "left_cap", "right_base", "right_cap"]
    );
    let by = |name: &str| r.objects.iter().find(|o| o.name == name).unwrap();
    assert_eq!(by("left_base").parent, None);
    assert_eq!(by("left_cap").parent.as_deref(), Some("left_base"));
    assert_eq!(by("right_base").parent.as_deref(), Some("board"));
    assert_eq!(by("right_cap").parent.as_deref(), Some("right_base"));
    assert_eq!(by("left_base").at, [-0.05, 0.05, 0.0]);
    assert_eq!(by("right_base").rotate, [0.0, 0.0, 45.0]);
    assert_eq!(by("right_base").scale, [1.5; 3]);
    assert_eq!(by("left_cap").at, [0.0, 0.0, 0.01]);
    assert_eq!(by("right_cap").rotate, [90.0, 0.0, 0.0]);
    assert_eq!(r.uses.len(), 2);
    assert_eq!(r.uses[0].objects, ["left_base", "left_cap"]);
    assert_eq!(r.uses[1].parent.as_deref(), Some("board"));
    assert_eq!(r.copy_of("right_cap").unwrap().prefix, "right_");
    assert!(r.copy_of("board").is_none());
    assert_eq!(validate(&r), []);
}

#[test]
fn a_use_composes_its_placement_with_the_parts_own() {
    let part = "manfred = 1\nname = \"part\"\n\n[[object]]\nname = \"arm\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\nat = [0.1, 0.0, 0.0]\nrotate = [0.0, 30.0, 0.0]\nscale = [1.0, 2.0, 1.0]\n\n[[object]]\nname = \"slot\"\nkind = \"pocket\"\ninto = \"plate\"\noutline = { circle = [0.0, 0.0, 0.01] }\ndepth = 0.002\n\n[[object]]\nname = \"plate\"\nkind = \"outline\"\noutline = { rect = [-0.05, -0.05, 0.05, 0.05] }\nfront = 0.0\nback = 0.01\n\n[[object]]\nname = \"label\"\nkind = \"text\"\non = \"plate\"\nat = [0.01, 0.0, 0.0]\nspans = [{ text = \"hi\", size = 0.01 }]\n";
    let main = format!(
        "{HEAD}\n[[use]]\nfile = \"part.toml\"\nprefix = \"p_\"\nat = [1.0, 0.0, 0.0]\nrotate = [0.0, 90.0, 0.0]\nscale = 2.0\n"
    );
    let dir = scratch("compose", &[("part.toml", part), ("scene.toml", &main)]);
    let r = read(&dir.join("scene.toml"));
    let arm = &r.objects[0];
    assert_eq!(arm.at, [1.0, 0.0, -0.2]);
    assert_eq!(arm.rotate, [0.0, 120.0, 0.0]);
    assert_eq!(arm.scale, [2.0, 4.0, 2.0]);
    let Kind::Pocket { into, .. } = &r.objects[1].kind else {
        panic!("a pocket");
    };
    assert_eq!(into, "p_plate");
    let Kind::Text { on, .. } = &r.objects[3].kind else {
        panic!("text");
    };
    assert_eq!(on.as_deref(), Some("p_plate"));
    assert_eq!(r.objects[3].at, [0.01, 0.0, 0.0]);

    let main = format!(
        "{HEAD}\n[[use]]\nfile = \"part.toml\"\nprefix = \"p_\"\nrotate = [90.0, 0.0, 0.0]\n"
    );
    let dir = scratch(
        "compose-axes",
        &[("part.toml", part), ("scene.toml", &main)],
    );
    let arm = read(&dir.join("scene.toml")).objects[0].clone();
    assert_eq!(arm.at, [0.1, 0.0, 0.0]);
    let expected = [90.0, 0.0, 30.0];
    for (got, want) in arm.rotate.iter().zip(expected) {
        assert!((got - want).abs() < 1e-9, "{:?}", arm.rotate);
    }

    let main = format!(
        "{HEAD}\n[[use]]\nfile = \"part.toml\"\nprefix = \"p_\"\nscale = [1.0, 2.0, 1.0]\n"
    );
    let dir = scratch(
        "compose-shear",
        &[("part.toml", part), ("scene.toml", &main)],
    );
    let e = error(&dir.join("scene.toml"));
    assert_eq!(e.path, "use[p_].scale");
    assert!(e.text.contains("`p_arm`"), "{e}");
}

#[test]
fn uses_nest_and_compose_every_level() {
    let leaf = "manfred = 1\nname = \"leaf\"\n\n[[object]]\nname = \"bit\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\nat = [0.1, 0.0, 0.0]\n";
    let mid = "manfred = 1\nname = \"mid\"\n\n[[object]]\nname = \"hub\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n\n[[use]]\nfile = \"leaf.toml\"\nprefix = \"a_\"\nat = [0.0, 1.0, 0.0]\n\n[[use]]\nfile = \"leaf.toml\"\nprefix = \"b_\"\nparent = \"hub\"\n";
    let main =
        format!("{HEAD}\n[[use]]\nfile = \"mid.toml\"\nprefix = \"m_\"\nat = [0.0, 0.0, 2.0]\n");
    let dir = scratch(
        "nested",
        &[
            ("leaf.toml", leaf),
            ("mid.toml", mid),
            ("scene.toml", &main),
        ],
    );
    let r = read(&dir.join("scene.toml"));
    assert_eq!(names(&r), ["m_hub", "m_a_bit", "m_b_bit"]);
    assert_eq!(r.objects[1].at, [0.1, 1.0, 2.0]);
    assert_eq!(r.objects[2].parent.as_deref(), Some("m_hub"));
    assert_eq!(r.objects[2].at, [0.1, 0.0, 0.0]);
    assert_eq!(r.uses[0].objects, ["m_hub", "m_a_bit", "m_b_bit"]);

    let main = format!("{HEAD}\n[[use]]\nfile = \"self.toml\"\nprefix = \"s_\"\n");
    let own = "manfred = 1\nname = \"self\"\n\n[[use]]\nfile = \"self.toml\"\nprefix = \"s_\"\n";
    let dir = scratch("nested-cycle", &[("self.toml", own), ("scene.toml", &main)]);
    assert!(error(&dir.join("scene.toml")).text.contains("is a cycle"));
}

#[test]
fn a_use_is_refused_where_it_cannot_place() {
    let part = "manfred = 1\nname = \"part\"\n\n[[object]]\nname = \"bit\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n";
    for (case, body, path, text) in [
        ("no-file", "prefix = \"a_\"\n", "use[a_].file", "missing"),
        (
            "number-prefix",
            "file = \"part.toml\"\nprefix = 1\n",
            "use[0].prefix",
            "expected a string",
        ),
        (
            "unknown",
            "file = \"part.toml\"\nprefix = \"a_\"\nscale_px = 1.0\n",
            "use[a_].scale_px",
            "unknown key",
        ),
        (
            "missing",
            "file = \"gone.toml\"\nprefix = \"a_\"\n",
            "use[a_].file",
            "cannot read gone.toml",
        ),
    ] {
        let main = format!("{HEAD}\n[[use]]\n{body}");
        let dir = scratch(
            &format!("refused-{case}"),
            &[("part.toml", part), ("scene.toml", &main)],
        );
        let e = error(&dir.join("scene.toml"));
        assert_eq!(e.path, path, "{case}: {e}");
        assert!(e.text.contains(text), "{case}: {e}");
    }
    let main = format!(
        "{HEAD}{}\n[[use]]\nfile = \"part.toml\"\nprefix = \"a_\"\n\n[[use]]\nfile = \"part.toml\"\nprefix = \"a_\"\n",
        object("a_bit")
    );
    let dir = scratch(
        "refused-clash",
        &[("part.toml", part), ("scene.toml", &main)],
    );
    let e = error(&dir.join("scene.toml"));
    assert_eq!(e.path, "part.toml:4: object[a_bit]");
    assert!(e.text.contains("scene.toml"), "{e}");
    let main =
        format!("{HEAD}\n[[use]]\nfile = \"part.toml\"\nprefix = \"a_\"\nparent = \"nobody\"\n");
    let dir = scratch(
        "refused-parent",
        &[("part.toml", part), ("scene.toml", &main)],
    );
    let problems = validate(&read(&dir.join("scene.toml")));
    assert!(
        problems.iter().any(|p| p.path == "use[a_].parent"),
        "{problems:?}"
    );
}

#[test]
fn a_use_without_a_prefix_keeps_the_parts_names() {
    let part = "[[object]]\nname = \"mark\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\nscale = 0.001\n\n[[object]]\nname = \"mark_tile\"\nkind = \"box\"\nsize = [64.0, 22.0, 64.0]\nat = [0.0, 11.0, 0.0]\nparent = \"mark\"\n";
    let main = format!("{HEAD}\n[[use]]\nfile = \"parts/mark.toml\"\nat = [-0.2, 0.0, 0.0]\n");
    let dir = scratch(
        "no-prefix",
        &[("parts/mark.toml", part), ("scene.toml", &main)],
    );
    let mut l = load(&dir.join("scene.toml")).unwrap();
    assert_eq!(names(&l.recipe), ["mark", "mark_tile"]);
    assert_eq!(l.recipe.objects[0].at, [-0.2, 0.0, 0.0]);
    assert_eq!(l.recipe.objects[0].scale, [0.001; 3]);
    assert_eq!(l.recipe.objects[1].parent.as_deref(), Some("mark"));
    assert_eq!(l.recipe.uses[0].prefix, "");
    assert_eq!(validate(&l.recipe), []);
    let e = apply(&mut l, &set(&["object", "@mark", "at"], "[0.0, 0.0, 0.0]")).unwrap_err();
    assert!(e.text.contains("a use places from parts/mark.toml"), "{e}");
}

#[test]
fn variants_and_sets_reach_included_and_used_objects() {
    let mut text = std::fs::read_to_string(fixture("panel.toml")).unwrap();
    text.push_str(
        "\n[variant.bare]\ndrop = [\"right_*\"]\nset = { \"light.lamp.intensity\" = 5.0 }\n",
    );
    let dir = copied("variants");
    std::fs::write(dir.join("panel.toml"), &text).unwrap();
    let r = read(&dir.join("panel.toml"));
    let bare = apply_variant(&r, "bare").unwrap();
    assert_eq!(names(&bare), ["board", "left_base", "left_cap"]);
    assert_eq!(bare.lights[0].intensity, 5.0);
    let set = apply_set(
        &bare,
        &[
            "object.left_cap.radius=0.01".to_string(),
            "use.left_.at=[0.0, 0.0, 0.0]".to_string(),
            "sky.rotation_deg=20".to_string(),
        ],
    )
    .unwrap();
    let cap = set.objects.iter().find(|o| o.name == "left_cap").unwrap();
    assert!(matches!(cap.kind, Kind::Cylinder { radius, .. } if radius == 0.01));
    assert_eq!(set.objects[1].at, [0.0, 0.0, 0.0]);
    assert_eq!(set.sky.as_ref().unwrap().rotation_deg, 20.0);
    assert_eq!(set.lights[0].intensity, 5.0);
    assert_eq!(names(&set), ["board", "left_base", "left_cap"]);
    assert_eq!(validate(&r), []);
}

#[test]
fn a_superellipse_outline_reads_as_its_points() {
    let text = format!(
        "{HEAD}\n[[object]]\nname = \"tile\"\nkind = \"outline\"\nfront = 0.0\nback = 0.01\noutline = {{ superellipse = {{ half = [0.03, 0.02], exponent = 4.0, segments = 16, centre = [0.1, 0.0] }} }}\n\n[[object]]\nname = \"round\"\nkind = \"outline\"\nfront = 0.0\nback = 0.01\noutline = {{ union = [{{ superellipse = {{ half = [0.02, 0.02], exponent = 2.0 }} }}, {{ rect = [0.0, -0.01, 0.05, 0.01] }}] }}\n"
    );
    let r = parse(&text).unwrap();
    let Kind::Outline {
        outline: Outline::Points { points, .. },
        ..
    } = &r.objects[0].kind
    else {
        panic!("points");
    };
    assert_eq!(points.len(), 16);
    assert_eq!(points[0], [0.1 + 0.03, 0.0, 0.0]);
    assert_eq!(points[4], [0.1, 0.02, 0.0]);
    let Kind::Outline {
        outline: Outline::Union(parts),
        ..
    } = &r.objects[1].kind
    else {
        panic!("a union");
    };
    let Outline::Points { points, .. } = &parts[0] else {
        panic!("points");
    };
    assert_eq!(points.len(), 64);
    for (i, p) in points.iter().enumerate() {
        let a = std::f64::consts::TAU * i as f64 / 64.0;
        assert!((p[0] - 0.02 * a.cos()).abs() < 1e-15 && (p[1] - 0.02 * a.sin()).abs() < 1e-15);
    }
    for (bad, key) in [
        ("half = [0.0, 0.02], exponent = 4.0", "half"),
        ("half = [0.02, 0.02], exponent = 0.0", "exponent"),
        (
            "half = [0.02, 0.02], exponent = 4.0, segments = 30",
            "segments",
        ),
        (
            "half = [0.02, 0.02], exponent = 4.0, radius = 1.0",
            "radius",
        ),
    ] {
        let text = format!(
            "{HEAD}\n[[object]]\nname = \"tile\"\nkind = \"outline\"\nfront = 0.0\nback = 0.01\noutline = {{ superellipse = {{ {bad} }} }}\n"
        );
        let e = parse(&text).unwrap_err();
        assert_eq!(
            e.path,
            format!("object[tile].outline.superellipse.{key}"),
            "{e}"
        );
        assert_eq!(e.line, Some(9), "{e}");
    }
}

#[test]
fn the_golden_panel_reads_as_its_flat_twin() {
    let family = read(&fixture("panel.toml"));
    let flat = read(&fixture("flat.toml"));
    assert_eq!(family.objects, flat.objects);
    assert_eq!(family.cameras, flat.cameras);
    assert_eq!(family.lights, flat.lights);
    assert_eq!(family.sun, flat.sun);
    assert_eq!(family.sky, flat.sky);
    assert_eq!(family.look, flat.look);
    assert_eq!(family.materials, flat.materials);
    assert_eq!(family.output, flat.output);
    assert_eq!(family.name, flat.name);
    assert!(flat.uses.is_empty());
    let alone = read(&fixture("parts/knob.toml"));
    assert_eq!(alone.cameras, flat.cameras);
    assert_eq!(
        alone
            .objects
            .iter()
            .map(|o| o.name.as_str())
            .collect::<Vec<_>>(),
        ["base", "cap"]
    );
}

fn panel(name: &str) -> (PathBuf, Loaded) {
    let dir = copied(name);
    let loaded = load(&dir.join("panel.toml")).unwrap();
    (dir, loaded)
}

fn text(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).unwrap()
}

#[test]
fn the_window_edits_a_key_in_the_file_that_holds_it() {
    let (dir, mut l) = panel("window-route");
    let rig = text(&dir, "rig.toml");
    let own = text(&dir, "panel.toml");
    assert_eq!(write(&l), own);
    let intensity = keys(&["light", "@lamp", "intensity"]);
    assert_eq!(value_at(&l.doc, &intensity).unwrap().as_float(), Some(20.0));

    let undo = apply(&mut l, &set(&["light", "@lamp", "intensity"], "30.0")).unwrap();
    save(&l).unwrap();
    assert_eq!(text(&dir, "panel.toml"), own);
    assert_eq!(
        text(&dir, "rig.toml"),
        rig.replace("intensity = 20.0", "intensity = 30.0")
    );
    assert_eq!(l.recipe.lights[0].intensity, 30.0);
    assert_eq!(value_at(&l.doc, &intensity).unwrap().as_float(), Some(30.0));

    let back = apply_all(
        &mut l,
        &[
            set(&["sky", "intensity"], "2.0"),
            set(&["sky", "rotation_deg"], "15.0"),
            set(&["object", "@board", "at"], "[0.0, 0.06, -0.005]"),
        ],
    )
    .unwrap();
    save(&l).unwrap();
    assert_eq!(
        text(&dir, "panel.toml"),
        own.replace("intensity = 1.5", "intensity = 2.0")
            .replace("at = [0.0, 0.05, -0.005]", "at = [0.0, 0.06, -0.005]")
    );
    assert_eq!(
        text(&dir, "rig.toml"),
        rig.replace("intensity = 20.0", "intensity = 30.0")
            .replace("rotation_deg = 10.0", "rotation_deg = 15.0")
    );
    apply_all(&mut l, &back).unwrap();
    apply(&mut l, &undo).unwrap();
    save(&l).unwrap();
    assert_eq!(text(&dir, "panel.toml"), own);
    assert_eq!(text(&dir, "rig.toml"), rig);
    assert_eq!(
        text(&dir, "parts/knob.toml"),
        text(&fixture(""), "parts/knob.toml")
    );
}

#[test]
fn the_window_removes_and_restores_an_included_entry_in_place() {
    let dir = copied("window-remove");
    let rig = text(&dir, "rig.toml");
    let more = format!(
        "{rig}\n[[camera]]\nname = \"side\"\nkind = \"perspective\"\nposition = [1.0, 0.1, 0.0]\ntarget = [0.0, 0.05, 0.0]\nfov_y_deg = 30.0\n"
    );
    std::fs::write(dir.join("rig.toml"), &more).unwrap();
    let own = format!(
        "{}\n[[camera]]\nname = \"top\"\nkind = \"orthographic\"\nposition = [0.0, 1.0, 0.0]\ntarget = [0.0, 0.0, 0.0]\nup = [0.0, 0.0, -1.0]\northo_height = 0.3\n",
        text(&dir, "panel.toml")
    );
    std::fs::write(dir.join("panel.toml"), &own).unwrap();
    let mut l = load(&dir.join("panel.toml")).unwrap();
    let cams = |l: &Loaded| {
        l.recipe
            .cameras
            .iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(cams(&l), ["main", "side", "top"]);
    let undo = apply(
        &mut l,
        &Patch {
            path: keys(&["camera", "@main"]),
            value: None,
        },
    )
    .unwrap();
    assert_eq!(undo.path, vec![Key::Table("camera".into()), Key::Index(0)]);
    save(&l).unwrap();
    assert_eq!(cams(&l), ["side", "top"]);
    assert!(!text(&dir, "rig.toml").contains("name = \"main\""));
    assert_eq!(text(&dir, "panel.toml"), own);
    apply(&mut l, &undo).unwrap();
    save(&l).unwrap();
    assert_eq!(cams(&l), ["main", "side", "top"]);
    assert_eq!(text(&dir, "panel.toml"), own);
    let restored = text(&dir, "rig.toml");
    assert!(restored.find("name = \"main\"").unwrap() < restored.find("name = \"side\"").unwrap());

    let added = apply(
        &mut l,
        &Patch {
            path: keys(&["light", "@desk"]),
            value: Some(Item::Table(
                "name = \"desk\"\nposition = [0.0, 0.2, 0.0]\nintensity = 2.0\n"
                    .parse::<toml_edit::DocumentMut>()
                    .unwrap()
                    .as_table()
                    .clone(),
            )),
        },
    );
    let removed = added.unwrap();
    save(&l).unwrap();
    assert!(text(&dir, "panel.toml").contains("name = \"desk\""));
    assert!(!text(&dir, "rig.toml").contains("desk"));
    assert_eq!(
        removed.path,
        vec![Key::Table("light".into()), Key::Index(1)]
    );
    apply(&mut l, &removed).unwrap();
    save(&l).unwrap();
    assert!(!text(&dir, "panel.toml").contains("desk"));
    assert_eq!(l.recipe.lights.len(), 1);
}

#[test]
fn the_window_refuses_to_edit_a_copy_and_edits_its_use() {
    let (dir, mut l) = panel("window-copy");
    let own = text(&dir, "panel.toml");
    let e = apply(&mut l, &set(&["object", "@left_cap", "radius"], "0.01")).unwrap_err();
    assert_eq!(e.path, "object[left_cap].radius");
    assert!(
        e.text.contains("use[left_]") && e.text.contains("parts/knob.toml"),
        "{e}"
    );
    apply(&mut l, &set(&["use", "@left_", "at"], "[-0.06, 0.05, 0.0]")).unwrap();
    save(&l).unwrap();
    assert_eq!(
        text(&dir, "panel.toml"),
        own.replace("at = [-0.05, 0.05, 0.0]", "at = [-0.06, 0.05, 0.0]")
    );
    assert_eq!(l.recipe.objects[1].at, [-0.06, 0.05, 0.0]);
}

#[test]
fn a_recipe_with_uses_and_no_includes_edits_as_one_file() {
    let part = "manfred = 1\nname = \"part\"\n\n[[object]]\nname = \"bit\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n";
    let main = format!(
        "{HEAD}{}\n[[use]]\nfile = \"part.toml\"\nprefix = \"a_\"\n",
        object("own")
    );
    let dir = scratch("uses-only", &[("part.toml", part), ("scene.toml", &main)]);
    let mut l = load(&dir.join("scene.toml")).unwrap();
    assert!(!l.recipe.files.included());
    assert_eq!(l.doc.to_string(), main);
    apply(&mut l, &set(&["object", "@own", "at"], "[1.0, 0.0, 0.0]")).unwrap();
    assert!(apply(&mut l, &set(&["object", "@a_bit", "at"], "[1.0, 0.0, 0.0]")).is_err());
    save(&l).unwrap();
    assert!(text(&dir, "scene.toml").contains("at = [1.0, 0.0, 0.0]"));
    assert_eq!(text(&dir, "part.toml"), part);
    assert_eq!(names(&l.recipe), ["own", "a_bit"]);
}

#[test]
fn an_outside_edit_to_an_included_file_wins_over_the_windows_copy() {
    let (dir, mut l) = panel("window-outside");
    apply(&mut l, &set(&["light", "@lamp", "intensity"], "30.0")).unwrap();
    save(&l).unwrap();
    let outside = text(&dir, "rig.toml").replace("exposure = 1.1", "exposure = 1.3");
    std::fs::write(dir.join("rig.toml"), &outside).unwrap();
    apply(&mut l, &set(&["light", "@lamp", "radius"], "0.03")).unwrap();
    save(&l).unwrap();
    let now = text(&dir, "rig.toml");
    assert!(now.contains("exposure = 1.3"), "{now}");
    assert!(
        now.contains("radius = 0.03") && now.contains("intensity = 30.0"),
        "{now}"
    );
    assert_eq!(l.recipe.look.as_ref().unwrap().exposure, 1.3);
}
