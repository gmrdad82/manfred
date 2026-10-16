use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use manfred_build::{Registry, build_with, write};
use manfred_recipe::toml_edit::DocumentMut;
use serde_json::{Map, Value, json};

use super::process::on_path;
use super::refuse;
use super::timeline::{base, chosen};

pub(super) const MANIFEST: &str = "shoot.json";
pub(super) const WAV: &str = "sound.wav";
const STAGE: &str = ".stage-";
const IEND: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82];

#[derive(clap::Args)]
pub(super) struct Args {
    #[arg(value_name = "recipe", help = "The recipe to build and shoot")]
    recipe: String,
    #[arg(
        long,
        value_name = "dir",
        help = "The folder of the frames, the WAV and shoot.json"
    )]
    out: PathBuf,
    #[arg(
        long,
        conflicts_with = "final_",
        help = "Draft quality, the default: pfx's half-size sun shadow map"
    )]
    draft: bool,
    #[arg(
        id = "final_",
        long = "final",
        help = "Final quality: full shadow maps, TAA warmed before the first frame"
    )]
    final_: bool,
    #[arg(
        long,
        value_name = "a..b",
        value_parser = frames,
        help = "Only frames a to b, both included; every frame of the timeline when absent"
    )]
    range: Option<RangeInclusive<u32>>,
    #[arg(
        long,
        value_name = "name",
        help = "The [[camera]] to shoot through; [output] scene_camera, else the first, when absent"
    )]
    camera: Option<String>,
    #[arg(
        long,
        help = "One JSON line per frame, then shoot.json as the last line"
    )]
    json: bool,
}

fn frames(text: &str) -> Result<RangeInclusive<u32>, String> {
    let parsed = text
        .split_once("..")
        .and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?)));
    match parsed {
        Some((a, b)) if a <= b => Ok(a..=b),
        _ => Err("not a..b, two frame numbers with a at most b".into()),
    }
}

pub(super) fn frame_name(frame: u32) -> String {
    format!("frame-{frame:05}.png")
}

fn complete(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut tail = [0u8; 12];
    file.seek(SeekFrom::End(-12)).is_ok() && file.read_exact(&mut tail).is_ok() && tail == IEND
}

pub(super) fn runs(range: RangeInclusive<u32>, done: impl Fn(u32) -> bool) -> Vec<(u32, u32)> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for frame in range {
        if done(frame) {
            continue;
        }
        match runs.last_mut() {
            Some((from, count)) if *from + *count == frame => *count += 1,
            _ => runs.push((frame, 1)),
        }
    }
    runs
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Quality {
    Draft,
    Final,
}

pub(super) struct Plan<'a> {
    pub(super) scene: &'a Path,
    pub(super) camera: &'a str,
    pub(super) fps: u32,
    pub(super) size: [u32; 2],
    pub(super) quality: Quality,
    pub(super) from: u32,
    pub(super) frames: u32,
    pub(super) stage: &'a Path,
    pub(super) audio: Option<&'a Path>,
}

pub(super) fn pfx_line(plan: &Plan, queue: bool) -> Vec<String> {
    let mut line: Vec<String> = Vec::new();
    if queue {
        line.extend(["pgpu", "run", "--class", "clip", "--as", "manfred", "--"].map(String::from));
    }
    line.extend(["pfx".to_string(), "shot".to_string()]);
    line.push(plan.scene.display().to_string());
    line.extend([
        "--from".to_string(),
        plan.from.to_string(),
        "--frames".to_string(),
        plan.frames.to_string(),
        "--fps".to_string(),
        plan.fps.to_string(),
        "--size".to_string(),
        format!("{}x{}", plan.size[0], plan.size[1]),
        "--camera".to_string(),
        plan.camera.to_string(),
        match plan.quality {
            Quality::Draft => "--draft",
            Quality::Final => "--final",
        }
        .to_string(),
        "--out".to_string(),
        plan.stage.display().to_string(),
    ]);
    if let Some(wav) = plan.audio {
        line.extend(["--audio".to_string(), wav.display().to_string()]);
    }
    line.push("--json".to_string());
    line
}

