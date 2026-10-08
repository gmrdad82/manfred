use egui::{
    Align2, Color32, ColorImage, CornerRadius, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2,
};
use glam::{DMat4, DVec3};
use manfred_build::Built;
use manfred_recipe::{Recipe, Shadow};
use manfred_scene::Eye;

use crate::orbit::Basis;
use crate::pick::Lens;
use crate::theme::*;

pub const ASPECT: f32 = 16.0 / 10.0;
pub const CHECKER: [usize; 2] = [320, 200];
pub const MARGIN: f32 = 24.0;
const PASSEPARTOUT: f32 = 0.6;
const SQUARE: usize = 20;
const GRID_REACH: f64 = 3.0;
const GRID_CELLS: i64 = 24;
const GRID_NEAR: f64 = 0.02;
const GRID_ALPHA: f32 = 0.3;
const AXIS_ALPHA: f32 = 0.75;
const GRID_SLICES: usize = 16;
const GRID_SLACK: f64 = 1e-4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    Cell,
    X,
    Z,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub from: Pos2,
    pub to: Pos2,
    pub rule: Rule,
    pub fade: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Solid {
    pub min: DVec3,
    pub max: DVec3,
    pub objects: Vec<usize>,
}

impl Solid {
    fn corners(&self) -> [DVec3; 8] {
        std::array::from_fn(|k| {
            DVec3::new(
                if k & 1 == 0 { self.min.x } else { self.max.x },
                if k & 2 == 0 { self.min.y } else { self.max.y },
                if k & 4 == 0 { self.min.z } else { self.max.z },
            )
        })
    }

    pub fn moved(&self, motion: &DMat4) -> Solid {
        let corners = self.corners().map(|corner| motion.transform_point3(corner));
        Solid {
            min: corners.iter().fold(DVec3::INFINITY, |a, b| a.min(*b)),
            max: corners.iter().fold(DVec3::NEG_INFINITY, |a, b| a.max(*b)),
            objects: self.objects.clone(),
        }
    }

    fn enters(&self, origin: DVec3, direction: DVec3) -> Option<f64> {
        let (mut near, mut far) = (f64::NEG_INFINITY, f64::INFINITY);
        for k in 0..3 {
            if direction[k].abs() < 1e-15 {
                if origin[k] < self.min[k] || origin[k] > self.max[k] {
                    return None;
                }
                continue;
            }
            let a = (self.min[k] - origin[k]) / direction[k];
            let b = (self.max[k] - origin[k]) / direction[k];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
        }
        (near <= far && far > 0.0).then_some(near.max(0.0))
    }
}

pub fn solids(recipe: &Recipe, built: &Built) -> Vec<Solid> {
    let parent_of = |object: usize| {
        recipe.objects[object]
            .parent
            .as_deref()
            .and_then(|name| recipe.objects.iter().rposition(|o| o.name == name))
    };
    built
        .parts
        .iter()
        .filter(|part| !part.hidden && part.shadow != Shadow::Only)
        .filter(|part| !part.node.mesh.positions.is_empty())
        .map(|part| {
            let (min, max) = part.node.mesh.positions.iter().fold(
                (DVec3::INFINITY, DVec3::NEG_INFINITY),
                |(min, max), p| {
                    let p = part
                        .node
                        .transform
                        .transform_point3(DVec3::from_array(p.map(f64::from)));
                    (min.min(p), max.max(p))
                },
            );
            let mut objects = vec![part.object];
            while let Some(parent) = parent_of(*objects.last().unwrap()) {
                if objects.contains(&parent) {
                    break;
                }
                objects.push(parent);
            }
            Solid { min, max, objects }
        })
        .collect()
}

impl Line {
    pub fn colour(&self) -> Color32 {
        match self.rule {
            Rule::Cell => TEXT_MUTED.gamma_multiply(GRID_ALPHA * self.fade),
            Rule::X => DANGER.gamma_multiply(AXIS_ALPHA * self.fade),
            Rule::Z => INFO.gamma_multiply(AXIS_ALPHA * self.fade),
        }
    }
}

