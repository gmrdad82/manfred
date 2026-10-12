use crate::picture::Film;
use crate::staging::{Staging, View, eyepiece};
use crate::text::Overlay;
use manfred_build::Registry;
use manfred_recipe::{Camera, Recipe};
use pfx::adapter::{Report, TurnState, turn_reporting};
use pfx_denoise::Denoiser;
use pfx_gpu::Gpu;
use pfx_gpu::pace::{Pacer, Stats};
use pfx_post::Chain;
use pfx_trace::sky::Environment;
use pfx_trace::stage::Staged;
use pfx_trace::{Adaptive, Round, Trace, TraceError};
use std::cell::RefCell;
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
    Trace {
        done: u32,
        total: u32,
    },
    Converge {
        gpu_ms: u64,
        cap: u32,
        done: u64,
        total: u64,
    },
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
    pub crop: Option<Crop>,
    pub denoise: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crop {
    pub rect: [u32; 4],
    pub apron: u32,
    pub finished: bool,
}

impl Crop {
    pub fn fits(rect: [u32; 4], size: [u32; 2]) -> bool {
        let [x, y, width, height] = rect;
        width > 0
            && height > 0
            && x.checked_add(width).is_some_and(|end| end <= size[0])
            && y.checked_add(height).is_some_and(|end| end <= size[1])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub full: [u32; 2],
    pub origin: [u32; 2],
    pub size: [u32; 2],
}

impl Region {
    pub fn whole(size: [u32; 2]) -> Region {
        Region {
            full: size,
            origin: [0, 0],
            size,
        }
    }

    pub fn around(full: [u32; 2], rect: [u32; 4], apron: u32) -> Region {
        let [x, y, width, height] = rect;
        let low = [x.saturating_sub(apron), y.saturating_sub(apron)];
        let high = [
            (x + width).saturating_add(apron).min(full[0]),
            (y + height).saturating_add(apron).min(full[1]),
        ];
        Region {
            full,
            origin: low,
            size: [high[0] - low[0], high[1] - low[1]],
        }
    }

    fn open(&self, gpu: &Gpu, staged: &Staged<Environment>) -> Result<Trace, Fault> {
        let mut trace = staged
            .trace(gpu, self.size[0], self.size[1])
            .map_err(refused)?;
        if *self != Region::whole(self.full) {
            trace
                .set_window(self.full, self.origin)
                .map_err(|e| Fault::Failed(format!("the tracer's window: {e}")))?;
        }
        Ok(trace)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    rounds: Vec<u32>,
    reached: usize,
    active: u64,
    work: u64,
    work_ms: f64,
}

impl Plan {
    pub fn new(adaptive: &Adaptive, pixels: u64) -> Plan {
        Plan {
            rounds: adaptive.rounds(),
            reached: 0,
            active: pixels,
            work: 0,
            work_ms: 0.0,
        }
    }

    fn before(&self) -> u32 {
        self.reached.checked_sub(1).map_or(0, |k| self.rounds[k])
    }

    pub fn reach(&mut self, round: Round, gpu_ms: f64) {
        self.work += u64::from(round.samples.saturating_sub(self.before())) * self.active;
        self.active = round.active;
        self.reached = (self.reached + 1).min(self.rounds.len());
        self.work_ms = gpu_ms;
        if round.active == 0 {
            self.reached = self.rounds.len();
        }
    }

    fn next(&self) -> u64 {
        self.rounds.get(self.reached).map_or(0, |&samples| {
            u64::from(samples - self.before()) * self.active
        })
    }

    pub fn total(&self) -> u64 {
        let mut total = self.work;
        let mut before = self.before();
        for &samples in &self.rounds[self.reached..] {
            total += u64::from(samples - before) * self.active;
            before = samples;
        }
        total
    }

