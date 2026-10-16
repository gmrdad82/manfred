use std::fmt;

use egui::epaint::RectShape;
use egui::{
    Align, Align2, Button, CentralPanel, Color32, ComboBox, Context, CornerRadius, Event, Frame,
    Id, Key, Layout, Margin, Painter, Pos2, Rect, Response, RichText, Sense, Shape, Slider, Stroke,
    StrokeKind, TextEdit, TextureHandle, TextureId, TextureOptions, Ui, UiBuilder, Vec2,
};
use manfred_build::Registry;
use manfred_scene::Eye;

use crate::cameras;
use crate::document::{Document, Lamp};
use crate::edit::{
    self, Dial, Editor, Fields, Join, Lit, LitFields, Outcome, Said, Slots, Smoothing, Triple,
    rename, set_lit, set_material, set_smoothing, set_triple,
};
use crate::fixture::{self, Fixture};
use crate::follow::{self, Seen};
use crate::gizmo::{Gizmo, Matrix, Mode, Poses};
use crate::orbit;
use crate::panel::Panel;
use crate::pick::{self, Lens};
use crate::sample;
use crate::shots::{Phase, Picture, Shots};
use crate::spans;
use crate::theme::*;
use crate::timeline::{Clock, Span};
use crate::viewport;
use crate::widgets::{Dotted, Header, KeyCap, Number, Pill, Tabs, Tone};
use pfx_editor_shell::Pgpu;
use pfx_editor_shell::focus::{Keys, Press};

pub const TABS: [&str; 4] = ["Scene", "Cameras", "Lights", "Render"];
const TAB_PANELS: [Panel; 4] = [Panel::B1, Panel::B2, Panel::B3, Panel::C3];
const VERSION: &str = env!("CARGO_PKG_VERSION");
const GPU_TIME: &str = "GPU time of the last frame, from the engine's timestamp queries; the window's own CPU work is not in it";
const GAP: f32 = 6.0;
const TOP: f32 = 40.0;
const HINTS: f32 = 26.0;
const STATUS: f32 = 24.0;
const LEFT: f32 = 300.0;
const RIGHT: f32 = 320.0;
const CAMERAS_HEIGHT: f32 = 150.0;
const LIGHTS_HEIGHT: f32 = 150.0;
const RENDER_HEIGHT: f32 = 300.0;
const LABEL: f32 = 78.0;
const BUTTONS: f32 = 22.0;
const THUMB: f32 = 100.0;

const CARET_OPEN: &str = "\u{f0d7}";
const CARET_SHUT: &str = "\u{f0da}";
const CUBE: &str = "\u{f1b2}";
const FOLDER: &str = "\u{f07b}";
const ROUTE: &str = "\u{f018}";
const CAMERA: &str = "\u{f030}";
const SUN: &str = "\u{f185}";
const CLOUD: &str = "\u{f0c2}";
const BULB: &str = "\u{f0eb}";
const SEARCH: &str = "\u{f002}";
const PLAY: &str = "\u{f04b}";
const PAUSE: &str = "\u{f04c}";
const STOP: &str = "\u{f04d}";

mod bare;
mod transport;

pub fn status_version() -> String {
    format!("manfred v{VERSION}")
}

fn search_id() -> Id {
    Id::new("manfred search")
}

fn header_id(panel: Panel) -> Id {
    Id::new(("manfred header", panel.code()))
}

fn tab_of(panel: Panel) -> Option<usize> {
    match panel {
        Panel::C2 => Some(0),
        _ => TAB_PANELS.iter().position(|tab| *tab == panel),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Change {
    Name(String),
    Triple(Triple, usize, f32),
    Material(String),
    Smoothing(Smoothing),
    Lit(Dial),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Dials {
    position: [f32; 3],
    color: [f32; 3],
    strength: f32,
    range: f32,
    rotation: f32,
    shadow: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Undo,
    Redo,
}

struct ShotViews {
    thumb: TextureHandle,
    full: TextureHandle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Grab {
    eye: Eye,
    origin: Pos2,
    pan: bool,
}

pub struct Shell {
    pub(crate) fixture: Option<Fixture>,
    pub(crate) chosen: Option<usize>,
    pub(crate) values: [f32; 3],
    focus: Panel,
    keys: Keys,
    search: String,
    document: Document,
    open: Vec<bool>,
    object: Option<usize>,
    camera: usize,
    light: usize,
    lit_shown: bool,
    lit: Option<LitFields>,
    dials: Dials,
    hour: f32,
    hour_set: bool,
    name: String,
    location: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    material: String,
    fields: Option<Fields>,
    editor: Option<Editor>,
    scene: bool,
    drains: bool,
    smoothing: usize,
    angle: f32,
    render_camera: usize,
    render_chosen: bool,
    size: [f32; 2],
    shots: Shots,
    shot_views: Option<ShotViews>,
    full: bool,
    settle: Option<(String, Eye)>,
    confirm: Option<String>,
    note: Option<&'static str>,
    viewport: Option<Rect>,
    panels: Vec<(Panel, Rect)>,
    reveal: bool,
    eye: Option<Eye>,
    through: bool,
    grab: Option<Grab>,
    view: Option<TextureId>,
    texture: Option<TextureHandle>,
    view_error: Option<String>,
    preparing: Option<String>,
    picker: crate::open::Picker,
    calm: bool,
    first: Option<&'static str>,
    solids: Vec<viewport::Solid>,
    book: crate::open::Book,
    opened: Option<String>,
    registry: Registry,
    trouble: Option<String>,
    busy: bool,
    pgpu: Pgpu,
    frame_ms: Option<f32>,
    timing_drops: u64,
    edits: crate::edit::Edits,
    gizmo: Gizmo,
    routes: crate::routes::Routes,
    pick: Option<[f32; 2]>,
    spans: crate::spans::Spans,
    clock: Clock,
    chase_eye: Option<Eye>,
    scrubber: Option<Rect>,
    bare: bool,
    wire: crate::wire::Look,
}

impl fmt::Debug for Shell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shell")
            .field("fixture", &self.fixture)
            .field("focus", &self.focus)
            .field("recipe", &self.document.path)
            .field(
                "object",
                &self
                    .object
                    .map(|object| &self.document.objects[object].name),
            )
            .field("eye", &self.eye)
            .field("viewport", &self.viewport)
            .finish_non_exhaustive()
    }
}

impl Default for Shell {
    fn default() -> Shell {
        Shell::new()
    }
}

fn default_eye() -> Eye {
    Eye {
        position: [0.0, 0.6, 2.4],
        target: [0.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        fov_y_deg: 40.0,
        shift: [0.0, 0.0],
        ortho_height: None,
    }
}

pub(crate) fn code(ui: &Ui, text: impl Into<String>, colour: Color32) -> RichText {
    RichText::new(text)
        .font(Theme::of(ui.ctx()).font(SIZE_CODE))
        .color(colour)
}

pub(crate) fn small(ui: &Ui, text: impl Into<String>, colour: Color32) -> RichText {
    RichText::new(text)
        .font(Theme::of(ui.ctx()).font(SIZE_SMALL))
        .color(colour)
}

const WHOLE: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));

fn bar(
    panel: egui::Panel,
    height: f32,
    fill: Color32,
    ui: &mut Ui,
    body: impl FnOnce(&mut Ui),
) -> Rect {
    panel
        .exact_size(height)
        .show_separator_line(false)
        .resizable(false)
        .frame(
            Frame::NONE
                .fill(fill)
                .inner_margin(Margin::symmetric(10, 0)),
        )
        .show(ui, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), body);
        })
        .response
        .rect
}

fn side(panel: egui::Panel, width: f32, ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    panel
        .exact_size(width)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::NONE
                .fill(BG_BASE)
                .inner_margin(Margin::same(GAP as i8)),
        )
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = GAP;
            body(ui);
        });
}

fn notice(painter: &Painter, image: Rect, title: &str, colour: Color32, body: &str) {
    let inner = image.shrink(SECTION_GAP * 2.0);
    let title = painter.layout_no_wrap(
        title.to_string(),
        Theme::of(painter.ctx()).bold_font(SIZE_BODY),
        colour,
    );
    let body = painter.layout(
        body.to_string(),
        Theme::of(painter.ctx()).font(SIZE_CODE),
        TEXT_CODE,
        inner.width().max(1.0),
    );
    let tall = title.size().y + ITEM_SPACING + body.size().y;
    let top = (image.center().y - tall / 2.0).round().max(inner.top());
    let title_height = title.size().y;
    painter.galley(Pos2::new(inner.left(), top), title, colour);
    painter.galley(
        Pos2::new(inner.left(), top + title_height + ITEM_SPACING),
        body,
        TEXT_CODE,
    );
}

fn band(painter: &Painter, min: Pos2, text: String, width: Option<f32>) {
    let galley = painter.layout_no_wrap(text, Theme::of(painter.ctx()).font(SIZE_SMALL), ACCENT);
    let band = Rect::from_min_size(
        min,
        Vec2::new(width.unwrap_or(galley.size().x + 16.0), 24.0),
    );
    painter.rect_filled(band, CornerRadius::same(RADIUS), BG_DEEP);
    painter.galley(
        band.left_center() + Vec2::new(8.0, -galley.size().y / 2.0),
        galley,
        ACCENT,
    );
}

fn remember_in(book: &crate::open::Book, path: &str) {
    let path = std::path::Path::new(path);
    if path.is_file() {
        let _ = book.remember(path);
    }
}

fn first_line(text: &str, most: usize) -> String {
    let line = text.lines().next().unwrap_or("");
    if line.chars().count() <= most {
        return line.to_string();
    }
    let cut: String = line.chars().take(most.saturating_sub(1)).collect();
    format!("{cut}…")
}

impl Shell {
    pub fn new() -> Shell {
        let mut shell = Shell::with(Document::sample());
        if let Some(knob) = shell.document.object("mv_knob") {
            shell.select(knob);
            if let Some(node) = shell.document.node_of(knob) {
                shell.open[node] = true;
            }
        }
        shell.location = [0.412, -0.081, -0.006];
        shell
    }

    pub fn open(path: &str) -> Shell {
        if crate::watched::is_scene(std::path::Path::new(path)) {
            return Shell::scene(path);
        }
        Shell {
            editor: Some(Editor::new(path)),
            drains: !cfg!(test),
            ..Shell::with(Document::empty(path))
        }
    }

    pub fn start(path: &str) -> Shell {
        Shell {
            calm: true,
            first: Some(crate::start::MARK),
            ..Shell::open(path)
        }
    }

    pub fn empty() -> Shell {
        Shell::with(Document::empty(""))
    }

    pub fn scene(path: &str) -> Shell {
        Shell {
            scene: true,
            ..Shell::with(Document::empty(path))
        }
    }

    pub fn is_scene(&self) -> bool {
        self.scene
    }

    pub fn browse(folder: &str) -> Shell {
        let mut shell = Shell::with(Document::empty(folder));
        shell.picker = crate::open::Picker::on_folder(folder);
        shell
    }

    fn with(document: Document) -> Shell {
        let open = vec![false; document.nodes.len()];
        let hour = document.hour.unwrap_or(12.0);
        let book = if cfg!(test) {
            crate::open::Book::empty()
        } else {
            crate::open::Book::machine()
        };
        Shell {
            wire: book.view(),
            fixture: None,
            chosen: None,
            values: [0.25, 0.25, 0.25],
            focus: Panel::default(),
            keys: Keys::default(),
            search: String::new(),
            document,
            open,
            object: None,
            camera: 0,
            light: 0,
            lit_shown: false,
            lit: None,
            dials: Dials::default(),
            hour,
            hour_set: false,
            name: String::new(),
            location: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
            material: String::new(),
            fields: None,
            editor: None,
            scene: false,
            drains: false,
            smoothing: 2,
            angle: 30.0,
            render_camera: 0,
            render_chosen: false,
            size: [1586.0, 992.0],
            shots: Shots::new(),
            shot_views: None,
            full: false,
            settle: None,
            confirm: None,
            note: None,
            viewport: None,
            panels: Vec::new(),
            reveal: false,
            eye: None,
            through: false,
            grab: None,
            view: None,
            texture: None,
            view_error: None,
            preparing: None,
            picker: crate::open::Picker::default(),
            calm: false,
            first: None,
            solids: Vec::new(),
            book,
            opened: None,
            registry: Registry::new(),
            trouble: None,
            busy: false,
            pgpu: Pgpu::Free,
            frame_ms: None,
            timing_drops: 0,
            edits: crate::edit::Edits::default(),
            gizmo: Gizmo::default(),
            routes: crate::routes::Routes::default(),
            pick: None,
            spans: crate::spans::Spans::default(),
            clock: Clock::default(),
            chase_eye: None,
            scrubber: None,
            bare: false,
        }
    }

    pub fn with_installed_showcase(mut self) -> Shell {
        self.picker
            .set_showcase_fallback(crate::open::installed_showcase());
        self
    }

    pub fn remember_recent(path: &str) {
        remember_in(&crate::open::Book::machine(), path);
    }

    pub fn without_recent(mut self) -> Shell {
        self.book = crate::open::Book::empty();
        self.wire = crate::wire::Look::default();
        self
    }

    pub fn wireframe(&self) -> crate::wire::Look {
        self.wire
    }

    pub fn set_wireframe(&mut self, amount: f32) {
        self.wire.amount = amount.clamp(0.0, 1.0);
    }

    pub fn set_xray(&mut self, on: bool) {
        self.wire.xray = on;
    }

    fn toggle_wireframe(&mut self) {
        let amount = if self.wire.on() { 0.0 } else { 1.0 };
        self.set_wireframe(amount);
        let _ = self.book.keep_view(self.wire);
    }

    fn toggle_xray(&mut self) {
        self.wire.xray = !self.wire.xray;
        let _ = self.book.keep_view(self.wire);
    }

    pub fn without_showcase(mut self) -> Shell {
        self.picker.without_showcase();
        self
    }

    pub fn fixture(fixture: Fixture) -> Shell {
        Shell {
            fixture: Some(fixture),
            ..Shell::new()
        }
    }

    fn live(&self) -> bool {
        self.editor.is_some() || self.scene
    }

    pub fn focus(&self) -> Panel {
        self.focus
    }

    pub fn edits(&mut self) -> &mut crate::edit::Edits {
        &mut self.edits
    }

    pub fn gizmo(&self) -> &Gizmo {
        &self.gizmo
    }

    pub fn set_poses(&mut self, poses: Poses) {
        self.gizmo.set_poses(poses);
    }

    pub fn set_solids(&mut self, solids: Vec<viewport::Solid>) {
        self.solids = solids;
    }

