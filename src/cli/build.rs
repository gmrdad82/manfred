use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use manfred_build::{Registry, build_with, write};
use manfred_recipe::{Error, Loaded, Recipe, apply_set, apply_variant, load, validate};

use super::{pair, print_problems, read_failure};

#[derive(clap::Args)]
pub(super) struct Args {
    #[arg(value_name = "recipe", help = "The recipe to build")]
    recipe: String,
    #[arg(
        long,
        value_name = "name",
        conflicts_with = "all_variants",
        help = "Build this variant instead of the recipe"
    )]
    variant: Option<String>,
    #[arg(long, help = "Build the recipe and then every variant")]
    all_variants: bool,
    #[arg(
        long,
        value_name = "key=value",
        value_parser = pair,
        help = "Replace a value the recipe has; repeatable"
    )]
    set: Vec<String>,
    #[arg(
        long,
        value_name = "dir",
        help = "Write every file into this folder instead of the recipe's [output] paths"
    )]
    out: Option<PathBuf>,
}

fn recipes(loaded: Loaded, args: &Args) -> Result<Vec<Recipe>, Error> {
    let mut recipes = Vec::new();
    match &args.variant {
        Some(name) => recipes.push(apply_variant(&loaded.recipe, name)?),
        None => {
            if args.all_variants {
                for name in loaded.recipe.variants.keys() {
                    recipes.push(apply_variant(&loaded.recipe, name)?);
                }
            }
            recipes.insert(0, loaded.recipe);
        }
    }
    recipes
        .iter()
        .map(|recipe| apply_set(recipe, &args.set))
        .collect()
}

fn unbaked(bake: &Path) -> String {
    format!(
        "{} is the bake recipe; baking it needs the engine's `pfx bake`, which is not on the PATH",
        bake.display()
    )
}

fn run_with(
    args: &Args,
    registry: &Registry,
    pfx: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let file = args.recipe.as_str();
    let recipes = match load(Path::new(file)).and_then(|loaded| recipes(loaded, args)) {
        Ok(recipes) => recipes,
        Err(e) => {
            let _ = writeln!(err, "manfred build: {}", read_failure(file, &e));
            return 1;
        }
    };
    for recipe in &recipes {
        let problems = validate(recipe);
        if !problems.is_empty() {
            print_problems(out, file, &problems);
            return 1;
        }
    }
    let base = Path::new(file)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    for recipe in &recipes {
        let written = build_with(recipe, base, registry).and_then(|built| {
            for note in &built.notes {
                let _ = writeln!(err, "{file}: {note}");
            }
            write(&built, recipe, base, args.out.as_deref())
        });
        match written {
            Ok(paths) => {
                for path in paths {
                    let _ = writeln!(out, "{}", path.display());
                    if !pfx && path.file_name().is_some_and(|name| name == "bake.toml") {
                        let _ = writeln!(err, "manfred build: {}", unbaked(&path));
                    }
                }
            }
            Err(e) => {
                let _ = writeln!(err, "manfred build: {file}: {e}");
                return 1;
            }
        }
    }
    0
}

