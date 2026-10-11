use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use serde_json::Value;

use pfx_editor_shell::worker::Wake;

pub const OUT_DIR: &str = "tmp/manfred/renders";
pub const TICK: Duration = Duration::from_millis(100);
pub const STATUS_EVERY: u32 = 4;
pub const SAMPLES: u32 = 256;
const STDERR_WAIT: Duration = Duration::from_millis(300);
const STDERR_KEPT: usize = 4000;
const FULL_SIDE: usize = 2048;
const THUMB_SIDE: usize = 640;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Queued,
    Waiting(String),
    Running,
    Done,
    Failed(String),
    Cancelled,
}

impl Phase {
    pub fn ended(&self) -> bool {
        matches!(self, Phase::Done | Phase::Failed(_) | Phase::Cancelled)
    }

    pub fn word(&self) -> &'static str {
        match self {
            Phase::Queued => "queued",
            Phase::Waiting(_) => "waiting",
            Phase::Running => "running",
            Phase::Done => "done",
            Phase::Failed(_) => "failed",
            Phase::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exit {
    pub ok: bool,
    pub code: Option<i32>,
    pub stderr: String,
}

pub trait Process: Send {
    fn pid(&self) -> u32;
    fn poll(&mut self) -> Option<Exit>;
    fn terminate(&mut self);

    fn progress(&mut self) -> Option<(u32, u32)> {
        None
    }
}

pub trait Runner: Send + Sync {
    fn start(&self, argv: &[String]) -> Result<Box<dyn Process>, String>;
    fn status(&self) -> Option<Value>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub camera: String,
    pub size: [u32; 2],
    pub out: PathBuf,
    pub pid: Option<u32>,
    pub phase: Phase,
    pub cancelling: bool,
    pub progress: Option<(u32, u32)>,
}

fn file_part(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn out_path(dir: &Path, recipe: &str, camera: &str, size: [u32; 2]) -> PathBuf {
    let stem = Path::new(recipe).file_stem().map_or_else(
        || "recipe".to_string(),
        |s| s.to_string_lossy().into_owned(),
    );
    dir.join(format!(
        "{}-{}-{}x{}.png",
        file_part(&stem),
        file_part(camera),
        size[0],
        size[1]
    ))
}

pub fn argv(
    exe: &str,
    recipe: &str,
    camera: &str,
    size: [u32; 2],
    samples: u32,
    out: &Path,
) -> Vec<String> {
    [
        exe,
        "render",
        recipe,
        "--camera",
        camera,
        "--size",
        &format!("{}x{}", size[0], size[1]),
        "--spp",
        &samples.max(1).to_string(),
        "--threshold",
        "0",
        "--out",
        &out.display().to_string(),
    ]
    .map(str::to_string)
    .to_vec()
}

fn phase_of(job: &Value) -> Phase {
    let state = job.get("state").and_then(Value::as_str).unwrap_or("");
    let waits = job.get("waits_for").and_then(Value::as_str);
    match (state, waits) {
        ("running" | "lent", _) => Phase::Running,
        (_, Some(reason)) => Phase::Waiting(reason.to_string()),
        ("yielded", None) => Phase::Waiting("yielded".to_string()),
        _ => Phase::Queued,
    }
}

pub fn parse_progress(line: &str) -> Option<(u32, u32)> {
    let (done, total) = line.trim().strip_prefix("progress ")?.split_once('/')?;
    let (done, total) = (done.parse::<u32>().ok()?, total.parse::<u32>().ok()?);
    (total > 0 && done <= total).then_some((done, total))
}

fn last_line(text: &str) -> String {
    let line = text
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let mut cut: String = line.chars().take(160).collect();
    if line.chars().count() > 160 {
        cut.push('…');
    }
    cut
}

impl Job {
    pub fn new(camera: &str, size: [u32; 2], out: PathBuf) -> Job {
        Job {
            camera: camera.to_string(),
            size,
            out,
            pid: None,
            phase: Phase::Queued,
            cancelling: false,
            progress: None,
        }
    }

    pub fn summary(&self) -> String {
        let what = format!("{} {}×{}", self.camera, self.size[0], self.size[1]);
        match &self.phase {
            Phase::Waiting(reason) => format!("{what} · waiting for {reason}"),
            Phase::Failed(reason) if !reason.is_empty() => {
                format!("{what} · failed: {reason}")
            }
            Phase::Running => match self.progress {
                Some((done, total)) => format!("{what} · running {done}/{total}"),
                None => format!("{what} · running"),
            },
            phase => format!("{what} · {}", phase.word()),
        }
    }

    pub fn tick(&mut self, process: &mut dyn Process, status: Option<&Value>) {
        if self.phase.ended() {
            return;
        }
        if let Some(exit) = process.poll() {
            self.finish(&exit);
            return;
        }
        if let Some(progress) = process.progress() {
            self.progress = Some(progress);
            if self.phase == Phase::Queued {
                self.phase = Phase::Running;
            }
        }
        if let Some(status) = status {
            self.watch(status);
        }
    }

    pub fn cancel(&mut self, process: &mut dyn Process) {
        if self.phase.ended() || self.cancelling {
            return;
        }
        self.cancelling = true;
        process.terminate();
    }

    fn watch(&mut self, status: &Value) {
        let Some(jobs) = status.get("jobs").and_then(Value::as_array) else {
            return;
        };
        let out = self.out.display().to_string();
        let ours = jobs.iter().find(|job| {
            let pid = job.get("pid").and_then(Value::as_u64);
            let label = job.get("label").and_then(Value::as_str).unwrap_or("");
            (pid.is_some() && pid == self.pid.map(u64::from))
                || (job.get("as").and_then(Value::as_str) == Some("manfred")
                    && label.contains(&out))
        });
        if let Some(job) = ours {
            self.phase = phase_of(job);
        }
    }

    fn finish(&mut self, exit: &Exit) {
        self.phase = if exit.ok && self.out.is_file() {
            Phase::Done
        } else if self.cancelling {
            Phase::Cancelled
        } else if exit.ok {
            Phase::Failed("manfred render wrote no picture".to_string())
        } else {
            let said = last_line(&exit.stderr);
            let how = match exit.code {
                Some(code) => format!("exit {code}"),
                None => "stopped by a signal".to_string(),
            };
            Phase::Failed(if said.is_empty() {
                how
            } else {
                format!("{how}: {said}")
            })
        };
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub full: Image,
    pub thumb: Image,
}

pub fn shrink(image: &Image, most: usize) -> Image {
    let [width, height] = image.size;
    let side = width.max(height);
    if side <= most || width == 0 || height == 0 {
        return image.clone();
    }
    let factor = side.div_ceil(most);
    let (w, h) = (width.div_ceil(factor), height.div_ceil(factor));
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            for sy in (y * factor)..((y + 1) * factor).min(height) {
                for sx in (x * factor)..((x + 1) * factor).min(width) {
                    let at = (sy * width + sx) * 4;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += u32::from(image.rgba[at + channel]);
                    }
                    count += 1;
                }
            }
            rgba.extend(sum.map(|total| (total / count.max(1)) as u8));
        }
    }
    Image { size: [w, h], rgba }
}

pub fn decode(bytes: &[u8]) -> Option<Picture> {
    let picture = manfred_scene::decode(bytes).ok()?;
    let image = Image {
        size: [picture.width as usize, picture.height as usize],
        rgba: picture.texels,
    };
    let full = shrink(&image, FULL_SIDE);
    let thumb = shrink(&full, THUMB_SIDE);
    Some(Picture { full, thumb })
}

struct Update {
    job: Job,
    picture: Option<Picture>,
}

struct Active {
    updates: Receiver<Update>,
    cancel: Arc<AtomicBool>,
}

pub struct Shots {
    runner: Arc<dyn Runner>,
    exe: String,
    dir: PathBuf,
    samples: u32,
    tick: Duration,
    waker: Option<Wake>,
    active: Option<Active>,
    latest: Option<Job>,
    picture: Option<Picture>,
}

impl Default for Shots {
    fn default() -> Shots {
        Shots::new()
    }
}

impl Shots {
    pub fn new() -> Shots {
        let exe = std::env::current_exe()
            .map_or_else(|_| "manfred".to_string(), |path| path.display().to_string());
        Shots::with(Arc::new(Real), exe)
    }

