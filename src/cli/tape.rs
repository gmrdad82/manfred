use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc::{self, TrySendError};
use std::time::{Duration, Instant};

use manfred_build::Registry;
use manfred_ui::{Follow, Manfred, Mode, Shell, Tape, Viewer};
use serde_json::json;

use super::gui::Args;
use super::process::on_path;

pub(super) const FPS: [u32; 3] = [30, 60, 120];
const SIZE: [u32; 2] = [1586, 992];
const LIFT: &str = "format=gbrp,zscale,format=gbrp16le";
const TO_709: &str = "format=rgb48le,zscale=matrix=709:range=limited:filter=bicubic:chromal=left,format=yuv420p,setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv";
const X264_THREADS: &str = "24";
const LARGEST_COPY: u64 = 256 * 1024 * 1024;
const COMMAND: &str = "gui --clip";
const QUEUED: usize = 4;
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

fn x264(fps: u32) -> Vec<String> {
    let gop = (2 * fps).to_string();
    [
        "-vf",
        &format!("{LIFT},{TO_709}"),
        "-c:v",
        "libx264",
        "-threads",
        X264_THREADS,
        "-preset",
        "slow",
        "-profile:v",
        "high",
        "-tune",
        "animation",
        "-pix_fmt",
        "yuv420p",
        "-crf",
        "14",
        "-g",
        &gop,
        "-keyint_min",
        &gop,
        "-sc_threshold",
        "0",
        "-color_primaries",
        "bt709",
        "-color_trc",
        "bt709",
        "-colorspace",
        "bt709",
        "-color_range",
        "tv",
        "-movflags",
        "+faststart+write_colr",
        "-an",
    ]
    .map(str::to_string)
    .to_vec()
}

