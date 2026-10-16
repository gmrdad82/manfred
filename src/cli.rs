mod build;
mod check;
mod clip;
mod curve;
mod diff;
mod document;
mod entries;
mod export;
mod gui;
mod import;
mod key;
mod library;
mod library_add;
mod library_bake;
mod new;
mod process;
mod render;
mod route;
mod set;
mod shoot;
mod shot;
mod still;
mod tape;
mod timeline;

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use clap::{Arg, CommandFactory, FromArgMatches, Parser, Subcommand};
use manfred_build::Registry;
use manfred_recipe::{
    Camera, Error, Pick, Problem, Trace, apply_set, apply_variant, load, validate,
};

pub use manfred_ui::Pulse;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const RAW: &str = "args";

pub type Beat = fn(Option<&str>) -> Option<Box<dyn Pulse>>;

pub type Run = fn(&[String], &Registry) -> ExitCode;

#[derive(Clone, Copy)]
pub struct Command {
    pub name: &'static str,
    pub usage: &'static str,
    pub run: Run,
}

#[derive(Clone, Copy, Default)]
pub struct Host {
    pub beat: Option<Beat>,
    pub usage: &'static str,
    pub commands: &'static [Command],
}

#[derive(Parser)]
#[command(
    name = "manfred",
    version,
    about = "Scenes as recipes: GLB headless, a live window and path-traced stills",
    arg_required_else_help = false
)]
struct Line {
    #[command(subcommand)]
    verb: Verb,
}

#[derive(Subcommand)]
enum Verb {
    #[command(about = "Build a recipe into its GLB, its sidecar and its bake recipe")]
    Build(build::Args),
    #[command(about = "Validate a recipe and list its problems")]
    Check(check::Args),
    #[command(about = "Compare two GLBs by structure, or draw both alike and compare them by PSNR")]
    Diff(diff::Args),
    #[command(about = "One still on the live renderer, headless")]
    Shot(still::Args),
    #[command(about = "A still path-traced on the engine's tracer")]
    Render(render::Args),
    #[command(about = "A camera moving along the recipe's path, as frames and a video")]
    Clip(clip::Args),
    #[command(about = "The window, or one frame of it offscreen with --shot")]
    Gui(gui::Args),
    #[command(
        about = "List a recipe's routes, or move, add or remove a route's points in the file"
    )]
    Route(route::Args),
    #[command(
        about = "Write values into a recipe's file by dotted key, keeping its comments, and check it"
    )]
    Set(set::SetArgs),
    #[command(about = "Print a recipe's values by dotted key")]
    Get(set::GetArgs),
    #[command(
        about = "List a generator object's curve param, or move, add or remove its points in the file"
    )]
    Curve(curve::Args),
    #[command(
        about = "Set or insert a key of an animatable property in the recipe, as object.hero.at at 2 s"
    )]
    Key(key::KeyArgs),
    #[command(about = "Remove one key of a property, or every key of it")]
    Unkey(key::UnkeyArgs),
    #[command(
        about = "Write a starter recipe",
        subcommand_required = true,
        arg_required_else_help = false
    )]
    New(new::Args),
    #[command(
        about = "Report a recipe's timeline: frames, keyed nodes, the camera and what travels"
    )]
    Timeline(timeline::Args),
    #[command(about = "Build a recipe and render its timeline's frames with pfx shot, resumably")]
    Shoot(shoot::Args),
    #[command(
        about = "The object library: list, index, check, bake and add library/<kind>/<name> models and sidecars",
        subcommand_required = true,
        arg_required_else_help = false
    )]
    Library(library::Args),
    #[command(about = "Append an entry to one of the recipe's arrays of tables, in the file")]
    Add(entries::Add),
    #[command(about = "Remove an entry from the recipe, refused while another entry refers to it")]
    Remove(entries::Remove),
    #[command(about = "Copy an entry under a new name, with keys changed")]
    Duplicate(entries::Duplicate),
    #[command(about = "List the recipe's entries and their names")]
    List(entries::List),
    #[command(about = "Pack a recipe, its files and its build into one .manf document")]
    Save(document::SaveArgs),
    #[command(about = "Write a .manf document's recipe and files into a folder")]
    Unpack(document::UnpackArgs),
    #[command(
        about = "List the scene edits pfx made in a .manf, fold them into the recipe or keep them"
    )]
    Fold(document::FoldArgs),
    #[command(
        about = "Read a glTF 2.0 binary into a .manf document with its recipe and detected facts"
    )]
    Import(import::Args),
    #[command(
        about = "Write a .manf or a recipe as a standalone glTF 2.0 binary with no Manfred extras"
    )]
    Export(export::Args),
}

