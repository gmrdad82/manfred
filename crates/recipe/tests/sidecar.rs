use manfred_recipe::{Recipe, apply_variant, parse, validate};
use serde_json::json;

const HEAD: &str = r#"manfred = 1
name = "desk"

[plate]
width = 100
height = 50
metres_per_pixel = 0.01
origin = "top_left"

[output]
glb = "scene/desk.glb"
sidecar = "scene/desk.json"
sidecar_dialect = "facts"

[[camera]]
name = "main"
position = [0.0, 0.0, 1.0]
target = [0.0, 0.0, 0.0]
fov_y_deg = 20.0

[sun]
model = "authored"
toward = [0.0, 1.0, 0.0]
irradiance = 4.5

[[object]]
name = "mv_key"
kind = "box"
size = [0.1, 0.1, 0.1]
material = "m"

[[object]]
name = "mv_page"
kind = "box"
size = [0.1, 0.1, 0.1]
material = "m"

[[object]]
name = "slat0"
kind = "box"
size = [0.1, 0.1, 0.1]
material = "m"
"#;

fn recipe(tail: &str) -> Recipe {
    parse(&format!("{HEAD}{tail}")).unwrap()
}

fn error(tail: &str) -> String {
    parse(&format!("{HEAD}{tail}")).unwrap_err().to_string()
}

fn problems(r: &Recipe) -> Vec<String> {
    validate(r)
        .into_iter()
        .map(|p| format!("{}: {}", p.path, p.text))
        .collect()
}

#[test]
fn a_mover_in_plate_pixels_equals_the_same_mover_in_metres() {
    let pixels = recipe(
        r#"
[[mover]]
group = "page"
pivot_px = [20, 10, -0.005]
axis = [0.0, 1.0, 0.0]
travel_px = 1
clip_px = [0, 1, 0, 5]
clip2_px = [0, -1, 0, 40]
"#,
    );
    let metres = recipe(
        r#"
[[mover]]
group = "page"
pivot = [-0.3, 0.15, -0.005]
axis = [0.0, 1.0, 0.0]
travel = 0.01
clip = [0.0, 1.0, 0.0, 0.2]
clip2 = [0.0, -1.0, 0.0, 0.15]
"#,
    );
    assert_eq!(pixels.movers, metres.movers);
    assert_eq!(validate(&pixels), []);
}

#[test]
fn a_pivot_in_pixels_takes_its_depth_as_a_third_number_and_defaults_to_zero() {
    let r = recipe(
        r#"
[[mover]]
group = "key"
pivot_px = [60, 25]
axis = [0.0, 0.0, -1.0]
travel = 0.0045
"#,
    );
    assert_eq!(r.movers[0].pivot, [0.1, 0.0, 0.0]);
}

#[test]
fn a_mover_names_its_pivot_and_travel_one_way() {
    let both = error(
        r#"
[[mover]]
group = "key"
pivot = [0.0, 0.0, 0.0]
pivot_px = [0, 0]
axis = [0.0, 0.0, -1.0]
travel = 0.1
"#,
    );
    assert!(
        both.contains("`pivot` and `pivot_px` are both set"),
        "{both}"
    );
    let travel = error(
        r#"
[[mover]]
group = "key"
pivot = [0.0, 0.0, 0.0]
axis = [0.0, 0.0, -1.0]
travel = 0.1
travel_px = 1
"#,
    );
    assert!(
        travel.contains("`travel` and `travel_px` are both set"),
        "{travel}"
    );
    let none = error(
        r#"
[[mover]]
group = "key"
axis = [0.0, 0.0, -1.0]
travel = 0.1
"#,
    );
    assert!(
        none.contains("mover[key].pivot: missing, expected an array of 3 numbers, or `pivot_px`"),
        "{none}"
    );
    let none = error(
        r#"
[[mover]]
group = "key"
pivot = [0.0, 0.0, 0.0]
axis = [0.0, 0.0, -1.0]
"#,
    );
    assert!(
        none.contains("mover[key].travel: missing, expected a number, or `travel_px`"),
        "{none}"
    );
}

#[test]
fn a_clip_plane_in_pixels_runs_across_the_plates_rows() {
    let slanted = error(
        r#"
[[mover]]
group = "key"
pivot = [0.0, 0.0, 0.0]
axis = [0.0, 0.0, -1.0]
travel = 0.1
clip_px = [1, 1, 0, 5]
"#,
    );
    assert!(
        slanted.contains("mover[key].clip_px: expected a normal along the plate's y axis"),
        "{slanted}"
    );
    let both = error(
        r#"
[[mover]]
group = "key"
pivot = [0.0, 0.0, 0.0]
axis = [0.0, 0.0, -1.0]
travel = 0.1
clip = [0.0, 1.0, 0.0, 0.0]
clip_px = [0, 1, 0, 5]
"#,
    );
    assert!(both.contains("`clip` and `clip_px` are both set"), "{both}");
}

#[test]
fn a_twin_without_a_plate_is_a_problem_naming_the_twin() {
    let r = parse(
        r#"manfred = 1
name = "t"

[[object]]
name = "mv_key"
kind = "box"
size = [0.1, 0.1, 0.1]

[[mover]]
group = "key"
pivot_px = [1, 2]
axis = [0.0, 0.0, -1.0]
travel_px = 1
"#,
    )
    .unwrap();
    let found = problems(&r);
    assert!(
        found.iter().any(|p| p.starts_with("mover[key].pivot_px: ")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|p| p.starts_with("mover[key].travel_px: ")),
        "{found:?}"
    );
}

