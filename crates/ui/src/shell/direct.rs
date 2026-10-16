use glam::DVec3;

use super::*;
use crate::overlay::{self, Guides};
use crate::tape::{Act, View};

const FIT_MARGIN: f64 = 1.15;
const FALLBACK_FOV: f64 = 40.0;
const PATH_WIDTH: f32 = 1.6;
const GUIDE_WIDTH: f32 = 1.2;

pub fn glob(pattern: &str, text: &str) -> bool {
    fn from(pattern: &[u8], text: &[u8]) -> bool {
        match (pattern.first(), text.first()) {
            (None, None) => true,
            (Some(b'*'), _) => {
                from(&pattern[1..], text) || (!text.is_empty() && from(pattern, &text[1..]))
            }
            (Some(b'?'), Some(_)) => from(&pattern[1..], &text[1..]),
            (Some(a), Some(b)) if a == b => from(&pattern[1..], &text[1..]),
            _ => false,
        }
    }
    from(pattern.as_bytes(), text.as_bytes())
}

pub fn fitted(eye: &Eye, toward: DVec3, up: DVec3, centre: DVec3, radius: f64, aspect: f64) -> Eye {
    let fov = if eye.ortho_height.is_none() && eye.fov_y_deg > 0.0 {
        eye.fov_y_deg
    } else {
        FALLBACK_FOV
    };
    let half_y = (fov.to_radians() * 0.5).tan();
    let half = half_y.min(half_y * aspect.max(1e-3)).atan();
    let distance = (radius.max(1e-3) / half.sin()) * FIT_MARGIN;
    Eye {
        position: (centre + toward.normalize_or(DVec3::Z) * distance).to_array(),
        target: centre.to_array(),
        up: up.to_array(),
        fov_y_deg: fov,
        shift: [0.0, 0.0],
        ortho_height: None,
    }
}

pub fn dollied(eye: &Eye, metres: f64) -> Eye {
    let target = DVec3::from_array(eye.target);
    let offset = DVec3::from_array(eye.position) - target;
    let distance = offset.length();
    let Some(along) = offset.try_normalize() else {
        return *eye;
    };
    let nearer = (distance - metres).max(orbit::NEAREST);
    Eye {
        position: (target + along * nearer).to_array(),
        ortho_height: eye
            .ortho_height
            .map(|height| (height * nearer / distance.max(1e-9)).max(orbit::NEAREST)),
        ..*eye
    }
}

pub fn blend(from: &Eye, to: &Eye, amount: f64) -> Eye {
    let mix = |a: [f64; 3], b: [f64; 3]| {
        DVec3::from_array(a)
            .lerp(DVec3::from_array(b), amount)
            .to_array()
    };
    Eye {
        position: mix(from.position, to.position),
        target: mix(from.target, to.target),
        up: DVec3::from_array(mix(from.up, to.up))
            .try_normalize()
            .unwrap_or(DVec3::Y)
            .to_array(),
        fov_y_deg: from.fov_y_deg + (to.fov_y_deg - from.fov_y_deg) * amount,
        ..*to
    }
}

impl Shell {
    pub fn set_guides(&mut self, guides: Guides) -> bool {
        if guides == self.guides {
            return false;
        }
        self.guides = guides;
        true
    }

    pub fn selection(&self) -> Vec<usize> {
        let mut chosen: Vec<usize> = self.object.into_iter().collect();
        for name in &self.also {
            if let Some(object) = self.document.object(name)
                && !chosen.contains(&object)
            {
                chosen.push(object);
            }
        }
        chosen
    }

    pub fn region_shown(&self) -> Option<[f32; 4]> {
        self.region
            .filter(|_| self.through().is_some() || self.chase_view().is_some())
    }

    fn aspect_shown(&self) -> f64 {
        self.viewport
            .map_or(f64::from(self.document.aspect), |image| {
                f64::from(image.width().max(1.0)) / f64::from(image.height().max(1.0))
            })
    }

    pub fn pieces(&self) -> &[String] {
        &self.pieces
    }

    pub fn set_strip(&mut self, on: bool) {
        self.strip = on;
    }

    pub(super) fn margin(&self) -> f32 {
        if self.bare { 0.0 } else { viewport::MARGIN }
    }

    fn bounds(&self, objects: &[usize], pieces: &[String]) -> Option<(DVec3, f64)> {
        let solids = self.solids_shown();
        let every = objects.is_empty() && pieces.is_empty();
        let (min, max) = solids
            .iter()
            .filter(|solid| {
                every
                    || solid.objects.iter().any(|o| objects.contains(o))
                    || pieces.contains(&solid.name)
            })
            .fold(None, |held: Option<(DVec3, DVec3)>, solid| {
                Some(held.map_or((solid.min, solid.max), |(min, max)| {
                    (min.min(solid.min), max.max(solid.max))
                }))
            })?;
        Some(((min + max) * 0.5, ((max - min) * 0.5).length()))
    }

