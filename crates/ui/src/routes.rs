use egui::{Painter, Pos2, Stroke, Vec2};
use glam::DVec3;
use manfred_motion::Route;
use manfred_recipe::{Kind, Loaded, Patch, PointEdit, Recipe, route_patch};

use crate::edit::round;
use crate::pick::{Lens, Ray};
use crate::theme::*;

pub const DISC: f32 = 4.5;
pub const HIT: f32 = 8.0;
pub const LINE_HIT: f32 = 6.0;
const SAMPLES: f64 = 512.0;
const LEAST_STEP: f64 = 0.05;
const DECIMALS: usize = 3;
const LEAST_POINTS: usize = 2;

#[derive(Clone, Debug)]
pub struct Line {
    pub name: String,
    pub route: Route,
    pub path: Vec<DVec3>,
}

pub fn lines(recipe: &Recipe) -> Vec<Line> {
    recipe
        .routes
        .iter()
        .filter_map(|spec| {
            let route = Route::new(&spec.points, spec.closed).ok()?;
            let step = (route.length() / SAMPLES).max(LEAST_STEP);
            let path = route
                .sample(step)
                .into_iter()
                .map(|(_, frame)| frame.position)
                .collect();
            Some(Line {
                name: spec.name.clone(),
                route,
                path,
            })
        })
        .collect()
}

pub fn named_by(recipe: &Recipe, object: &str) -> Option<String> {
    let found = recipe.objects.iter().find(|o| o.name == object)?;
    match &found.kind {
        Kind::Generator { params, .. } => params.get("route")?.as_str().map(str::to_string),
        _ => None,
    }
}

fn snapped(point: DVec3) -> [f64; 3] {
    point.to_array().map(|v| round(v, DECIMALS))
}

fn on_plane(point: DVec3, ray: &Ray) -> Option<DVec3> {
    let facing = ray.direction.y;
    if facing.abs() < 1e-9 {
        return None;
    }
    let distance = (point.y - ray.origin.y) / facing;
    (distance > 0.0).then(|| ray.at(distance))
}

fn on_upright(point: DVec3, ray: &Ray) -> Option<DVec3> {
    let b = ray.direction.y;
    let denominator = 1.0 - b * b;
    if denominator < 1e-9 {
        return None;
    }
    let w = point - ray.origin;
    let t = (b * ray.direction.dot(w) - w.y) / denominator;
    Some(point + DVec3::Y * t)
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

#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub name: String,
    pub index: usize,
    pub start: [f64; 3],
    pub at: [f64; 3],
    pub join: u64,
    offset: Vec2,
    lens: Lens,
}

impl Drag {
    pub fn label(&self) -> String {
        format!("move route {} point {}", self.name, self.index)
    }

    pub fn to(&mut self, pointer: Pos2, upright: bool) -> bool {
        let ray = self.lens.ray(pointer + self.offset);
        let from = DVec3::from_array(self.at);
        let moved = if upright {
            on_upright(from, &ray)
        } else {
            on_plane(from, &ray)
        };
        let Some(moved) = moved.filter(|p| p.is_finite()) else {
            return false;
        };
        let next = snapped(moved);
        if next == self.at {
            return false;
        }
        self.at = next;
        true
    }
}

#[derive(Clone, Debug, Default)]
pub struct Routes {
    pub all: bool,
    pub chosen: Option<String>,
    pub point: Option<(String, usize)>,
    lines: Vec<Line>,
    drag: Option<Drag>,
    joins: u64,
}

impl Routes {
    pub fn set(&mut self, recipe: Option<&Recipe>) {
        self.lines = recipe.map(lines).unwrap_or_default();
        let known = |name: &str| self.lines.iter().any(|line| line.name == name);
        if self.chosen.as_deref().is_some_and(|name| !known(name)) {
            self.chosen = None;
        }
        let keep = self.point.as_ref().is_some_and(|(name, index)| {
            self.lines
                .iter()
                .any(|line| &line.name == name && *index < line.route.points().len())
        });
        if !keep {
            self.point = None;
        }
    }

    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    pub fn dragging(&self) -> Option<&Drag> {
        self.drag.as_ref()
    }

    pub fn toggle(&mut self, name: &str) {
        if self.chosen.as_deref() == Some(name) {
            self.chosen = None;
        } else {
            self.chosen = Some(name.to_string());
        }
    }