fn line(host: &Host) -> clap::Command {
    let mut line = Line::command();
    for app in host.commands {
        if Verb::has_subcommand(app.name) {
            continue;
        }
        line = line.subcommand(
            clap::Command::new(app.name)
                .about(app.usage)
                .override_usage(app.usage)
                .arg(
                    Arg::new(RAW)
                        .num_args(0..)
                        .trailing_var_arg(true)
                        .allow_hyphen_values(true),
                ),
        );
    }
    if !host.usage.is_empty() {
        line = line.after_help(host.usage);
    }
    line
}

fn answer(error: &clap::Error) -> ExitCode {
    let _ = error.print();
    ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(2))
}

fn refuse(command: &str, message: &str) -> ExitCode {
    eprintln!("manfred {command}: {message}");
    ExitCode::FAILURE
}

fn size(text: &str) -> Result<[u32; 2], String> {
    let parsed = text
        .split_once(['x', 'X'])
        .and_then(|(w, h)| Some([w.parse::<u32>().ok()?, h.parse::<u32>().ok()?]));
    match parsed {
        Some([w, h]) if w > 0 && h > 0 && w <= 16384 && h <= 16384 => Ok([w, h]),
        _ => Err("not WxH, with each side from 1 to 16384".into()),
    }
}

fn seconds(text: &str) -> Result<f64, String> {
    match text.parse::<f64>() {
        Ok(at) if at.is_finite() && at >= 0.0 => Ok(at),
        _ => Err("not a number of seconds from 0 up".into()),
    }
}

#[derive(clap::Args, Clone, Debug, Default, PartialEq)]
pub(super) struct Picks {
    #[arg(
        long,
        value_name = "N|name",
        help = "Build only the objects of the recipe's stages 1 to N (a number or a stage's name)"
    )]
    pub(super) stage: Option<String>,
    #[arg(
        long,
        value_name = "glob[,glob]",
        value_delimiter = ',',
        help = "Build only these objects, or <object>.<piece> pieces; globs take * and ?"
    )]
    pub(super) only: Vec<String>,
}

impl Picks {
    pub(super) fn pick(&self) -> Pick {
        Pick {
            stage: self.stage.clone(),
            only: self.only.clone(),
        }
    }
}

fn pair(text: &str) -> Result<String, String> {
    match text.split_once('=') {
        Some((key, _)) if !key.trim().is_empty() => Ok(text.to_string()),
        _ => Err("expected <key>=<value>".into()),
    }
}

fn read_failure(file: &str, e: &Error) -> String {
    let line = e.line.map_or_else(String::new, |l| format!(":{l}"));
    if e.path.is_empty() || e.path == file {
        format!("{file}{line}: {}", e.text)
    } else {
        format!("{file}{line}: {}: {}", e.path, e.text)
    }
}

fn print_problems(out: &mut dyn Write, file: &str, problems: &[Problem]) {
    for p in problems {
        let _ = writeln!(
            out,
            "{file}:{}: {}: {}",
            p.line.unwrap_or(1),
            p.path,
            p.text
        );
    }
}

fn precheck(
    recipe: &Path,
    variant: Option<&str>,
    set: &[String],
    camera: &str,
    time: Option<f64>,
) -> Result<(Camera, Trace), String> {
    precheck_picked(recipe, variant, set, &Pick::default(), camera, time)
}

