use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use egui::{
    Color32, Context, Event, FullOutput, Id, Key, LayerId, Modifiers, MouseWheelUnit, Order,
    PointerButton, Pos2, RawInput, Rect, Shape, Stroke, TexturesDelta, Vec2, ViewportId,
    ViewportInfo,
};
use egui_wgpu::{RendererOptions, ScreenDescriptor};
use pfx_editor_shell::paint::Painter;
use pfx_editor_shell::{App, Error, MAX_SIDE, Mods, Script, Step, block_on, check, context};
use pfx_gpu::pace::Turns;
use pfx_gpu::{Gpu, wgpu};

use crate::Manfred;
use crate::file::Step as FileStep;
use crate::theme::Theme;

const VERBS: [&str; 9] = [
    "key", "text", "click", "move", "press", "release", "drag", "scroll", "frame",
];
const GLIDE_SPEED: f32 = 1500.0;
const GLIDE_SHORTEST: f32 = 0.18;
const GLIDE_LONGEST: f32 = 0.6;
const DRAG_SPEED: f32 = 600.0;
const DRAG_SHORTEST: f32 = 0.3;
const DRAG_LONGEST: f32 = 1.2;
const TYPING: f32 = 15.0;
const LONGEST_WAIT: f32 = 600.0;
pub const MAX_FRAMES: usize = 36_000;
const POLL: Duration = Duration::from_millis(20);
const PATIENCE: Duration = Duration::from_secs(30 * 60);
const SETTLE_PASSES: u32 = 240;
const TRACKS: [&str; 7] = [
    "distance",
    "lateral",
    "height",
    "bank",
    "lag",
    "shake",
    "look_ahead",
];
const AMBER: Color32 = Color32::from_rgb(0xF2, 0xBF, 0x58);
const INK: Color32 = Color32::from_rgb(0x0E, 0x0E, 0x0E);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Top,
    Front,
    Side,
    Back,
    Orbit,
}