    pub fn shown(&self, object_route: Option<&str>) -> Vec<&Line> {
        self.lines
            .iter()
            .filter(|line| {
                let name = Some(line.name.as_str());
                self.all
                    || self.chosen.as_deref() == name
                    || self.point.as_ref().map(|(n, _)| n.as_str()) == name
                    || self.drag.as_ref().map(|d| d.name.as_str()) == name
                    || object_route == name
            })
            .collect()
    }

    pub fn handle_at(
        &self,
        lens: &Lens,
        object_route: Option<&str>,
        at: Pos2,
    ) -> Option<(String, usize)> {
        let mut best: Option<(f32, String, usize)> = None;
        for line in self.shown(object_route) {
            for (index, point) in line.route.points().iter().enumerate() {
                let Some(screen) = lens.project(DVec3::from_array(*point)) else {
                    continue;
                };
                let gap = screen.distance(at);
                if gap <= HIT && best.as_ref().is_none_or(|(g, _, _)| gap < *g) {
                    best = Some((gap, line.name.clone(), index));
                }
            }
        }
        best.map(|(_, name, index)| (name, index))
    }

    pub fn line_at(&self, lens: &Lens, object_route: Option<&str>, at: Pos2) -> Option<Insert> {
        let mut best: Option<(f32, String, DVec3)> = None;
        for line in self.shown(object_route) {
            for pair in line.path.windows(2) {
                let (Some(a), Some(b)) = (lens.project(pair[0]), lens.project(pair[1])) else {
                    continue;
                };
                let (gap, t) = segment_gap(at, a, b);
                if gap <= LINE_HIT && best.as_ref().is_none_or(|(g, _, _)| gap < *g) {
                    best = Some((gap, line.name.clone(), pair[0].lerp(pair[1], f64::from(t))));
                }
            }
        }
        let (_, name, near) = best?;
        let line = self.lines.iter().find(|line| line.name == name)?;
        let (distance, segment) = line.route.nearest(near);
        let point = snapped(line.route.at(distance).position);
        Some(Insert {
            name,
            index: segment + 1,
            point,
        })
    }

    pub fn begin(&mut self, lens: Lens, handle: (String, usize), origin: Pos2) -> bool {
        let Some(line) = self.lines.iter().find(|line| line.name == handle.0) else {
            return false;
        };
        let Some(point) = line.route.points().get(handle.1).copied() else {
            return false;
        };
        let Some(screen) = lens.project(DVec3::from_array(point)) else {
            return false;
        };
        self.joins += 1;
        self.point = Some(handle.clone());
        self.drag = Some(Drag {
            name: handle.0,
            index: handle.1,
            start: point,
            at: point,
            join: self.joins,
            offset: screen - origin,
            lens,
        });
        true
    }

    pub fn to(&mut self, pointer: Pos2, upright: bool) -> Option<&Drag> {
        let drag = self.drag.as_mut()?;
        drag.to(pointer, upright).then_some(&*drag)
    }

    pub fn finish(&mut self) -> Option<Drag> {
        self.drag.take()
    }

