use manfred_build::Registry;
use manfred_glb::{Summary, diff, summary};
use manfred_recipe::Recipe;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::size;

const FLOOR_DB: f64 = 35.0;
const FRAME: [u32; 2] = [1586, 992];
const OUT: &str = "tmp/diff";

fn decibel(text: &str) -> Result<f64, String> {
    text.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| "not a number of dB".to_string())
}

#[derive(clap::Args)]
pub(super) struct Args {
    #[arg(value_name = "a.glb", help = "The first GLB")]
    a: String,
    #[arg(value_name = "b.glb", help = "The second GLB")]
    b: String,
    #[arg(long, help = "Print the report as JSON")]
    json: bool,
    #[arg(
        long,
        requires = "recipe",
        help = "Also draw both alike through a recipe and compare the pictures by PSNR"
    )]
    render: bool,
    #[arg(
        long,
        value_name = "recipe",
        requires = "render",
        help = "The recipe whose cameras, lights and look draw both"
    )]
    recipe: Option<PathBuf>,
    #[arg(
        long,
        value_name = "name",
        requires = "render",
        help = "The camera to draw through [default: the recipe's first]"
    )]
    camera: Option<String>,
    #[arg(
        long,
        value_name = "WxH",
        requires = "render",
        value_parser = size,
        help = "The pictures' size [default: the recipe's plate, or 1586x992]"
    )]
    size: Option<[u32; 2]>,
    #[arg(
        long,
        value_name = "dB",
        requires = "render",
        value_parser = decibel,
        help = "The least PSNR that passes [default: 35]"
    )]
    floor: Option<f64>,
}

#[derive(Debug, PartialEq)]
struct Render {
    recipe: PathBuf,
    camera: Option<String>,
    size: Option<[u32; 2]>,
    floor: f64,
}

struct Compare {
    files: [String; 2],
    json: bool,
    render: Option<Render>,
}

impl Args {
    fn compare(&self) -> Compare {
        Compare {
            files: [self.a.clone(), self.b.clone()],
            json: self.json,
            render: self.recipe.clone().map(|recipe| Render {
                recipe,
                camera: self.camera.clone(),
                size: self.size,
                floor: self.floor.unwrap_or(FLOOR_DB),
            }),
        }
    }
}

fn load(path: &str) -> Result<Summary, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("manfred diff: {path}: {e}"))?;
    summary(&bytes).map_err(|e| format!("manfred diff: {path}: {e}"))
}

struct Drawn {
    psnr: f64,
    pictures: [PathBuf; 3],
}

fn merged(render: &Render, registry: &Registry) -> Result<Recipe, String> {
    manfred_scene::load(&render.recipe, None, &[], registry)
        .map(|(recipe, _)| recipe)
        .map_err(|e| format!("manfred diff: {}: {e}", render.recipe.display()))
}

fn draw(
    files: &[String; 2],
    render: &Render,
    registry: &Registry,
    out: &Path,
) -> Result<Drawn, String> {
    let fail = |e: manfred_scene::Error| format!("manfred diff: {e}");
    let recipe = merged(render, registry)?;
    let camera = match &render.camera {
        Some(name) => name.clone(),
        None => recipe
            .cameras
            .first()
            .map(|c| c.name.clone())
            .ok_or_else(|| {
                format!(
                    "manfred diff: {}: the recipe has no cameras",
                    render.recipe.display()
                )
            })?,
    };
    let size = render.size.unwrap_or(match &recipe.plate {
        Some(plate) if plate.width >= 1.0 && plate.height >= 1.0 => {
            [plate.width as u32, plate.height as u32]
        }
        _ => FRAME,
    });
    let gpu = manfred_scene::headless().map_err(fail)?;
    let mut images = Vec::with_capacity(2);
    for file in files {
        let bytes = std::fs::read(file).map_err(|e| format!("manfred diff: {file}: {e}"))?;
        let parts =
            manfred_scene::glb_parts(&bytes).map_err(|e| format!("manfred diff: {file}: {e}"))?;
        let rgba = manfred_scene::shoot(&gpu, &recipe, &parts, &camera, size)
            .map_err(|e| format!("manfred diff: {file}: {e}"))?;
        images.push(rgba);
    }
    let psnr = manfred_scene::psnr(&images[0], &images[1]).map_err(fail)?;
    let pictures = [
        out.join("a.png"),
        out.join("b.png"),
        out.join("difference.png"),
    ];
    let difference = manfred_scene::difference(&images[0], &images[1]);
    for (path, rgba) in pictures.iter().zip([&images[0], &images[1], &difference]) {
        manfred_scene::write_png(path, size, rgba).map_err(fail)?;
    }
    Ok(Drawn { psnr, pictures })
}

fn decibels(psnr: f64) -> String {
    if psnr.is_finite() {
        format!("{psnr:.2} dB")
    } else {
        "inf dB (identical)".into()
    }
}

