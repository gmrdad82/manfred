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
const AMBER: Color32 = Color32::from_rgb(0xF2, 0xBF, 0x58);
const INK: Color32 = Color32::from_rgb(0x0E, 0x0E, 0x0E);

#[derive(Clone, Debug, PartialEq)]
enum Beat {
    Step(Step),
    Wait(f32),
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
                order.push((number, Some(seconds(number, rest.trim())?)));
                script.push('\n');
                continue;
            }
            if !VERBS.contains(&command) {
                return Err(Error(format!(
                    "line {number}: {command} is not a step; a tape's steps are key, text, click, move, press, release, drag, scroll, frame and wait"
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
            .map(|(_, wait)| match wait {
                Some(seconds) => Ok(Beat::Wait(seconds)),
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

#[derive(Clone, Debug, Default, PartialEq)]
struct Frame {
    step: Option<usize>,
    events: Vec<Event>,
    modifiers: Modifiers,
    pointer: Option<Pos2>,
    down: bool,
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
        });
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
        let app = &mut *self.app;
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
    let frames = plan(tape, fps, layout.points())?;
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
    };
    for (index, frame) in frames.iter().enumerate() {
        let time = index as f64 / f64::from(fps);
        let first = player.pass(frame, time, true);
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