#[test]
fn the_sidecar_table_is_read_as_json_in_the_recipes_units() {
    let r = recipe(
        r#"
[sidecar.motion]
cup = [0.28, -0.12, 0.1]
radius = 0.016
density = 1.8
"#,
    );
    assert_eq!(
        serde_json::Value::Object(r.sidecar.clone()),
        json!({"motion": {"cup": [0.28, -0.12, 0.1], "radius": 0.016, "density": 1.8}})
    );
    assert_eq!(validate(&r), []);
}

#[test]
fn a_sidecar_table_with_no_dialect_has_nowhere_to_go() {
    let r = parse("manfred = 1\nname = \"t\"\n[sidecar.motion]\ndensity = 1.0\n").unwrap();
    let found = problems(&r);
    assert!(
        found
            .iter()
            .any(|p| p.starts_with("sidecar: `[sidecar]` has no file to go in")),
        "{found:?}"
    );
    let flat = error("sidecar = 3\n");
    assert!(flat.contains("sidecar"), "{flat}");
}

fn with_variant(variant: &str) -> Recipe {
    recipe(&format!(
        "[sidecar.motion]\ndensity = 1.8\n\n[sidecar.extra]\nkeep = true\n{variant}"
    ))
}

#[test]
fn a_variant_names_its_own_files_beside_the_recipes() {
    let r = with_variant("[variant.bare]\ndrop = [\"slat*\"]\n");
    let bare = apply_variant(&r, "bare").unwrap();
    assert_eq!(bare.output.glb.as_deref(), Some("scene/desk-bare.glb"));
    assert_eq!(bare.output.sidecar.as_deref(), Some("scene/desk-bare.json"));
    assert_eq!(bare.output.bake, None);
    assert_eq!(r.output.glb.as_deref(), Some("scene/desk.glb"));
    assert_eq!(validate(&r), []);
}

#[test]
fn a_variants_own_output_table_wins_over_the_naming_rule() {
    let r = with_variant(
        "[variant.bare]\ndrop = [\"slat*\"]\n[variant.bare.output]\nglb = \"other/plain.glb\"\nsidecar = \"other/plain.json\"\n",
    );
    let bare = apply_variant(&r, "bare").unwrap();
    assert_eq!(bare.output.glb.as_deref(), Some("other/plain.glb"));
    assert_eq!(bare.output.sidecar.as_deref(), Some("other/plain.json"));
}

#[test]
fn a_variant_without_a_glb_in_the_recipe_still_gets_a_file_of_its_own() {
    let r = parse(
        "manfred = 1\nname = \"t\"\n[[object]]\nname = \"a\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n[variant.lean]\ndrop = [\"a\"]\n",
    )
    .unwrap();
    let lean = apply_variant(&r, "lean").unwrap();
    assert_eq!(lean.output.glb.as_deref(), Some("t-lean.glb"));
    assert_eq!(lean.output.sidecar, None);
}

#[test]
fn a_variant_removes_and_sets_sidecar_keys() {
    let r = with_variant(
        "[variant.bare]\nremove = [\"sidecar.motion\"]\n[variant.bare.set]\n\"sidecar.extra.keep\" = false\n\"sidecar.extra.more\" = [1, 2]\n\"sun.irradiance\" = 0.0\n",
    );
    let bare = apply_variant(&r, "bare").unwrap();
    assert_eq!(
        serde_json::Value::Object(bare.sidecar.clone()),
        json!({"extra": {"keep": false, "more": [1, 2]}})
    );
    assert_eq!(bare.sun.as_ref().unwrap().irradiance, 0.0);
    assert!(r.sidecar.contains_key("motion"));
}

#[test]
fn a_variant_removing_a_sidecar_key_that_is_not_there_is_an_error() {
    let r = with_variant("[variant.bare]\nremove = [\"sidecar.nothing\"]\n");
    let found = problems(&r);
    assert!(
        found
            .iter()
            .any(|p| p.starts_with("variant.bare.remove: ") && p.contains("`sidecar.nothing`")),
        "{found:?}"
    );
}

#[test]
fn a_variant_that_writes_a_file_another_writes_is_a_problem() {
    let same = with_variant(
        "[variant.bare]\ndrop = [\"slat*\"]\n[variant.bare.output]\nglb = \"scene/desk.glb\"\n",
    );
    let found = problems(&same);
    assert!(
        found
            .iter()
            .any(|p| p.starts_with("variant.bare.output: writes `scene/desk.glb`")),
        "{found:?}"
    );
    let twins = with_variant(
        "[variant.a]\ndrop = [\"slat*\"]\n[variant.a.output]\nglb = \"x.glb\"\n[variant.b]\ndrop = [\"slat*\"]\n[variant.b.output]\nglb = \"x.glb\"\n",
    );
    let found = problems(&twins);
    assert!(
        found
            .iter()
            .any(|p| p.starts_with("variant.b.output: writes `x.glb`")),
        "{found:?}"
    );
    assert!(!found.iter().any(|p| p.starts_with("variant.a.output")));
}

#[test]
fn a_variant_without_its_own_bake_drops_the_recipes_bake() {
    let r = recipe(
        r#"
[bake]
product = "sample"
anchors = [12.0]
samples = 4
[[bake.volume]]
name = "all"
min = [0.0, 0.0, 0.0]
max = [1.0, 1.0, 1.0]
spacing = 0.5

[variant.bare]
drop = ["slat*"]
"#,
    );
    let with_bake = parse(
        &r.source
            .replace("[output]\n", "[output]\nbake = \"bake.toml\"\n"),
    )
    .unwrap();
    assert_eq!(with_bake.output.bake.as_deref(), Some("bake.toml"));
    let bare = apply_variant(&with_bake, "bare").unwrap();
    assert_eq!(bare.output.bake, None);
    assert!(bare.bake.is_none());
    assert_eq!(validate(&with_bake), []);
}