pub fn checker() -> ColorImage {
    let [width, height] = CHECKER;
    let pixels = (0..width * height)
        .map(|index| {
            let (x, y) = (index % width / SQUARE, index / width / SQUARE);
            if (x + y) % 2 == 0 {
                BG_DEEP
            } else {
                BG_SELECTED
            }
        })
        .collect();
    ColorImage::new(CHECKER, pixels)
}

pub fn fit(area: Rect, aspect: f32) -> Rect {
    if area.width() <= 0.0 || area.height() <= 0.0 {
        return Rect::from_center_size(area.center(), Vec2::ZERO);
    }
    let size = if area.width() / area.height() > aspect {
        Vec2::new(area.height() * aspect, area.height())
    } else {
        Vec2::new(area.width(), area.width() / aspect)
    };
    let size = size.round();
    let min = (area.min + (area.size() - size) / 2.0).round();
    Rect::from_min_size(min, size)
}

pub fn cover(area: Rect, aspect: f32) -> Rect {
    if area.width() <= 0.0 || area.height() <= 0.0 {
        return Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    }
    let wide = area.width() / area.height();
    let (across, down) = if wide > aspect {
        (1.0, aspect / wide)
    } else {
        (wide / aspect, 1.0)
    };
    Rect::from_center_size(Pos2::new(0.5, 0.5), Vec2::new(across, down))
}

pub fn widen(eye: &Eye, frame: Rect, image: Rect) -> Eye {
    if frame.width() <= 0.0 || frame.height() <= 0.0 {
        return *eye;
    }
    let k = [
        f64::from(image.width() / frame.width()),
        f64::from(image.height() / frame.height()),
    ];
    let offset = frame.center() - image.center();
    let centre = [
        f64::from(offset.x / (image.width() * 0.5).max(1e-6)),
        -f64::from(offset.y / (image.height() * 0.5).max(1e-6)),
    ];
    let half = (eye.fov_y_deg.to_radians() * 0.5).tan() * k[1];
    Eye {
        fov_y_deg: (half.atan() * 2.0).to_degrees(),
        ortho_height: eye.ortho_height.map(|height| height * k[1]),
        shift: [
            eye.shift[0] / k[0] - centre[0],
            eye.shift[1] / k[1] - centre[1],
        ],
        ..*eye
    }
}

pub fn passepartout(painter: &Painter, image: Rect, frame: Rect) {
    let shade = BG_DEEP.gamma_multiply(PASSEPARTOUT);
    for band in [
        Rect::from_min_max(image.min, Pos2::new(image.right(), frame.top())),
        Rect::from_min_max(Pos2::new(image.left(), frame.bottom()), image.max),
        Rect::from_min_max(
            Pos2::new(image.left(), frame.top()),
            Pos2::new(frame.left(), frame.bottom()),
        ),
        Rect::from_min_max(
            Pos2::new(frame.right(), frame.top()),
            Pos2::new(image.right(), frame.bottom()),
        ),
    ] {
        if band.width() > 0.0 && band.height() > 0.0 {
            painter.rect_filled(band, CornerRadius::ZERO, shade);
        }
    }
    painter.rect_stroke(
        frame,
        CornerRadius::ZERO,
        Stroke::new(STROKE, LINE_BORDER),
        StrokeKind::Outside,
    );
}

pub fn frame(painter: &Painter, image: Rect) {
    painter.rect_stroke(
        image.expand(1.0),
        CornerRadius::ZERO,
        Stroke::new(STROKE, LINE_BORDER),
        StrokeKind::Inside,
    );
}

pub fn gizmo(painter: &Painter, image: Rect) {
    let centre = image.center().round();
    let reach = (image.height() * 0.16).clamp(24.0, 72.0).round();
    let axes: [(&str, Vec2, Color32); 3] = [
        ("x", Vec2::new(0.866, 0.5), DANGER),
        ("y", Vec2::new(0.0, -1.0), OK),
        ("z", Vec2::new(-0.866, 0.5), INFO),
    ];
    painter.circle_stroke(centre, reach + 14.0, Stroke::new(1.5_f32, ACCENT));
    for (name, direction, colour) in axes {
        let end: Pos2 = centre + direction * reach;
        painter.line_segment([centre, end], Stroke::new(2.0_f32, colour));
        painter.circle_filled(end, 3.5, colour);
        painter.text(
            centre + direction * (reach + 26.0),
            Align2::CENTER_CENTER,
            name,
            Theme::of(painter.ctx()).font(SIZE_CODE),
            colour,
        );
    }
    painter.circle_filled(centre, 3.0, ACCENT);
}