fn encode_line(size: [u32; 2], fps: u32, out: &Path) -> Vec<String> {
    let mut line: Vec<String> = [
        "-hide_banner",
        "-v",
        "error",
        "-y",
        "-filter_threads",
        "1",
        "-filter_complex_threads",
        "1",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-s",
        &format!("{}x{}", size[0], size[1]),
        "-framerate",
        &fps.to_string(),
        "-i",
        "-",
    ]
    .map(str::to_string)
    .to_vec();
    line.extend(x264(fps));
    line.extend([
        "-f".to_string(),
        "mp4".to_string(),
        out.display().to_string(),
    ]);
    line
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

fn following(shell: Shell, recipe: PathBuf, registry: &Registry) -> (Shell, Option<Viewer>) {
    let viewer = Viewer::with(Follow::idle_with(recipe, Mode::Stage, registry.clone()));
    (shell, Some(viewer))
}

fn opened(
    place: &Place,
    work: &Path,
    skip: &Path,
    registry: &Registry,
) -> Result<(Shell, Option<Viewer>), String> {
    let text = |path: &Path| path.display().to_string();
    Ok(match place {
        Place::Nothing => (Shell::new(), None),
        Place::Start => {
            let recipe = manfred_ui::start_recipe(work)
                .map_err(|error| format!("the start scene is not written: {error}"))?;
            let recipe = recipe.strip_prefix(work).unwrap_or(&recipe).to_path_buf();
            following(Shell::start(&text(&recipe)), recipe, registry)
        }
        Place::Folder(folder) => (Shell::browse(&text(&copied(folder, work, skip)?)), None),
        Place::Recipe(recipe) => {
            let recipe = copied(recipe, work, skip)?;
            following(Shell::open(&text(&recipe)), recipe, registry)
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

fn film(args: &Args, rest: &[String], registry: &Registry) -> Result<ExitCode, String> {
    let out = args.clip.clone().unwrap_or_default();
    let tape_path = args.tape.clone().unwrap_or_default();
    let size = args.size.unwrap_or(SIZE);
    let fps = args.fps.unwrap_or(FPS[0]);
    let poster = args.poster.unwrap_or(Poster::First);
    let named = |path: &Path, error: &dyn std::fmt::Display| format!("{}: {error}", path.display());
    let text = std::fs::read_to_string(&tape_path).map_err(|error| named(&tape_path, &error))?;
    let tape = Tape::parse(&text).map_err(|error| named(&tape_path, &error))?;
    let frames =
        manfred_ui::tape_frames(&tape, fps, size).map_err(|error| named(&tape_path, &error))?;
    let poster_frame = poster.frame(fps, frames)?;
    let place = place(args)?;
    if !on_path("ffmpeg") {
        return Err("no ffmpeg on the PATH; a clip's master is x264, made by ffmpeg".into());
    }
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
    let partial = out.join(format!("{stem}.master.mp4.partial"));
    let poster_path = out.join(format!("{stem}.poster.png"));
    let manifest = out.join(format!("{stem}.json"));
    let work = out.join(format!("{stem}.work"));
    if work.exists() {
        std::fs::remove_dir_all(&work).map_err(|error| named(&work, &error))?;
    }
    std::fs::create_dir_all(&work).map_err(|error| named(&work, &error))?;
    let (shell, viewer) = opened(&place, &work, &out, registry)?;
    std::env::set_current_dir(&work).map_err(|error| named(&work, &error))?;
    let mut shell = shell.without_recent().without_showcase();
    shell.set_registry(registry.clone());
    let mut ffmpeg = Command::new("ffmpeg")
        .args(encode_line(size, fps, &partial))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start ffmpeg: {error}"))?;
    let mut input = ffmpeg.stdin.take().ok_or("ffmpeg took no input")?;
    let (frames_in, frames_out) = mpsc::sync_channel::<Vec<u8>>(QUEUED);
    let writer = std::thread::spawn(move || -> Result<(), String> {
        for frame in frames_out {
            input
                .write_all(&frame)
                .map_err(|error| format!("ffmpeg stopped taking frames: {error}"))?;
        }
        Ok(())
    });
    let mut idle = Idle::new();
    let mut kept = None;
    let played = manfred_ui::play(
        &mut Manfred::new(&mut shell, viewer),
        &tape,
        size,
        fps,
        |index, rgba| {
            if index == poster_frame {
                kept = Some(rgba.to_vec());
            }
            let mut frame = rgba.to_vec();
            loop {
                match frames_in.try_send(frame) {
                    Ok(()) => break,
                    Err(TrySendError::Full(back)) => {
                        frame = back;
                        std::thread::sleep(Duration::from_millis(10));
                        idle.beat();
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        return Err("ffmpeg stopped taking frames".into());
                    }
                }
            }
            Ok(())
        },
    );
    drop(frames_in);
    let written = writer
        .join()
        .unwrap_or_else(|_| Err("the frame writer stopped".into()));
    let ended = loop {
        match ffmpeg.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {
                std::thread::sleep(Duration::from_millis(50));
                idle.beat();
            }
            Err(error) => break Err(error),
        }
    };
    let mut said = String::new();
    if let Some(mut stderr) = ffmpeg.stderr.take() {
        let _ = std::io::Read::read_to_string(&mut stderr, &mut said);
    }
    let _ = std::env::set_current_dir(&out);
    let fail = |message: String| {
        let _ = std::fs::remove_file(&partial);
        message
    };
    let played = played.map_err(|error| fail(error.0))?;
    let status = ended.map_err(|error| fail(format!("ffmpeg: {error}")))?;
    if !status.success() {
        return Err(fail(format!("ffmpeg failed ({status}): {}", said.trim())));
    }
    written.map_err(fail)?;
    std::fs::rename(&partial, &master).map_err(|error| fail(named(&master, &error)))?;
    let still = kept.ok_or_else(|| fail("the poster's frame was never drawn".into()))?;
    let png = manfred_ui::poster_png(size, &still).map_err(|error| fail(error.0))?;
    std::fs::write(&poster_path, &png).map_err(|error| fail(named(&poster_path, &error)))?;
    let bytes = |path: &Path| std::fs::metadata(path).map_or(0, |meta| meta.len());
    let file = |path: &Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    };
    let value = json!({
        "tool": {"name": "manfred", "version": super::VERSION},
        "kind": "window-clip",
        "target": if args.start { Some("start".to_string()) } else { args.target.clone() },
        "tape": tape_path.display().to_string(),
        "params": {"size": size, "fps": fps, "poster": poster.word()},
        "frames": {
            "master": played,
            "seconds": played as f64 / f64::from(fps),
            "poster": poster_frame,
        },
        "outputs": {
            "master": "x264 High, CRF 14, 8-bit 4:2:0, BT.709 limited range",
            "poster": "8-bit sRGB PNG of the master's frame before encoding",
        },
        "files": [
            {"file": file(&master), "role": "master", "bytes": bytes(&master), "fps": fps, "size": size},
            {"file": file(&poster_path), "role": "poster", "bytes": bytes(&poster_path), "fps": fps, "size": size, "frame": poster_frame},
        ],
        "encodes": [
            {"file": file(&master), "role": "master", "fps": fps, "size": size, "args": x264(fps)},
        ],
    });
    let text = serde_json::to_string_pretty(&value).map_err(|error| fail(error.to_string()))?;
    std::fs::write(&manifest, text + "\n").map_err(|error| fail(named(&manifest, &error)))?;
    let _ = std::fs::remove_dir_all(&work);
    for path in [&master, &poster_path, &manifest] {
        println!("{}", path.display());
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
        for text in [
            "--clip tmp/c",
            "--tape t.tape",
            "--clip tmp/c --tape t.tape --fps 24",
            "--clip tmp/c --tape t.tape --poster soon",
            "--clip tmp/c --tape t.tape --shot a.png",
            "--clip tmp/c --tape t.tape --input s.txt",
            "--shot a.png --fps 30",
            "--fps 30",
        ] {
            assert!(args(text).is_err(), "{text}");
        }
    }

    #[test]
    fn the_master_is_x264_high_through_zscale_on_fixed_threads() {
        let line = encode_line([640, 360], 60, Path::new("out.mp4.partial"));
        let joined = line.join(" ");
        assert!(joined.starts_with(
            "-hide_banner -v error -y -filter_threads 1 -filter_complex_threads 1 -f rawvideo -pix_fmt rgba -s 640x360 -framerate 60 -i - -vf format=gbrp,zscale,format=gbrp16le,format=rgb48le,zscale=matrix=709"
        ));
        assert!(joined.contains("-c:v libx264 -threads 24 -preset slow -profile:v high -tune animation -pix_fmt yuv420p -crf 14 -g 120 -keyint_min 120 -sc_threshold 0"));
        assert!(joined.ends_with("-an -f mp4 out.mp4.partial"));
    }

    #[test]
    fn a_poster_is_a_frame_inside_the_clip() {
        assert_eq!(Poster::First.frame(30, 90), Ok(0));
        assert_eq!(Poster::Last.frame(30, 90), Ok(89));
        assert_eq!(Poster::At(1.0).frame(30, 90), Ok(30));
        assert!(Poster::At(3.0).frame(30, 90).is_err());
    }
}