fn precheck_picked(
    recipe: &Path,
    variant: Option<&str>,
    set: &[String],
    pick: &Pick,
    camera: &str,
    time: Option<f64>,
) -> Result<(Camera, Trace), String> {
    let file = recipe.display().to_string();
    let loaded = load(recipe).map_err(|e| read_failure(&file, &e))?;
    let chosen = match variant {
        Some(name) => apply_variant(&loaded.recipe, name),
        None => Ok(loaded.recipe),
    }
    .and_then(|chosen| apply_set(&chosen, set))
    .and_then(|chosen| pick.apply(&chosen))
    .map_err(|e| read_failure(&file, &e))?;
    let mut problems = Vec::new();
    print_problems(&mut problems, &file, &validate(&chosen));
    if !problems.is_empty() {
        return Err(String::from_utf8_lossy(&problems).trim_end().to_string());
    }
    if let Some(refusal) = time.and_then(|at| manfred_build::time_refusal(&chosen, at)) {
        return Err(refusal);
    }
    manfred_scene::camera(&chosen, camera)
        .cloned()
        .map(|camera| (camera, chosen.trace))
        .map_err(|e| e.0)
}

pub fn version_line() -> String {
    format!("manfred {VERSION}")
}

pub fn main(args: &[String], registry: &Registry) -> ExitCode {
    main_with(args, registry, &Host::default())
}

pub fn main_with(args: &[String], registry: &Registry, host: &Host) -> ExitCode {
    route(args, registry, host, false)
}

pub fn main_installed(args: &[String], registry: &Registry, host: &Host) -> ExitCode {
    route(args, registry, host, true)
}

fn route(args: &[String], registry: &Registry, host: &Host, installed: bool) -> ExitCode {
    let words = std::iter::once("manfred").chain(args.iter().map(String::as_str));
    let matches = match line(host).try_get_matches_from(words) {
        Ok(matches) => matches,
        Err(error) => return answer(&error),
    };
    if let Some((name, given)) = matches.subcommand()
        && !Verb::has_subcommand(name)
        && let Some(app) = host.commands.iter().find(|app| app.name == name)
    {
        let rest: Vec<String> = given
            .get_many::<String>(RAW)
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        return (app.run)(&rest, registry);
    }
    let verb = match Line::from_arg_matches(&matches) {
        Ok(line) => line.verb,
        Err(error) => return answer(&error),
    };
    let rest = args.get(1..).unwrap_or_default();
    let mut verb = verb;
    let opened = match document::swap(&mut verb) {
        Ok(opened) => opened,
        Err((name, e)) => return refuse(name, &e),
    };
    let code = run_verb(verb, rest, registry, host, installed);
    match opened.map(|opened| opened.finish(registry)) {
        Some(Err(e)) => refuse("document", &e),
        _ => code,
    }
}

fn run_verb(
    verb: Verb,
    rest: &[String],
    registry: &Registry,
    host: &Host,
    installed: bool,
) -> ExitCode {
    match verb {
        Verb::Build(parsed) => build::run(&parsed, registry),
        Verb::Check(parsed) => check::run(&parsed, registry),
        Verb::Diff(parsed) => diff::run(&parsed, registry),
        Verb::Shot(parsed) => still::run(&parsed, registry),
        Verb::Render(parsed) => render::run(&parsed, rest, registry),
        Verb::Clip(parsed) => clip::run(&parsed, rest, registry),
        Verb::Gui(parsed) => gui::run(&parsed, rest, registry, host, installed),
        Verb::Route(parsed) => route::run(&parsed, registry),
        Verb::Set(parsed) => set::run_set(&parsed, registry),
        Verb::Get(parsed) => set::run_get(&parsed, registry),
        Verb::Curve(parsed) => curve::run(&parsed),
        Verb::New(parsed) => new::run(&parsed),
        Verb::Timeline(parsed) => timeline::run(&parsed, registry),
        Verb::Shoot(parsed) => shoot::run(&parsed, registry),
        Verb::Library(parsed) => library::run(&parsed, registry),
        Verb::Key(parsed) => key::key(&parsed, registry),
        Verb::Unkey(parsed) => key::unkey(&parsed, registry),
        Verb::Add(parsed) => entries::add(&parsed),
        Verb::Remove(parsed) => entries::remove(&parsed),
        Verb::Duplicate(parsed) => entries::duplicate(&parsed),
        Verb::List(parsed) => entries::list(&parsed),
        Verb::Save(parsed) => document::save(&parsed, registry),
        Verb::Unpack(parsed) => document::unpack(&parsed),
        Verb::Fold(parsed) => document::fold(&parsed, registry),
        Verb::Import(parsed) => import::run(&parsed, registry),
        Verb::Export(parsed) => export::run(&parsed, registry),
    }
}