    pub fn with(runner: Arc<dyn Runner>, exe: String) -> Shots {
        Shots {
            runner,
            exe,
            dir: PathBuf::from(OUT_DIR),
            samples: SAMPLES,
            tick: TICK,
            waker: None,
            active: None,
            latest: None,
            picture: None,
        }
    }

    pub fn set_waker(&mut self, waker: Wake) {
        self.waker = Some(waker);
    }

    #[cfg(test)]
    pub fn set_tick(&mut self, tick: Duration) {
        self.tick = tick;
    }

    #[cfg(test)]
    pub fn set_dir(&mut self, dir: PathBuf) {
        self.dir = dir;
    }

    pub fn samples(&self) -> u32 {
        self.samples
    }

    pub fn set_samples(&mut self, samples: u32) {
        self.samples = samples.max(1);
    }

    pub fn job(&self) -> Option<&Job> {
        self.latest.as_ref()
    }

    #[cfg(test)]
    pub fn show(&mut self, job: Option<Job>) {
        self.latest = job;
    }

    pub fn busy(&self) -> bool {
        self.active.is_some()
    }

    #[cfg(test)]
    pub fn put_picture(&mut self, picture: Picture) {
        self.picture = Some(picture);
    }

    pub fn take_picture(&mut self) -> Option<Picture> {
        self.picture.take()
    }