impl View {
    pub fn named(name: &str) -> Option<View> {
        match name {
            "top" => Some(View::Top),
            "front" => Some(View::Front),
            "side" => Some(View::Side),
            "back" => Some(View::Back),
            "orbit" => Some(View::Orbit),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Stage(String),
    View(View),
    Camera(String),
    Select(Vec<String>),
    Orbit(f64),
    Dolly(f64),
    Fit {
        objects: Vec<String>,
        before: f64,
        now: f64,
    },
    Reveal {
        object: String,
        goal: f64,
        before: f64,
        now: f64,
    },
    Region(Option<[f32; 4]>),
    Strip(bool),
}

pub fn toward(value: f64, goal: f64, before: f64, now: f64) -> f64 {
    let left = 1.0 - before;
    if left <= 1e-12 || now >= 1.0 {
        return goal;
    }
    value + (goal - value) * ((now - before) / left).clamp(0.0, 1.0)
}

#[derive(Clone, Debug, PartialEq)]
enum Beat {
    Step(Step),
    Wait(f32),
    Fade {
        to: f32,
        seconds: f32,
    },
    Xray(bool),
    Curve(Curve),
    Act(Act),
    Orbit {
        degrees: f64,
        seconds: f32,
    },
    Dolly {
        metres: f64,
        seconds: f32,
    },
    Fit {
        objects: Vec<String>,
        seconds: f32,
    },
    Reveal {
        object: String,
        to: f64,
        seconds: f32,
    },
    Shade {
        to: crate::clay::Shading,
        seconds: f32,
    },
    Seek(String),
    File(FileStep),
}

#[derive(Clone, Debug, PartialEq)]
struct Curve {
    camera: String,
    track: String,
    key: usize,
    at: [f64; 2],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tape {
    beats: Vec<Beat>,
    lines: Vec<usize>,
}

fn seconds(number: usize, text: &str) -> Result<f32, Error> {
    match text.parse::<f32>() {
        Ok(value) if value.is_finite() && (0.0..=LONGEST_WAIT).contains(&value) => Ok(value),
        _ => Err(Error(format!(
            "line {number}: wait wants seconds, from 0 to {LONGEST_WAIT}"
        ))),
    }
}

fn shading(number: usize, rest: &str) -> Result<Beat, Error> {
    let words: Vec<&str> = rest.split_whitespace().collect();
    let refused = |why: String| Error(format!("line {number}: {why}"));
    match words.as_slice() {
        ["xray", "on"] => Ok(Beat::Xray(true)),
        ["xray", "off"] => Ok(Beat::Xray(false)),
        ["xray", ..] => Err(refused("wireframe xray wants on or off".into())),
        [amount] => Ok(Beat::Fade {
            to: crate::wire::amount(amount).map_err(refused)?,
            seconds: 0.0,
        }),
        [amount, time] => Ok(Beat::Fade {
            to: crate::wire::amount(amount).map_err(refused)?,
            seconds: seconds(number, time).map_err(|_| {
                refused(format!(
                    "a wireframe fade wants seconds, from 0 to {LONGEST_WAIT}"
                ))
            })?,
        }),
        _ => Err(refused(
            "wireframe wants on, off, xray on|off, or an amount from 0 to 1 and seconds".into(),
        )),
    }
}

fn curve(number: usize, rest: &str) -> Result<Beat, Error> {
    let refused = |why: &str| Error(format!("line {number}: {why}"));
    let [camera, track, key, t, v] = rest.split_whitespace().collect::<Vec<_>>()[..] else {
        return Err(refused(
            "curve wants a camera, a track, a key index, a time and a value",
        ));
    };
    if !TRACKS.contains(&track) {
        return Err(refused(&format!(
            "{track} is not a rig track; the tracks are {}",
            TRACKS.join(", ")
        )));
    }
    let key = key
        .parse::<usize>()
        .map_err(|_| refused(&format!("{key} is not a key index: 0, 1, 2 and on")))?;
    let number_of = |text: &str| text.parse::<f64>().ok().filter(|value| value.is_finite());
    let (Some(t), Some(v)) = (number_of(t), number_of(v)) else {
        return Err(refused("curve wants a key's time and value as numbers"));
    };
    if t < 0.0 {
        return Err(refused("a key's time is 0 or more seconds"));
    }
    Ok(Beat::Curve(Curve {
        camera: camera.into(),
        track: track.into(),
        key,
        at: [t, v],
    }))
}

fn names(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

fn number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn directing(number_at: usize, command: &str, rest: &str) -> Result<Option<Beat>, Error> {
    let words: Vec<&str> = rest.split_whitespace().collect();
    let refused = |why: &str| Error(format!("line {number_at}: {command} wants {why}"));
    let time = |text: &str| {
        seconds(number_at, text).map_err(|_| {
            Error(format!(
                "line {number_at}: {command} wants seconds, from 0 to {LONGEST_WAIT}"
            ))
        })
    };
    let beat = match (command, words.as_slice()) {
        ("stage", [stage]) => Beat::Act(Act::Stage((*stage).to_string())),
        ("stage", _) => return Err(refused("a stage's number or name")),
        ("view", [name]) => match View::named(name) {
            Some(view) => Beat::Act(Act::View(view)),
            None => return Err(refused("top, front, side, back or orbit")),
        },
        ("view", _) => return Err(refused("top, front, side, back or orbit")),
        ("camera", [name]) => Beat::Act(Act::Camera((*name).to_string())),
        ("camera", _) => return Err(refused("a camera's name")),
        ("select", [_, ..]) if !names(rest).is_empty() => Beat::Act(Act::Select(names(rest))),
        ("select", _) => return Err(refused("objects, separated by commas")),
        ("orbit", [degrees, span]) => match number(degrees) {
            Some(degrees) => Beat::Orbit {
                degrees,
                seconds: time(span)?,
            },
            None => return Err(refused("degrees and seconds")),
        },
        ("orbit", _) => return Err(refused("degrees and seconds")),
        ("dolly", [metres, span]) => match number(metres) {
            Some(metres) => Beat::Dolly {
                metres,
                seconds: time(span)?,
            },
            None => return Err(refused("metres and seconds")),
        },
        ("dolly", _) => return Err(refused("metres and seconds")),
        ("frame", []) => return Ok(None),
        ("frame", [objects]) => Beat::Fit {
            objects: names(objects),
            seconds: 0.0,
        },
        ("frame", [objects, span]) => Beat::Fit {
            objects: names(objects),
            seconds: time(span)?,
        },
        ("frame", _) => return Err(refused("nothing, or an object and seconds")),
        ("reveal", [object, fraction, span]) => match number(fraction) {
            Some(to) if (0.0..=1.0).contains(&to) => Beat::Reveal {
                object: (*object).to_string(),
                to,
                seconds: time(span)?,
            },
            _ => return Err(refused("an object, a fraction from 0 to 1 and seconds")),
        },
        ("reveal", _) => return Err(refused("an object, a fraction from 0 to 1 and seconds")),
        ("region", ["off"]) => Beat::Act(Act::Region(None)),
        ("strip", ["on"]) => Beat::Act(Act::Strip(true)),
        ("strip", ["off"]) => Beat::Act(Act::Strip(false)),
        ("strip", _) => return Err(refused("on or off")),
        ("region", [x0, y0, x1, y1]) => {
            let corners: Vec<f32> = [x0, y0, x1, y1]
                .iter()
                .filter_map(|text| text.parse::<f32>().ok())
                .filter(|value| (0.0..=1.0).contains(value))
                .collect();
            match corners.as_slice() {
                [x0, y0, x1, y1] if x0 < x1 && y0 < y1 => {
                    Beat::Act(Act::Region(Some([*x0, *y0, *x1, *y1])))
                }
                _ => {
                    return Err(refused(
                        "off, or x0 y0 x1 y1 as fractions of the viewport from 0 to 1, x0 < x1 and y0 < y1",
                    ));
                }
            }
        }
        ("region", _) => {
            return Err(refused(
                "off, or x0 y0 x1 y1 as fractions of the viewport from 0 to 1, x0 < x1 and y0 < y1",
            ));
        }
        _ => return Ok(None),
    };
    Ok(Some(beat))
}

const DIRECTING: [&str; 10] = [
    "stage", "view", "camera", "select", "orbit", "dolly", "frame", "reveal", "region", "strip",
];

fn clay(number: usize, command: &str, rest: &str) -> Result<Beat, Error> {
    use crate::clay::Shading;
    let words: Vec<&str> = rest.split_whitespace().collect();
    let refused = |why: String| Error(format!("line {number}: {why}"));
    let shade = |mode: &str, seconds: f32| {
        Shading::parse(mode)
            .map(|to| Beat::Shade { to, seconds })
            .map_err(refused)
    };
    match (command, words.as_slice()) {
        ("clay", ["on"]) => Ok(Beat::Shade {
            to: Shading::Clay,
            seconds: 0.0,
        }),
        ("clay", ["off"]) => Ok(Beat::Shade {
            to: Shading::Shaded,
            seconds: 0.0,
        }),
        ("clay", _) => Err(refused("clay wants on or off".into())),
        (_, [mode]) => shade(mode, 0.0),
        (_, [mode, time]) => shade(
            mode,
            seconds(number, time).map_err(|_| {
                refused(format!(
                    "a shade fade wants seconds, from 0 to {LONGEST_WAIT}"
                ))
            })?,
        ),
        _ => Err(refused(
            "shade wants shaded, clay or wireframe, and seconds to fade over".into(),
        )),
    }
}

impl Tape {
    pub fn parse(text: &str) -> Result<Tape, Error> {
        let mut script = String::new();
        let mut order = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let number = index + 1;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                script.push('\n');
                continue;
            }
            let (command, rest) = trimmed
                .split_once(char::is_whitespace)
                .unwrap_or((trimmed, ""));
            if command == "wait" {
                order.push((number, Some(Beat::Wait(seconds(number, rest.trim())?))));
                script.push('\n');
                continue;
            }
            if command == "seek" {
                let to = rest.trim();
                if to.is_empty() || to.contains(char::is_whitespace) {
                    return Err(Error(format!(
                        "line {number}: seek wants a marker's name or seconds on the timeline"
                    )));
                }
                order.push((number, Some(Beat::Seek(to.to_string()))));
                script.push('\n');
                continue;
            }
            if command == "wireframe" {
                order.push((number, Some(shading(number, rest)?)));
                script.push('\n');
                continue;
            }
            if command == "curve" {
                order.push((number, Some(curve(number, rest)?)));
                script.push('\n');
                continue;
            }
            if DIRECTING.contains(&command)
                && let Some(beat) = directing(number, command, rest)?
            {
                order.push((number, Some(beat)));
                script.push('\n');
                continue;
            }
            if command == "clay" || command == "shade" {
                order.push((number, Some(clay(number, command, rest)?)));
                script.push('\n');
                continue;
            }
            if let Some(step) = crate::file::step(command, rest) {
                let step = step.map_err(|why| Error(format!("line {number}: {why}")))?;
                order.push((number, Some(Beat::File(step))));
                script.push('\n');
                continue;
            }
            if !VERBS.contains(&command) {
                return Err(Error(format!(
                    "line {number}: {command} is not a step; a tape's steps are key, text, click, move, press, release, drag, scroll, frame, wait, wireframe, curve, stage, view, camera, select, orbit, dolly, reveal, region, strip, clay, shade, seek, open, save, save-as, import, export and drop"
                )));
            }
            order.push((number, None));
            script.push_str(line);
            script.push('\n');
        }
        let mut steps = Script::parse(&script)?.steps.into_iter();
        let lines = order.iter().map(|(number, _)| *number).collect();
        let beats = order
            .into_iter()
            .map(|(_, beat)| match beat {
                Some(beat) => Ok(beat),
                None => steps
                    .next()
                    .map(Beat::Step)
                    .ok_or_else(|| Error("the tape lost a step".into())),
            })
            .collect::<Result<_, _>>()?;
        Ok(Tape { beats, lines })
    }

    pub fn line(&self, step: usize) -> Option<usize> {
        self.lines.get(step).copied()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub size: [u32; 2],
    pub scale: f32,
    pub window: Option<[u32; 2]>,
}

impl Layout {
    pub fn plain(size: [u32; 2]) -> Layout {
        Layout {
            size,
            scale: 1.0,
            window: None,
        }
    }

    pub fn points(&self) -> [f32; 2] {
        match self.window {
            Some(window) => window.map(|side| side as f32),
            None => self.size.map(|side| side as f32 / self.scale),
        }
    }

    pub fn pixels_per_point(&self) -> f32 {
        match self.window {
            Some(window) => self
                .scale
                .min(self.size[0] as f32 / window[0] as f32)
                .min(self.size[1] as f32 / window[1] as f32),
            None => self.scale,
        }
    }

    pub fn drawn(&self) -> [u32; 2] {
        let ppp = self.pixels_per_point();
        let points = self.points();
        [0, 1].map(|axis| ((points[axis] * ppp).round() as u32).clamp(1, self.size[axis]))
    }
}

fn centred(size: [u32; 2], drawn: [u32; 2], ground: Color32, rgba: Vec<u8>) -> Vec<u8> {
    if drawn == size {
        return rgba;
    }
    let mut frame = ground
        .to_array()
        .repeat(size[0] as usize * size[1] as usize);
    let left = ((size[0] - drawn[0]) / 2) as usize;
    let top = ((size[1] - drawn[1]) / 2) as usize;
    let row = drawn[0] as usize * 4;
    for (y, line) in rgba.chunks_exact(row).enumerate() {
        let start = ((top + y) * size[0] as usize + left) * 4;
        frame[start..start + row].copy_from_slice(line);
    }
    frame
}

#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|error| format!("{}: {error}", self.0.display()))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Frame {
    step: Option<usize>,
    events: Vec<Event>,
    modifiers: Modifiers,
    pointer: Option<Pos2>,
    down: bool,
    wire: Option<f32>,
    xray: Option<bool>,
    curve: Option<Curve>,
    acts: Vec<Act>,
    clay: Option<f32>,
    seek: Option<String>,
    file: Option<FileStep>,
    dropped: Option<(PathBuf, bool)>,
}

fn modifiers_of(mods: Mods) -> Modifiers {
    Modifiers {
        alt: mods.alt,
        ctrl: mods.ctrl,
        shift: mods.shift,
        mac_cmd: false,
        command: mods.ctrl,
    }
}

fn key(name: &str, pressed: bool, modifiers: Modifiers) -> Option<Event> {
    Some(Event::Key {
        key: Key::from_name(name)?,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    })
}

fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

struct Plan {
    fps: f32,
    home: Pos2,
    frames: Vec<Frame>,
    pointer: Option<Pos2>,
    down: Option<PointerButton>,
    held: Modifiers,
    step: Option<usize>,
    wire: f32,
    clay: f32,
}

impl Plan {
    fn full(&self) -> bool {
        self.frames.len() > MAX_FRAMES
    }

    fn push(&mut self, events: Vec<Event>, modifiers: Modifiers) {
        if self.full() {
            return;
        }
        self.frames.push(Frame {
            step: self.step,
            events,
            modifiers,
            pointer: self.pointer,
            down: self.down.is_some(),
            wire: None,
            xray: None,
            curve: None,
            acts: Vec::new(),
            clay: None,
            seek: None,
            file: None,
            dropped: None,
        });
    }

    fn act(&mut self, act: Act) {
        self.frame(Vec::new());
        if let Some(last) = self.frames.last_mut() {
            last.acts.push(act);
        }
    }

    fn timed(
        &mut self,
        seconds: f32,
        curve: fn(f32) -> f32,
        mut make: impl FnMut(f64, f64) -> Act,
    ) {
        let frames = self.count(seconds).clamp(1, MAX_FRAMES + 1);
        for step in 1..=frames {
            if self.full() {
                return;
            }
            let before = f64::from(curve((step - 1) as f32 / frames as f32));
            let now = f64::from(curve(step as f32 / frames as f32));
            self.act(make(before, now));
        }
    }

    fn file(&mut self, step: &FileStep) {
        self.frame(Vec::new());
        if let Some(last) = self.frames.last_mut() {
            match step {
                FileStep::Drop { path, shift } => last.dropped = Some((path.clone(), *shift)),
                step => last.file = Some(step.clone()),
            }
        }
    }

    fn shade(&mut self, wire: Option<f32>, xray: Option<bool>) {
        self.frame(Vec::new());
        if let Some(last) = self.frames.last_mut() {
            last.wire = wire;
            last.xray = xray;
        }
    }

    fn curve(&mut self, curve: &Curve) {
        self.frame(Vec::new());
        if let Some(last) = self.frames.last_mut() {
            last.curve = Some(curve.clone());
        }
    }

    fn mould(&mut self, clay: f32, wire: f32) {
        self.frame(Vec::new());
        if let Some(last) = self.frames.last_mut() {
            last.clay = Some(clay);
            last.wire = Some(wire);
        }
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.push(events, self.held);
    }

    fn count(&self, seconds: f32) -> usize {
        (seconds * self.fps).round() as usize
    }

    fn travel(&mut self, from: Pos2, to: Pos2, frames: usize) {
        for step in 1..=frames {
            let at = from.lerp(to, ease(step as f32 / frames as f32));
            self.pointer = Some(at);
            self.frame(vec![Event::PointerMoved(at)]);
        }
    }

    fn glide(&mut self, to: Pos2) {
        let from = self.pointer.unwrap_or(self.home);
        let distance = from.distance(to);
        if self.pointer.is_some() && distance < 0.5 {
            return;
        }
        let seconds = (distance / GLIDE_SPEED).clamp(GLIDE_SHORTEST, GLIDE_LONGEST);
        let frames = self.count(seconds).max(1);
        self.travel(from, to, frames);
    }

    fn button(&self, button: PointerButton, pressed: bool) -> Event {
        Event::PointerButton {
            pos: self.pointer.unwrap_or(self.home),
            button,
            pressed,
            modifiers: self.held,
        }
    }

    fn press(&mut self, button: PointerButton, mods: Mods) {
        self.held = modifiers_of(mods);
        let event = self.button(button, true);
        self.down = Some(button);
        self.frame(vec![event]);
    }

    fn release(&mut self) {
        let event = self.button(self.down.unwrap_or(PointerButton::Primary), false);
        self.down = None;
        self.frame(vec![event]);
        self.held = Modifiers::NONE;
    }

    fn step(&mut self, step: &Step) {
        match step {
            Step::Key(name, mods) => {
                let chord = self.held | modifiers_of(*mods);
                let events = [key(name, true, chord), key(name, false, chord)]
                    .into_iter()
                    .flatten()
                    .collect();
                self.push(events, chord);
            }
            Step::Text(text) => {
                let every = self.count(1.0 / TYPING).max(1);
                for typed in text.chars() {
                    if self.full() {
                        return;
                    }
                    self.frame(vec![Event::Text(typed.to_string())]);
                    for _ in 1..every {
                        self.frame(Vec::new());
                    }
                }
            }
            Step::Move([x, y]) => self.glide(Pos2::new(*x, *y)),
            Step::Click([x, y]) => {
                self.glide(Pos2::new(*x, *y));
                self.press(PointerButton::Primary, Mods::default());
                self.release();
            }
            Step::Press { at, mods } => {
                self.glide(Pos2::new(at[0], at[1]));
                self.press(PointerButton::Primary, *mods);
            }
            Step::Release([x, y]) => {
                self.glide(Pos2::new(*x, *y));
                self.release();
            }
            Step::Drag {
                from,
                to,
                middle,
                mods,
            } => {
                let (from, to) = (Pos2::new(from[0], from[1]), Pos2::new(to[0], to[1]));
                self.glide(from);
                let button = if *middle {
                    PointerButton::Middle
                } else {
                    PointerButton::Primary
                };
                self.press(button, *mods);
                let seconds = (from.distance(to) / DRAG_SPEED).clamp(DRAG_SHORTEST, DRAG_LONGEST);
                let frames = self.count(seconds).max(1);
                self.travel(from, to, frames);
                self.release();
            }
            Step::Scroll { at, lines } => {
                self.glide(Pos2::new(at[0], at[1]));
                self.frame(vec![Event::MouseWheel {
                    unit: MouseWheelUnit::Line,
                    delta: Vec2::new(0.0, *lines),
                    modifiers: self.held,
                    phase: egui::TouchPhase::Move,
                }]);
            }
            Step::Frame => self.frame(Vec::new()),
        }
    }
}

fn plan(tape: &Tape, fps: u32, points: [f32; 2]) -> Result<Vec<Frame>, Error> {
    plan_from(tape, fps, points, crate::wire::Look::default())
}

fn plan_from(
    tape: &Tape,
    fps: u32,
    points: [f32; 2],
    start: crate::wire::Look,
) -> Result<Vec<Frame>, Error> {
    if fps == 0 {
        return Err(Error("a clip has at least one frame a second".into()));
    }
    let mut plan = Plan {
        fps: fps as f32,
        home: Pos2::new(points[0] / 2.0, points[1] / 2.0),
        frames: vec![Frame::default()],
        pointer: None,
        down: None,
        held: Modifiers::NONE,
        step: None,
        wire: start.amount,
        clay: start.clay,
    };
    for (index, beat) in tape.beats.iter().enumerate() {
        plan.step = Some(index);
        match beat {
            Beat::Step(step) => plan.step(step),
            Beat::Wait(seconds) => {
                for _ in 0..plan.count(*seconds).min(MAX_FRAMES + 1) {
                    plan.frame(Vec::new());
                }
            }
            Beat::Fade { to, seconds } => {
                let frames = plan.count(*seconds).min(MAX_FRAMES + 1);
                for amount in crate::wire::fade(plan.wire, *to, frames) {
                    plan.shade(Some(amount), None);
                }
                plan.wire = *to;
            }
            Beat::Xray(on) => plan.shade(None, Some(*on)),
            Beat::Curve(curve) => plan.curve(curve),
            Beat::Act(act) => plan.act(act.clone()),
            Beat::Orbit { degrees, seconds } => {
                plan.timed(*seconds, ease, |before, now| {
                    Act::Orbit(degrees * (now - before))
                });
            }
            Beat::Dolly { metres, seconds } => {
                plan.timed(*seconds, ease, |before, now| {
                    Act::Dolly(metres * (now - before))
                });
            }
            Beat::Fit { objects, seconds } => plan.timed(*seconds, ease, |before, now| Act::Fit {
                objects: objects.clone(),
                before,
                now,
            }),
            Beat::Reveal {
                object,
                to,
                seconds,
            } => plan.timed(
                *seconds,
                |t| t,
                |before, now| Act::Reveal {
                    object: object.clone(),
                    goal: *to,
                    before,
                    now,
                },
            ),
            Beat::Shade { to, seconds } => {
                let frames = plan.count(*seconds).min(MAX_FRAMES + 1);
                for (clay, wire) in crate::clay::fade(plan.clay, plan.wire, *to, frames) {
                    plan.mould(clay, wire);
                }
                (plan.clay, plan.wire) = to.amounts();
            }
            Beat::Seek(to) => {
                plan.frame(Vec::new());
                if let Some(last) = plan.frames.last_mut() {
                    last.seek = Some(to.clone());
                }
            }
            Beat::File(step) => plan.file(step),
        }
        if plan.full() {
            return Err(Error(format!(
                "the tape runs past {MAX_FRAMES} frames at {fps} a second"
            )));
        }
    }
    Ok(plan.frames)
}

pub fn steps(tape: &Tape, fps: u32, points: [f32; 2]) -> Result<Vec<Option<usize>>, Error> {
    plan(tape, fps, points).map(|frames| frames.iter().map(|frame| frame.step).collect())
}

fn pointer(ctx: &Context, at: Pos2, down: bool) {
    let painter = ctx.layer_painter(LayerId::new(Order::Debug, Id::new("manfred tape pointer")));
    let point = |x: f32, y: f32| at + Vec2::new(x, y);
    if down {
        painter.circle_filled(at, 11.0, AMBER.gamma_multiply(0.35));
        painter.circle_stroke(at, 11.0, Stroke::new(1.5_f32, AMBER));
    }
    let tail = vec![
        point(4.04, 14.32),
        point(7.16, 13.02),
        point(9.85, 19.48),
        point(6.73, 20.78),
    ];
    let head = vec![point(0.0, 0.0), point(0.0, 16.0), point(11.3, 11.3)];
    let outline = vec![
        point(0.0, 0.0),
        point(0.0, 16.0),
        point(4.04, 14.32),
        point(6.73, 20.78),
        point(9.85, 19.48),
        point(7.16, 13.02),
        point(11.3, 11.3),
    ];
    painter.add(Shape::convex_polygon(tail, Color32::WHITE, Stroke::NONE));
    painter.add(Shape::convex_polygon(head, Color32::WHITE, Stroke::NONE));
    painter.add(Shape::closed_line(outline, Stroke::new(1.2_f32, INK)));
}

type Stamp = Option<(Option<SystemTime>, u64)>;

fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok(), meta.len()))
}