#[cfg(test)]
fn parsed(words: &[&str]) -> Result<Verb, String> {
    Line::try_parse_from(std::iter::once("manfred").chain(words.iter().copied()))
        .map(|line| line.verb)
        .map_err(|error| error.render().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_the_estate_format() {
        let line = version_line();
        let (name, semver) = line.split_once(' ').unwrap();
        assert_eq!(name, "manfred");
        assert_eq!(semver.split('.').count(), 3);
        assert!(semver.split('.').all(|p| p.parse::<u32>().is_ok()));
        let version = Line::command().render_version();
        assert_eq!(version.trim_end(), line);
    }

    #[test]
    fn sizes_are_width_by_height() {
        assert_eq!(size("1586x992"), Ok([1586, 992]));
        assert_eq!(size("64X32"), Ok([64, 32]));
        for bad in ["1586", "0x10", "10x", "x10", "axb", "20000x10"] {
            assert!(size(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_line_it_cannot_parse_gets_clap_s_short_answer() {
        let rule = |error: &str, usage: Option<&str>| {
            let usage = usage.map_or(String::new(), |usage| format!("Usage: {usage}\n\n"));
            format!("{error}\n\n{usage}For more information, try '--help'.\n")
        };
        for (words, wanted) in [
            (
                &["nope"][..],
                rule(
                    "error: unrecognized subcommand 'nope'",
                    Some("manfred <COMMAND>"),
                ),
            ),
            (
                &["--x"][..],
                rule(
                    "error: unexpected argument '--x' found",
                    Some("manfred <COMMAND>"),
                ),
            ),
            (
                &["check", "a.toml", "--x"][..],
                rule(
                    "error: unexpected argument '--x' found\n\n  tip: to pass '--x' as a value, use '-- --x'",
                    Some("manfred check <recipe>"),
                ),
            ),
            (
                &["build", "a.toml", "--out"][..],
                rule(
                    "error: a value is required for '--out <dir>' but none was supplied",
                    None,
                ),
            ),
            (
                &[][..],
                rule(
                    "error: 'manfred' requires a subcommand but one was not provided\n  [subcommands: build, check, diff, shot, render, clip, gui, route, curve, set, get, key, unkey, new, timeline, shoot, library, add, remove, duplicate, list, save, unpack, fold, import, export, help]",
                    Some("manfred <COMMAND>"),
                ),
            ),
        ] {
            let error = parsed(words).err().unwrap_or_default();
            assert_eq!(error, wanted, "{words:?}");
            assert!(!error.contains("Commands:"), "{error}");
        }
        let error = Line::try_parse_from(["manfred", "nope"]).err().unwrap();
        assert_eq!(error.exit_code(), 2);
        assert!(error.use_stderr());
    }

    #[test]
    fn help_prints_the_whole_help_to_stdout_and_exits_0() {
        for words in [
            &["--help"][..],
            &["-h"],
            &["help"],
            &["help", "render"],
            &["render", "a.toml", "--help"],
        ] {
            let error =
                Line::try_parse_from(std::iter::once("manfred").chain(words.iter().copied()))
                    .err()
                    .unwrap();
            assert_eq!(error.exit_code(), 0, "{words:?}");
            assert!(!error.use_stderr(), "{words:?}");
        }
    }

    const FULL: &str = "crates/recipe/tests/fixtures/full.toml";

    #[test]
    fn the_precheck_names_what_is_wrong_before_any_queue() {
        let ok = precheck(Path::new(FULL), None, &[], "wide", None).unwrap();
        assert_eq!(ok.0.name, "wide");
        let camera = precheck(Path::new(FULL), None, &[], "nope", None).unwrap_err();
        assert!(camera.contains("camera.nope: no such camera"), "{camera}");
        assert!(camera.contains("main, wide, plan"), "{camera}");
        let missing =
            precheck(Path::new("nowhere/none.toml"), None, &[], "main", None).unwrap_err();
        assert_eq!(
            missing,
            "nowhere/none.toml: cannot read the recipe: no such file"
        );
        let variant = precheck(Path::new(FULL), Some("nope"), &[], "main", None).unwrap_err();
        assert!(variant.contains("nope"), "{variant}");
        let set = precheck(
            Path::new(FULL),
            None,
            &["no.such.key=1".into()],
            "main",
            None,
        )
        .unwrap_err();
        assert!(set.contains("no.such.key"), "{set}");
        let loop_file = "crates/recipe/tests/fixtures/errors/v_parent_loop.toml";
        let invalid = precheck(Path::new(loop_file), None, &[], "main", None).unwrap_err();
        assert!(invalid.contains("the parents form a loop"), "{invalid}");
    }

    fn echo(args: &[String], registry: &Registry) -> ExitCode {
        if args == ["ok", "--loud"]
            && registry
                .names()
                .all(|name| name.starts_with(manfred_build::BUILTIN))
        {
            ExitCode::from(7)
        } else {
            ExitCode::from(9)
        }
    }

    const ECHO: Host = Host {
        beat: None,
        usage: "Also:\n  manfred --echo-test",
        commands: &[
            Command {
                name: "echo",
                usage: "manfred echo ok",
                run: echo,
            },
            Command {
                name: "build",
                usage: "manfred build mine",
                run: echo,
            },
        ],
    };

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn an_app_adds_its_own_command_through_the_host() {
        assert_eq!(
            main_with(&strings(&["echo", "ok", "--loud"]), &Registry::new(), &ECHO),
            ExitCode::from(7)
        );
        assert_eq!(
            main_with(&strings(&["build"]), &Registry::new(), &ECHO),
            ExitCode::from(2)
        );
        assert_eq!(
            main_with(&strings(&["nope"]), &Registry::new(), &ECHO),
            ExitCode::from(2)
        );
        let help = line(&ECHO).render_long_help().to_string();
        assert!(help.contains("manfred echo ok"), "{help}");
        assert!(!help.contains("manfred build mine"), "{help}");
        assert!(help.ends_with("Also:\n  manfred --echo-test\n"), "{help}");
    }

    #[test]
    fn the_readme_names_every_command_and_its_flags() {
        let readme = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md"),
        )
        .unwrap();
        let block = readme
            .split("## Commands")
            .nth(1)
            .and_then(|rest| rest.split("```").nth(1))
            .unwrap();
        for command in Line::command().get_subcommands() {
            let name = command.get_name();
            if name == "help" {
                continue;
            }
            let lines: Vec<&str> = block
                .lines()
                .filter(|row| row.trim_start().starts_with(&format!("manfred {name} ")))
                .collect();
            assert!(!lines.is_empty(), "README omits manfred {name}");
            for arg in command.get_arguments() {
                let Some(long) = arg.get_long() else {
                    continue;
                };
                if long == "help" {
                    continue;
                }
                let flag = format!("--{long}");
                assert!(
                    lines.iter().any(|row| row
                        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                        .any(|word| word == flag)),
                    "README's manfred {name} omits {flag}"
                );
            }
        }
    }
}
