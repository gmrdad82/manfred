use std::path::{Path, PathBuf};
use std::process::ExitCode;

use std::time::{Duration, Instant};

use clap::builder::{PossibleValuesParser, TypedValueParser};
use manfred_build::Registry;
use manfred_recipe::Trace;
use manfred_render::{
    Crop, DRAFT, EQUIRECT_SIZE, Fault, Format, NOISE, Noise, Rendered, Request, SAMPLES, SIZE,
    Step, View,
};

use super::{pair, refuse, size};

const PROGRESS_LINES: u32 = 20;
const JSON_EVERY: Duration = Duration::from_secs(1);
const PLAIN_EVERY: Duration = Duration::from_secs(30);
const DENOISE_APRON: u32 = 96;

fn threshold(text: &str) -> Result<f32, String> {
    match text.parse::<f32>() {
        Ok(value) if value.is_finite() && value >= 0.0 => Ok(value),
        _ => Err("is not a number of 0 or more".into()),
    }
}

fn rect(text: &str) -> Result<[u32; 4], String> {
    let parts: Vec<u32> = text
        .split(',')
        .map(|part| part.trim().parse::<u32>())
        .collect::<Result<_, _>>()
        .map_err(|_| "is not x,y,w,h in whole pixels".to_string())?;
    match parts[..] {
        [x, y, width, height] if width > 0 && height > 0 => Ok([x, y, width, height]),
        _ => Err("is not x,y,w,h with a width and height above 0".into()),
    }
}

fn master(text: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(text);
    match Format::of(&path) {
        Some(Format::Png) => Ok(path),
        _ => Err("does not end in .png".into()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum Progress {
    Plain,
    Json,
}

fn output(text: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(text);
    match Format::of(&path) {
        Some(_) => Ok(path),
        None => Err("ends in neither .png, .exr nor .hdr".into()),
    }
}

#[derive(clap::Args, Clone, Debug, PartialEq)]
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
        alias = "samples",
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..),
        help = "Samples per pixel, the most any pixel takes [default: the recipe's trace.spp, else 64]"
    )]
    samples: Option<u32>,
    #[arg(
        long,
        value_name = "T",
        value_parser = threshold,
        help = "A pixel stops once its noise is under this; 0 runs every pixel to --spp [default: the recipe's trace.threshold, else 0.01]"
    )]
    threshold: Option<f32>,
    #[arg(
        long = "min-samples",
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(2..),
        help = "Samples every pixel takes before it may stop [default: the recipe's trace.min_samples, else 16]"
    )]
    min_samples: Option<u32>,
    #[arg(
        long,
        conflicts_with_all = ["samples", "threshold", "min_samples"],
        help = "A quick look: 12 spp at most, threshold 0.05, 6 samples first"
    )]
    draft: bool,
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
        value_name = "x,y,w,h",
        value_parser = rect,
        help = "Render only this rectangle of the frame, the same pixels as the full frame's"
    )]
    crop: Option<[u32; 4]>,
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
        action = clap::ArgAction::Set,
        hide_possible_values = true,
        value_parser = PossibleValuesParser::new(["off", "on"]).map(|v| v == "on"),
        help = "Denoise the trace [default: on for a .png or master, off for .exr and .hdr]"
    )]
    denoise: Option<bool>,
    #[arg(
        long,
        value_name = "file",
        value_parser = output,
        help = "The picture to write: .png (8-bit, dithered), .exr or .hdr"
    )]
    out: PathBuf,
    #[arg(
        long,
        value_name = "file",
        value_parser = master,
        help = "Also write a 16-bit .png master through the recipe's finish"
    )]
    master: Option<PathBuf>,
    #[arg(
        long,
        value_name = "plain|json",
        value_enum,
        default_value = "plain",
        hide_possible_values = true,
        help = "Progress on stderr: plain lines, or JSON lines with the fraction and an ETA"
    )]
    progress: Progress,
    #[arg(skip)]
    trace: Trace,
}

