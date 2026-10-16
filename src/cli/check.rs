use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use manfred_build::{Registry, missing_materials, outlines};
use manfred_recipe::{Problem, Recipe, load, validate};

use super::{print_problems, read_failure};

#[derive(clap::Args)]
pub(super) struct Args {
    #[arg(value_name = "recipe", help = "The recipe to check")]
    recipe: String,
}

pub fn run(args: &Args, registry: &Registry) -> ExitCode {
    let code = check(
        &args.recipe,
        registry,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );
    ExitCode::from(code)
}

fn check(file: &str, registry: &Registry, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let loaded = match load(Path::new(file)) {
        Ok(loaded) => loaded,
        Err(e) => {
            let _ = writeln!(err, "manfred check: {}", read_failure(file, &e));
            return 1;
        }
    };
    let problems = problems(&loaded.recipe, file, registry);
    print_problems(out, file, &problems);
    u8::from(!problems.is_empty())
}

pub(super) fn problems(recipe: &Recipe, file: &str, registry: &Registry) -> Vec<Problem> {
    let mut problems = validate(recipe);
    if problems.is_empty() {
        problems = outlines(recipe);
    }
    if problems.is_empty() {
        let base = Path::new(file)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        problems = missing_materials(recipe, base, registry);
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = "crates/recipe/tests/fixtures";

    fn catalogue() -> Registry {
        let mut registry = Registry::new();
        let names = ["card", "default", "rubber", "steel", "anodised", "knob"];
        let text: String = names
            .iter()
            .map(|name| format!("[materials.{name}]\nbase = [0.5, 0.5, 0.5]\n"))
            .collect();
        registry.register_materials_toml(&text).unwrap();
        registry
    }

    fn run_on(args: &[&str]) -> (u8, String, String) {
        run_in(args, &Registry::new())
    }

    fn run_in(args: &[&str], registry: &Registry) -> (u8, String, String) {
        let file = match super::super::parsed(&[&["check"], args].concat()) {
            Ok(super::super::Verb::Check(args)) => args.recipe,
            Ok(_) => unreachable!(),
            Err(error) => return (2, String::new(), error),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = check(&file, registry, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn fixture(name: &str) -> String {
        format!("{FIXTURES}/{name}")
    }

    fn written(name: &str, text: &str) -> String {
        let dir = "tmp/check-tests";
        std::fs::create_dir_all(dir).unwrap();
        let file = format!("{dir}/{name}");
        std::fs::write(
            &file,
            format!(
                "manfred = 1\nname = \"t\"\n{text}\n[materials.plain]\nbase = [0.5, 0.5, 0.5]\n"
            ),
        )
        .unwrap();
        file
    }

    fn refused(name: &str, text: &str) -> String {
        let file = written(name, text);
        let (code, out, err) = run_on(&[&file]);
        assert_eq!((code, err.as_str()), (1, ""), "{out}");
        out.strip_prefix(&format!("{file}:")).unwrap().to_string()
    }

    const SLAB: &str = r#"
[[object]]
name = "slab"
kind = "outline"
material = "plain"
front = 0.0
back = 0.02
outline = { rect = [-0.1, -0.05, 0.1, 0.05], radius = 0.01 }
"#;

    #[test]
    fn a_clean_recipe_exits_0_and_prints_nothing() {
        for name in ["full.toml", "plate.toml", "driven.toml", "flat.toml"] {
            let (code, out, err) = run_in(&[&fixture(name)], &catalogue());
            assert_eq!((code, out.as_str(), err.as_str()), (0, "", ""), "{name}");
        }
    }

    #[test]
    fn problems_print_one_per_line_and_exit_1() {
        let file = fixture("errors/v_parent_loop.toml");
        let (code, out, err) = run_on(&[&file]);
        assert_eq!(code, 1);
        assert_eq!(err, "");
        assert_eq!(
            out,
            format!(
                "{file}:8: object[a].parent: the parents form a loop\n{file}:14: object[b].parent: the parents form a loop\n"
            )
        );
    }

    #[test]
    fn a_hole_on_the_edge_is_refused_as_the_build_refuses_it() {
        let text = r#"
[[object]]
name = "frame"
kind = "outline"
front = 0.0
back = 0.02
outline = { rect = [-0.1, -0.1, 0.1, 0.1] }
holes = [{ rect = [-0.05, -0.05, 0.1, 0.05] }]
"#;
        assert_eq!(
            refused("hole.toml", text),
            "10: object[frame].holes[0]: the hole touches or crosses the edge of `outline`; a hole lies inside the outline, clear of its edge\n"
        );
    }

    #[test]
    fn a_pocket_into_several_shapes_is_refused_as_the_build_refuses_it() {
        let text = r#"
[[object]]
name = "pair"
kind = "outline"
front = 0.0
back = 0.02
outline = { union = [{ rect = [-0.1, -0.05, -0.01, 0.05] }, { rect = [0.01, -0.05, 0.1, 0.05] }] }

[[object]]
name = "notch"
kind = "pocket"
into = "pair"
outline = { circle = [-0.05, 0.0, 0.01] }
depth = 0.01
"#;
        assert_eq!(
            refused("shapes.toml", text),
            "9: object[pair].outline: the outline makes 2 separate shapes, expected one\n"
        );
    }

    #[test]
    fn a_pocket_over_its_targets_edge_is_refused_as_the_build_refuses_it() {
        let text = format!(
            "{SLAB}{}",
            r#"
[[object]]
name = "notch"
kind = "pocket"
into = "slab"
outline = { rect = [0.08, -0.02, 0.12, 0.02] }
depth = 0.01
"#
        );
        assert_eq!(
            refused("edge.toml", &text),
            "16: object[notch].outline: the pocket reaches past the edge of `slab`, with 50.0% of its area outside it\n"
        );
        let inside = text.replace("0.12, 0.02", "0.09, 0.02");
        let file = written("inside.toml", &inside);
        assert_eq!(run_on(&[&file]), (0, String::new(), String::new()));
    }

    #[test]
    fn the_old_sky_preparation_spelling_is_refused_with_the_new_one() {
        let text = "[sky]\nkind = \"hdr\"\npath = \"sky.hdr\"\nsource = \"test\"\nprepare = \"softened\"\n";
        let file = written("prepare.toml", text);
        let (code, out, err) = run_on(&[&file]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert_eq!(
            err.strip_prefix(&format!("manfred check: {file}:"))
                .unwrap(),
            "7: sky.prepare: `prepare` is a table now, expected `[sky.prepare]` with `cap = 60.0` and `mean = 0.3` (and `balance = [r, g, b]` when the sky has one)\n"
        );
    }

    #[test]
    fn a_generator_object_passes_the_outline_pass() {
        let file = written(
            "generator.toml",
            "[[object]]\nname = \"birds\"\nkind = \"generator\"\ngenerator = \"app/birds\"\n",
        );
        assert_eq!(run_on(&[&file]), (0, String::new(), String::new()));
    }

    #[test]
    fn a_recipe_that_does_not_parse_exits_1_with_the_key_and_line() {
        let file = fixture("errors/unknown_key.toml");
        let (code, out, err) = run_on(&[&file]);
        assert_eq!(code, 1);
        assert_eq!(out, "");
        assert!(err.starts_with(&format!(
            "manfred check: {file}:8: object[a].colour: unknown key `colour` in `object[a]`"
        )));
    }

    #[test]
    fn a_toml_syntax_error_exits_1_with_its_line() {
        let file = fixture("errors/toml_syntax.toml");
        let (code, _, err) = run_on(&[&file]);
        assert_eq!(code, 1);
        assert_eq!(
            err,
            format!("manfred check: {file}:3: not valid TOML: extra `=`, expected nothing\n")
        );
    }

    #[test]
    fn a_file_that_cannot_be_read_exits_1() {
        let (code, out, err) = run_on(&["nowhere/none.toml"]);
        assert_eq!(code, 1);
        assert_eq!(out, "");
        assert_eq!(
            err,
            "manfred check: nowhere/none.toml: cannot read the recipe: no such file\n"
        );
    }

    fn built_text(file: &str, name: &str) -> Option<String> {
        let loaded = load(Path::new(file)).unwrap();
        manfred_scene::material(&loaded.recipe, name)
            .err()
            .map(|e| e.0)
    }

    fn assert_agree(file: &str, name: &str, line: usize, key: &str) {
        let (code, out, err) = run_on(&[file]);
        let built = built_text(file, name).unwrap();
        assert_eq!((code, err.as_str()), (1, ""), "{out}");
        let (head, text) = out.split_once(&format!(": {key}: ")).unwrap();
        assert_eq!(head, format!("{file}:{line}"));
        assert_eq!(
            built.strip_prefix(&format!("materials.{name}: ")).unwrap(),
            text.trim_end()
        );
    }

    #[test]
    fn a_wrong_material_key_is_refused_as_the_build_refuses_it() {
        let file = written(
            "emissive.toml",
            "[materials.lamp]\nbase = [1.0, 1.0, 1.0]\nemissive = [1, 0, 0]\n",
        );
        assert_agree(&file, "lamp", 5, "materials.lamp.emissive");
        let (_, out, _) = run_on(&[&file]);
        assert!(
            out.contains("unknown field `emissive`, expected one of `family`, "),
            "{out}"
        );
        assert!(out.contains("`emission`"), "{out}");
    }

    #[test]
    fn the_right_material_key_passes_check_and_the_build() {
        let file = written(
            "emission.toml",
            "[materials.lamp]\nbase = [1.0, 1.0, 1.0]\nemission = [1, 0, 0]\n",
        );
        assert_eq!(run_on(&[&file]), (0, String::new(), String::new()));
        assert_eq!(built_text(&file, "lamp"), None);
    }

    #[test]
    fn a_wrong_material_type_is_refused_as_the_build_refuses_it() {
        let file = written(
            "type.toml",
            "[materials.lamp]\nbase = [1.0, 1.0, 1.0]\nroughness = \"rough\"\n",
        );
        assert_agree(&file, "lamp", 5, "materials.lamp.roughness");
    }

    #[test]
    fn a_bad_key_in_an_included_files_material_is_refused_as_the_build_refuses_it() {
        let dir = "tmp/check-tests/include";
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            format!("{dir}/rig.toml"),
            "[materials.lamp]\nbase = [1.0, 1.0, 1.0]\nemissive = [1, 0, 0]\n",
        )
        .unwrap();
        let file = format!("{dir}/scene.toml");
        std::fs::write(
            &file,
            "manfred = 1\nname = \"t\"\ninclude = [\"rig.toml\"]\n",
        )
        .unwrap();
        let (code, out, err) = run_on(&[&file]);
        assert_eq!((code, err.as_str()), (1, ""), "{out}");
        assert!(
            out.contains("rig.toml:3: materials.lamp.emissive: unknown field `emissive`"),
            "{out}"
        );
        assert!(
            built_text(&file, "lamp")
                .unwrap()
                .contains("unknown field `emissive`")
        );
    }

    fn render_refusal(file: &str) -> String {
        let (recipe, built) =
            manfred_scene::load(Path::new(file), None, &[], &Registry::new()).unwrap();
        let pieces = manfred_scene::pieces(&built.parts);
        manfred_scene::palette(&recipe, &pieces, vec![false; pieces.len()])
            .err()
            .unwrap()
            .0
    }

    const SHELF: &str = "";
    const CRATE: &str = "\n[[object]]\nname = \"crate\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n";

    #[test]
    fn an_object_without_a_material_is_refused_as_render_refuses_it() {
        let file = written("bare.toml", &format!("{SHELF}{CRATE}"));
        let (code, out, err) = run_on(&[&file]);
        assert_eq!((code, err.as_str()), (1, ""), "{out}");
        let (head, text) = out.split_once(": object[crate].material: ").unwrap();
        assert_eq!(head, format!("{file}:4"));
        assert!(
            text.starts_with("material \"default\" is defined by neither"),
            "{text}"
        );
        assert!(text.contains("(it defines plain)"), "{text}");
        assert_eq!(
            render_refusal(&file),
            format!("object crate: {}", text.trim_end())
        );
    }

    #[test]
    fn an_undefined_material_name_is_refused_as_render_refuses_it() {
        let text = format!("{SHELF}{CRATE}material = \"chrome\"\n");
        let file = written("undefined.toml", &text);
        let (code, out, err) = run_on(&[&file]);
        assert_eq!((code, err.as_str()), (1, ""), "{out}");
        let (head, said) = out.split_once(": object[crate].material: ").unwrap();
        assert_eq!(head, format!("{file}:8"));
        assert!(said.contains("[materials.chrome]"), "{said}");
        assert_eq!(
            render_refusal(&file),
            format!("object crate: {}", said.trim_end())
        );
        let defined = text.replace("chrome", "plain");
        let file = written("defined.toml", &defined);
        assert_eq!(run_on(&[&file]), (0, String::new(), String::new()));
    }

    #[test]
    fn a_registry_that_defines_the_material_lets_check_pass() {
        let file = written(
            "catalogued.toml",
            &format!("{CRATE}material = \"chrome\"\n"),
        );
        let mut registry = Registry::new();
        registry
            .register_materials_toml("[materials.chrome]\nbase = [0.8, 0.8, 0.8]\n")
            .unwrap();
        assert_eq!(
            run_in(&[&file], &registry),
            (0, String::new(), String::new())
        );
        assert_eq!(run_on(&[&file]).0, 1);
    }

    #[test]
    fn a_hidden_pocket_text_or_generator_object_needs_no_material() {
        let text = format!(
            "{SLAB}{}",
            r#"
[[object]]
name = "notch"
kind = "pocket"
into = "slab"
outline = { rect = [-0.05, -0.02, 0.05, 0.02] }
depth = 0.01

[[object]]
name = "birds"
kind = "generator"
generator = "app/birds"

[[object]]
name = "ghost"
kind = "box"
size = [1.0, 1.0, 1.0]
hidden = true
"#
        );
        let file = written("exempt.toml", &text);
        assert_eq!(run_on(&[&file]), (0, String::new(), String::new()));
    }
}