#[derive(Default)]
struct Watching {
    files: Vec<PathBuf>,
    stamps: Vec<Stamp>,
}

impl Watching {
    fn of(files: Vec<PathBuf>) -> Watching {
        let stamps = files.iter().map(|file| stamp(file)).collect();
        Watching { files, stamps }
    }
}

struct Player<'p, 'a> {
    app: &'p mut Manfred<'a>,
    ctx: Context,
    gpu: Gpu,
    painter: Painter,
    target: pfx_gpu::OffscreenTarget,
    theme: Theme,
    layout: Layout,
    fps: u32,
    textures: TexturesDelta,
    watching: Watching,
    turns: Turns,
    lines: Vec<usize>,
    refused: Option<String>,
}

impl Player<'_, '_> {
    fn pass(&mut self, frame: &Frame, time: f64, draw: bool) -> (FullOutput, bool) {
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                Vec2::from(self.layout.points()),
            )),
            max_texture_side: Some(MAX_SIDE as usize),
            time: Some(time),
            predicted_dt: 1.0 / self.fps as f32,
            focused: true,
            events: std::iter::once(Event::ModifiersChanged(frame.modifiers))
                .chain(frame.events.iter().cloned())
                .collect(),
            ..RawInput::default()
        };
        input.viewports.insert(
            ViewportId::ROOT,
            ViewportInfo {
                native_pixels_per_point: Some(self.layout.pixels_per_point()),
                focused: Some(true),
                ..ViewportInfo::default()
            },
        );
        if let Some((path, shift)) = &frame.dropped {
            input.dropped_files.push(Arc::new(Dropped(path.clone())));
            if *shift {
                input.events.push(Event::ModifiersChanged(Modifiers {
                    shift: true,
                    ..frame.modifiers
                }));
            }
        }
        let app = &mut *self.app;
        if let Some(step) = &frame.file {
            app.file(step);
        }
        if let Some(amount) = frame.wire {
            app.set_wireframe(amount);
        }
        if let Some(on) = frame.xray {
            app.set_xray(on);
        }
        for act in &frame.acts {
            if let Err(why) = app.act(act) {
                let line = frame.step.and_then(|step| self.lines.get(step).copied());
                self.refused.get_or_insert(match line {
                    Some(line) => format!("line {line}: {why}"),
                    None => why,
                });
            }
        }
        if let Some(amount) = frame.clay {
            app.set_clay(amount);
        }
        let mut output = self.ctx.run_ui(input, |ui| {
            app.ui(ui);
            if let Some(at) = frame.pointer {
                pointer(ui.ctx(), at, frame.down);
            }
        });
        self.textures
            .append(std::mem::take(&mut output.textures_delta));
        if self.app.opened() {
            self.watching = Watching::of(self.app.watched().files);
        }
        let more = draw && self.app.viewport() && {
            let Player {
                app,
                gpu,
                painter,
                turns,
                ..
            } = self;
            turns.time(|| app.frame(gpu, painter.renderer(), output.pixels_per_point))
        };
        self.watch();
        self.turns.poll();
        (output, more)
    }

    fn watch(&mut self) {
        let files = self.app.watched().files;
        if files != self.watching.files {
            self.watching = Watching::of(files);
            return;
        }
        let now: Vec<Stamp> = files.iter().map(|file| stamp(file)).collect();
        let changed: Vec<PathBuf> = files
            .iter()
            .zip(self.watching.stamps.iter().zip(&now))
            .filter(|(_, (then, now))| then != now)
            .map(|(file, _)| file.clone())
            .collect();
        if !changed.is_empty() {
            self.watching.stamps = now;
            self.app.changed(&changed);
        }
    }

    fn settle(
        &mut self,
        frame: &Frame,
        time: f64,
        first: (FullOutput, bool),
    ) -> Result<FullOutput, Error> {
        let quiet = Frame {
            events: Vec::new(),
            acts: Vec::new(),
            file: None,
            dropped: None,
            ..frame.clone()
        };
        let (mut last, mut more) = first;
        let started = Instant::now();
        let mut passes = 0;
        loop {
            let building = self.app.building();
            let working = building || self.app.rendering();
            if !working && (!more || passes >= SETTLE_PASSES) {
                return Ok(last);
            }
            if started.elapsed() > PATIENCE {
                return Err(Error(format!(
                    "the window was still building or rendering after {} minutes",
                    PATIENCE.as_secs() / 60
                )));
            }
            if !more {
                std::thread::sleep(POLL);
                self.turns.poll();
            }
            (last, more) = self.pass(&quiet, time, more || building);
            passes += 1;
        }
    }

    fn paint(&mut self, output: FullOutput) -> Result<Vec<u8>, Error> {
        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let screen = ScreenDescriptor {
            size_in_pixels: self.layout.drawn(),
            pixels_per_point: output.pixels_per_point,
        };
        let mut textures = std::mem::take(&mut self.textures);
        let Player {
            gpu,
            painter,
            target,
            theme,
            turns,
            layout,
            ..
        } = self;
        let rgba = turns.time(|| {
            painter.paint(gpu, &target.view, &screen, &textures, &primitives, theme.bg);
            gpu.readback_rgba8(target)
        });
        textures.clear();
        let ground = theme.bg;
        rgba.map(|rgba| centred(layout.size, layout.drawn(), ground, rgba))
            .map_err(Error)
    }
}

