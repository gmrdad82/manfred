use std::path::PathBuf;
use std::process::ExitCode;

use clap::builder::{PossibleValuesParser, TypedValueParser};
use manfred_build::Registry;
use manfred_render::{EQUIRECT_SIZE, Fault, Format, Rendered, Request, SAMPLES, SIZE, Step, View};

use super::{pair, refuse, size};

const STAGES: [&str; 3] = ["stage", "trace", "write"];
const PROGRESS_LINES: u32 = 20;

fn output(text: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(text);
    match Format::of(&path) {
        Some(_) => Ok(path),
        None => Err("ends in neither .png, .exr nor .hdr".into()),
    }
}

#[derive(clap::Args, Debug, PartialEq)]
pub(super) struct Args {
    #[arg(value_name = "recipe", help = "The recipe to render")]
    recipe: PathBuf,
    #[arg(
        long,
        value_name = "name",
        help = "The recipe's camera to look through"
    )]
    camera: String,
    #[arg(
        long = "spp",
        value_name = "N",
        default_value_t = SAMPLES,
        value_parser = clap::value_parser!(u32).range(1..),
        help = "Samples per pixel"
    )]
    samples: u32,
    #[arg(
        long,
        value_name = "S",
        help = "The tracer's seed [default: the recipe's seed, cut to 32 bits]"
    )]
    seed: Option<u32>,
    #[arg(
        long = "size",
        value_name = "WxH",
        value_parser = size,
        help = "The picture's size [default: 1586x992, or 2048x1024 with --equirect]"
    )]
    frame: Option<[u32; 2]>,
    #[arg(
        long,
        conflicts_with = "ortho",
        help = "The full sphere around the camera"
    )]
    equirect: bool,
    #[arg(long, help = "An orthographic view of the camera's ortho_height")]
    ortho: bool,
    #[arg(
        long,
        value_name = "name",
        help = "Render this variant instead of the recipe"
    )]
    variant: Option<String>,
    #[arg(
        long,
        value_name = "key=value",
        value_parser = pair,
        help = "Replace a value the recipe has, after the variant; repeatable"
    )]
    set: Vec<String>,
    #[arg(
        long,
        value_name = "off|on",
        default_value = "on",
        action = clap::ArgAction::Set,
        hide_possible_values = true,
        value_parser = PossibleValuesParser::new(["off", "on"]).map(|v| v == "on"),
        help = "Off writes a .png from the raw trace; .exr and .hdr are never denoised"
    )]
    denoise: bool,
    #[arg(
        long,
        value_name = "file",
        value_parser = output,
        help = "The picture to write: .png, .exr or .hdr"
    )]
    out: PathBuf,
}

impl Args {
    fn view(&self) -> View {
        match (self.equirect, self.ortho) {
            (true, _) => View::Equirect,
            (_, true) => View::Ortho,
            _ => View::Lens,
        }
    }

    fn size(&self) -> [u32; 2] {
        self.frame.unwrap_or(if self.view() == View::Equirect {
            EQUIRECT_SIZE
        } else {
            SIZE
        })
    }
}

fn precheck(args: &Args) -> Result<(), String> {
    super::precheck(
        &args.recipe,
        args.variant.as_deref(),
        &args.set,
        &args.camera,
    )
    .map(|_| ())
}

fn requeue(args: &[String]) -> Option<ExitCode> {
    super::process::requeue("render", args, "runs directly")
}

fn record(args: &Args) -> super::process::Record {
    super::process::Record::start(
        &format!("render {}", args.out.display()),
        &format!(
            "{} {} {}x{} {} spp",
            args.recipe.display(),
            args.camera,
            args.size()[0],
            args.size()[1],
            args.samples
        ),
        &args.out,
        &STAGES,
    )
}

fn progress_line(done: u32, total: u32) -> Option<String> {
    let every = (total / PROGRESS_LINES).max(1);
    (done == 0 || done == total || done.is_multiple_of(every))
        .then(|| format!("progress {done}/{total}"))
}