    fn solids_shown(&self) -> std::borrow::Cow<'_, [viewport::Solid]> {
        let Some((object, motion)) = self.gizmo.preview() else {
            return std::borrow::Cow::Borrowed(&self.solids);
        };
        let motion = glam::DMat4::from_cols_array_2d(&motion.map(|column| column.map(f64::from)));
        std::borrow::Cow::Owned(
            self.solids
                .iter()
                .map(|solid| {
                    if solid.objects.contains(&object) {
                        solid.moved(&motion)
                    } else {
                        solid.clone()
                    }
                })
                .collect(),
        )
    }

    pub fn preview(&self) -> Option<(usize, Matrix)> {
        self.gizmo.preview()
    }

    pub fn drop_preview(&mut self) {
        self.gizmo.drop_held();
    }

    pub fn take_pick(&mut self) -> Option<[f32; 2]> {
        self.pick.take()
    }

    pub fn picked(&mut self, object: Option<usize>) {
        match object.filter(|object| *object < self.document.objects.len()) {
            Some(object) => {
                self.select(object);
                self.lit_shown = false;
                self.reveal = true;
            }
            None => self.object = None,
        }
    }

    pub fn selected(&self) -> Option<usize> {
        self.object
    }

    pub fn viewport(&self) -> Option<Rect> {
        self.viewport
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn eye(&self) -> Option<Eye> {
        self.eye
    }

    pub fn through(&self) -> Option<&manfred_recipe::Camera> {
        self.document
            .views
            .get(self.camera)
            .filter(|_| self.through && self.live())
    }

    fn render_aspect(&self) -> f32 {
        self.size[0].max(1.0) / self.size[1].max(1.0)
    }

    pub fn span(&self) -> Option<Span> {
        self.document.timeline.filter(|_| self.live())
    }

    pub fn time(&self) -> Option<f64> {
        self.span().map(|_| self.clock.time)
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    pub fn chasing(&self) -> bool {
        self.clock.chase && self.span().is_some()
    }

    pub fn chase_eye(&self) -> Option<Eye> {
        self.chase_eye
    }

    pub fn set_chase_eye(&mut self, eye: Option<Eye>) {
        self.chase_eye = eye;
    }

    fn chase_view(&self) -> Option<(Eye, Rect)> {
        let image = self.viewport?;
        let span = self.span().filter(|_| self.clock.chase)?;
        let eye = self.chase_eye?;
        Some((
            eye,
            viewport::fit(image.shrink(viewport::MARGIN), span.aspect),
        ))
    }

    pub fn camera_frame(&self) -> Option<Rect> {
        if let Some((_, frame)) = self.chase_view() {
            return Some(frame);
        }
        let image = self.viewport?;
        self.through()?;
        Some(viewport::fit(
            image.shrink(viewport::MARGIN),
            self.render_aspect(),
        ))
    }

    fn free_frame(&self, image: Rect) -> Rect {
        viewport::fit(image, self.document.aspect)
    }

    pub fn view_eye(&self) -> Option<Eye> {
        let image = self.viewport?;
        if let Some((eye, frame)) = self.chase_view() {
            return Some(viewport::widen(&eye, frame, image));
        }
        if let (Some(camera), Some(frame)) = (self.through(), self.camera_frame()) {
            let eye = Eye::from_camera(camera, f64::from(self.render_aspect()));
            return Some(viewport::widen(&eye, frame, image));
        }
        let eye = self.eye?;
        Some(viewport::widen(&eye, self.free_frame(image), image))
    }

    pub fn lens(&self) -> Option<Lens> {
        Some(Lens::new(&self.view_eye()?, self.viewport?))
    }

    pub fn shown_camera(&self) -> Option<&manfred_recipe::Camera> {
        self.document.views.get(self.camera)
    }

    pub fn hour(&self) -> Option<f64> {
        self.hour_set.then_some(f64::from(self.hour))
    }

    pub fn paused(&self) -> bool {
        self.pgpu.paused()
    }

    pub fn pgpu(&self) -> Pgpu {
        self.pgpu
    }

    pub fn trouble(&self) -> Option<&str> {
        self.trouble.as_deref()
    }

    pub fn view_error(&self) -> Option<&str> {
        self.view_error.as_deref()
    }

    pub fn set_pgpu(&mut self, pgpu: Pgpu) {
        self.pgpu = pgpu;
    }

    pub fn set_frame_ms(&mut self, ms: Option<f32>) {
        self.frame_ms = ms;
    }

    pub fn set_timing_drops(&mut self, drops: u64) {
        self.timing_drops = drops;
    }

    pub fn set_busy(&mut self, busy: bool) {
        self.busy = busy;
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    pub fn rendering(&self) -> bool {
        self.shots.busy()
    }

    pub fn set_trouble(&mut self, trouble: Option<String>) {
        self.trouble = trouble;
    }

    pub fn preparing(&self) -> Option<&str> {
        self.preparing.as_deref()
    }

    pub fn set_preparing(&mut self, preparing: Option<String>) {
        self.preparing = preparing;
    }

    pub fn set_view(&mut self, texture: Option<TextureId>, error: Option<String>) {
        self.view = texture;
        self.view_error = error;
    }

    pub fn editor(&self) -> Option<&Editor> {
        self.editor.as_ref()
    }

    pub fn show(&mut self, document: Document) {
        let document = match self.editor.as_mut() {
            Some(editor) => {
                let seen = editor.seen(document.source);
                match (seen, editor.loaded()) {
                    (Seen::Current, _) | (_, None) => document,
                    (_, Some(loaded)) if follow::hash(&loaded.recipe.source) == document.source => {
                        document
                    }
                    (_, Some(loaded)) => Document::from_recipe(&loaded.recipe, &document.path),
                }
            }
            None => document,
        };
        self.place(document, &[]);
    }

    fn place(&mut self, document: Document, renamed: &[(String, String)]) {
        let recipe = self.editor.as_ref().and_then(Editor::loaded);
        self.routes.set(recipe.map(|loaded| &loaded.recipe));
        let follow_rename = |name: String| {
            renamed.iter().fold(
                name,
                |name, (old, new)| if name == *old { new.clone() } else { name },
            )
        };
        let chosen = self
            .object
            .map(|object| follow_rename(self.document.objects[object].name.clone()));
        let opened: Vec<String> = self
            .document
            .nodes
            .iter()
            .zip(&self.open)
            .filter(|(_, open)| **open)
            .filter_map(|(node, _)| {
                node.object
                    .map(|object| follow_rename(self.document.objects[object].name.clone()))
            })
            .collect();
        let camera = self
            .document
            .cameras
            .get(self.camera)
            .map(|(name, _)| name.clone());
        let render_camera = self
            .document
            .cameras
            .get(self.render_camera)
            .map(|(name, _)| name.clone());
        let lamp = self
            .document
            .lights
            .get(self.light)
            .map(|(kind, name, _)| (*kind, name.clone()));
        let first = self.document.nodes.is_empty();
        if document.hour != self.document.hour {
            self.hour_set = false;
        }
        self.document = document;
        self.clock.fit(self.span());
        self.open = self
            .document
            .nodes
            .iter()
            .map(|node| {
                first
                    || node
                        .object
                        .is_some_and(|object| opened.contains(&self.document.objects[object].name))
            })
            .collect();
        self.object = None;
        let first = self.first.and_then(|name| self.document.object(name));
        if first.is_some() {
            self.first = None;
        }
        let again = chosen
            .and_then(|name| self.document.object(&name))
            .or(first)
            .or_else(|| (!self.calm && !self.document.objects.is_empty()).then_some(0));
        if let Some(object) = again {
            self.select(object);
        }
        self.camera = camera
            .and_then(|name| {
                self.document
                    .cameras
                    .iter()
                    .position(|(known, _)| *known == name)
            })
            .unwrap_or(0);
        self.render_camera = render_camera
            .and_then(|name| {
                self.document
                    .cameras
                    .iter()
                    .position(|(known, _)| *known == name)
            })
            .unwrap_or_else(|| {
                self.render_camera
                    .min(self.document.cameras.len().saturating_sub(1))
            });
        if !self.render_chosen {
            self.render_camera = self.camera;
        }
        self.confirm = None;
        self.light = lamp
            .and_then(|(kind, name)| {
                self.document
                    .lights
                    .iter()
                    .position(|(known, named, _)| *known == kind && *named == name)
            })
            .unwrap_or_else(|| self.light.min(self.document.lights.len().saturating_sub(1)));
        self.show_lit();
        if !self.hour_set {
            self.hour = self.document.hour.unwrap_or(12.0);
        }
        if self.live() && self.eye.is_none() {
            self.eye = Some(self.home());
        }
        self.settled();
    }

    fn settled(&mut self) {
        let Some((name, eye)) = &self.settle else {
            return;
        };
        let Some(at) = self
            .document
            .cameras
            .iter()
            .position(|(known, _)| known == name)
        else {
            return;
        };
        let Some(view) = self.document.views.get(at) else {
            return;
        };
        let home = Eye::from_camera(view, f64::from(self.document.aspect));
        if cameras::near(&home, eye) {
            self.camera = at;
            self.through = true;
            if !self.render_chosen {
                self.render_camera = at;
            }
            self.eye = Some(home);
            self.settle = None;
        }
    }

    fn home(&self) -> Eye {
        self.document
            .views
            .get(self.camera)
            .map_or_else(default_eye, |camera| {
                Eye::from_camera(camera, f64::from(self.document.aspect))
            })
    }

    fn jump(&mut self, camera: usize) {
        self.camera = camera;
        self.settle = None;
        if !self.render_chosen {
            self.render_camera = camera;
        }
        if self.live() {
            self.eye = Some(self.home());
            self.through = self.document.views.get(camera).is_some();
            self.grab = None;
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = &ui.ctx().clone();
        if let Some(fixture) = self.fixture {
            fixture::ui(self, fixture, ui);
            return;
        }
        self.drain(ctx);
        self.take_shots(ctx);
        self.keyboard(ctx);
        self.pointer(ctx);
        self.tick(ctx);
        self.panels.clear();
        let texture = self
            .texture
            .get_or_insert_with(|| {
                ctx.load_texture("viewport", viewport::checker(), TextureOptions::NEAREST)
            })
            .clone();
        if self.bare {
            self.bare_view(ui, &texture);
        } else {
            self.top(ui);
            self.status(ui);
            self.hints(ui);
            self.left(ui);
            self.right(ui);
            self.centre(ui, &texture);
            self.open_layer(ctx);
        }
        self.reveal = false;
        self.drain(ctx);
    }

    fn drain(&mut self, ctx: &Context) {
        if !self.drains {
            return;
        }
        let gestures = self.edits.take();
        if gestures.is_empty() {
            return;
        }
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        let mut outcome = Outcome::default();
        for gesture in &gestures {
            let one = editor.gesture(gesture);
            outcome.changed |= one.changed;
            outcome.refused |= one.refused;
            outcome.renamed.extend(one.renamed);
        }
        self.after(ctx, outcome);
    }

    fn command(&mut self, ctx: &Context, command: Command) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        let outcome = match command {
            Command::Undo => editor.undo(),
            Command::Redo => editor.redo(),
        };
        self.after(ctx, outcome);
    }

    fn after(&mut self, ctx: &Context, outcome: Outcome) {
        if outcome.refused {
            self.drop_preview();
        }
        if outcome.changed {
            self.refresh(&outcome.renamed);
        } else {
            if let Some(object) = self.object {
                self.select(object);
            }
            self.show_lit();
        }
        ctx.request_repaint();
    }

    fn refresh(&mut self, renamed: &[(String, String)]) {
        let Some(loaded) = self.editor.as_ref().and_then(Editor::loaded) else {
            return;
        };
        let document = Document::from_recipe(&loaded.recipe, &self.document.path);
        self.place(document, renamed);
    }

    fn said(&self) -> Option<&Said> {
        self.editor.as_ref().and_then(Editor::said)
    }

    fn lit_at(&self, index: usize) -> Option<Lit> {
        match self.document.lights.get(index)? {
            (Lamp::Bulb, name, _) => Some(Lit::Light(name.clone())),
            (Lamp::Sun, _, _) => Some(Lit::Sun),
            (Lamp::Sky, _, _) => Some(Lit::Sky),
            (Lamp::Daylight, _, _) => None,
        }
    }

    fn show_lit(&mut self) {
        self.lit = self.lit_at(self.light).and_then(|lit| {
            let loaded = self.editor.as_ref().and_then(Editor::loaded)?;
            edit::lit_fields(loaded, &lit)
        });
        if let Some(fields) = &self.lit {
            let narrow = |values: [f64; 3]| values.map(|value| value as f32);
            self.dials = Dials {
                position: narrow(fields.position),
                color: narrow(fields.color),
                strength: fields.strength as f32,
                range: fields.range as f32,
                rotation: fields.rotation as f32,
                shadow: fields.shadow,
            };
        }
    }

    fn choose_light(&mut self, index: usize) {
        self.light = index;
        self.lit_shown = true;
        self.show_lit();
    }

    fn change_lit(&mut self, dial: Dial) {
        if self.editor.as_mut().is_some_and(Editor::fresh) {
            self.refresh(&[]);
        }
        let Some(lit) = self.lit_at(self.light) else {
            return;
        };
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        let Some(loaded) = editor.loaded() else {
            return;
        };
        let label = lit.key(&dial).unwrap_or("light");
        match set_lit(loaded, &lit, dial) {
            Ok(patches) if !patches.is_empty() => self.edits.push(label, patches),
            Ok(_) => {}
            Err(why) => editor.refuse(format!("{label}: {why}")),
        }
        if self.edits.is_empty() {
            self.show_lit();
        }
    }

    fn change(&mut self, change: Change) {
        if let Change::Lit(dial) = change {
            return self.change_lit(dial);
        }
        let Some(object) = self.object else {
            return;
        };
        let name = self.document.objects[object].name.clone();
        if self.editor.as_mut().is_some_and(Editor::fresh) {
            self.refresh(&[]);
        }
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        let Some(loaded) = editor.loaded() else {
            return;
        };
        let (label, made) = match change {
            Change::Name(new) => ("rename", rename(loaded, &name, &new)),
            Change::Triple(triple, axis, value) => (
                match triple {
                    Triple::Location => "location",
                    Triple::Rotation => "rotation",
                    Triple::Scale => "scale",
                },
                set_triple(loaded, &name, triple, axis, f64::from(value)),
            ),
            Change::Material(material) => ("material", set_material(loaded, &name, &material)),
            Change::Smoothing(smoothing) => ("smoothing", set_smoothing(loaded, &name, &smoothing)),
            Change::Lit(_) => return,
        };
        match made {
            Ok(patches) if !patches.is_empty() => self.edits.push(label, patches),
            Ok(_) => {}
            Err(why) => editor.refuse(format!("{label}: {why}")),
        }
        if self.edits.is_empty()
            && let Some(object) = self.document.object(&name)
        {
            self.select(object);
        }
    }

    fn select(&mut self, object: usize) {
        self.object = Some(object);
        let item = &self.document.objects[object];
        self.name = item.name.clone();
        self.material = item.material.clone();
        self.location = item.location;
        self.rotation = item.rotation;
        self.scale = item.scale;
        self.fields = self
            .editor
            .as_ref()
            .and_then(Editor::loaded)
            .and_then(|loaded| edit::fields(loaded, &item.name));
        if let Some(fields) = &self.fields {
            let narrow = |values: [f64; 3]| values.map(|value| value as f32);
            self.location = narrow(fields.location);
            self.rotation = narrow(fields.rotation);
            self.scale = narrow(fields.scale);
            self.material = match &fields.material {
                Slots::One(material) => material.clone().unwrap_or_default(),
                Slots::Many(list) => list.join(" "),
            };
            match fields.smoothing {
                Smoothing::Flat => self.smoothing = 0,
                Smoothing::Smooth => self.smoothing = 1,
                Smoothing::Angle(angle) => {
                    self.smoothing = 2;
                    self.angle = angle as f32;
                }
            }
        }
        let mut above = self
            .document
            .node_of(object)
            .and_then(|node| self.document.nodes[node].parent);
        while let Some(node) = above {
            self.open[node] = true;
            above = self.document.nodes[node].parent;
        }
    }

    fn selected_node(&self) -> Option<usize> {
        self.object.and_then(|object| self.document.node_of(object))
    }

    fn visible(&self) -> Vec<(usize, usize)> {
        let needle = self.search.trim().to_lowercase();
        let nodes = &self.document.nodes;
        if !needle.is_empty() {
            return nodes
                .iter()
                .enumerate()
                .filter(|(_, node)| {
                    node.object.is_some_and(|object| {
                        self.document.objects[object]
                            .name
                            .to_lowercase()
                            .contains(&needle)
                    })
                })
                .map(|(index, _)| (index, 0))
                .collect();
        }
        let mut visible = Vec::new();
        for (index, node) in nodes.iter().enumerate() {
            let mut above = node.parent;
            let mut shown = true;
            while let Some(parent) = above {
                shown &= self.open[parent];
                above = nodes[parent].parent;
            }
            if shown {
                visible.push((index, node.depth));
            }
        }
        visible
    }

    fn open_chord(ctx: &Context) -> bool {
        ctx.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    Event::Key {
                        key: Key::O,
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.ctrl || modifiers.command
                )
            })
        })
    }

    fn nav_key(ctx: &Context) -> bool {
        ctx.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    Event::Key {
                        key: Key::Enter | Key::Escape | Key::ArrowUp | Key::ArrowDown,
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.is_none()
                )
            })
        })
    }

    fn keyboard(&mut self, ctx: &Context) {
        if Self::open_chord(ctx) {
            self.show_picker(ctx);
            return;
        }
        let in_picker = ctx.memory(|memory| memory.has_focus(crate::open::field_id()));
        if ctx.egui_wants_keyboard_input() && !in_picker {
            self.keys = Keys::default();
            return;
        }
        if in_picker || (self.picker.shown() && Self::nav_key(ctx)) {
            self.keys = Keys::default();
            return;
        }
        let commands: Vec<Command> = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.ctrl || modifiers.command => match key {
                        Key::Z if modifiers.shift => Some(Command::Redo),
                        Key::Z => Some(Command::Undo),
                        Key::Y => Some(Command::Redo),
                        _ => None,
                    },
                    _ => None,
                })
                .collect()
        });
        if self.full && !commands.is_empty() {
            self.full = false;
            return;
        }
        for command in commands {
            self.command(ctx, command);
        }
        let xray = ctx.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    Event::Key {
                        key: Key::Z,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } if modifiers.shift_only()
                )
            })
        });
        if xray && self.live() {
            self.toggle_xray();
        }
        let pressed: Vec<Key> = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } if modifiers.is_none() => Some(*key),
                    _ => None,
                })
                .collect()
        });
        if self.full && !pressed.is_empty() {
            self.full = false;
            return;
        }
        for key in pressed {
            match self.keys.press(&Panel::ALL, key) {
                Press::Focus(panel) => self.go(ctx, panel),
                Press::Pending => {}
                Press::Other(key) => self.panel_key(ctx, key),
            }
        }
    }

    fn panel_key(&mut self, ctx: &Context, key: Key) {
        let step = |value: usize, len: usize, down: bool| {
            if down {
                (value + 1).min(len.saturating_sub(1))
            } else {
                value.saturating_sub(1)
            }
        };
        if let Some(name) = self.confirm.take()
            && self.focus == Panel::B2
        {
            match key {
                Key::Enter => self.delete_camera(&name),
                Key::Escape => {}
                _ => return self.panel_key(ctx, key),
            }
            return;
        }
        match (self.focus, key) {
            (_, Key::Slash) => {
                self.focus = Panel::A;
                ctx.memory_mut(|memory| memory.request_focus(search_id()));
            }
            (_, Key::Num0) if self.live() => self.jump(self.camera),
            (_, Key::Z) if self.live() => self.toggle_wireframe(),
            (_, Key::Space | Key::Comma | Key::Period | Key::F) if self.span().is_some() => {
                self.transport_key(key);
            }
            (Panel::B1, Key::ArrowUp | Key::ArrowDown) => {
                let objects: Vec<usize> = self
                    .visible()
                    .into_iter()
                    .filter_map(|(node, _)| self.document.nodes[node].object)
                    .collect();
                if let Some(at) = objects.iter().position(|o| Some(*o) == self.object) {
                    let next = objects[step(at, objects.len(), key == Key::ArrowDown)];
                    self.select(next);
                    self.lit_shown = false;
                    self.reveal = true;
                } else if let Some(first) = objects.first() {
                    self.select(*first);
                    self.lit_shown = false;
                    self.reveal = true;
                }
            }
            (Panel::B1, Key::ArrowRight) => {
                if let Some(node) = self.selected_node()
                    && !self.document.nodes[node].children.is_empty()
                {
                    self.open[node] = true;
                }
            }
            (Panel::B1, Key::ArrowLeft) => {
                if let Some(node) = self.selected_node() {
                    if self.open[node] && !self.document.nodes[node].children.is_empty() {
                        self.open[node] = false;
                    } else if let Some(parent) = self.document.nodes[node].parent
                        && let Some(object) = self.document.nodes[parent].object
                    {
                        self.select(object);
                        self.open[parent] = false;
                        self.reveal = true;
                    }
                }
            }
            (Panel::B2, Key::ArrowUp | Key::ArrowDown) => {
                let camera = step(
                    self.camera,
                    self.document.cameras.len(),
                    key == Key::ArrowDown,
                );
                self.jump(camera);
            }
            (Panel::B3, Key::ArrowUp | Key::ArrowDown) => {
                let light = step(
                    self.light,
                    self.document.lights.len(),
                    key == Key::ArrowDown,
                );
                self.choose_light(light);
            }
            (Panel::B3, Key::ArrowLeft) if self.hour_frozen().is_none() => {
                self.set_hour((self.hour - 0.25).max(0.0));
            }
            (Panel::B3, Key::ArrowRight) if self.hour_frozen().is_none() => {
                self.set_hour((self.hour + 0.25).min(24.0));
            }
            (Panel::C3, Key::ArrowUp | Key::ArrowDown) => {
                self.render_chosen = true;
                self.render_camera = step(
                    self.render_camera,
                    self.document.cameras.len(),
                    key == Key::ArrowDown,
                );
            }
            (Panel::B2, Key::S) => self.set_from_view(),
            (Panel::B2, Key::N) => self.new_camera(),
            (Panel::B2, Key::Delete) => self.ask_delete(),
            (Panel::C3, Key::Enter) => self.render(),
            (Panel::C3, Key::Escape) => self.shots.cancel(),
            (Panel::C3, Key::V) => self.full = self.shot_views.is_some(),
            (Panel::C1, Key::Escape) if self.live() && self.routes.dragging().is_some() => {
                self.cancel_route();
            }
            (Panel::C1, Key::G | Key::R | Key::S | Key::L | Key::Escape) if self.live() => {
                self.gizmo_key(key);
            }
            (Panel::C1, Key::P) if self.live() && !self.scene => {
                self.routes.all = !self.routes.all;
            }
            (Panel::C1, Key::Delete | Key::Backspace) if self.live() => self.remove_point(),
            _ => {}
        }
    }

    fn object_route(&self) -> Option<String> {
        let object = self.document.objects.get(self.object?)?;
        let loaded = self.editor.as_ref().and_then(Editor::loaded)?;
        crate::routes::named_by(&loaded.recipe, &object.name)
    }

    fn route_click(
        &mut self,
        lens: &Lens,
        object_route: Option<&str>,
        at: Pos2,
        add: bool,
    ) -> bool {
        if add {
            let Some(insert) = self.routes.line_at(lens, object_route, at) else {
                return false;
            };
            let Some(loaded) = self.editor.as_ref().and_then(Editor::loaded) else {
                return true;
            };
            match crate::routes::inserted(loaded, &insert) {
                Ok(patch) => {
                    self.edits.push(insert.label(), vec![patch]);
                    self.routes.point = Some((insert.name.clone(), insert.index));
                }
                Err(why) => self.refuse_route(&insert.label(), why),
            }
            return true;
        }
        match self.routes.handle_at(lens, object_route, at) {
            Some(handle) => {
                self.routes.point = Some(handle);
                true
            }
            None => {
                self.routes.point = None;
                false
            }
        }
    }

    fn refuse_route(&mut self, label: &str, why: String) {
        if let Some(editor) = self.editor.as_mut() {
            editor.refuse(format!("{label}: {why}"));
        }
    }

    fn move_point(&mut self, drag: &crate::routes::Drag) {
        let Some(loaded) = self.editor.as_ref().and_then(Editor::loaded) else {
            return;
        };
        match crate::routes::moved(loaded, &drag.name, drag.index, drag.at) {
            Ok(patch) => self
                .edits
                .join(drag.label(), vec![patch], Join::Into(drag.join)),
            Err(why) => self.refuse_route(&drag.label(), why),
        }
    }

    fn cancel_route(&mut self) {
        if let Some(drag) = self.routes.finish() {
            self.edits
                .join(drag.label(), Vec::new(), Join::Cancel(drag.join));
        }
        self.grab = None;
    }

    fn remove_point(&mut self) {
        let Some((name, index)) = self.routes.point.clone() else {
            return;
        };
        if self.routes.dragging().is_some() {
            return;
        }
        let Some(loaded) = self.editor.as_ref().and_then(Editor::loaded) else {
            return;
        };
        let label = crate::routes::removal_label(&name, index);
        match crate::routes::removed(loaded, &name, index) {
            Ok(patch) => {
                self.edits.push(label, vec![patch]);
                self.routes.point = None;
            }
            Err(why) => self.refuse_route(&label, why),
        }
    }

    fn gizmo_key(&mut self, key: Key) {
        if key == Key::Escape {
            self.gizmo.cancel();
            return;
        }
        if self.gizmo.dragging().is_some() {
            return;
        }
        match key {
            Key::G => self.gizmo.mode = Mode::Move,
            Key::R => self.gizmo.mode = Mode::Rotate,
            Key::S => self.gizmo.mode = Mode::Scale,
            Key::L => self.gizmo.space = self.gizmo.space.other(),
            _ => {}
        }
    }

    fn set_hour(&mut self, hour: f32) {
        if hour != self.hour {
            self.hour = hour;
            self.hour_set = true;
        }
    }

    fn set_from_view(&mut self) {
        let Some(eye) = self.eye.filter(|_| !self.scene) else {
            return;
        };
        let Some(camera) = self.document.views.get(self.camera) else {
            return;
        };
        let name = camera.name.clone();
        let patches = cameras::set_from_view(camera, &eye);
        self.edits
            .push(format!("set camera {name} from view"), patches);
        self.settle = Some((name, eye));
    }

    fn new_camera(&mut self) {
        let Some(eye) = self.eye.filter(|_| !self.scene) else {
            return;
        };
        let (name, patches) = cameras::new_camera(&self.document.views, &eye);
        self.edits.push(format!("new camera {name}"), patches);
        self.settle = Some((name, eye));
    }

    fn ask_delete(&mut self) {
        if self.editor.is_some()
            && let Some((name, _)) = self.document.cameras.get(self.camera)
        {
            self.confirm = Some(name.clone());
        }
    }

    fn delete_camera(&mut self, name: &str) {
        self.edits
            .push(format!("delete camera {name}"), cameras::delete(name));
    }

    fn unrenderable(&self) -> Option<&'static str> {
        if !self.live() {
            Some("render: open a recipe to render it")
        } else if self.scene {
            Some("render: manfred renders recipes; a scene file renders through pfx")
        } else if self.document.cameras.is_empty() {
            Some("render: the recipe has no camera")
        } else {
            None
        }
    }

    fn render(&mut self) {
        if let Some(why) = self.unrenderable() {
            self.note = Some(why);
            return;
        }
        let Some((camera, _)) = self.document.cameras.get(self.render_camera) else {
            return;
        };
        let size = self.size.map(|side| side.round().max(1.0) as u32);
        let camera = camera.clone();
        self.note = None;
        if self.shots.start(&self.document.path, &camera, size) {
            self.shot_views = None;
            self.full = false;
        }
    }

    pub fn set_registry(&mut self, registry: Registry) {
        let shared = std::sync::Arc::new(registry.clone());
        self.editor = self
            .editor
            .take()
            .map(|editor| editor.with_registry(shared));
        self.registry = registry;
    }

    pub(crate) fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn label(&self) -> Option<String> {
        if self.document.path == sample::RECIPE_PATH {
            return None;
        }
        std::path::Path::new(&self.document.path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    }

    pub(crate) fn take_opened(&mut self) -> Option<String> {
        self.opened.take()
    }

    pub fn set_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.shots.set_waker(std::sync::Arc::new(waker));
    }

    fn take_shots(&mut self, ctx: &Context) {
        self.shots.poll();
        if let Some(Picture { full, thumb }) = self.shots.take_picture() {
            let load = |name: &str, image: crate::shots::Image| {
                ctx.load_texture(
                    name,
                    egui::ColorImage::from_rgba_unmultiplied(image.size, &image.rgba),
                    TextureOptions::LINEAR,
                )
            };
            self.shot_views = Some(ShotViews {
                thumb: load("render thumb", thumb),
                full: load("render full", full),
            });
        }
    }

    fn pointer(&mut self, ctx: &Context) {
        let Some(at) = ctx.input(|input| {
            input
                .pointer
                .primary_pressed()
                .then(|| input.pointer.interact_pos())
                .flatten()
        }) else {
            return;
        };
        if let Some((panel, _)) = self.panels.iter().find(|(_, rect)| rect.contains(at)) {
            self.focus = *panel;
        }
    }

    fn viewing_hints(panel: Panel) -> &'static [(&'static str, &'static str)] {
        match panel {
            Panel::B2 => &[("↑↓", "camera"), ("0", "back to camera")],
            Panel::C1 => &[
                ("click", "select"),
                ("drag", "orbit"),
                ("⇧ drag", "pan"),
                ("wheel", "dolly"),
                ("z ⇧z", "wire x-ray"),
                ("0", "back to camera"),
            ],
            _ => Shell::hints_for(panel, false),
        }
    }

    fn hints_for(panel: Panel, live: bool) -> &'static [(&'static str, &'static str)] {
        match (panel, live) {
            (Panel::B2, true) => {
                return &[
                    ("↑↓", "camera"),
                    ("s", "set from view"),
                    ("n", "new"),
                    ("del", "delete"),
                    ("0", "back to camera"),
                ];
            }
            (Panel::C1, true) => {
                return &[
                    ("click", "select"),
                    ("drag", "orbit"),
                    ("⇧ drag", "pan"),
                    ("wheel", "dolly"),
                    ("g r s", "move rotate scale"),
                    ("l", "local"),
                    ("p", "routes"),
                    ("ctrl", "snap"),
                    ("esc", "cancel"),
                    ("z ⇧z", "wire x-ray"),
                    ("0", "back to camera"),
                ];
            }
            _ => {}
        }
        match panel {
            Panel::A => &[("/", "search")],
            Panel::B1 => &[("↑↓", "select"), ("←→", "fold"), ("/", "search")],
            Panel::B2 => &[("↑↓", "camera")],
            Panel::B3 => &[("↑↓", "light"), ("←→", "hour")],
            Panel::C1 => &[("/", "search")],
            Panel::C2 => &[("tab", "next field"), ("⏎", "commit"), ("esc", "cancel")],
            Panel::C3 => &[
                ("↑↓", "camera"),
                ("⏎", "render"),
                ("esc", "cancel"),
                ("v", "view"),
            ],
            Panel::D1 => &[("/", "search")],
        }
    }

    fn edge(&self, panel: Panel) -> Color32 {
        if self.focus == panel {
            LINE_STRONG
        } else {
            LINE_BORDER
        }
    }

    fn border(&self, ui: &Ui, panel: Panel, rect: Rect) {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(RADIUS),
            Stroke::new(STROKE, self.edge(panel)),
            StrokeKind::Inside,
        );
    }

    fn header(&mut self, ui: &mut Ui, panel: Panel, header: Header) {
        let header = header.id(header_id(panel)).focused(self.focus == panel);
        if ui.add(header).clicked() {
            self.go(ui.ctx(), panel);
        }
    }

    fn go(&mut self, ctx: &Context, panel: Panel) {
        self.focus = panel;
        self.confirm = None;
        self.keys = Keys::default();
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
    }

    fn top(&mut self, ui: &mut Ui) {
        let rect = bar(egui::Panel::top("A"), TOP, BG_DEEP, ui, |ui| {
            let kind = if self.scene { "scene" } else { "recipe" };
            self.header(ui, Panel::A, Header::new("A", kind).width(92.0));
            ui.label(
                RichText::new(&self.document.name)
                    .font(Theme::of(ui.ctx()).bold_font(SIZE_HEADING))
                    .color(TEXT_PRIMARY),
            );
            ui.label(code(ui, &self.document.path, TEXT_MUTED));
            ui.add_space(SECTION_GAP * 2.0);
            let mut tab = tab_of(self.focus);
            if ui.add(Tabs::new(&mut tab, &TABS)).changed()
                && let Some(tab) = tab
            {
                self.go(ui.ctx(), TAB_PANELS[tab]);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let hint = format!("{SEARCH}  search  /");
                text_field(ui, &mut self.search, &hint, 240.0, Some(search_id()));
            });
        });
        self.panels.push((Panel::A, rect));
        self.border_over(ui.ctx(), Panel::A, rect);
    }

    fn border_over(&self, ctx: &Context, panel: Panel, rect: Rect) {
        let colour = self.edge(panel);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            Id::new(("border", panel.code())),
        ));
        if self.focus == panel {
            painter.rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(STROKE, colour),
                StrokeKind::Inside,
            );
        } else {
            let y = if panel == Panel::A {
                rect.bottom() - 0.5
            } else {
                rect.top() + 0.5
            };
            painter.hline(rect.x_range(), y, Stroke::new(STROKE, colour));
        }
    }

    fn status(&mut self, ui: &mut Ui) {
        let rect = bar(egui::Panel::bottom("D1"), STATUS, BG_DEEP, ui, |ui| {
            self.header(ui, Panel::D1, Header::new("D1", "status").width(92.0));
            let kind = if self.scene { "scene" } else { "recipe" };
            item(ui, kind, &self.document.path, TEXT_CODE);
            divider(ui);
            match (&self.trouble, self.note, self.said()) {
                (_, _, Some(Said::Refused(why))) => {
                    item(ui, "refused", &first_line(why, 96), DANGER)
                }
                _ if self.busy => item(ui, "build", "rebuilding…", ACCENT),
                (Some(trouble), _, _) => item(ui, "error", &first_line(trouble, 96), DANGER),
                (None, Some(note), _) => item(ui, "note", note, ACCENT),
                (None, None, Some(Said::Reloaded(what))) => {
                    item(ui, "reloaded", &first_line(what, 96), ACCENT)
                }
                (None, None, Some(Said::Patched(line))) => {
                    item(ui, "patch", &first_line(line, 96), TEXT_CODE)
                }
                (None, None, None) => item(ui, "patch", &self.document.patch, TEXT_CODE),
            }
            if let Some(job) = self.shots.job() {
                divider(ui);
                item(ui, "render", &job.summary(), phase_colour(&job.phase));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            code(ui, status_version(), TEXT_MUTED),
                        );
                        divider(ui);
                        let tone = match self.pgpu {
                            Pgpu::Free => Tone::Ok,
                            Pgpu::Play => Tone::Busy,
                            Pgpu::Queued => Tone::Neutral,
                        };
                        ui.add(Pill::new(self.pgpu.word(), tone));
                        ui.label(
                            code(ui, "pgpu", TEXT_MUTED),
                        );
                        divider(ui);
                        let frame = self
                            .frame_ms
                            .map_or_else(|| "—".to_string(), |ms| format!("{ms:.1} ms"));
                        ui.label(code(ui, frame, TEXT_CODE))
                            .on_hover_text(GPU_TIME);
                        ui.label(
                            code(ui, "GPU frame", TEXT_MUTED),
                        )
                        .on_hover_text(GPU_TIME);
                        if self.timing_drops > 0 {
                            divider(ui);
                            ui.label(
                                code(ui, format!("{} dropped", self.timing_drops), ACCENT),
                            )
                            .on_hover_text(format!(
                                "{} frames' GPU timings never arrived or arrived too late, so the GPU frame time skips them",
                                self.timing_drops
                            ));
                        }
                    });
        });
        self.panels.push((Panel::D1, rect));
        self.border_over(ui.ctx(), Panel::D1, rect);
    }

    fn hints(&mut self, ui: &mut Ui) {
        bar(egui::Panel::bottom("hints"), HINTS, BG_BASE, ui, |ui| {
            ui.label(code(ui, self.focus.code(), TEXT_DIM));
            let hints = if self.scene {
                Shell::viewing_hints(self.focus)
            } else {
                Shell::hints_for(self.focus, self.live())
            };
            for (key, word) in hints {
                ui.add_space(4.0);
                ui.add(KeyCap::new(key));
                ui.label(small(ui, *word, TEXT_SECONDARY));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(small(ui, "panel", TEXT_SECONDARY));
                ui.add(KeyCap::new("1–3"));
                let pending = self.keys.pending().map(|letter| letter.to_string());
                ui.add(KeyCap::new(pending.as_deref().unwrap_or("a–d")).lit(pending.is_some()));
                ui.add_space(SECTION_GAP);
                ui.label(small(ui, "open", TEXT_SECONDARY));
                let closed = !self.picker.showing(self.live());
                if ui
                    .add_enabled(closed, KeyCap::new("ctrl o").button())
                    .clicked()
                {
                    self.show_picker(ui.ctx());
                }
                let Some((undo, redo)) = self.editor.as_ref().map(Editor::steps) else {
                    return;
                };
                ui.add_space(SECTION_GAP);
                for (key, word, command, steps) in [
                    ("ctrl ⇧ z", "redo", Command::Redo, redo),
                    ("ctrl z", "undo", Command::Undo, undo),
                ] {
                    ui.label(small(ui, word, TEXT_SECONDARY));
                    if ui
                        .add_enabled(steps > 0, KeyCap::new(key).button())
                        .clicked()
                    {
                        self.full = false;
                        self.command(ui.ctx(), command);
                    }
                    ui.add_space(4.0);
                }
            });
        });
    }

    fn show_picker(&mut self, ctx: &Context) {
        self.picker.show();
        ctx.memory_mut(|memory| memory.request_focus(crate::open::field_id()));
    }

    fn section(
        &mut self,
        ui: &mut Ui,
        panel: Panel,
        height: f32,
        info: &str,
        body: impl FnOnce(&mut Shell, &mut Ui),
    ) {
        let size = Vec2::new(ui.available_width(), height.max(ROW_HEIGHT * 3.0));
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let inner = rect.shrink(f32::from(PANEL_PADDING));
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
        );
        child.set_clip_rect(rect.shrink(1.0));
        self.header(
            &mut child,
            panel,
            Header::new(panel.code(), panel.title()).info(info),
        );
        child.add_space(2.0);
        body(self, &mut child);
        self.border(ui, panel, rect);
        self.panels.push((panel, rect));
    }

    fn left(&mut self, ui: &mut Ui) {
        side(egui::Panel::left("B"), LEFT, ui, |ui| {
            let total = ui.available_height();
            let outliner = total - CAMERAS_HEIGHT - LIGHTS_HEIGHT - 2.0 * GAP;
            let count = format!("{} objects", self.document.objects.len());
            self.section(ui, Panel::B1, outliner, &count, Shell::outliner);
            let cameras = self.document.cameras.len().to_string();
            self.section(ui, Panel::B2, CAMERAS_HEIGHT, &cameras, Shell::cameras);
            self.section(ui, Panel::B3, LIGHTS_HEIGHT, "", Shell::lights);
        });
    }

    fn right(&mut self, ui: &mut Ui) {
        side(egui::Panel::right("C"), RIGHT, ui, |ui| {
            let total = ui.available_height();
            let properties = total - RENDER_HEIGHT - GAP;
            let name = match (self.lit_shown, self.object) {
                (true, _) => self
                    .document
                    .lights
                    .get(self.light)
                    .map_or_else(String::new, |(_, name, _)| name.clone()),
                (false, Some(object)) => self.document.objects[object].name.clone(),
                (false, None) => String::new(),
            };
            self.section(ui, Panel::C2, properties, &name, Shell::properties);
            self.section(ui, Panel::C3, RENDER_HEIGHT, "still", Shell::render_panel);
        });
    }

    fn centre(&mut self, ui: &mut Ui, texture: &TextureHandle) {
        CentralPanel::default()
            .frame(
                Frame::NONE
                    .fill(BG_BASE)
                    .inner_margin(Margin::symmetric(0, GAP as i8)),
            )
            .show(ui, |ui| {
                let height = ui.available_height();
                let info = self.view_info();
                self.section(ui, Panel::C1, height, &info, |shell, ui| {
                    let sense = if shell.live() {
                        Sense::click_and_drag()
                    } else {
                        Sense::hover()
                    };
                    let bar = if shell.span().is_some() {
                        transport::HEIGHT
                    } else {
                        0.0
                    };
                    let room = ui.available_size() - Vec2::new(0.0, bar);
                    let (area, response) =
                        ui.allocate_exact_size(room.max(Vec2::splat(1.0)), sense);
                    ui.painter().rect_filled(area, CornerRadius::ZERO, BG_DEEP);
                    let image = Rect::from_min_max(area.min.round(), area.max.round());
                    if !shell.live() {
                        let painter = ui.painter();
                        painter.image(
                            texture.id(),
                            image,
                            viewport::cover(image, viewport::ASPECT),
                            Color32::WHITE,
                        );
                        viewport::frame(painter, image);
                        viewport::gizmo(painter, image);
                        shell.viewport = Some(image);
                        return;
                    }
                    shell.viewport = Some(image);
                    if shell.full {
                        if response.clicked() {
                            shell.full = false;
                        }
                    } else if shell.picker.shown() {
                        if response.clicked() {
                            shell.picker.close();
                        }
                    } else {
                        shell.steer(ui, &response, image);
                    }
                    shell.picture(ui, image);
                    shell.shading(ui, image);
                    shell.shot_overlay(ui, image);
                    if bar > 0.0 {
                        shell.transport(ui);
                    }
                });
            });
    }

    fn open_layer(&mut self, ctx: &Context) {
        let live = self.live();
        if !self.picker.showing(live) {
            return;
        }
        let act = self.picker.draw(ctx, live, &self.document.path, &self.book);
        match act {
            Some(crate::open::Act::Open(path)) => self.open_recipe(&path),
            Some(crate::open::Act::Close) => self.picker.close(),
            None => {}
        }
    }

    fn open_recipe(&mut self, path: &std::path::Path) {
        if crate::watched::is_scene(path) {
            return self.open_scene(path);
        }
        if let Err(error) = manfred_recipe::load(path) {
            self.picker.fail(error.to_string());
            return;
        }
        let path_text = path.display().to_string();
        let mut editor =
            Editor::new(&path_text).with_registry(std::sync::Arc::new(self.registry.clone()));
        editor.fresh();
        let document = match editor.loaded() {
            Some(loaded) => Document::from_recipe(&loaded.recipe, &path_text),
            None => Document::empty(&path_text),
        };
        self.editor = Some(editor);
        self.scene = false;
        self.drains |= !cfg!(test);
        self.adopt(path, document);
    }

    fn open_scene(&mut self, path: &std::path::Path) {
        let scene = match pfx_load::scene::Scene::open(path) {
            Ok(scene) => scene,
            Err(error) => {
                self.picker.fail(error.to_string());
                return;
            }
        };
        let path_text = path.display().to_string();
        let document = crate::watched::document(&scene, &path_text);
        self.editor = None;
        self.scene = true;
        self.adopt(path, document);
    }

    fn adopt(&mut self, path: &std::path::Path, document: Document) {
        let path_text = path.display().to_string();
        let _ = self.book.remember(path);
        self.edits.take();
        self.hour_set = false;
        self.eye = None;
        self.grab = None;
        self.pick = None;
        self.settle = None;
        self.through = false;
        self.confirm = None;
        self.note = None;
        self.object = None;
        self.search.clear();
        self.full = false;
        self.render_chosen = false;
        self.view = None;
        self.view_error = None;
        self.trouble = None;
        self.preparing = None;
        self.gizmo = Gizmo::default();
        self.routes = crate::routes::Routes::default();
        self.spans = crate::spans::Spans::default();
        self.clock = Clock::default();
        self.chase_eye = None;
        self.picker.close();
        self.calm = false;
        self.first = None;
        self.place(document, &[]);
        self.opened = Some(path_text);
    }

    fn view_info(&self) -> String {
        if !self.live() {
            let camera = self
                .document
                .cameras
                .get(self.camera)
                .map_or("", |(name, _)| name);
            return format!("{camera} · placeholder");
        }
        let camera = self
            .document
            .cameras
            .get(self.camera)
            .map(|(name, _)| name.as_str());
        let mut info = match (camera, self.through().is_some()) {
            _ if self.chase_view().is_some() => "chase cam".to_string(),
            (Some(name), true) => name.to_string(),
            (Some(name), false) => format!("orbit from {name}"),
            (None, _) => "orbit".to_string(),
        };
        if self.paused() {
            info.push_str(" · paused");
        }
        if self.wire.on() {
            info.push_str(if self.wire.xray {
                " · wireframe x-ray"
            } else {
                " · wireframe"
            });
        }
        info
    }

    fn steer(&mut self, ui: &Ui, response: &Response, image: Rect) {
        let chased = self.chase_view().map(|(eye, _)| eye);
        let (Some(eye), Some(lens)) = (chased.or(self.eye), self.lens()) else {
            return;
        };
        let tall = f64::from(self.free_frame(image).height());
        let (origin, pointer, hover, middle, shift, snap) = ui.input(|input| {
            (
                input.pointer.press_origin(),
                input.pointer.interact_pos(),
                input.pointer.hover_pos(),
                input.pointer.middle_down(),
                input.modifiers.shift,
                input.modifiers.ctrl,
            )
        });
        let object_route = self.object_route();
        if response.drag_started() {
            self.settle = None;
            let handle = origin.filter(|_| !middle).and_then(|origin| {
                self.routes
                    .handle_at(&lens, object_route.as_deref(), origin)
            });
            let route = match (handle, origin) {
                (Some(handle), Some(origin)) => self.routes.begin(lens, handle, origin),
                _ => false,
            };
            let handled = route
                || !middle
                    && !shift
                    && match (origin, self.object) {
                        (Some(origin), Some(object)) => self.gizmo.begin(object, origin, lens),
                        _ => false,
                    };
            if !handled {
                self.grab = origin.map(|origin| Grab {
                    eye,
                    origin,
                    pan: middle || shift,
                });
            }
        }
        if response.dragged()
            && self.gizmo.dragging().is_some()
            && let Some(at) = pointer
        {
            self.gizmo.to(at, snap);
        }
        if response.dragged()
            && let Some(at) = pointer
            && let Some(drag) = self.routes.to(at, shift).cloned()
        {
            self.move_point(&drag);
        }
        if response.dragged()
            && !self.gizmo.swallowing()
            && self.gizmo.dragging().is_none()
            && self.routes.dragging().is_none()
            && let Some(grab) = self.grab
            && let Some(at) = pointer
        {
            let moved = [
                f64::from(at.x - grab.origin.x),
                f64::from(at.y - grab.origin.y),
            ];
            self.eye = Some(if grab.pan {
                orbit::pan(&grab.eye, moved, tall)
            } else {
                orbit::turn(&grab.eye, moved)
            });
            self.through = false;
            self.clock.chase = false;
        }
        if response.drag_stopped() {
            if let Some((label, patches)) = self.gizmo.finish() {
                self.edits.push(label, patches);
            }
            self.routes.finish();
            self.grab = None;
        }
        if response.clicked()
            && let Some(at) = pointer
            && !self.route_click(&lens, object_route.as_deref(), at, snap)
        {
            let handle = self
                .object
                .and_then(|object| self.gizmo.hit(&lens, object, at));
            if handle.is_none() {
                self.pick = pick::uv(image, at);
            }
        }
        if self.gizmo.dragging().is_none() {
            let handle = hover
                .filter(|_| response.hovered())
                .zip(self.object)
                .and_then(|(at, object)| self.gizmo.hit(&lens, object, at));
            self.gizmo.hover(handle);
        }
        if response.hovered() && self.grab.is_none() {
            let options = ui.ctx().options(|options| options.input_options);
            let scroll = ui.input(|input| scrolled(input, &options));
            if scroll != 0.0
                && let Some(eye) = chased.or(self.eye)
            {
                self.eye = Some(orbit::dolly(&eye, f64::from(scroll)));
                self.through = false;
                self.clock.chase = false;
            }
        }
    }

    fn picture(&self, ui: &Ui, image: Rect) {
        let painter = ui.painter();
        match (&self.view_error, self.view) {
            (Some(error), _) => notice(painter, image, "C1 cannot draw this recipe", DANGER, error),
            (None, Some(id)) => {
                painter.image(id, image, WHOLE, Color32::WHITE);
                if let Some(preparing) = &self.preparing {
                    let min = Pos2::new(image.left(), image.bottom() - 24.0)
                        + Vec2::new(SECTION_GAP, -SECTION_GAP);
                    band(painter, min, preparing.clone(), None);
                }
            }
            (None, None) => {
                if let Some(preparing) = &self.preparing {
                    notice(painter, image, "C1 preparing", ACCENT, preparing);
                }
            }
        }
        viewport::frame(painter, image);
        if let Some(eye) = self.view_eye() {
            if self.view_error.is_none() {
                viewport::grid(painter, &eye, image, &self.solids_shown());
            }
            if let Some(frame) = self.camera_frame() {
                viewport::passepartout(painter, image, frame);
            }
            viewport::axes(painter, image, &orbit::basis(&eye));
            if !self.scene && self.view_error.is_none() {
                let clipped = painter.with_clip_rect(image);
                let object_route = self.object_route();
                self.routes
                    .draw(&clipped, &Lens::new(&eye, image), object_route.as_deref());
            }
            if let Some(object) = self.object.filter(|_| self.view_error.is_none()) {
                let clipped = painter.with_clip_rect(image);
                self.gizmo.draw(&clipped, &Lens::new(&eye, image), object);
            }
            if !self.scene {
                self.gizmo.badge(painter, image);
            }
        }
        if self.paused() {
            band(
                painter,
                image.left_top() + Vec2::splat(SECTION_GAP),
                format!("{PAUSE}  paused while a game plays"),
                Some(240.0),
            );
        }
    }

    fn outliner(&mut self, ui: &mut Ui) {
        let focused = self.focus == Panel::B1;
        let visible = self.visible();
        egui::ScrollArea::vertical()
            .id_salt("outliner")
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, depth) in visible {
                    let node = &self.document.nodes[index];
                    let parent = !node.children.is_empty() && self.search.is_empty();
                    let caret = if !parent {
                        ""
                    } else if self.open[index] {
                        CARET_OPEN
                    } else {
                        CARET_SHUT
                    };
                    let object = node.object;
                    let (label, icon) = match object {
                        Some(object) if !self.search.is_empty() => {
                            (self.document.objects[object].name.as_str(), (CUBE, INFO))
                        }
                        Some(_) => (node.label.as_str(), (CUBE, INFO)),
                        None => (node.label.as_str(), (FOLDER, TEXT_MUTED)),
                    };
                    let detail = match object {
                        Some(object) => self.document.objects[object].material.as_str(),
                        None => "",
                    };
                    let text = if caret.is_empty() {
                        label.to_string()
                    } else {
                        format!("{label} {caret}")
                    };
                    let selected = object.is_some() && object == self.object;
                    let copy = object.and_then(|object| self.copy_note(object));
                    let mark = copy.as_ref().map(|_| "use");
                    let response = marked_row(
                        ui,
                        Row {
                            focused,
                            selected,
                            depth,
                            icon,
                            label: &text,
                            detail,
                            mark,
                        },
                    );
                    let response = match copy {
                        Some(note) => response.on_hover_text(note),
                        None => response,
                    };
                    if selected && self.reveal {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() {
                        match object {
                            Some(object) if !(parent && selected) => {
                                self.select(object);
                                self.lit_shown = false;
                            }
                            _ => self.open[index] = !self.open[index],
                        }
                    }
                }
                self.route_rows(ui, focused);
            });
    }

    fn route_rows(&mut self, ui: &mut Ui, focused: bool) {
        if self.scene || !self.search.is_empty() {
            return;
        }
        let rows: Vec<(String, String)> = self
            .routes
            .lines()
            .iter()
            .map(|line| {
                let detail = format!(
                    "route · {} points · {:.0} m",
                    line.route.points().len(),
                    line.route.length()
                );
                (line.name.clone(), detail)
            })
            .collect();
        for (name, detail) in rows {
            let selected = self.routes.chosen.as_deref() == Some(name.as_str());
            let response = list_row(ui, focused, selected, 0, (ROUTE, ACCENT), &name, &detail)
                .on_hover_text(
                    "show or hide this route's line and points in C1; p shows every route",
                );
            if response.clicked() {
                self.routes.toggle(&name);
            }
        }
    }

    fn copy_note(&self, object: usize) -> Option<String> {
        let name = &self.document.objects.get(object)?.name;
        let recipe = &self.editor.as_ref()?.loaded()?.recipe;
        let used = recipe.copy_of(name)?;
        let place = if used.prefix.is_empty() {
            "the use".to_string()
        } else {
            format!("use[{}]", used.prefix)
        };
        Some(format!(
            "a copy, read only: {place} places it from {}",
            used.file
        ))
    }

    fn cameras(&mut self, ui: &mut Ui) {
        let focused = self.focus == Panel::B2;
        let list = (ui.available_height() - BUTTONS - GAP).max(ROW_HEIGHT);
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .id_salt("cameras")
            .max_height(list)
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, (name, detail)) in self.document.cameras.iter().enumerate() {
                    let response = list_row(
                        ui,
                        focused,
                        index == self.camera,
                        0,
                        (CAMERA, INFO),
                        name,
                        detail,
                    );
                    if response.clicked() {
                        chosen = Some(index);
                    }
                }
            });
        if let Some(index) = chosen {
            self.jump(index);
        }
        ui.add_space(GAP / 2.0);
        self.camera_buttons(ui);
    }

    fn camera_buttons(&mut self, ui: &mut Ui) {
        let mut act = Gesture::None;
        let editing = self.eye.is_some() && !self.scene;
        let any = !self.document.cameras.is_empty();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if let Some(name) = &self.confirm {
                ui.label(code(ui, format!("delete {name}?"), DANGER));
                if chip(ui, "Delete  ⏎", true).clicked() {
                    act = Gesture::Delete(name.clone());
                }
                if chip(ui, "Keep  esc", true).clicked() {
                    act = Gesture::Keep;
                }
                return;
            }
            if chip(ui, "Set from view  s", editing && any).clicked() {
                act = Gesture::Set;
            }
            if chip(ui, "New  n", editing).clicked() {
                act = Gesture::New;
            }
            if chip(ui, "Delete  del", self.editor.is_some() && any).clicked() {
                act = Gesture::Ask;
            }
        });
        match act {
            Gesture::None => {}
            Gesture::Set => self.set_from_view(),
            Gesture::New => self.new_camera(),
            Gesture::Ask => self.ask_delete(),
            Gesture::Delete(name) => {
                self.confirm = None;
                self.delete_camera(&name);
            }
            Gesture::Keep => self.confirm = None,
        }
    }

    fn lights(&mut self, ui: &mut Ui) {
        let focused = self.focus == Panel::B3;
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut chosen = None;
        for (index, (lamp, name, detail)) in self.document.lights.iter().enumerate() {
            let icon = match lamp {
                Lamp::Sun | Lamp::Daylight => SUN,
                Lamp::Sky => CLOUD,
                Lamp::Bulb => BULB,
            };
            let response = list_row(
                ui,
                focused,
                index == self.light,
                0,
                (icon, ACCENT),
                name,
                detail,
            );
            if response.clicked() {
                chosen = Some(index);
            }
        }
        if let Some(index) = chosen {
            self.choose_light(index);
        }
        ui.add_space(GAP);
        ui.add(Dotted::new());
        ui.spacing_mut().item_spacing.y = GAP;
        let mut hour = self.hour;
        let kept = self.hour_kept();
        let mut write = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(small(ui, "hour", TEXT_SECONDARY));
            let label = kept.map_or("Use hour", |(label, _)| label);
            let chip_font = Theme::of(ui.ctx()).font(SIZE_CODE);
            let chip_width = ui.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(label.to_string(), chip_font, TEXT_CODE)
                    .size()
                    .x
            }) + 18.0;
            ui.spacing_mut().slider_width = (ui.available_width() - chip_width - 60.0).max(40.0);
            let frozen = self.hour_frozen();
            let slider = ui.add_enabled(
                frozen.is_none(),
                Slider::new(&mut hour, 0.0..=24.0).show_value(false),
            );
            if let Some(why) = frozen {
                slider.on_disabled_hover_text(why);
            }
            let minutes = (hour * 60.0).round() as u32;
            ui.label(code(
                ui,
                format!("{:02}:{:02}", minutes / 60, minutes % 60),
                TEXT_CODE,
            ));
            let response = chip(ui, label, kept.is_none());
            write = response.clicked();
            if let Some((_, why)) = kept {
                response.on_disabled_hover_text(why);
            }
        });
        self.set_hour(hour);
        if write {
            self.use_hour();
        }
    }

    fn hour_frozen(&self) -> Option<&'static str> {
        if self.scene {
            Some("a scene file's sun is not moved by the hour; edit it in its file")
        } else if self.editor.is_none() {
            Some("open a recipe to move its sun by the hour")
        } else {
            None
        }
    }

    fn hour_kept(&self) -> Option<(&'static str, &'static str)> {
        if self.scene {
            return Some(("scene file", "a scene file is not written"));
        }
        if self.editor.is_none() {
            return Some(("Use hour", "open a recipe to write its hour"));
        }
        let Some(recipe) = self.document.hour else {
            let authored = self
                .document
                .lights
                .iter()
                .any(|(lamp, ..)| *lamp == Lamp::Sun);
            return Some(if authored {
                ("authored sun", "the authored sun has no hour of its own")
            } else {
                (
                    "no [sun]",
                    "the recipe has no daylight [sun] to give an hour",
                )
            });
        };
        let round = |hour: f32| (hour * 100.0).round();
        (round(recipe) == round(self.hour))
            .then_some(("Use hour", "the recipe has this hour already"))
    }

    fn use_hour(&mut self) {
        if self.editor.as_mut().is_some_and(Editor::fresh) {
            self.refresh(&[]);
        }
        let hour = f64::from(self.hour);
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        let Some(loaded) = editor.loaded() else {
            return;
        };
        match edit::set_hour(loaded, hour) {
            Ok(patches) if !patches.is_empty() => self.edits.push("hour", patches),
            Ok(_) => {}
            Err(why) => editor.refuse(format!("hour: {why}")),
        }
    }

    fn properties(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = GAP;
        ui.add_space(4.0);
        if self.lit_shown && self.light < self.document.lights.len() {
            return self.lit_properties(ui);
        }
        if self.object.is_none() {
            ui.label(small(ui, "nothing selected", TEXT_MUTED));
            return;
        }
        if self.editor.is_none() {
            return self.read_properties(ui);
        }
        let mut change = None;
        let escape = ui.input(|input| input.key_pressed(Key::Escape));
        let name = &mut self.name;
        labelled(ui, "name", |ui| {
            let width = ui.available_width();
            let response = text_field(ui, name, "", width, Some(Id::new("manfred c2 name")));
            if response.lost_focus() && !escape {
                change = Some(Change::Name(name.clone()));
            }
        });
        if escape
            && change.is_none()
            && let Some(object) = self.object
        {
            self.name = self.document.objects[object].name.clone();
        }
        ui.add(Dotted::new());
        let fields = self.fields.as_ref();
        for (triple, label, values) in [
            (Triple::Location, "location", &mut self.location),
            (Triple::Rotation, "rotation", &mut self.rotation),
            (Triple::Scale, "scale", &mut self.scale),
        ] {
            if let Some((axis, value)) = triple_row(ui, label, triple, values, fields) {
                change = Some(Change::Triple(triple, axis, value));
            }
        }
        let named = self.document.objects[self.object.unwrap_or_default()]
            .name
            .clone();
        self.spans.follow(&self.document.path, Some(&named));
        if !self.spans.is_text() {
            ui.add(Dotted::new());
            let materials = &self.document.materials;
            let material = &mut self.material;
            let many = matches!(fields.map(|fields| &fields.material), Some(Slots::Many(_)));
            labelled(ui, "material", |ui| {
                if many {
                    ui.label(code(ui, material.as_str(), TEXT_MUTED))
                        .on_hover_text("several material slots: change `materials` in the recipe");
                    return;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ComboBox::from_id_salt("material")
                        .width(24.0)
                        .selected_text("")
                        .show_ui(ui, |ui| {
                            for name in materials {
                                let chosen = *name == *material;
                                if ui.selectable_label(chosen, name.as_str()).clicked() {
                                    *material = name.clone();
                                    change = Some(Change::Material(name.clone()));
                                }
                            }
                        });
                    let width = ui.available_width();
                    let response = text_field(
                        ui,
                        material,
                        "type a name",
                        width,
                        Some(Id::new("manfred c2 material")),
                    );
                    if response.lost_focus() && !escape {
                        change = Some(Change::Material(material.clone()));
                    }
                });
            });
            let smoothing = &mut self.smoothing;
            let angle = &mut self.angle;
            let angle_params = fields.and_then(|fields| fields.angle_params.as_deref());
            labelled(ui, "smoothing", |ui| {
                let chosen = |index: usize, angle: f32| match index {
                    0 => Smoothing::Flat,
                    1 => Smoothing::Smooth,
                    _ => Smoothing::Angle(f64::from(angle)),
                };
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let before = *smoothing;
                    if *smoothing == 2 {
                        match angle_params {
                            Some(reference) => params_slot(ui, reference, 64.0),
                            None => {
                                let response = ui.add(
                                    Number::new(angle)
                                        .unit("°")
                                        .speed(0.5)
                                        .decimals(0)
                                        .width(64.0),
                                );
                                if committed(&response) {
                                    change = Some(Change::Smoothing(chosen(2, *angle)));
                                }
                            }
                        }
                    }
                    ComboBox::from_id_salt("smoothing")
                        .width(ui.available_width())
                        .selected_text(code(ui, sample::SMOOTHING[*smoothing], TEXT_CODE))
                        .show_ui(ui, |ui| {
                            for (index, name) in sample::SMOOTHING.iter().enumerate() {
                                ui.selectable_value(smoothing, index, *name);
                            }
                        });
                    if *smoothing != before {
                        change = Some(Change::Smoothing(chosen(*smoothing, *angle)));
                    }
                });
            });
        }
        if let Some(change) = change {
            self.change(change);
        }
        self.text_spans(ui);
    }

    fn read_properties(&self, ui: &mut Ui) {
        let why = if self.scene {
            "read only; a scene file is edited in its file"
        } else {
            "read only; open a recipe to edit it"
        };
        labelled(ui, "name", |ui| {
            read_slot(ui, &self.name, why.to_string(), ui.available_width());
        });
        ui.add(Dotted::new());
        for (label, values, unit) in [
            ("location", self.location, "m"),
            ("rotation", self.rotation, "°"),
            ("scale", self.scale, ""),
        ] {
            labelled(ui, label, |ui| {
                let width = ((ui.available_width() - 2.0 * GAP) / 3.0).floor();
                for value in values {
                    read_slot(ui, &format!("{value:.3}{unit}"), why.to_string(), width);
                }
            });
        }
        ui.add(Dotted::new());
        labelled(ui, "material", |ui| {
            read_slot(ui, &self.material, why.to_string(), ui.available_width());
        });
        labelled(ui, "smoothing", |ui| {
            let word = sample::SMOOTHING[self.smoothing];
            read_slot(ui, word, why.to_string(), ui.available_width());
        });
    }

    fn lit_properties(&mut self, ui: &mut Ui) {
        let (_, name, detail) = &self.document.lights[self.light];
        labelled(ui, "name", |ui| {
            ui.label(code(ui, name.as_str(), TEXT_CODE));
        });
        ui.add(Dotted::new());
        let Some(lit) = self.lit.as_ref().map(|fields| fields.lit.clone()) else {
            let why = match (self.live(), self.document.lights[self.light].0) {
                (false, _) => "open a recipe to edit its lights",
                (true, Lamp::Daylight) => {
                    "the hour slider in B3 previews only; Use hour writes it to the recipe"
                }
                (true, _) => detail.as_str(),
            };
            ui.label(small(ui, why, TEXT_MUTED));
            return;
        };
        let dials = &mut self.dials;
        let mut dial = None;
        if lit.has(&Dial::Position(0, 0.0))
            && let Some((axis, value)) =
                dial_row(ui, "position", "m", 4, 0.001, &mut dials.position)
        {
            dial = Some(Dial::Position(axis, f64::from(value)));
        }
        if lit.has(&Dial::Color(0, 0.0))
            && let Some((axis, value)) = colour_row(ui, &mut dials.color)
        {
            dial = Some(Dial::Color(axis, f64::from(value)));
        }
        let (label, unit) = match lit {
            Lit::Sun => ("irradiance", "W/m²"),
            Lit::Sky => ("intensity", "×"),
            Lit::Light(_) => ("intensity", ""),
        };
        if let Some(value) = dial_number(ui, label, unit, 3, 0.05, &mut dials.strength) {
            dial = Some(Dial::Strength(f64::from(value)));
        }
        if lit.has(&Dial::Range(0.0))
            && let Some(value) = dial_number(ui, "range", "m", 3, 0.01, &mut dials.range)
        {
            dial = Some(Dial::Range(f64::from(value)));
        }
        if lit.has(&Dial::Rotation(0.0))
            && let Some(value) = dial_number(ui, "rotation", "°", 2, 0.5, &mut dials.rotation)
        {
            dial = Some(Dial::Rotation(f64::from(value)));
        }
        if lit.has(&Dial::Shadow(false)) {
            let shadow = &mut dials.shadow;
            labelled(ui, "shadow", |ui| {
                let word = if *shadow { "cast  on" } else { "cast  off" };
                if chip(ui, word, true).clicked() {
                    dial = Some(Dial::Shadow(!*shadow));
                }
            });
        }
        if let Some(dial) = dial {
            self.change(Change::Lit(dial));
        }
    }

    fn text_spans(&mut self, ui: &mut Ui) {
        let object = self
            .object
            .map(|object| self.document.objects[object].name.clone());
        if !self.spans.follow(&self.document.path, object.as_deref()) {
            return;
        }
        ui.add(Dotted::new());
        let name = self.spans.object().unwrap_or_default().to_string();
        let (unit, speed) = if self.spans.overlay {
            ("px", 0.5)
        } else {
            ("m", 0.0005)
        };
        let decimals = self.spans.size_decimals();
        let mut finished = Vec::new();
        egui::ScrollArea::vertical()
            .id_salt("text spans")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = GAP;
                for span in 0..self.spans.rows.len() {
                    let row = &mut self.spans.rows[span];
                    let mut text_done = None;
                    labelled(ui, &format!("span {}", span + 1), |ui| {
                        let width = ui.available_width();
                        let id = Id::new(("manfred span", name.as_str(), span));
                        let response = text_field(ui, &mut row.text, "", width, Some(id));
                        if response.lost_focus() {
                            text_done = Some(ui.input(|input| input.key_pressed(Key::Escape)));
                        }
                    });
                    let mut size_done = false;
                    labelled(ui, "size", |ui| {
                        let response = ui.add(
                            Number::new(&mut row.size)
                                .unit(unit)
                                .speed(speed)
                                .decimals(decimals)
                                .width(ui.available_width()),
                        );
                        size_done = committed(&response);
                    });
                    let color_done = colour_row(ui, &mut row.color).is_some();
                    match text_done {
                        Some(true) => self.spans.cancel_text(span),
                        Some(false) => finished.extend(self.spans.finish_text(span)),
                        None => {}
                    }
                    if size_done {
                        finished.extend(self.spans.finish_size(span));
                    }
                    if color_done {
                        finished.extend(self.spans.finish_color(span));
                    }
                }
            });
        for (label, patches) in finished {
            self.edits.push(label, patches);
        }
    }

    fn render_panel(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = GAP;
        ui.add_space(4.0);
        let unrenderable = self.unrenderable();
        let renders = unrenderable.is_none();
        let camera = &mut self.render_camera;
        let cameras = &self.document.cameras;
        let before = *camera;
        labelled(ui, "camera", |ui| {
            if !renders {
                ui.disable();
            }
            ComboBox::from_id_salt("render camera")
                .width(ui.available_width())
                .selected_text(code(
                    ui,
                    cameras.get(*camera).map_or("—", |(name, _)| name.as_str()),
                    TEXT_CODE,
                ))
                .show_ui(ui, |ui| {
                    for (index, (name, _)) in cameras.iter().enumerate() {
                        ui.selectable_value(camera, index, name.as_str());
                    }
                });
        });
        self.render_chosen |= self.render_camera != before;
        let size = &mut self.size;
        labelled(ui, "size", |ui| {
            if !renders {
                ui.disable();
            }
            let width = ((ui.available_width() - GAP - 14.0) / 2.0).floor();
            ui.add(
                Number::new(&mut size[0])
                    .unit("px")
                    .speed(1.0)
                    .decimals(0)
                    .width(width),
            );
            ui.label(code(ui, "×", TEXT_MUTED));
            ui.add(
                Number::new(&mut size[1])
                    .unit("px")
                    .speed(1.0)
                    .decimals(0)
                    .width(width),
            );
        });
        let mut samples = self.shots.samples() as f32;
        labelled(ui, "samples", |ui| {
            if !renders {
                ui.disable();
            }
            ui.add(
                Number::new(&mut samples)
                    .unit("spp")
                    .speed(1.0)
                    .decimals(0)
                    .width(ui.available_width()),
            );
        });
        self.shots
            .set_samples(samples.round().clamp(1.0, 1_000_000.0) as u32);
        ui.add(Dotted::new());
        let busy = self.shots.busy();
        let (label, fill, text) = match (busy, renders) {
            (true, _) => (format!("{STOP}  Cancel"), BG_CHIP, TEXT_PRIMARY),
            (false, true) => (format!("{PLAY}  Render"), ACCENT, ACCENT_TEXT),
            (false, false) => (format!("{PLAY}  Render"), BG_CHIP, TEXT_DIM),
        };
        let button = Button::new(
            RichText::new(label)
                .font(Theme::of(ui.ctx()).bold_font(SIZE_BODY))
                .color(text),
        )
        .fill(fill)
        .stroke(Stroke::new(STROKE, if busy { LINE_STRONG } else { fill }))
        .corner_radius(CornerRadius::same(RADIUS))
        .min_size(Vec2::new(ui.available_width(), 28.0));
        let response = ui.add_enabled(busy || renders, button);
        if let Some(why) = unrenderable {
            response.clone().on_disabled_hover_text(why);
        }
        if response.clicked() {
            if busy {
                self.shots.cancel();
            } else {
                self.render();
            }
        }
        self.render_state(ui);
    }

    fn render_state(&mut self, ui: &mut Ui) {
        let Some(job) = self.shots.job().cloned() else {
            ui.label(code(ui, "no render yet", TEXT_DIM));
            return;
        };
        ui.horizontal(|ui| {
            let word = match (&job.phase, job.progress) {
                _ if job.cancelling && !job.phase.ended() => "cancelling".to_string(),
                (Phase::Running, Some((done, total))) => format!("running {done}/{total}"),
                (phase, _) => phase.word().to_string(),
            };
            ui.add(Pill::new(&word, phase_tone(&job.phase)));
            let detail = match &job.phase {
                Phase::Waiting(reason) => format!("for {reason}"),
                _ => format!("{} {}×{}", job.camera, job.size[0], job.size[1]),
            };
            ui.label(code(ui, detail, TEXT_MUTED));
        });
        if let Phase::Failed(reason) = &job.phase {
            ui.label(code(ui, first_line(reason, 120), DANGER));
        }
        let (Phase::Done, Some(views)) = (&job.phase, &self.shot_views) else {
            return;
        };
        let [width, height] = views.thumb.size();
        let aspect = width as f32 / height.max(1) as f32;
        let tall = (ui.available_width() / aspect).min(THUMB);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(tall * aspect, tall), Sense::click());
        ui.painter()
            .image(views.thumb.id(), rect, WHOLE, Color32::WHITE);
        ui.painter().rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(STROKE, LINE_BORDER),
            StrokeKind::Inside,
        );
        let clicked = response.clicked();
        ui.label(code(
            ui,
            tail_text(&job.out.display().to_string(), 40),
            TEXT_MUTED,
        ));
        if clicked {
            self.full = true;
        }
    }

    fn shading(&mut self, ui: &mut Ui, image: Rect) {
        if self.full || self.view_error.is_some() {
            return;
        }
        let mut right = image.right() - SECTION_GAP * 0.5;
        let top = image.bottom() - SECTION_GAP * 0.5 - ROW_HEIGHT;
        let mut toggle = |ui: &mut Ui, text: &str, on: bool| {
            let colour = if on { ACCENT } else { TEXT_SECONDARY };
            let font = Theme::of(ui.ctx()).font(SIZE_CODE);
            let width = ui
                .painter()
                .layout_no_wrap(text.to_string(), font, colour)
                .size()
                .x
                + 2.0 * SECTION_GAP;
            let rect =
                Rect::from_min_size(Pos2::new(right - width, top), Vec2::new(width, ROW_HEIGHT));
            right = rect.left() - ITEM_SPACING;
            ui.put(
                rect,
                Button::new(code(ui, text, colour))
                    .wrap_mode(egui::TextWrapMode::Extend)
                    .fill(if on { BG_SELECTED } else { BG_CHIP })
                    .stroke(Stroke::new(STROKE, if on { ACCENT } else { LINE_BORDER }))
                    .corner_radius(CornerRadius::same(RADIUS)),
            )
            .on_hover_text(if text == "wire" { "z" } else { "shift+z" })
            .clicked()
        };
        if toggle(ui, "wire", self.wire.on()) {
            self.toggle_wireframe();
        }
        if self.wire.on() && toggle(ui, "x-ray", self.wire.xray) {
            self.toggle_xray();
        }
    }

    fn shot_overlay(&self, ui: &Ui, image: Rect) {
        let (true, Some(views), Some(job)) = (self.full, &self.shot_views, self.shots.job()) else {
            return;
        };
        let painter = ui.painter();
        painter.rect_filled(image, CornerRadius::ZERO, BG_DEEP);
        let [width, height] = views.full.size();
        let fitted = viewport::fit(image, width as f32 / height.max(1) as f32);
        painter.image(views.full.id(), fitted, WHOLE, Color32::WHITE);
        viewport::frame(painter, fitted);
        let text = format!(
            "render {} {}×{}  ·  any key closes",
            job.camera, job.size[0], job.size[1]
        );
        band(
            painter,
            image.left_top() + Vec2::splat(SECTION_GAP),
            text,
            None,
        );
    }
}