pub fn axes(painter: &Painter, image: Rect, basis: &Basis) {
    let reach = (image.height() * 0.05).clamp(14.0, 32.0).round();
    let centre = Pos2::new(image.left() + reach + 22.0, image.bottom() - reach - 22.0).round();
    let world: [(&str, [f64; 3], Color32); 3] = [
        ("x", [1.0, 0.0, 0.0], DANGER),
        ("y", [0.0, 1.0, 0.0], OK),
        ("z", [0.0, 0.0, 1.0], INFO),
    ];
    let mut order: Vec<(f64, &str, Vec2, Color32)> = world
        .into_iter()
        .map(|(name, axis, colour)| {
            let axis = glam::DVec3::from_array(axis);
            let flat = Vec2::new(axis.dot(basis.right) as f32, -(axis.dot(basis.up) as f32));
            (axis.dot(basis.forward), name, flat, colour)
        })
        .collect();
    order.sort_by(|a, b| b.0.total_cmp(&a.0));
    painter.circle_filled(centre, reach + 12.0, BG_DEEP.gamma_multiply(0.8));
    for (_, name, flat, colour) in order {
        let end = centre + flat * reach;
        painter.line_segment([centre, end], Stroke::new(1.5_f32, colour));
        painter.circle_filled(end, 2.5, colour);
        painter.text(
            centre + flat * (reach + 9.0),
            Align2::CENTER_CENTER,
            name,
            Theme::of(painter.ctx()).font(SIZE_CODE),
            colour,
        );
    }
    painter.circle_filled(centre, 2.0, ACCENT);
}

fn span(eye: &Eye) -> f64 {
    eye.ortho_height
        .unwrap_or_else(|| {
            (DVec3::from_array(eye.position) - DVec3::from_array(eye.target)).length()
        })
        .max(1e-6)
}

pub fn spacing(eye: &Eye) -> f64 {
    10f64.powf((span(eye) / 2.0).log10().floor())
}

fn clip(lens: &Lens, a: DVec3, b: DVec3, near: f64) -> Option<(DVec3, DVec3)> {
    let (da, db) = (lens.depth(a), lens.depth(b));
    match (da >= near, db >= near) {
        (true, true) => Some((a, b)),
        (false, false) => None,
        (true, false) => Some((a, a + (b - a) * ((da - near) / (da - db)))),
        (false, true) => Some((b + (a - b) * ((db - near) / (db - da)), b)),
    }
}

fn hidden(eye: &Eye, solids: &[&Solid], point: DVec3) -> bool {
    let (origin, direction) = match eye.ortho_height {
        Some(height) => {
            let forward = (DVec3::from_array(eye.target) - DVec3::from_array(eye.position))
                .try_normalize()
                .unwrap_or(DVec3::NEG_Z);
            let reach = forward * (height + span(eye)) * 1e3;
            (point - reach, reach)
        }
        None => {
            let origin = DVec3::from_array(eye.position);
            (origin, point - origin)
        }
    };
    solids.iter().any(|solid| {
        solid
            .enters(origin, direction)
            .is_some_and(|at| at < 1.0 - GRID_SLACK)
    })
}

fn rect(lens: &Lens, solid: &Solid) -> Rect {
    let mut points = Vec::with_capacity(8);
    for corner in solid.corners() {
        match lens.project(corner) {
            Some(point) => points.push(point),
            None => return Rect::EVERYTHING,
        }
    }
    Rect::from_points(&points)
}