impl Args {
    fn with(&self, trace: Trace) -> Args {
        Args {
            trace,
            ..self.clone()
        }
    }

    fn view(&self) -> View {
        match (self.equirect, self.ortho) {
            (true, _) => View::Equirect,
            (_, true) => View::Ortho,
            _ => View::Lens,
        }
    }

    fn framing(&self) -> String {
        let [width, height] = self.size();
        match self.crop {
            Some([x, y, w, h]) => format!("{w}x{h} at {x},{y} of {width}x{height}"),
            None => format!("{width}x{height}"),
        }
    }

    fn origin(&self) -> [u32; 2] {
        self.crop.map_or([0, 0], |[x, y, _, _]| [x, y])
    }

    fn finished(&self) -> bool {
        Format::of(&self.out) == Some(Format::Png) || self.master.is_some()
    }

    fn png(&self) -> bool {
        Format::of(&self.out) == Some(Format::Png)
    }

    fn denoised(&self) -> bool {
        self.denoise != Some(false)
    }

    fn denoised_linear(&self) -> bool {
        self.denoise == Some(true)
    }

    fn denoising(&self) -> bool {
        (self.finished() && self.denoised()) || (!self.png() && self.denoised_linear())
    }

    fn samples(&self) -> u32 {
        if self.draft {
            DRAFT.0
        } else {
            self.samples.or(self.trace.spp).unwrap_or(SAMPLES)
        }
    }

    fn noise(&self) -> Option<Noise> {
        if self.draft {
            return Some(DRAFT.1);
        }
        let samples = self.samples();
        let threshold = self
            .threshold
            .or(self.trace.threshold.map(|t| t as f32))
            .unwrap_or(NOISE.threshold);
        let min_samples = self
            .min_samples
            .or(self.trace.min_samples)
            .unwrap_or(NOISE.min_samples);
        (threshold > 0.0 && samples >= 2).then_some(Noise {
            threshold,
            min_samples: min_samples.min(samples),
        })
    }

    fn size(&self) -> [u32; 2] {
        self.frame.unwrap_or(if self.view() == View::Equirect {
            EQUIRECT_SIZE
        } else {
            SIZE
        })
    }
}

fn precheck(args: &Args) -> Result<Trace, String> {
    super::precheck(
        &args.recipe,
        args.variant.as_deref(),
        &args.set,
        &args.camera,
    )
    .map(|(_, trace)| trace)
}

fn requeue(args: &[String]) -> Option<ExitCode> {
    super::process::requeue("render", args, "runs directly")
}

fn progress_line(done: u32, total: u32) -> Option<String> {
    let every = (total / PROGRESS_LINES).max(1);
    (done == 0 || done == total || done.is_multiple_of(every))
        .then(|| format!("progress {done}/{total}"))
}

struct Watch {
    progress: Progress,
    threshold: f32,
    began: Instant,
    last: Option<Duration>,
}

impl Watch {
    fn new(progress: Progress, threshold: f32) -> Watch {
        Watch {
            progress,
            threshold,
            began: Instant::now(),
            last: None,
        }
    }

    fn due(&mut self, at: Duration, every: Duration, edge: bool) -> bool {
        let due = edge || self.last.is_none_or(|last| at >= last + every);
        if due {
            self.last = Some(at);
        }
        due
    }