enum Gesture {
    None,
    Set,
    New,
    Ask,
    Delete(String),
    Keep,
}

fn phase_tone(phase: &Phase) -> Tone {
    match phase {
        Phase::Queued | Phase::Waiting(_) | Phase::Cancelled => Tone::Neutral,
        Phase::Running => Tone::Busy,
        Phase::Done => Tone::Ok,
        Phase::Failed(_) => Tone::Danger,
    }
}

fn phase_colour(phase: &Phase) -> Color32 {
    match phase {
        Phase::Queued | Phase::Waiting(_) | Phase::Cancelled => TEXT_CODE,
        Phase::Running => ACCENT,
        Phase::Done => OK,
        Phase::Failed(_) => DANGER,
    }
}

pub(crate) fn tail_text(text: &str, most: usize) -> String {
    let count = text.chars().count();
    if count <= most {
        return text.to_string();
    }
    let kept: String = text.chars().skip(count - (most - 1)).collect();
    format!("…{kept}")
}

fn chip(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let colour = if enabled { TEXT_SECONDARY } else { TEXT_DIM };
    ui.add_enabled(
        enabled,
        Button::new(code(ui, text, colour))
            .fill(BG_CHIP)
            .stroke(Stroke::new(STROKE, LINE_BORDER))
            .corner_radius(CornerRadius::same(RADIUS)),
    )
}