fn note(record: &super::process::Record, step: Step) {
    if let Step::Trace { done, total } = step
        && let Some(line) = progress_line(done, total)
    {
        eprintln!("{line}");
    }
    match step {
        Step::Stage => record.stage(STAGES[0], None, "steps"),
        Step::Trace { done: 0, total } => {
            record.stage(STAGES[1], Some(u64::from(total)), "samples")
        }
        Step::Trace { done, .. } => record.progress(u64::from(done)),
        Step::Write => record.stage(STAGES[2], None, "files"),
    }
}

fn traced(
    args: &Args,
    registry: &Registry,
    progress: &mut dyn FnMut(Step),
) -> Result<(Rendered, String), Fault> {
    let request = Request {
        recipe: &args.recipe,
        camera: &args.camera,
        size: args.size(),
        samples: args.samples,
        seed: args.seed,
        view: args.view(),
        variant: args.variant.as_deref(),
        set: &args.set,
    };
    let rendered = manfred_render::render(&request, registry, progress)?;
    progress(Step::Write);
    manfred_render::write_with(
        &args.out,
        &rendered.film,
        (&rendered.finish, &rendered.overlay),
        args.denoise,
    )?;
    let paced = rendered.paced;
    let summary = format!(
        "{}x{}, {} samples, seed {}: {} submissions, the longest {:.2} ms, {:.0} ms on the GPU",
        args.size()[0],
        args.size()[1],
        args.samples,
        rendered.seed,
        paced.submissions,
        paced.longest_ms,
        paced.gpu_ms
    );
    Ok((rendered, summary))
}

