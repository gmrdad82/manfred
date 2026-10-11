use crate::picture::Film;
use crate::staging::{Staging, View, eyepiece};
use crate::text::Overlay;
use manfred_build::Registry;
use manfred_recipe::{Camera, Recipe};
use pfx::adapter::{Report, TurnState, turn_reporting};
use pfx_gpu::Gpu;
use pfx_gpu::pace::{Pacer, Stats};
use pfx_post::Chain;
use pfx_trace::sky::Environment;
use pfx_trace::stage::Staged;
use pfx_trace::{Adaptive, Trace, TraceError};
use std::fmt;
use std::path::Path;

pub const SAMPLES: u32 = 128;
pub const SIZE: [u32; 2] = [1586, 992];
pub const EQUIRECT_SIZE: [u32; 2] = [2048, 1024];
pub const NOISE: Noise = Noise {
    threshold: 0.01,
    min_samples: 16,
};
pub const DRAFT: (u32, Noise) = (
    16,
    Noise {
        threshold: 0.05,
        min_samples: 8,
    },
);
const GROWTH: f32 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
    pub threshold: f32,
    pub min_samples: u32,
}

impl Noise {
    fn adaptive(self, cap: u32) -> Option<Adaptive> {
        let adaptive = Adaptive {
            threshold: self.threshold,
            min_samples: self.min_samples.clamp(2, cap.max(2)),
            max_samples: cap,
            growth: GROWTH,
        };
        adaptive.valid().then_some(adaptive)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Stage,
    Trace { done: u32, total: u32 },
    Converge { gpu_ms: u64, cap: u32 },
    Write,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Fault {
    Refused(String),
    Failed(String),
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::Refused(message) | Fault::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Fault {}

impl From<manfred_scene::Error> for Fault {
    fn from(error: manfred_scene::Error) -> Fault {
        Fault::Failed(error.0)
    }
}

impl From<String> for Fault {
    fn from(message: String) -> Fault {
        Fault::Failed(message)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    pub recipe: &'a Path,
    pub camera: &'a str,
    pub size: [u32; 2],
    pub samples: u32,
    pub noise: Option<Noise>,
    pub seed: Option<u32>,
    pub view: View,
    pub variant: Option<&'a str>,
    pub set: &'a [String],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Paced {
    pub submissions: usize,
    pub longest_ms: f64,
    pub gpu_ms: f64,
    pub wall_ms: f64,
}

impl Paced {
    fn add(&mut self, stats: &Stats) {
        self.submissions += stats.count();
        self.longest_ms = self.longest_ms.max(stats.longest_ms());
        self.gpu_ms += stats.milliseconds.iter().sum::<f64>();
        self.wall_ms += stats.wall_ms;
    }
}

struct Turns<'a> {
    state: TurnState,
    turn: &'a mut dyn FnMut(Report),
}

impl<'a> Turns<'a> {
    fn new(turn: &'a mut dyn FnMut(Report)) -> Turns<'a> {
        Turns {
            state: TurnState::default(),
            turn,
        }
    }

    fn soon(&mut self, slice_ms: f64) -> bool {
        self.state.add(slice_ms);
        if !self.state.due() {
            return false;
        }
        self.now();
        true
    }

    fn now(&mut self) {
        (self.turn)(self.state.take());
    }
}

pub struct Rendered {
    pub film: Film,
    pub finish: Chain,
    pub overlay: Overlay,
    pub paced: Paced,
    pub seed: u32,
    pub mean_samples: f64,
}

pub fn render(
    request: &Request<'_>,
    registry: &Registry,
    progress: &mut dyn FnMut(Step),
) -> Result<Rendered, Fault> {
    if request.samples == 0 {
        return Err(Fault::Failed("a render needs at least one sample".into()));
    }
    progress(Step::Stage);
    let mut turn = turn_reporting;
    let (recipe, built) =
        manfred_scene::load(request.recipe, request.variant, request.set, registry)?;
    turn(Report::default());
    let staging = Staging::of(&recipe, &built.parts)?;
    let camera = manfred_scene::camera(&recipe, request.camera)?;
    let staged = staging.stage(eyepiece(camera, request.view, request.size)?)?;
    turn(Report::default());
    let gpu = manfred_scene::headless()?;
    let seed = request.seed.unwrap_or_else(|| manfred_scene::seed(&recipe));
    let adaptive = request
        .noise
        .and_then(|noise| noise.adaptive(request.samples));
    let ((film, paced), mean_samples) = match adaptive {
        Some(adaptive) => {
            let (film, paced, mean) = converge(
                &gpu,
                &staged,
                request.size,
                adaptive,
                seed,
                progress,
                &mut turn,
            )?;
            ((film, paced), mean)
        }
        None => (
            trace(
                &gpu,
                &staged,
                request.size,
                request.samples,
                seed,
                progress,
                &mut turn,
            )?,
            f64::from(request.samples),
        ),
    };
    Ok(Rendered {
        film,
        overlay: Overlay::of(&staging.text),
        finish: staging.finish,
        paced,
        seed,
        mean_samples,
    })
}

pub struct Reel {
    pub recipe: Recipe,
    pub seed: u32,
    staging: Staging,
    gpu: Gpu,
}

impl Reel {
    pub fn open(
        recipe: &Path,
        variant: Option<&str>,
        set: &[String],
        seed: Option<u32>,
        registry: &Registry,
    ) -> Result<Reel, Fault> {
        let (recipe, built) = manfred_scene::load(recipe, variant, set, registry)?;
        turn_reporting(Report::default());
        let staging = Staging::of(&recipe, &built.parts)?;
        let gpu = manfred_scene::headless()?;
        let seed = seed.unwrap_or_else(|| manfred_scene::seed(&recipe));
        Ok(Reel {
            recipe,
            seed,
            staging,
            gpu,
        })
    }

    pub fn frame(
        &self,
        camera: &Camera,
        size: [u32; 2],
        samples: u32,
        progress: &mut dyn FnMut(Step),
    ) -> Result<Rendered, Fault> {
        if samples == 0 {
            return Err(Fault::Failed("a render needs at least one sample".into()));
        }
        progress(Step::Stage);
        let mut turn = turn_reporting;
        let staged = self.staging.stage(eyepiece(camera, View::Lens, size)?)?;
        turn(Report::default());
        let (film, paced) = trace(
            &self.gpu, &staged, size, samples, self.seed, progress, &mut turn,
        )?;
        Ok(Rendered {
            film,
            overlay: Overlay::of(&self.staging.text),
            finish: self.staging.finish.clone(),
            paced,
            seed: self.seed,
            mean_samples: f64::from(samples),
        })
    }
}

pub fn trace(
    gpu: &Gpu,
    staged: &Staged<Environment>,
    size: [u32; 2],
    samples: u32,
    seed: u32,
    progress: &mut dyn FnMut(Step),
    turn: &mut dyn FnMut(Report),
) -> Result<(Film, Paced), Fault> {
    let mut trace = staged.trace(gpu, size[0], size[1]).map_err(refused)?;
    let mut turns = Turns::new(turn);
    let paced = sampled(samples, &mut turns, progress, |pacer, between| {
        trace.sample_paced(gpu, 1, seed, pacer, between)
    })?;
    let film = developed(gpu, &trace, size, samples)?;
    turns.now();
    Ok((film, paced))
}

pub fn converge(
    gpu: &Gpu,
    staged: &Staged<Environment>,
    size: [u32; 2],
    adaptive: Adaptive,
    seed: u32,
    progress: &mut dyn FnMut(Step),
    turn: &mut dyn FnMut(Report),
) -> Result<(Film, Paced, f64), Fault> {
    let mut trace = staged.trace(gpu, size[0], size[1]).map_err(refused)?;
    let mut turns = Turns::new(turn);
    let mut pacer = Pacer::default();
    let cap = adaptive.max_samples;
    turns.now();
    progress(Step::Converge { gpu_ms: 0, cap });
    let mut gpu_ms = 0.0;
    let converged = trace
        .sample_adaptive(gpu, adaptive, seed, &mut pacer, |ms| {
            gpu_ms += ms;
            progress(Step::Converge {
                gpu_ms: gpu_ms as u64,
                cap,
            });
            turns.soon(ms)
        })
        .map_err(|e| Fault::Failed(format!("the tracer: {e}")))?;
    let mut paced = Paced::default();
    paced.add(&converged.stats);
    let film = developed(gpu, &trace, size, converged.max_samples())?;
    turns.now();
    Ok((film, paced, converged.mean_samples()))
}

fn developed(gpu: &Gpu, trace: &Trace, size: [u32; 2], samples: u32) -> Result<Film, Fault> {
    let output = trace
        .readback(gpu)
        .map_err(|e| Fault::Failed(format!("the tracer's readback: {e}")))?;
    Ok(Film {
        size,
        color: floats(&output.color),
        albedo: floats(&output.albedo),
        normal: floats(&output.normal),
        samples,
    })
}

fn sampled(
    samples: u32,
    turns: &mut Turns<'_>,
    progress: &mut dyn FnMut(Step),
    mut sample: impl FnMut(&mut Pacer, &mut dyn FnMut(f64) -> bool) -> Result<Stats, String>,
) -> Result<Paced, Fault> {
    let mut pacer = Pacer::default();
    let mut paced = Paced::default();
    turns.now();
    progress(Step::Trace {
        done: 0,
        total: samples,
    });
    for done in 1..=samples {
        let stats = sample(&mut pacer, &mut |ms| turns.soon(ms))
            .map_err(|e| Fault::Failed(format!("the tracer: {e}")))?;
        paced.add(&stats);
        progress(Step::Trace {
            done,
            total: samples,
        });
    }
    turns.now();
    Ok(paced)
}

fn refused(error: TraceError) -> Fault {
    match error {
        TraceError::Refused(refusal) => Fault::Refused(format!(
            "this GPU is below the tracer's floor, so manfred render cannot trace on it: {refusal}"
        )),
        TraceError::Setup(message) => {
            Fault::Failed(format!("the tracer could not start: {message}"))
        }
    }
}

fn floats(bytes: &[u8]) -> Vec<[f32; 4]> {
    bytes
        .chunks_exact(16)
        .map(|pixel| {
            std::array::from_fn(|k| {
                f32::from_le_bytes([
                    pixel[k * 4],
                    pixel[k * 4 + 1],
                    pixel[k * 4 + 2],
                    pixel[k * 4 + 3],
                ])
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pfx_gpu::pace::Timing;
    use std::cell::RefCell;
    use std::time::{Duration, Instant};

    #[derive(Clone, Debug, PartialEq)]
    enum Event {
        Turn(f64),
        Sample,
        Done(u32),
    }

    fn slices(milliseconds: &[f64]) -> Stats {
        Stats {
            milliseconds: milliseconds.to_vec(),
            timings: vec![Timing::Gpu; milliseconds.len()],
            wall_ms: milliseconds.iter().sum(),
        }
    }

    fn run(samples: u32, slice_ms: &[f64]) -> (Vec<Event>, Paced) {
        let events = RefCell::new(Vec::new());
        let mut turn = |report: Report| events.borrow_mut().push(Event::Turn(report.work_ms));
        let mut turns = Turns {
            state: TurnState::new(50.0).with_wall_ms(1e9),
            turn: &mut turn,
        };
        let paced = sampled(
            samples,
            &mut turns,
            &mut |step| {
                if let Step::Trace { done, .. } = step {
                    events.borrow_mut().push(Event::Done(done));
                }
            },
            |_, between| {
                events.borrow_mut().push(Event::Sample);
                for &ms in slice_ms {
                    between(ms);
                }
                Ok(slices(slice_ms))
            },
        )
        .unwrap();
        (events.into_inner(), paced)
    }

    #[test]
    fn the_loop_turns_after_staging_and_after_the_last_sample() {
        let (events, paced) = run(3, &[1.0, 2.0, 1.0]);
        assert_eq!(
            events,
            [
                Event::Turn(0.0),
                Event::Done(0),
                Event::Sample,
                Event::Done(1),
                Event::Sample,
                Event::Done(2),
                Event::Sample,
                Event::Done(3),
                Event::Turn(12.0),
            ]
        );
        assert_eq!(paced.submissions, 9);
        assert_eq!(paced.longest_ms, 2.0);
        assert_eq!(paced.gpu_ms, 12.0);
    }

    #[test]
    fn the_loop_turns_every_fifty_ms_of_slices_and_reports_the_rest_at_the_end() {
        let (events, _) = run(2, &[20.0, 20.0, 20.0]);
        let turns: Vec<f64> = events
            .iter()
            .filter_map(|event| match event {
                Event::Turn(ms) => Some(*ms),
                _ => None,
            })
            .collect();
        assert_eq!(turns, [0.0, 60.0, 60.0, 0.0]);
        assert_eq!(events.first(), Some(&Event::Turn(0.0)));
        assert_eq!(events.last(), Some(&Event::Turn(0.0)));
    }

    #[test]
    fn a_failed_sample_stops_the_loop_with_the_tracers_words() {
        let mut turned = 0;
        let mut turn = |_: Report| turned += 1;
        let mut turns = Turns {
            state: TurnState::new(50.0).with_wall_ms(1e9),
            turn: &mut turn,
        };
        let fault = sampled(4, &mut turns, &mut |_| {}, |_, _| Err("lost".into()))
            .err()
            .unwrap();
        assert_eq!(fault, Fault::Failed("the tracer: lost".into()));
        assert_eq!(turned, 1);
    }

    #[test]
    fn the_engines_wall_rule_turns_after_little_work_once_four_hundred_ms_have_passed() {
        let reports = RefCell::new(Vec::new());
        let mut turn = |report: Report| reports.borrow_mut().push(report);
        let mut state = TurnState::new(50.0);
        state.reset_at(Instant::now() - Duration::from_secs(1));
        let mut late = Turns {
            state,
            turn: &mut turn,
        };
        assert!(late.soon(1.5));
        let mut idle = Turns {
            state: TurnState::new(50.0).with_wall_ms(1e9),
            turn: &mut turn,
        };
        assert!(!idle.soon(1.5));
        assert!(!idle.soon(48.0));
        assert!(idle.soon(0.5));
        let reports = reports.into_inner();
        assert_eq!(
            reports,
            [
                Report {
                    work_ms: 1.5,
                    longest_ms: 1.5
                },
                Report {
                    work_ms: 50.0,
                    longest_ms: 48.0
                }
            ]
        );
    }

    #[test]
    fn readback_bytes_become_pixels() {
        let bytes: Vec<u8> = [1.0f32, 0.5, -2.0, 1.0, 0.0, 3.25, 7.0, 1.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        assert_eq!(
            floats(&bytes),
            [[1.0, 0.5, -2.0, 1.0], [0.0, 3.25, 7.0, 1.0]]
        );
    }

    #[test]
    fn a_refusal_is_told_in_plain_words() {
        let refusal = pfx_gpu::floor::GpuRefusal {
            adapter: None,
            shortfall: pfx_gpu::floor::GpuShortfall::NoAdapter,
        };
        let Fault::Refused(message) = refused(TraceError::Refused(refusal)) else {
            panic!("a refusal stays a refusal");
        };
        assert!(message.contains("below the tracer's floor"), "{message}");
        assert_eq!(
            refused(TraceError::Setup("bad".into())),
            Fault::Failed("the tracer could not start: bad".into())
        );
    }

    #[test]
    fn a_render_without_samples_or_a_recipe_fails_before_the_gpu() {
        let mut steps = Vec::new();
        let request = Request {
            recipe: Path::new("missing.toml"),
            camera: "main",
            size: [8, 8],
            samples: 0,
            noise: None,
            seed: None,
            view: View::Lens,
            variant: None,
            set: &[],
        };
        assert!(render(&request, &Registry::new(), &mut |s| steps.push(s)).is_err());
        assert!(steps.is_empty());
        let request = Request {
            samples: 4,
            ..request
        };
        assert!(render(&request, &Registry::new(), &mut |s| steps.push(s)).is_err());
        assert_eq!(steps, [Step::Stage]);
    }
}