pub(crate) struct Row<'a> {
    pub focused: bool,
    pub selected: bool,
    pub depth: usize,
    pub icon: (&'a str, Color32),
    pub label: &'a str,
    pub detail: &'a str,
    pub mark: Option<&'a str>,
}

pub(crate) fn list_row(
    ui: &mut Ui,
    focused: bool,
    selected: bool,
    depth: usize,
    icon: (&str, Color32),
    label: &str,
    detail: &str,
) -> Response {
    marked_row(
        ui,
        Row {
            focused,
            selected,
            depth,
            icon,
            label,
            detail,
            mark: None,
        },
    )
}

pub(crate) fn marked_row(ui: &mut Ui, row: Row) -> Response {
    let Row {
        focused,
        selected,
        depth,
        icon,
        label,
        detail,
        mark,
    } = row;
    let size = Vec2::new(ui.available_width(), ROW_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let lit = selected && focused;
    let fill = if lit {
        Some(ACCENT)
    } else if selected || response.hovered() {
        Some(BG_SELECTED)
    } else {
        None
    };
    let dim = mark.is_some();
    let painter = ui.painter();
    if let Some(fill) = fill {
        painter.rect_filled(rect, CornerRadius::same(RADIUS), fill);
    }
    let ink = |colour| if lit { ACCENT_TEXT } else { colour };
    let x = rect.left() + 4.0 + depth as f32 * SECTION_GAP;
    painter.text(
        Pos2::new(x, rect.center().y),
        Align2::LEFT_CENTER,
        icon.0,
        Theme::of(painter.ctx()).font(SIZE_BODY),
        ink(if dim { TEXT_DIM } else { icon.1 }),
    );
    painter.text(
        Pos2::new(x + 20.0, rect.center().y - 0.5),
        Align2::LEFT_CENTER,
        label,
        Theme::of(painter.ctx()).font(SIZE_BODY),
        ink(if dim { TEXT_MUTED } else { TEXT_PRIMARY }),
    );
    let mut right = rect.right() - 4.0;
    if !detail.is_empty() {
        let placed = painter.text(
            Pos2::new(right, rect.center().y - 0.5),
            Align2::RIGHT_CENTER,
            detail,
            Theme::of(painter.ctx()).font(SIZE_CODE),
            ink(TEXT_MUTED),
        );
        right = placed.left() - 6.0;
    }
    if let Some(mark) = mark {
        let galley = painter.layout_no_wrap(
            mark.to_string(),
            Theme::of(painter.ctx()).font(SIZE_CODE),
            ink(TEXT_MUTED),
        );
        let badge = Rect::from_center_size(
            Pos2::new(right - galley.size().x / 2.0 - 4.0, rect.center().y),
            galley.size() + Vec2::new(8.0, 2.0),
        );
        painter.rect_stroke(
            badge,
            CornerRadius::same(RADIUS),
            Stroke::new(STROKE, if lit { ACCENT_TEXT } else { LINE_BORDER }),
            egui::StrokeKind::Inside,
        );
        painter.galley(badge.center() - galley.size() / 2.0, galley, ACCENT_TEXT);
    }
    response
}

fn committed(response: &Response) -> bool {
    response.drag_stopped() || (response.changed() && !response.dragged())
}

fn triple_row(
    ui: &mut Ui,
    label: &str,
    triple: Triple,
    values: &mut [f32; 3],
    fields: Option<&Fields>,
) -> Option<(usize, f32)> {
    let mut done = None;
    labelled(ui, label, |ui| {
        let width = ((ui.available_width() - 2.0 * GAP) / 3.0).floor();
        for (axis, value) in values.iter_mut().enumerate() {
            let reference =
                fields.and_then(|fields| fields.params[triple as usize][axis].as_deref());
            if let Some(reference) = reference {
                params_slot(ui, reference, width);
                continue;
            }
            if let Some(reason) = fields.and_then(|fields| fields.locked) {
                let unit = fields.map_or("", |fields| fields.unit(triple, axis));
                locked_slot(ui, &format!("{value:.3}{unit}"), reason, width);
                continue;
            }
            let unit = match (fields, triple) {
                (Some(fields), _) => fields.unit(triple, axis),
                (None, Triple::Location) => "m",
                (None, Triple::Rotation) => "°",
                (None, Triple::Scale) => "",
            };
            let (decimals, speed) = match unit {
                "px" => (1, 0.5),
                "°" => (1, 0.1),
                _ => (3, 0.001),
            };
            let response = ui.add(
                Number::new(value)
                    .unit(unit)
                    .speed(speed)
                    .decimals(decimals)
                    .width(width),
            );
            if committed(&response) {
                done = Some((axis, *value));
            }
        }
    });
    done
}

fn dial_row(
    ui: &mut Ui,
    label: &str,
    unit: &str,
    decimals: usize,
    speed: f32,
    values: &mut [f32; 3],
) -> Option<(usize, f32)> {
    let mut done = None;
    labelled(ui, label, |ui| {
        let width = ((ui.available_width() - 2.0 * GAP) / 3.0).floor();
        for (axis, value) in values.iter_mut().enumerate() {
            let response = ui.add(
                Number::new(value)
                    .unit(unit)
                    .speed(speed)
                    .decimals(decimals)
                    .width(width),
            );
            if committed(&response) {
                done = Some((axis, *value));
            }
        }
    });
    done
}

fn colour_row(ui: &mut Ui, color: &mut [f32; 3]) -> Option<(usize, f32)> {
    let mut done = None;
    labelled(ui, "colour", |ui| {
        let swatch = ROW_HEIGHT;
        let width = ((ui.available_width() - swatch - 3.0 * GAP) / 3.0).floor();
        for (axis, channel) in color.iter_mut().enumerate() {
            let response = ui.add(
                Number::new(channel)
                    .speed(0.005)
                    .decimals(spans::COLOR_DECIMALS)
                    .width(width),
            );
            if committed(&response) {
                done = Some((axis, *channel));
            }
        }
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(swatch), Sense::hover());
        let [r, g, b] = color.map(|c| c.clamp(0.0, 1.0));
        ui.painter().rect(
            rect,
            CornerRadius::same(RADIUS),
            Color32::from(egui::Rgba::from_rgb(r, g, b)),
            Stroke::new(STROKE, LINE_BORDER),
            StrokeKind::Inside,
        );
    });
    done
}