pub fn run(args: &Args, registry: &Registry) -> ExitCode {
    ExitCode::from(run_with(
        args,
        registry,
        super::process::on_path("pfx"),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> Registry {
        let mut registry = Registry::new();
        registry.register("app/crate", crate_piece);
        registry
    }

    fn run_on(args: &[&str]) -> (u8, String, String) {
        run_with_registry(args, &app())
    }

    fn line(args: &[&str]) -> Result<Args, String> {
        match super::super::parsed(&[&["build"], args].concat())? {
            super::super::Verb::Build(args) => Ok(args),
            _ => Err("not build".into()),
        }
    }

    fn run_with_registry(args: &[&str], registry: &Registry) -> (u8, String, String) {
        let args = match line(args) {
            Ok(args) => args,
            Err(error) => return (2, String::new(), error),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_with(&args, registry, true, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn listed(dir: &Path, names: &[&str]) -> String {
        names
            .iter()
            .map(|name| format!("{}\n", dir.join(name).display()))
            .collect()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn without_pfx_a_written_bake_recipe_says_what_bakes_it() {
        let dir = scratch("build-no-pfx");
        let recipe = dir.join("small.toml");
        std::fs::copy("crates/build/tests/sky/small.toml", &recipe).unwrap();
        let args = line(&[recipe.to_str().unwrap()]).unwrap();
        let bake = dir.join("bake.toml");
        let mut registry = Registry::new();
        registry
            .register_materials_toml(
                "[materials.wood]\nbase = [0.4, 0.25, 0.12]\nroughness = 0.6\n",
            )
            .unwrap();
        for (pfx, note) in [
            (false, format!("manfred build: {}\n", unbaked(&bake))),
            (true, String::new()),
        ] {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = run_with(&args, &registry, pfx, &mut out, &mut err);
            let printed = String::from_utf8(out).unwrap();
            assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
            assert!(
                printed.lines().any(|line| Path::new(line) == bake),
                "{printed}"
            );
            assert_eq!(String::from_utf8(err).unwrap(), note);
        }
        assert!(bake.is_file());
    }

    #[test]
    fn the_table_builds_into_the_out_folder_and_prints_each_path() {
        let dir = scratch("build-table");
        let out = dir.to_string_lossy().into_owned();
        let (code, printed, err) = run_on(&["tests/golden/table/table.toml", "--out", &out]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert_eq!(printed, listed(&dir, &["table.glb", "table.json"]));
        let golden = std::fs::read("tests/golden/table/table.glb").unwrap();
        assert_eq!(std::fs::read(dir.join("table.glb")).unwrap(), golden);
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(names, ["table.glb", "table.json"]);
    }

    #[test]
    fn problems_print_as_check_prints_them_and_exit_1() {
        let file = "crates/recipe/tests/fixtures/errors/v_parent_loop.toml";
        let (code, out, err) = run_on(&[file]);
        assert_eq!((code, err.as_str()), (1, ""));
        assert!(out.starts_with(&format!(
            "{file}:8: object[a].parent: the parents form a loop\n"
        )));
    }

    #[test]
    fn read_and_build_errors_refuse_in_one_line_and_exit_1() {
        let (code, out, err) = run_on(&["nowhere/none.toml"]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert_eq!(
            err,
            "manfred build: nowhere/none.toml: cannot read the recipe: no such file\n"
        );
        let dir = scratch("build-plate");
        let out_dir = dir.to_string_lossy().into_owned();
        let file = "crates/recipe/tests/fixtures/full.toml";
        let (code, out, err) = run_on(&[file, "--out", &out_dir]);
        let missing =
            std::fs::read(Path::new(file).with_file_name("parts/handle.glb")).unwrap_err();
        assert_eq!((code, out.as_str()), (1, ""));
        assert_eq!(
            err,
            format!(
                "manfred build: {file}: object[imported].path: cannot read parts/handle.glb: {missing}\n"
            )
        );
    }

    #[test]
    fn all_variants_builds_the_recipe_and_each_variant_in_one_call() {
        let dir = scratch("build-all-variants");
        let out = dir.to_string_lossy().into_owned();
        let (code, printed, err) = run_on(&[
            "tests/golden/toybox/toybox.toml",
            "--all-variants",
            "--out",
            &out,
        ]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert_eq!(
            printed,
            listed(
                &dir,
                &[
                    "toybox.glb",
                    "toybox.json",
                    "toybox-bare.glb",
                    "toybox-bare.json"
                ]
            )
        );
        for name in [
            "toybox.glb",
            "toybox.json",
            "toybox-bare.glb",
            "toybox-bare.json",
        ] {
            assert_eq!(
                std::fs::read(dir.join(name)).unwrap(),
                std::fs::read(format!("tests/golden/toybox/{name}")).unwrap(),
                "{name}"
            );
        }
    }

    #[test]
    fn one_variant_writes_only_its_own_files() {
        let dir = scratch("build-one-variant");
        let out = dir.to_string_lossy().into_owned();
        let (code, printed, err) = run_on(&[
            "tests/golden/toybox/toybox.toml",
            "--variant",
            "bare",
            "--out",
            &out,
        ]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert_eq!(
            printed,
            listed(&dir, &["toybox-bare.glb", "toybox-bare.json"])
        );
    }

    #[test]
    fn all_variants_of_a_recipe_with_none_builds_just_the_recipe() {
        let dir = scratch("build-all-none");
        let out = dir.to_string_lossy().into_owned();
        let (code, printed, err) = run_on(&[
            "tests/golden/table/table.toml",
            "--all-variants",
            "--out",
            &out,
        ]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert_eq!(printed, listed(&dir, &["table.glb", "table.json"]));
    }

    #[test]
    fn an_unknown_variant_exits_1() {
        let (code, _, err) = run_on(&["tests/golden/table/table.toml", "--variant", "nope"]);
        assert_eq!(code, 1);
        assert!(err.contains("variant.nope: no such variant"), "{err}");
    }

    #[test]
    fn a_line_it_cannot_parse_is_clap_s_error() {
        for args in [
            &[][..],
            &["a.toml", "b.toml"][..],
            &["a.toml", "--out"][..],
            &["a.toml", "--variant", "a", "--variant", "b"][..],
            &["a.toml", "--all-variants", "--variant", "bare"][..],
            &["a.toml", "--set", "side"][..],
            &["a.toml", "--nope"][..],
        ] {
            let (code, out, err) = run_on(args);
            assert_eq!((code, out.as_str()), (2, ""), "{args:?}");
            assert!(err.starts_with("error: "), "{err}");
            assert!(
                err.ends_with("For more information, try '--help'.\n"),
                "{err}"
            );
        }
    }

    fn crate_piece(call: &manfred_build::Call) -> Result<Vec<manfred_build::Piece>, String> {
        let name = call
            .params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("wants a `name`")?;
        let side = call
            .params
            .get("side")
            .map_or(Some(1.0), |v| v.as_f64())
            .ok_or("`side` wants a number")?;
        if side <= 0.0 {
            return Err(format!("`side` must be above 0, found {side}"));
        }
        let mut mesh = manfred_mesh::rounded_box([side; 3], 0.0, 1);
        mesh.set_material("wood");
        Ok(vec![manfred_build::Piece {
            name: name.to_string(),
            mesh,
        }])
    }

    const FIXTURE: &str = "tests/fixtures/app/crates.toml";

    fn width_of(dir: &std::path::Path, node: &str) -> f64 {
        let bytes = std::fs::read(dir.join("crates.glb")).unwrap();
        let summary = manfred_glb::summary(&bytes).unwrap();
        let found = summary.nodes.iter().find(|n| n.name == node).unwrap();
        found.bounds[1][0] - found.bounds[0][0]
    }

    #[test]
    fn a_set_changes_what_the_recipe_reads_and_leaves_its_file_alone() {
        let before = std::fs::read(FIXTURE).unwrap();
        let dir = scratch("build-set");
        let out = dir.to_string_lossy().into_owned();
        let (code, printed, err) = run_on(&[
            FIXTURE,
            "--set",
            "object.left.params.side=0.5",
            "--set",
            "object.floor.size=[2.0, 0.1, 2.0]",
            "--out",
            &out,
        ]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert_eq!(printed, listed(&dir, &["crates.glb"]));
        assert!((width_of(&dir, "left_crate") - 0.5).abs() < 1e-6);
        assert!((width_of(&dir, "right_crate") - 1.0 * 2f64.sqrt()).abs() < 1e-6);
        assert!((width_of(&dir, "floor") - 2.0).abs() < 1e-6);
        let (code, _, err) = run_on(&[
            FIXTURE,
            "--set",
            "object.left.params.side=-1",
            "--out",
            &out,
        ]);
        assert_eq!(code, 1);
        assert!(err.contains("`side` must be above 0, found -1"), "{err}");
        assert_eq!(std::fs::read(FIXTURE).unwrap(), before);
    }

    #[test]
    fn a_set_of_an_unknown_key_names_it_and_exits_1() {
        let before = std::fs::read(FIXTURE).unwrap();
        for (set, key) in [
            ("sky.intensty=0.9", "sky.intensty"),
            (
                "object.left.params.exposure=1.5",
                "object.left.params.exposure",
            ),
        ] {
            let (code, out, err) = run_on(&[FIXTURE, "--set", set, "--out", "tmp/never"]);
            assert_eq!((code, out.as_str()), (1, ""), "{set}");
            assert!(
                err.starts_with(&format!(
                    "manfred build: {FIXTURE}: {key}: --set: the recipe has no value at `{key}`"
                )),
                "{err}"
            );
        }
        let (code, _, err) = run_on(&[FIXTURE, "--set", "object.left.params.side"]);
        assert_eq!(code, 2);
        assert!(err.contains("expected <key>=<value>"), "{err}");
        assert_eq!(std::fs::read(FIXTURE).unwrap(), before);
    }

    #[test]
    fn a_registry_that_knows_the_generator_builds_the_fixture() {
        let dir = scratch("build-generator");
        let out = dir.to_string_lossy().into_owned();
        let (code, printed, err) = run_on(&[FIXTURE, "--out", &out]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert_eq!(printed, listed(&dir, &["crates.glb"]));
        let bytes = std::fs::read(dir.join("crates.glb")).unwrap();
        let names: Vec<String> = manfred_glb::summary(&bytes)
            .unwrap()
            .nodes
            .into_iter()
            .map(|n| n.name)
            .collect();
        assert_eq!(names, ["floor", "left_crate", "right_crate"]);
    }

    #[test]
    fn manfred_alone_refuses_a_generator_only_an_app_knows() {
        let (code, out, err) =
            run_with_registry(&[FIXTURE, "--out", "tmp/never"], &Registry::new());
        assert_eq!((code, out.as_str()), (1, ""));
        assert_eq!(
            err,
            format!(
                "manfred build: {FIXTURE}: object[left].generator: `app/crate` is built by the app's own build binary, which registers it through the Manfred library\n"
            )
        );
    }
}