    fn line(&mut self, step: Step, at: Duration) -> Option<String> {
        let elapsed = at.as_secs_f64();
        match (self.progress, step) {
            (Progress::Plain, Step::Trace { done, total }) => progress_line(done, total),
            (
                Progress::Plain,
                Step::Converge {
                    gpu_ms,
                    cap,
                    done,
                    total,
                },
            ) => {
                let threshold = self.threshold;
                self.due(at, PLAIN_EVERY, gpu_ms == 0)
                    .then(|| converge_line(gpu_ms, cap, threshold, fraction(done, total)))
            }
            (Progress::Plain, _) => None,
            (Progress::Json, Step::Stage) => Some(json_line("stage", elapsed, [])),
            (Progress::Json, Step::Write) => Some(json_line("write", elapsed, [])),
            (Progress::Json, Step::Trace { done, total }) => self
                .due(at, JSON_EVERY, done == 0 || done == total)
                .then(|| {
                    let fraction = f64::from(done) / f64::from(total.max(1));
                    let eta =
                        (done > 0).then(|| elapsed / f64::from(done) * f64::from(total - done));
                    json_line(
                        "trace",
                        elapsed,
                        [
                            ("done", done.into()),
                            ("total", total.into()),
                            ("fraction", rounded(fraction, 4)),
                            (
                                "eta_s",
                                eta.map_or(serde_json::Value::Null, |e| rounded(e, 1)),
                            ),
                        ],
                    )
                }),
            (
                Progress::Json,
                Step::Converge {
                    gpu_ms,
                    cap,
                    done,
                    total,
                },
            ) => {
                let threshold = self.threshold;
                self.due(at, JSON_EVERY, gpu_ms == 0).then(|| {
                    let eta = (done > 0)
                        .then(|| elapsed / done as f64 * total.saturating_sub(done) as f64);
                    json_line(
                        "trace",
                        elapsed,
                        [
                            ("cap", cap.into()),
                            ("threshold", rounded(f64::from(threshold), 6)),
                            ("gpu_s", rounded(gpu_ms as f64 / 1000.0, 1)),
                            ("fraction", rounded(fraction(done, total), 4)),
                            (
                                "eta_s",
                                eta.map_or(serde_json::Value::Null, |e| rounded(e, 1)),
                            ),
                        ],
                    )
                })
            }
        }
    }

    fn done(&self, out: &Path, rendered: &Rendered, at: Duration) -> Option<String> {
        (self.progress == Progress::Json).then(|| {
            json_line(
                "done",
                at.as_secs_f64(),
                [
                    ("out", out.display().to_string().into()),
                    ("mean_samples", rounded(rendered.mean_samples, 1)),
                    ("most_samples", rendered.most_samples.into()),
                    ("gpu_s", rounded(rendered.paced.gpu_ms / 1000.0, 1)),
                ],
            )
        })
    }
}

fn rounded(value: f64, places: i32) -> serde_json::Value {
    let scale = 10f64.powi(places);
    serde_json::json!((value * scale).round() / scale)
}

fn json_line<const N: usize>(
    stage: &str,
    elapsed: f64,
    fields: [(&str, serde_json::Value); N],
) -> String {
    let mut line = serde_json::Map::new();
    line.insert("stage".into(), stage.into());
    line.insert("elapsed_s".into(), rounded(elapsed, 1));
    for (key, value) in fields {
        line.insert(key.into(), value);
    }
    serde_json::Value::Object(line).to_string()
}

fn fraction(done: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (done as f64 / total as f64).min(1.0)
}

fn converge_line(gpu_ms: u64, cap: u32, threshold: f32, fraction: f64) -> String {
    format!(
        "progress {:.0}%, {:.0} s on the GPU, each pixel to a noise of {threshold} or {cap} samples",
        fraction * 100.0,
        gpu_ms as f64 / 1000.0
    )
}

fn note(watch: &mut Watch, step: Step) {
    if let Some(line) = watch.line(step, watch.began.elapsed()) {
        eprintln!("{line}");
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
        samples: args.samples(),
        noise: args.noise(),
        seed: args.seed,
        view: args.view(),
        variant: args.variant.as_deref(),
        set: &args.set,
        crop: args.crop.map(|rect| Crop {
            rect,
            apron: if args.denoising() { DENOISE_APRON } else { 0 },
            finished: args.finished(),
        }),
        denoise: args.denoising(),
    };
    let rendered = manfred_render::render(&request, registry, progress)?;
    progress(Step::Write);
    written(args, &rendered)?;
    let paced = rendered.paced;
    let samples = match args.noise() {
        Some(noise) => format!(
            "{:.1} samples a pixel on average, {} at most of {}, threshold {}",
            rendered.mean_samples,
            rendered.most_samples,
            args.samples(),
            noise.threshold
        ),
        None => format!("{} samples", args.samples()),
    };
    let summary = format!(
        "{}, {samples}, seed {}: {} submissions, the longest {:.2} ms, {:.0} ms on the GPU",
        args.framing(),
        rendered.seed,
        paced.submissions,
        paced.longest_ms,
        paced.gpu_ms
    );
    Ok((rendered, summary))
}

