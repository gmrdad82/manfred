use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use manfred_build::{Built, Registry};
use manfred_glb::DMat4;
use manfred_recipe::{Camera, CameraPath, Recipe};
use manfred_render::Step;

use super::process::on_path;
use super::{pair, refuse, size};

const SAMPLES: u32 = 64;
const MAX_FPS: i64 = 240;
const MANIFEST: &str = "clip.txt";

type Matrix = [[f32; 4]; 4];

fn turns(text: &str) -> Result<f64, String> {
    match text.parse::<f64>() {
        Ok(turns) if turns.is_finite() => Ok(turns),
        _ => Err("is not a finite number of turns".into()),
    }
}

fn video(text: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(text);
    match path.extension().and_then(|e| e.to_str()) {
        Some("mp4" | "webm") => Ok(path),
        _ => Err("ends in neither .mp4 nor .webm".into()),
    }
}

#[derive(clap::Args, Clone, Debug, PartialEq)]
pub(super) struct Args {
    #[arg(value_name = "recipe", help = "The recipe to film")]
    pub(super) recipe: PathBuf,
    #[arg(
        long,
        value_name = "name",
        help = "The recipe's camera, with a path to move along, or still with --spin"
    )]
    camera: String,
    #[arg(
        long,
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..),
        required_unless_present = "timeline",
        conflicts_with = "timeline",
        help = "How many frames"
    )]
    frames: Option<u32>,
    #[arg(
        long,
        value_name = "F",
        value_parser = clap::value_parser!(u32).range(1..=MAX_FPS),
        required_unless_present = "timeline",
        conflicts_with = "timeline",
        help = "Frames per second, from 1 to 240"
    )]
    fps: Option<u32>,
    #[arg(
        long,
        help = "Film the recipe's [timeline], frames 0 to its last at its fps, the camera from its rig when it has one"
    )]
    timeline: bool,
    #[arg(
        long,
        value_name = "object",
        conflicts_with = "timeline",
        help = "Turn this object and everything parented to it about the world Y axis through its origin, the camera and lights held still"
    )]
    spin: Option<String>,
    #[arg(
        long = "turns",
        value_name = "N",
        requires = "spin",
        allow_negative_numbers = true,
        value_parser = turns,
        help = "How many turns --spin makes over the clip, negative the other way [default: 1]"
    )]
    turns: Option<f64>,
    #[arg(long, value_name = "WxH", value_parser = size, help = "Each frame's size in pixels")]
    size: [u32; 2],
    #[arg(
        long,
        value_name = "dir",
        help = "The folder for the frames; a second run resumes it"
    )]
    out: PathBuf,
    #[arg(long, help = "Path-trace each frame instead of drawing it live")]
    trace: bool,
    #[arg(
        long,
        value_name = "N",
        requires = "trace",
        value_parser = clap::value_parser!(u32).range(1..),
        help = "The tracer's samples per pixel [default: 64]"
    )]
    spp: Option<u32>,
    #[arg(
        long,
        value_name = "name",
        help = "Film this variant instead of the recipe"
    )]
    variant: Option<String>,
    #[arg(
        long,
        value_name = "key=value",
        value_parser = pair,
        help = "Replace a value the recipe has; repeatable"
    )]
    set: Vec<String>,
    #[arg(
        long = "video",
        value_name = "file",
        value_parser = video,
        help = "Encode the frames into this .mp4 or .webm with pcod"
    )]
    film: Option<PathBuf>,
}

impl Args {
    fn samples(&self) -> Option<u32> {
        self.trace.then(|| self.spp.unwrap_or(SAMPLES))
    }

    fn count(&self) -> u32 {
        self.frames.unwrap_or(1)
    }

    fn rate(&self) -> u32 {
        self.fps.unwrap_or(1)
    }

    fn spun(&self) -> f64 {
        self.turns.unwrap_or(1.0)
    }
}

fn chosen(args: &Args) -> Result<Recipe, String> {
    let file = args.recipe.display().to_string();
    let failed = |e: manfred_recipe::Error| super::read_failure(&file, &e);
    let loaded = manfred_recipe::load(&args.recipe).map_err(failed)?;
    let chosen = match &args.variant {
        Some(name) => manfred_recipe::apply_variant(&loaded.recipe, name).map_err(failed)?,
        None => loaded.recipe,
    };
    manfred_recipe::apply_set(&chosen, &args.set).map_err(failed)
}

fn degrees(turns: f64, frame: u32, frames: u32) -> f64 {
    turns * 360.0 * f64::from(frame) / f64::from(frames.max(1))
}