    pub fn done(&self, gpu_ms: f64) -> u64 {
        if self.work == 0 || self.work_ms <= 0.0 {
            return 0;
        }
        let rate = self.work as f64 / self.work_ms;
        let partial = ((gpu_ms - self.work_ms).max(0.0) * rate) as u64;
        self.work + partial.min(self.next())
    }
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
    pub most_samples: u32,
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
    if let Some(crop) = request.crop
        && crop.finished
        && let Some(reason) =
            crate::picture::crop_refusal((&staging.finish, &Overlay::of(&staging.text)))
    {
        return Err(Fault::Refused(format!(
            "a crop's .png or master cannot match the full frame: {reason}; crop to .exr or .hdr"
        )));
    }
    let staged = staging.stage(eyepiece(camera, request.view, request.size)?)?;
    turn(Report::default());
    let adaptive = request
        .noise
        .and_then(|noise| noise.adaptive(request.samples));
    let region = match request.crop {
        Some(crop) if !Crop::fits(crop.rect, request.size) => {
            return Err(Fault::Refused(format!(
                "the crop {:?} does not lie inside the {}x{} frame",
                crop.rect, request.size[0], request.size[1]
            )));
        }
        Some(crop) => Region::around(
            request.size,
            crop.rect,
            crop.apron.max(adaptive.map_or(0, |a| a.margin())),
        ),
        None => Region::whole(request.size),
    };
    let gpu = manfred_scene::headless()?;
    let seed = request.seed.unwrap_or_else(|| manfred_scene::seed(&recipe));
    let mut denoiser = request.denoise.then(|| denoiser(&gpu)).transpose()?;
    let (film, paced, mean_samples, most_samples) = match adaptive {
        Some(adaptive) => converge(
            &gpu,
            &staged,
            Shot {
                region,
                seed,
                denoiser: denoiser.as_mut(),
            },
            adaptive,
            progress,
            &mut turn,
        )?,
        None => {
            let (film, paced) = trace_in(
                &gpu,
                &staged,
                Shot {
                    region,
                    seed,
                    denoiser: denoiser.as_mut(),
                },
                request.samples,
                progress,
                &mut turn,
            )?;
            (film, paced, f64::from(request.samples), request.samples)
        }
    };
    let film = match request.crop {
        Some(crop) => film.cut(
            [
                crop.rect[0] - region.origin[0],
                crop.rect[1] - region.origin[1],
            ],
            [crop.rect[2], crop.rect[3]],
        ),
        None => film,
    };
    Ok(Rendered {
        film,
        overlay: Overlay::of(&staging.text),
        finish: staging.finish,
        paced,
        seed,
        mean_samples,
        most_samples,
    })
}

pub struct Reel {
    pub recipe: Recipe,
    pub seed: u32,
    staging: Staging,
    gpu: Gpu,
    denoiser: Option<Denoiser>,
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
            denoiser: None,
        })
    }

    pub fn frame(
        &mut self,
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
        if self.denoiser.is_none() {
            self.denoiser = Some(denoiser(&self.gpu)?);
        }
        let (film, paced) = trace_in(
            &self.gpu,
            &staged,
            Shot {
                region: Region::whole(size),
                seed: self.seed,
                denoiser: self.denoiser.as_mut(),
            },
            samples,
            progress,
            &mut turn,
        )?;
        Ok(Rendered {
            film,
            overlay: Overlay::of(&self.staging.text),
            finish: self.staging.finish.clone(),
            paced,
            seed: self.seed,
            mean_samples: f64::from(samples),
            most_samples: samples,
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
    trace_in(
        gpu,
        staged,
        Shot {
            region: Region::whole(size),
            seed,
            denoiser: None,
        },
        samples,
        progress,
        turn,
    )
}

pub struct Shot<'a> {
    pub region: Region,
    pub seed: u32,
    pub denoiser: Option<&'a mut Denoiser>,
}

fn trace_in(
    gpu: &Gpu,
    staged: &Staged<Environment>,
    shot: Shot<'_>,
    samples: u32,
    progress: &mut dyn FnMut(Step),
    turn: &mut dyn FnMut(Report),
) -> Result<(Film, Paced), Fault> {
    let Shot {
        region,
        seed,
        denoiser,
    } = shot;
    let mut trace = region.open(gpu, staged)?;
    let mut turns = Turns::new(turn);
    let paced = sampled(samples, &mut turns, progress, |pacer, between| {
        trace.sample_paced(gpu, 1, seed, pacer, between)
    })?;
    let film = developed(gpu, &trace, region.size, samples, denoiser)?;
    turns.now();
    Ok((film, paced))
}

pub fn converge(
    gpu: &Gpu,
    staged: &Staged<Environment>,
    shot: Shot<'_>,
    adaptive: Adaptive,
    progress: &mut dyn FnMut(Step),
    turn: &mut dyn FnMut(Report),
) -> Result<(Film, Paced, f64, u32), Fault> {
    let Shot {
        region,
        seed,
        denoiser,
    } = shot;
    let mut trace = region.open(gpu, staged)?;
    let mut turns = Turns::new(turn);
    let mut pacer = Pacer::default();
    let cap = adaptive.max_samples;
    let pixels = u64::from(region.size[0]) * u64::from(region.size[1]);
    let watch = RefCell::new((Plan::new(&adaptive, pixels), 0.0f64, progress));
    let tell = |watch: &mut (Plan, f64, &mut dyn FnMut(Step))| {
        let (plan, gpu_ms, progress) = watch;
        progress(Step::Converge {
            gpu_ms: *gpu_ms as u64,
            cap,
            done: plan.done(*gpu_ms),
            total: plan.total(),
        });
    };
    turns.now();
    tell(&mut watch.borrow_mut());
    let converged = trace
        .sample_adaptive_with(
            gpu,
            adaptive,
            seed,
            &mut pacer,
            |ms| {
                let mut watch = watch.borrow_mut();
                watch.1 += ms;
                tell(&mut watch);
                turns.soon(ms)
            },
            |round| {
                let mut watch = watch.borrow_mut();
                let gpu_ms = watch.1;
                watch.0.reach(round, gpu_ms);
                tell(&mut watch);
            },
        )
        .map_err(|e| Fault::Failed(format!("the tracer: {e}")))?;
    let mut paced = Paced::default();
    paced.add(&converged.stats);
    turns.now();
    let film = developed(gpu, &trace, region.size, cap, denoiser)?;
    turns.now();
    Ok((
        film,
        paced,
        converged.mean_samples(),
        converged.max_samples(),
    ))
}

fn denoiser(gpu: &Gpu) -> Result<Denoiser, Fault> {
    Denoiser::new(gpu).map_err(|e| Fault::Failed(format!("the denoiser: {e}")))
}

fn developed(
    gpu: &Gpu,
    trace: &Trace,
    size: [u32; 2],
    samples: u32,
    denoiser: Option<&mut Denoiser>,
) -> Result<Film, Fault> {
    let denoised = denoiser
        .map(|denoiser| {
            denoiser.denoise_textures(
                gpu,
                &trace.color.texture,
                &trace.albedo.texture,
                &trace.normal.texture,
            )
        })
        .transpose()
        .map_err(|e| Fault::Failed(format!("the denoiser: {e}")))?;
    let output = trace
        .readback(gpu)
        .map_err(|e| Fault::Failed(format!("the tracer's readback: {e}")))?;
    Ok(Film {
        size,
        color: floats(&output.color),
        albedo: floats(&output.albedo),
        normal: floats(&output.normal),
        denoised,
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
    fn the_plan_counts_pixel_samples_and_shrinks_as_pixels_stop() {
        let adaptive = Adaptive {
            threshold: 0.01,
            min_samples: 16,
            max_samples: 100,
            growth: 1.5,
        };
        let mut plan = Plan::new(&adaptive, 10);
        assert_eq!(plan.total(), 1000);
        assert_eq!(plan.done(5.0), 0);
        plan.reach(
            Round {
                samples: 8,
                active: 10,
            },
            8.0,
        );
        assert_eq!(plan.total(), 1000);
        assert_eq!(plan.done(8.0), 80);
        assert_eq!(plan.done(12.0), 120);
        assert_eq!(plan.done(1e6), 160);
        plan.reach(
            Round {
                samples: 16,
                active: 4,
            },
            16.0,
        );
        assert_eq!(plan.total(), 160 + 84 * 4);
        plan.reach(
            Round {
                samples: 24,
                active: 0,
            },
            19.0,
        );
        assert_eq!(plan.total(), 192);
        assert_eq!(plan.done(30.0), 192);
    }

    #[test]
    fn a_crop_grows_by_its_apron_inside_the_frame() {
        assert_eq!(
            Region::around([100, 50], [10, 40, 20, 10], 12),
            Region {
                full: [100, 50],
                origin: [0, 28],
                size: [42, 22],
            }
        );
        assert_eq!(
            Region::around([100, 50], [0, 0, 100, 50], 9),
            Region::whole([100, 50])
        );
        assert!(Crop::fits([90, 0, 10, 50], [100, 50]));
        for rect in [[91, 0, 10, 1], [0, 0, 0, 1], [u32::MAX, 0, 2, 1]] {
            assert!(!Crop::fits(rect, [100, 50]), "{rect:?}");
        }
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
            crop: None,
            denoise: false,
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