impl Drop for Player<'_, '_> {
    fn drop(&mut self) {
        self.textures.clear();
    }
}

pub fn play(
    app: &mut Manfred<'_>,
    tape: &Tape,
    layout: Layout,
    fps: u32,
    mut sink: impl FnMut(usize, &[u8]) -> Result<(), String>,
) -> Result<usize, Error> {
    let mut turns = Turns::default();
    turns.turn();
    check(layout.size)?;
    check(layout.drawn())?;
    let frames = plan_from(tape, fps, layout.points(), app.wireframe())?;
    let theme = app.theme();
    let gpu = block_on(Gpu::headless()).map_err(Error)?;
    turns.turn();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let painter = Painter::new(&gpu, format, RendererOptions::PREDICTABLE);
    let drawn = layout.drawn();
    let target = gpu.offscreen(drawn[0], drawn[1], format).map_err(Error)?;
    turns.turn();
    let ctx = context(&theme);
    app.steady();
    app.forget();
    app.waker(Arc::new(|| {}));
    let watching = Watching::of(app.watched().files);
    let mut player = Player {
        app,
        ctx,
        gpu,
        painter,
        target,
        theme,
        layout,
        fps,
        textures: TexturesDelta::default(),
        watching,
        turns,
        lines: tape.lines.clone(),
        refused: None,
    };
    for (index, frame) in frames.iter().enumerate() {
        let time = index as f64 / f64::from(fps);
        if let Some(curve) = &frame.curve {
            let line = frame.step.and_then(|step| tape.line(step)).unwrap_or(0);
            player
                .app
                .curve_key(&curve.camera, &curve.track, curve.key, curve.at)
                .map_err(|why| Error(format!("line {line}: {why}")))?;
        }
        if let Some(to) = &frame.seek {
            player.app.seek_to(to).map_err(|why| {
                let line = frame.step.and_then(|step| tape.line(step)).unwrap_or(0);
                Error(format!("line {line}: {why}"))
            })?;
        }
        let first = player.pass(frame, time, true);
        if let Some(why) = player.refused.take() {
            return Err(Error(why));
        }
        let output = player.settle(frame, time, first)?;
        let rgba = player.paint(output)?;
        sink(index, &rgba).map_err(Error)?;
    }
    Ok(frames.len())
}