fn seen(
    eye: &Eye,
    lens: &Lens,
    solids: &[(Rect, &Solid)],
    [a, b]: [DVec3; 2],
    rule: Rule,
    fade: f32,
    lines: &mut Vec<Line>,
) {
    let (Some(from), Some(to)) = (lens.project(a), lens.project(b)) else {
        return;
    };
    let reach = Rect::from_two_pos(from, to);
    let near: Vec<&Solid> = solids
        .iter()
        .filter(|(rect, _)| rect.intersects(reach))
        .map(|(_, solid)| *solid)
        .collect();
    if near.is_empty() {
        lines.push(Line {
            from,
            to,
            rule,
            fade,
        });
        return;
    }
    let mut run: Option<DVec3> = None;
    for slice in 0..GRID_SLICES {
        let start = a.lerp(b, slice as f64 / GRID_SLICES as f64);
        let end = a.lerp(b, (slice + 1) as f64 / GRID_SLICES as f64);
        let open = !hidden(eye, &near, (start + end) * 0.5);
        match (open, run) {
            (true, None) => run = Some(start),
            (false, Some(first)) => {
                if let (Some(from), Some(to)) = (lens.project(first), lens.project(start)) {
                    lines.push(Line {
                        from,
                        to,
                        rule,
                        fade,
                    });
                }
                run = None;
            }
            _ => {}
        }
    }
    if let Some(first) = run
        && let (Some(from), Some(to)) = (lens.project(first), lens.project(b))
    {
        lines.push(Line {
            from,
            to,
            rule,
            fade,
        });
    }
}

pub fn grid_lines(eye: &Eye, image: Rect, solids: &[Solid]) -> Vec<Line> {
    let lens = Lens::new(eye, image);
    let span = span(eye);
    let step = spacing(eye);
    let reach = span * GRID_REACH;
    let target = DVec3::from_array(eye.target);
    let centre = [
        (target.x / step).round() as i64,
        (target.z / step).round() as i64,
    ];
    let cells = ((reach / step).ceil() as i64).min(GRID_CELLS);
    let near = eye
        .ortho_height
        .map_or(span * GRID_NEAR, |_| f64::NEG_INFINITY);
    let solids: Vec<(Rect, &Solid)> = solids
        .iter()
        .map(|solid| (rect(&lens, solid), solid))
        .collect();
    let mut lines = Vec::new();
    for along_x in [true, false] {
        let (own, other) = if along_x { (0, 1) } else { (1, 0) };
        for k in -cells..=cells {
            let fixed = centre[other] + k;
            let rule = match (fixed, along_x) {
                (0, true) => Rule::X,
                (0, false) => Rule::Z,
                _ => Rule::Cell,
            };
            for piece in -cells..cells {
                let run = [centre[own] + piece, centre[own] + piece + 1];
                let [a, b] = run.map(|at| {
                    let (at, fixed) = (at as f64 * step, fixed as f64 * step);
                    if along_x {
                        DVec3::new(at, 0.0, fixed)
                    } else {
                        DVec3::new(fixed, 0.0, at)
                    }
                });
                let middle = (a + b) * 0.5;
                let away = ((middle.x - target.x).powi(2) + (middle.z - target.z).powi(2)).sqrt();
                let fade = (1.0 - away / reach).max(0.0).powi(2) as f32;
                if fade <= 0.0 {
                    continue;
                }
                if let Some((a, b)) = clip(&lens, a, b, near) {
                    seen(eye, &lens, &solids, [a, b], rule, fade, &mut lines);
                }
            }
        }
    }
    lines.sort_by_key(|line| line.rule != Rule::Cell);
    lines
}