fn written(args: &Args, rendered: &Rendered) -> Result<(), Fault> {
    let size = rendered.size();
    if args.finished() {
        let pixels = rendered.finished(args.denoised())?;
        if let Some(master) = &args.master {
            manfred_render::write_master(master, size, &pixels)?;
        }
        if args.png() {
            manfred_render::write_delivery(&args.out, size, args.origin(), &pixels)?;
        }
    }
    if !args.png() {
        let film = rendered.crop();
        let pixels = manfred_render::developed(&film, args.denoised_linear())?;
        manfred_render::write_linear(&args.out, size, &pixels)?;
    }
    Ok(())
}

pub fn run(parsed: &Args, args: &[String], registry: &Registry) -> ExitCode {
    let parsed = &match precheck(parsed) {
        Ok(trace) => parsed.with(trace),
        Err(message) => return refuse("render", &message),
    };
    if let Some(code) = requeue(args) {
        return code;
    }
    let threshold = parsed.noise().map_or(0.0, |noise| noise.threshold);
    let mut watch = Watch::new(parsed.progress, threshold);
    match traced(parsed, registry, &mut |step| note(&mut watch, step)) {
        Ok((rendered, summary)) => {
            if let Some(line) = watch.done(&parsed.out, &rendered, watch.began.elapsed()) {
                eprintln!("{line}");
            }
            eprintln!("manfred render: {summary}");
            println!("{}", parsed.out.display());
            if let Some(master) = &parsed.master {
                println!("{}", master.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => refuse("render", &error.to_string()),
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
            "table.toml --camera main --spp 64 --threshold 0.02 --min-samples 16 --seed 9 --size 320x200 --ortho --crop 10,20,30,40 --variant bare --set sky.intensity=0.4 --set object.a.params.b=/x/y --denoise off --out tmp/a.exr --master tmp/a.master.png --progress json",
        ))
        .unwrap();
        assert_eq!(
            args,
            Args {
                recipe: "table.toml".into(),
                camera: "main".into(),
                samples: Some(64),
                threshold: Some(0.02),
                min_samples: Some(16),
                draft: false,
                seed: Some(9),
                frame: Some([320, 200]),
                equirect: false,
                ortho: true,
                crop: Some([10, 20, 30, 40]),
                variant: Some("bare".into()),
                set: vec!["sky.intensity=0.4".into(), "object.a.params.b=/x/y".into()],
                denoise: Some(false),
                out: "tmp/a.exr".into(),
                master: Some("tmp/a.master.png".into()),
                progress: Progress::Json,
                trace: Trace::default(),
            }
        );
    }

    #[test]
    fn the_defaults_are_adaptive_to_64_samples_and_the_frame_or_a_two_to_one_sphere() {
        let args = parse(&strings("a.toml --out a.png --camera main")).unwrap();
        assert_eq!(args.samples(), 64);
        assert_eq!(
            args.noise(),
            Some(Noise {
                threshold: 0.01,
                min_samples: 16
            })
        );
        let draft = parse(&strings("a.toml --out a.png --camera main --draft")).unwrap();
        assert_eq!(draft.samples(), 12);
        assert_eq!(
            draft.noise(),
            Some(Noise {
                threshold: 0.05,
                min_samples: 6
            })
        );
        assert!(
            parse(&strings(
                "a.toml --out a.png --camera main --draft --spp 64"
            ))
            .is_err()
        );
        assert_eq!(args.progress, Progress::Plain);
        assert_eq!(args.master, None);
        assert_eq!(args.seed, None);
        assert!(args.denoise.is_none() && args.denoising());
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
            "a.toml --camera main --out a.png --threshold -0.1",
            "a.toml --camera main --out a.png --threshold nan",
            "a.toml --camera main --out a.png --min-samples 1",
            "a.toml --camera main --out a.png --master a.exr",
            "a.toml --camera main --out a.png --progress yaml",
            "a.toml --camera main --out a.png --crop 1,2,3",
            "a.toml --camera main --out a.png --crop 1,2,0,4",
            "a.toml --camera main --out a.png --crop a,b,c,d",
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
    fn the_recipes_sampling_stands_unless_a_flag_overrides_it() {
        let trace = Trace {
            spp: Some(256),
            threshold: Some(0.05),
            min_samples: Some(8),
            ..Trace::default()
        };
        let recipe = parse(&strings("a.toml --camera c --out a.png"))
            .unwrap()
            .with(trace);
        assert_eq!(recipe.samples(), 256);
        assert_eq!(
            recipe.noise(),
            Some(Noise {
                threshold: 0.05,
                min_samples: 8
            })
        );
        let flags = parse(&strings(
            "a.toml --camera c --out a.png --spp 64 --threshold 0.02 --min-samples 16",
        ))
        .unwrap()
        .with(trace);
        assert_eq!(flags.samples(), 64);
        assert_eq!(
            flags.noise(),
            Some(Noise {
                threshold: 0.02,
                min_samples: 16
            })
        );
        let off = parse(&strings("a.toml --camera c --out a.png --threshold 0"))
            .unwrap()
            .with(trace);
        assert_eq!((off.samples(), off.noise()), (256, None));
        let draft = parse(&strings("a.toml --camera c --out a.png --draft"))
            .unwrap()
            .with(trace);
        assert_eq!((draft.samples(), draft.noise()), (DRAFT.0, Some(DRAFT.1)));
    }

    #[test]
    fn a_zero_threshold_or_a_single_sample_traces_every_pixel_to_the_cap() {
        let off = parse(&strings(
            "a.toml --camera c --out a.png --threshold 0 --spp 1024",
        ))
        .unwrap();
        assert_eq!(off.noise(), None);
        let one = parse(&strings("a.toml --camera c --out a.png --spp 1")).unwrap();
        assert_eq!(one.noise(), None);
        let few = parse(&strings("a.toml --camera c --out a.png --samples 8")).unwrap();
        assert_eq!(few.noise().unwrap().min_samples, 8);
    }

    fn fields(line: &str) -> serde_json::Value {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn json_progress_gives_the_fraction_and_eta_once_a_second() {
        let mut watch = Watch::new(Progress::Json, 0.01);
        let at = Duration::from_secs_f64;
        let first = fields(
            &watch
                .line(
                    Step::Trace {
                        done: 0,
                        total: 100,
                    },
                    at(2.0),
                )
                .unwrap(),
        );
        assert_eq!(first["stage"], "trace");
        assert_eq!(first["fraction"], 0.0);
        assert!(first["eta_s"].is_null());
        assert_eq!(
            watch.line(
                Step::Trace {
                    done: 10,
                    total: 100
                },
                at(2.5)
            ),
            None
        );
        let line = fields(
            &watch
                .line(
                    Step::Trace {
                        done: 25,
                        total: 100,
                    },
                    at(12.0),
                )
                .unwrap(),
        );
        assert_eq!(line["done"], 25);
        assert_eq!(line["total"], 100);
        assert_eq!(line["fraction"], 0.25);
        assert_eq!(line["eta_s"], 36.0);
        assert_eq!(line["elapsed_s"], 12.0);
        assert!(
            watch
                .line(
                    Step::Trace {
                        done: 100,
                        total: 100
                    },
                    at(12.1)
                )
                .is_some()
        );
        let mut adaptive = Watch::new(Progress::Json, 0.01);
        let start = fields(
            &adaptive
                .line(
                    Step::Converge {
                        gpu_ms: 0,
                        cap: 1024,
                        done: 0,
                        total: 1000,
                    },
                    at(1.0),
                )
                .unwrap(),
        );
        assert_eq!(start["cap"], 1024);
        assert_eq!(start["threshold"], 0.01);
        assert_eq!(start["fraction"], 0.0);
        assert!(start["eta_s"].is_null());
        let later = fields(
            &adaptive
                .line(
                    Step::Converge {
                        gpu_ms: 61_234,
                        cap: 1024,
                        done: 250,
                        total: 1000,
                    },
                    at(90.0),
                )
                .unwrap(),
        );
        assert_eq!(later["gpu_s"], 61.2);
        assert_eq!(later["fraction"], 0.25);
        assert_eq!(later["eta_s"], 270.0);
        assert_eq!(
            fields(&adaptive.line(Step::Write, at(91.0)).unwrap())["stage"],
            "write"
        );
    }

    #[test]
    fn plain_progress_keeps_its_sample_lines_and_tells_adaptive_time_twice_a_minute() {
        let mut watch = Watch::new(Progress::Plain, 0.01);
        let at = Duration::from_secs;
        assert_eq!(
            watch
                .line(
                    Step::Trace {
                        done: 12,
                        total: 256
                    },
                    at(1)
                )
                .as_deref(),
            Some("progress 12/256")
        );
        assert_eq!(watch.line(Step::Stage, at(1)), None);
        assert!(
            watch
                .line(
                    Step::Converge {
                        gpu_ms: 0,
                        cap: 1024,
                        done: 0,
                        total: 1000,
                    },
                    at(1)
                )
                .is_some()
        );
        assert_eq!(
            watch.line(
                Step::Converge {
                    gpu_ms: 9_000,
                    cap: 1024,
                    done: 100,
                    total: 1000,
                },
                at(10)
            ),
            None
        );
        assert_eq!(
            watch
                .line(
                    Step::Converge {
                        gpu_ms: 30_000,
                        cap: 1024,
                        done: 300,
                        total: 1000,
                    },
                    at(31)
                )
                .as_deref(),
            Some("progress 30%, 30 s on the GPU, each pixel to a noise of 0.01 or 1024 samples")
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
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_png_crop_through_bloom_and_vignette_is_the_full_frames_finish_there() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let recipe = root.join("tests/golden/toybox/toybox.toml");
        let rect = [101, 47, 60, 40];
        for (denoise, out, worst) in [
            ("off", "tmp/render-crop.png", 1e-4),
            ("on", "tmp/render-crop.png", 2.0 / 255.0),
            ("on", "tmp/render-crop.exr", 2.0 / 255.0),
        ] {
            let line = |extra: &str| {
                format!(
                    "{} --camera main --size 320x200 --seed 3 --spp 32 --threshold 0.05 --min-samples 8 --set look.grade.bloom=0.6 --set look.grade.vignette=0.4 --denoise {denoise} --out {} {extra}",
                    recipe.display(),
                    root.join(out).display()
                )
            };
            let full = parse(&strings(&line(""))).unwrap();
            let (whole, _) = traced(&full, &Registry::new(), &mut |_| {}).unwrap();
            let crop = parse(&strings(&line(&format!(
                "--crop {},{},{},{}",
                rect[0], rect[1], rect[2], rect[3]
            ))))
            .unwrap();
            let (part, _) = traced(&crop, &Registry::new(), &mut |_| {}).unwrap();
            let (got, want) = if out.ends_with(".png") {
                let on = denoise == "on";
                (part.finished(on).unwrap(), whole.finished(on).unwrap())
            } else {
                (
                    part.crop().denoised.unwrap(),
                    whole.film.denoised.clone().unwrap(),
                )
            };
            let want = manfred_render::cut(&want, 320, [rect[0], rect[1]], [rect[2], rect[3]]);
            assert_eq!(got.len(), want.len());
            let differences: Vec<f32> = got
                .iter()
                .zip(&want)
                .flat_map(|(a, b)| (0..3).map(move |k| (a[k] - b[k]).abs()))
                .collect();
            let most = differences.iter().copied().fold(0.0, f32::max);
            let mean = differences.iter().sum::<f32>() / differences.len() as f32;
            println!("--denoise {denoise} {out}: mean {mean:.6}, most {most:.6}");
            assert!(most <= worst, "--denoise {denoise} {out}: {most} > {worst}");
        }
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn the_table_traces_at_64_samples_beside_the_live_shot() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let recipe = root.join("tests/golden/table/table.toml");
        let out = root.join("tmp/render-table.png");
        let live_out = root.join("tmp/render-table-live.png");
        let line = format!(
            "{} --camera main --spp 64 --threshold 0 --seed 7 --out {}",
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

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn the_table_stops_most_pixels_early_and_writes_a_master() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let recipe = root.join("tests/golden/table/table.toml");
        let out = root.join("tmp/render-table-adaptive.png");
        let master = root.join("tmp/render-table-adaptive.master.png");
        let line = format!(
            "{} --camera main --spp 256 --threshold 0.05 --min-samples 16 --seed 7 --size 400x250 --denoise off --out {} --master {}",
            recipe.display(),
            out.display(),
            master.display()
        );
        let args = parse(&strings(&line)).unwrap();
        let mut steps = Vec::new();
        let (rendered, summary) = traced(&args, &Registry::new(), &mut |s| steps.push(s)).unwrap();
        println!("manfred render of the table, adaptive: {summary}");
        assert_eq!(
            steps[1],
            Step::Converge {
                gpu_ms: 0,
                cap: 256,
                done: 0,
                total: 25_600_000,
            }
        );
        assert!(rendered.mean_samples >= 16.0, "{}", rendered.mean_samples);
        assert!(rendered.mean_samples < 200.0, "{}", rendered.mean_samples);
        assert!(rendered.most_samples <= 256);
        assert_eq!(rendered.film.samples, 256);
        assert!(rendered.paced.longest_ms < 300.0, "{:?}", rendered.paced);
        let (size, rgba) = manfred_scene::read_png(&out).unwrap();
        assert_eq!(size, [400, 250]);
        let reader = png::Decoder::new(std::io::BufReader::new(
            std::fs::File::open(&master).unwrap(),
        ))
        .read_info()
        .unwrap();
        assert_eq!(reader.info().bit_depth, png::BitDepth::Sixteen);
        assert_eq!(reader.info().size(), (400, 250));
        let fixed =
            manfred_render::finished(&rendered.film, (&rendered.finish, &rendered.overlay), false)
                .unwrap();
        let near = manfred_render::dithered(size, &fixed);
        assert_eq!(rgba, near);
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_crop_traces_the_same_pixels_as_the_full_frame() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let recipe = root.join("tests/golden/table/table.toml");
        for sampling in [
            "--spp 32 --threshold 0",
            "--spp 64 --threshold 0.05 --min-samples 8",
        ] {
            let line = |extra: &str| {
                format!(
                    "{} --camera main --size 320x200 --seed 3 {sampling} --denoise off --out {} {extra}",
                    recipe.display(),
                    root.join("tmp/render-crop.exr").display()
                )
            };
            let full = parse(&strings(&line(""))).unwrap();
            let (whole, _) = traced(&full, &Registry::new(), &mut |_| {}).unwrap();
            let crop = parse(&strings(&line("--crop 100,50,60,40"))).unwrap();
            let (part, summary) = traced(&crop, &Registry::new(), &mut |_| {}).unwrap();
            println!("{sampling}: {summary}");
            assert_eq!(part.film.size, [60, 40]);
            assert!(
                part.film == whole.film.cut([100, 50], [60, 40]),
                "{sampling}: the crop differs from the full frame"
            );
        }
    }
}
