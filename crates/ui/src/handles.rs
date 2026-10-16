use egui::{Painter, Pos2, Stroke, Vec2};
use glam::{DMat4, DVec3};
use manfred_recipe::{CurveEdit, Kind, Loaded, Ortho, Patch, Recipe, curve_patch, curves_of};

use crate::edit::round;
use crate::pick::Lens;
use crate::routes::{DISC, HIT, LINE_HIT};
use crate::theme::*;

const DECIMALS: usize = 3;
const LEAST_POINTS: usize = 2;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub object: String,
    pub path: String,
    pub points: Vec<[f64; 2]>,
    pub world: Vec<DVec3>,
    view: Ortho,
    depth: f64,
    matrix: DMat4,
}

pub fn shown_in(curve: Option<Ortho>, view: Ortho) -> bool {
    view.shares_plane(curve.unwrap_or(Ortho::Front))
}

pub fn lines(recipe: &Recipe, object: &str, matrix: DMat4, view: Ortho) -> Vec<Line> {
    let Some(found) = recipe.objects.iter().find(|o| o.name == object) else {
        return Vec::new();
    };
    let Kind::Generator { params, .. } = &found.kind else {
        return Vec::new();
    };
    curves_of(params)
        .into_iter()
        .filter(|curve| shown_in(curve.view, view))
        .map(|curve| {
            let world = curve
                .points
                .iter()
                .map(|p| matrix.transform_point3(DVec3::from_array(view.place(*p, curve.depth))))
                .collect();
            Line {
                object: object.to_string(),
                path: curve.path,
                points: curve.points,
                world,
                view,
                depth: curve.depth,
                matrix,
            }
        })
        .collect()
}