fn spun(recipe: &Recipe, name: &str) -> Result<usize, String> {
    recipe
        .objects
        .iter()
        .position(|object| object.name == name)
        .ok_or_else(|| {
            let known: Vec<&str> = recipe.objects.iter().map(|o| o.name.as_str()).collect();
            if known.is_empty() {
                format!("--spin {name}: the recipe has no objects")
            } else {
                format!(
                    "--spin {name}: no such object, expected one of: {}",
                    known.join(", ")
                )
            }
        })
}

fn motion(recipe: &Recipe, object: usize, degrees: f64) -> Result<DMat4, String> {
    let pivot = manfred_build::world(recipe, object)
        .map_err(|e| e.to_string())?
        .w_axis
        .truncate();
    Ok(DMat4::from_translation(pivot)
        * manfred_build::rotation([0.0, degrees, 0.0])
        * DMat4::from_translation(-pivot))
}

fn narrow(matrix: DMat4) -> Matrix {
    matrix
        .to_cols_array_2d()
        .map(|column| column.map(|v| v as f32))
}

fn held(args: &Args, camera: &Camera) -> Result<(), String> {
    if args.timeline {
        return Ok(());
    }
    match (&args.spin, &camera.path) {
        (Some(object), Some(_)) => Err(format!(
            "camera.{}: --spin {object} holds the camera still, and this camera has a [camera.path]; film the spin through a camera without one",
            camera.name
        )),
        (Some(_), None) => Ok(()),
        (None, _) => path_of(camera).map(|_| ()),
    }
}

fn resolved(args: &Args) -> Result<Args, String> {
    if !args.timeline {
        return Ok(args.clone());
    }
    let file = args.recipe.display().to_string();
    let timeline = chosen(args)?.timeline.ok_or_else(|| {
        format!("{file}: --timeline films the recipe's [timeline], and the recipe has none (docs/recipe.md, Timeline)")
    })?;
    Ok(Args {
        frames: Some(timeline.frames + 1),
        fps: Some(timeline.fps),
        ..args.clone()
    })
}

fn name(frame: u32, frames: u32) -> String {
    let digits = (frames.saturating_sub(1)).to_string().len().max(4);
    format!("frame-{frame:0digits$}.png")
}

fn partial(frame: u32, frames: u32) -> String {
    format!(
        ".{}.partial.png",
        name(frame, frames).trim_end_matches(".png")
    )
}

fn manifest(args: &Args) -> String {
    let recipe = std::fs::canonicalize(&args.recipe).unwrap_or_else(|_| args.recipe.clone());
    let mut text = format!(
        "manfred clip\nrecipe {}\ncamera {}\nframes {}\nsize {}x{}\n",
        recipe.display(),
        args.camera,
        args.count(),
        args.size[0],
        args.size[1]
    );
    if args.timeline {
        text.push_str("timeline\n");
    }
    if let Some(object) = &args.spin {
        text.push_str(&format!("spin {object}, {} turns\n", args.spun()));
    }
    match args.samples() {
        Some(samples) => text.push_str(&format!("renderer traced, {samples} spp\n")),
        None => text.push_str("renderer live\n"),
    }
    if let Some(variant) = &args.variant {
        text.push_str(&format!("variant {variant}\n"));
    }
    for set in &args.set {
        text.push_str(&format!("set {set}\n"));
    }
    text
}