fn attach(json: &mut serde_json::Value, drawn: &Drawn, floor: f64) {
    let Some(object) = json.as_object_mut() else {
        return;
    };
    let psnr = if drawn.psnr.is_finite() {
        serde_json::json!(drawn.psnr)
    } else {
        serde_json::json!("inf")
    };
    object.insert(
        "render".into(),
        serde_json::json!({
            "psnr_db": psnr,
            "floor_db": floor,
            "a": drawn.pictures[0].display().to_string(),
            "b": drawn.pictures[1].display().to_string(),
            "difference": drawn.pictures[2].display().to_string(),
        }),
    );
}

pub fn run(args: &Args, registry: &Registry) -> ExitCode {
    let args = args.compare();
    let (a, b) = match (load(&args.files[0]), load(&args.files[1])) {
        (Ok(a), Ok(b)) => (a, b),
        (a, b) => {
            for message in [a.err(), b.err()].into_iter().flatten() {
                eprintln!("{message}");
            }
            return ExitCode::FAILURE;
        }
    };
    let report = diff(&a, &b);
    let drawn = match &args.render {
        Some(render) => match draw(&args.files, render, registry, Path::new(OUT)) {
            Ok(drawn) => Some((drawn, render.floor)),
            Err(message) => {
                eprintln!("{message}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    if args.json {
        let mut json = report.json();
        if let Some((drawn, floor)) = &drawn {
            attach(&mut json, drawn, *floor);
        }
        let text = serde_json::to_string_pretty(&json).unwrap_or_default();
        println!("{text}");
    } else {
        print!("{report}");
        if let Some((drawn, floor)) = &drawn {
            println!(
                "render: PSNR {} against a floor of {floor} dB; {}, {}, {}",
                decibels(drawn.psnr),
                drawn.pictures[0].display(),
                drawn.pictures[1].display(),
                drawn.pictures[2].display()
            );
        }
    }
    let failed = match &drawn {
        Some((drawn, floor)) => drawn.psnr < *floor,
        None => report.differs(),
    };
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_glb::{DMat4, Node, Order, write};
    use manfred_mesh::{Mesh, Slot};
    use std::path::PathBuf;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn line(words: &[String]) -> Result<Args, String> {
        let words: Vec<&str> = std::iter::once("diff")
            .chain(words.iter().map(String::as_str))
            .collect();
        match super::super::parsed(&words)? {
            super::super::Verb::Diff(args) => Ok(args),
            _ => Err("not diff".into()),
        }
    }

    fn parse(words: &[String]) -> Result<Compare, String> {
        line(words).map(|args| args.compare())
    }

    fn run_line(words: &[String], registry: &Registry) -> ExitCode {
        match line(words) {
            Ok(args) => run(&args, registry),
            Err(_) => ExitCode::from(2),
        }
    }

    fn scratch(name: &str, y: f32) -> String {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp");
        std::fs::create_dir_all(&dir).unwrap();
        let mesh = Mesh {
            positions: vec![[0.0, y, 0.0], [1.0, y, 0.0], [0.0, y + 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            uvs: None,
            uvs1: None,
            indices: vec![0, 1, 2],
            slots: vec![Slot {
                material: "m".into(),
                start: 0,
                count: 3,
            }],
        };
        let node = Node {
            name: "tri".into(),
            mesh,
            transform: DMat4::IDENTITY,
            keep_transform: false,
        };
        let path = dir.join(name);
        std::fs::write(&path, write(&[node], Order::File, "manfred test")).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn two_files_are_required() {
        assert!(parse(&strings(&["a.glb"])).is_err());
        assert!(parse(&strings(&["a.glb", "b.glb", "c.glb"])).is_err());
        let args = parse(&strings(&["a.glb", "--json", "b.glb"])).unwrap();
        assert!(args.json);
        assert_eq!(strings(&["a.glb", "b.glb"]), args.files);
    }

    #[test]
    fn render_needs_a_recipe_and_unknown_options_are_refused() {
        assert!(parse(&strings(&["a.glb", "b.glb", "--render"])).is_err());
        assert!(parse(&strings(&["a.glb", "b.glb", "--nope"])).is_err());
        assert!(parse(&strings(&["a.glb", "b.glb", "--camera", "main"])).is_err());
        assert!(parse(&strings(&["a.glb", "b.glb", "--render", "--recipe"])).is_err());
        assert!(
            parse(&strings(&[
                "a.glb", "b.glb", "--render", "--recipe", "r.toml", "--floor", "x"
            ]))
            .is_err()
        );
        let args = parse(&strings(&[
            "a.glb", "--render", "b.glb", "--recipe", "r.toml", "--size", "64x32",
        ]))
        .unwrap();
        assert_eq!(
            args.render,
            Some(Render {
                recipe: "r.toml".into(),
                camera: None,
                size: Some([64, 32]),
                floor: 35.0,
            })
        );
        let floor = parse(&strings(&[
            "a.glb", "b.glb", "--render", "--recipe", "r.toml", "--camera", "top", "--floor",
            "28.5",
        ]))
        .unwrap()
        .render
        .unwrap();
        assert_eq!((floor.camera.as_deref(), floor.floor), (Some("top"), 28.5));
    }

    #[test]
    fn a_catalogue_only_material_resolves_in_the_drawn_recipe() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("diff-catalogue.toml");
        std::fs::write(
            &path,
            "manfred = 1\nname = \"c\"\n\n[[object]]\nname = \"top\"\nkind = \"plane\"\nsize = [1.0, 1.0]\nmaterial = \"catalogue_only\"\n\n[[camera]]\nname = \"main\"\nposition = [0.0, 1.0, 2.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 30.0\n",
        )
        .unwrap();
        let render = Render {
            recipe: path,
            camera: None,
            size: None,
            floor: FLOOR_DB,
        };
        let mut registry = Registry::new();
        registry
            .register_materials_toml(
                "[materials.catalogue_only]\nbase = [0.2, 0.4, 0.6]\nroughness = 0.5\n",
            )
            .unwrap();
        let recipe = merged(&render, &registry).unwrap();
        let material = manfred_scene::material(&recipe, "catalogue_only").unwrap();
        assert!(material.is_some());
        let raw = manfred_recipe::load(&render.recipe).unwrap().recipe;
        assert!(
            manfred_scene::material(&raw, "catalogue_only")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn decibels_read_plainly() {
        assert_eq!(decibels(37.456), "37.46 dB");
        assert_eq!(decibels(f64::INFINITY), "inf dB (identical)");
    }

    #[test]
    fn the_exit_code_says_what_happened() {
        let a = scratch("diff-a.glb", 0.0);
        let same = scratch("diff-same.glb", 0.0);
        let moved = scratch("diff-moved.glb", 0.5);
        let code = |args: &[&str]| format!("{:?}", run_line(&strings(args), &Registry::new()));
        assert_eq!(code(&[&a, &same]), format!("{:?}", ExitCode::SUCCESS));
        assert_eq!(code(&[&a, &moved]), format!("{:?}", ExitCode::from(1)));
        assert_eq!(
            code(&[&a, &moved, "--json"]),
            format!("{:?}", ExitCode::from(1))
        );
        assert_eq!(
            code(&[&a, "missing.glb"]),
            format!("{:?}", ExitCode::FAILURE)
        );
        assert_eq!(code(&[&a, "--render"]), format!("{:?}", ExitCode::from(2)));
    }

    fn gpu_turn(since: std::time::Instant) {
        let ms = (since.elapsed().as_secs_f64() * 1000.0).ceil().to_string();
        let _ = std::process::Command::new("pgpu")
            .args(["turn", "--work-ms", &ms, "--longest-ms", &ms])
            .status();
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn diff_render_draws_the_table_against_itself_and_a_moved_copy() {
        let started = std::time::Instant::now();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let recipe = root.join("tests/golden/table/table.toml");
        let golden = root.join("tests/golden/table/table.glb");
        let golden = golden.to_string_lossy().into_owned();
        let (loaded, built) = manfred_scene::load(&recipe, None, &[], &Registry::new()).unwrap();
        let mut nodes = built.nodes();
        let crate_node = nodes.iter_mut().find(|n| n.name == "crate").unwrap();
        crate_node.transform = DMat4::from_cols_array(&[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.3, 0.0, 0.0, 1.0,
        ]);
        crate_node.keep_transform = true;
        let moved = root.join("tmp/diff-table-moved.glb");
        std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
        std::fs::write(&moved, write(&nodes, built.order, "manfred test")).unwrap();
        let moved = moved.to_string_lossy().into_owned();
        assert!(!loaded.cameras.is_empty());
        let recipe_arg = recipe.to_string_lossy().into_owned();
        let render = |files: [&str; 2], extra: &[&str]| {
            let mut args = vec![files[0], files[1], "--render", "--recipe", &recipe_arg];
            args.extend_from_slice(&["--size", "160x100"]);
            args.extend_from_slice(extra);
            run_line(&strings(&args), &Registry::new())
        };
        let code = |c: ExitCode| format!("{c:?}");
        assert_eq!(
            code(render([&golden, &golden], &[])),
            code(ExitCode::SUCCESS)
        );
        for name in ["a.png", "b.png", "difference.png"] {
            assert!(root.join(OUT).join(name).is_file(), "{name}");
        }
        gpu_turn(started);
        let floor = ["--floor", "200"];
        assert_eq!(
            code(render([&golden, &moved], &floor)),
            code(ExitCode::from(1))
        );
        let parsed = parse(&strings(&[
            &golden,
            &moved,
            "--render",
            "--recipe",
            &recipe_arg,
            "--size",
            "160x100",
            "--floor",
            "200",
            "--json",
        ]))
        .unwrap();
        let drawn = draw(
            &parsed.files,
            parsed.render.as_ref().unwrap(),
            &Registry::new(),
            Path::new(OUT),
        )
        .unwrap();
        let mut json = serde_json::json!({});
        attach(&mut json, &drawn, 200.0);
        let object = &json["render"];
        let psnr = object["psnr_db"].as_f64().unwrap();
        assert!(psnr.is_finite() && psnr > 0.0 && psnr < 200.0, "{psnr}");
        assert_eq!(object["floor_db"].as_f64(), Some(200.0));
        for key in ["a", "b", "difference"] {
            assert!(Path::new(object[key].as_str().unwrap()).is_file(), "{key}");
        }
        gpu_turn(started);
    }
}