pub fn run(parsed: &Args, args: &[String], registry: &Registry) -> ExitCode {
    if let Err(message) = precheck(parsed) {
        return refuse("render", &message);
    }
    if let Some(code) = requeue(args) {
        return code;
    }
    let record = record(parsed);
    match traced(parsed, registry, &mut |step| note(&record, step)) {
        Ok((_, summary)) => {
            record.finish();
            eprintln!("manfred render: {summary}");
            println!("{}", parsed.out.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            record.fail(&error.to_string());
            refuse("render", &error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::process::{self, Machine};
    use super::*;

    fn line(exe: &str, args: &[String], machine: Machine<'_>) -> Option<Vec<String>> {
        process::line("render", exe, args, machine)
    }

    fn strings(text: &str) -> Vec<String> {
        text.split_whitespace().map(String::from).collect()
    }

    fn parse(words: &[String]) -> Result<Args, String> {
        let words: Vec<&str> = std::iter::once("render")
            .chain(words.iter().map(String::as_str))
            .collect();
        match super::super::parsed(&words)? {
            super::super::Verb::Render(args) => Ok(args),
            _ => Err("not render".into()),
        }
    }

    fn run_line(text: &str) -> ExitCode {
        match parse(&strings(text)) {
            Ok(args) => run(&args, &strings(text), &Registry::new()),
            Err(_) => ExitCode::from(2),
        }
    }

    #[test]
    fn every_flag_is_read() {
        let args = parse(&strings(
            "table.toml --camera main --spp 64 --seed 9 --size 320x200 --ortho --variant bare --set sky.intensity=0.4 --set object.a.params.b=/x/y --denoise off --out tmp/a.exr",
        ))
        .unwrap();
        assert_eq!(
            args,
            Args {
                recipe: "table.toml".into(),
                camera: "main".into(),
                samples: 64,
                seed: Some(9),
                frame: Some([320, 200]),
                equirect: false,
                ortho: true,
                variant: Some("bare".into()),
                set: vec!["sky.intensity=0.4".into(), "object.a.params.b=/x/y".into()],
                denoise: false,
                out: "tmp/a.exr".into(),
            }
        );
    }

    #[test]
    fn the_defaults_are_256_samples_and_the_frame_or_a_two_to_one_sphere() {
        let args = parse(&strings("a.toml --out a.png --camera main")).unwrap();
        assert_eq!(args.samples, 256);
        assert_eq!(args.seed, None);
        assert!(args.denoise);
        assert_eq!(args.size(), [1586, 992]);
        assert_eq!(args.view(), View::Lens);
        let round = parse(&strings("a.toml --equirect --camera main --out sky.hdr")).unwrap();
        assert_eq!(round.view(), View::Equirect);
        assert_eq!(round.size(), [2048, 1024]);
    }

    #[test]
    fn mistakes_are_refused() {
        for text in [
            "",
            "a.toml",
            "a.toml --camera main",
            "a.toml --out a.png",
            "a.toml --camera main --out a.jpg",
            "a.toml --camera main --out a",
            "a.toml --camera main --out a.png --spp 0",
            "a.toml --camera main --out a.png --spp many",
            "a.toml --camera main --out a.png --seed -1",
            "a.toml --camera main --out a.png --size 0x5",
            "a.toml --camera main --out a.png --equirect --ortho",
            "a.toml --camera main --out a.png --nope",
            "a.toml --camera main --out a.png --denoise maybe",
            "a.toml --camera main --out a.png --denoise",
            "a.toml b.toml --camera main --out a.png",
            "a.toml --camera",
            "a.toml --camera main --out a.png --set",
        ] {
            assert!(parse(&strings(text)).is_err(), "{text:?}");
        }
        for text in [
            "a.toml --camera main --camera other --out a.png",
            "a.toml --camera main --out a.png --spp 4 --spp 8",
        ] {
            assert!(parse(&strings(text)).is_err(), "{text:?}");
        }
        assert!(
            parse(&strings(
                "a.toml --camera c --out a.png --set a.b=1 --set c.d=2"
            ))
            .is_ok()
        );
    }

    #[test]
    fn progress_lines_are_stable_and_come_every_few_samples() {
        assert_eq!(progress_line(0, 256).as_deref(), Some("progress 0/256"));
        assert_eq!(progress_line(12, 256).as_deref(), Some("progress 12/256"));
        assert_eq!(progress_line(13, 256), None);
        assert_eq!(progress_line(24, 256).as_deref(), Some("progress 24/256"));
        assert_eq!(progress_line(256, 256).as_deref(), Some("progress 256/256"));
        let lines: Vec<String> = (0..=256).filter_map(|n| progress_line(n, 256)).collect();
        assert_eq!(lines.len(), 23);
        let small: Vec<String> = (0..=3).filter_map(|n| progress_line(n, 3)).collect();
        assert_eq!(
            small,
            [
                "progress 0/3",
                "progress 1/3",
                "progress 2/3",
                "progress 3/3"
            ]
        );
        for line in &lines {
            let (done, total) = line
                .strip_prefix("progress ")
                .and_then(|rest| rest.split_once('/'))
                .unwrap();
            assert!(done.parse::<u32>().unwrap() <= total.parse::<u32>().unwrap());
        }
    }

    #[test]
    fn the_process_line_is_build_slice_then_pgpu_clip_unless_queued() {
        let args = strings("a.toml --camera main --out a.png");
        let full = Machine {
            queued: false,
            pgpu: true,
            systemd: true,
            cpus: Some("2-5"),
        };
        assert_eq!(
            line("/opt/manfred", &args, full).unwrap().join(" "),
            "systemd-run --user --quiet --scope --collect --slice=build.slice -p MemoryMax=12G -p MemorySwapMax=0 -- taskset -c 2-5 nice -n 10 pgpu run --class clip --as manfred -- /opt/manfred render a.toml --camera main --out a.png"
        );
        let bare = Machine {
            systemd: false,
            cpus: None,
            ..full
        };
        assert_eq!(
            line("manfred", &args, bare).unwrap().join(" "),
            "pgpu run --class clip --as manfred -- manfred render a.toml --camera main --out a.png"
        );
        let untasked = Machine { cpus: None, ..full };
        assert!(
            !line("manfred", &args, untasked)
                .unwrap()
                .contains(&"taskset".to_string())
        );
        assert_eq!(
            line(
                "manfred",
                &args,
                Machine {
                    queued: true,
                    ..full
                }
            ),
            None
        );
        assert_eq!(
            line(
                "manfred",
                &args,
                Machine {
                    pgpu: false,
                    ..full
                }
            ),
            None
        );
    }

    #[test]
    fn a_bad_recipe_or_camera_refuses_with_1_before_the_queue() {
        let one = format!("{:?}", ExitCode::FAILURE);
        for line in [
            "nowhere/none.toml --camera main --out tmp/never.png",
            "crates/recipe/tests/fixtures/full.toml --camera nope --out tmp/never.png",
            "crates/recipe/tests/fixtures/full.toml --camera main --variant nope --out tmp/never.png",
        ] {
            assert_eq!(format!("{:?}", run_line(line)), one, "{line}");
        }
    }

    fn linear(byte: u8) -> f64 {
        let v = f64::from(byte) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }

    fn regions(rgba: &[u8], size: [u32; 2], grid: [u32; 2]) -> Vec<f64> {
        let [width, height] = size;
        let mut means = Vec::new();
        for row in 0..grid[1] {
            for column in 0..grid[0] {
                let (x0, x1) = (column * width / grid[0], (column + 1) * width / grid[0]);
                let (y0, y1) = (row * height / grid[1], (row + 1) * height / grid[1]);
                let mut sum = 0.0;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let at = ((y * width + x) * 4) as usize;
                        let [r, g, b] = [0, 1, 2].map(|k| linear(rgba[at + k]));
                        sum += 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    }
                }
                means.push(sum / f64::from((x1 - x0) * (y1 - y0)));
            }
        }
        means
    }

    #[test]
    #[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
    fn the_table_traces_at_64_samples_beside_the_live_shot() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let recipe = root.join("tests/golden/table/table.toml");
        let out = root.join("tmp/render-table.png");
        let live_out = root.join("tmp/render-table-live.png");
        let line = format!(
            "{} --camera main --spp 64 --seed 7 --out {}",
            recipe.display(),
            out.display()
        );
        let args = parse(&strings(&line)).unwrap();
        let mut steps = Vec::new();
        let (rendered, summary) = traced(&args, &Registry::new(), &mut |s| steps.push(s)).unwrap();
        println!("manfred render of the table: {summary}");
        assert_eq!(steps.first(), Some(&Step::Stage));
        assert_eq!(steps[1], Step::Trace { done: 0, total: 64 });
        assert_eq!(
            steps[steps.len() - 2],
            Step::Trace {
                done: 64,
                total: 64
            }
        );
        assert_eq!(steps.last(), Some(&Step::Write));
        assert!(rendered.paced.longest_ms < 300.0, "{:?}", rendered.paced);
        assert!(out.is_file());
        let traced =
            manfred_render::looked(&rendered.film, (&rendered.finish, &rendered.overlay)).unwrap();
        let live = manfred_scene::still_with(&recipe, "main", args.size(), None, &Registry::new())
            .unwrap();
        manfred_scene::write_png(&live_out, args.size(), &live).unwrap();
        let grid = [7, 5];
        let ours = regions(&traced, args.size(), grid);
        let theirs = regions(&live, args.size(), grid);
        let ratios: Vec<f64> = theirs
            .iter()
            .zip(&ours)
            .map(|(live, traced)| live / traced.max(1e-6))
            .collect();
        for row in ratios.chunks(grid[0] as usize) {
            println!(
                "live / traced: {}",
                row.iter()
                    .map(|r| format!("{r:.3}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        let mean = |values: &[f64]| values.iter().sum::<f64>() / values.len() as f64;
        let whole = mean(&theirs) / mean(&ours);
        let mut sorted = ratios.clone();
        sorted.sort_by(f64::total_cmp);
        println!(
            "the table's mean luminance, live / traced: {whole:.3} over the frame; per region min {:.3}, median {:.3}, max {:.3}",
            sorted[0],
            sorted[sorted.len() / 2],
            sorted[sorted.len() - 1]
        );
        assert!((0.5..2.0).contains(&whole), "{whole}");
    }
}
