use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use manfred_build::Registry;
use manfred_recipe::{Camera, CameraPath};
use manfred_render::Step;

use super::process::on_path;
use super::{pair, refuse, size};

const SAMPLES: u32 = 64;
const MAX_FPS: i64 = 240;
const MANIFEST: &str = "clip.txt";
const LIST: &str = "frames.ffconcat";
const STAGES: [&str; 2] = ["frames", "video"];

fn video(text: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(text);
    match path.extension().and_then(|e| e.to_str()) {
        Some("mp4" | "webm") => Ok(path),
        _ => Err("ends in neither .mp4 nor .webm".into()),
    }
}

#[derive(clap::Args, Debug, PartialEq)]
pub(super) struct Args {
    #[arg(value_name = "recipe", help = "The recipe to film")]
    recipe: PathBuf,
    #[arg(
        long,
        value_name = "name",
        help = "The recipe's camera, with a path to move along"
    )]
    camera: String,
    #[arg(
        long,
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..),
        help = "How many frames"
    )]
    frames: u32,
    #[arg(
        long,
        value_name = "F",
        value_parser = clap::value_parser!(u32).range(1..=MAX_FPS),
        help = "Frames per second, from 1 to 240"
    )]
    fps: u32,
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
        help = "Encode the frames into this .mp4 or .webm with pconv"
    )]
    film: Option<PathBuf>,
}

impl Args {
    fn samples(&self) -> Option<u32> {
        self.trace.then(|| self.spp.unwrap_or(SAMPLES))
    }
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
        args.frames,
        args.size[0],
        args.size[1]
    );
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
    manfred_scene::read_png(path).is_ok_and(|(found, _)| found == size)
}

fn pending(dir: &Path, frames: u32, size: [u32; 2]) -> Vec<u32> {
    (0..frames)
        .filter(|&frame| !finished(&dir.join(name(frame, frames)), size))
        .collect()
}

fn concat(frames: u32, fps: u32) -> String {
    let at = |frame: u32| (u64::from(frame) * 1_000_000 + u64::from(fps) / 2) / u64::from(fps);
    let mut text = String::from("ffconcat version 1.0\n");
    for frame in 0..frames {
        let micros = at(frame + 1) - at(frame);
        text.push_str(&format!(
            "file {}\nduration {}.{:06}\n",
            name(frame, frames),
            micros / 1_000_000,
            micros % 1_000_000
        ));
    }
    text
}

fn encode_line(list: &Path, video: &Path, fps: u32) -> Vec<String> {
    vec![
        list.display().to_string(),
        video.display().to_string(),
        "--fps".into(),
        fps.to_string(),
        "--profile".into(),
        "best".into(),
        "--force".into(),
    ]
}

fn precheck(args: &Args) -> Result<(), String> {
    let camera = super::precheck(
        &args.recipe,
        args.variant.as_deref(),
        &args.set,
        &args.camera,
    )?;
    path_of(&camera).map(|_| ())
}

fn requeue(args: &[String]) -> Option<ExitCode> {
    super::process::requeue("clip", args, "traces directly")
}

fn record(args: &Args) -> super::process::Record {
    let record = super::process::Record::start(
        &format!("clip {}", args.out.display()),
        &format!(
            "{} {} {} frames {}x{} {} spp",
            args.recipe.display(),
            args.camera,
            args.frames,
            args.size[0],
            args.size[1],
            args.samples().unwrap_or(0)
        ),
        &args.out,
        &STAGES,
    );
    record.stage(STAGES[0], Some(u64::from(args.frames)), "frames");
    record
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
    Live(Box<manfred_scene::Reel>),
    Traced(Box<manfred_render::Reel>, u32),
}

