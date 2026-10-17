use std::path::PathBuf;
use std::process::ExitCode;

use manfred_build::Registry;
use manfred_build::gltf_io;
use serde_json::json;

use super::refuse;

const COMMAND: &str = "export";

#[derive(clap::Args)]
pub(super) struct Args {
    #[arg(value_name = "file.manf|recipe", help = "A .manf document or a recipe")]
    source: PathBuf,
    #[arg(long, value_name = "out.glb", help = "The standalone GLB to write")]
    glb: PathBuf,
    #[arg(
        long,
        value_name = "object",
        help = "Export this one object of a recipe"
    )]
    only: Option<String>,
    #[arg(long, help = "One JSON object on stdout")]
    json: bool,
}

pub fn run(args: &Args, registry: &Registry) -> ExitCode {
    let glb = match gltf_io::export_glb(&args.source, registry, args.only.as_deref()) {
        Ok(glb) => glb,
        Err(text) => return refuse(COMMAND, &text),
    };
    if let Some(dir) = args.glb.parent().filter(|d| !d.as_os_str().is_empty())
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return refuse(COMMAND, &format!("{}: {e}", dir.display()));
    }
    let temp = args.glb.with_extension("glb.part");
    let written = std::fs::write(&temp, &glb).and_then(|()| std::fs::rename(&temp, &args.glb));
    if let Err(e) = written {
        return refuse(COMMAND, &format!("{}: {e}", args.glb.display()));
    }
    let nodes = gltf::Gltf::from_slice(&glb).map_or(0, |g| g.document.nodes().len());
    if args.json {
        println!(
            "{}",
            json!({
                "source": args.source.display().to_string(),
                "glb": args.glb.display().to_string(),
                "bytes": glb.len(),
                "nodes": nodes,
            })
        );
    } else {
        println!("{}: {nodes} nodes, {} bytes", args.glb.display(), glb.len());
    }
    ExitCode::SUCCESS
}