pub fn grid(painter: &Painter, eye: &Eye, image: Rect, solids: &[Solid]) {
    let clipped = painter.with_clip_rect(image);
    for line in grid_lines(eye, image, solids) {
        clipped.line_segment([line.from, line.to], Stroke::new(1.0_f32, line.colour()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn area(width: f32, height: f32) -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(width, height))
    }

    #[test]
    fn a_wide_area_letterboxes_left_and_right() {
        let image = fit(area(1000.0, 400.0), ASPECT);
        assert_eq!(image.size(), Vec2::new(640.0, 400.0));
        assert_eq!(image.min, Pos2::new(190.0, 20.0));
    }

    #[test]
    fn a_tall_area_letterboxes_top_and_bottom() {
        let image = fit(area(480.0, 700.0), ASPECT);
        assert_eq!(image.size(), Vec2::new(480.0, 300.0));
        assert_eq!(image.min, Pos2::new(10.0, 220.0));
    }

    #[test]
    fn an_exact_area_fills() {
        let image = fit(area(320.0, 200.0), ASPECT);
        assert_eq!(image, area(320.0, 200.0));
    }

    #[test]
    fn the_checker_alternates_in_the_two_tokens() {
        let image = checker();
        assert_eq!(image.size, CHECKER);
        assert_eq!(image.pixels[0], BG_DEEP);
        assert_eq!(image.pixels[SQUARE], BG_SELECTED);
        assert_eq!(image.pixels[CHECKER[0] * SQUARE], BG_SELECTED);
        assert_eq!(CHECKER[0] as f32 / CHECKER[1] as f32, ASPECT);
    }

    fn eye(position: [f64; 3], target: [f64; 3]) -> Eye {
        Eye {
            position,
            target,
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 35.0,
            shift: [0.0, 0.0],
            ortho_height: None,
        }
    }

    fn floor(lens: &Lens, at: Pos2) -> DVec3 {
        let ray = lens.ray(at);
        ray.at(-ray.origin.y / ray.direction.y)
    }

    fn start() -> Eye {
        eye([-0.2263, 0.2824, 0.5082], [0.0, 0.0576, 0.0])
    }

    #[test]
    fn the_grid_lies_on_the_floor_with_the_x_and_z_axes_along_it() {
        let (eye, image) = (start(), area(800.0, 500.0));
        let lens = Lens::new(&eye, image);
        let lines = grid_lines(&eye, image, &[]);
        let step = spacing(&eye);
        assert!((step - 0.1).abs() < 1e-12, "{step}");
        for rule in [Rule::Cell, Rule::X, Rule::Z] {
            assert!(lines.iter().any(|line| line.rule == rule), "{rule:?}");
        }
        for line in &lines {
            for end in [line.from, line.to] {
                let hit = floor(&lens, end);
                let on = |value: f64| (value / step - (value / step).round()).abs() < 0.02;
                match line.rule {
                    Rule::X => assert!(hit.z.abs() < 2e-3, "{hit}"),
                    Rule::Z => assert!(hit.x.abs() < 2e-3, "{hit}"),
                    Rule::Cell => assert!(on(hit.x) || on(hit.z), "{hit}"),
                }
            }
        }
        let first_axis = lines
            .iter()
            .position(|line| line.rule != Rule::Cell)
            .unwrap();
        assert!(
            lines[first_axis..]
                .iter()
                .all(|line| line.rule != Rule::Cell)
        );
    }

    #[test]
    fn the_grid_fades_with_distance_from_the_target() {
        let (eye, image) = (start(), area(800.0, 500.0));
        let lens = Lens::new(&eye, image);
        let lines = grid_lines(&eye, image, &[]);
        let target = DVec3::from_array(eye.target);
        let away = |line: &Line| {
            let middle = floor(&lens, line.from.lerp(line.to, 0.5));
            ((middle.x - target.x).powi(2) + (middle.z - target.z).powi(2)).sqrt()
        };
        let mut by_distance: Vec<(f64, f32)> =
            lines.iter().map(|line| (away(line), line.fade)).collect();
        by_distance.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert!(by_distance[0].1 > 0.9, "{:?}", by_distance[0]);
        assert!(
            by_distance.last().unwrap().1 < 0.2,
            "{:?}",
            by_distance.last()
        );
        assert!(lines.iter().all(|line| line.fade > 0.0 && line.fade <= 1.0));
        let near = lines.iter().find(|line| line.rule == Rule::X).unwrap();
        assert!(near.colour().a() > 0 && near.colour().a() < 255);
    }

    #[test]
    fn the_spacing_follows_the_view_by_powers_of_ten() {
        let at = |distance: f64| spacing(&eye([0.0, 0.0, distance], [0.0; 3]));
        assert!((at(0.6) - 0.1).abs() < 1e-12);
        assert!((at(6.0) - 1.0).abs() < 1e-12);
        assert!((at(0.05) - 0.01).abs() < 1e-12);
        let flat = Eye {
            ortho_height: Some(0.4),
            ..eye([0.0, 1.0, 0.0], [0.0; 3])
        };
        assert!((spacing(&flat) - 0.1).abs() < 1e-12);
    }

    #[test]
    fn lines_running_behind_the_eye_are_cut_at_the_near_plane() {
        let low = eye([0.0, 0.05, 0.0], [0.0, 0.05, -1.0]);
        let image = area(800.0, 500.0);
        let lines = grid_lines(&low, image, &[]);
        assert!(!lines.is_empty());
        for line in &lines {
            for end in [line.from, line.to] {
                assert!(end.x.is_finite() && end.y.is_finite());
                assert!(end.y >= image.center().y, "{end:?}");
                assert!(end.x.abs() < 1e6 && end.y.abs() < 1e6, "{end:?}");
            }
        }
        assert!(lines.iter().any(|line| line.rule == Rule::Z));
    }

    #[test]
    fn a_solid_hides_the_grid_behind_it_and_not_in_front_of_it() {
        let eye = eye([0.0, 0.3, 1.0], [0.0, 0.1, 0.0]);
        let image = area(800.0, 500.0);
        let lens = Lens::new(&eye, image);
        let solid = Solid {
            min: DVec3::new(-0.1, 0.0, -0.05),
            max: DVec3::new(0.1, 0.2, 0.05),
            objects: vec![3, 0],
        };
        let along_z = |solids: &[Solid]| -> Vec<f64> {
            grid_lines(&eye, image, solids)
                .iter()
                .filter(|line| line.rule == Rule::Z)
                .flat_map(|line| (0..=8).map(|k| line.from.lerp(line.to, k as f32 / 8.0)))
                .map(|at| floor(&lens, at).z)
                .collect()
        };
        let behind = |z: &f64| (-0.6..-0.1).contains(z);
        let open = along_z(&[]);
        assert!(open.iter().any(behind));
        let shut = along_z(std::slice::from_ref(&solid));
        assert!(!shut.iter().any(behind), "{shut:?}");
        assert!(shut.iter().any(|z| *z > 0.1));
        let away = solid.moved(&DMat4::from_translation(DVec3::new(1.0, 0.0, 0.0)));
        assert_eq!(away.objects, solid.objects);
        assert!((away.min.x - 0.9).abs() < 1e-12);
        assert!(along_z(&[away]).iter().any(behind));
    }

    #[test]
    fn the_start_mark_s_parts_are_solids_that_name_their_parents() {
        let state = Path::new(env!("CARGO_MANIFEST_DIR")).join("tmp/viewport-start");
        let path = crate::start_recipe(&state).unwrap();
        let loaded = manfred_recipe::load(&path).unwrap();
        let recipe = &loaded.recipe;
        let built = manfred_build::build(recipe, path.parent().unwrap()).unwrap();
        let found = solids(recipe, &built);
        let index = |name: &str| recipe.objects.iter().position(|o| o.name == name).unwrap();
        let badge = found
            .iter()
            .find(|solid| solid.objects[0] == index("badge"))
            .unwrap();
        assert_eq!(
            badge.objects,
            vec![index("badge"), index(crate::start::MARK)]
        );
        assert!(badge.max.y > 0.12 && badge.min.y.abs() < 2e-3, "{badge:?}");
        assert!(
            found
                .iter()
                .all(|solid| solid.objects[0] != index(crate::start::MARK))
        );
        let ground = found
            .iter()
            .find(|solid| solid.objects[0] == index("ground"))
            .unwrap();
        let eye = eye([-0.2263, 0.2824, 0.5082], [0.0, 0.0576, 0.0]);
        let image = area(800.0, 500.0);
        let all = grid_lines(&eye, image, &[]).len();
        let with_ground = grid_lines(&eye, image, std::slice::from_ref(ground)).len();
        assert_eq!(all, with_ground);
    }
}