fn claim(dir: &Path, wanted: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(MANIFEST);
    match std::fs::read_to_string(&path) {
        Ok(found) if found == wanted => Ok(()),
        Ok(found) => Err(format!(
            "{}: the frames here were made by another line, so they cannot be resumed with this one; use another --out or clear it\nthere:\n{found}here:\n{wanted}",
            path.display()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::write(&path, wanted).map_err(|e| format!("{}: {e}", path.display()))
        }
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn finished(path: &Path, size: [u32; 2]) -> bool {
    manfred_scene::read_png_size(path).is_ok_and(|found| found == size)
}

fn pending(dir: &Path, frames: u32, size: [u32; 2]) -> Vec<u32> {
    (0..frames)
        .filter(|&frame| !finished(&dir.join(name(frame, frames)), size))
        .collect()
}

fn encode_line(frames: &Path, video: &Path, fps: u32) -> Result<Vec<String>, String> {
    let absolute = |path: &Path| {
        std::path::absolute(path)
            .map(|path| path.display().to_string())
            .map_err(|e| format!("{}: {e}", path.display()))
    };
    Ok(vec![
        absolute(frames)?,
        absolute(video)?,
        "--fps".into(),
        fps.to_string(),
        "--profile".into(),
        "best".into(),
        "--force".into(),
    ])
}

fn precheck(args: &Args) -> Result<(), String> {
    let (camera, _) = super::precheck(
        &args.recipe,
        args.variant.as_deref(),
        &args.set,
        &args.camera,
        None,
    )?;
    if let Some(name) = &args.spin {
        spun(&chosen(args)?, name)?;
    }
    held(args, &camera)
}

fn requeue(args: &[String]) -> Option<ExitCode> {
    super::process::requeue("clip", args, "traces directly")
}

fn path_of(camera: &Camera) -> Result<&CameraPath, String> {
    camera.path.as_ref().ok_or_else(|| {
        format!(
            "camera.{}: the camera has no path, so every frame would be the same; give it a [camera.path] (docs/recipe.md)",
            camera.name
        )
    })
}

enum Renderer {
    Live(Box<manfred_scene::Reel>, Box<(Recipe, Built)>),
    Traced(Box<manfred_render::Reel>, u32),
}

impl Renderer {
    fn recipe(&self) -> &Recipe {
        match self {
            Renderer::Live(_, base) => &base.0,
            Renderer::Traced(reel, _) => &reel.recipe,
        }
    }

    fn spin(&mut self, object: usize, motion: Matrix) -> Result<(), String> {
        match self {
            Renderer::Live(reel, _) => reel.preview(object, motion).map_err(|e| e.0),
            Renderer::Traced(reel, _) => reel.spin(object, motion).map_err(|e| e.to_string()),
        }
    }

    fn pose(&mut self, time: f64, name: &str) -> Result<Camera, String> {
        match self {
            Renderer::Live(reel, base) => {
                let (recipe, built) = (&base.0, &base.1);
                let fps = recipe.timeline.map_or(1, |t| t.fps);
                reel.set_poses(manfred_scene::poses(built, fps, time));
                let lit = recipe.keyed.keys().any(|entry| entry.starts_with("light."));
                let recipe =
                    manfred_build::pose_cameras(recipe, built, time).map_err(|e| e.to_string())?;
                if lit {
                    reel.set_lights(&recipe.lights).map_err(|e| e.0)?;
                }
                manfred_scene::camera(&recipe, name)
                    .cloned()
                    .map_err(|e| e.0)
            }
            Renderer::Traced(reel, _) => {
                reel.pose(time).map_err(|e| e.to_string())?;
                manfred_scene::camera(&reel.recipe, name)
                    .cloned()
                    .map_err(|e| e.0)
            }
        }
    }

    fn frame(&mut self, camera: &Camera, size: [u32; 2], to: &Path) -> Result<String, String> {
        match self {
            Renderer::Live(reel, _) => {
                let (rgba, gpu_ms) = reel.shoot(&reel.eye(camera)).map_err(|e| e.0)?;
                manfred_scene::write_frame(to, size, &rgba).map_err(|e| e.0)?;
                Ok(format!("{gpu_ms:.0} ms on the GPU"))
            }
            Renderer::Traced(reel, samples) => {
                let rendered = reel
                    .frame(camera, size, *samples, &mut |_: Step| {})
                    .map_err(|e| e.to_string())?;
                let rgba = manfred_render::looked_with(
                    &rendered.film,
                    (&rendered.finish, &rendered.overlay),
                    true,
                )?;
                manfred_scene::write_frame(to, rendered.film.size, &rgba).map_err(|e| e.0)?;
                let paced = rendered.paced;
                let builds = reel.builds().map_or(String::new(), |builds| {
                    format!(
                        ", the trace's {} bottom and {} top builds",
                        builds.bottom, builds.top
                    )
                });
                Ok(format!(
                    "{} submissions, the longest {:.2} ms, {:.0} ms on the GPU{builds}",
                    paced.submissions, paced.longest_ms, paced.gpu_ms
                ))
            }
        }
    }
}

fn open(args: &Args, registry: &Registry) -> Result<(Renderer, Camera), String> {
    let (renderer, recipe) = match args.samples() {
        None => {
            let (recipe, built) =
                manfred_scene::load(&args.recipe, args.variant.as_deref(), &args.set, registry)
                    .map_err(|e| e.0)?;
            let camera = manfred_scene::camera(&recipe, &args.camera).map_err(|e| e.0)?;
            held(args, camera)?;
            if let Some(name) = &args.spin {
                spun(&recipe, name)?;
            }
            let gpu = manfred_scene::headless().map_err(|e| e.0)?;
            let reel = manfred_scene::Reel::new(&gpu, &recipe, &built.parts, args.size)
                .map_err(|e| e.0)?;
            let base = Box::new((recipe.clone(), built));
            (Renderer::Live(Box::new(reel), base), recipe)
        }
        Some(samples) => {
            let reel = manfred_render::Reel::open(
                &args.recipe,
                args.variant.as_deref(),
                &args.set,
                None,
                registry,
            )
            .map_err(|e| e.to_string())?;
            let recipe = reel.recipe.clone();
            (Renderer::Traced(Box::new(reel), samples), recipe)
        }
    };
    let camera = manfred_scene::camera(&recipe, &args.camera).map_err(|e| e.0)?;
    held(args, camera)?;
    Ok((renderer, camera.clone()))
}

fn frames(args: &Args, registry: &Registry, progress: &mut dyn FnMut(u32)) -> Result<(), String> {
    claim(&args.out, &manifest(args))?;
    let count = args.count();
    let todo = pending(&args.out, count, args.size);
    let done = count - todo.len() as u32;
    eprintln!(
        "manfred clip: {} of {count} frames to make, {done} already finished in {}",
        todo.len(),
        args.out.display()
    );
    progress(done);
    if todo.is_empty() {
        return Ok(());
    }
    let (mut renderer, camera) = open(args, registry)?;
    let spin = match &args.spin {
        Some(name) => Some(spun(renderer.recipe(), name)?),
        None => None,
    };
    let path = if args.timeline || spin.is_some() {
        None
    } else {
        Some(path_of(&camera)?)
    };
    for (made, &frame) in todo.iter().enumerate() {
        let pose = match (spin, path) {
            (Some(object), _) => {
                let turned = motion(
                    renderer.recipe(),
                    object,
                    degrees(args.spun(), frame, count),
                )?;
                renderer.spin(object, narrow(turned))?;
                camera.clone()
            }
            (None, Some(path)) => camera.at(path.time(frame, count)),
            (None, None) => {
                renderer.pose(f64::from(frame) / f64::from(args.rate()), &args.camera)?
            }
        };
        let part = args.out.join(partial(frame, count));
        let to = args.out.join(name(frame, count));
        let summary = renderer.frame(&pose, args.size, &part)?;
        std::fs::rename(&part, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        eprintln!("manfred clip: {}, {summary}", to.display());
        println!("{}", to.display());
        progress(done + made as u32 + 1);
    }
    Ok(())
}

fn unencoded(args: &Args, video: &Path) -> String {
    format!(
        "{} needs pcod (pito-codecs) on the PATH for --video; the frames are kept in {}",
        video.display(),
        args.out.display()
    )
}

fn encode(args: &Args, video: &Path, pcod: bool) -> Result<(), String> {
    if !pcod {
        return Err(unencoded(args, video));
    }
    let line = encode_line(&args.out, video, args.rate())?;
    eprintln!("manfred clip: pcod {}", line.join(" "));
    let status = Command::new("pcod")
        .args(&line)
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("cannot start pcod: {e}"))?;
    if !status.success() {
        return Err(format!("pcod failed ({status}) making {}", video.display()));
    }
    println!("{}", video.display());
    Ok(())
}

pub fn run(parsed: &Args, args: &[String], registry: &Registry) -> ExitCode {
    let parsed = &match resolved(parsed) {
        Ok(parsed) => parsed,
        Err(message) => return refuse("clip", &message),
    };
    if parsed.samples().is_some() {
        if let Err(message) = precheck(parsed) {
            return refuse("clip", &message);
        }
        if let Some(code) = requeue(args) {
            return code;
        }
    }
    let made = frames(parsed, registry, &mut |_| {}).and_then(|()| match &parsed.film {
        Some(video) => encode(parsed, video, on_path("pcod")),
        None => Ok(()),
    });
    match made {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => refuse("clip", &error),
    }
}

#[cfg(test)]
mod tests {
    use super::super::process::{self, Machine};
    use super::*;

    fn line(exe: &str, args: &[String], machine: Machine<'_>) -> Option<Vec<String>> {
        process::line("clip", exe, args, machine)
    }

    fn strings(text: &str) -> Vec<String> {
        text.split_whitespace().map(String::from).collect()
    }

    fn parse(words: &[String]) -> Result<Args, String> {
        let words: Vec<&str> = std::iter::once("clip")
            .chain(words.iter().map(String::as_str))
            .collect();
        match super::super::parsed(&words)? {
            super::super::Verb::Clip(args) => Ok(args),
            _ => Err("not clip".into()),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tmp/clip-tests")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn without_an_encoder_the_frames_stay_and_the_way_on_is_named() {
        let dir = scratch("no-encoder");
        let mut args = parse(&strings(
            "a.toml --camera main --frames 2 --fps 24 --size 8x8 --out d",
        ))
        .unwrap();
        args.out = dir.clone();
        let video = dir.join("c.mp4");
        let error = encode(&args, &video, false).unwrap_err();
        assert_eq!(error, unencoded(&args, &video));
        assert!(error.contains("needs pcod (pito-codecs)"));
        assert!(!error.contains("ffmpeg"));
        assert!(!video.exists());
    }

    #[test]
    fn every_flag_is_read() {
        let args = parse(&strings(
            "table.toml --camera main --frames 24 --fps 30 --spin body --turns -0.5 --size 320x200 --out tmp/c --trace --spp 16 --variant bare --set sky.intensity=0.4 --video tmp/c.webm",
        ))
        .unwrap();
        assert_eq!(
            args,
            Args {
                recipe: "table.toml".into(),
                camera: "main".into(),
                frames: Some(24),
                fps: Some(30),
                timeline: false,
                spin: Some("body".into()),
                turns: Some(-0.5),
                size: [320, 200],
                out: "tmp/c".into(),
                trace: true,
                spp: Some(16),
                variant: Some("bare".into()),
                set: vec!["sky.intensity=0.4".into()],
                film: Some("tmp/c.webm".into()),
            }
        );
        let live = parse(&strings(
            "a.toml --camera main --frames 3 --fps 24 --size 8x8 --out d",
        ))
        .unwrap();
        assert_eq!(live.samples(), None);
        assert_eq!(live.film, None);
        let traced = parse(&strings(
            "a.toml --camera main --frames 3 --fps 24 --size 8x8 --out d --trace",
        ))
        .unwrap();
        assert_eq!(traced.samples(), Some(64));
    }

    #[test]
    fn mistakes_are_refused() {
        let base = "a.toml --camera main --frames 3 --fps 24 --size 8x8 --out d";
        assert!(parse(&strings(base)).is_ok());
        let filmed = parse(&strings(
            "a.toml --camera main --timeline --size 8x8 --out d",
        ))
        .unwrap();
        assert!(filmed.timeline && filmed.frames.is_none() && filmed.fps.is_none());
        for extra in [
            "--spp 8",
            "--trace --spp 0",
            "--video a.gif",
            "--video a",
            "--nope",
            "b.toml",
            "--set",
            "--timeline",
            "--turns 2",
            "--spin body --turns",
            "--spin body --turns inf",
            "--spin body --turns x",
        ] {
            assert!(
                parse(&strings(&format!("{base} {extra}"))).is_err(),
                "{extra}"
            );
        }
        for bad in [
            "a.toml --frames 3 --fps 24 --size 8x8 --out d",
            "a.toml --camera main --fps 24 --size 8x8 --out d",
            "a.toml --camera main --frames 3 --size 8x8 --out d",
            "a.toml --camera main --frames 3 --fps 24 --out d",
            "a.toml --camera main --frames 3 --fps 24 --size 8x8",
            "--camera main --frames 3 --fps 24 --size 8x8 --out d",
            "a.toml --camera main --frames 0 --fps 24 --size 8x8 --out d",
            "a.toml --camera main --frames 3 --fps 0 --size 8x8 --out d",
            "a.toml --camera main --frames 3 --fps 241 --size 8x8 --out d",
            "a.toml --camera main --frames -1 --fps 24 --size 8x8 --out d",
            "a.toml --camera main --frames 3 --fps 24 --size 0x8 --out d",
            "a.toml --camera main --timeline --spin body --size 8x8 --out d",
        ] {
            assert!(parse(&strings(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn frames_are_numbered_from_zero_wide_enough_for_the_last() {
        assert_eq!(name(0, 24), "frame-0000.png");
        assert_eq!(name(23, 24), "frame-0023.png");
        assert_eq!(name(9999, 10000), "frame-9999.png");
        assert_eq!(name(7, 10001), "frame-00007.png");
        assert_eq!(partial(3, 24), ".frame-0003.partial.png");
    }

    #[test]
    fn an_opaque_frame_is_written_as_rgb_and_a_transparent_one_keeps_its_alpha() {
        let dir = scratch("colour");
        let colour = |path: &Path| {
            let file = std::fs::File::open(path).unwrap();
            png::Decoder::new(std::io::BufReader::new(file))
                .read_info()
                .unwrap()
                .info()
                .color_type
        };
        let opaque: Vec<u8> = (0..4 * 2 * 4)
            .map(|n| if n % 4 == 3 { 255 } else { (n * 7) as u8 })
            .collect();
        let mut clear = opaque.clone();
        clear[7] = 128;
        for (rgba, wanted) in [
            (&opaque, png::ColorType::Rgb),
            (&clear, png::ColorType::Rgba),
        ] {
            let path = dir.join(format!("{wanted:?}.png"));
            manfred_scene::write_frame(&path, [4, 2], rgba).unwrap();
            assert_eq!(colour(&path), wanted);
            assert_eq!(
                manfred_scene::read_png(&path).unwrap(),
                ([4, 2], rgba.clone())
            );
            assert!(finished(&path, [4, 2]));
        }
    }

    #[test]
    fn resuming_skips_finished_frames() {
        let dir = scratch("resume");
        let size = [4, 2];
        let rgba = vec![200u8; 4 * 2 * 4];
        let opaque = vec![255u8; 4 * 2 * 4];
        manfred_scene::write_png(&dir.join(name(0, 5)), size, &rgba).unwrap();
        manfred_scene::write_frame(&dir.join(name(2, 5)), size, &opaque).unwrap();
        manfred_scene::write_png(&dir.join(name(3, 5)), [2, 2], &rgba[..16]).unwrap();
        let whole = std::fs::read(dir.join(name(0, 5))).unwrap();
        std::fs::write(dir.join(name(4, 5)), &whole[..whole.len() - 20]).unwrap();
        std::fs::write(dir.join(partial(1, 5)), &whole).unwrap();
        assert_eq!(pending(&dir, 5, size), [1, 3, 4]);
        assert_eq!(pending(&dir, 5, [4, 4]), [0, 1, 2, 3, 4]);
        assert_eq!(pending(&scratch("empty"), 3, size), [0, 1, 2]);
    }

    #[test]
    fn a_folder_resumes_only_the_line_that_made_it() {
        let dir = scratch("claim");
        let line = |extra: &str| {
            parse(&strings(&format!(
                "a.toml --camera main --frames 3 --size 8x8 --out {} {extra}",
                dir.display()
            )))
            .unwrap()
        };
        let first = manifest(&line("--fps 24"));
        assert!(first.contains("renderer live\n"), "{first}");
        claim(&dir, &first).unwrap();
        claim(&dir, &first).unwrap();
        assert_eq!(manifest(&line("--fps 30 --video a.mp4")), first);
        let traced = manifest(&line("--fps 24 --trace --spp 32 --set sky.intensity=0.5"));
        assert!(traced.contains("renderer traced, 32 spp\nset sky.intensity=0.5\n"));
        let refused = claim(&dir, &traced).unwrap_err();
        assert!(refused.contains("made by another line"), "{refused}");
    }

    #[test]
    fn pcod_reads_the_frames_folder_at_the_clips_rate() {
        let line = encode_line(Path::new("-d"), Path::new("-c.webm"), 24).unwrap();
        let here = std::env::current_dir().unwrap();
        assert_eq!(
            line,
            [
                here.join("-d").display().to_string(),
                here.join("-c.webm").display().to_string(),
                "--fps".into(),
                "24".into(),
                "--profile".into(),
                "best".into(),
                "--force".into(),
            ]
        );
    }

    #[test]
    fn the_traced_line_is_build_slice_then_pgpu_clip_unless_queued() {
        let args = strings("a.toml --camera main --trace");
        let full = Machine {
            queued: false,
            pgpu: true,
            systemd: true,
            cpus: Some("2-5"),
        };
        assert_eq!(
            line("/opt/manfred", &args, full).unwrap().join(" "),
            "systemd-run --user --quiet --scope --collect --slice=build.slice -p MemoryMax=12G -p MemorySwapMax=0 -- taskset -c 2-5 nice -n 10 pgpu run --class clip --as manfred -- /opt/manfred clip a.toml --camera main --trace"
        );
        let queued = Machine {
            queued: true,
            ..full
        };
        assert_eq!(line("manfred", &args, queued), None);
    }

    #[test]
    fn a_bad_recipe_or_camera_refuses_with_1_before_the_queue() {
        let one = format!("{:?}", ExitCode::FAILURE);
        let tail = "--frames 3 --fps 24 --size 8x8 --out tmp/clip-tests/never --trace";
        for line in [
            format!("nowhere/none.toml --camera main {tail}"),
            format!("crates/recipe/tests/fixtures/full.toml --camera nope {tail}"),
            format!("crates/recipe/tests/fixtures/full.toml --camera main {tail}"),
        ] {
            let words = strings(&line);
            let code = run(&parse(&words).unwrap(), &words, &Registry::new());
            assert_eq!(format!("{code:?}"), one, "{line}");
        }
        assert!(!Path::new("tmp/clip-tests/never").exists());
    }

    const BODY: &str = "manfred = 1\nname = \"turntable\"\n[[object]]\nname = \"body\"\nkind = \"box\"\nsize = [0.01, 0.01, 0.01]\nat = [1.0, 0.5, -2.0]\nrotate = [0.0, 30.0, 0.0]\nhidden = true\n[[object]]\nname = \"figure\"\nkind = \"box\"\nsize = [0.2, 0.4, 0.2]\nparent = \"body\"\nat = [0.3, 0.0, 0.1]\nrotate = [0.0, 0.0, 10.0]\n[[object]]\nname = \"hat\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\nparent = \"figure\"\nat = [0.0, 0.3, 0.0]\n[[object]]\nname = \"floor\"\nkind = \"box\"\nsize = [4.0, 0.1, 4.0]\n[[camera]]\nname = \"main\"\nposition = [0.0, 1.0, 3.0]\ntarget = [1.0, 0.5, -2.0]\nfov_y_deg = 40.0\n";

    #[test]
    fn a_spin_turns_from_zero_to_its_turns_over_the_clip() {
        assert_eq!(degrees(1.0, 0, 8), 0.0);
        assert_eq!(degrees(1.0, 8, 8), 360.0);
        assert_eq!(degrees(1.0, 2, 8), 90.0);
        assert_eq!(degrees(-2.0, 8, 8), -720.0);
        assert_eq!(degrees(0.5, 4, 8), 90.0);
        let recipe = manfred_recipe::parse(BODY).unwrap();
        let body = spun(&recipe, "body").unwrap();
        assert_eq!(
            motion(&recipe, body, degrees(1.0, 0, 8)).unwrap(),
            DMat4::IDENTITY
        );
        let whole = motion(&recipe, body, degrees(3.0, 8, 8)).unwrap();
        assert!(whole.abs_diff_eq(DMat4::IDENTITY, 1e-12), "{whole:?}");
        let message = spun(&recipe, "nobody").unwrap_err();
        assert!(
            message.contains("no such object, expected one of: body, figure, hat, floor"),
            "{message}"
        );
    }

    #[test]
    fn a_spin_turns_the_whole_parent_chain_about_its_own_origin() {
        let recipe = manfred_recipe::parse(BODY).unwrap();
        let body = spun(&recipe, "body").unwrap();
        let turned = motion(&recipe, body, 75.0).unwrap();
        let mut rotated = recipe.clone();
        rotated.objects[body].rotate[1] += 75.0;
        for name in ["body", "figure", "hat"] {
            let at = spun(&recipe, name).unwrap();
            let moved = turned * manfred_build::world(&recipe, at).unwrap();
            let expected = manfred_build::world(&rotated, at).unwrap();
            assert!(moved.abs_diff_eq(expected, 1e-12), "{name}");
        }
        let pivot = manfred_build::world(&recipe, body).unwrap().w_axis;
        assert!((turned * pivot).abs_diff_eq(pivot, 1e-12));
        assert_eq!(
            manfred_scene::family_of(&recipe, body),
            [true, true, true, false]
        );
        let figure = spun(&recipe, "figure").unwrap();
        assert_eq!(
            manfred_scene::family_of(&recipe, figure),
            [false, true, true, false]
        );
        let hat = manfred_build::world(&recipe, spun(&recipe, "hat").unwrap()).unwrap();
        let around = motion(&recipe, figure, 90.0).unwrap() * hat;
        let origin = manfred_build::world(&recipe, figure).unwrap().w_axis;
        let lever = hat.w_axis - origin;
        let swung = around.w_axis - origin;
        assert!((swung.x - lever.z).abs() < 1e-12 && (swung.z + lever.x).abs() < 1e-12);
        assert!((swung.y - lever.y).abs() < 1e-12);
    }

    #[test]
    fn a_spin_holds_a_still_camera_and_refuses_a_moving_one() {
        let recipe = manfred_recipe::parse(BODY).unwrap();
        let line = |rest: &str| {
            parse(&strings(&format!(
                "a.toml --camera main --frames 8 --fps 24 --size 8x8 --out d {rest}"
            )))
            .unwrap()
        };
        assert_eq!(held(&line("--spin body"), &recipe.cameras[0]), Ok(()));
        assert!(
            held(&line(""), &recipe.cameras[0])
                .unwrap_err()
                .contains("has no path")
        );
        let mut moving = recipe.cameras[0].clone();
        moving.path = orbit();
        let refused = held(&line("--spin body --turns 2"), &moving).unwrap_err();
        assert!(refused.contains("holds the camera still"), "{refused}");
        let spun = manifest(&line("--spin body --turns -1.5"));
        assert!(spun.contains("spin body, -1.5 turns\n"), "{spun}");
        assert_eq!(
            manifest(&line("--spin body")),
            manifest(&line("--spin body --turns 1"))
        );
        assert!(!manifest(&line("")).contains("spin"));
    }

    fn orbit() -> Option<CameraPath> {
        manfred_recipe::parse(
            "manfred = 1\nname = \"o\"\n[[camera]]\nname = \"main\"\nposition = [0.0, 1.0, 2.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 40.0\n[camera.path]\nkind = \"orbit\"\ntarget = [0.0, 0.0, 0.0]\nturns = 1.0\n",
        )
        .unwrap()
        .cameras[0]
            .path
            .clone()
    }

    #[test]
    fn a_camera_without_a_path_is_refused() {
        let recipe = manfred_recipe::parse(
            "manfred = 1\nname = \"still\"\n[[camera]]\nname = \"main\"\nposition = [0.0, 1.0, 2.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 40.0\n",
        )
        .unwrap();
        let message = path_of(&recipe.cameras[0]).unwrap_err();
        assert!(message.contains("has no path"), "{message}");
    }

    fn table_orbit(dir: &Path) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let table = std::fs::read_to_string(root.join("tests/golden/table/table.toml")).unwrap();
        let recipe = dir.join("table-orbit.toml");
        std::fs::write(
            &recipe,
            format!(
                "{table}\n[camera.path]\nkind = \"orbit\"\ntarget = [0.0, 0.03, -0.02]\nturns = 1.0\n"
            ),
        )
        .unwrap();
        recipe
    }

    fn table_line(recipe: &Path, out: &Path, rest: &str) -> Args {
        parse(&strings(&format!(
            "{} --camera main --frames 3 --fps 24 --out {} {rest}",
            recipe.display(),
            out.display()
        )))
        .unwrap()
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn three_table_frames_start_on_the_shot_and_resume_to_the_same_bytes() {
        let dir = scratch("live");
        let recipe = table_orbit(&dir);
        let out = dir.join("frames");
        let args = table_line(&recipe, &out, "--size 320x200");
        frames(&args, &Registry::new(), &mut |_| {}).unwrap();
        let read = |frame: u32| {
            let (size, rgba) = manfred_scene::read_png(&out.join(name(frame, 3))).unwrap();
            assert_eq!(size, args.size);
            rgba
        };
        let made: Vec<Vec<u8>> = (0..3).map(read).collect();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let shot = manfred_scene::still_with(
            &root.join("tests/golden/table/table.toml"),
            "main",
            args.size,
            None,
            &Registry::new(),
        )
        .unwrap();
        assert!(
            made[0] == shot,
            "frame 0 differs from manfred shot of the camera"
        );
        assert!(made[1] != made[0] && made[2] != made[1]);
        for frame in [1, 2] {
            std::fs::remove_file(out.join(name(frame, 3))).unwrap();
        }
        assert_eq!(pending(&out, 3, args.size), [1, 2]);
        frames(&args, &Registry::new(), &mut |_| {}).unwrap();
        for frame in 0..3 {
            assert!(
                read(frame) == made[frame as usize],
                "frame {frame} after resuming"
            );
        }
        assert!(pending(&out, 3, args.size).is_empty());
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_traced_frame_is_the_render_of_its_pose() {
        let dir = scratch("traced");
        let recipe = table_orbit(&dir);
        let out = dir.join("frames");
        let args = table_line(&recipe, &out, "--size 160x100 --trace --spp 4");
        let mut done = Vec::new();
        frames(&args, &Registry::new(), &mut |n| done.push(n)).unwrap();
        assert_eq!(done, [0, 1, 2, 3]);
        let request = manfred_render::Request {
            recipe: &recipe,
            camera: "main",
            size: args.size,
            samples: 4,
            noise: None,
            seed: None,
            view: manfred_render::View::Lens,
            variant: None,
            set: &[],
            pick: &manfred_recipe::Pick::default(),
            crop: None,
            denoise: true,
            time: None,
        };
        let rendered = manfred_render::render(&request, &Registry::new(), &mut |_| {}).unwrap();
        let still = dir.join("render.png");
        manfred_render::write_with(
            &still,
            &rendered.film,
            (&rendered.finish, &rendered.overlay),
            true,
        )
        .unwrap();
        let (_, first) = manfred_scene::read_png(&out.join(name(0, 3))).unwrap();
        let (_, render) = manfred_scene::read_png(&still).unwrap();
        assert!(
            first == render,
            "frame 0 differs from manfred render of the camera"
        );
        let (_, second) = manfred_scene::read_png(&out.join(name(1, 3))).unwrap();
        assert!(second != first);
    }
}
