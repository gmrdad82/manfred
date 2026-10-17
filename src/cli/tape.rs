use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc::{self, Receiver, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use manfred_build::Registry;
use manfred_ui::{Follow, Layout, Manfred, Mode, Shell, Tape, Viewer};
use serde_json::json;

use super::gui::Args;
use super::process::on_path;

pub(super) const FPS: [u32; 3] = [30, 60, 120];
const SIZE: [u32; 2] = [1586, 992];
const ENCODER: &str = "pcod";
const CRF: &str = "14";
const LARGEST_COPY: u64 = 256 * 1024 * 1024;
const COMMAND: &str = "gui --clip";
const QUEUED: usize = 4;
const PNG_WRITERS: usize = 4;
const BEAT: Duration = Duration::from_millis(200);

struct Idle {
    queued: bool,
    last: Instant,
}

impl Idle {
    fn new() -> Idle {
        Idle {
            queued: std::env::var_os("GPU_QUEUE_PID").is_some()
                || std::env::var_os("GPU_QUEUE").is_some(),
            last: Instant::now(),
        }
    }

    fn beat(&mut self) {
        if self.queued && self.last.elapsed() >= BEAT {
            let _ = Command::new("pgpu")
                .arg("turn")
                .stdout(Stdio::null())
                .status();
            self.last = Instant::now();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Poster {
    First,
    Last,
    At(f32),
}

pub(super) fn poster(text: &str) -> Result<Poster, String> {
    match text {
        "first" => Ok(Poster::First),
        "last" => Ok(Poster::Last),
        _ => match text.parse::<f32>() {
            Ok(seconds) if seconds.is_finite() && seconds >= 0.0 => Ok(Poster::At(seconds)),
            _ => Err("not first, last, or a time in seconds".into()),
        },
    }
}

impl Poster {
    fn frame(self, fps: u32, frames: usize) -> Result<usize, String> {
        match self {
            Poster::First => Ok(0),
            Poster::Last => Ok(frames.saturating_sub(1)),
            Poster::At(seconds) => {
                let frame = (seconds * fps as f32).round() as usize;
                if frame < frames {
                    Ok(frame)
                } else {
                    Err(format!(
                        "the poster at {seconds} s is past the clip's last frame, at {:.3} s",
                        frames.saturating_sub(1) as f32 / fps as f32
                    ))
                }
            }
        }
    }

    fn word(self) -> String {
        match self {
            Poster::First => "first".into(),
            Poster::Last => "last".into(),
            Poster::At(seconds) => seconds.to_string(),
        }
    }
}

fn pcod_args(fps: u32) -> Vec<String> {
    [
        "--fps",
        &fps.to_string(),
        "--codec",
        "h264",
        "--route",
        "cpu",
        "--crf",
        CRF,
    ]
    .map(str::to_string)
    .to_vec()
}

fn encode_line(frames: &Path, fps: u32, out: &Path) -> Vec<String> {
    let mut line = vec!["frames".to_string(), frames.display().to_string()];
    line.extend(pcod_args(fps));
    line.push(out.display().to_string());
    line
}

fn pcod_version() -> Result<String, String> {
    let said = Command::new(ENCODER)
        .arg("--version")
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("cannot start pcod: {error}"))?;
    String::from_utf8_lossy(&said.stdout)
        .split_whitespace()
        .nth(1)
        .map(str::to_string)
        .ok_or_else(|| "pcod --version named no version".into())
}

#[derive(Debug, PartialEq)]
enum Place {
    Nothing,
    Start,
    Folder(PathBuf),
    Recipe(PathBuf),
    Scene(PathBuf),
}

fn place(args: &Args) -> Result<Place, String> {
    if args.start {
        return Ok(Place::Start);
    }
    let Some(target) = &args.target else {
        return Ok(Place::Nothing);
    };
    let path = PathBuf::from(target);
    if path.is_dir() {
        Ok(Place::Folder(path))
    } else if !path.is_file() {
        Err(format!("{target}: no such recipe, scene or folder"))
    } else if manfred_ui::is_scene(&path) {
        Ok(Place::Scene(path))
    } else {
        Ok(Place::Recipe(path))
    }
}

fn copy_tree(from: &Path, to: &Path, skip: &Path, left: &mut u64) -> Result<(), String> {
    let named = |path: &Path, error: std::io::Error| format!("{}: {error}", path.display());
    std::fs::create_dir_all(to).map_err(|error| named(to, error))?;
    let mut entries: Vec<_> = std::fs::read_dir(from)
        .map_err(|error| named(from, error))?
        .flatten()
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        if name == ".git" || name == "target" {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            let real = path.canonicalize().map_err(|error| named(&path, error))?;
            if real != skip
                && !real.starts_with(skip)
                && !entry.file_type().is_ok_and(|kind| kind.is_symlink())
            {
                copy_tree(&path, &to.join(&name), skip, left)?;
            }
        } else if meta.is_file() {
            if meta.len() > *left {
                return Err(format!(
                    "{} holds more than {} MiB; clip a recipe from a smaller folder",
                    from.display(),
                    LARGEST_COPY / 1024 / 1024
                ));
            }
            *left -= meta.len();
            std::fs::copy(&path, to.join(&name)).map_err(|error| named(&path, error))?;
        }
    }
    Ok(())
}

fn copied(path: &Path, work: &Path, skip: &Path) -> Result<PathBuf, String> {
    let real = path
        .canonicalize()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let folder = if real.is_dir() {
        real.clone()
    } else {
        real.parent().map(Path::to_path_buf).unwrap_or_default()
    };
    let name = folder
        .file_name()
        .map_or_else(|| PathBuf::from("folder"), PathBuf::from);
    let mut left = LARGEST_COPY;
    copy_tree(&folder, &work.join(&name), skip, &mut left)?;
    Ok(match real.file_name().filter(|_| !real.is_dir()) {
        Some(file) => name.join(file),
        None => name,
    })
}

fn following(
    shell: Shell,
    (registry, pick): (&Registry, &super::Picks),
) -> (Shell, Option<Viewer>) {
    let recipe = PathBuf::from(shell.recipe_path());
    let viewer = Viewer::with(super::gui::picked(
        Follow::idle_with(recipe, Mode::Stage, registry.clone()),
        pick,
    ));
    (shell, Some(viewer))
}

fn opened(
    place: &Place,
    work: &Path,
    skip: &Path,
    (registry, pick): (&Registry, &super::Picks),
) -> Result<(Shell, Option<Viewer>), String> {
    let text = |path: &Path| path.display().to_string();
    Ok(match place {
        Place::Nothing => (Shell::new(), None),
        Place::Start => {
            let recipe = manfred_ui::start_recipe(work)
                .map_err(|error| format!("the start scene is not written: {error}"))?;
            let recipe = recipe.strip_prefix(work).unwrap_or(&recipe).to_path_buf();
            following(Shell::start(&text(&recipe)), (registry, pick))
        }
        Place::Folder(folder) => (Shell::browse(&text(&copied(folder, work, skip)?)), None),
        Place::Recipe(recipe) => {
            let recipe = copied(recipe, work, skip)?;
            following(Shell::open(&text(&recipe)), (registry, pick))
        }
        Place::Scene(scene) => {
            let scene = copied(scene, work, skip)?;
            let viewer = Viewer::scene(&scene, Mode::Stage);
            (Shell::scene(&text(&scene)), Some(viewer))
        }
    })
}

pub fn run(args: &Args, rest: &[String], registry: &Registry) -> ExitCode {
    match film(args, rest, registry) {
        Ok(code) => code,
        Err(message) => super::refuse(COMMAND, &message),
    }
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(super) fn scale(text: &str) -> Result<f32, String> {
    match text.parse::<f32>() {
        Ok(factor) if factor.is_finite() && (1.0..=4.0).contains(&factor) => Ok(factor),
        _ => Err("not a factor from 1 to 4".into()),
    }
}

fn frame_name(index: usize) -> String {
    format!("frame-{index:05}.png")
}

type Writer = std::thread::JoinHandle<Result<(), String>>;

fn png_writers(dir: PathBuf, size: [u32; 2], frames: Receiver<(usize, Vec<u8>)>) -> Vec<Writer> {
    let shared = Arc::new(Mutex::new(frames));
    (0..PNG_WRITERS)
        .map(|_| {
            let (shared, dir) = (shared.clone(), dir.clone());
            std::thread::spawn(move || -> Result<(), String> {
                loop {
                    let next = shared
                        .lock()
                        .map_err(|_| "the frame writers stopped".to_string())?
                        .recv();
                    let Ok((index, rgba)) = next else {
                        return Ok(());
                    };
                    let png = manfred_ui::frame_png(size, &rgba).map_err(|error| error.0)?;
                    let path = dir.join(frame_name(index));
                    std::fs::write(&path, png)
                        .map_err(|error| format!("{}: {error}", path.display()))?;
                }
            })
        })
        .collect()
}

fn encode(frames: &Path, fps: u32, out: &Path, idle: &mut Idle) -> Result<(), String> {
    let mut child = Command::new(ENCODER)
        .args(encode_line(frames, fps, out))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start pcod: {error}"))?;
    let mut stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(stderr) = stderr.as_mut() {
            let _ = std::io::Read::read_to_string(stderr, &mut text);
        }
        text
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {
                std::thread::sleep(Duration::from_millis(50));
                idle.beat();
            }
            Err(error) => break Err(error),
        }
    };
    let said = reader.join().unwrap_or_default();
    let status = status.map_err(|error| format!("pcod: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        let line = said.lines().last().unwrap_or("").trim().to_string();
        Err(format!("pcod frames failed ({status}): {line}"))
    }
}

fn film(args: &Args, rest: &[String], registry: &Registry) -> Result<ExitCode, String> {
    let out = args.clip.clone().unwrap_or_default();
    let tape_path = args.tape.clone().unwrap_or_default();
    let size = args.size.unwrap_or(SIZE);
    let fps = args.fps.unwrap_or(FPS[0]);
    let poster = args.poster.unwrap_or(Poster::First);
    let layout = Layout {
        size,
        scale: args.scale.unwrap_or(1.0),
        window: args.framed.then(|| args.window.unwrap_or(SIZE)),
    };
    let named = |path: &Path, error: &dyn std::fmt::Display| format!("{}: {error}", path.display());
    let bytes = std::fs::read(&tape_path).map_err(|error| named(&tape_path, &error))?;
    let tape_sha256 = sha256(&bytes);
    let text = String::from_utf8(bytes).map_err(|error| named(&tape_path, &error))?;
    let tape = Tape::parse(&text).map_err(|error| named(&tape_path, &error))?;
    let steps = manfred_ui::tape_steps(&tape, fps, layout.points())
        .map_err(|error| named(&tape_path, &error))?;
    let frames = steps.len();
    let poster_frame = poster.frame(fps, frames)?;
    let place = place(args)?;
    if args.bare && matches!(place, Place::Nothing | Place::Folder(_)) {
        return Err(
            "--bare films a recipe's or a scene's viewport alone; name one, or --start".into(),
        );
    }
    let encoder = if args.frames {
        None
    } else {
        if !on_path(ENCODER) {
            return Err(
                "no pcod on the PATH; a clip's master is encoded by pito-codecs' pcod".into(),
            );
        }
        if size.iter().any(|side| side % 2 == 1) {
            return Err(format!(
                "pcod encodes even sizes; {}x{} is odd",
                size[0], size[1]
            ));
        }
        Some(pcod_version()?)
    };
    if let Some(code) = super::process::requeue("gui", rest, "draws directly") {
        return Ok(code);
    }
    std::fs::create_dir_all(&out).map_err(|error| named(&out, &error))?;
    let out = out.canonicalize().map_err(|error| named(&out, &error))?;
    let shot = tape_path.file_stem().map_or_else(
        || "tape".to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let stem = format!("{shot}-{}x{}-{fps}fps", size[0], size[1]);
    let master = out.join(format!("{stem}.master.mp4"));
    let partial = out.join(format!("{stem}.partial.mp4"));
    let folder = out.join(&stem);
    let partial_folder = out.join(format!("{stem}.partial"));
    let poster_path = out.join(format!("{stem}.poster.png"));
    let manifest = out.join(format!("{stem}.json"));
    let work = out.join(format!("{stem}.work"));
    for dir in [&work, &partial_folder] {
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(|error| named(dir, &error))?;
        }
    }
    std::fs::create_dir_all(&work).map_err(|error| named(&work, &error))?;
    let (shell, viewer) = opened(&place, &work, &out, (registry, &args.pick))?;
    std::env::set_current_dir(&work).map_err(|error| named(&work, &error))?;
    let mut shell = shell.without_recent().without_showcase();
    if args.bare {
        shell = shell.bare().strip(args.strip);
    }
    shell.set_registry(registry.clone());
    super::gui::shade(args, &mut shell);
    let (frames_in, frames_out) = mpsc::sync_channel::<(usize, Vec<u8>)>(QUEUED);
    std::fs::create_dir_all(&partial_folder).map_err(|error| named(&partial_folder, &error))?;
    let writers = png_writers(partial_folder.clone(), size, frames_out);
    let mut idle = Idle::new();
    let mut kept = None;
    let played = manfred_ui::play(
        &mut Manfred::new(&mut shell, viewer),
        &tape,
        layout,
        fps,
        |index, rgba| {
            if index == poster_frame && !args.frames {
                kept = Some(rgba.to_vec());
            }
            let mut frame = (index, rgba.to_vec());
            loop {
                match frames_in.try_send(frame) {
                    Ok(()) => break,
                    Err(TrySendError::Full(back)) => {
                        frame = back;
                        std::thread::sleep(Duration::from_millis(10));
                        idle.beat();
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        return Err("the frame writer stopped taking frames".into());
                    }
                }
            }
            if args.json {
                eprintln!(
                    "{}",
                    json!({"stage": "frame", "frame": index, "done": index + 1, "total": frames})
                );
            }
            Ok(())
        },
    );
    drop(frames_in);
    let written: Result<Vec<()>, String> = writers
        .into_iter()
        .map(|writer| {
            writer
                .join()
                .unwrap_or_else(|_| Err("the frame writer stopped".into()))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .collect();
    let _ = std::env::set_current_dir(&out);
    let fail = |message: String| {
        let _ = std::fs::remove_file(&partial);
        let _ = std::fs::remove_dir_all(&partial_folder);
        message
    };
    let played = played.map_err(|error| fail(error.0))?;
    written.map_err(fail)?;
    if encoder.is_some() {
        encode(&partial_folder, fps, &partial, &mut idle).map_err(fail)?;
        let _ = std::fs::remove_dir_all(&partial_folder);
    }
    let bytes = |path: &Path| std::fs::metadata(path).map_or(0, |meta| meta.len());
    let file = |path: &Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    };
    let mut params = json!({
        "size": size,
        "fps": fps,
        "scale": layout.scale,
        "framed": args.framed,
        "window": layout.window,
        "pixels_per_point": layout.pixels_per_point(),
        "drawn": layout.drawn(),
        "bare": args.bare,
        "strip": args.strip,
    });
    let target = if args.start {
        Some("start".to_string())
    } else {
        args.target.clone()
    };
    let (value, written): (serde_json::Value, Vec<&PathBuf>) = if args.frames {
        if folder.exists() {
            std::fs::remove_dir_all(&folder).map_err(|error| fail(named(&folder, &error)))?;
        }
        std::fs::rename(&partial_folder, &folder).map_err(|error| fail(named(&folder, &error)))?;
        let each: Vec<serde_json::Value> = steps
            .iter()
            .enumerate()
            .take(played)
            .map(|(index, step)| {
                let name = frame_name(index);
                json!({
                    "file": format!("{stem}/{name}"),
                    "role": "frame",
                    "frame": index,
                    "bytes": bytes(&folder.join(&name)),
                    "step": step,
                    "line": step.and_then(|step| tape.line(step)),
                })
            })
            .collect();
        let value = json!({
            "tool": {"name": "manfred", "version": super::VERSION},
            "kind": "window-frames",
            "target": target,
            "tape": tape_path.display().to_string(),
            "tape_sha256": tape_sha256,
            "params": params,
            "frames": {
                "count": played,
                "seconds": played as f64 / f64::from(fps),
                "folder": stem,
                "pattern": "frame-%05d.png",
            },
            "outputs": {
                "frames": "8-bit sRGB PNGs, one a frame from frame-00000.png, as drawn before any encoding",
            },
            "files": each,
        });
        (value, vec![&folder, &manifest])
    } else {
        std::fs::rename(&partial, &master).map_err(|error| fail(named(&master, &error)))?;
        let still = kept.ok_or_else(|| fail("the poster's frame was never drawn".into()))?;
        let png = manfred_ui::poster_png(size, &still).map_err(|error| fail(error.0))?;
        std::fs::write(&poster_path, &png).map_err(|error| fail(named(&poster_path, &error)))?;
        params["poster"] = poster.word().into();
        let value = json!({
            "tool": {"name": "manfred", "version": super::VERSION},
            "encoder": {"name": ENCODER, "version": encoder},
            "kind": "window-clip",
            "target": target,
            "tape": tape_path.display().to_string(),
            "tape_sha256": tape_sha256,
            "params": params,
            "frames": {
                "master": played,
                "seconds": played as f64 / f64::from(fps),
                "poster": poster_frame,
            },
            "outputs": {
                "master": "H.264 by pito-codecs' pcod frames on the CPU, CRF 14, 8-bit 4:2:0, BT.709, a silent stereo AAC track",
                "poster": "8-bit sRGB PNG of the master's frame before encoding",
            },
            "files": [
                {"file": file(&master), "role": "master", "bytes": bytes(&master), "fps": fps, "size": size},
                {"file": file(&poster_path), "role": "poster", "bytes": bytes(&poster_path), "fps": fps, "size": size, "frame": poster_frame},
            ],
            "encodes": [
                {"file": file(&master), "role": "master", "fps": fps, "size": size, "encoder": ENCODER, "args": pcod_args(fps)},
            ],
        });
        (value, vec![&master, &poster_path, &manifest])
    };
    let text = serde_json::to_string_pretty(&value).map_err(|error| fail(error.to_string()))?;
    std::fs::write(&manifest, text + "\n").map_err(|error| fail(named(&manifest, &error)))?;
    let _ = std::fs::remove_dir_all(&work);
    if args.json {
        let files: Vec<String> = written
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        println!(
            "{}",
            json!({
                "stage": "done",
                "kind": value["kind"],
                "frames": played,
                "seconds": played as f64 / f64::from(fps),
                "manifest": manifest.display().to_string(),
                "files": files,
            })
        );
    } else {
        for path in written {
            println!("{}", path.display());
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Result<Args, String> {
        let words: Vec<&str> = std::iter::once("gui")
            .chain(text.split_whitespace())
            .collect();
        match super::super::parsed(&words)? {
            super::super::Verb::Gui(args) => Ok(args),
            _ => Err("not gui".into()),
        }
    }

    #[test]
    fn a_clip_wants_a_tape_and_keeps_to_its_own_options() {
        let parsed =
            args("showcase --clip tmp/c --tape t.tape --size 800x500 --fps 60 --poster 1.5")
                .unwrap();
        assert_eq!(parsed.clip, Some(PathBuf::from("tmp/c")));
        assert_eq!(parsed.tape, Some(PathBuf::from("t.tape")));
        assert_eq!(parsed.size, Some([800, 500]));
        assert_eq!(parsed.fps, Some(60));
        assert_eq!(parsed.poster, Some(Poster::At(1.5)));
        assert!(args("--clip tmp/c --tape t.tape --start").unwrap().start);
        let framed = args(
            "--clip tmp/c --tape t.tape --size 2160x3840 --scale 2 --framed --window 1600x1000 --frames --json",
        )
        .unwrap();
        assert_eq!(
            (
                framed.scale,
                framed.framed,
                framed.window,
                framed.frames,
                framed.json
            ),
            (Some(2.0), true, Some([1600, 1000]), true, true)
        );
        let bare = args("a.toml --clip tmp/c --tape t.tape --bare --size 1080x1920").unwrap();
        assert!(bare.bare && !bare.framed);
        for text in [
            "--clip tmp/c",
            "--tape t.tape",
            "--clip tmp/c --tape t.tape --fps 24",
            "--clip tmp/c --tape t.tape --poster soon",
            "--clip tmp/c --tape t.tape --shot a.png",
            "--clip tmp/c --tape t.tape --input s.txt",
            "--shot a.png --fps 30",
            "--fps 30",
            "--clip tmp/c --tape t.tape --scale 5",
            "--clip tmp/c --tape t.tape --scale 0.5",
            "--clip tmp/c --tape t.tape --window 800x600",
            "--clip tmp/c --tape t.tape --frames --poster last",
            "--shot a.png --frames",
            "--shot a.png --json",
            "--clip tmp/c --tape t.tape --bare --framed",
            "--bare --tape t.tape",
        ] {
            assert!(args(text).is_err(), "{text}");
        }
    }

    #[test]
    fn the_master_is_pcod_frames_on_the_cpu_at_crf_14() {
        let line = encode_line(
            Path::new("out/t.partial"),
            60,
            Path::new("out/t.partial.mp4"),
        );
        assert_eq!(
            line.join(" "),
            "frames out/t.partial --fps 60 --codec h264 --route cpu --crf 14 out/t.partial.mp4"
        );
    }

    #[test]
    fn a_poster_is_a_frame_inside_the_clip() {
        assert_eq!(Poster::First.frame(30, 90), Ok(0));
        assert_eq!(Poster::Last.frame(30, 90), Ok(89));
        assert_eq!(Poster::At(1.0).frame(30, 90), Ok(30));
        assert!(Poster::At(3.0).frame(30, 90).is_err());
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_short_tape_writes_its_frames_and_a_manifest_the_same_twice() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tmp/tape-tests/frames");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tape = dir.join("still.tape");
        std::fs::write(&tape, "wait 0.1\nframe\n").unwrap();
        let line = format!(
            "--clip {} --tape {} --frames --size 320x200 --scale 2",
            dir.join("out").display(),
            tape.display()
        );
        let read = || {
            film(&args(&line).unwrap(), &[], &Registry::new()).unwrap();
            let folder = dir.join("out/still-320x200-30fps");
            (0..5)
                .map(|frame| {
                    let path = folder.join(frame_name(frame));
                    assert_eq!(manfred_scene::read_png_size(&path).unwrap(), [320, 200]);
                    std::fs::read(path).unwrap()
                })
                .collect::<Vec<_>>()
        };
        let first = read();
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("out/still-320x200-30fps.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["kind"], "window-frames");
        assert_eq!(manifest["frames"]["count"], 5);
        assert_eq!(manifest["params"]["size"], json!([320, 200]));
        assert_eq!(manifest["files"].as_array().unwrap().len(), 5);
        assert_eq!(manifest["files"][4]["line"], 2);
        assert_eq!(manifest["tape_sha256"].as_str().unwrap().len(), 64);
        assert!(!dir.join("out/still-320x200-30fps/frame-00005.png").exists());
        assert!(read() == first, "the same tape drew different frames");
    }
}