fn dial_number(
    ui: &mut Ui,
    label: &str,
    unit: &str,
    decimals: usize,
    speed: f32,
    value: &mut f32,
) -> Option<f32> {
    let mut done = None;
    labelled(ui, label, |ui| {
        let response = ui.add(
            Number::new(value)
                .unit(unit)
                .speed(speed)
                .decimals(decimals)
                .width(ui.available_width()),
        );
        if committed(&response) {
            done = Some(*value);
        }
    });
    done
}

fn params_slot(ui: &mut Ui, reference: &str, width: f32) {
    let shown = reference.strip_prefix("params.").unwrap_or(reference);
    let hover = format!("{reference}: read only; the params file is its truth");
    read_slot(ui, shown, hover, width);
}

fn locked_slot(ui: &mut Ui, shown: &str, reason: &str, width: f32) {
    read_slot(ui, shown, format!("read only; {reason}"), width);
}

fn read_slot(ui: &mut Ui, shown: &str, hover: String, width: f32) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same(RADIUS),
            BG_BASE,
            Stroke::new(STROKE, LINE_SUBTLE),
            StrokeKind::Inside,
        );
        let text = painter.layout_no_wrap(
            shown.to_string(),
            Theme::of(painter.ctx()).font(SIZE_SMALL),
            TEXT_MUTED,
        );
        let inner = rect.shrink2(Vec2::new(6.0, 0.0));
        let at = Pos2::new(
            if text.size().x > inner.width() {
                inner.right() - text.size().x
            } else {
                inner.left()
            },
            rect.center().y - text.size().y / 2.0,
        );
        painter.with_clip_rect(inner).galley(at, text, TEXT_MUTED);
    }
    response.on_hover_text(hover);
}

fn labelled(ui: &mut Ui, label: &str, body: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(LABEL, ROW_HEIGHT), Sense::hover());
        ui.painter().text(
            rect.left_center() - Vec2::new(0.0, 0.5),
            Align2::LEFT_CENTER,
            label,
            Theme::of(ui.ctx()).font(SIZE_SMALL),
            TEXT_SECONDARY,
        );
        body(ui);
    });
}

fn item(ui: &mut Ui, label: &str, value: &str, colour: Color32) {
    ui.label(code(ui, label, TEXT_MUTED));
    ui.label(code(ui, value, colour));
}

fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 12.0), Sense::hover());
    ui.painter().vline(
        rect.center().x.floor() + 0.5,
        rect.y_range(),
        Stroke::new(STROKE, LINE_SUBTLE),
    );
}

fn scrolled(input: &egui::InputState, options: &egui::InputOptions) -> f32 {
    let page = input.viewport_rect().height();
    input
        .events
        .iter()
        .map(|event| match event {
            Event::MouseWheel {
                unit,
                delta,
                modifiers,
                ..
            } => {
                let delta = match unit {
                    egui::MouseWheelUnit::Point => *delta,
                    egui::MouseWheelUnit::Line => options.line_scroll_speed * *delta,
                    egui::MouseWheelUnit::Page => page * *delta,
                };
                let horizontal = modifiers.matches_any(options.horizontal_scroll_modifier);
                let vertical = modifiers.matches_any(options.vertical_scroll_modifier);
                if horizontal && !vertical {
                    0.0
                } else if !horizontal && vertical {
                    delta.x + delta.y
                } else {
                    delta.y
                }
            }
            _ => 0.0,
        })
        .sum()
}

pub(crate) fn text_field(
    ui: &mut Ui,
    text: &mut String,
    hint: &str,
    width: f32,
    id: Option<Id>,
) -> Response {
    let back = ui.painter().add(Shape::Noop);
    let mut edit = TextEdit::singleline(text)
        .frame(Frame::NONE)
        .margin(Margin::symmetric(6, 2))
        .desired_width(width - 12.0)
        .font(Theme::of(ui.ctx()).font(SIZE_CODE))
        .text_color(TEXT_CODE)
        .hint_text(code(ui, hint, TEXT_DIM));
    if let Some(id) = id {
        edit = edit.id(id);
    }
    let response = ui.add(edit);
    let border = if response.has_focus() {
        ACCENT
    } else if response.hovered() {
        LINE_STRONG
    } else {
        LINE_BORDER
    };
    ui.painter().set(
        back,
        RectShape::new(
            response.rect,
            CornerRadius::same(RADIUS),
            BG_DEEP,
            Stroke::new(STROKE, border),
            StrokeKind::Inside,
        ),
    );
    response
}

#[cfg(test)]
mod controls;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::follow::Mode;
    use crate::viewer::Viewer;
    use crate::{Script, scratch, theme};
    use egui::{Modifiers, RawInput};
    use pfx_editor_shell::drive;

    const SCENE: &str = r#"manfred = 1
name = "pair"

[[object]]
name = "table"
kind = "box"
size = [1.0, 0.1, 1.0]
material = "oak"

[[object]]
name = "cup"
kind = "box"
size = [0.1, 0.1, 0.1]
parent = "table"
material = "enamel"

[[camera]]
name = "front"
position = [0.0, 0.5, 2.0]
target = [0.0, 0.0, 0.0]
fov_y_deg = 30.0

[[camera]]
name = "side"
position = [2.0, 0.5, 0.0]
target = [0.0, 0.0, 0.0]
fov_y_deg = 30.0

