use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::builder::{PossibleValuesParser, TypedValueParser};
use manfred_build::Registry;
use manfred_ui::{Fixture, Follow, Manfred, Mode, Shell, Viewer};

use super::{Host, size};

fn fixtures() -> impl TypedValueParser<Value = Fixture> {
    PossibleValuesParser::new(Fixture::ALL.map(Fixture::name))
        .map(|name| Fixture::from_name(&name).unwrap_or(Fixture::ALL[0]))
}

fn rates() -> impl TypedValueParser<Value = u32> {
    PossibleValuesParser::new(["30", "60", "120"])
        .map(|fps| fps.parse().unwrap_or(super::tape::FPS[0]))
}

#[derive(clap::Args, Debug, Default, PartialEq)]
#[command(group(clap::ArgGroup::new("offscreen").args(["shot", "clip"])))]
pub(super) struct Args {
    #[arg(
        value_name = "recipe|folder",
        help = "A recipe, a .scene.toml, or a folder to pick one from [default: the start scene]"
    )]
    pub(super) target: Option<String>,
    #[arg(
        long,
        value_name = "png",
        help = "Draw one frame offscreen into this PNG"
    )]
    pub(super) shot: Option<String>,
    #[arg(
        long,
        value_name = "dir",
        conflicts_with_all = ["shot", "input", "fake", "fixture"],
        requires = "tape",
        help = "Film the window playing a tape: an x264 master, its poster and a manifest in this folder"
    )]
    pub(super) clip: Option<PathBuf>,
    #[arg(
        long,
        value_name = "file",
        requires = "clip",
        help = "The tape a clip plays: the script's steps, one a line, and wait <seconds>"
    )]
    pub(super) tape: Option<PathBuf>,
    #[arg(
        long,
        value_name = "F",
        requires = "clip",
        value_parser = rates(),
        help = "A clip's frames per second [default: 30]"
    )]
    pub(super) fps: Option<u32>,
    #[arg(
        long,
        value_name = "when",
        requires = "clip",
        value_parser = super::tape::poster,
        help = "A clip's poster: first, last, or the frame at a time in seconds [default: first]"
    )]
    pub(super) poster: Option<super::tape::Poster>,
    #[arg(
        long,
        requires = "clip",
        conflicts_with = "poster",
        help = "Write a clip as PNG frames in <dir>/<stem>/ instead of an x264 master; no ffmpeg needed"
    )]
    pub(super) frames: bool,
    #[arg(
        long,
        value_name = "factor",
        requires = "clip",
        value_parser = super::tape::scale,
        help = "A clip's pixels per point, from 1 to 4: --size 2160x3840 --scale 2 lays the window out at 1080x1920 [default: 1]"
    )]
    pub(super) scale: Option<f32>,
    #[arg(
        long,
        requires = "clip",
        help = "Lay a clip's window out at --window and centre it in the --size frame on the editor's ground, fitted to the frame"
    )]
    pub(super) framed: bool,
    #[arg(
        long,
        value_name = "WxH",
        requires = "framed",
        value_parser = size,
        help = "A framed clip's window in points [default: 1586x992]"
    )]
    pub(super) window: Option<[u32; 2]>,
    #[arg(
        long,
        requires = "clip",
        help = "A clip's progress as JSON lines on stderr and one JSON line of the files written on stdout"
    )]
    pub(super) json: bool,
    #[arg(
        long,
        value_name = "WxH",
        requires = "offscreen",
        value_parser = size,
        help = "The frame's size [default: 1586x992, or the fixture's]"
    )]
    pub(super) size: Option<[u32; 2]>,
    #[arg(
        long,
        value_name = "script",
        requires = "shot",
        help = "Play a script of keys and clicks, one step a line, before the frame"
    )]
    pub(super) input: Option<String>,
    #[arg(
        long,
        requires_all = ["shot", "target"],
        help = "Read the recipe without building it"
    )]
    pub(super) fake: bool,
    #[arg(
        long,
        value_name = "name",
        requires = "shot",
        conflicts_with = "target",
        value_parser = fixtures(),
        help = "Draw one widget of the editor style"
    )]
    pub(super) fixture: Option<Fixture>,
    #[arg(
        long,
        requires = "offscreen",
        conflicts_with_all = ["target", "fixture"],
        help = "Draw the start scene"
    )]
    pub(super) start: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Empty,
    Start(String),
    Recipe(String),
    Scene(String),
    Folder(String),
}

fn target(arg: Option<&str>) -> Target {
    match arg {
        None => Target::Empty,
        Some(path) if Path::new(path).is_dir() => Target::Folder(path.to_string()),
        Some(path) if manfred_ui::is_scene(Path::new(path)) => Target::Scene(path.to_string()),
        Some(path) => Target::Recipe(path.to_string()),
    }
}

pub fn run(
    args: &Args,
    rest: &[String],
    registry: &Registry,
    host: &Host,
    installed: bool,
) -> ExitCode {
    if args.shot.is_some() {
        return super::shot::run(args, registry);
    }
    if args.clip.is_some() {
        return super::tape::run(args, rest, registry);
    }
    let mut choice = target(args.target.as_deref());
    if choice == Target::Empty {
        match manfred_ui::start_recipe_here() {
            Ok(path) => choice = Target::Start(path.display().to_string()),
            Err(error) => eprintln!("manfred gui: the start scene is not written: {error}"),
        }
    }
    if let Target::Recipe(path) | Target::Scene(path) = &choice {
        Shell::remember_recent(path);
    }
    let mut shell = match &choice {
        Target::Recipe(path) => Shell::open(path),
        Target::Start(path) => Shell::start(path),
        Target::Scene(path) => Shell::scene(path),
        Target::Folder(path) => Shell::browse(path),
        Target::Empty => Shell::empty(),
    };
    let pulse = host.beat.and_then(|beat| beat(shell.label().as_deref()));
    shell.set_registry(registry.clone());
    if installed {
        shell = shell.with_installed_showcase();
    }
    let viewer = match &choice {
        Target::Recipe(path) | Target::Start(path) => Some(Viewer::with(Follow::idle_with(
            path,
            Mode::Stage,
            registry.clone(),
        ))),
        Target::Scene(path) => Some(Viewer::scene(path, Mode::Stage)),
        Target::Empty | Target::Folder(_) => None,
    };
    match manfred_ui::window(Manfred::new(&mut shell, viewer), pulse) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("manfred gui: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tmp/gui-tests")
            .join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_folder_is_a_folder_a_scene_toml_is_a_scene_and_a_file_or_a_missing_path_is_a_recipe() {
        let dir = scratch("target");
        let file = dir.join("scene.toml");
        std::fs::write(&file, "manfred = 1\nname = \"scene\"\n").unwrap();
        let folder = dir.join("scenes");
        std::fs::create_dir_all(&folder).unwrap();
        let missing = dir.join("missing.toml");
        let scene = dir.join("room.scene.toml");
        assert_eq!(target(None), Target::Empty);
        assert_eq!(
            target(Some(file.to_str().unwrap())),
            Target::Recipe(file.display().to_string())
        );
        assert_eq!(
            target(Some(folder.to_str().unwrap())),
            Target::Folder(folder.display().to_string())
        );
        assert_eq!(
            target(Some(missing.to_str().unwrap())),
            Target::Recipe(missing.display().to_string())
        );
        assert_eq!(
            target(Some(scene.to_str().unwrap())),
            Target::Scene(scene.display().to_string())
        );
    }
}
