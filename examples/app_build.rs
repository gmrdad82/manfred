use std::process::ExitCode;

use manfred::build::{Call, Piece, Registry, SceneFacts};
use manfred::cli::{Command, Host};
use manfred::mesh::rounded_box;
use serde_json::{Value, json};

const MATERIALS: &str = r#"
[materials.cube]
base = [0.8, 0.3, 0.1]
roughness = 0.35
"#;

fn cube(call: &Call) -> Result<Vec<Piece>, String> {
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
    let mut mesh = rounded_box([side; 3], 0.0, 1);
    mesh.set_material("cube");
    Ok(vec![Piece {
        name: name.to_string(),
        mesh,
    }])
}

fn sidecar(facts: &SceneFacts) -> Result<Value, String> {
    let camera = facts
        .cameras
        .first()
        .ok_or("the app's sidecar wants a camera")?;
    Ok(json!({
        "app": 1,
        "eye": camera.position,
        "look_at": camera.target,
    }))
}

fn hello(args: &[String], registry: &Registry) -> ExitCode {
    println!("hello {} {}", args.join(" "), registry.names().count());
    ExitCode::SUCCESS
}

const HOST: Host = Host {
    beat: None,
    usage: "",
    commands: &[Command {
        name: "hello",
        usage: "app hello <words>...",
        run: hello,
    }],
};

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.register("app/cube", cube);
    registry.register_sidecar("app", sidecar);
    registry
        .register_materials_toml(MATERIALS)
        .expect("the app's materials are TOML");
    registry
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    manfred::cli::main_with(&args, &registry(), &HOST)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const FIXTURE: &str = "tests/fixtures/app/cube.toml";

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
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
    fn the_app_registers_its_generator_writer_and_materials() {
        let app = registry();
        let names: Vec<&str> = app
            .names()
            .filter(|name| !name.starts_with(manfred::build::BUILTIN))
            .collect();
        assert_eq!(names, ["app/cube"]);
        let writers: Vec<&str> = app.sidecar_names().collect();
        assert!(writers.contains(&"app") && writers.contains(&manfred::build::FACTS));
        assert_eq!(app.material_names().collect::<Vec<_>>(), ["cube"]);
    }

    #[test]
    fn the_app_build_binary_builds_and_checks_the_fixture() {
        let dir = scratch("app-build");
        let out = dir.to_string_lossy().into_owned();
        let built = manfred::cli::main(&strings(&["build", FIXTURE, "--out", &out]), &registry());
        assert_eq!(built, ExitCode::SUCCESS);
        let bytes = std::fs::read(dir.join("cube.glb")).unwrap();
        let summary = manfred::glb::summary(&bytes).unwrap();
        let names: Vec<&str> = summary.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["big_cube", "small_cube"]);
        let sidecar: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("cube.json")).unwrap()).unwrap();
        assert_eq!(
            sidecar,
            json!({"app": 1, "eye": [0.0, 1.0, 6.0], "look_at": [0.0, 0.0, 0.0], "note": "kept"})
        );
        assert_eq!(
            manfred::cli::main(&strings(&["check", FIXTURE]), &registry()),
            ExitCode::SUCCESS
        );
        assert_eq!(
            manfred::cli::main(
                &strings(&["build", FIXTURE, "--out", &out]),
                &Registry::new()
            ),
            ExitCode::FAILURE
        );
    }

    #[test]
    fn the_catalogue_names_the_material_the_scene_draws() {
        let recipe = manfred::recipe::load(std::path::Path::new(FIXTURE))
            .unwrap()
            .recipe;
        let built = manfred::build::build_with(
            &recipe,
            std::path::Path::new("tests/fixtures/app"),
            &registry(),
        )
        .unwrap();
        let drawn = built.recipe(&recipe);
        assert_eq!(drawn.materials.keys().collect::<Vec<_>>(), ["cube"]);
        assert_eq!(drawn.materials["cube"]["roughness"], json!(0.35));
        assert!(built.notes.is_empty());
    }

    #[test]
    fn the_app_adds_its_own_command() {
        assert_eq!(
            manfred::cli::main_with(&strings(&["hello", "there"]), &registry(), &HOST),
            ExitCode::SUCCESS
        );
    }
}