pub(super) fn pfx_manifest(from: u32) -> String {
    format!("pfx-{from:05}.json")
}

fn stage_of(dir: &Path, from: u32) -> PathBuf {
    dir.join(format!("{STAGE}{from:05}"))
}

fn harvest(dir: &Path) -> Result<Vec<u32>, String> {
    let mut moved = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(moved);
    };
    let mut stages: Vec<(u32, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let from = name.strip_prefix(STAGE)?.parse::<u32>().ok()?;
            Some((from, entry.path()))
        })
        .collect();
    stages.sort();
    for (from, stage) in stages {
        for entry in std::fs::read_dir(&stage)
            .map_err(|e| format!("cannot read {}: {e}", stage.display()))?
            .filter_map(Result::ok)
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(index) = name
                .strip_suffix(".png")
                .and_then(|n| n.parse::<u32>().ok())
            else {
                continue;
            };
            if !complete(&entry.path()) {
                continue;
            }
            let frame = from + index;
            std::fs::rename(entry.path(), dir.join(frame_name(frame)))
                .map_err(|e| format!("cannot move {}: {e}", entry.path().display()))?;
            moved.push(frame);
        }
        let manifest = stage.join("manifest.json");
        if manifest.is_file() {
            std::fs::rename(&manifest, dir.join(pfx_manifest(from)))
                .map_err(|e| format!("cannot move {}: {e}", manifest.display()))?;
        }
        std::fs::remove_dir_all(&stage)
            .map_err(|e| format!("cannot remove {}: {e}", stage.display()))?;
    }
    moved.sort_unstable();
    Ok(moved)
}

fn has_sound(scene: &Path) -> bool {
    std::fs::read_to_string(scene)
        .ok()
        .and_then(|text| text.parse::<DocumentMut>().ok())
        .is_some_and(|doc| doc.get("sound").is_some_and(|item| item.is_str()))
}

fn previous_runs(dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(dir.join(MANIFEST))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|manifest| manifest["runs"].as_array().cloned())
        .unwrap_or_default()
}

struct Shoot<'a> {
    args: &'a Args,
    out: &'a mut dyn Write,
}