pub fn poster_png(size: [u32; 2], rgba: &[u8]) -> Result<Vec<u8>, Error> {
    rgb_png(size, rgba, png::Compression::Balanced)
}

pub fn frame_png(size: [u32; 2], rgba: &[u8]) -> Result<Vec<u8>, Error> {
    rgb_png(size, rgba, png::Compression::Fast)
}

fn rgb_png(size: [u32; 2], rgba: &[u8], compression: png::Compression) -> Result<Vec<u8>, Error> {
    let rgb: Vec<u8> = rgba
        .chunks_exact(4)
        .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect();
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, size[0], size[1]);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(compression);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder
        .write_header()
        .map_err(|error| Error(error.to_string()))?;
    writer
        .write_image_data(&rgb)
        .map_err(|error| Error(error.to_string()))?;
    writer.finish().map_err(|error| Error(error.to_string()))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved(frames: &[Frame]) -> Vec<Pos2> {
        frames
            .iter()
            .flat_map(|frame| &frame.events)
            .filter_map(|event| match event {
                Event::PointerMoved(at) => Some(*at),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_tape_is_the_kits_steps_plus_wait_and_keeps_its_line_numbers() {
        let tape = Tape::parse("# open\nkey ctrl+o\nwait 0.5\n\ntext a b\nframe\n").unwrap();
        assert_eq!(
            tape.beats,
            vec![
                Beat::Step(Step::Key(
                    "o".into(),
                    Mods {
                        ctrl: true,
                        ..Mods::default()
                    }
                )),
                Beat::Wait(0.5),
                Beat::Step(Step::Text("a b".into())),
                Beat::Step(Step::Frame),
            ]
        );
        assert_eq!(
            Tape::parse("frame\nwait\n").unwrap_err().0,
            "line 2: wait wants seconds, from 0 to 600"
        );
        assert_eq!(
            Tape::parse("wait 1\n\nclick 1\n").unwrap_err().0,
            "line 3: wants two coordinates, x and y"
        );
        assert!(
            Tape::parse("hold 1\n")
                .unwrap_err()
                .0
                .starts_with("line 1: hold is not a step")
        );
    }

    #[test]
    fn the_plan_is_the_tape_in_frames_at_the_clips_rate() {
        let tape = Tape::parse("wait 1\nclick 100 100\nwait 0.5\ntext ab\nkey Enter\n").unwrap();
        let frames = plan(&tape, 30, [200.0, 100.0]).unwrap();
        let glide = (Pos2::new(100.0, 50.0).distance(Pos2::new(100.0, 100.0)) / GLIDE_SPEED)
            .clamp(GLIDE_SHORTEST, GLIDE_LONGEST);
        let glide = (glide * 30.0).round() as usize;
        assert_eq!(frames.len(), 1 + 30 + glide + 2 + 15 + 2 * 2 + 1);
        assert_eq!(moved(&frames).last(), Some(&Pos2::new(100.0, 100.0)));
        let pressed = 1 + 30 + glide;
        assert!(frames[pressed].down);
        assert!(!frames[pressed + 1].down);
        let second = Tape::parse("wait 1\n").unwrap();
        assert_eq!(plan(&second, 60, [200.0, 100.0]).unwrap().len(), 61);
    }

    #[test]
    fn a_seek_step_takes_one_frame_and_carries_its_marker_or_seconds() {
        let tape = Tape::parse("seek tip\nwait 0.1\nseek 1.5\n").unwrap();
        assert_eq!(
            tape.beats,
            vec![
                Beat::Seek("tip".into()),
                Beat::Wait(0.1),
                Beat::Seek("1.5".into())
            ]
        );
        let frames = plan(&tape, 30, [200.0, 100.0]).unwrap();
        let seeks: Vec<Option<&str>> = frames.iter().map(|f| f.seek.as_deref()).collect();
        assert_eq!(seeks, [None, Some("tip"), None, None, None, Some("1.5")]);
        for bad in ["seek\n", "seek a b\n"] {
            let error = Tape::parse(bad).unwrap_err().0;
            assert!(error.starts_with("line 1: seek wants"), "{error}");
        }
    }

    #[test]
    fn a_wireframe_step_switches_or_fades_the_shading_one_amount_a_frame() {
        let tape =
            Tape::parse("wireframe on\nwireframe xray on\nwireframe 0.5 0.1\nwireframe off\n")
                .unwrap();
        assert_eq!(
            tape.beats,
            vec![
                Beat::Fade {
                    to: 1.0,
                    seconds: 0.0
                },
                Beat::Xray(true),
                Beat::Fade {
                    to: 0.5,
                    seconds: 0.1
                },
                Beat::Fade {
                    to: 0.0,
                    seconds: 0.0
                },
            ]
        );
        let frames = plan(&tape, 30, [200.0, 100.0]).unwrap();
        let wires: Vec<Option<f32>> = frames.iter().map(|frame| frame.wire).collect();
        assert_eq!(
            wires,
            [
                None,
                Some(1.0),
                None,
                Some(1.0 - 0.5 / 3.0),
                Some(1.0 - 1.0 / 3.0),
                Some(0.5),
                Some(0.0)
            ]
        );
        assert_eq!(frames[2].xray, Some(true));
        let faded = plan_from(
            &Tape::parse("wireframe 1 1\n").unwrap(),
            60,
            [200.0, 100.0],
            crate::wire::Look {
                amount: 0.5,
                ..crate::wire::Look::default()
            },
        )
        .unwrap();
        assert_eq!(faded.len(), 61);
        assert_eq!(faded[1].wire, Some(0.5 + 0.5 / 60.0));
        assert_eq!(faded[60].wire, Some(1.0));
        assert_eq!(
            Tape::parse("frame\nwireframe 2\n").unwrap_err().0,
            "line 2: 2 is not a wireframe amount: on, off or a number from 0 to 1"
        );
        assert!(
            Tape::parse("wireframe xray maybe\n")
                .unwrap_err()
                .0
                .starts_with("line 1: wireframe xray wants")
        );
    }

    fn acts(frames: &[Frame]) -> Vec<Act> {
        frames.iter().flat_map(|frame| frame.acts.clone()).collect()
    }

    #[test]
    fn the_directing_steps_parse_and_timed_ones_spread_over_their_frames() {
        let tape = Tape::parse(
            "stage 2\nview top\ncamera chase\nselect hero, road.crosswalks\nregion 0.1 0.2 0.9 0.8\nregion off\nstrip off\nframe\nframe hero\n",
        )
        .unwrap();
        assert_eq!(
            tape.beats,
            vec![
                Beat::Act(Act::Stage("2".into())),
                Beat::Act(Act::View(View::Top)),
                Beat::Act(Act::Camera("chase".into())),
                Beat::Act(Act::Select(vec!["hero".into(), "road.crosswalks".into()])),
                Beat::Act(Act::Region(Some([0.1, 0.2, 0.9, 0.8]))),
                Beat::Act(Act::Region(None)),
                Beat::Act(Act::Strip(false)),
                Beat::Step(Step::Frame),
                Beat::Fit {
                    objects: vec!["hero".into()],
                    seconds: 0.0
                },
            ]
        );
        for (line, why) in [
            (
                "view up",
                "line 1: view wants top, front, side, back or orbit",
            ),
            ("orbit 90", "line 1: orbit wants degrees and seconds"),
            ("dolly x 1", "line 1: dolly wants metres and seconds"),
            (
                "reveal road 2 1",
                "line 1: reveal wants an object, a fraction from 0 to 1 and seconds",
            ),
            ("camera", "line 1: camera wants a camera's name"),
            ("strip maybe", "line 1: strip wants on or off"),
        ] {
            assert_eq!(Tape::parse(line).unwrap_err().0, why);
        }
        assert!(Tape::parse("region 0.5 0 0.2 1").is_err());
        let orbit = plan(&Tape::parse("orbit 90 0.1\n").unwrap(), 30, [200.0, 100.0]).unwrap();
        let turns: Vec<f64> = acts(&orbit)
            .into_iter()
            .map(|act| match act {
                Act::Orbit(degrees) => degrees,
                other => panic!("{other:?}"),
            })
            .collect();
        let eased = [ease(1.0 / 3.0), ease(2.0 / 3.0), 1.0].map(f64::from);
        let expected = [eased[0], eased[1] - eased[0], eased[2] - eased[1]].map(|step| 90.0 * step);
        assert_eq!(turns.len(), 3);
        for (turn, want) in turns.iter().zip(expected) {
            assert!((turn - want).abs() < 1e-9, "{turns:?}");
        }
        assert!((turns.iter().sum::<f64>() - 90.0).abs() < 1e-9);
        let dolly = plan(&Tape::parse("dolly -2 0\n").unwrap(), 30, [200.0, 100.0]).unwrap();
        assert_eq!(acts(&dolly), vec![Act::Dolly(-2.0)]);
        let reveal = plan(
            &Tape::parse("reveal road 1 0.1\n").unwrap(),
            40,
            [200.0, 100.0],
        )
        .unwrap();
        let mut value = 0.0;
        let mut values = Vec::new();
        for act in acts(&reveal) {
            let Act::Reveal {
                object,
                goal,
                before,
                now,
            } = act
            else {
                panic!("{act:?}");
            };
            assert_eq!(object, "road");
            value = toward(value, goal, before, now);
            values.push(value);
        }
        assert_eq!(values.len(), 4);
        for (got, want) in values.iter().zip([0.25, 0.5, 0.75, 1.0]) {
            assert!((got - want).abs() < 1e-9, "{values:?}");
        }
    }

    #[test]
    fn a_curve_step_moves_one_rig_key_on_one_frame_of_the_plan() {
        let tape = Tape::parse("wait 0.1\ncurve chase distance 2 3.5 -6.25\nwait 0.1\n").unwrap();
        let moved = Curve {
            camera: "chase".into(),
            track: "distance".into(),
            key: 2,
            at: [3.5, -6.25],
        };
        assert_eq!(tape.beats[1], Beat::Curve(moved.clone()));
        assert_eq!(tape.line(1), Some(2));
        let frames = plan(&tape, 60, [200.0, 100.0]).unwrap();
        let curved: Vec<usize> = (0..frames.len())
            .filter(|&index| frames[index].curve.is_some())
            .collect();
        assert_eq!(curved, [7]);
        assert_eq!(frames[7].curve, Some(moved));
        assert_eq!(frames[7].step, Some(1));
        assert_eq!(frames.len(), 1 + 6 + 1 + 6);
        assert!(
            Tape::parse("frame\ncurve chase roll 0 1 2\n")
                .unwrap_err()
                .0
                .starts_with("line 2: roll is not a rig track")
        );
        assert!(Tape::parse("curve chase lag one 1 2\n").is_err());
        assert!(Tape::parse("curve chase lag 0 -1 2\n").is_err());
        assert!(Tape::parse("curve chase lag 0 1\n").is_err());
    }

    #[test]
    fn clay_and_shade_steps_fade_the_clay_and_the_wireframe_together() {
        use crate::clay::Shading;
        let tape = Tape::parse("clay on\nshade wireframe 0.1\nshade shaded\nclay off\n").unwrap();
        assert_eq!(
            tape.beats,
            vec![
                Beat::Shade {
                    to: Shading::Clay,
                    seconds: 0.0
                },
                Beat::Shade {
                    to: Shading::Wireframe,
                    seconds: 0.1
                },
                Beat::Shade {
                    to: Shading::Shaded,
                    seconds: 0.0
                },
                Beat::Shade {
                    to: Shading::Shaded,
                    seconds: 0.0
                },
            ]
        );
        let frames = plan(&tape, 30, [200.0, 100.0]).unwrap();
        let shades: Vec<Option<(f32, f32)>> = frames
            .iter()
            .map(|frame| frame.clay.zip(frame.wire))
            .collect();
        assert_eq!(
            shades,
            [
                None,
                Some((1.0, 0.0)),
                Some((1.0, 1.0 / 3.0)),
                Some((1.0, 2.0 / 3.0)),
                Some((0.0, 1.0)),
                Some((0.0, 0.0)),
                Some((0.0, 0.0)),
            ]
        );
        let faded = plan_from(
            &Tape::parse("shade clay 1\n").unwrap(),
            60,
            [200.0, 100.0],
            crate::wire::Look::default(),
        )
        .unwrap();
        assert_eq!(faded.len(), 61);
        assert_eq!(faded[30].clay, Some(0.5));
        assert_eq!(faded[60].clay, Some(1.0));
        assert_eq!(
            Tape::parse("shade solid\n").unwrap_err().0,
            "line 1: solid is not a shading mode: shaded, clay or wireframe"
        );
        assert_eq!(
            Tape::parse("frame\nclay maybe\n").unwrap_err().0,
            "line 2: clay wants on or off"
        );
        assert!(
            Tape::parse("shade clay forever\n")
                .unwrap_err()
                .0
                .starts_with("line 1: a shade fade wants seconds")
        );
    }

    #[test]
    fn the_file_steps_are_tape_beats_one_frame_each() {
        let tape = Tape::parse(
            "open a.manf\nsave\nsave-as b.manf\nimport car.glb\nexport out.glb --selected\ndrop car.glb shift\n",
        )
        .unwrap();
        assert_eq!(
            tape.beats,
            vec![
                Beat::File(FileStep::Open("a.manf".into())),
                Beat::File(FileStep::Save),
                Beat::File(FileStep::SaveAs("b.manf".into())),
                Beat::File(FileStep::Import("car.glb".into())),
                Beat::File(FileStep::Export {
                    path: "out.glb".into(),
                    selected: true
                }),
                Beat::File(FileStep::Drop {
                    path: "car.glb".into(),
                    shift: true
                }),
            ]
        );
        let frames = plan(&tape, 30, [200.0, 100.0]).unwrap();
        assert_eq!(frames.len(), 7);
        assert_eq!(frames[2].file, Some(FileStep::Save));
        assert_eq!(frames[6].file, None);
        assert_eq!(frames[6].dropped, Some(("car.glb".into(), true)));
        assert_eq!(
            Tape::parse("frame\nsave-as\n").unwrap_err().0,
            "line 2: save-as wants a path"
        );
    }

    #[test]
    fn a_drag_holds_the_button_and_its_modifiers_until_it_lets_go() {
        let tape = Tape::parse("drag 10 10 310 10 shift\nmove 10 10\n").unwrap();
        let frames = plan(&tape, 30, [400.0, 200.0]).unwrap();
        let held: Vec<&Frame> = frames.iter().filter(|frame| frame.down).collect();
        assert!(held.len() > 2);
        assert!(held.iter().all(|frame| frame.modifiers.shift));
        assert_eq!(frames.last().unwrap().modifiers, Modifiers::NONE);
        assert!(!frames.last().unwrap().down);
    }

    #[test]
    fn each_frame_knows_its_tape_step_and_a_framed_window_fits_the_frame() {
        let tape = Tape::parse("# wait\nwait 0.1\n\nframe\n").unwrap();
        assert_eq!(
            steps(&tape, 30, [200.0, 100.0]).unwrap(),
            [None, Some(0), Some(0), Some(0), Some(1)]
        );
        assert_eq!(
            (tape.line(0), tape.line(1), tape.line(2)),
            (Some(2), Some(4), None)
        );
        let tall = Layout {
            size: [2160, 3840],
            scale: 2.0,
            window: None,
        };
        assert_eq!(
            (tall.points(), tall.drawn()),
            ([1080.0, 1920.0], [2160, 3840])
        );
        let framed = Layout {
            window: Some([1586, 992]),
            ..tall
        };
        assert_eq!(framed.points(), [1586.0, 992.0]);
        assert_eq!(framed.drawn(), [2160, 1351]);
        let ground = Color32::from_rgb(1, 2, 3);
        let frame = centred([4, 3], [2, 1], ground, vec![9; 8]);
        assert_eq!(&frame[..4], &[1, 2, 3, 255]);
        assert_eq!(&frame[20..28], &[9; 8]);
        assert_eq!(&frame[28..32], &[1, 2, 3, 255]);
    }
}