    pub fn start(&mut self, recipe: &str, camera: &str, size: [u32; 2]) -> bool {
        if self.active.is_some() {
            return false;
        }
        let out = out_path(&self.dir, recipe, camera, size);
        let line = argv(&self.exe, recipe, camera, size, self.samples, &out);
        let job = Job::new(camera, size, out);
        let (send, updates) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let drive = Drive {
            job: job.clone(),
            runner: self.runner.clone(),
            line,
            cancel: cancel.clone(),
            send,
            waker: self.waker.clone(),
            tick: self.tick,
        };
        std::thread::spawn(move || drive.run());
        self.latest = Some(job);
        self.picture = None;
        self.active = Some(Active { updates, cancel });
        true
    }

    pub fn cancel(&mut self) {
        if let Some(active) = &self.active {
            active.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn poll(&mut self) -> bool {
        let Some(active) = &self.active else {
            return false;
        };
        let mut changed = false;
        let mut ended = false;
        loop {
            match active.updates.try_recv() {
                Ok(update) => {
                    changed = true;
                    ended = update.job.phase.ended();
                    self.latest = Some(update.job);
                    if update.picture.is_some() {
                        self.picture = update.picture;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    ended = true;
                    break;
                }
            }
        }
        if ended {
            if let Some(job) = self.latest.as_mut()
                && !job.phase.ended()
            {
                job.phase = Phase::Failed("the render thread stopped".to_string());
                changed = true;
            }
            self.active = None;
        }
        changed
    }
}

struct Drive {
    job: Job,
    runner: Arc<dyn Runner>,
    line: Vec<String>,
    cancel: Arc<AtomicBool>,
    send: Sender<Update>,
    waker: Option<Wake>,
    tick: Duration,
}

impl Drive {
    fn tell(&self, job: &Job, picture: Option<Picture>) -> bool {
        let sent = self
            .send
            .send(Update {
                job: job.clone(),
                picture,
            })
            .is_ok();
        if let Some(waker) = &self.waker {
            waker();
        }
        sent
    }

    fn run(self) {
        let mut job = self.job.clone();
        if let Some(parent) = job.out.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            job.phase = Phase::Failed(format!("cannot make {}: {error}", parent.display()));
            self.tell(&job, None);
            return;
        }
        let mut process = match self.runner.start(&self.line) {
            Ok(process) => process,
            Err(error) => {
                job.phase = Phase::Failed(error);
                self.tell(&job, None);
                return;
            }
        };
        job.pid = Some(process.pid());
        let mut sent = job.clone();
        if !self.tell(&job, None) {
            process.terminate();
            return;
        }
        let mut turn = 0u32;
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                job.cancel(process.as_mut());
            }
            let status = turn
                .is_multiple_of(STATUS_EVERY)
                .then(|| self.runner.status())
                .flatten();
            turn += 1;
            job.tick(process.as_mut(), status.as_ref());
            if job != sent {
                let picture = (job.phase == Phase::Done)
                    .then(|| std::fs::read(&job.out).ok().and_then(|b| decode(&b)))
                    .flatten();
                sent = job.clone();
                if !self.tell(&job, picture) {
                    process.terminate();
                    return;
                }
            }
            if job.phase.ended() {
                return;
            }
            std::thread::sleep(self.tick);
        }
    }
}

struct Real;

struct Running {
    child: Child,
    stderr: Receiver<String>,
    seen: Arc<AtomicU64>,
}

impl Process for Running {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn poll(&mut self) -> Option<Exit> {
        match self.child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => Some(Exit {
                ok: status.success(),
                code: status.code(),
                stderr: self.stderr.recv_timeout(STDERR_WAIT).unwrap_or_default(),
            }),
            Err(error) => Some(Exit {
                ok: false,
                code: None,
                stderr: error.to_string(),
            }),
        }
    }

