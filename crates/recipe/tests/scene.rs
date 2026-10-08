use manfred_recipe::{Problem, Recipe, apply_variant, parse, validate};

const CAMERAS: &str = "[[camera]]\nname = \"main\"\nposition = [0.0, 0.0, 1.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 30.0\n\n[[camera]]\nname = \"plan\"\nkind = \"orthographic\"\nposition = [0.0, 2.0, 0.0]\ntarget = [0.0, 0.0, 0.0]\nup = [0.0, 0.0, -1.0]\northo_height = 1.0\n";

fn recipe(text: &str) -> Recipe {
    parse(&format!("manfred = 1\nname = \"t\"\n{text}\n{CAMERAS}"))
        .unwrap_or_else(|e| panic!("{e}"))
}

fn problems(text: &str) -> Vec<String> {
    validate(&recipe(text))
        .iter()
        .map(Problem::to_string)
        .collect()
}

#[test]
fn the_scene_and_its_camera_are_read_from_the_output() {
    let r =
        recipe("[output]\nglb = \"t.glb\"\nscene = \"t.scene.toml\"\nscene_camera = \"plan\"\n");
    assert_eq!(validate(&r), Vec::new());
    assert_eq!(r.output.scene.as_deref(), Some("t.scene.toml"));
    assert_eq!(r.output.scene_camera.as_deref(), Some("plan"));
    let bare = recipe("[output]\nglb = \"t.glb\"\n");
    assert_eq!(bare.output.scene, None);
    assert_eq!(bare.output.scene_camera, None);
}

#[test]
fn a_scene_that_is_not_toml_and_a_camera_without_a_scene_or_a_name_are_problems() {
    assert_eq!(
        problems("[output]\nscene = \"t.json\"\n"),
        [
            "line 4: output.scene: `t.json` is not a TOML file, expected a path ending in `.scene.toml`"
        ]
    );
    assert_eq!(
        problems("[output]\nscene_camera = \"main\"\n"),
        [
            "line 4: output.scene_camera: names the scene's camera, but there is no `scene` to write, expected `output.scene`"
        ]
    );
    assert_eq!(
        problems("[output]\nscene = \"t.scene.toml\"\nscene_camera = \"side\"\n"),
        [
            "line 5: output.scene_camera: no camera named `side`, expected the name of a `[[camera]]`"
        ]
    );
}

#[test]
fn a_variant_writes_its_own_scene_before_the_scene_suffix() {
    let r = recipe(
        "[output]\nglb = \"out/t.glb\"\nscene = \"out/t.scene.toml\"\n\n[variant.bare]\ndrop = []\n\n[variant.dark]\ndrop = []\n\n[variant.dark.output]\nscene = \"dark/t.scene.toml\"\n",
    );
    assert_eq!(validate(&r), Vec::new());
    let bare = apply_variant(&r, "bare").unwrap();
    assert_eq!(bare.output.scene.as_deref(), Some("out/t-bare.scene.toml"));
    let dark = apply_variant(&r, "dark").unwrap();
    assert_eq!(dark.output.scene.as_deref(), Some("dark/t.scene.toml"));
    let none = recipe("[output]\nglb = \"t.glb\"\n\n[variant.bare]\ndrop = []\n");
    assert_eq!(apply_variant(&none, "bare").unwrap().output.scene, None);
}

#[test]
fn a_variant_scene_on_the_recipes_path_is_a_problem() {
    let found = problems(
        "[output]\nglb = \"t.glb\"\nscene = \"t.scene.toml\"\n\n[variant.bare]\ndrop = []\n\n[variant.bare.output]\nscene = \"t.scene.toml\"\n",
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("variant.bare.output: writes `t.scene.toml`"),
        "{found:?}"
    );
}

#[test]
fn an_unknown_output_key_still_names_the_scene_keys() {
    let error = parse("manfred = 1\nname = \"t\"\n[output]\nscenes = \"t.scene.toml\"\n")
        .unwrap_err()
        .to_string();
    assert!(error.contains("scene"), "{error}");
}

#[test]
fn the_scene_format_is_1_unless_the_output_asks_for_0() {
    assert_eq!(
        recipe("[output]\nscene = \"t.scene.toml\"\n")
            .output
            .scene_format,
        1
    );
    let zero = recipe("[output]\nscene = \"t.scene.toml\"\nscene_format = 0\n");
    assert_eq!(validate(&zero), Vec::new());
    assert_eq!(zero.output.scene_format, 0);
    assert_eq!(
        problems("[output]\nscene = \"t.scene.toml\"\nscene_format = 2\n"),
        [
            "line 5: output.scene_format: Manfred writes scene format 0 or 1, not 2, expected 1 (the default) or 0"
        ]
    );
    for format in [0, 1] {
        assert_eq!(
            problems(&format!("[output]\nscene_format = {format}\n")),
            [
                "line 4: output.scene_format: says how to write the scene, but there is no `scene` to write, expected `output.scene`"
            ]
        );
    }
    assert_eq!(validate(&recipe("[output]\nglb = \"t.glb\"\n")), Vec::new());
}

#[test]
fn an_object_takes_an_id_of_ten_crockford_characters_that_no_other_object_has() {
    let box_named = |name: &str, id: &str| {
        format!(
            "[[object]]\nname = \"{name}\"\nid = \"{id}\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n"
        )
    };
    let r = recipe(&format!(
        "{}\n[[object]]\nname = \"b\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n",
        box_named("a", "7k2m9q4xzr")
    ));
    assert_eq!(validate(&r), Vec::new());
    assert_eq!(r.objects[0].id.as_deref(), Some("7k2m9q4xzr"));
    assert_eq!(r.objects[1].id, None);
    assert_eq!(
        problems(&format!(
            "{}\n{}\n{}",
            box_named("a", "7k2m9q4xzr"),
            box_named("b", "7K2M9Q4XZR"),
            box_named("c", "7k2m9q4xz")
        )),
        [
            "line 11: object[b].id: `7K2M9Q4XZR` is not an id (an id takes 0-9 and a-z without i, l, o and u, not 'K'), expected 10 lowercase Crockford base32 characters, such as \"7k2m9q4xzr\"",
            "line 17: object[c].id: `7k2m9q4xz` is not an id (an id is 10 characters, not 9: lowercase Crockford base32), expected 10 lowercase Crockford base32 characters, such as \"7k2m9q4xzr\"",
        ]
    );
    assert_eq!(
        problems(&format!(
            "{}\n{}",
            box_named("a", "7k2m9q4xzr"),
            box_named("b", "7k2m9q4xzr")
        )),
        [
            "line 11: object[b].id: `7k2m9q4xzr` is also the id of `a`, expected an id no other object uses"
        ]
    );
}
