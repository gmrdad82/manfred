use egui::{Align2, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use glam::{DMat3, DMat4, DQuat, DVec3};
use manfred_recipe::{Kind, Patch, Plate, Recipe, number, numbers, params_in};
use toml_edit::{DocumentMut, Item, Table};

use crate::edit::{object_path, round};

use crate::pick::Lens;
use crate::theme::*;
use crate::viewport::Solid;

pub type Matrix = [[f32; 4]; 4];

pub const REACH: f32 = 80.0;
pub const SLACK: f32 = 6.0;
const BOX: f32 = 5.0;
const RING_STEPS: usize = 64;
const START: f64 = 0.2;
const PLANE: [f64; 2] = [0.25, 0.45];
const SNAP_METRES: f64 = 0.001;
const SNAP_PIXELS: f64 = 1.0;
const SNAP_DEGREES: f64 = 5.0;
const SNAP_SCALE: f64 = 0.05;
const LEAST_SCALE: f64 = 0.001;
const LEAST_AREA: f32 = 16.0;
const ORIGIN_SLACK: f64 = 1e-4;
const AXES: [DVec3; 3] = [DVec3::X, DVec3::Y, DVec3::Z];
const COLOURS: [Color32; 3] = [DANGER, OK, INFO];
const NAMES: [&str; 3] = ["x", "y", "z"];
const KEYS: [&str; 3] = ["at", "rotate", "scale"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Move,
    Rotate,
    Scale,
}

impl Mode {
    pub fn word(self) -> &'static str {
        match self {
            Mode::Move => "move",
            Mode::Rotate => "rotate",
            Mode::Scale => "scale",
        }
    }

    fn index(self) -> usize {
        match self {
            Mode::Move => 0,
            Mode::Rotate => 1,
            Mode::Scale => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Space {
    #[default]
    World,
    Local,
}

impl Space {
    pub fn word(self) -> &'static str {
        match self {
            Space::World => "world",
            Space::Local => "local",
        }
    }

    pub fn other(self) -> Space {
        match self {
            Space::World => Space::Local,
            Space::Local => Space::World,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Axis(usize),
    Plane(usize),
    Ring(usize),
    Centre,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Metres,
    Pixels { depth: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pose {
    pub name: String,
    pub at: DVec3,
    pub rotate: DVec3,
    pub scale: DVec3,
    pub parent: DMat4,
    pub place: Place,
    pub uniform: bool,
    pub locks: [Option<String>; 3],
    pub pivot: DVec3,
}

impl Pose {
    pub fn local(&self) -> DMat4 {
        DMat4::from_translation(self.at)
            * manfred_build::rotation(self.rotate.to_array())
            * DMat4::from_scale(self.scale)
    }

    pub fn world(&self) -> DMat4 {
        self.parent * self.local()
    }

    pub fn centre(&self) -> DVec3 {
        self.world().transform_point3(self.pivot)
    }

    fn turned_pivot(&self) -> DVec3 {
        manfred_build::rotation(self.rotate.to_array()).transform_vector3(self.scale * self.pivot)
    }

    fn keeping_centre(&self, start: &Pose) -> DVec3 {
        start.at + start.turned_pivot() - self.turned_pivot()
    }

    pub fn lock(&self, mode: Mode) -> Option<&str> {
        self.locks[mode.index()].as_deref()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Poses {
    pub objects: Vec<Pose>,
    pub plate: Option<Plate>,
}

fn tables(doc: &DocumentMut) -> Vec<Table> {
    doc.get("object")
        .and_then(Item::as_array_of_tables)
        .map(|tables| tables.iter().cloned().collect())
        .unwrap_or_default()
}

impl Poses {
    pub fn of(recipe: &Recipe) -> Poses {
        Poses::in_doc(recipe, &recipe.source.parse().unwrap_or_default())
    }

    pub fn in_doc(recipe: &Recipe, doc: &DocumentMut) -> Poses {
        let tables = tables(doc);
        let objects = recipe
            .objects
            .iter()
            .map(|object| {
                let table = tables
                    .iter()
                    .find(|table| table.get("name").and_then(Item::as_str) == Some(&object.name));
                let get = |key: &str| table.and_then(|table| table.get(key));
                let place = match get("at_px").and_then(Item::as_array) {
                    Some(array) => Place::Pixels {
                        depth: array.len() > 2,
                    },
                    None => Place::Metres,
                };
                let mut locks: [Option<String>; 3] = Default::default();
                let at_key = match place {
                    Place::Metres => "at",
                    Place::Pixels { .. } => "at_px",
                };
                for (lock, key) in locks.iter_mut().zip([at_key, "rotate", "scale"]) {
                    *lock = get(key).and_then(Item::as_value).and_then(params_in).map(|reference| {
                        format!("{key} comes from {reference}; the params file stays its truth")
                    });
                }
                if matches!(object.kind, Kind::Pocket { .. }) {
                    locks = std::array::from_fn(|_| {
                        Some(
                            "a pocket sits in its target's outline; its at, rotate and scale are ignored"
                                .to_string(),
                        )
                    });
                }
                let parent = object
                    .parent
                    .as_deref()
                    .and_then(|name| recipe.objects.iter().rposition(|o| o.name == name))
                    .and_then(|index| manfred_build::world(recipe, index).ok())
                    .unwrap_or(DMat4::IDENTITY);
                Pose {
                    name: object.name.clone(),
                    at: DVec3::from_array(object.at),
                    rotate: DVec3::from_array(object.rotate),
                    scale: DVec3::from_array(object.scale),
                    parent,
                    place,
                    uniform: get("scale").is_some_and(|item| {
                        item.as_value()
                            .is_some_and(|value| value.as_float().is_some() || value.as_integer().is_some())
                    }),
                    locks,
                    pivot: DVec3::ZERO,
                }
            })
            .collect();
        Poses {
            objects,
            plate: recipe.plate.clone(),
        }
    }

    pub fn get(&self, object: usize) -> Option<&Pose> {
        self.objects.get(object)
    }

    pub fn centred(mut self, solids: &[Solid]) -> Poses {
        for (object, pose) in self.objects.iter_mut().enumerate() {
            if solids.iter().any(|solid| solid.objects[0] == object)
                || pose.lock(Mode::Move).is_some()
            {
                continue;
            }
            let Some((min, max)) = solids
                .iter()
                .filter(|solid| solid.objects.contains(&object))
                .map(|solid| (solid.min, solid.max))
                .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
            else {
                continue;
            };
            let centre = (min + max) * 0.5;
            if (centre - pose.centre()).length() < ORIGIN_SLACK {
                continue;
            }
            let Some(inverse) = invertible(pose.world()) else {
                continue;
            };
            pose.pivot = inverse.transform_point3(centre);
        }
        self
    }
}

fn invertible(matrix: DMat4) -> Option<DMat4> {
    (matrix.determinant().abs() > 1e-18).then(|| matrix.inverse())
}

pub fn axes(pose: &Pose, mode: Mode, space: Space) -> [DVec3; 3] {
    if mode != Mode::Scale && space == Space::World {
        return AXES;
    }
    let linear = DMat3::from_mat4(pose.world());
    std::array::from_fn(|k| linear.col(k).try_normalize().unwrap_or(AXES[k]))
}

fn length(lens: &Lens, centre: DVec3) -> f64 {
    f64::from(REACH) * lens.metres_per_point(centre)
}

#[derive(Clone, Debug, PartialEq)]
struct Part {
    handle: Handle,
    points: Vec<Pos2>,
    closed: bool,
}

fn parts(lens: &Lens, pose: &Pose, mode: Mode, space: Space) -> Vec<Part> {
    let centre = pose.centre();
    let axes = axes(pose, mode, space);
    let reach = length(lens, centre);
    let project = |points: &[DVec3]| -> Option<Vec<Pos2>> {
        points.iter().map(|point| lens.project(*point)).collect()
    };
    let mut parts = Vec::new();
    let mut add = |handle: Handle, points: &[DVec3], closed: bool| {
        if let Some(points) = project(points)
            && (!closed || points.len() != 4 || area(&points) >= LEAST_AREA)
        {
            parts.push(Part {
                handle,
                points,
                closed,
            });
        }
    };
    match mode {
        Mode::Move => {
            for k in 0..3 {
                let (i, j) = ((k + 1) % 3, (k + 2) % 3);
                let corner = |a: f64, b: f64| centre + (axes[i] * a + axes[j] * b) * reach;
                add(
                    Handle::Plane(k),
                    &[
                        corner(PLANE[0], PLANE[0]),
                        corner(PLANE[1], PLANE[0]),
                        corner(PLANE[1], PLANE[1]),
                        corner(PLANE[0], PLANE[1]),
                    ],
                    true,
                );
            }
            for (k, axis) in axes.iter().enumerate() {
                add(
                    Handle::Axis(k),
                    &[centre + *axis * reach * START, centre + *axis * reach],
                    false,
                );
            }
        }
        Mode::Rotate => {
            for k in 0..3 {
                let (u, v) = (axes[(k + 1) % 3], axes[(k + 2) % 3]);
                let ring: Vec<DVec3> = (0..RING_STEPS)
                    .map(|step| {
                        let angle = std::f64::consts::TAU * step as f64 / RING_STEPS as f64;
                        centre + (u * angle.cos() + v * angle.sin()) * reach
                    })
                    .collect();
                add(Handle::Ring(k), &ring, true);
            }
        }
        Mode::Scale => {
            for (k, axis) in axes.iter().enumerate() {
                add(Handle::Axis(k), &[centre, centre + *axis * reach], false);
            }
        }
    }
    parts
}

fn area(points: &[Pos2]) -> f32 {
    let twice: f32 = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum();
    twice.abs() / 2.0
}

fn segment_distance(at: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 {
        ((at - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (at - (a + ab * t)).length()
}

fn inside(at: Pos2, points: &[Pos2]) -> bool {
    let mut sign = 0.0_f32;
    for (index, a) in points.iter().enumerate() {
        let b = points[(index + 1) % points.len()];
        let cross = (b - *a).x * (at - *a).y - (b - *a).y * (at - *a).x;
        if cross != 0.0 {
            if sign != 0.0 && cross.signum() != sign {
                return false;
            }
            sign = cross.signum();
        }
    }
    true
}

fn distance(part: &Part, at: Pos2) -> f32 {
    if part.closed && part.points.len() == 4 && inside(at, &part.points) {
        return 0.0;
    }
    let count = if part.closed {
        part.points.len()
    } else {
        part.points.len() - 1
    };
    (0..count)
        .map(|index| {
            segment_distance(
                at,
                part.points[index],
                part.points[(index + 1) % part.points.len()],
            )
        })
        .fold(f32::INFINITY, f32::min)
}

pub fn hit(lens: &Lens, pose: &Pose, mode: Mode, space: Space, at: Pos2) -> Option<Handle> {
    if mode == Mode::Scale
        && let Some(centre) = lens.project(pose.centre())
        && (at.x - centre.x).abs().max((at.y - centre.y).abs()) <= BOX + 2.0 + SLACK
    {
        return Some(Handle::Centre);
    }
    let mut best: Option<(f32, Handle)> = None;
    for part in parts(lens, pose, mode, space) {
        let gap = distance(&part, at);
        if gap <= SLACK && best.is_none_or(|(nearest, _)| gap < nearest) {
            best = Some((gap, part.handle));
        }
    }
    best.map(|(_, handle)| handle)
}

fn closest_on_line(point: DVec3, direction: DVec3, ray: &crate::pick::Ray) -> Option<f64> {
    let b = direction.dot(ray.direction);
    let denominator = 1.0 - b * b;
    if denominator < 1e-9 {
        return None;
    }
    let w = point - ray.origin;
    Some((b * ray.direction.dot(w) - direction.dot(w)) / denominator)
}

fn on_plane(point: DVec3, normal: DVec3, ray: &crate::pick::Ray) -> Option<DVec3> {
    let facing = ray.direction.dot(normal);
    if facing.abs() < 1e-9 {
        return None;
    }
    let distance = (point - ray.origin).dot(normal) / facing;
    Some(ray.at(distance))
}

pub fn to_key(at: DVec3, place: Place, plate: Option<&Plate>) -> DVec3 {
    match (place, plate) {
        (Place::Pixels { .. }, Some(plate)) => {
            let [x, y] = plate.to_pixels([at.x, at.y]);
            DVec3::new(x, y, at.z)
        }
        _ => at,
    }
}

pub fn from_key(key: DVec3, place: Place, plate: Option<&Plate>) -> DVec3 {
    match (place, plate) {
        (Place::Pixels { .. }, Some(plate)) => {
            let [x, y] = plate.to_metres([key.x, key.y]);
            DVec3::new(x, y, key.z)
        }
        _ => key,
    }
}

fn snapped(value: f64, step: f64) -> f64 {
    (value / step).round() * step
}

pub fn euler(rotation: DMat3) -> DVec3 {
    let r20 = rotation.col(0).z;
    let (x, y, z) = if r20.abs() < 1.0 - 1e-12 {
        (
            rotation.col(1).z.atan2(rotation.col(2).z),
            (-r20).asin(),
            rotation.col(0).y.atan2(rotation.col(0).x),
        )
    } else {
        (
            0.0,
            (-r20.clamp(-1.0, 1.0)).asin(),
            (-rotation.col(1).x).atan2(rotation.col(1).y),
        )
    };
    DVec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
}

fn nearest_turn(angle: f64, near: f64) -> f64 {
    angle + ((near - angle) / 360.0).round() * 360.0
}

#[derive(Clone, Debug)]
pub struct Drag {
    pub object: usize,
    pub handle: Handle,
    pub mode: Mode,
    pub start: Pose,
    pub pose: Pose,
    axes: [DVec3; 3],
    lens: Lens,
    origin: Pos2,
    last: Pos2,
    turned: f64,
}

impl Drag {
    pub fn begin(
        object: usize,
        pose: &Pose,
        mode: Mode,
        space: Space,
        handle: Handle,
        origin: Pos2,
        lens: Lens,
    ) -> Drag {
        Drag {
            object,
            handle,
            mode,
            start: pose.clone(),
            pose: pose.clone(),
            axes: axes(pose, mode, space),
            lens,
            origin,
            last: origin,
            turned: 0.0,
        }
    }

    pub fn to(&mut self, at: Pos2, snap: bool, plate: Option<&Plate>) {
        let mut pose = self.start.clone();
        match self.mode {
            Mode::Move => pose.at = self.moved(at, snap, plate),
            Mode::Rotate => pose.rotate = self.turned(at, snap),
            Mode::Scale => pose.scale = self.scaled(at, snap),
        }
        if self.mode != Mode::Move {
            pose.at = pose.keeping_centre(&self.start);
        }
        self.pose = pose;
    }

    fn moved(&self, at: Pos2, snap: bool, plate: Option<&Plate>) -> DVec3 {
        let centre = self.start.centre();
        let from = self.lens.ray(self.origin);
        let to = self.lens.ray(at);
        let shift = match self.handle {
            Handle::Axis(k) => {
                let axis = self.axes[k];
                let from = self.lens.ray(self.on_axis(self.origin, axis));
                let to = self.lens.ray(self.on_axis(at, axis));
                match (
                    closest_on_line(centre, axis, &from),
                    closest_on_line(centre, axis, &to),
                ) {
                    (Some(a), Some(b)) => axis * (b - a),
                    _ => DVec3::ZERO,
                }
            }
            Handle::Plane(k) => {
                let normal = self.axes[k];
                match (
                    on_plane(centre, normal, &from),
                    on_plane(centre, normal, &to),
                ) {
                    (Some(a), Some(b)) => b - a,
                    _ => DVec3::ZERO,
                }
            }
            Handle::Ring(_) | Handle::Centre => DVec3::ZERO,
        };
        let moved = self
            .start
            .parent
            .inverse()
            .transform_point3(self.start.parent.transform_point3(self.start.at) + shift);
        if !snap {
            return moved;
        }
        let place = self.start.place;
        let before = to_key(self.start.at, place, plate);
        let after = to_key(moved, place, plate);
        let steps = match (place, plate) {
            (Place::Pixels { .. }, Some(_)) => [SNAP_PIXELS, SNAP_PIXELS, SNAP_METRES],
            _ => [SNAP_METRES; 3],
        };
        let key = DVec3::from_array(std::array::from_fn(|k| {
            before[k] + snapped(after[k] - before[k], steps[k])
        }));
        from_key(key, place, plate)
    }

    fn turned(&mut self, at: Pos2, snap: bool) -> DVec3 {
        let Handle::Ring(k) = self.handle else {
            return self.start.rotate;
        };
        let centre = self.start.centre();
        if let Some(middle) = self.lens.project(centre) {
            let (a, b) = (self.last - middle, at - middle);
            if a.length() > 1.0 && b.length() > 1.0 {
                let cross = f64::from(a.x * b.y - a.y * b.x);
                let dot = f64::from(a.dot(b));
                self.turned -= cross.atan2(dot);
            }
        }
        self.last = at;
        let axis = self.axes[k];
        let sign = if axis.dot(self.lens.toward_eye(centre)) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let mut degrees = (self.turned * sign).to_degrees();
        if snap {
            degrees = snapped(degrees, SNAP_DEGREES);
        }
        turn(&self.start, axis, degrees)
    }

    fn scaled(&self, at: Pos2, snap: bool) -> DVec3 {
        let moved = at - self.origin;
        let centre = self.start.centre();
        let factor = match self.handle {
            Handle::Axis(k) => {
                let reach = length(&self.lens, centre);
                match (
                    self.lens.project(centre),
                    self.lens.project(centre + self.axes[k] * reach),
                ) {
                    (Some(a), Some(b)) if (b - a).length() > 1.0 => {
                        1.0 + f64::from(moved.dot(b - a) / (b - a).length_sq())
                    }
                    _ => 1.0,
                }
            }
            Handle::Centre => 1.0 + f64::from((moved.x - moved.y) / REACH),
            Handle::Plane(_) | Handle::Ring(_) => 1.0,
        };
        let factor = factor.max(LEAST_SCALE);
        let mut scale = self.start.scale;
        let touched: Vec<usize> = match self.handle {
            Handle::Axis(k) => vec![k],
            _ => vec![0, 1, 2],
        };
        for k in touched {
            scale[k] *= factor;
            if snap {
                let size = snapped(scale[k].abs(), SNAP_SCALE).max(SNAP_SCALE);
                scale[k] = size.copysign(self.start.scale[k]);
            }
        }
        scale
    }

    pub fn motion(&self) -> Matrix {
        motion(&self.start, &self.pose)
    }

    pub fn label(&self) -> String {
        let what = match self.handle {
            Handle::Axis(k) => format!(" along {}", NAMES[k]),
            Handle::Ring(k) => format!(" about {}", NAMES[k]),
            Handle::Plane(k) => format!(" in {}{}", NAMES[(k + 1) % 3], NAMES[(k + 2) % 3]),
            Handle::Centre => String::new(),
        };
        format!("{} {}{what}", self.mode.word(), self.start.name)
    }

    fn on_axis(&self, at: Pos2, axis: DVec3) -> Pos2 {
        let centre = self.start.centre();
        let reach = length(&self.lens, centre);
        match (
            self.lens.project(centre),
            self.lens.project(centre + axis * reach),
        ) {
            (Some(a), Some(b)) if (b - a).length() > 1.0 => {
                let direction = (b - a).normalized();
                a + direction * (at - a).dot(direction)
            }
            _ => at,
        }
    }

    pub fn patches(&self, plate: Option<&Plate>) -> Vec<Patch> {
        patches(&self.start, &self.pose, plate)
    }
}

pub fn turn(pose: &Pose, axis: DVec3, degrees: f64) -> DVec3 {
    let spin = DQuat::from_axis_angle(axis, degrees.to_radians());
    let (_, parent, _) = pose.parent.to_scale_rotation_translation();
    let local = parent.inverse() * spin * parent;
    let before = DMat3::from_mat4(manfred_build::rotation(pose.rotate.to_array()));
    let after = euler(DMat3::from_quat(local) * before);
    DVec3::from_array(std::array::from_fn(|k| {
        nearest_turn(after[k], pose.rotate[k])
    }))
}

pub fn motion(start: &Pose, pose: &Pose) -> Matrix {
    let motion = pose.world() * start.world().inverse();
    motion
        .to_cols_array_2d()
        .map(|column| column.map(|v| v as f32))
}

pub fn patches(start: &Pose, pose: &Pose, plate: Option<&Plate>) -> Vec<Patch> {
    let mut patches = Vec::new();
    let place = match (start.place, plate) {
        (Place::Pixels { depth }, Some(_)) => Place::Pixels { depth },
        _ => Place::Metres,
    };
    let at = |pose: &Pose| -> Vec<f64> {
        let key = to_key(pose.at, place, plate);
        match place {
            Place::Pixels { .. } => vec![round(key.x, 3), round(key.y, 3), round(key.z, 6)],
            Place::Metres => key.to_array().map(|v| round(v, 6)).to_vec(),
        }
    };
    let (before, after) = (at(start), at(pose));
    if before != after {
        let (key, values) = match place {
            Place::Pixels { depth } if !depth && after[2] == 0.0 => ("at_px", &after[..2]),
            Place::Pixels { .. } => ("at_px", &after[..]),
            Place::Metres => (KEYS[0], &after[..]),
        };
        patches.push(Patch {
            path: object_path(&start.name, key),
            value: Some(numbers(values)),
        });
    }
    let rotate = |pose: &Pose| pose.rotate.to_array().map(|v| round(v, 4));
    if rotate(start) != rotate(pose) {
        patches.push(Patch {
            path: object_path(&start.name, KEYS[1]),
            value: Some(numbers(&rotate(pose))),
        });
    }
    let scale = |pose: &Pose| pose.scale.to_array().map(|v| round(v, 6));
    let after = scale(pose);
    if scale(start) != after {
        let value = if start.uniform && after[0] == after[1] && after[1] == after[2] {
            number(after[0])
        } else {
            numbers(&after)
        };
        patches.push(Patch {
            path: object_path(&start.name, KEYS[2]),
            value: Some(value),
        });
    }
    patches
}

#[derive(Clone, Debug, Default)]
pub struct Gizmo {
    pub mode: Mode,
    pub space: Space,
    poses: Poses,
    drag: Option<Drag>,
    held: Option<(usize, Pose)>,
    hover: Option<Handle>,
    swallow: bool,
}

impl Gizmo {
    pub fn set_poses(&mut self, poses: Poses) {
        self.poses = poses;
        self.held = None;
    }

    pub fn poses(&self) -> &Poses {
        &self.poses
    }

    pub fn dragging(&self) -> Option<&Drag> {
        self.drag.as_ref()
    }

    pub fn swallowing(&self) -> bool {
        self.swallow
    }

    pub fn hover(&mut self, handle: Option<Handle>) {
        self.hover = handle;
    }

    pub fn shown(&self, object: usize) -> Option<&Pose> {
        match (&self.drag, &self.held) {
            (Some(drag), _) if drag.object == object => Some(&drag.pose),
            (_, Some((held, pose))) if *held == object => Some(pose),
            _ => self.poses.get(object),
        }
    }

    pub fn hit(&self, lens: &Lens, object: usize, at: Pos2) -> Option<Handle> {
        let pose = self.shown(object)?;
        hit(lens, pose, self.mode, self.space, at)
    }

    pub fn begin(&mut self, object: usize, at: Pos2, lens: Lens) -> bool {
        let Some(pose) = self.shown(object).cloned() else {
            return false;
        };
        if pose.lock(self.mode).is_some() {
            return false;
        }
        let Some(handle) = hit(&lens, &pose, self.mode, self.space, at) else {
            return false;
        };
        self.drag = Some(Drag::begin(
            object, &pose, self.mode, self.space, handle, at, lens,
        ));
        self.held = None;
        true
    }

    pub fn to(&mut self, at: Pos2, snap: bool) {
        let plate = self.poses.plate.clone();
        if let Some(drag) = &mut self.drag {
            drag.to(at, snap, plate.as_ref());
        }
    }

    pub fn finish(&mut self) -> Option<(String, Vec<Patch>)> {
        self.swallow = false;
        let drag = self.drag.take()?;
        let patches = drag.patches(self.poses.plate.as_ref());
        if patches.is_empty() {
            return None;
        }
        self.held = Some((drag.object, drag.pose.clone()));
        Some((drag.label(), patches))
    }

    pub fn cancel(&mut self) -> bool {
        let cancelled = self.drag.take().is_some();
        self.swallow |= cancelled;
        cancelled
    }

    pub fn drop_held(&mut self) {
        self.held = None;
    }

    pub fn preview(&self) -> Option<(usize, Matrix)> {
        let (object, start, pose) = match (&self.drag, &self.held) {
            (Some(drag), _) => (drag.object, &drag.start, &drag.pose),
            (None, Some((object, pose))) => (*object, self.poses.get(*object)?, pose),
            (None, None) => return None,
        };
        Some((object, motion(start, pose)))
    }

    pub fn draw(&self, painter: &Painter, lens: &Lens, object: usize) {
        let Some(pose) = self.shown(object) else {
            return;
        };
        let lock = pose.lock(self.mode);
        let active = self.drag.as_ref().map(|drag| drag.handle);
        let lit = active.or(self.hover);
        let colour = |handle: Handle, k: usize| {
            if lock.is_some() {
                TEXT_DIM
            } else if lit == Some(handle) {
                ACCENT
            } else {
                COLOURS[k]
            }
        };
        let Some(centre) = lens.project(pose.centre()) else {
            return;
        };
        for part in parts(lens, pose, self.mode, self.space) {
            match part.handle {
                Handle::Plane(k) => {
                    let tone = colour(part.handle, k);
                    painter.add(Shape::convex_polygon(
                        part.points.clone(),
                        tone.gamma_multiply(0.3),
                        Stroke::new(STROKE, tone),
                    ));
                }
                Handle::Axis(k) => {
                    let tone = colour(part.handle, k);
                    let (from, end) = (part.points[0], part.points[1]);
                    painter.line_segment([from, end], Stroke::new(2.0_f32, tone));
                    let direction = (end - centre).normalized();
                    if self.mode == Mode::Move {
                        let side = Vec2::new(-direction.y, direction.x) * 5.0;
                        painter.add(Shape::convex_polygon(
                            vec![end + direction * 12.0, end + side, end - side],
                            tone,
                            Stroke::NONE,
                        ));
                    } else {
                        painter.rect_filled(
                            Rect::from_center_size(end, Vec2::splat(BOX * 2.0)),
                            CornerRadius::ZERO,
                            tone,
                        );
                    }
                    painter.text(
                        end + direction * 22.0,
                        Align2::CENTER_CENTER,
                        NAMES[k],
                        Theme::of(painter.ctx()).font(SIZE_CODE),
                        tone,
                    );
                }
                Handle::Ring(k) => {
                    let tone = colour(part.handle, k);
                    let mut points = part.points.clone();
                    points.push(points[0]);
                    painter.add(Shape::line(points, Stroke::new(2.0_f32, tone)));
                }
                Handle::Centre => {}
            }
        }
        let middle = if lock.is_some() {
            TEXT_DIM
        } else if self.mode == Mode::Scale && lit == Some(Handle::Centre) {
            ACCENT_TEXT
        } else {
            ACCENT
        };
        if self.mode == Mode::Scale {
            painter.rect_stroke(
                Rect::from_center_size(centre, Vec2::splat(BOX * 2.0 + 4.0)),
                CornerRadius::ZERO,
                Stroke::new(1.5_f32, if lock.is_some() { TEXT_DIM } else { ACCENT }),
                egui::StrokeKind::Middle,
            );
        }
        painter.circle_filled(centre, 3.0, middle);
        if let Some(reason) = lock {
            let text = painter.layout(
                reason.to_string(),
                Theme::of(painter.ctx()).font(SIZE_SMALL),
                TEXT_SECONDARY,
                (lens.image().width() * 0.5).max(160.0),
            );
            let at = centre + Vec2::new(14.0, 14.0);
            let band = Rect::from_min_size(at, text.size() + Vec2::new(16.0, 8.0));
            painter.rect_filled(band, CornerRadius::same(RADIUS), BG_DEEP);
            painter.rect_stroke(
                band,
                CornerRadius::same(RADIUS),
                Stroke::new(STROKE, LINE_BORDER),
                egui::StrokeKind::Inside,
            );
            painter.galley(at + Vec2::new(8.0, 4.0), text, TEXT_SECONDARY);
        }
    }

    pub fn badge(&self, painter: &Painter, image: Rect) {
        let space = if self.mode == Mode::Scale {
            Space::Local
        } else {
            self.space
        };
        let text = painter.layout_no_wrap(
            format!("{} · {}", self.mode.word(), space.word()),
            Theme::of(painter.ctx()).font(SIZE_SMALL),
            ACCENT,
        );
        let size = text.size() + Vec2::new(16.0, 8.0);
        let band = Rect::from_min_size(
            Pos2::new(
                image.right() - SECTION_GAP - size.x,
                image.top() + SECTION_GAP,
            ),
            size,
        );
        painter.rect_filled(band, CornerRadius::same(RADIUS), BG_DEEP);
        painter.galley(band.min + Vec2::new(8.0, 4.0), text, ACCENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_recipe::{Key, Origin};
    use manfred_scene::Eye;

    const PLAIN: &str = "manfred = 1\nname = \"g\"\n\n[[object]]\nname = \"box\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\nat = [0.5, 0.0, 0.0]\n\n[[object]]\nname = \"child\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\nparent = \"box\"\nat = [0.0, 1.0, 0.0]\nscale = 2.0\n";

    fn front() -> Eye {
        Eye {
            position: [0.0, 0.0, 4.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 90.0,
            shift: [0.0, 0.0],
            ortho_height: None,
        }
    }

    fn image() -> Rect {
        Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(800.0, 400.0))
    }

    fn pose(at: [f64; 3]) -> Pose {
        Pose {
            name: "thing".into(),
            at: DVec3::from_array(at),
            rotate: DVec3::ZERO,
            scale: DVec3::ONE,
            parent: DMat4::IDENTITY,
            place: Place::Metres,
            uniform: false,
            locks: Default::default(),
            pivot: DVec3::ZERO,
        }
    }

    fn close(a: DVec3, b: DVec3) -> bool {
        (a - b).length() < 1e-9
    }

    fn text(patch: &Patch) -> String {
        format!(
            "{} = {}",
            key_text(&patch.path),
            patch.value.as_ref().unwrap().to_string().trim()
        )
    }

    fn key_text(path: &[Key]) -> String {
        path.iter()
            .map(|key| match key {
                Key::Table(name) | Key::Object(name) => name.clone(),
                Key::Index(index) => index.to_string(),
            })
            .collect::<Vec<_>>()
            .join(".")
    }

    #[test]
    fn the_x_arrow_projects_where_the_hand_says() {
        let lens = Lens::new(&front(), image());
        let thing = pose([0.0, 0.0, 0.0]);
        let reach = length(&lens, thing.centre());
        assert!((reach - 80.0 * 8.0 / 400.0).abs() < 1e-12);
        let found = parts(&lens, &thing, Mode::Move, Space::World);
        let x = found
            .iter()
            .find(|part| part.handle == Handle::Axis(0))
            .unwrap();
        assert!((x.points[1] - Pos2::new(480.0, 200.0)).length() < 1e-3);
        assert!((x.points[0] - Pos2::new(416.0, 200.0)).length() < 1e-3);
        let y = found
            .iter()
            .find(|part| part.handle == Handle::Axis(1))
            .unwrap();
        assert!((y.points[1] - Pos2::new(400.0, 120.0)).length() < 1e-3);
    }

    #[test]
    fn hits_find_the_handle_within_the_slack() {
        let lens = Lens::new(&front(), image());
        let thing = pose([0.0, 0.0, 0.0]);
        let at = |x, y| hit(&lens, &thing, Mode::Move, Space::World, Pos2::new(x, y));
        assert_eq!(at(470.0, 204.0), Some(Handle::Axis(0)));
        assert_eq!(at(470.0, 210.0), None);
        assert_eq!(at(400.0, 130.0), Some(Handle::Axis(1)));
        assert_eq!(at(428.0, 172.0), Some(Handle::Plane(2)));
        assert_eq!(at(300.0, 300.0), None);
        let ring = |x, y| hit(&lens, &thing, Mode::Rotate, Space::World, Pos2::new(x, y));
        assert_eq!(
            ring(400.0 + 80.0 * 0.6, 200.0 - 80.0 * 0.8),
            Some(Handle::Ring(2))
        );
        assert_eq!(ring(600.0, 350.0), None);
        let scale = |x, y| hit(&lens, &thing, Mode::Scale, Space::World, Pos2::new(x, y));
        assert_eq!(scale(402.0, 199.0), Some(Handle::Centre));
        assert_eq!(scale(478.0, 202.0), Some(Handle::Axis(0)));
    }

    #[test]
    fn hits_follow_the_lens_shift() {
        let mut eye = front();
        eye.shift = [0.25, 0.0];
        let lens = Lens::new(&eye, image());
        let thing = pose([0.0, 0.0, 0.0]);
        assert_eq!(lens.project(DVec3::ZERO), Some(Pos2::new(300.0, 200.0)));
        let at = |x, y| hit(&lens, &thing, Mode::Move, Space::World, Pos2::new(x, y));
        assert_eq!(at(370.0, 200.0), Some(Handle::Axis(0)));
        assert_eq!(at(470.0, 200.0), None);
    }

    #[test]
    fn a_drag_of_n_points_along_x_moves_by_n_times_the_metres_per_point() {
        let lens = Lens::new(&front(), image());
        let thing = pose([0.0, 0.0, 0.0]);
        let mut drag = Drag::begin(
            0,
            &thing,
            Mode::Move,
            Space::World,
            Handle::Axis(0),
            Pos2::new(460.0, 200.0),
            lens,
        );
        drag.to(Pos2::new(510.0, 230.0), false, None);
        let metres = 50.0 * 8.0 / 400.0;
        assert!(
            close(drag.pose.at, DVec3::new(metres, 0.0, 0.0)),
            "{:?}",
            drag.pose.at
        );
        drag.to(Pos2::new(510.0, 200.0), false, None);
        assert!(
            close(drag.pose.at, DVec3::new(metres, 0.0, 0.0)),
            "{:?}",
            drag.pose.at
        );
        let motion = drag.motion();
        assert!((motion[3][0] - metres as f32).abs() < 1e-6);
        assert_eq!(motion[0], [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_plane_drag_follows_the_pointer_in_the_plane() {
        let lens = Lens::new(&front(), image());
        let thing = pose([0.0, 0.0, 0.0]);
        let mut drag = Drag::begin(
            0,
            &thing,
            Mode::Move,
            Space::World,
            Handle::Plane(2),
            Pos2::new(428.0, 172.0),
            lens,
        );
        drag.to(Pos2::new(438.0, 152.0), false, None);
        let mpp = 8.0 / 400.0;
        assert!(close(drag.pose.at, DVec3::new(10.0 * mpp, 20.0 * mpp, 0.0)));
    }

    #[test]
    fn a_child_moves_in_its_parents_frame() {
        let lens = Lens::new(&front(), image());
        let mut child = pose([0.1, 0.0, 0.0]);
        child.parent = DMat4::from_translation(DVec3::new(0.0, 0.0, 0.0))
            * manfred_build::rotation([0.0, 0.0, 90.0])
            * DMat4::from_scale(DVec3::splat(2.0));
        let centre = child.centre();
        assert!(close(centre, DVec3::new(0.0, 0.2, 0.0)));
        let start = lens
            .project(centre + DVec3::X * length(&lens, centre) * 0.6)
            .unwrap();
        let mut drag = Drag::begin(
            1,
            &child,
            Mode::Move,
            Space::World,
            Handle::Axis(0),
            start,
            lens,
        );
        drag.to(start + Vec2::new(20.0, 0.0), false, None);
        let metres = 20.0 * 8.0 / 400.0;
        assert!(close(
            drag.pose.centre(),
            centre + DVec3::new(metres, 0.0, 0.0)
        ));
        assert!(
            close(drag.pose.at, DVec3::new(0.1, -metres / 2.0, 0.0)),
            "{:?}",
            drag.pose.at
        );
        let patches = drag.patches(None);
        assert_eq!(patches.len(), 1);
        assert_eq!(text(&patches[0]), "object.thing.at = [0.1, -0.2, 0.0]");
    }

    #[test]
    fn a_ring_drag_turns_by_the_angle_swept_around_the_centre() {
        let lens = Lens::new(&front(), image());
        let thing = pose([0.0, 0.0, 0.0]);
        let mut drag = Drag::begin(
            0,
            &thing,
            Mode::Rotate,
            Space::World,
            Handle::Ring(2),
            Pos2::new(480.0, 200.0),
            lens,
        );
        drag.to(Pos2::new(457.0, 143.0), false, None);
        drag.to(Pos2::new(400.0, 120.0), false, None);
        assert!(
            close(drag.pose.rotate, DVec3::new(0.0, 0.0, 90.0)),
            "{:?}",
            drag.pose.rotate
        );
        drag.to(Pos2::new(320.0, 200.0), false, None);
        drag.to(Pos2::new(400.0, 280.0), false, None);
        drag.to(Pos2::new(480.0, 201.0), false, None);
        assert!((drag.pose.rotate.z.abs() - 360.0).abs() < 1.0 || drag.pose.rotate.z.abs() < 1.0);
        let mut snap = Drag::begin(
            0,
            &thing,
            Mode::Rotate,
            Space::World,
            Handle::Ring(2),
            Pos2::new(480.0, 200.0),
            lens,
        );
        snap.to(Pos2::new(480.0, 190.0), true, None);
        let swept = (10.0_f64 / 80.0).atan().to_degrees();
        assert!(swept > 7.0 && swept < 7.5);
        assert!(
            close(snap.pose.rotate, DVec3::new(0.0, 0.0, 5.0)),
            "{:?}",
            snap.pose.rotate
        );
    }

    #[test]
    fn a_ring_seen_from_behind_turns_the_other_way() {
        let mut back = front();
        back.position = [0.0, 0.0, -4.0];
        let lens = Lens::new(&back, image());
        let thing = pose([0.0, 0.0, 0.0]);
        let start = lens.project(DVec3::new(0.16, 0.0, 0.0)).unwrap();
        let mut drag = Drag::begin(
            0,
            &thing,
            Mode::Rotate,
            Space::World,
            Handle::Ring(2),
            start,
            lens,
        );
        drag.to(Pos2::new(400.0, 120.0), false, None);
        assert!(
            close(drag.pose.rotate, DVec3::new(0.0, 0.0, 90.0)),
            "{:?}",
            drag.pose.rotate
        );
    }

    #[test]
    fn local_rotation_turns_about_the_objects_own_axis() {
        let mut thing = pose([0.0, 0.0, 0.0]);
        thing.rotate = DVec3::new(0.0, 0.0, -90.0);
        let local = axes(&thing, Mode::Rotate, Space::Local);
        assert!(close(local[0], DVec3::new(0.0, -1.0, 0.0)));
        let turned = turn(&thing, local[0], 30.0);
        let expected = manfred_build::rotation([30.0, 0.0, -90.0]);
        let got = manfred_build::rotation(turned.to_array());
        assert!(got.abs_diff_eq(expected, 1e-9), "{turned:?}");
        let world = turn(&thing, DVec3::X, 30.0);
        let expected =
            manfred_build::rotation([30.0, 0.0, 0.0]) * manfred_build::rotation([0.0, 0.0, -90.0]);
        assert!(manfred_build::rotation(world.to_array()).abs_diff_eq(expected, 1e-9));
    }

    #[test]
    fn euler_round_trips_the_builders_rotation() {
        for degrees in [
            [10.0, 20.0, 30.0],
            [-45.0, 80.0, 170.0],
            [90.0, 0.0, -90.0],
            [0.0, 90.0, 30.0],
            [0.0, -90.0, 0.0],
        ] {
            let matrix = manfred_build::rotation(degrees);
            let back = euler(DMat3::from_mat4(matrix));
            let again = manfred_build::rotation(back.to_array());
            assert!(again.abs_diff_eq(matrix, 1e-9), "{degrees:?} -> {back:?}");
        }
    }

    #[test]
    fn scale_handles_grow_by_the_dragged_share_of_the_reach() {
        let lens = Lens::new(&front(), image());
        let thing = pose([0.0, 0.0, 0.0]);
        let mut drag = Drag::begin(
            0,
            &thing,
            Mode::Scale,
            Space::World,
            Handle::Axis(0),
            Pos2::new(478.0, 200.0),
            lens,
        );
        drag.to(Pos2::new(518.0, 230.0), false, None);
        assert!(close(drag.pose.scale, DVec3::new(1.5, 1.0, 1.0)));
        let mut uniform = Drag::begin(
            0,
            &thing,
            Mode::Scale,
            Space::World,
            Handle::Centre,
            Pos2::new(400.0, 200.0),
            lens,
        );
        uniform.to(Pos2::new(420.0, 180.0), false, None);
        assert!(close(uniform.pose.scale, DVec3::splat(1.5)));
        uniform.to(Pos2::new(408.0, 200.0), true, None);
        assert!(
            close(uniform.pose.scale, DVec3::splat(1.1)),
            "{:?}",
            uniform.pose.scale
        );
        uniform.to(Pos2::new(0.0, 400.0), false, None);
        assert!(close(uniform.pose.scale, DVec3::splat(LEAST_SCALE)));
    }

    #[test]
    fn snapping_moves_in_whole_millimetres_from_where_it_was() {
        let lens = Lens::new(&front(), image());
        let thing = pose([0.0375, 0.0, 0.0]);
        let start = lens.project(thing.centre() + DVec3::X * 0.8).unwrap();
        let mut drag = Drag::begin(
            0,
            &thing,
            Mode::Move,
            Space::World,
            Handle::Axis(0),
            start,
            lens,
        );
        drag.to(start + Vec2::new(1.32, 0.0), false, None);
        assert!(
            (drag.pose.at.x - (0.0375 + 0.0264)).abs() < 1e-6,
            "{:?}",
            drag.pose.at
        );
        drag.to(start + Vec2::new(1.32, 0.0), true, None);
        assert!(
            close(drag.pose.at, DVec3::new(0.0375 + 0.026, 0.0, 0.0)),
            "{:?}",
            drag.pose.at
        );
    }

    fn plate() -> Plate {
        Plate {
            width: 1000.0,
            height: 500.0,
            metres_per_pixel: 0.001,
            origin: Origin::TopLeft,
            camera: None,
        }
    }

    #[test]
    fn plate_pixels_convert_both_ways() {
        let plate = plate();
        let place = Place::Pixels { depth: false };
        let key = DVec3::new(509.0, 63.0, -0.003);
        let metres = from_key(key, place, Some(&plate));
        assert!(close(metres, DVec3::new(0.009, 0.187, -0.003)));
        assert!(close(to_key(metres, place, Some(&plate)), key));
        assert!(close(to_key(metres, Place::Metres, Some(&plate)), metres));
    }

    #[test]
    fn snapping_under_a_plate_moves_whole_pixels() {
        let lens = Lens::new(&front(), image());
        let plate = plate();
        let mut thing = pose([0.0, 0.0, 0.0]);
        thing.place = Place::Pixels { depth: false };
        thing.at = from_key(DVec3::new(500.5, 250.0, 0.0), thing.place, Some(&plate));
        let start = lens.project(thing.centre() + DVec3::X * 0.8).unwrap();
        let mut drag = Drag::begin(
            0,
            &thing,
            Mode::Move,
            Space::World,
            Handle::Axis(0),
            start,
            lens,
        );
        drag.to(start + Vec2::new(1.32, 0.0), true, Some(&plate));
        let key = to_key(drag.pose.at, thing.place, Some(&plate));
        assert!(close(key, DVec3::new(526.5, 250.0, 0.0)), "{key:?}");
        let patches = drag.patches(Some(&plate));
        assert_eq!(text(&patches[0]), "object.thing.at_px = [526.5, 250.0]");
    }

    #[test]
    fn patches_use_the_recipes_own_keys() {
        let recipe = manfred_recipe::parse(PLAIN).unwrap();
        let poses = Poses::of(&recipe);
        let start = &poses.objects[0];
        assert_eq!(start.place, Place::Metres);
        let mut moved = start.clone();
        moved.at.x += 0.25;
        moved.rotate.y = 12.5;
        let written: Vec<String> = patches(start, &moved, None).iter().map(text).collect();
        assert_eq!(
            written,
            [
                "object.box.at = [0.75, 0.0, 0.0]",
                "object.box.rotate = [0.0, 12.5, 0.0]"
            ]
        );
        let child = &poses.objects[1];
        assert!(child.uniform);
        assert!(close(child.centre(), DVec3::new(0.5, 1.0, 0.0)));
        let mut bigger = child.clone();
        bigger.scale = DVec3::splat(3.0);
        assert_eq!(
            patches(child, &bigger, None)
                .iter()
                .map(text)
                .collect::<Vec<_>>(),
            ["object.child.scale = 3.0"]
        );
        bigger.scale.x = 2.5;
        assert_eq!(
            text(&patches(child, &bigger, None)[0]),
            "object.child.scale = [2.5, 3.0, 3.0]"
        );
        assert!(patches(child, child, None).is_empty());
    }

    #[test]
    fn patches_apply_to_the_recipe_and_keep_its_text() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp/ui-tests/gizmo-patch/scene.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            PLAIN.replace("at = [0.5, 0.0, 0.0]", "at = [0.5, 0.0, 0.0] # hand placed"),
        )
        .unwrap();
        let mut loaded = manfred_recipe::load(&path).unwrap();
        let poses = Poses::of(&loaded.recipe);
        let mut moved = poses.objects[0].clone();
        moved.at.y = 0.125;
        for patch in patches(&poses.objects[0], &moved, None) {
            manfred_recipe::apply(&mut loaded, &patch).unwrap();
        }
        let written = manfred_recipe::write(&loaded);
        assert!(
            written.contains("at = [0.5, 0.125, 0.0] # hand placed\n"),
            "{written}"
        );
        assert_eq!(loaded.recipe.objects[0].at, [0.5, 0.125, 0.0]);
    }

    #[test]
    fn params_and_pockets_lock_the_gizmo_with_a_reason() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp/ui-tests/gizmo-params");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.json"), "{\"lamp\": {\"x\": 0.3}}").unwrap();
        let text = "manfred = 1\nname = \"p\"\nparams = \"p.json\"\n\n[[object]]\nname = \"lamp\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\nat = [\"${params.lamp.x}\", 0.0, 0.0]\n\n[[object]]\nname = \"slab\"\nkind = \"outline\"\noutline = { rect = [0.0, 0.0, 1.0, 1.0] }\nfront = 0.0\nback = -0.1\n\n[[object]]\nname = \"cut\"\nkind = \"pocket\"\ninto = \"slab\"\noutline = { rect = [0.2, 0.2, 0.4, 0.4] }\ndepth = -0.05\n";
        std::fs::write(dir.join("scene.toml"), text).unwrap();
        let loaded = manfred_recipe::load(&dir.join("scene.toml")).unwrap();
        let poses = Poses::of(&loaded.recipe);
        let lamp = &poses.objects[0];
        assert_eq!(
            lamp.lock(Mode::Move),
            Some("at comes from params.lamp.x; the params file stays its truth")
        );
        assert_eq!(lamp.lock(Mode::Rotate), None);
        assert!(poses.objects[1].lock(Mode::Move).is_none());
        assert!(
            poses.objects[2]
                .lock(Mode::Scale)
                .unwrap()
                .starts_with("a pocket")
        );
        let mut gizmo = Gizmo::default();
        gizmo.set_poses(poses);
        let lens = Lens::new(&front(), image());
        let on = lens
            .project(DVec3::new(0.3, 0.0, 0.0) + DVec3::X * 0.12)
            .unwrap();
        assert!(!gizmo.begin(0, on, lens));
        gizmo.mode = Mode::Rotate;
        assert!(gizmo.dragging().is_none());
    }

    #[test]
    fn an_included_objects_params_lock_holds() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp/ui-tests/gizmo-included");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.json"), "{\"lamp\": {\"x\": 0.3}}").unwrap();
        std::fs::write(
            dir.join("rig.toml"),
            "manfred = 1\n\n[[object]]\nname = \"lamp\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\nat = [\"${params.lamp.x}\", 0.0, 0.0]\n\n[[object]]\nname = \"post\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\nscale = 2.0\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("scene.toml"),
            "manfred = 1\nname = \"h\"\nparams = \"p.json\"\ninclude = [\"rig.toml\"]\n\n[[object]]\nname = \"own\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n",
        )
        .unwrap();
        let loaded = manfred_recipe::load(&dir.join("scene.toml")).unwrap();
        let poses = Poses::in_doc(&loaded.recipe, &loaded.doc);
        let lamp = poses
            .objects
            .iter()
            .find(|pose| pose.name == "lamp")
            .unwrap();
        assert_eq!(
            lamp.lock(Mode::Move),
            Some("at comes from params.lamp.x; the params file stays its truth")
        );
        let post = poses
            .objects
            .iter()
            .find(|pose| pose.name == "post")
            .unwrap();
        assert!(post.uniform);
        let own = poses
            .objects
            .iter()
            .find(|pose| pose.name == "own")
            .unwrap();
        assert!(own.lock(Mode::Move).is_none());
    }

    #[test]
    fn a_gizmo_holds_its_preview_after_release_until_the_recipe_returns() {
        let recipe = manfred_recipe::parse(PLAIN).unwrap();
        let mut gizmo = Gizmo::default();
        gizmo.set_poses(Poses::of(&recipe));
        let lens = Lens::new(&front(), image());
        let centre = lens.project(DVec3::new(0.5, 0.0, 0.0)).unwrap();
        let start = centre + Vec2::new(60.0, 0.0);
        assert!(gizmo.begin(0, start, lens));
        gizmo.to(start + Vec2::new(25.0, 0.0), false);
        let (object, motion) = gizmo.preview().unwrap();
        assert_eq!(object, 0);
        assert!((motion[3][0] - 0.5).abs() < 1e-6);
        let (label, patches) = gizmo.finish().unwrap();
        assert_eq!(label, "move box along x");
        assert_eq!(text(&patches[0]), "object.box.at = [1.0, 0.0, 0.0]");
        assert_eq!(gizmo.preview().unwrap().1, motion);
        assert!((gizmo.shown(0).unwrap().at.x - 1.0).abs() < 1e-9);
        gizmo.set_poses(Poses::of(&recipe));
        assert_eq!(gizmo.preview(), None);
        assert!(gizmo.begin(0, start, lens));
        gizmo.to(start + Vec2::new(25.0, 0.0), false);
        assert!(gizmo.cancel());
        assert!(gizmo.swallowing());
        assert_eq!(gizmo.preview(), None);
        assert!(gizmo.finish().is_none());
        assert!(!gizmo.swallowing());
    }

    const GROUP: &str = "manfred = 1\nname = \"g\"\n\n[[object]]\nname = \"pair\"\nkind = \"box\"\nsize = [0.001, 0.001, 0.001]\nat = [1.0, 0.0, 0.0]\nhidden = true\n\n[[object]]\nname = \"left\"\nkind = \"box\"\nsize = [0.2, 0.2, 0.2]\nparent = \"pair\"\nat = [-0.4, 0.3, 0.0]\n\n[[object]]\nname = \"right\"\nkind = \"box\"\nsize = [0.2, 0.2, 0.2]\nparent = \"pair\"\nat = [0.6, 0.3, 0.0]\n";

    fn group() -> (Recipe, Poses) {
        let recipe = manfred_recipe::parse(GROUP).unwrap();
        let built = manfred_build::build(&recipe, std::path::Path::new(".")).unwrap();
        let solids = crate::viewport::solids(&recipe, &built);
        let poses = Poses::of(&recipe).centred(&solids);
        (recipe, poses)
    }

    #[test]
    fn a_group_s_gizmo_sits_at_the_centre_of_its_bounds_and_a_part_keeps_its_origin() {
        let (_, poses) = group();
        let pair = poses.get(0).unwrap();
        let near = |a: DVec3, b: DVec3| (a - b).length() < 1e-6;
        assert!(
            near(pair.centre(), DVec3::new(1.1, 0.3, 0.0)),
            "{}",
            pair.centre()
        );
        assert!(near(pair.pivot, DVec3::new(0.1, 0.3, 0.0)));
        for (object, at) in [(1, [0.6, 0.3, 0.0]), (2, [1.6, 0.3, 0.0])] {
            let part = poses.get(object).unwrap();
            assert_eq!(part.pivot, DVec3::ZERO);
            assert!(close(part.centre(), DVec3::from_array(at)));
        }
    }

    #[test]
    fn a_group_turns_and_scales_about_its_centre_and_writes_the_at_that_keeps_it() {
        let (_, poses) = group();
        let lens = Lens::new(&front(), image());
        let start = poses.get(0).unwrap().clone();
        let middle = lens.project(start.centre()).unwrap();
        let mut drag = Drag::begin(
            0,
            &start,
            Mode::Rotate,
            Space::World,
            Handle::Ring(2),
            middle + Vec2::new(80.0, 0.0),
            lens,
        );
        drag.to(middle + Vec2::new(0.0, -80.0), false, None);
        assert!(
            (drag.pose.rotate.z.abs() - 90.0).abs() < 1e-6,
            "{}",
            drag.pose.rotate
        );
        assert!(
            close(drag.pose.centre(), start.centre()),
            "{}",
            drag.pose.centre()
        );
        let patches: Vec<String> = drag.patches(None).iter().map(text).collect();
        assert_eq!(patches.len(), 2, "{patches:?}");
        assert!(patches[0].starts_with("object.pair.at = "), "{patches:?}");
        assert!(
            patches[1].starts_with("object.pair.rotate = "),
            "{patches:?}"
        );
        let mut grow = Drag::begin(
            0,
            &start,
            Mode::Scale,
            Space::World,
            Handle::Centre,
            middle,
            lens,
        );
        grow.to(middle + Vec2::new(40.0, -40.0), false, None);
        assert!(grow.pose.scale.x > 1.5, "{}", grow.pose.scale);
        assert!(
            close(grow.pose.centre(), start.centre()),
            "{}",
            grow.pose.centre()
        );
        assert!(!close(grow.pose.at, start.at));
    }

    #[test]
    fn a_group_moves_by_its_at_from_its_centre() {
        let (_, poses) = group();
        let lens = Lens::new(&front(), image());
        let start = poses.get(0).unwrap().clone();
        let middle = lens.project(start.centre()).unwrap();
        let mut drag = Drag::begin(
            0,
            &start,
            Mode::Move,
            Space::World,
            Handle::Axis(0),
            middle + Vec2::new(60.0, 0.0),
            lens,
        );
        drag.to(middle + Vec2::new(110.0, 0.0), false, None);
        let moved = drag.pose.at - start.at;
        assert!(
            moved.x > 0.5 && moved.y.abs() < 1e-9 && moved.z.abs() < 1e-9,
            "{moved}"
        );
        assert!(close(drag.pose.centre() - start.centre(), moved));
        assert!(close(drag.pose.pivot, start.pivot));
    }
}