[sun]
model = "daylight"
hour = 9.5
latitude = 52.0
heading = 0.0
"#;

    const SIZE: [u32; 2] = [1586, 992];

    fn editing(name: &str, text: &str) -> (Shell, Viewer, std::path::PathBuf) {
        let (mut shell, viewer, path) = live(name, text);
        shell.drains = true;
        (shell, viewer, path)
    }

    fn live(name: &str, text: &str) -> (Shell, Viewer, std::path::PathBuf) {
        let path = scratch(name);
        std::fs::write(&path, text).unwrap();
        let mut viewer = Viewer::new(&path, Mode::Fake);
        let mut shell = Shell::open(&path.display().to_string());
        viewer.sync(&mut shell);
        (shell, viewer, path)
    }

    fn offset(eye: &Eye) -> glam::DVec3 {
        glam::DVec3::from_array(eye.position) - glam::DVec3::from_array(eye.target)
    }

    fn run(shell: &mut Shell, size: [u32; 2], script: &str) {
        let ctx = Context::default();
        theme::apply_theme(&ctx, &theme::manfred());
        drive(&ctx, size, &Script::parse(script).unwrap(), |ui| {
            shell.ui(ui)
        });
    }

    #[test]
    fn a_code_jumps_focus() {
        let mut shell = Shell::new();
        assert_eq!(shell.focus(), Panel::C1);
        run(&mut shell, [1586, 992], "key b\nkey 1\nframe");
        assert_eq!(shell.focus(), Panel::B1);
        run(&mut shell, [1586, 992], "key c\nkey 3\nframe\nkey a\nframe");
        assert_eq!(shell.focus(), Panel::A);
    }

    #[test]
    fn typing_in_search_does_not_jump() {
        let mut shell = Shell::new();
        run(
            &mut shell,
            [1586, 992],
            "key /\nframe\ntext b1\nkey b\nkey 1\nframe",
        );
        assert_eq!(shell.focus(), Panel::A);
        assert_eq!(shell.search, "b1");
    }

    #[test]
    fn arrows_walk_the_outliner() {
        let mut shell = Shell::new();
        let knob = shell.object;
        run(
            &mut shell,
            [1586, 992],
            "key b\nkey 1\nkey ArrowDown\nframe",
        );
        assert_eq!(shell.name, "mv_knob__face");
        run(&mut shell, [1586, 992], "key ArrowUp\nkey ArrowUp\nframe");
        assert_eq!(shell.name, "slot_bezel");
        assert_ne!(shell.object, knob);
    }

    #[test]
    fn a_click_focuses_the_panel_under_it() {
        let mut shell = Shell::new();
        run(&mut shell, [1586, 992], "frame\nclick 1400 400\nframe");
        assert_eq!(shell.focus(), Panel::C2);
    }

    fn fills_c1(shell: &Shell, size: [u32; 2]) {
        let image = shell.viewport().unwrap();
        let c1 = shell
            .panels
            .iter()
            .find(|(panel, _)| *panel == Panel::C1)
            .unwrap()
            .1;
        let inner = c1.shrink(f32::from(PANEL_PADDING));
        assert!(
            inner.expand(1.0).contains_rect(image),
            "{size:?}: {image:?} in {c1:?}"
        );
        assert!(
            image.width() >= inner.width() - 1.0,
            "{size:?}: {image:?} in {c1:?}"
        );
        assert!(
            (image.bottom() - inner.bottom()).abs() <= 1.0,
            "{size:?}: {image:?} in {c1:?}"
        );
        assert!(
            image.top() - inner.top() < 40.0,
            "{size:?}: {image:?} in {c1:?}"
        );
        assert_eq!(image.min, image.min.round());
        assert_eq!(image.size(), image.size().round());
    }

    #[test]
    fn the_viewport_fills_c1_at_any_window_size() {
        for size in [[1586, 992], [1280, 720], [2560, 1440], [1000, 1400]] {
            let mut shell = Shell::new();
            run(&mut shell, size, "");
            fills_c1(&shell, size);
            let (mut live, _, _) = live("shell-fill-c1", SCENE);
            run(&mut live, size, "frame");
            fills_c1(&live, size);
            assert!(live.camera_frame().is_none());
            let image = live.viewport().unwrap();
            let lens = live.lens().unwrap();
            let home = live.home();
            let target = glam::DVec3::from_array(home.target);
            let middle = lens.project(target).unwrap();
            assert!(
                (middle - image.center()).length() < 1.0,
                "{size:?}: {middle:?}"
            );
            let frame = viewport::fit(image, live.document().aspect);
            let basis = orbit::basis(&home);
            let distance = (glam::DVec3::from_array(home.position) - target).length();
            let edge = target + basis.up * distance * (home.fov_y_deg.to_radians() / 2.0).tan();
            let top = lens.project(edge).unwrap();
            assert!(
                (top.y - frame.top()).abs() < 1.0,
                "{size:?}: {top:?} {frame:?}"
            );
        }
    }

    #[test]
    fn zero_looks_through_the_camera_with_the_render_frame_and_an_orbit_leaves_it() {
        let (mut shell, _, _) = live("shell-through", SCENE);
        run(&mut shell, SIZE, "frame");
        assert!(shell.through().is_none());
        assert!(shell.view_info().starts_with("orbit from"));
        run(&mut shell, SIZE, "key 0\nframe");
        assert_eq!(shell.view_info(), "front");
        let image = shell.viewport().unwrap();
        let frame = shell.camera_frame().unwrap();
        assert!(image.contains_rect(frame), "{frame:?} in {image:?}");
        let ratio = frame.width() / frame.height();
        assert!((ratio - 1586.0 / 992.0).abs() < 0.01, "{frame:?}");
        let camera = shell.through().unwrap().clone();
        let eye = Eye::from_camera(&camera, f64::from(ratio));
        let lens = shell.lens().unwrap();
        let target = glam::DVec3::from_array(eye.target);
        let basis = orbit::basis(&eye);
        let distance = (glam::DVec3::from_array(eye.position) - target).length();
        let half = distance * (eye.fov_y_deg.to_radians() / 2.0).tan();
        let top = lens.project(target + basis.up * half).unwrap();
        let right = lens
            .project(target + basis.right * half * f64::from(ratio))
            .unwrap();
        assert!((top.y - frame.top()).abs() < 1.0, "{top:?} {frame:?}");
        assert!((right.x - frame.right()).abs() < 1.0, "{right:?} {frame:?}");
        run(&mut shell, SIZE, "key c\nkey 3\nframe");
        shell.size = [800.0, 800.0];
        run(&mut shell, SIZE, "frame");
        let square = shell.camera_frame().unwrap();
        assert!(
            (square.width() - square.height()).abs() <= 1.0,
            "{square:?}"
        );
        run(&mut shell, SIZE, "drag 700 400 760 400\nframe");
        assert!(shell.through().is_none());
        assert!(shell.camera_frame().is_none());
        assert!(shell.view_info().starts_with("orbit from"));
        run(
            &mut shell,
            SIZE,
            "key c\nkey 1\nkey 0\nframe\nscroll 700 400 2\nframe",
        );
        assert!(shell.through().is_none());
    }

    #[test]
    fn the_timeline_bar_plays_steps_and_scrubs_and_a_rebuild_keeps_the_time() {
        let (mut shell, _, _) = live("shell-timeline", SCENE);
        let span = Span {
            fps: 10,
            frames: 20,
            aspect: 0.5625,
        };
        let mut document = shell.document().clone();
        document.timeline = Some(span);
        shell.place(document.clone(), &[]);
        let size = [1586, 992];
        run(
            &mut shell,
            size,
            "key Period\nkey Period\nkey Period\nkey Comma\nframe",
        );
        assert!((shell.time().unwrap() - 0.2).abs() < 1e-9);
        let bar = shell.scrubber.unwrap();
        assert!(bar.top() >= shell.viewport().unwrap().bottom());
        let at = bar.left() + bar.width() * 0.75;
        run(
            &mut shell,
            size,
            &format!("click {at} {}\nframe", bar.center().y),
        );
        assert!((shell.time().unwrap() - 1.5).abs() < 0.02);
        run(&mut shell, size, "key Space\nframe");
        assert!(shell.clock().playing);
        run(&mut shell, size, "key Space\nframe");
        assert!(!shell.clock().playing);
        let kept = shell.time().unwrap();
        shell.place(document.clone(), &[]);
        assert_eq!(shell.time(), Some(kept));
        document.timeline = Some(Span { frames: 10, ..span });
        shell.place(document, &[]);
        assert_eq!(shell.time(), Some(1.0));
        let orbit = shell.view_eye().unwrap();
        let chase = Eye {
            position: [0.0, 1.0, 5.0],
            target: [0.0, 1.0, 4.0],
            ..orbit
        };
        shell.set_chase_eye(Some(chase));
        run(&mut shell, size, "key f\nframe");
        assert!(shell.chasing());
        assert_eq!(shell.view_info(), "chase cam");
        assert_eq!(shell.view_eye().unwrap().position, chase.position);
        let frame = shell.camera_frame().unwrap();
        assert!((frame.width() / frame.height() - 0.5625).abs() < 0.01);
        let middle = shell.viewport().unwrap().left_top() + Vec2::new(80.0, 80.0);
        run(
            &mut shell,
            size,
            &format!(
                "drag {} {} {} {}\nframe",
                middle.x,
                middle.y,
                middle.x + 40.0,
                middle.y
            ),
        );
        assert!(!shell.clock().chase);
        assert_ne!(shell.view_eye().unwrap().position, chase.position);
    }

    #[test]
    fn a_recipe_fills_the_outliner_cameras_and_eye() {
        let (shell, _, _) = live("shell-fill", SCENE);
        let document = shell.document();
        assert_eq!(document.name, "pair");
        let labels: Vec<(usize, &str)> = document
            .nodes
            .iter()
            .map(|node| (node.depth, node.label.as_str()))
            .collect();
        assert_eq!(labels, [(0, "table"), (1, "cup")]);
        assert_eq!(document.cameras.len(), 2);
        assert_eq!(shell.eye(), Some(shell.home()));
        assert_eq!(shell.hour, 9.5);
        assert_eq!(shell.hour(), None);
        assert_eq!(shell.name, "table");
    }

    #[test]
    fn a_drag_in_the_viewport_orbits_by_its_length() {
        let (mut shell, _, _) = live("shell-orbit", SCENE);
        let home = shell.eye().unwrap();
        run(&mut shell, SIZE, "frame\ndrag 700 400 820 400\nframe");
        let eye = shell.eye().unwrap();
        let level = |e: &Eye| offset(e) * glam::DVec3::new(1.0, 0.0, 1.0);
        let angle = level(&home).angle_between(level(&eye)).to_degrees();
        assert!(
            (angle - 120.0 * orbit::DEGREES_PER_POINT).abs() < 1e-4,
            "{angle}"
        );
        assert!((offset(&home).y - offset(&eye).y).abs() < 1e-9);
        assert_eq!(eye.target, home.target);
        assert_eq!(shell.focus(), Panel::C1);
        assert!(shell.view_info().starts_with("orbit from"));
    }

    #[test]
    fn shift_or_middle_drags_pan_and_the_wheel_dollies() {
        let (mut shell, _, _) = live("shell-pan", SCENE);
        let home = shell.eye().unwrap();
        run(&mut shell, SIZE, "frame\ndrag 700 400 760 400 shift\nframe");
        let panned = shell.eye().unwrap();
        assert_ne!(panned.target, home.target);
        assert!((offset(&panned) - offset(&home)).length() < 1e-9);
        run(
            &mut shell,
            SIZE,
            "frame\ndrag 700 400 700 460 middle\nframe",
        );
        let again = shell.eye().unwrap();
        assert_ne!(again.target, panned.target);
        run(&mut shell, SIZE, "frame\nscroll 700 400 3\nframe");
        let nearer = shell.eye().unwrap();
        assert!(offset(&nearer).length() < offset(&again).length());
        assert_eq!(nearer.target, again.target);
    }

    #[test]
    fn a_camera_in_b2_jumps_and_zero_returns_to_it() {
        let (mut shell, _, _) = live("shell-camera", SCENE);
        run(&mut shell, SIZE, "frame\ndrag 700 400 900 300\nframe");
        assert_ne!(shell.eye(), Some(shell.home()));
        run(&mut shell, SIZE, "key 0\nframe");
        assert_eq!(shell.eye(), Some(shell.home()));
        run(&mut shell, SIZE, "key b\nkey 2\nkey ArrowDown\nframe");
        assert_eq!(shell.camera, 1);
        assert_eq!(shell.eye(), Some(shell.home()));
        run(&mut shell, SIZE, "drag 700 400 900 300\nkey 0\nframe");
        assert_eq!(shell.camera, 1);
        assert_eq!(shell.eye(), Some(shell.home()));
    }

    #[test]
    fn c3_follows_the_camera_in_b2_until_it_is_chosen() {
        let (mut shell, _, _) = live("shell-render-follow", SCENE);
        run(&mut shell, SIZE, "frame");
        assert_eq!(shell.render_camera, 0);
        run(&mut shell, SIZE, "key b\nkey 2\nkey ArrowDown\nframe");
        assert_eq!((shell.camera, shell.render_camera), (1, 1));
        run(&mut shell, SIZE, "key c\nkey 3\nkey ArrowUp\nframe");
        assert_eq!((shell.camera, shell.render_camera), (1, 0));
        run(
            &mut shell,
            SIZE,
            "key b\nkey 2\nkey ArrowUp\nkey ArrowDown\nframe",
        );
        assert_eq!((shell.camera, shell.render_camera), (1, 0));
    }

    #[test]
    fn the_hour_slider_reaches_the_stage_only_once_moved() {
        let (mut shell, _, _) = live("shell-hour", SCENE);
        run(&mut shell, SIZE, "key b\nkey 3\nkey ArrowRight\nframe");
        assert_eq!(shell.hour(), Some(9.75));
    }

    const SLIDE: &str = "frame\nkey b\nkey 3\nkey ArrowUp\nframe\ndrag 60 904 80 904\nframe";
    const USE_HOUR: &str = "click 228 905\nframe\nframe";

    fn with_lamps(text: &str) -> String {
        format!(
            "{text}\n[sky]\nkind = \"analytic\"\n\n[[light]]\nname = \"lamp\"\nposition = [0.0, 1.0, 0.0]\nintensity = 20.0\n"
        )
    }

    fn shown(shell: &Shell) -> f32 {
        shell.hour().expect("the slider was moved") as f32
    }

    fn painted(shape: &egui::Shape, into: &mut Vec<(String, Color32, Pos2)>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| painted(shape, into)),
            egui::Shape::Text(text) => {
                let colour = text.galley.job.sections[0].format.color;
                into.push((text.galley.text().to_string(), colour, text.pos));
            }
            _ => {}
        }
    }

    #[test]
    fn a_copy_in_the_outliner_is_dimmed_and_badged_with_its_use() {
        let path = scratch("shell-copy-badge");
        std::fs::write(
            path.with_file_name("knob.toml"),
            "manfred = 1\n\n[[object]]\nname = \"cap\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n",
        )
        .unwrap();
        std::fs::write(
            &path,
            "manfred = 1\nname = \"panel\"\n\n[[object]]\nname = \"board\"\nkind = \"box\"\nsize = [1.0, 0.1, 1.0]\n\n[[use]]\nfile = \"knob.toml\"\nprefix = \"left_\"\n",
        )
        .unwrap();
        let mut viewer = Viewer::new(&path, Mode::Fake);
        let mut shell = Shell::open(&path.display().to_string());
        viewer.sync(&mut shell);
        let board = shell.document.object("board").unwrap();
        let cap = shell.document.object("left_cap").unwrap();
        assert_eq!(shell.copy_note(board), None);
        assert_eq!(
            shell.copy_note(cap).as_deref(),
            Some("a copy, read only: use[left_] places it from knob.toml")
        );
        let ctx = Context::default();
        theme::apply_theme(&ctx, &theme::manfred());
        let output = drive(&ctx, [1586, 992], &Script::default(), |ui| shell.ui(ui));
        let mut texts = Vec::new();
        for clipped in &output.shapes {
            painted(&clipped.shape, &mut texts);
        }
        let outliner = shell
            .panels
            .iter()
            .find(|(panel, _)| *panel == Panel::B1)
            .map(|(_, rect)| *rect)
            .unwrap();
        let colour = |text: &str| {
            texts
                .iter()
                .filter(|(shown, _, at)| shown == text && outliner.contains(*at))
                .map(|(_, colour, _)| *colour)
                .collect::<Vec<_>>()
        };
        assert_eq!(colour("use").len(), 1, "{texts:?}");
        assert_eq!(colour("left_cap"), [TEXT_MUTED]);
        assert_eq!(colour("board"), [TEXT_PRIMARY]);
    }

    #[test]
    fn use_this_hour_writes_the_shown_hour_and_undo_puts_both_back() {
        let text = with_lamps(SCENE);
        let (mut shell, _, path) = editing("shell-hour-write", &text);
        run(&mut shell, SIZE, SLIDE);
        let moved = shown(&shell);
        assert_ne!(moved, 9.5);
        assert!(shell.hour_kept().is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        run(&mut shell, SIZE, USE_HOUR);
        let wanted = (moved * 100.0).round() / 100.0;
        let edited = std::fs::read_to_string(&path).unwrap();
        assert!(edited.contains(&format!("hour = {wanted}\n")), "{edited}");
        assert_eq!(d1(&shell), Some(Said::Patched("hour · sun.hour".into())));
        assert_eq!(shell.document().hour, Some(wanted));
        assert_eq!(shell.hour, wanted);
        assert_eq!(shell.hour(), None);
        assert!(shell.hour_kept().is_some());
        undo(&mut shell);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(shell.document().hour, Some(9.5));
        assert_eq!(shell.hour, 9.5);
        assert_eq!(shell.hour(), None);
        redo(&mut shell, Key::Y);
        assert_eq!(shell.document().hour, Some(wanted));
        assert_eq!(shell.hour, wanted);
        assert_eq!(shell.hour(), None);
    }

    #[test]
    fn use_this_hour_is_off_while_the_recipe_has_that_hour() {
        let text = with_lamps(SCENE);
        let (mut shell, _, path) = editing("shell-hour-same", &text);
        run(&mut shell, SIZE, "frame\nkey b\nkey 3\nframe");
        assert_eq!(
            shell.hour_kept(),
            Some(("Use hour", "the recipe has this hour already"))
        );
        run(&mut shell, SIZE, USE_HOUR);
        assert!(shell.edits().is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(d1(&shell), None);
        run(&mut shell, SIZE, "key ArrowRight\nframe");
        assert!(shell.hour_kept().is_none());
        run(&mut shell, SIZE, "key ArrowLeft\nframe");
        assert!(shell.hour_kept().is_some());
    }

    #[test]
    fn use_this_hour_says_why_when_the_sun_has_no_hour() {
        let text = sun_and_sky();
        let (mut shell, _, path) = editing("shell-hour-authored", &text);
        run(
            &mut shell,
            SIZE,
            "frame\nkey b\nkey 3\nkey ArrowRight\nframe",
        );
        assert_eq!(
            shell.hour_kept(),
            Some(("authored sun", "the authored sun has no hour of its own"))
        );
        shell.use_hour();
        run(&mut shell, SIZE, "frame");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(
            d1(&shell),
            Some(Said::Refused(
                "hour: the authored sun has no hour of its own; it follows the hour of the shot"
                    .into()
            ))
        );
        let bare = SCENE.replace(
            "[sun]\nmodel = \"daylight\"\nhour = 9.5\nlatitude = 52.0\nheading = 0.0\n",
            "",
        );
        let (mut shell, _, _) = editing("shell-hour-none", &bare);
        run(
            &mut shell,
            SIZE,
            "frame\nkey b\nkey 3\nkey ArrowRight\nframe",
        );
        assert_eq!(
            shell.hour_kept(),
            Some((
                "no [sun]",
                "the recipe has no daylight [sun] to give an hour"
            ))
        );
    }

    #[test]
    fn use_this_hour_refuses_a_params_driven_hour() {
        let path = scratch("shell-hour-params");
        let text = with_lamps(SCENE)
            .replace("hour = 9.5", "hour = \"${params.day.hour}\"")
            .replace(
                "manfred = 1\n",
                "manfred = 1\nparams = \"day.params.json\"\n",
            );
        std::fs::write(&path, &text).unwrap();
        std::fs::write(
            path.with_file_name("day.params.json"),
            r#"{ "day": { "hour": 9.5 } }"#,
        )
        .unwrap();
        let mut viewer = Viewer::new(&path, Mode::Fake);
        let mut shell = Shell::open(&path.display().to_string());
        viewer.sync(&mut shell);
        shell.drains = true;
        run(&mut shell, SIZE, SLIDE);
        let moved = shown(&shell);
        run(&mut shell, SIZE, USE_HOUR);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(
            d1(&shell),
            Some(Said::Refused(
                "hour: sun.hour comes from params.day.hour; change it in the params file".into()
            ))
        );
        assert_eq!(shell.hour(), Some(f64::from(moved)));
    }

    #[test]
    fn a_file_change_reloads_and_keeps_the_orbit() {
        let (mut shell, mut viewer, path) = live("shell-reload", SCENE);
        run(&mut shell, SIZE, "frame\ndrag 700 400 800 400\nframe");
        let eye = shell.eye();
        let more = SCENE.replace(
            "[[camera]]\nname = \"front\"",
            "[[object]]\nname = \"saucer\"\nkind = \"disc\"\nradius = 0.1\nparent = \"cup\"\n\n[[camera]]\nname = \"front\"",
        );
        std::fs::write(&path, more).unwrap();
        assert!(viewer.stale());
        assert!(viewer.reload(&mut shell));
        let labels: Vec<&str> = shell
            .document()
            .nodes
            .iter()
            .map(|node| node.label.as_str())
            .collect();
        assert_eq!(labels, ["table", "cup", "saucer"]);
        assert_eq!(shell.eye(), eye);
        assert_eq!(shell.trouble(), None);
        assert_eq!(shell.name, "table");
    }

    #[test]
    fn a_failed_reload_keeps_the_last_view_and_says_why_in_d1() {
        let (mut shell, mut viewer, path) = live("shell-broken", SCENE);
        run(
            &mut shell,
            SIZE,
            "frame\nkey b\nkey 1\nkey ArrowDown\nframe",
        );
        assert_eq!(shell.name, "cup");
        let before = shell.document().clone();
        std::fs::write(&path, "manfred = 1\nname = \"pair\"\n[[object]\n").unwrap();
        assert!(viewer.reload(&mut shell));
        assert_eq!(shell.document(), &before);
        assert_eq!(shell.name, "cup");
        assert!(
            shell.trouble().unwrap().contains("TOML"),
            "{:?}",
            shell.trouble()
        );
        run(&mut shell, SIZE, "frame");
        std::fs::write(&path, SCENE).unwrap();
        assert!(viewer.reload(&mut shell));
        assert_eq!(shell.trouble(), None);
        assert_eq!(shell.name, "cup");
    }

    #[test]
    fn a_game_in_front_pauses_the_view() {
        let (mut shell, _, _) = live("shell-play", SCENE);
        shell.set_pgpu(Pgpu::Play);
        assert!(shell.paused());
        assert!(shell.view_info().ends_with("· paused"));
        shell.set_pgpu(Pgpu::Queued);
        assert!(!shell.paused());
    }

    #[test]
    fn a_rebuild_runs_off_the_ui_thread_and_d1_says_so() {
        let (mut shell, mut viewer, path) = live("shell-request", SCENE);
        std::fs::write(&path, SCENE.replace("name = \"pair\"", "name = \"later\"")).unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        viewer.request(&mut shell, move || send.send(()).unwrap());
        assert!(shell.busy());
        assert_eq!(shell.document().name, "pair");
        receive.recv().unwrap();
        assert!(viewer.settle(&mut shell));
        assert!(!shell.busy());
        assert_eq!(shell.document().name, "later");
    }

    const NAME: &str = "click 1468 98";
    const MATERIAL: &str = "click 1450 224";

    fn cell(row: usize, axis: usize) -> String {
        format!("{} {}", 1398 + 70 * axis, 136 + 25 * row)
    }

    fn chord(shell: &mut Shell, key: Key, modifiers: Modifiers) {
        let ctx = Context::default();
        theme::apply_theme(&ctx, &theme::manfred());
        let event = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(SIZE[0] as f32, SIZE[1] as f32),
            )),
            events: vec![
                Event::ModifiersChanged(modifiers),
                event(true),
                event(false),
            ],
            ..RawInput::default()
        };
        ctx.run_ui(input, |ui| shell.ui(ui)).textures_delta.clear();
    }

    fn undo(shell: &mut Shell) {
        chord(shell, Key::Z, Modifiers::CTRL);
    }

    fn redo(shell: &mut Shell, key: Key) {
        let modifiers = if key == Key::Z {
            Modifiers::CTRL | Modifiers::SHIFT
        } else {
            Modifiers::CTRL
        };
        chord(shell, key, modifiers);
    }

    fn typed(field: &str, erase: usize, text: &str) -> String {
        format!(
            "frame\n{field}\nframe\nkey End\n{}text {text}\nkey Enter\nframe",
            "key Backspace\n".repeat(erase)
        )
    }

    fn d1(shell: &Shell) -> Option<Said> {
        shell.said().cloned()
    }

    #[test]
    fn a_press_and_release_in_one_frame_open_a_number() {
        let (mut shell, _, path) = editing("shell-c2-one-frame", SCENE);
        let ctx = Context::default();
        theme::apply_theme(&ctx, &theme::manfred());
        let screen = Some(Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(SIZE[0] as f32, SIZE[1] as f32),
        ));
        let at = Pos2::new(1468.0, 136.0);
        let pass = |shell: &mut Shell, events: Vec<Event>, time: f64| {
            let input = RawInput {
                screen_rect: screen,
                time: Some(time),
                events,
                ..RawInput::default()
            };
            ctx.run_ui(input, |ui| shell.ui(ui)).textures_delta.clear();
        };
        pass(&mut shell, vec![], 0.0);
        pass(&mut shell, vec![Event::PointerMoved(at)], 0.1);
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        pass(&mut shell, vec![button(true), button(false)], 0.2);
        pass(&mut shell, vec![], 0.3);
        let key = |key| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let mut events = vec![key(Key::Backspace); 5];
        events.push(Event::Text("0.5".into()));
        events.push(key(Key::Enter));
        pass(&mut shell, events, 0.4);
        pass(&mut shell, vec![], 0.5);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("at = [0.0, 0.5, 0.0]"), "{text}");
    }

    #[test]
    fn a_typed_location_lands_in_the_file_and_the_keys_undo_and_redo_it() {
        let (mut shell, _, path) = editing("shell-c2-location", SCENE);
        let script = format!(
            "frame\nclick {}\n{}text 0.25\nkey Enter\nframe",
            cell(0, 0),
            "key Backspace\n".repeat(5)
        );
        run(&mut shell, SIZE, &script);
        let edited = std::fs::read_to_string(&path).unwrap();
        assert!(
            edited.contains("material = \"oak\"\nat = [0.25, 0.0, 0.0]\n"),
            "{edited}"
        );
        assert_eq!(
            d1(&shell),
            Some(Said::Patched("location · object[table].at".into()))
        );
        assert_eq!(shell.location, [0.25, 0.0, 0.0]);
        assert_eq!(shell.document().objects[0].location, [0.25, 0.0, 0.0]);
        undo(&mut shell);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SCENE);
        assert_eq!(shell.location, [0.0, 0.0, 0.0]);
        assert_eq!(
            d1(&shell),
            Some(Said::Patched("undo location · object[table].at".into()))
        );
        redo(&mut shell, Key::Z);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
        undo(&mut shell);
        redo(&mut shell, Key::Y);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
        assert_eq!(shell.editor().unwrap().steps(), (1, 0));
    }

    fn edit_by_script(shell: &mut Shell) {
        let script = format!(
            "frame\nclick {}\n{}text 0.25\nkey Enter\nframe",
            cell(0, 0),
            "key Backspace\n".repeat(5)
        );
        run(shell, SIZE, &script);
    }

    #[test]
    fn scripted_chords_undo_and_redo_a_c2_edit_on_disk() {
        let (mut shell, _, path) = editing("polish-c2-chords", SCENE);
        edit_by_script(&mut shell);
        let edited = std::fs::read_to_string(&path).unwrap();
        assert_ne!(edited, SCENE);
        assert_eq!(shell.location, [0.25, 0.0, 0.0]);
        run(&mut shell, SIZE, "key ctrl+z\nframe");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SCENE);
        assert_eq!(shell.location, [0.0, 0.0, 0.0]);
        run(&mut shell, SIZE, "key ctrl+shift+z\nframe");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
        assert_eq!(shell.location, [0.25, 0.0, 0.0]);
        run(&mut shell, SIZE, "key ctrl+z\nframe");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SCENE);
        run(&mut shell, SIZE, "key ctrl+y\nframe");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
        assert_eq!(shell.editor().unwrap().steps(), (1, 0));
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn the_c2_undo_shot_shows_the_edit_taken_back() {
        let since = std::time::Instant::now();
        let (mut shell, _, path) = editing("polish-c2-undo", SCENE);
        edit_by_script(&mut shell);
        let edited = std::fs::read_to_string(&path).unwrap();
        assert_ne!(edited, SCENE);
        let script = Script::parse("key ctrl+z\nframe").unwrap();
        let png = crate::shot(&mut crate::Manfred::new(&mut shell, None), SIZE, &script).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SCENE);
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        std::fs::write(root.join("tmp/polish-c2-undo.png"), png).unwrap();
        let script = Script::parse("key ctrl+shift+z\nframe").unwrap();
        crate::shot(&mut crate::Manfred::new(&mut shell, None), SIZE, &script).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
        crate::gpu_turn(since);
    }

    #[test]
    fn a_drag_on_a_number_is_one_step_on_release() {
        let (mut shell, _, path) = editing("shell-c2-drag", SCENE);
        let at = cell(1, 1);
        let to = cell(1, 1).replace(" ", "x");
        let (x, y) = to.split_once('x').unwrap();
        let script = format!(
            "frame\ndrag {at} {} {y}\nframe",
            x.parse::<u32>().unwrap() + 60
        );
        run(&mut shell, SIZE, &script);
        assert_eq!(shell.editor().unwrap().steps(), (1, 0));
        let text = std::fs::read_to_string(&path).unwrap();
        let line = text
            .lines()
            .find(|line| line.starts_with("rotate = "))
            .unwrap();
        let turned: f64 = line
            .trim_start_matches("rotate = [0.0, ")
            .trim_end_matches(", 0.0]")
            .parse()
            .unwrap();
        assert!(turned > 1.0 && turned <= 6.0, "{line}");
        undo(&mut shell);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SCENE);
    }

    #[test]
    fn a_rename_in_c2_updates_the_parent_and_keeps_the_selection() {
        let (mut shell, _, path) = editing("shell-c2-rename", SCENE);
        run(&mut shell, SIZE, &typed(NAME, 5, "desk"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("name = \"desk\""), "{text}");
        assert!(text.contains("parent = \"desk\""), "{text}");
        assert_eq!(shell.name, "desk");
        assert_eq!(shell.document().nodes[0].label, "desk");
        assert_eq!(
            d1(&shell),
            Some(Said::Patched("rename · object[table].name +1".into()))
        );
        undo(&mut shell);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SCENE);
        assert_eq!(shell.name, "table");
    }

    #[test]
    fn a_rename_to_a_name_in_use_is_refused_in_d1() {
        let (mut shell, _, path) = editing("shell-c2-taken", SCENE);
        run(&mut shell, SIZE, &typed(NAME, 5, "cup"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SCENE);
        assert_eq!(
            d1(&shell),
            Some(Said::Refused(
                "rename: `cup` is already the name of another object".into()
            ))
        );
        assert_eq!(shell.name, "table");
        assert_eq!(shell.editor().unwrap().steps(), (0, 0));
    }

    #[test]
    fn a_typed_material_and_the_smoothing_land_in_the_file() {
        let (mut shell, _, path) = editing("shell-c2-material", SCENE);
        run(&mut shell, SIZE, &typed(MATERIAL, 3, "walnut"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("material = \"walnut\"\n\n[[object]]\nname = \"cup\""));
        shell.change(Change::Smoothing(Smoothing::Angle(35.0)));
        run(&mut shell, SIZE, "frame");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("material = \"walnut\"\nsmooth_angle = 35.0\n"),
            "{text}"
        );
        assert_eq!(shell.smoothing, 2);
        undo(&mut shell);
        undo(&mut shell);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SCENE);
    }

    fn lit_scene() -> String {
        format!(
            "{SCENE}\n[[light]]\nname = \"lamp\"\nposition = [0.2, 0.3, 0.1]\nintensity = 20.0\nshadow = true\n\n[[light]]\nname = \"fill\"\nposition = [-1.0, 1.0, 0.0]\nintensity = 5.0\n"
        )
    }

    fn retyped(row: usize, text: &str) -> String {
        format!(
            "frame\nclick {}\nkey End\n{}text {text}\nkey Enter\nframe",
            cell(row, 0),
            "key Backspace\n".repeat(10)
        )
    }

    #[test]
    fn a_light_picked_in_b3_shows_in_c2_and_a_typed_intensity_lands() {
        let text = lit_scene();
        let (mut shell, _, path) = editing("shell-c2-light", &text);
        assert!(!shell.lit_shown);
        run(
            &mut shell,
            SIZE,
            "frame\nkey b\nkey 3\nkey ArrowDown\nframe",
        );
        assert!(shell.lit_shown);
        assert_eq!(
            shell.lit.as_ref().map(|fields| fields.lit.clone()),
            Some(Lit::Light("lamp".into()))
        );
        assert_eq!(shell.dials.strength, 20.0);
        run(&mut shell, SIZE, &retyped(2, "12.5"));
        let edited = std::fs::read_to_string(&path).unwrap();
        assert!(
            edited.contains("intensity = 12.5\nshadow = true"),
            "{edited}"
        );
        assert_eq!(
            d1(&shell),
            Some(Said::Patched("intensity · light[lamp].intensity".into()))
        );
        assert!(shell.lit_shown);
        assert_eq!(shell.document().lights[shell.light].1, "lamp");
        assert_eq!(shell.dials.strength, 12.5);
        undo(&mut shell);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(shell.dials.strength, 20.0);
        redo(&mut shell, Key::Z);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
    }

    #[test]
    fn c2_writes_a_lights_position_colour_range_and_shadow() {
        let text = lit_scene();
        let (mut shell, _, path) = editing("shell-c2-light-dials", &text);
        run(
            &mut shell,
            SIZE,
            "frame\nkey b\nkey 3\nkey ArrowDown\nkey ArrowDown\nframe",
        );
        assert_eq!(shell.document().lights[shell.light].1, "fill");
        run(&mut shell, SIZE, &retyped(0, "-0.5"));
        run(
            &mut shell,
            SIZE,
            &format!("frame\nclick {}\nframe", cell(4, 0)),
        );
        shell.change(Change::Lit(Dial::Color(1, 0.5)));
        shell.change(Change::Lit(Dial::Range(2.0)));
        run(&mut shell, SIZE, "frame");
        let edited = std::fs::read_to_string(&path).unwrap();
        assert!(
            edited.ends_with(
                "name = \"fill\"\nposition = [-0.5, 1.0, 0.0]\nintensity = 5.0\nshadow = true\ncolor = [1.0, 0.5, 1.0]\nrange = 2.0\n"
            ),
            "{edited}"
        );
        assert!(shell.dials.shadow);
        assert_eq!(shell.dials.color, [1.0, 0.5, 1.0]);
        assert_eq!(shell.editor().unwrap().steps(), (4, 0));
        for _ in 0..4 {
            undo(&mut shell);
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn a_refused_light_edit_puts_c2_back_and_an_object_pick_shows_the_object() {
        let text = lit_scene();
        let (mut shell, _, path) = editing("shell-c2-light-refused", &text);
        run(
            &mut shell,
            SIZE,
            "frame\nkey b\nkey 3\nkey ArrowDown\nframe",
        );
        run(&mut shell, SIZE, &retyped(3, "-2"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(
            d1(&shell),
            Some(Said::Refused(
                "range: light[lamp].range: expected a number from 0 up".into()
            ))
        );
        assert_eq!(shell.dials.range, 0.0);
        run(
            &mut shell,
            SIZE,
            "frame\nkey b\nkey 1\nkey ArrowDown\nframe",
        );
        assert!(!shell.lit_shown);
        assert_eq!(shell.name, "cup");
        run(&mut shell, SIZE, &format!("frame\n{NAME}\nframe"));
        assert_eq!(shell.focus(), Panel::C2);
    }

    fn sun_and_sky() -> String {
        SCENE.replace(
            "[sun]\nmodel = \"daylight\"\nhour = 9.5\nlatitude = 52.0\nheading = 0.0\n",
            "[sun]\nmodel = \"authored\"\ntoward = [0.3, 0.8, 0.5]\nirradiance = 3.0\n\n[sky]\nkind = \"analytic\"\n",
        )
    }

    #[test]
    fn c2_writes_the_suns_irradiance_and_colour() {
        let text = sun_and_sky();
        let (mut shell, _, path) = editing("shell-c2-sun", &text);
        run(&mut shell, SIZE, "frame\nkey b\nkey 3\nkey ArrowUp\nframe");
        assert_eq!(
            shell.lit.as_ref().map(|fields| fields.lit.clone()),
            Some(Lit::Sun)
        );
        run(&mut shell, SIZE, &retyped(1, "4.5"));
        let edited = std::fs::read_to_string(&path).unwrap();
        assert!(edited.contains("irradiance = 4.5\n"), "{edited}");
        assert_eq!(
            d1(&shell),
            Some(Said::Patched("irradiance · sun.irradiance".into()))
        );
        assert_eq!(shell.document().lights[0].2, "4.5 W/m²");
        assert_eq!(shell.dials.strength, 4.5);
        run(&mut shell, SIZE, &retyped(0, "0.8"));
        let edited = std::fs::read_to_string(&path).unwrap();
        assert!(
            edited.contains("irradiance = 4.5\ncolor = [0.8, 1.0, 1.0]\n"),
            "{edited}"
        );
        undo(&mut shell);
        undo(&mut shell);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(shell.dials.strength, 3.0);
    }

    #[test]
    fn c2_writes_the_skys_intensity_and_rotation() {
        let text = sun_and_sky();
        let (mut shell, _, path) = editing("shell-c2-sky", &text);
        run(
            &mut shell,
            SIZE,
            "frame\nkey b\nkey 3\nkey ArrowUp\nkey ArrowDown\nframe",
        );
        assert_eq!(
            shell.lit.as_ref().map(|fields| fields.lit.clone()),
            Some(Lit::Sky)
        );
        run(&mut shell, SIZE, &retyped(0, "0.6"));
        run(&mut shell, SIZE, &retyped(1, "-45"));
        let edited = std::fs::read_to_string(&path).unwrap();
        assert!(
            edited.ends_with("kind = \"analytic\"\nintensity = 0.6\nrotation_deg = -45.0\n"),
            "{edited}"
        );
        assert_eq!(shell.document().lights[1].2, "analytic  ×0.60  -45°");
        assert_eq!(shell.editor().unwrap().steps(), (2, 0));
        undo(&mut shell);
        undo(&mut shell);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn daylight_in_c2_edits_nothing() {
        let (mut shell, _, path) = editing("shell-c2-daylight", &lit_scene());
        run(&mut shell, SIZE, "frame\nkey b\nkey 3\nkey ArrowUp\nframe");
        assert!(shell.lit_shown);
        assert_eq!(shell.document().lights[shell.light].0, Lamp::Daylight);
        assert_eq!(shell.lit, None);
        run(&mut shell, SIZE, &retyped(0, "1"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), lit_scene());
    }

    #[test]
    fn a_light_without_a_recipe_says_so_in_c2() {
        let mut shell = Shell::new();
        run(
            &mut shell,
            SIZE,
            "frame\nkey b\nkey 3\nkey ArrowDown\nframe",
        );
        assert!(shell.lit_shown);
        assert_eq!(shell.lit, None);
        assert_eq!(shell.light, 1);
    }

    #[test]
    fn the_label_names_the_recipe_or_the_folder_and_not_the_sample() {
        assert_eq!(Shell::new().label(), None);
        assert_eq!(
            Shell::open("scenes/desk.toml").label().as_deref(),
            Some("desk.toml")
        );
        assert_eq!(
            Shell::browse("scenes/props/").label().as_deref(),
            Some("props")
        );
        let (mut shell, _, _) = live("shell-label", SCENE);
        let other = scratch("shell-label-other");
        std::fs::write(&other, SCENE).unwrap();
        shell.open_recipe(&other);
        assert_eq!(
            shell.label().as_deref(),
            other.file_name().and_then(|name| name.to_str())
        );
    }

    #[test]
    fn a_pockets_c2_rows_are_read_only() {
        let text = format!(
            "{}\n[[object]]\nname = \"slab\"\nkind = \"outline\"\noutline = {{ rect = [0.0, 0.0, 1.0, 1.0] }}\nfront = 0.0\nback = -0.1\n\n[[object]]\nname = \"cut\"\nkind = \"pocket\"\ninto = \"slab\"\noutline = {{ rect = [0.2, 0.2, 0.4, 0.4] }}\ndepth = -0.05\n",
            SCENE
        );
        let (mut shell, _, path) = editing("shell-c2-pocket", &text);
        run(&mut shell, SIZE, "frame");
        let at = shell.document.object("cut").unwrap();
        shell.select(at);
        run(&mut shell, SIZE, "frame");
        assert_eq!(
            shell.fields.as_ref().unwrap().locked,
            Some(crate::edit::POCKET_LOCK)
        );
        let script = format!(
            "frame\nclick {}\n{}text 0.9\nkey Enter\nframe",
            cell(0, 0),
            "key Backspace\n".repeat(5)
        );
        run(&mut shell, SIZE, &script);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(shell.editor().unwrap().steps(), (0, 0));
    }

    #[test]
    fn a_params_value_shows_read_only_and_is_never_written() {
        let path = scratch("shell-c2-params");
        std::fs::write(
            path.with_file_name("pair.params.json"),
            r#"{ "table": { "x": 0.4 } }"#,
        )
        .unwrap();
        let text = SCENE
            .replace(
                "name = \"pair\"\n",
                "name = \"pair\"\nparams = \"pair.params.json\"\n",
            )
            .replace(
                "material = \"oak\"\n",
                "material = \"oak\"\nat = [\"${params.table.x}\", 0.0, 0.0]\n",
            );
        std::fs::write(&path, &text).unwrap();
        let mut viewer = Viewer::new(&path, Mode::Fake);
        let mut shell = Shell::open(&path.display().to_string());
        shell.drains = true;
        viewer.sync(&mut shell);
        assert_eq!(shell.location, [0.4, 0.0, 0.0]);
        assert_eq!(
            shell.fields.as_ref().unwrap().params[0][0].as_deref(),
            Some("params.table.x")
        );
        let script = format!(
            "frame\nclick {}\n{}text 0.9\nkey Enter\nframe",
            cell(0, 0),
            "key Backspace\n".repeat(5)
        );
        run(&mut shell, SIZE, &script);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(shell.editor().unwrap().steps(), (0, 0));
        let lift = set_triple(
            shell.editor().unwrap().loaded().unwrap(),
            "table",
            Triple::Location,
            1,
            0.3,
        )
        .unwrap();
        shell.edits().push("gizmo", lift);
        shell.edits().push(
            "gizmo",
            vec![manfred_recipe::Patch {
                path: vec![
                    manfred_recipe::Key::Table("object".into()),
                    manfred_recipe::Key::Object("table".into()),
                    manfred_recipe::Key::Table("at".into()),
                ],
                value: Some(manfred_recipe::numbers(&[0.5, 0.0, 0.0])),
            }],
        );
        run(&mut shell, SIZE, "frame");
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("at = [\"${params.table.x}\", 0.3, 0.0]"),
            "{after}"
        );
        assert_eq!(
            d1(&shell),
            Some(Said::Refused(
                "gizmo: object[table].at comes from params.table.x; change it in the params file"
                    .into()
            ))
        );
    }

    fn path_of(path: &[manfred_recipe::Key]) -> String {
        use manfred_recipe::Key;
        let mut text = String::new();
        for key in path {
            match key {
                Key::Table(name) if text.is_empty() => text.push_str(name),
                Key::Table(name) => text.push_str(&format!(".{name}")),
                Key::Object(name) => text.push_str(&format!("[{name}]")),
                Key::Index(at) => text.push_str(&format!("[{at}]")),
            }
        }
        text
    }

    fn gestures(shell: &mut Shell) -> Vec<crate::edit::Gesture> {
        shell.edits().take()
    }

    fn patched(path: &std::path::Path, gestures: &[crate::edit::Gesture]) {
        let mut file = manfred_recipe::load(path).unwrap();
        for gesture in gestures {
            for patch in &gesture.patches {
                manfred_recipe::apply(&mut file, patch).unwrap();
            }
        }
        manfred_recipe::save(&file).unwrap();
    }

    fn handle_point(shell: &Shell, object: usize) -> egui::Pos2 {
        let image = shell.viewport.unwrap();
        let lens = shell.lens().unwrap();
        let step = 4;
        (0..image.height() as i32 / step)
            .flat_map(|row| (0..image.width() as i32 / step).map(move |column| (row, column)))
            .map(|(row, column)| {
                egui::Pos2::new(
                    image.left() + (column * step) as f32,
                    image.top() + (row * step) as f32,
                )
            })
            .find(|at| shell.gizmo().hit(&lens, object, *at).is_some())
            .unwrap()
    }

    const GROUPED: &str = "manfred = 1\nname = \"grouped\"\n\n[[object]]\nname = \"pair\"\nkind = \"box\"\nsize = [0.001, 0.001, 0.001]\nat = [0.2, 0.0, 0.0]\nhidden = true\n\n[[object]]\nname = \"left\"\nkind = \"box\"\nsize = [0.2, 0.2, 0.2]\nparent = \"pair\"\nat = [-0.4, 0.3, 0.0]\n\n[[object]]\nname = \"right\"\nkind = \"box\"\nsize = [0.2, 0.2, 0.2]\nparent = \"pair\"\nat = [0.6, 0.3, 0.0]\n";

    #[test]
    fn a_group_turns_about_its_centre_and_one_undo_puts_the_file_back() {
        let (mut shell, _, path) = editing("gizmo-group-turn", GROUPED);
        run(&mut shell, SIZE, "frame");
        shell.select(0);
        run(&mut shell, SIZE, "key r\nframe");
        assert_eq!(shell.gizmo().mode, crate::gizmo::Mode::Rotate);
        let start = shell.gizmo().shown(0).unwrap().clone();
        assert!((start.centre() - glam::DVec3::new(0.3, 0.3, 0.0)).length() < 1e-6);
        let lens = shell.lens().unwrap();
        assert!(
            (lens.project(start.centre()).unwrap() - shell.viewport().unwrap().center()).length()
                > 1.0
        );
        let at = handle_point(&shell, 0);
        let script = format!(
            "drag {} {} {} {}\nframe",
            at.x,
            at.y,
            at.x + 40.0,
            at.y + 30.0
        );
        run(&mut shell, SIZE, &script);
        let turned = std::fs::read_to_string(&path).unwrap();
        assert_ne!(turned, GROUPED);
        let recipe = manfred_recipe::load(&path).unwrap().recipe;
        let pair = &recipe.objects[0];
        assert_ne!(pair.rotate, [0.0; 3]);
        assert_ne!(pair.at, [0.2, 0.0, 0.0]);
        let kept = manfred_build::world(&recipe, 0)
            .unwrap()
            .transform_point3(start.pivot);
        assert!((kept - start.centre()).length() < 1e-5, "{kept}");
        let changed: Vec<&str> = turned
            .lines()
            .filter(|line| !GROUPED.lines().any(|old| old == *line))
            .collect();
        assert_eq!(changed.len(), 2, "{changed:?}");
        assert!(changed[0].starts_with("at = ") && changed[1].starts_with("rotate = "));
        assert_eq!(shell.editor().unwrap().steps(), (1, 0));
        run(&mut shell, SIZE, "key ctrl+z\nframe");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), GROUPED);
        run(&mut shell, SIZE, "key ctrl+shift+z\nframe");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), turned);
    }

    #[test]
    fn a_drag_with_the_render_overlay_up_pushes_no_gesture() {
        let (mut shell, _, _) = live("shell-overlay-drag", SCENE);
        run(
            &mut shell,
            SIZE,
            "frame\nkey b\nkey 1\nkey ArrowDown\nframe",
        );
        let object = shell.object.unwrap();
        let at = handle_point(&shell, object);
        let script = format!(
            "drag {} {} {} {}\nframe",
            at.x,
            at.y,
            at.x + 30.0,
            at.y + 10.0
        );
        run(&mut shell, SIZE, &script);
        assert_eq!(gestures(&mut shell).len(), 1);
        shell.full = true;
        let eye = shell.eye();
        run(&mut shell, SIZE, &script);
        assert!(gestures(&mut shell).is_empty());
        assert_eq!(shell.eye(), eye);
    }

    #[test]
    fn a_click_on_the_viewport_closes_the_render_overlay() {
        let (mut shell, _, _) = live("shell-overlay-click", SCENE);
        run(&mut shell, SIZE, "frame");
        shell.full = true;
        run(&mut shell, SIZE, "click 700 400\nframe");
        assert!(!shell.full);
        assert!(shell.pick.is_none());
    }

    #[test]
    fn undo_and_redo_with_the_render_overlay_up_only_close_it() {
        let (mut shell, _, _) = editing("shell-overlay-undo", SCENE);
        run(&mut shell, SIZE, "frame");
        shell.full = true;
        undo(&mut shell);
        assert!(!shell.full);
        shell.full = true;
        redo(&mut shell, Key::Y);
        assert!(!shell.full);
    }

    #[test]
    fn set_from_view_pushes_one_gesture_for_the_chosen_camera() {
        let (mut shell, _, _) = live("shell-set", SCENE);
        run(&mut shell, SIZE, "frame\ndrag 700 400 900 300\nframe");
        let eye = shell.eye().unwrap();
        run(&mut shell, SIZE, "key b\nkey 2\nkey s\nframe");
        let pushed = gestures(&mut shell);
        assert_eq!(pushed.len(), 1);
        assert_eq!(pushed[0].label, "set camera front from view");
        let keys: Vec<String> = pushed[0]
            .patches
            .iter()
            .map(|patch| path_of(&patch.path))
            .collect();
        assert_eq!(
            keys,
            [
                "camera[front].position",
                "camera[front].target",
                "camera[front].up",
                "camera[front].fov_y_deg"
            ]
        );
        assert!(shell.edits().is_empty());
        assert_eq!(shell.eye(), Some(eye));
    }

    #[test]
    fn the_view_snaps_to_the_camera_once_the_patch_lands() {
        let (mut shell, mut viewer, path) = live("shell-snap", SCENE);
        run(&mut shell, SIZE, "frame\ndrag 700 400 900 300\nframe");
        let eye = shell.eye().unwrap();
        run(&mut shell, SIZE, "key b\nkey 2\nkey s\nframe");
        assert_ne!(shell.eye(), Some(shell.home()));
        patched(&path, &gestures(&mut shell));
        assert!(viewer.reload(&mut shell));
        assert_eq!(shell.eye(), Some(shell.home()));
        assert!(orbit_close(&shell.eye().unwrap(), &eye));
        assert_eq!(shell.view_info(), "front");
        assert!(shell.settle.is_none());
    }

    fn orbit_close(a: &Eye, b: &Eye) -> bool {
        (offset(a) - offset(b)).length() < 1e-5
    }

    #[test]
    fn a_patch_that_never_lands_leaves_the_view_alone() {
        let (mut shell, mut viewer, path) = live("shell-never", SCENE);
        run(&mut shell, SIZE, "frame\ndrag 700 400 900 300\nframe");
        run(&mut shell, SIZE, "key b\nkey 2\nkey s\nframe");
        let eye = shell.eye();
        gestures(&mut shell);
        std::fs::write(&path, SCENE.replace("name = \"pair\"", "name = \"later\"")).unwrap();
        assert!(viewer.reload(&mut shell));
        assert_eq!(shell.eye(), eye);
        assert!(shell.view_info().starts_with("orbit from"));
    }

    #[test]
    fn new_camera_appends_one_selects_it_and_shows_its_name() {
        let (mut shell, mut viewer, path) = live("shell-new", SCENE);
        run(&mut shell, SIZE, "frame\ndrag 700 400 900 300\nframe");
        run(&mut shell, SIZE, "key b\nkey 2\nkey n\nframe");
        let pushed = gestures(&mut shell);
        assert_eq!(pushed.len(), 1);
        assert_eq!(pushed[0].label, "new camera view1");
        patched(&path, &pushed);
        assert!(viewer.reload(&mut shell));
        let names: Vec<&str> = shell
            .document()
            .cameras
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["front", "side", "view1"]);
        assert_eq!(shell.camera, 2);
        assert_eq!(shell.view_info(), "view1");
        run(&mut shell, SIZE, "key n\nframe");
        let again = gestures(&mut shell);
        assert_eq!(again[0].label, "new camera view2");
    }

    #[test]
    fn delete_asks_first_and_enter_confirms() {
        let (mut shell, mut viewer, path) = live("shell-delete", SCENE);
        run(
            &mut shell,
            SIZE,
            "frame\nkey b\nkey 2\nkey ArrowDown\nkey Delete\nframe",
        );
        assert_eq!(shell.confirm.as_deref(), Some("side"));
        assert!(shell.edits().is_empty());
        run(&mut shell, SIZE, "key Escape\nframe");
        assert_eq!(shell.confirm, None);
        assert!(shell.edits().is_empty());
        run(&mut shell, SIZE, "key Delete\nkey ArrowUp\nframe");
        assert_eq!(shell.confirm, None);
        assert!(shell.edits().is_empty());
        assert_eq!(shell.camera, 0);
        run(
            &mut shell,
            SIZE,
            "key ArrowDown\nkey Delete\nkey Enter\nframe",
        );
        let pushed = gestures(&mut shell);
        assert_eq!(pushed.len(), 1);
        assert_eq!(pushed[0].label, "delete camera side");
        patched(&path, &pushed);
        assert!(viewer.reload(&mut shell));
        assert_eq!(shell.document().cameras.len(), 1);
        assert_eq!(shell.camera, 0);
    }

    #[test]
    fn b2_edits_nothing_without_a_recipe_open() {
        let mut shell = Shell::new();
        run(
            &mut shell,
            SIZE,
            "frame\nkey b\nkey 2\nkey s\nkey n\nkey Delete\nframe",
        );
        assert!(shell.edits().is_empty());
        assert_eq!(shell.confirm, None);
    }

    struct Bench {
        shell: Shell,
        viewer: Viewer,
        plans: Vec<std::sync::Arc<std::sync::Mutex<crate::shots::Plan>>>,
        statuses: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        runner: std::sync::Arc<crate::shots::Runs>,
        dir: std::path::PathBuf,
        saved: Option<Picture>,
    }

    const SHOT_RUNNING: &str =
        r#"{"jobs":[{"pid":42,"state":"running","as":"manfred","label":"x","waits_for":null}]}"#;
    const SHOT_WAITING: &str =
        r#"{"jobs":[{"pid":42,"state":"waiting","as":"manfred","label":"x","waits_for":"desk"}]}"#;
    const SHOT_QUEUED: &str =
        r#"{"jobs":[{"pid":42,"state":"waiting","as":"manfred","label":"x","waits_for":null}]}"#;

    fn bench(name: &str) -> Bench {
        let (mut shell, viewer, path) = live(name, SCENE);
        let dir = path.with_file_name("renders");
        let statuses = std::sync::Arc::new(std::sync::Mutex::new(vec![SHOT_QUEUED.to_string()]));
        let runner = std::sync::Arc::new(crate::shots::Runs {
            processes: std::sync::Mutex::new(Vec::new()),
            statuses: statuses.clone(),
            refuse: false,
        });
        let mut shots = crate::shots::Shots::with(runner.clone(), "manfred".to_string());
        shots.set_tick(std::time::Duration::from_millis(2));
        shots.set_dir(dir.clone());
        shell.shots = shots;
        Bench {
            shell,
            viewer,
            plans: Vec::new(),
            statuses,
            runner,
            dir,
            saved: None,
        }
    }

    impl Bench {
        fn process(&mut self, polls: Vec<Option<crate::shots::Exit>>) {
            let plan = std::sync::Arc::new(std::sync::Mutex::new(crate::shots::Plan {
                polls,
                ..crate::shots::Plan::default()
            }));
            self.plans.push(plan.clone());
            self.runner
                .processes
                .lock()
                .unwrap()
                .push(crate::shots::Fake {
                    pid: 42,
                    script: plan,
                });
        }

        fn status(&self, text: &str) {
            *self.statuses.lock().unwrap() = vec![text.to_string()];
        }

        fn until(&mut self, wanted: impl Fn(&Phase) -> bool) {
            for _ in 0..5000 {
                self.shell.shots.poll();
                if self.shell.shots.job().is_some_and(|job| wanted(&job.phase)) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            panic!("the job never got there: {:?}", self.shell.shots.job());
        }

        fn out(&self) -> std::path::PathBuf {
            self.dir.join("scene-front-1586x992.png")
        }

        fn shoot(&mut self, script: &str, file: &str) {
            if let Some(saved) = &self.saved {
                self.shell.shots.put_picture(saved.clone());
            }
            let png = crate::shot(
                &mut crate::Manfred::lent(&mut self.shell, &mut self.viewer),
                SIZE,
                &Script::parse(script).unwrap(),
            )
            .unwrap();
            std::fs::write(self.dir.with_file_name(file), png).unwrap();
        }
    }

    fn fail() -> crate::shots::Exit {
        crate::shots::Exit {
            ok: false,
            code: Some(1),
            stderr: "manfred shot: no adapter\n".to_string(),
        }
    }

    fn good() -> crate::shots::Exit {
        crate::shots::Exit {
            ok: true,
            code: Some(0),
            stderr: String::new(),
        }
    }

    fn write_picture(path: &std::path::Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let rgba: Vec<u8> = (0..160 * 100)
            .flat_map(|n| [(n % 160) as u8, (n / 160 * 2) as u8, 120, 255])
            .collect();
        std::fs::write(path, crate::encode_png([160, 100], &rgba).unwrap()).unwrap();
    }

    #[test]
    fn enter_in_c3_renders_the_chosen_camera_and_the_window_goes_on() {
        let mut bench = bench("shell-render");
        bench.process(vec![]);
        write_picture(&bench.out());
        run(
            &mut bench.shell,
            SIZE,
            "frame\nkey c\nkey 3\nkey Enter\nframe",
        );
        assert_eq!(bench.shell.render_camera, 0);
        assert!(bench.shell.shots.busy());
        let job = bench.shell.shots.job().unwrap();
        assert_eq!(job.camera, "front");
        assert_eq!(job.size, [1586, 992]);
        assert_eq!(job.phase, Phase::Queued);
        run(&mut bench.shell, SIZE, "frame\nkey Enter\nframe");
        assert!(bench.shell.shots.busy());
        bench.status(SHOT_WAITING);
        bench.until(|phase| *phase == Phase::Waiting("desk".to_string()));
        bench.status(SHOT_RUNNING);
        bench.until(|phase| *phase == Phase::Running);
        run(&mut bench.shell, SIZE, "frame");
        bench.plans[0].lock().unwrap().polls = vec![Some(good())];
        bench.until(|phase| *phase == Phase::Done);
        assert!(!bench.shell.shots.busy());
        run(&mut bench.shell, SIZE, "frame");
        assert!(bench.shell.shot_views.is_some());
        run(&mut bench.shell, SIZE, "key c\nkey 3\nkey v\nframe");
        assert!(bench.shell.full);
        run(&mut bench.shell, SIZE, "key ArrowDown\nframe");
        assert!(!bench.shell.full);
        assert_eq!(bench.shell.render_camera, 0);
        assert!(bench.shell.shot_views.is_some());
    }

    #[test]
    fn escape_in_c3_cancels_with_term() {
        let mut bench = bench("shell-cancel");
        bench.process(vec![]);
        run(
            &mut bench.shell,
            SIZE,
            "frame\nkey c\nkey 3\nkey Enter\nframe",
        );
        bench.status(SHOT_RUNNING);
        bench.until(|phase| *phase == Phase::Running);
        run(&mut bench.shell, SIZE, "key Escape\nframe");
        bench.until(|phase| *phase == Phase::Cancelled);
        assert!(bench.plans[0].lock().unwrap().cancelled);
        assert!(!bench.shell.shots.busy());
        assert!(bench.shell.shot_views.is_none());
    }

    #[test]
    fn a_failed_render_keeps_the_window_and_says_why() {
        let mut bench = bench("shell-failed");
        bench.process(vec![Some(fail())]);
        run(
            &mut bench.shell,
            SIZE,
            "frame\nkey c\nkey 3\nkey Enter\nframe",
        );
        bench.until(|phase| matches!(phase, Phase::Failed(_)));
        assert_eq!(
            bench.shell.shots.job().unwrap().phase,
            Phase::Failed("exit 1: manfred shot: no adapter".to_string())
        );
        bench.process(vec![]);
        run(&mut bench.shell, SIZE, "key Enter\nframe");
        assert!(bench.shell.shots.busy());
        assert_eq!(bench.shell.shots.job().unwrap().phase, Phase::Queued);
    }

    #[test]
    fn render_without_a_recipe_or_a_camera_only_notes() {
        let mut shell = Shell::new();
        run(&mut shell, SIZE, "frame\nkey c\nkey 3\nkey Enter\nframe");
        assert!(!shell.shots.busy());
        assert_eq!(shell.note, Some("render: open a recipe to render it"));
    }

    #[test]
    fn d1_carries_the_job_summary() {
        let mut bench = bench("shell-d1");
        bench.shell.shots.show(Some(crate::shots::Job {
            camera: "front".to_string(),
            size: [1586, 992],
            out: bench.out(),
            pid: Some(42),
            phase: Phase::Waiting("vram".to_string()),
            cancelling: false,
            progress: None,
        }));
        run(&mut bench.shell, SIZE, "frame");
        assert_eq!(
            bench.shell.shots.job().unwrap().summary(),
            "front 1586×992 · waiting for vram"
        );
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn c3_and_d1_show_the_samples_of_a_running_render() {
        let mut bench = bench("shell-progress");
        bench.process(vec![]);
        run(
            &mut bench.shell,
            SIZE,
            "frame\nkey c\nkey 3\nkey Enter\nframe",
        );
        bench.status(SHOT_RUNNING);
        bench.until(|phase| *phase == Phase::Running);
        assert_eq!(bench.shell.shots.job().unwrap().progress, None);
        bench.plans[0].lock().unwrap().progress = Some((96, 256));
        for _ in 0..5000 {
            bench.shell.shots.poll();
            if bench.shell.shots.job().unwrap().progress.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let job = bench.shell.shots.job().unwrap();
        assert_eq!(job.progress, Some((96, 256)));
        assert_eq!(job.summary(), "front 1586×992 · running 96/256");
        bench.shoot("frame", "shell-progress.png");
        assert!(bench.shell.shots.busy());
    }

    #[test]
    fn the_watchers_rebuild_of_an_own_write_is_not_an_outside_change() {
        let (mut shell, mut viewer, _) = editing("shell-c2-own", SCENE);
        let script = format!(
            "frame\nclick {}\n{}text 2\nkey Enter\nframe",
            cell(2, 1),
            "key Backspace\n".repeat(5)
        );
        run(&mut shell, SIZE, &script);
        assert!(viewer.stale());
        assert!(viewer.reload(&mut shell));
        assert_eq!(
            d1(&shell),
            Some(Said::Patched("scale · object[table].scale".into()))
        );
        assert_eq!(shell.editor().unwrap().steps(), (1, 0));
        assert_eq!(shell.scale, [1.0, 2.0, 1.0]);
    }

    #[test]
    fn an_outside_write_reloads_and_d1_notes_it() {
        let (mut shell, mut viewer, path) = editing("shell-c2-outside", SCENE);
        run(&mut shell, SIZE, &typed(MATERIAL, 3, "walnut"));
        let outside = std::fs::read_to_string(&path)
            .unwrap()
            .replace("name = \"cup\"", "name = \"mug\"");
        std::fs::write(&path, &outside).unwrap();
        assert!(viewer.reload(&mut shell));
        assert_eq!(
            d1(&shell),
            Some(Said::Reloaded(
                "the recipe changed outside the window; every undo step still applies".into()
            ))
        );
        assert_eq!(shell.document().objects[1].name, "mug");
        run(&mut shell, SIZE, "frame");
        undo(&mut shell);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("name = \"mug\""), "{text}");
        assert!(text.contains("material = \"oak\""), "{text}");
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn b2_after_set_from_view_and_c3_in_each_state_are_drawn() {
        let since = std::time::Instant::now();
        let mut bench = bench("shell-render");
        run(
            &mut bench.shell,
            SIZE,
            "frame\ndrag 700 400 900 300\nframe\nkey b\nkey 2\nkey s\nframe",
        );
        let pushed = gestures(&mut bench.shell);
        let path = std::path::PathBuf::from(&bench.shell.document().path);
        patched(&path, &pushed);
        assert!(bench.viewer.reload(&mut bench.shell));
        bench.shoot("frame", "cameras-b2.png");
        bench.process(vec![]);
        write_picture(&bench.out());
        bench.status(SHOT_QUEUED);
        run(
            &mut bench.shell,
            SIZE,
            "frame\nkey c\nkey 3\nkey ArrowUp\nkey Enter\nframe",
        );
        bench.shoot("frame", "cameras-c3-queued.png");
        bench.status(SHOT_WAITING);
        bench.until(|phase| matches!(phase, Phase::Waiting(_)));
        bench.shoot("frame", "cameras-c3-waiting.png");
        bench.status(SHOT_RUNNING);
        bench.until(|phase| *phase == Phase::Running);
        bench.shoot("frame", "cameras-c3-running.png");
        bench.plans[0].lock().unwrap().polls = vec![Some(good())];
        bench.until(|phase| *phase == Phase::Done);
        bench.saved = bench.shell.shots.take_picture();
        assert!(bench.saved.is_some());
        bench.shoot("frame", "cameras-c3-done.png");
        bench.shoot("key c\nkey 3\nkey v\nframe", "cameras-c3-full.png");
        run(&mut bench.shell, SIZE, "key Escape\nframe");
        assert!(!bench.shell.full);
        bench.process(vec![Some(fail())]);
        run(&mut bench.shell, SIZE, "key Enter\nframe");
        bench.until(|phase| matches!(phase, Phase::Failed(_)));
        bench.saved = None;
        bench.shoot("frame", "cameras-c3-failed.png");
        bench.process(vec![]);
        run(&mut bench.shell, SIZE, "key Enter\nframe");
        bench.status(SHOT_RUNNING);
        bench.until(|phase| *phase == Phase::Running);
        run(&mut bench.shell, SIZE, "key Escape\nframe");
        bench.until(|phase| *phase == Phase::Cancelled);
        bench.shoot("frame", "cameras-c3-cancelled.png");
        run(&mut bench.shell, SIZE, "key b\nkey 2\nkey Delete\nframe");
        bench.shoot("frame", "cameras-b2-confirm.png");
        crate::gpu_turn(since);
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn d1_labels_its_frame_time_as_gpu_and_counts_dropped_timings() {
        let since = std::time::Instant::now();
        let (mut shell, mut viewer, _) = live("shell-d1-gpu", SCENE);
        let script = Script::parse("frame").unwrap();
        shell.set_frame_ms(Some(3.4));
        let png = crate::shot(
            &mut crate::Manfred::lent(&mut shell, &mut viewer),
            SIZE,
            &script,
        )
        .unwrap();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        std::fs::write(out.join("window-d1-gpu.png"), png).unwrap();
        shell.set_timing_drops(2);
        let png = crate::shot(
            &mut crate::Manfred::lent(&mut shell, &mut viewer),
            SIZE,
            &script,
        )
        .unwrap();
        std::fs::write(out.join("window-d1-drops.png"), png).unwrap();
        crate::gpu_turn(since);
    }

    const LABELLED: &str = r#"manfred = 1
name = "labelled"

[[object]]
name = "label"
kind = "text"
on = "slab"
spans = [{ text = "manfred", size = 0.02 }, { text = " draws", size = 0.015, color = [0.9, 0.5, 0.1] }]

[[object]]
name = "slab"
kind = "box"
size = [0.4, 0.2, 0.1]
material = "orange"

[[camera]]
name = "front"
position = [0.0, 0.2, 1.0]
target = [0.0, 0.0, 0.0]
fov_y_deg = 35.0

[materials.orange]
family = "lacquer"
base = [0.73, 0.06, 0.0]
roughness = 0.45
clearcoat = 1.0
clearcoat_roughness = 0.07
"#;

    fn centre_of(ctx: &Context, id: Id) -> Pos2 {
        ctx.read_response(id)
            .unwrap_or_else(|| panic!("{id:?} was not drawn"))
            .rect
            .center()
    }

    fn labelled(name: &str) -> (Shell, Viewer, std::path::PathBuf, Context) {
        let (mut shell, viewer, path) = live(name, LABELLED);
        let ctx = Context::default();
        theme::apply_theme(&ctx, &theme::manfred());
        drive(&ctx, SIZE, &Script::default(), |ui| shell.ui(ui));
        (shell, viewer, path, ctx)
    }

    fn span_field(span: usize) -> Id {
        Id::new(("manfred span", "label", span))
    }

    #[test]
    fn c2_shows_a_text_objects_spans_and_no_others() {
        let (mut shell, _viewer, _path, ctx) = labelled("spans-shown");
        assert_eq!(shell.document().objects[0].name, "label");
        assert!(ctx.read_response(span_field(0)).is_some());
        assert!(ctx.read_response(span_field(1)).is_some());
        assert!(ctx.read_response(span_field(2)).is_none());
        shell.select(1);
        drive(&ctx, SIZE, &Script::default(), |ui| shell.ui(ui));
        assert!(ctx.read_response(span_field(0)).is_none());
    }

    #[test]
    fn c2_hides_the_material_and_smoothing_rows_for_a_text_object() {
        let material = Id::new("manfred c2 material");
        let name = Id::new("manfred c2 name");
        let (mut shell, _viewer, _path, ctx) = labelled("spans-no-material");
        assert!(ctx.read_response(name).is_some());
        assert!(ctx.read_response(material).is_none());
        shell.select(1);
        drive(&ctx, SIZE, &Script::default(), |ui| shell.ui(ui));
        assert!(ctx.read_response(material).is_some());
        shell.select(0);
        drive(&ctx, SIZE, &Script::default(), |ui| shell.ui(ui));
        assert!(ctx.read_response(material).is_none());
        assert!(ctx.read_response(span_field(0)).is_some());
    }

    #[test]
    fn a_span_edit_in_c2_is_one_gesture_when_enter_finishes_it() {
        let (mut shell, _viewer, path, ctx) = labelled("spans-enter");
        let field = centre_of(&ctx, span_field(0));
        println!("span 1's field at {} {}", field.x, field.y);
        let script = format!(
            "click {} {}\nkey End\ntext ,headless\nframe\nkey Enter\n",
            field.x, field.y
        );
        drive(&ctx, SIZE, &Script::parse(&script).unwrap(), |ui| {
            shell.ui(ui)
        });
        let gestures = shell.edits().take();
        assert_eq!(gestures.len(), 1, "{gestures:?}");
        assert_eq!(gestures[0].label, "text of label span 1");
        assert_eq!(gestures[0].patches.len(), 1);
        let patch = &gestures[0].patches[0];
        assert_eq!(patch.path, spans::span_path("label", 0, "text"));
        assert_eq!(
            patch.value.as_ref().and_then(|v| v.as_str()),
            Some("manfred,headless")
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), LABELLED);
    }

    #[test]
    fn escape_puts_a_span_back_and_makes_no_gesture() {
        let (mut shell, _viewer, _path, ctx) = labelled("spans-escape");
        let field = centre_of(&ctx, span_field(1));
        let script = format!(
            "click {} {}\nkey End\ntext more\nframe\nkey Escape\nframe\n",
            field.x, field.y
        );
        drive(&ctx, SIZE, &Script::parse(&script).unwrap(), |ui| {
            shell.ui(ui)
        });
        assert!(shell.edits().is_empty());
        assert_eq!(shell.spans.rows[1].text, " draws");
    }

    #[test]
    fn dragging_a_span_size_is_one_gesture_on_release() {
        let (mut shell, _viewer, _path, ctx) = labelled("spans-drag");
        let field = centre_of(&ctx, span_field(0));
        let size = Pos2::new(field.x, field.y + ROW_HEIGHT + GAP);
        let script = format!("drag {} {} {} {}\n", size.x, size.y, size.x + 40.0, size.y);
        drive(&ctx, SIZE, &Script::parse(&script).unwrap(), |ui| {
            shell.ui(ui)
        });
        let gestures = shell.edits().take();
        assert_eq!(gestures.len(), 1, "{gestures:?}");
        assert_eq!(gestures[0].label, "size of label span 1");
        let patch = &gestures[0].patches[0];
        assert_eq!(patch.path, spans::span_path("label", 0, "size"));
        let size = patch.value.as_ref().and_then(|v| v.as_float()).unwrap();
        assert!((0.035..=0.0401).contains(&size), "{size}");
        assert_eq!(size, (size * 1e4).round() / 1e4);
    }

    fn recipe(name: &str, object: &str) -> String {
        format!(
            "manfred = 1\nname = \"{name}\"\n\n[[object]]\nname = \"{object}\"\nkind = \"box\"\nsize = [0.2, 0.4, 0.2]\n"
        )
    }

    fn isolated_book(name: &str) -> crate::open::Book {
        let dir = scratch(name).parent().unwrap().join("state");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        crate::open::Book::at(&dir)
    }

    #[test]
    fn ctrl_o_then_a_typed_path_then_enter_opens_it_and_resets_undo() {
        let (mut shell, mut viewer, path) = editing("open-second", SCENE);
        shell.book = isolated_book("open-second");
        edit_by_script(&mut shell);
        assert_eq!(shell.editor().unwrap().steps(), (1, 0));
        let edited = std::fs::read_to_string(&path).unwrap();
        let second = scratch("open-second-b");
        std::fs::write(&second, recipe("second", "lamp")).unwrap();
        let script = format!("key ctrl+o\ntext {}\nkey Enter\nframe", second.display());
        let ctx = Context::default();
        theme::apply_theme(&ctx, &theme::manfred());
        let generation = viewer.follow().generation();
        drive(&ctx, SIZE, &Script::parse(&script).unwrap(), |ui| {
            shell.ui(ui);
            viewer.sync(&mut shell);
        });
        assert_eq!(viewer.follow().generation(), generation);
        assert_eq!(shell.document().path, second.display().to_string());
        assert_eq!(shell.document().name, "second");
        assert_eq!(shell.document().objects[0].name, "lamp");
        assert_eq!(shell.editor().unwrap().steps(), (0, 0));
        assert_eq!(
            shell.take_opened().as_deref(),
            Some(second.display().to_string()).as_deref()
        );
        let recent = shell.book.list();
        assert_eq!(recent[0], std::path::absolute(&second).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
        undo(&mut shell);
        assert_eq!(
            std::fs::read_to_string(&second).unwrap(),
            recipe("second", "lamp")
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
        assert_eq!(shell.editor().unwrap().steps(), (0, 0));
    }

    #[test]
    fn a_shell_under_test_has_an_empty_book_that_remembers_nothing() {
        let shell = Shell::new();
        assert!(shell.book.list().is_empty());
        shell
            .book
            .remember(std::path::Path::new("scene.toml"))
            .unwrap();
        assert!(shell.book.list().is_empty());
        assert!(Shell::new().without_recent().book.list().is_empty());
    }

    #[test]
    fn a_recipe_from_the_command_line_joins_the_recent_book_only_when_it_is_a_file() {
        let state = scratch("remember-cli").parent().unwrap().join("state");
        let _ = std::fs::remove_dir_all(&state);
        let book = isolated_book("remember-cli");
        let file = scratch("remember-cli-scene");
        std::fs::write(&file, recipe("scene", "lamp")).unwrap();
        remember_in(&book, "tmp/missing-recipe.toml");
        assert!(book.list().is_empty());
        remember_in(&book, &file.display().to_string());
        assert_eq!(book.list(), vec![std::path::absolute(&file).unwrap()]);
    }

    #[test]
    fn escape_closes_the_picker_when_a_recipe_is_open() {
        let (mut shell, _, _) = editing("open-esc", SCENE);
        shell.book = isolated_book("open-esc");
        run(&mut shell, SIZE, "key ctrl+o\nframe");
        assert!(shell.picker.shown());
        run(&mut shell, SIZE, "key Escape\nframe");
        assert!(!shell.picker.shown());
        assert_eq!(shell.document().name, "pair");
    }

    #[test]
    fn an_empty_window_draws_the_picker_and_escape_leaves_it_up() {
        let mut shell = Shell::empty();
        shell.book = isolated_book("open-empty");
        assert!(shell.document().objects.is_empty());
        assert!(shell.document().cameras.is_empty());
        assert!(shell.document().path.is_empty());
        let ctx = Context::default();
        theme::apply_theme(&ctx, &theme::manfred());
        drive(&ctx, SIZE, &Script::parse("frame").unwrap(), |ui| {
            shell.ui(ui)
        });
        assert!(ctx.read_response(crate::open::field_id()).is_some());
        assert!(!shell.picker.shown());
        assert!(!shell.live());
        drive(
            &ctx,
            SIZE,
            &Script::parse("key ctrl+o\nframe").unwrap(),
            |ui| shell.ui(ui),
        );
        assert!(shell.picker.shown());
        drive(
            &ctx,
            SIZE,
            &Script::parse("key Escape\nframe").unwrap(),
            |ui| shell.ui(ui),
        );
        assert!(!shell.picker.shown());
        assert!(ctx.read_response(crate::open::field_id()).is_some());
        assert!(!shell.live());
    }

    #[test]
    fn an_empty_start_opens_the_copy_of_the_start_scene_with_the_picker_closed() {
        let state = scratch("start-state").parent().unwrap().join("state");
        let _ = std::fs::remove_dir_all(&state);
        let path = crate::start_recipe(&state).unwrap();
        let path_text = path.display().to_string();
        let mut viewer = Viewer::new(&path, Mode::Fake);
        let mut shell = Shell::start(&path_text);
        viewer.sync(&mut shell);
        run(&mut shell, SIZE, "frame");
        assert!(shell.live());
        assert_eq!(
            shell.object,
            shell.document().object(crate::start::MARK),
            "{:?}",
            shell.object
        );
        assert!(!shell.picker.showing(shell.live()));
        assert_eq!(shell.document().path, path_text);
        for name in ["badge", "wordmark"] {
            assert!(shell.document().object(name).is_some(), "{name}");
        }
        run(&mut shell, SIZE, "key ctrl+o\nframe");
        assert!(shell.picker.shown());
    }

    #[test]
    fn the_start_mark_is_selected_with_its_gizmo_in_the_middle_and_a_drag_moves_it_whole() {
        let state = scratch("start-drag").parent().unwrap().join("state");
        let _ = std::fs::remove_dir_all(&state);
        let path = crate::start_recipe(&state).unwrap();
        let mut viewer = Viewer::new(&path, Mode::Fake);
        let mut shell = Shell::start(&path.display().to_string());
        shell.drains = true;
        viewer.sync(&mut shell);
        run(&mut shell, SIZE, "frame");
        let mark = shell.document().object(crate::start::MARK).unwrap();
        assert_eq!(shell.object, Some(mark));
        let image = shell.viewport.unwrap();
        let lens = shell.lens().unwrap();
        let centre = lens
            .project(shell.gizmo().shown(mark).unwrap().centre())
            .unwrap();
        assert!(
            (centre - image.center()).length() < image.height() * 0.08,
            "{centre:?} in {image:?}"
        );
        let names = [crate::start::MARK, "badge", "wordmark"];
        let places = |path: &std::path::Path| {
            let recipe = manfred_recipe::load(path).unwrap().recipe;
            names.map(|name| {
                let index = recipe.objects.iter().position(|o| o.name == name).unwrap();
                manfred_build::world(&recipe, index)
                    .unwrap()
                    .transform_point3(glam::DVec3::ZERO)
            })
        };
        let before_text = std::fs::read_to_string(&path).unwrap();
        let before = places(&path);
        let at = handle_point(&shell, mark);
        let script = format!(
            "drag {} {} {} {}\nframe",
            at.x,
            at.y,
            at.x + 40.0,
            at.y + 25.0
        );
        run(&mut shell, SIZE, &script);
        let after_text = std::fs::read_to_string(&path).unwrap();
        let changed: Vec<(&str, &str)> = before_text
            .lines()
            .zip(after_text.lines())
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert!(changed[0].1.starts_with("at = "), "{changed:?}");
        let after = places(&path);
        let moved = after[0] - before[0];
        assert!(moved.length() > 1e-3, "{moved}");
        for (name, (a, b)) in names.iter().zip(before.iter().zip(&after)) {
            assert!(((*b - *a) - moved).length() < 1e-9, "{name}");
        }
        assert_ne!(after_text, crate::start::start_text());
        assert_eq!(
            std::fs::read_to_string(crate::start_recipe(&state).unwrap()).unwrap(),
            crate::start::start_text()
        );
    }

    #[test]
    fn enter_on_a_folder_opens_the_first_recipe_by_name() {
        let folder = scratch("open-browse").parent().unwrap().to_path_buf();
        std::fs::write(folder.join("b.toml"), recipe("b", "bee")).unwrap();
        std::fs::write(folder.join("a.toml"), recipe("a", "lamp")).unwrap();
        std::fs::write(folder.join("notes.txt"), "no").unwrap();
        std::fs::create_dir_all(folder.join("sub")).unwrap();
        std::fs::write(folder.join("sub/d.toml"), recipe("d", "deep")).unwrap();
        std::fs::write(folder.join(".hidden.toml"), recipe("hidden", "nope")).unwrap();
        let mut shell = Shell::browse(&folder.display().to_string());
        shell.book = isolated_book("open-browse");
        run(&mut shell, SIZE, "frame\nkey Enter\nframe");
        assert!(shell.live());
        assert_eq!(shell.document().name, "a");
        assert_eq!(shell.document().objects[0].name, "lamp");
        assert_eq!(shell.editor().unwrap().steps(), (0, 0));
        assert!(shell.document().path.ends_with("a.toml"));
        assert!(!shell.picker.shown());
    }

    const STREET: &str = r#"manfred = 1
name = "street"

# the chase line
[[route]]
name = "main"
points = [
  [-20.0, 0.0, -30.0], # start
  [0.0, 0.0, 0.0],
  [15.0, 0.0, 30.0],
  [5.0, 0.0, 60.0], # end
]

[[object]]
name = "ground"
kind = "box"
size = [80.0, 0.1, 120.0]
at = [0.0, -0.05, 15.0]

[[camera]]
name = "top"
position = [70.0, 90.0, 15.0]
target = [0.0, 0.0, 15.0]
fov_y_deg = 50.0

[sun]
model = "daylight"
hour = 15.0
latitude = 47.0
heading = 30.0


[materials.default]
family = "plaster"
base = [0.35, 0.36, 0.38]
roughness = 0.8
"#;

    fn route_points(path: &std::path::Path) -> Vec<[f64; 3]> {
        manfred_recipe::load(path).unwrap().recipe.routes[0]
            .points
            .clone()
    }

    fn route_at(shell: &Shell, point: [f64; 3]) -> Pos2 {
        shell
            .lens()
            .unwrap()
            .project(glam::DVec3::from_array(point))
            .unwrap()
    }

    #[test]
    fn a_route_point_drags_adds_and_removes_through_the_viewport() {
        let (mut shell, mut viewer, path) = editing("routes-viewport", STREET);
        run(&mut shell, SIZE, "frame\nkey p\nframe");
        viewer.sync(&mut shell);
        let start = route_points(&path);
        let a = route_at(&shell, start[1]);
        let script = format!(
            "press {} {}\nmove {} {}\nframe\nmove {} {}\nframe\nrelease {} {}\nframe",
            a.x,
            a.y,
            a.x - 10.0,
            a.y + 10.0,
            a.x - 40.0,
            a.y + 50.0,
            a.x - 40.0,
            a.y + 50.0
        );
        run(&mut shell, SIZE, &script);
        let moved = route_points(&path);
        assert_eq!(moved.len(), 4);
        assert_ne!(moved[1], start[1]);
        assert_eq!(moved[1][1], 0.0);
        assert_eq!(shell.editor().unwrap().steps(), (1, 0));
        viewer.sync(&mut shell);
        let (b, c) = (route_at(&shell, moved[2]), route_at(&shell, moved[3]));
        let mid = b + (c - b) * 0.5;
        run(
            &mut shell,
            SIZE,
            &format!(
                "press {} {} ctrl\nrelease {} {}\nframe",
                mid.x, mid.y, mid.x, mid.y
            ),
        );
        let added = route_points(&path);
        assert_eq!(added.len(), 5);
        assert_eq!(added[2], moved[2]);
        assert_eq!(added[4], moved[3]);
        viewer.sync(&mut shell);
        let first = route_at(&shell, added[0]);
        run(
            &mut shell,
            SIZE,
            &format!("click {} {}\nframe\nkey Delete\nframe", first.x, first.y),
        );
        let removed = route_points(&path);
        assert_eq!(removed, added[1..].to_vec());
        viewer.sync(&mut shell);
        let end = route_at(&shell, removed[3]);
        let script = format!(
            "press {} {} shift\nmove {} {}\nframe\nmove {} {}\nframe\nrelease {} {}\nframe",
            end.x,
            end.y,
            end.x,
            end.y - 10.0,
            end.x,
            end.y - 40.0,
            end.x,
            end.y - 40.0
        );
        run(&mut shell, SIZE, &script);
        let lifted = route_points(&path);
        assert!(lifted[3][1] > 0.0);
        assert_eq!([lifted[3][0], lifted[3][2]], [removed[3][0], removed[3][2]]);
        assert_eq!(shell.editor().unwrap().steps(), (4, 0));
        for _ in 0..4 {
            run(&mut shell, SIZE, "key ctrl+z\nframe");
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), STREET);
    }
}