impl Shoot<'_> {
    fn say(&mut self, line: &Value) {
        if self.args.json {
            let _ = writeln!(self.out, "{line}");
        }
    }

    fn run(&mut self, registry: &Registry) -> Result<(), String> {
        let args = self.args;
        let file = args.recipe.as_str();
        let recipe = chosen(file)?;
        let timeline = recipe
            .timeline
            .ok_or_else(|| format!("{file} has no [timeline], so there is nothing to shoot"))?;
        let scene_file = recipe.output.scene.clone().ok_or_else(|| {
            format!("{file} has no [output] scene; pfx shoots the scene file the build writes")
        })?;
        let name = args
            .camera
            .clone()
            .or_else(|| recipe.output.scene_camera.clone())
            .or_else(|| recipe.cameras.first().map(|c| c.name.clone()))
            .ok_or_else(|| format!("{file} has no [[camera]] to shoot through"))?;
        manfred_scene::camera(&recipe, &name).map_err(|e| e.0)?;
        let node = format!("camera.{name}");
        let range = args.range.clone().unwrap_or(0..=timeline.frames);
        if *range.end() > timeline.frames {
            return Err(format!(
                "--range {}..{} runs past the timeline's last frame, {}",
                range.start(),
                range.end(),
                timeline.frames
            ));
        }
        if !on_path("pfx") {
            return Err("pfx is not on the PATH; shoot renders with `pfx shot`".into());
        }
        let queued = std::env::var_os("GPU_QUEUE").is_some();
        let queue = !queued && on_path("pgpu");
        if !queued && !queue {
            eprintln!(
                "manfred shoot: no GPU queue (pgpu) on the PATH, so pfx runs directly, paced the same"
            );
        }
        let base = base(file);
        let built = build_with(&recipe, base, registry).map_err(|e| format!("{file}: {e}"))?;
        for note in &built.notes {
            eprintln!("{file}: {note}");
        }
        let written = write(&built, &recipe, base, None).map_err(|e| format!("{file}: {e}"))?;
        let glb = written
            .iter()
            .find(|p| p.extension().is_some_and(|e| e == "glb"))
            .cloned()
            .ok_or_else(|| format!("{file}: the build wrote no GLB"))?;
        let wanted = Path::new(&scene_file).file_name();
        let scene = written
            .iter()
            .find(|p| p.file_name() == wanted)
            .cloned()
            .ok_or_else(|| format!("{file}: the build wrote no scene file"))?;
        let dir = &args.out;
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        harvest(dir)?;
        let quality = if args.final_ {
            Quality::Final
        } else {
            Quality::Draft
        };
        let wav = dir.join(WAV);
        let sound = has_sound(&scene);
        let todo = runs(range.clone(), |frame| {
            complete(&dir.join(frame_name(frame)))
        });
        let total = range.end() - range.start() + 1;
        let skipped = total - todo.iter().map(|(_, count)| count).sum::<u32>();
        let mut runs_done = previous_runs(dir);
        let mut pfx_version = Value::Null;
        let mut failure = None;
        for (from, count) in todo {
            let stage = stage_of(dir, from);
            let whole = from == 0 && from + count - 1 == timeline.frames;
            let audio = (sound && whole && !wav.is_file()).then_some(wav.as_path());
            let plan = Plan {
                scene: &scene,
                camera: &node,
                fps: timeline.fps,
                size: timeline.size,
                quality,
                from,
                frames: count,
                stage: &stage,
                audio,
            };
            let line = pfx_line(&plan, queue);
            let mut child = Command::new(&line[0])
                .args(&line[1..])
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|e| format!("cannot start {}: {e}", line[0]))?;
            let mut manifest = Value::Null;
            if let Some(stdout) = child.stdout.take() {
                for text in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let Ok(value) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    if let Some(index) = value["frame"].as_u64() {
                        self.say(&json!({
                            "frame": u64::from(from) + index,
                            "of": total,
                            "time": value["time"],
                            "gpu_ms": value["gpu_ms"],
                            "wall_ms": value["wall_ms"],
                        }));
                    } else if value.get("shot").is_some() {
                        manifest = value;
                    }
                }
            }
            let status = child.wait().map_err(|e| format!("pfx shot: {e}"))?;
            let moved = harvest(dir)?;
            if !manifest["pfx"].is_null() {
                pfx_version = manifest["pfx"].clone();
            }
            runs_done.push(json!({
                "from": from,
                "frames": count,
                "manifest": pfx_manifest(from),
                "written": moved.len(),
                "quality": match quality { Quality::Draft => "draft", Quality::Final => "final" },
                "wall_ms": manifest["wall_ms"],
                "gpu_ms": manifest["gpu_ms"],
                "warnings": manifest["warnings"],
            }));
            if !status.success() {
                failure = Some(format!(
                    "pfx shot stopped in frames {from} to {} ({status}); run the same shoot again to resume",
                    from + count - 1
                ));
                break;
            }
        }
        let files: Vec<String> = range
            .clone()
            .map(frame_name)
            .filter(|name| dir.join(name).is_file())
            .collect();
        let mut manifest = Map::new();
        manifest.insert("shoot".into(), json!(1));
        manifest.insert("recipe".into(), json!(file));
        manifest.insert("scene".into(), json!(scene.display().to_string()));
        manifest.insert("glb".into(), json!(glb.display().to_string()));
        manifest.insert("camera".into(), json!(node));
        manifest.insert("renderer".into(), json!("live"));
        manifest.insert(
            "quality".into(),
            json!(match quality {
                Quality::Draft => "draft",
                Quality::Final => "final",
            }),
        );
        manifest.insert("fps".into(), json!(timeline.fps));
        manifest.insert("size".into(), json!(timeline.size));
        manifest.insert("range".into(), json!([range.start(), range.end()]));
        manifest.insert("frames".into(), json!(total));
        manifest.insert("skipped".into(), json!(skipped));
        manifest.insert("files".into(), json!(files));
        manifest.insert(
            "wav".into(),
            if wav.is_file() {
                json!(WAV)
            } else {
                Value::Null
            },
        );
        manifest.insert("sound".into(), json!(sound));
        manifest.insert("pfx".into(), pfx_version);
        manifest.insert("runs".into(), json!(runs_done));
        let manifest = Value::Object(manifest);
        let text = serde_json::to_string_pretty(&manifest).unwrap_or_default();
        std::fs::write(dir.join(MANIFEST), format!("{text}\n"))
            .map_err(|e| format!("cannot write {}: {e}", dir.join(MANIFEST).display()))?;
        if let Some(failure) = failure {
            return Err(failure);
        }
        if args.json {
            let _ = writeln!(self.out, "{manifest}");
        } else {
            let _ = writeln!(
                self.out,
                "{}: {} of {total} frames ({skipped} already there){}",
                dir.display(),
                files.len(),
                if wav.is_file() {
                    format!(", {WAV}")
                } else {
                    String::new()
                }
            );
        }
        Ok(())
    }
}