fn segment_gap(at: Pos2, a: Pos2, b: Pos2) -> (f32, f32) {
    let ab = b - a;
    let length = ab.length_sq();
    let t = if length > 0.0 {
        ((at - a).dot(ab) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((a + ab * t).distance(at), t)
}

fn flattened(line: &Line, world: DVec3) -> [f64; 2] {
    let local = line.matrix.inverse().transform_point3(world);
    line.view
        .flatten(local.to_array())
        .map(|v| round(v, DECIMALS))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub object: String,
    pub path: String,
    pub index: usize,
    pub start: [f64; 2],
    pub at: [f64; 2],
    pub join: u64,
    offset: Vec2,
    lens: Lens,
    line: Line,
}

impl Drag {
    pub fn label(&self) -> String {
        format!("move {} {} point {}", self.object, self.path, self.index)
    }

    fn to(&mut self, pointer: Pos2) -> bool {
        let ray = self.lens.ray(pointer + self.offset);
        let held = self.line.matrix.transform_point3(DVec3::from_array(
            self.line.view.place(self.at, self.line.depth),
        ));
        let facing = ray.direction.length_squared();
        if facing < 1e-12 {
            return false;
        }
        let hit = ray.at((held - ray.origin).dot(ray.direction) / facing);
        if !hit.is_finite() {
            return false;
        }
        let next = flattened(&self.line, hit);
        if next == self.at {
            return false;
        }
        self.at = next;
        true
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Insert {
    pub object: String,
    pub path: String,
    pub index: usize,
    pub point: [f64; 2],
}

impl Insert {
    pub fn label(&self) -> String {
        format!("add {} {} point {}", self.object, self.path, self.index)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Curves {
    pub point: Option<(String, String, usize)>,
    drag: Option<Drag>,
    joins: u64,
}

impl Curves {
    pub fn dragging(&self) -> Option<&Drag> {
        self.drag.as_ref()
    }

    pub fn keep(&mut self, lines: &[Line]) {
        let known = self.point.as_ref().is_some_and(|(object, path, index)| {
            lines
                .iter()
                .any(|l| &l.object == object && &l.path == path && *index < l.points.len())
        });
        if !known {
            self.point = None;
        }
    }

    pub fn handle_at(lines: &[Line], lens: &Lens, at: Pos2) -> Option<(usize, usize)> {
        let mut best: Option<(f32, usize, usize)> = None;
        for (l, line) in lines.iter().enumerate() {
            for (index, point) in line.world.iter().enumerate() {
                let Some(screen) = lens.project(*point) else {
                    continue;
                };
                let gap = screen.distance(at);
                if gap <= HIT && best.is_none_or(|(g, _, _)| gap < g) {
                    best = Some((gap, l, index));
                }
            }
        }
        best.map(|(_, l, index)| (l, index))
    }

    pub fn line_at(lines: &[Line], lens: &Lens, at: Pos2) -> Option<Insert> {
        let mut best: Option<(f32, usize, usize, DVec3)> = None;
        for (l, line) in lines.iter().enumerate() {
            for (segment, pair) in line.world.windows(2).enumerate() {
                let (Some(a), Some(b)) = (lens.project(pair[0]), lens.project(pair[1])) else {
                    continue;
                };
                let (gap, t) = segment_gap(at, a, b);
                if gap <= LINE_HIT && best.is_none_or(|(g, ..)| gap < g) {
                    best = Some((gap, l, segment, pair[0].lerp(pair[1], f64::from(t))));
                }
            }
        }
        let (_, l, segment, near) = best?;
        let line = &lines[l];
        Some(Insert {
            object: line.object.clone(),
            path: line.path.clone(),
            index: segment + 1,
            point: flattened(line, near),
        })
    }

    pub fn begin(&mut self, lens: Lens, line: &Line, index: usize, origin: Pos2) -> bool {
        let (Some(point), Some(world)) = (line.points.get(index), line.world.get(index)) else {
            return false;
        };
        let Some(screen) = lens.project(*world) else {
            return false;
        };
        self.joins += 1;
        self.point = Some((line.object.clone(), line.path.clone(), index));
        self.drag = Some(Drag {
            object: line.object.clone(),
            path: line.path.clone(),
            index,
            start: *point,
            at: *point,
            join: self.joins,
            offset: screen - origin,
            lens,
            line: line.clone(),
        });
        true
    }

    pub fn to(&mut self, pointer: Pos2) -> Option<&Drag> {
        let drag = self.drag.as_mut()?;
        drag.to(pointer).then_some(&*drag)
    }

    pub fn finish(&mut self) -> Option<Drag> {
        self.drag.take()
    }

    pub fn draw(&self, painter: &Painter, lens: &Lens, lines: &[Line]) {
        let line_stroke = Stroke::new(1.5_f32, INFO.gamma_multiply(0.9));
        let disc_stroke = Stroke::new(STROKE + 0.5, INFO);
        for line in lines {
            let dragged = self
                .drag
                .as_ref()
                .filter(|d| d.object == line.object && d.path == line.path);
            let world: Vec<DVec3> = line
                .points
                .iter()
                .enumerate()
                .map(|(index, p)| match dragged.filter(|d| d.index == index) {
                    Some(d) => line
                        .matrix
                        .transform_point3(DVec3::from_array(line.view.place(d.at, line.depth))),
                    None => line
                        .matrix
                        .transform_point3(DVec3::from_array(line.view.place(*p, line.depth))),
                })
                .collect();
            let run: Vec<Pos2> = world.iter().filter_map(|p| lens.project(*p)).collect();
            if run.len() > 1 {
                painter.line(run, line_stroke);
            }
            for (index, point) in world.iter().enumerate() {
                let Some(screen) = lens.project(*point) else {
                    continue;
                };
                let held = dragged.is_some_and(|d| d.index == index)
                    || self.point.as_ref().is_some_and(|(o, p, i)| {
                        *o == line.object && *p == line.path && *i == index
                    });
                let fill = if held { INFO } else { BG_DEEP };
                painter.circle(screen, DISC, fill, disc_stroke);
            }
        }
    }
}

pub fn moved(l: &Loaded, drag: &Drag) -> Result<Patch, String> {
    curve_patch(
        l,
        &drag.object,
        &drag.path,
        CurveEdit::Set(drag.index, drag.at),
    )
    .map_err(|e| e.to_string())
}

pub fn inserted(l: &Loaded, insert: &Insert) -> Result<Patch, String> {
    curve_patch(
        l,
        &insert.object,
        &insert.path,
        CurveEdit::Add(Some(insert.index), insert.point),
    )
    .map_err(|e| e.to_string())
}

pub fn removed(l: &Loaded, object: &str, path: &str, index: usize) -> Result<Patch, String> {
    let count = manfred_recipe::curve_points(l, object, path).map_or(0, |p| p.len());
    if count <= LEAST_POINTS {
        return Err(format!(
            "{object} {path} keeps at least {LEAST_POINTS} points; it has {count}"
        ));
    }
    curve_patch(l, object, path, CurveEdit::Remove(index)).map_err(|e| e.to_string())
}

pub fn removal_label(object: &str, path: &str, index: usize) -> String {
    format!("remove {object} {path} point {index}")
}