    pub fn draw(&self, painter: &Painter, lens: &Lens, object_route: Option<&str>) {
        let line_stroke = Stroke::new(1.5_f32, ACCENT.gamma_multiply(0.85));
        let disc_stroke = Stroke::new(STROKE + 0.5, ACCENT);
        for line in self.shown(object_route) {
            let mut run: Vec<Pos2> = Vec::new();
            for point in &line.path {
                match lens.project(*point) {
                    Some(screen) => run.push(screen),
                    None => {
                        if run.len() > 1 {
                            painter.line(std::mem::take(&mut run), line_stroke);
                        }
                        run.clear();
                    }
                }
            }
            if run.len() > 1 {
                painter.line(run, line_stroke);
            }
            for (index, point) in line.route.points().iter().enumerate() {
                let Some(screen) = lens.project(DVec3::from_array(*point)) else {
                    continue;
                };
                let held = self
                    .drag
                    .as_ref()
                    .is_some_and(|d| d.name == line.name && d.index == index)
                    || self
                        .point
                        .as_ref()
                        .is_some_and(|(n, i)| *n == line.name && *i == index);
                let fill = if held { ACCENT } else { BG_DEEP };
                painter.circle(screen, DISC, fill, disc_stroke);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Insert {
    pub name: String,
    pub index: usize,
    pub point: [f64; 3],
}

impl Insert {
    pub fn label(&self) -> String {
        format!("add route {} point {}", self.name, self.index)
    }
}

pub fn moved(l: &Loaded, name: &str, index: usize, point: [f64; 3]) -> Result<Patch, String> {
    route_patch(l, name, PointEdit::Set(index, point)).map_err(|e| e.to_string())
}

pub fn inserted(l: &Loaded, insert: &Insert) -> Result<Patch, String> {
    route_patch(
        l,
        &insert.name,
        PointEdit::Add(Some(insert.index), insert.point),
    )
    .map_err(|e| e.to_string())
}

pub fn removed(l: &Loaded, name: &str, index: usize) -> Result<Patch, String> {
    let count = l.recipe.route(name).map_or(0, |route| route.points.len());
    if count <= LEAST_POINTS {
        return Err(format!(
            "route[{name}] keeps at least {LEAST_POINTS} points; it has {count}"
        ));
    }
    route_patch(l, name, PointEdit::Remove(index)).map_err(|e| e.to_string())
}

pub fn removal_label(name: &str, index: usize) -> String {
    format!("remove route {name} point {index}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{Editor, Gesture, Join};
    use manfred_recipe::{apply, reread, write};

    const STREET: &str = r#"manfred = 1
name = "street"

# the chase runs down main
[[route]]
name = "main" # the hero's line
points = [
  [0.0, 0.0, 0.0], # start
  [0.0, 0.0, 100.0],
  [20.0, 0.0, 200.0], # the bend
]

[[object]]
name = "box"
kind = "box"
size = [1.0, 1.0, 1.0]
"#;

    fn loaded() -> Loaded {
        reread(std::path::Path::new("street.toml"), STREET).unwrap()
    }

    fn round_trip(patch: Patch, expect: &[&str]) {
        let mut l = loaded();
        let inverse = apply(&mut l, &patch).unwrap();
        let text = write(&l);
        for line in expect {
            assert!(text.contains(line), "{line:?} not in\n{text}");
        }
        for kept in [
            "# the chase runs down main",
            "# the hero's line",
            "# the bend",
        ] {
            assert!(text.contains(kept), "{kept:?} lost in\n{text}");
        }
        apply(&mut l, &inverse).unwrap();
        assert_eq!(write(&l), STREET);
    }

    #[test]
    fn route_gestures_patch_the_points_and_undo_to_the_same_bytes() {
        let l = loaded();
        round_trip(
            moved(&l, "main", 1, [1.5, 0.25, 101.0]).unwrap(),
            &["  [1.5, 0.25, 101.0],\n  [20.0, 0.0, 200.0], # the bend"],
        );
        let insert = Insert {
            name: "main".into(),
            index: 1,
            point: [0.0, 0.0, 50.0],
        };
        round_trip(
            inserted(&l, &insert).unwrap(),
            &["# start\n  [0.0, 0.0, 50.0],\n  [0.0, 0.0, 100.0],"],
        );
        round_trip(
            removed(&l, "main", 1).unwrap(),
            &["# start\n  [20.0, 0.0, 200.0], # the bend"],
        );
        let mut two = l.clone();
        apply(&mut two, &removed(&l, "main", 1).unwrap()).unwrap();
        let refused = removed(&two, "main", 0).unwrap_err();
        assert_eq!(refused, "route[main] keeps at least 2 points; it has 2");
    }

    #[test]
    fn a_route_drag_is_one_undo_step_and_esc_puts_the_file_back() {
        let path = crate::scratch("routes-drag");
        std::fs::write(&path, STREET).unwrap();
        let mut editor = Editor::new(&path);
        assert!(editor.fresh());
        let step = |editor: &mut Editor, z: f64, join: Join| {
            let patch = moved(editor.loaded().unwrap(), "main", 1, [0.0, 0.0, z]).unwrap();
            let outcome = editor.gesture(&Gesture {
                label: "move route main point 1".into(),
                patches: vec![patch],
                join: Some(join),
            });
            assert!(!outcome.refused);
        };
        for z in [101.0, 102.0, 103.0] {
            step(&mut editor, z, Join::Into(1));
        }
        let dragged = std::fs::read_to_string(&path).unwrap();
        assert!(dragged.contains("  [0.0, 0.0, 103.0],\n"));
        assert_eq!(editor.steps(), (1, 0));
        editor.undo();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), STREET);
        editor.redo();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), dragged);
        for z in [90.0, 80.0] {
            step(&mut editor, z, Join::Into(2));
        }
        assert_eq!(editor.steps(), (2, 0));
        editor.gesture(&Gesture {
            label: "move route main point 1".into(),
            patches: Vec::new(),
            join: Some(Join::Cancel(2)),
        });
        assert_eq!(editor.steps(), (1, 0));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), dragged);
    }
}