impl Renderer {
    fn frame(&mut self, camera: &Camera, size: [u32; 2], to: &Path) -> Result<String, String> {
        match self {
            Renderer::Live(reel) => {
                let (rgba, gpu_ms) = reel.shoot(&reel.eye(camera)).map_err(|e| e.0)?;
                manfred_scene::write_png(to, size, &rgba).map_err(|e| e.0)?;
                Ok(format!("{gpu_ms:.0} ms on the GPU"))
            }
            Renderer::Traced(reel, samples) => {
                let rendered = reel
                    .frame(camera, size, *samples, &mut |_: Step| {})
                    .map_err(|e| e.to_string())?;
                manfred_render::write_with(
                    to,
                    &rendered.film,
                    (&rendered.finish, &rendered.overlay),
                    true,
                )
                .map_err(|e| e.to_string())?;
                let paced = rendered.paced;
                Ok(format!(
                    "{} submissions, the longest {:.2} ms, {:.0} ms on the GPU",
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
            path_of(manfred_scene::camera(&recipe, &args.camera).map_err(|e| e.0)?)?;
            let gpu = manfred_scene::headless().map_err(|e| e.0)?;
            let reel = manfred_scene::Reel::new(&gpu, &recipe, &built.parts, args.size)
                .map_err(|e| e.0)?;
            (Renderer::Live(Box::new(reel)), recipe)
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
    path_of(camera)?;
    Ok((renderer, camera.clone()))
}

fn frames(args: &Args, registry: &Registry, progress: &mut dyn FnMut(u32)) -> Result<(), String> {
    claim(&args.out, &manifest(args))?;
    let todo = pending(&args.out, args.frames, args.size);
    let done = args.frames - todo.len() as u32;
    eprintln!(
        "manfred clip: {} of {} frames to make, {done} already finished in {}",
        todo.len(),
        args.frames,
        args.out.display()
    );
    progress(done);
    if todo.is_empty() {
        return Ok(());
    }
    let (mut renderer, camera) = open(args, registry)?;
    let path = path_of(&camera)?;
    for (made, &frame) in todo.iter().enumerate() {
        let pose = camera.at(path.time(frame, args.frames));
        let part = args.out.join(partial(frame, args.frames));
        let to = args.out.join(name(frame, args.frames));
        let summary = renderer.frame(&pose, args.size, &part)?;
        std::fs::rename(&part, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        eprintln!("manfred clip: {}, {summary}", to.display());
        println!("{}", to.display());
        progress(done + made as u32 + 1);
    }
    Ok(())
}

fn unencoded(out: &Path, list: &Path, video: &Path) -> String {
    format!(
        "no video encoder (pconv) on the PATH, so {} was not made; the frames are in {} and {} lists them, so any encoder can make it, for example `ffmpeg -f concat -i {} {}`",
        video.display(),
        out.display(),
        list.display(),
        list.display(),
        video.display()
    )
}

fn encode(args: &Args, video: &Path, pconv: bool) -> Result<(), String> {
    let list = args.out.join(LIST);
    std::fs::write(&list, concat(args.frames, args.fps))
        .map_err(|e| format!("{}: {e}", list.display()))?;
    if !pconv {
        return Err(unencoded(&args.out, &list, video));
    }
    let line = encode_line(&list, video, args.fps);
    eprintln!("manfred clip: pconv {}", line.join(" "));
    let status = Command::new("pconv")
        .args(&line)
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("cannot start pconv: {e}"))?;
    if !status.success() {
        return Err(format!(
            "pconv failed ({status}) making {}",
            video.display()
        ));
    }
    println!("{}", video.display());
    Ok(())
}

pub fn run(parsed: &Args, args: &[String], registry: &Registry) -> ExitCode {
    if parsed.samples().is_some() {
        if let Err(message) = precheck(parsed) {
            return refuse("clip", &message);
        }
        if let Some(code) = requeue(args) {
            return code;
        }
    }
    let record = parsed.samples().map(|_| record(parsed));
    let made = frames(parsed, registry, &mut |done| {
        if let Some(record) = &record {
            record.progress(u64::from(done));
        }
    })
    .and_then(|()| match &parsed.film {
        Some(video) => {
            if let Some(record) = &record {
                record.stage(STAGES[1], None, "files");
            }
            encode(parsed, video, on_path("pconv"))
        }
        None => Ok(()),
    });
    match made {
        Ok(()) => {
            if let Some(record) = &record {
                record.finish();
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if let Some(record) = &record {
                record.fail(&error);
            }
            refuse("clip", &error)
        }
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
    fn without_an_encoder_the_frames_and_their_list_stay_and_the_way_on_is_named() {
        let dir = scratch("no-encoder");
        let mut args = parse(&strings(
            "a.toml --camera main --frames 2 --fps 24 --size 8x8 --out d",
        ))
        .unwrap();
        args.out = dir.clone();
        let video = dir.join("c.mp4");
        let error = encode(&args, &video, false).unwrap_err();
        let list = dir.join(LIST);
        assert_eq!(error, unencoded(&dir, &list, &video));
        assert!(error.starts_with("no video encoder (pconv) on the PATH"));
        assert!(error.contains(&format!("ffmpeg -f concat -i {}", list.display())));
        assert_eq!(std::fs::read_to_string(&list).unwrap(), concat(2, 24));
        assert!(!video.exists());
    }

    #[test]
    fn every_flag_is_read() {
        let args = parse(&strings(
            "table.toml --camera main --frames 24 --fps 30 --size 320x200 --out tmp/c --trace --spp 16 --variant bare --set sky.intensity=0.4 --video tmp/c.webm",
        ))
        .unwrap();
        assert_eq!(
            args,
            Args {
                recipe: "table.toml".into(),
                camera: "main".into(),
                frames: 24,
                fps: 30,
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
        for extra in [
            "--spp 8",
            "--trace --spp 0",
            "--video a.gif",
            "--video a",
            "--nope",
            "b.toml",
            "--set",
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
    fn resuming_skips_finished_frames() {
        let dir = scratch("resume");
        let size = [4, 2];
        let rgba = vec![200u8; 4 * 2 * 4];
        for frame in [0, 2] {
            manfred_scene::write_png(&dir.join(name(frame, 5)), size, &rgba).unwrap();
        }
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
    fn the_concat_list_holds_each_frame_for_its_share_of_a_second() {
        let list = concat(3, 24);
        assert_eq!(
            list,
            "ffconcat version 1.0\nfile frame-0000.png\nduration 0.041667\nfile frame-0001.png\nduration 0.041666\nfile frame-0002.png\nduration 0.041667\n"
        );
        let total: u64 = concat(24, 24)
            .lines()
            .filter_map(|l| l.strip_prefix("duration 0."))
            .map(|d| d.parse::<u64>().unwrap())
            .sum();
        assert_eq!(total, 1_000_000);
        assert_eq!(
            encode_line(Path::new("d/frames.ffconcat"), Path::new("c.webm"), 24).join(" "),
            "d/frames.ffconcat c.webm --fps 24 --profile best --force"
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
            crop: None,
            denoise: true,
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