pub fn run(args: &Args, registry: &Registry) -> ExitCode {
    let mut stdout = std::io::stdout();
    let mut shoot = Shoot {
        args,
        out: &mut stdout,
    };
    match shoot.run(registry) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => refuse("shoot", &message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(words: &[&str]) -> Result<Args, String> {
        match super::super::parsed(&[&["shoot"], words].concat())? {
            super::super::Verb::Shoot(args) => Ok(args),
            _ => Err("not shoot".into()),
        }
    }

    #[test]
    fn shoot_takes_its_flags_and_refuses_what_it_cannot_parse() {
        let args = parsed(&[
            "a.toml", "--out", "tmp/s", "--final", "--range", "10..20", "--camera", "chase",
            "--json",
        ])
        .unwrap();
        assert_eq!(args.recipe, "a.toml");
        assert_eq!(args.out, PathBuf::from("tmp/s"));
        assert!(args.final_ && !args.draft && args.json);
        assert_eq!(args.range, Some(10..=20));
        assert_eq!(args.camera.as_deref(), Some("chase"));
        let plain = parsed(&["a.toml", "--out", "o"]).unwrap();
        assert!(!plain.final_ && plain.range.is_none() && plain.camera.is_none());
        assert!(parsed(&["a.toml"]).is_err());
        assert!(parsed(&["a.toml", "--out", "o", "--trace"]).is_err());
        assert!(parsed(&["a.toml", "--out", "o", "--draft", "--final"]).is_err());
        for bad in ["5..2", "a..b", "7", "1...3"] {
            assert!(
                parsed(&["a.toml", "--out", "o", "--range", bad]).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn missing_frames_group_into_runs_so_a_shoot_resumes() {
        let done = [0, 1, 2, 5, 9];
        assert_eq!(
            runs(0..=10, |f| done.contains(&f)),
            [(3, 2), (6, 3), (10, 1)]
        );
        assert_eq!(runs(0..=2, |_| true), []);
        assert_eq!(runs(4..=6, |_| false), [(4, 3)]);
    }

    #[test]
    fn pfx_shot_gets_the_scene_the_range_and_the_camera_node() {
        let plan = Plan {
            scene: Path::new("out/street.scene.toml"),
            camera: "camera.chase",
            fps: 60,
            size: [1080, 1920],
            quality: Quality::Final,
            from: 30,
            frames: 12,
            stage: Path::new("shots/.stage-00030"),
            audio: Some(Path::new("shots/sound.wav")),
        };
        assert_eq!(
            pfx_line(&plan, true).join(" "),
            "pgpu run --class clip --as manfred -- pfx shot out/street.scene.toml --from 30 --frames 12 --fps 60 --size 1080x1920 --camera camera.chase --final --out shots/.stage-00030 --audio shots/sound.wav --json"
        );
        let direct = pfx_line(
            &Plan {
                quality: Quality::Draft,
                audio: None,
                ..plan
            },
            false,
        );
        assert_eq!(direct[0], "pfx");
        assert!(direct.contains(&"--draft".to_string()));
        assert!(!direct.contains(&"--audio".to_string()));
    }
}