    fn loose(&mut self) -> Eye {
        if let Some((eye, _)) = self.chase_view() {
            self.eye = Some(eye);
        } else if let Some(camera) = self.through() {
            self.eye = Some(Eye::from_camera(camera, f64::from(self.render_aspect())));
        }
        self.clock.chase = false;
        self.through = false;
        self.settle = None;
        self.grab = None;
        *self.eye.get_or_insert_with(default_eye)
    }

    fn resolve(&self, names: &[String]) -> Result<(Vec<usize>, Vec<String>), String> {
        let mut found: Vec<usize> = Vec::new();
        let mut pieces: Vec<String> = Vec::new();
        for name in names {
            let matched: Vec<usize> = self
                .document
                .objects
                .iter()
                .enumerate()
                .filter(|(_, item)| glob(name, &item.name))
                .map(|(index, _)| index)
                .collect();
            if matched.is_empty() {
                let parts: Vec<String> = self
                    .solids
                    .iter()
                    .filter(|solid| glob(name, &solid.name))
                    .map(|solid| solid.name.clone())
                    .collect();
                if parts.is_empty() {
                    return Err(format!("no object or piece {name}"));
                }
                for part in parts {
                    if !pieces.contains(&part) {
                        pieces.push(part);
                    }
                }
                continue;
            }
            for index in matched {
                if !found.contains(&index) {
                    found.push(index);
                }
            }
        }
        Ok((found, pieces))
    }

    pub(super) fn snap(&mut self, view: View) {
        match view {
            View::Top => self.snap_ortho(manfred_recipe::Ortho::Top),
            View::Front => self.snap_ortho(manfred_recipe::Ortho::Front),
            View::Side => self.snap_ortho(manfred_recipe::Ortho::Side),
            View::Back => self.snap_ortho(manfred_recipe::Ortho::Rear),
            View::Orbit => self.unsnap(),
        }
    }

    pub(super) fn fit_selection(&mut self) {
        let chosen = self.selection();
        let eye = self.loose();
        if let Some((centre, radius)) = self
            .bounds(&chosen, &self.pieces)
            .or_else(|| self.bounds(&[], &[]))
        {
            let toward = DVec3::from_array(eye.position) - DVec3::from_array(eye.target);
            let up = DVec3::from_array(eye.up);
            self.eye = Some(fitted(
                &eye,
                toward,
                up,
                centre,
                radius,
                self.aspect_shown(),
            ));
        }
    }

    pub fn direct(&mut self, act: &Act) -> Result<(), String> {
        if !self.live() {
            return Err("the window has no recipe or scene open".into());
        }
        match act {
            Act::View(view) => self.snap(*view),
            Act::Camera(name) => {
                let at = self
                    .document
                    .cameras
                    .iter()
                    .position(|(known, _)| known == name)
                    .ok_or_else(|| format!("no camera {name}"))?;
                self.jump(at);
                self.clock.chase = self
                    .document
                    .views
                    .get(at)
                    .is_some_and(|view| view.rig.is_some())
                    && self.span().is_some();
            }
            Act::Select(names) => {
                if let [name] = names.as_slice()
                    && self.resolve(names).is_err()
                    && let Some(light) = self
                        .document
                        .lights
                        .iter()
                        .position(|(_, known, _)| glob(name, known))
                {
                    self.choose_light(light);
                    return Ok(());
                }
                let (found, pieces) = self.resolve(names)?;
                match found.first() {
                    Some(first) => self.select(*first),
                    None => self.object = None,
                }
                self.also = found
                    .iter()
                    .skip(1)
                    .map(|object| self.document.objects[*object].name.clone())
                    .collect();
                self.pieces = pieces;
                self.lit_shown = false;
                self.reveal = true;
            }
            Act::Orbit(degrees) => {
                let eye = self.loose();
                self.eye = Some(orbit::turn(&eye, [degrees / orbit::DEGREES_PER_POINT, 0.0]));
            }
            Act::Dolly(metres) => {
                let eye = self.loose();
                self.eye = Some(dollied(&eye, *metres));
            }
            Act::Fit {
                objects,
                before,
                now,
            } => {
                let (found, pieces) = self.resolve(objects)?;
                let eye = self.loose();
                let (centre, radius) = self
                    .bounds(&found, &pieces)
                    .ok_or_else(|| format!("{} has nothing built to frame", objects.join(",")))?;
                let toward = DVec3::from_array(eye.position) - DVec3::from_array(eye.target);
                let goal = fitted(
                    &eye,
                    toward,
                    DVec3::from_array(eye.up),
                    centre,
                    radius,
                    self.aspect_shown(),
                );
                let left = 1.0 - before;
                let amount = if left <= 1e-12 {
                    1.0
                } else {
                    ((now - before) / left).clamp(0.0, 1.0)
                };
                self.eye = Some(blend(&eye, &goal, amount));
            }
            Act::Region(region) => self.region = *region,
            Act::Strip(on) => self.strip = *on,
            Act::Stage(_) | Act::Reveal { .. } => {
                return Err("stage and reveal rebuild the recipe through the viewer".into());
            }
        }
        Ok(())
    }