    fn progress(&mut self) -> Option<(u32, u32)> {
        let packed = self.seen.load(Ordering::Relaxed);
        (packed != 0).then_some(((packed >> 32) as u32, packed as u32))
    }

    fn terminate(&mut self) {
        let _ = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn tail(text: String) -> String {
    let count = text.chars().count();
    if count <= STDERR_KEPT {
        return text;
    }
    text.chars().skip(count - STDERR_KEPT).collect()
}

impl Runner for Real {
    fn start(&self, argv: &[String]) -> Result<Box<dyn Process>, String> {
        let (program, rest) = argv.split_first().ok_or("no command to run")?;
        let mut child = Command::new(program)
            .args(rest)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("cannot start {program}: {error}"))?;
        let (send, stderr) = mpsc::channel();
        let seen = Arc::new(AtomicU64::new(0));
        if let Some(pipe) = child.stderr.take() {
            let seen = seen.clone();
            std::thread::spawn(move || {
                let mut text = String::new();
                let mut lines = BufReader::new(pipe);
                let mut bytes = Vec::new();
                while lines
                    .read_until(b'\n', &mut bytes)
                    .is_ok_and(|read| read > 0)
                {
                    let line = String::from_utf8_lossy(&bytes);
                    match parse_progress(&line) {
                        Some((done, total)) => {
                            seen.store(u64::from(done) << 32 | u64::from(total), Ordering::Relaxed);
                        }
                        None => text.push_str(&line),
                    }
                    bytes.clear();
                }
                let _ = send.send(tail(text));
            });
        }
        Ok(Box::new(Running {
            child,
            stderr,
            seen,
        }))
    }

    fn status(&self) -> Option<Value> {
        pfx_editor_shell::pgpu::fetch()
    }
}

#[cfg(test)]
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub polls: Vec<Option<Exit>>,
    pub cancelled: bool,
    pub progress: Option<(u32, u32)>,
}

#[cfg(test)]
#[derive(Debug)]
pub struct Fake {
    pub pid: u32,
    pub script: Arc<Mutex<Plan>>,
}

#[cfg(test)]
impl Process for Fake {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn poll(&mut self) -> Option<Exit> {
        let mut script = self.script.lock().unwrap();
        if script.cancelled {
            return Some(Exit {
                ok: false,
                code: None,
                stderr: String::new(),
            });
        }
        if script.polls.is_empty() {
            None
        } else {
            script.polls.remove(0)
        }
    }

    fn progress(&mut self) -> Option<(u32, u32)> {
        self.script.lock().unwrap().progress
    }

    fn terminate(&mut self) {
        self.script.lock().unwrap().cancelled = true;
    }
}

#[cfg(test)]
pub struct Runs {
    pub processes: Mutex<Vec<Fake>>,
    pub statuses: Arc<Mutex<Vec<String>>>,
    pub refuse: bool,
}

#[cfg(test)]
impl Runner for Runs {
    fn start(&self, _: &[String]) -> Result<Box<dyn Process>, String> {
        if self.refuse {
            return Err("cannot start pgpu: not found".to_string());
        }
        let mut processes = self.processes.lock().unwrap();
        if processes.is_empty() {
            return Err("no process to start".to_string());
        }
        Ok(Box::new(processes.remove(0)))
    }