    pub(super) fn overlays_on(&self) -> bool {
        self.wire.on() || self.wire.clay > 0.0
    }

    pub(super) fn overlays(&self, painter: &Painter, eye: &Eye, image: Rect) {
        if !self.overlays_on() || self.view_error.is_some() {
            return;
        }
        let painter = painter.with_clip_rect(image);
        let lens = Lens::new(eye, image);
        let accent = Stroke::new(GUIDE_WIDTH, ACCENT);
        let aspect = self.span().map_or(f64::from(self.render_aspect()), |span| {
            f64::from(span.aspect)
        });
        let looked = if self.chase_view().is_some() {
            self.document
                .views
                .iter()
                .position(|view| view.rig.is_some())
        } else if self.through().is_some() {
            Some(self.camera)
        } else {
            None
        };
        for (index, view) in self.document.views.iter().enumerate() {
            if Some(index) == looked {
                continue;
            }
            let eye = match (&view.rig, self.chase_eye) {
                (Some(_), Some(chase)) => chase,
                _ => Eye::from_camera(view, aspect),
            };
            for line in overlay::frustum(&eye, aspect, overlay::FAR) {
                overlay::polyline(&painter, &lens, &line, accent);
            }
        }
        overlay::polyline(
            &painter,
            &lens,
            &self.guides.path,
            Stroke::new(PATH_WIDTH, ACCENT),
        );
        let lit = self
            .document
            .lights
            .get(self.light)
            .filter(|_| self.lit_shown)
            .map(|(_, name, _)| name.as_str());
        for lamp in &self.guides.lamps {
            let colour = if Some(lamp.name.as_str()) == lit {
                ACCENT
            } else {
                TEXT_SECONDARY
            };
            let stroke = Stroke::new(GUIDE_WIDTH, colour);
            let lines = match lamp.cone {
                Some((toward, cone, range)) => overlay::cone(lamp.position, toward, cone, range),
                None => overlay::marker(lamp.position),
            };
            for line in lines {
                overlay::polyline(&painter, &lens, &line, stroke);
            }
        }
        overlay::dots(
            &painter,
            &lens,
            &self.guides.dots,
            ACCENT.gamma_multiply(0.85),
        );
        if let Some(frame) = self.camera_frame() {
            painter.rect_stroke(frame, CornerRadius::ZERO, accent, StrokeKind::Outside);
        }
        if let Some([x0, y0, x1, y1]) = self.region_shown() {
            let at = |x: f32, y: f32| image.min + Vec2::new(x * image.width(), y * image.height());
            painter.rect_stroke(
                Rect::from_two_pos(at(x0, y0), at(x1, y1)),
                CornerRadius::ZERO,
                Stroke::new(PATH_WIDTH, ACCENT),
                StrokeKind::Middle,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glob_matches_stars_and_a_fit_sees_the_whole_sphere() {
        assert!(glob("hero*", "hero.body"));
        assert!(glob("road", "road"));
        assert!(!glob("road", "roads"));
        let eye = default_eye();
        let fit = fitted(
            &eye,
            DVec3::Z,
            DVec3::Y,
            DVec3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
        );
        let distance = (DVec3::from_array(fit.position) - DVec3::new(1.0, 0.0, 0.0)).length();
        assert!((distance - 2.0 / 20f64.to_radians().sin() * FIT_MARGIN).abs() < 1e-9);
        let near = dollied(&fit, 1.0);
        let closer = (DVec3::from_array(near.position) - DVec3::from_array(near.target)).length();
        assert!((distance - closer - 1.0).abs() < 1e-9);
        let floor = DVec3::from_array(dollied(&fit, 1e9).position) - DVec3::new(1.0, 0.0, 0.0);
        assert!(
            (floor - DVec3::Z * orbit::NEAREST).length() < 1e-12,
            "{floor:?}"
        );
    }
}