    fn status(&self) -> Option<Value> {
        let mut statuses = self.statuses.lock().unwrap();
        let text = if statuses.len() > 1 {
            statuses.remove(0)
        } else {
            statuses.first().cloned().unwrap_or_default()
        };
        serde_json::from_str(&text).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(text: &str) -> Value {
        serde_json::from_str(text).unwrap_or(Value::Null)
    }
    use crate::scratch;

    const RUNNING: &str = r#"{"jobs":[{"pid":7,"state":"running","as":"pfx","waits_for":null},{"pid":42,"state":"running","as":"manfred","label":"x","waits_for":null}]}"#;
    const WAITING: &str = r#"{"jobs":[{"pid":7,"state":"running","as":"pfx","waits_for":null},{"pid":42,"state":"waiting","as":"manfred","label":"x","waits_for":"desk"}]}"#;
    const BEHIND: &str = r#"{"jobs":[{"pid":7,"state":"running","as":"pfx","waits_for":null},{"pid":42,"state":"waiting","as":"manfred","label":"x","waits_for":null}]}"#;
    const YIELDED: &str =
        r#"{"jobs":[{"pid":42,"state":"yielded","as":"manfred","label":"x","waits_for":null}]}"#;
    const ABSENT: &str = r#"{"jobs":[{"pid":7,"state":"running","as":"pfx","waits_for":null}]}"#;

    fn job(out: &Path) -> Job {
        let mut job = Job::new("front", [1586, 992], out.to_path_buf());
        job.pid = Some(42);
        job
    }

    fn fake(polls: Vec<Option<Exit>>) -> Fake {
        Fake {
            pid: 42,
            script: Arc::new(Mutex::new(Plan {
                polls,
                ..Plan::default()
            })),
        }
    }

    fn good() -> Exit {
        Exit {
            ok: true,
            code: Some(0),
            stderr: String::new(),
        }
    }

    fn picture(path: &Path) {
        let rgba: Vec<u8> = (0..8 * 4).flat_map(|n| [n as u8, 0, 0, 255]).collect();
        std::fs::write(path, crate::encode_png([8, 4], &rgba).unwrap()).unwrap();
    }

    #[test]
    fn the_command_is_manfred_render_with_its_samples() {
        let out = out_path(Path::new(OUT_DIR), "work/scene.toml", "front", [1586, 992]);
        assert_eq!(
            out,
            Path::new("tmp/manfred/renders/scene-front-1586x992.png")
        );
        let line = argv(
            "/opt/manfred",
            "work/scene.toml",
            "front",
            [1586, 992],
            64,
            &out,
        );
        assert_eq!(
            line,
            [
                "/opt/manfred",
                "render",
                "work/scene.toml",
                "--camera",
                "front",
                "--size",
                "1586x992",
                "--spp",
                "64",
                "--threshold",
                "0",
                "--out",
                "tmp/manfred/renders/scene-front-1586x992.png",
            ]
        );
        assert_eq!(argv("manfred", "a.toml", "c", [1, 1], 0, &out)[8], "1");
    }

    #[test]
    fn the_samples_start_at_256_and_never_fall_below_one() {
        let mut shots = Shots::new();
        assert_eq!(shots.samples(), SAMPLES);
        assert_eq!(SAMPLES, 256);
        shots.set_samples(32);
        assert_eq!(shots.samples(), 32);
        shots.set_samples(0);
        assert_eq!(shots.samples(), 1);
    }

    #[test]
    fn a_camera_name_never_leaves_the_folder() {
        let out = out_path(Path::new(OUT_DIR), "a.toml", "../x y", [10, 20]);
        assert_eq!(out, Path::new("tmp/manfred/renders/a-.._x_y-10x20.png"));
    }

    #[test]
    fn a_render_goes_queued_waiting_running_done() {
        let out = scratch("shots-done").with_file_name("done.png");
        std::fs::remove_file(&out).ok();
        let mut job = job(&out);
        let mut process = fake(vec![None, None, None, None, None, Some(good())]);
        assert_eq!(job.phase, Phase::Queued);
        job.tick(&mut process, Some(&seen(BEHIND)));
        assert_eq!(job.phase, Phase::Queued);
        job.tick(&mut process, Some(&seen(WAITING)));
        assert_eq!(job.phase, Phase::Waiting("desk".to_string()));
        assert_eq!(job.summary(), "front 1586×992 · waiting for desk");
        job.tick(&mut process, Some(&seen(YIELDED)));
        assert_eq!(job.phase, Phase::Waiting("yielded".to_string()));
        job.tick(&mut process, Some(&seen(RUNNING)));
        assert_eq!(job.phase, Phase::Running);
        job.tick(&mut process, None);
        assert_eq!(job.phase, Phase::Running);
        picture(&out);
        job.tick(&mut process, Some(&seen(RUNNING)));
        assert_eq!(job.phase, Phase::Done);
        assert_eq!(job.summary(), "front 1586×992 · done");
        job.tick(&mut process, Some(&seen(WAITING)));
        assert_eq!(job.phase, Phase::Done);
    }

    #[test]
    fn without_the_queue_a_render_runs_once_its_progress_shows() {
        let out = scratch("shots-unqueued").with_file_name("unqueued.png");
        std::fs::remove_file(&out).ok();
        let mut job = job(&out);
        let mut process = fake(vec![None, None, None, Some(good())]);
        job.tick(&mut process, None);
        assert_eq!(job.phase, Phase::Queued);
        process.script.lock().unwrap().progress = Some((3, 8));
        job.tick(&mut process, None);
        assert_eq!(job.phase, Phase::Running);
        assert_eq!(job.summary(), "front 1586×992 · running 3/8");
        picture(&out);
        job.tick(&mut process, None);
        job.tick(&mut process, None);
        assert_eq!(job.phase, Phase::Done);
    }

    #[test]
    fn progress_lines_parse_and_nothing_else_does() {
        assert_eq!(parse_progress("progress 12/256"), Some((12, 256)));
        assert_eq!(parse_progress("progress 0/1\n"), Some((0, 1)));
        assert_eq!(parse_progress("  progress 256/256  "), Some((256, 256)));
        for text in [
            "",
            "progress",
            "progress 5",
            "progress 5/0",
            "progress 9/8",
            "progress -1/8",
            "progress a/b",
            "samples 5/8",
            "manfred render: progress 5/8",
        ] {
            assert_eq!(parse_progress(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_running_job_carries_the_samples_done_out_of_total() {
        let out = scratch("shots-progress").with_file_name("progress.png");
        let mut job = job(&out);
        let mut process = fake(vec![]);
        job.tick(&mut process, Some(&seen(RUNNING)));
        assert_eq!(job.progress, None);
        assert_eq!(job.summary(), "front 1586×992 · running");
        process.script.lock().unwrap().progress = Some((64, 256));
        job.tick(&mut process, None);
        assert_eq!(job.progress, Some((64, 256)));
        assert_eq!(job.summary(), "front 1586×992 · running 64/256");
    }

    #[test]
    fn a_real_render_is_read_line_by_line_while_it_runs() {
        let argv: Vec<String> = [
            "sh",
            "-c",
            "echo 'progress 3/8' >&2; echo 'cannot write' >&2; echo 'progress 5/8' >&2; exec sleep 30",
        ]
        .map(str::to_string)
        .to_vec();
        let mut process = Real.start(&argv).unwrap();
        let mut waited = 0;
        while process.progress() != Some((5, 8)) {
            assert!(process.poll().is_none());
            waited += 1;
            assert!(waited < 3000, "no progress line in three seconds");
            std::thread::sleep(Duration::from_millis(1));
        }
        process.terminate();
        let exit = loop {
            if let Some(exit) = process.poll() {
                break exit;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        assert!(!exit.ok);
        assert_eq!(exit.stderr, "cannot write\n");
    }

    #[test]
    fn a_status_without_the_job_or_with_noise_changes_nothing() {
        let out = scratch("shots-noise").with_file_name("noise.png");
        let mut job = job(&out);
        let mut process = fake(vec![]);
        job.tick(&mut process, Some(&seen(RUNNING)));
        assert_eq!(job.phase, Phase::Running);
        for text in [ABSENT, "", "not json", "{}", r#"{"jobs":"x"}"#] {
            job.tick(&mut process, Some(&seen(text)));
            assert_eq!(job.phase, Phase::Running, "{text:?}");
        }
    }

    #[test]
    fn the_job_is_found_by_its_label_when_the_pid_differs() {
        let out = scratch("shots-label").with_file_name("label.png");
        let mut job = job(&out);
        job.pid = Some(1);
        let text = format!(
            r#"{{"jobs":[{{"pid":9,"state":"running","as":"manfred","label":"manfred render --out {}"}}]}}"#,
            out.display()
        );
        job.tick(&mut fake(vec![]), Some(&seen(&text)));
        assert_eq!(job.phase, Phase::Running);
        let other =
            r#"{"jobs":[{"pid":9,"state":"running","as":"manfred","label":"elsewhere.png"}]}"#;
        let mut fresh = Job::new("front", [1, 1], PathBuf::from("a.png"));
        fresh.pid = Some(1);
        fresh.tick(&mut fake(vec![]), Some(&seen(other)));
        assert_eq!(fresh.phase, Phase::Queued);
    }

    #[test]
    fn a_failing_process_says_why() {
        let out = scratch("shots-fail").with_file_name("fail.png");
        std::fs::remove_file(&out).ok();
        let mut job = job(&out);
        let exit = Exit {
            ok: false,
            code: Some(3),
            stderr: "warming up\nmanfred render: no camera named side\n\n".to_string(),
        };
        job.tick(&mut fake(vec![Some(exit)]), Some(&seen(RUNNING)));
        assert_eq!(
            job.phase,
            Phase::Failed("exit 3: manfred render: no camera named side".to_string())
        );
        assert!(job.phase.ended());
        assert_eq!(
            job.summary(),
            "front 1586×992 · failed: exit 3: manfred render: no camera named side"
        );
    }

    #[test]
    fn a_clean_exit_without_a_picture_fails_and_a_signal_says_so() {
        let out = scratch("shots-none").with_file_name("none.png");
        std::fs::remove_file(&out).ok();
        let mut job = job(&out);
        job.tick(&mut fake(vec![Some(good())]), None);
        assert_eq!(
            job.phase,
            Phase::Failed("manfred render wrote no picture".to_string())
        );
        let mut job = self::job(&out);
        let killed = Exit {
            ok: false,
            code: None,
            stderr: String::new(),
        };
        job.tick(&mut fake(vec![Some(killed)]), None);
        assert_eq!(job.phase, Phase::Failed("stopped by a signal".to_string()));
    }

    #[test]
    fn cancel_sends_term_once_and_ends_cancelled() {
        let out = scratch("shots-cancel").with_file_name("cancel.png");
        std::fs::remove_file(&out).ok();
        let mut job = job(&out);
        let mut process = fake(vec![]);
        job.tick(&mut process, Some(&seen(RUNNING)));
        job.cancel(&mut process);
        assert!(job.cancelling);
        assert!(process.script.lock().unwrap().cancelled);
        assert_eq!(job.phase, Phase::Running);
        job.tick(&mut process, Some(&seen(RUNNING)));
        assert_eq!(job.phase, Phase::Cancelled);
        assert_eq!(job.summary(), "front 1586×992 · cancelled");
        process.script.lock().unwrap().cancelled = false;
        job.cancel(&mut process);
        assert!(!process.script.lock().unwrap().cancelled);
    }

    #[test]
    fn a_render_that_finishes_as_it_is_cancelled_is_done() {
        let out = scratch("shots-race").with_file_name("race.png");
        picture(&out);
        let mut job = job(&out);
        let mut process = fake(vec![Some(good())]);
        job.cancel(&mut process);
        process.script.lock().unwrap().cancelled = false;
        job.tick(&mut process, None);
        assert_eq!(job.phase, Phase::Done);
    }

    #[test]
    fn a_picture_is_boxed_down_to_the_side_asked() {
        let rgba: Vec<u8> = (0..40 * 20).flat_map(|_| [200, 100, 0, 255]).collect();
        let image = Image {
            size: [40, 20],
            rgba,
        };
        let small = shrink(&image, 10);
        assert_eq!(small.size, [10, 5]);
        assert_eq!(small.rgba.len(), 10 * 5 * 4);
        assert_eq!(&small.rgba[..4], &[200, 100, 0, 255]);
        assert_eq!(shrink(&image, 40), image);
        let odd = Image {
            size: [5, 3],
            rgba: vec![9; 5 * 3 * 4],
        };
        assert_eq!(shrink(&odd, 2).size, [2, 1]);
    }

    #[test]
    fn a_png_decodes_to_a_full_and_a_thumbnail() {
        let rgba: Vec<u8> = (0..3000 * 1000)
            .flat_map(|n| [(n % 251) as u8, 7, 9, 255])
            .collect();
        let bytes = crate::encode_png([3000, 1000], &rgba).unwrap();
        let picture = decode(&bytes).unwrap();
        assert_eq!(picture.full.size, [1500, 500]);
        assert!(picture.thumb.size[0] <= THUMB_SIDE);
        assert_eq!(decode(b"not a png"), None);
    }

    fn settle(shots: &mut Shots) -> Vec<String> {
        let mut seen = Vec::new();
        while shots.busy() {
            if shots.poll()
                && let Some(job) = shots.job()
            {
                seen.push(job.phase.word().to_string());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        shots.poll();
        seen
    }

    #[test]
    fn the_window_side_follows_a_thread_through_to_a_picture() {
        let recipe = scratch("shots-run").with_file_name("shots-run.toml");
        let dir = recipe.with_file_name("renders");
        let out = out_path(&dir, &recipe.display().to_string(), "front", [8, 4]);
        std::fs::create_dir_all(&dir).unwrap();
        picture(&out);
        let process = fake(vec![None, None, None, None, None, None, Some(good())]);
        let pid = process.pid;
        let runner = Arc::new(Runs {
            processes: Mutex::new(vec![process]),
            statuses: Arc::new(Mutex::new(vec![WAITING.to_string(), RUNNING.to_string()])),
            refuse: false,
        });
        let mut shots = Shots::with(runner, "manfred".to_string());
        shots.set_tick(Duration::from_millis(2));
        shots.set_dir(dir);
        assert!(shots.start(&recipe.display().to_string(), "front", [8, 4]));
        assert!(!shots.start("again.toml", "front", [8, 4]));
        assert_eq!(shots.job().unwrap().phase, Phase::Queued);
        let seen = settle(&mut shots);
        assert_eq!(seen.last().map(String::as_str), Some("done"), "{seen:?}");
        assert_eq!(shots.job().unwrap().pid, Some(pid));
        assert_eq!(shots.job().unwrap().phase, Phase::Done);
        let picture = shots.take_picture().unwrap();
        assert_eq!(picture.full.size, [8, 4]);
        assert!(shots.take_picture().is_none());
        assert!(!shots.busy());
        std::fs::remove_file(&out).ok();
    }

    #[test]
    fn a_thread_stops_the_process_when_cancel_is_asked() {
        let recipe = scratch("shots-stop").with_file_name("shots-stop.toml");
        let process = fake(vec![]);
        let runner = Arc::new(Runs {
            processes: Mutex::new(vec![process]),
            statuses: Arc::new(Mutex::new(vec![RUNNING.to_string()])),
            refuse: false,
        });
        let mut shots = Shots::with(runner, "manfred".to_string());
        shots.set_tick(Duration::from_millis(2));
        shots.set_dir(recipe.with_file_name("renders"));
        assert!(shots.start(&recipe.display().to_string(), "side", [8, 4]));
        while shots.job().unwrap().phase != Phase::Running {
            shots.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
        shots.cancel();
        settle(&mut shots);
        assert_eq!(shots.job().unwrap().phase, Phase::Cancelled);
        assert!(shots.take_picture().is_none());
    }

    #[test]
    fn a_command_that_will_not_start_fails_at_once() {
        let runner = Arc::new(Runs {
            processes: Mutex::new(vec![]),
            statuses: Arc::new(Mutex::new(vec![])),
            refuse: true,
        });
        let recipe = scratch("shots-refuse").with_file_name("shots-refuse.toml");
        let mut shots = Shots::with(runner, "manfred".to_string());
        shots.set_dir(recipe.with_file_name("renders"));
        shots.start(&recipe.display().to_string(), "front", [8, 4]);
        settle(&mut shots);
        assert_eq!(
            shots.job().unwrap().phase,
            Phase::Failed("cannot start pgpu: not found".to_string())
        );
        assert!(shots.start(&recipe.display().to_string(), "front", [8, 4]));
        settle(&mut shots);
    }
}
