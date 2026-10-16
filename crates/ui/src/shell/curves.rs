use super::*;
use crate::curves::{Change, CurveEdit, Range, TRACKS, keys, rigged, value};
use manfred_recipe::Rig;

pub(super) const HEADER: f32 = 24.0;
pub(super) const ROW: f32 = 34.0;
pub(super) const CURVES: &str = "\u{f201}";
const LABEL_ROOM: f32 = 96.0;
const READOUT_ROOM: f32 = 238.0;
const DOT: f32 = 4.0;
const HIT: f32 = 7.0;
const INSET: f32 = 5.0;
const STEP_PX: f32 = 2.0;

struct Lane {
    track: &'static str,
    rect: Rect,
    keys: Vec<[f64; 2]>,
    range: Range,
}

impl Lane {
    fn y(&self, v: f64) -> f32 {
        let along = self.range.along(v) as f32;
        let (top, bottom) = (self.rect.top() + INSET, self.rect.bottom() - INSET);
        bottom - along * (bottom - top)
    }
}

impl Shell {
    fn curve_rig(&self) -> Option<(String, Rig)> {
        let loaded = self.editor.as_ref().and_then(Editor::loaded)?;
        let camera = self.curves.camera.as_deref()?;
        rigged(&loaded.recipe)
            .into_iter()
            .find(|(name, _)| *name == camera)
            .map(|(name, rig)| (name.to_string(), rig.clone()))
    }

    pub(super) fn has_curves(&self) -> bool {
        self.span().is_some() && !self.scene && self.curve_rig().is_some()
    }

    pub(super) fn curves_height(&self) -> f32 {
        if !self.curves.open || self.span().is_none() || self.scene {
            return 0.0;
        }
        let Some((_, rig)) = self.curve_rig() else {
            return 0.0;
        };
        let rows = TRACKS
            .iter()
            .filter(|track| keys(&rig, track).is_some())
            .count()
            .max(1);
        HEADER + ROW * rows as f32
    }

    pub(super) fn toggle_curves(&mut self) {
        if self.has_curves() {
            self.curves.open = !self.curves.open;
            self.curves.drag = None;
        }
    }

    pub fn curves_open(&self) -> bool {
        self.curves.open && self.has_curves()
    }

    pub fn curve(&mut self, edit: CurveEdit, join: Option<Join>) -> Result<(), String> {
        let label = edit.label();
        let Some(loaded) = self.editor.as_ref().and_then(Editor::loaded) else {
            return Err("no recipe is open".to_string());
        };
        match crate::curves::apply(loaded, &edit) {
            Ok(applied) => {
                let patches = vec![applied.patch];
                match join {
                    Some(join) => self.edits.join(label, patches, join),
                    None => self.edits.push(label, patches),
                }
                if self.curves.camera.as_deref() == Some(edit.camera.as_str()) {
                    self.curves.selected = applied.key.map(|key| (edit.track.clone(), key));
                }
                Ok(())
            }
            Err(why) => {
                self.refuse_route(&label, why.clone());
                Err(why)
            }
        }
    }

    pub fn curve_step(
        &mut self,
        camera: &str,
        track: &str,
        key: usize,
        at: [f64; 2],
    ) -> Result<(), String> {
        let join = self.curves.join();
        self.curve(
            CurveEdit {
                camera: camera.to_string(),
                track: track.to_string(),
                change: Change::Move { key, at },
            },
            Some(Join::Into(join)),
        )
    }

    pub(super) fn cancel_curve(&mut self) {
        if let Some(drag) = self.curves.drag.take() {
            let label = format!("move {} key {}", drag.track, drag.key);
            self.edits.join(label, Vec::new(), Join::Cancel(drag.join));
        }
    }

    pub(super) fn remove_curve_key(&mut self) {
        if self.curves.drag.is_some() {
            return;
        }
        let (Some((track, key)), Some(camera)) =
            (self.curves.selected.clone(), self.curves.camera.clone())
        else {
            return;
        };
        let _ = self.curve(
            CurveEdit {
                camera,
                track,
                change: Change::Remove { key },
            },
            None,
        );
    }

    fn curve_axis(&self, rect: Rect) -> (f32, f32) {
        self.scrubber
            .map(|track| (track.left(), track.right()))
            .filter(|(left, right)| right - left > 8.0 && *left > rect.left())
            .unwrap_or((
                rect.left() + LABEL_ROOM,
                (rect.right() - READOUT_ROOM).max(rect.left() + LABEL_ROOM + 40.0),
            ))
    }

    pub(super) fn curves_strip(&mut self, ui: &mut Ui) {
        let height = self.curves_height();
        let (Some(span), Some((camera, rig))) = (self.span(), self.curve_rig()) else {
            return;
        };
        if height <= 0.0 {
            return;
        }
        if self.scrubber.is_none() {
            ui.ctx().request_repaint();
        }
        let seconds = span.seconds().max(1e-9);
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), height),
            Sense::click_and_drag(),
        );
        let (left, right) = self.curve_axis(rect);
        let width = (right - left).max(1.0);
        let x = |t: f64| left + width * (t / seconds) as f32;
        let time = |px: f32| f64::from((px - left) / width) * seconds;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, BG_BASE);
        let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), HEADER));
        let mut bar = ui.new_child(
            UiBuilder::new()
                .max_rect(header.shrink2(Vec2::new(6.0, 2.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        bar.spacing_mut().item_spacing.x = 4.0;
        bar.label(code(&bar, format!("{CURVES}  curves"), TEXT_SECONDARY));
        let cameras: Vec<String> = self
            .editor
            .as_ref()
            .and_then(Editor::loaded)
            .map(|l| {
                rigged(&l.recipe)
                    .into_iter()
                    .map(|(name, _)| name.to_string())
                    .collect()
            })
            .unwrap_or_default();
        for name in &cameras {
            let on = *name == camera;
            let (fill, colour) = if on {
                (ACCENT, ACCENT_TEXT)
            } else {
                (BG_CHIP, TEXT_SECONDARY)
            };
            let clicked = bar
                .add(
                    Button::new(code(&bar, format!("{CAMERA}  {name}"), colour))
                        .fill(fill)
                        .stroke(Stroke::new(STROKE, LINE_BORDER))
                        .corner_radius(CornerRadius::same(RADIUS)),
                )
                .clicked();
            if clicked && !on {
                self.curves.camera = Some(name.clone());
                self.curves.selected = None;
                self.curves.drag = None;
            }
        }
        bar.add_space(8.0);
        let mut create = None;
        for track in TRACKS {
            if keys(&rig, track).is_none() && chip(&mut bar, &format!("+ {track}"), true).clicked()
            {
                create = Some(track);
            }
        }
        let lanes: Vec<Lane> = TRACKS
            .iter()
            .filter_map(|track| Some((*track, keys(&rig, track)?.to_vec())))
            .enumerate()
            .map(|(row, (track, keys))| {
                let top = header.bottom() + ROW * row as f32;
                let rect = Rect::from_min_max(
                    Pos2::new(rect.left(), top),
                    Pos2::new(rect.right(), top + ROW),
                );
                let range = match &self.curves.drag {
                    Some(drag) if drag.track == track => drag.range,
                    _ => Range::of(&keys, seconds),
                };
                Lane {
                    track,
                    rect,
                    keys,
                    range,
                }
            })
            .collect();
        let now = self.clock.time;
        let theme = Theme::of(ui.ctx());
        for lane in &lanes {
            painter.line_segment(
                [lane.rect.left_top(), lane.rect.right_top()],
                Stroke::new(STROKE, LINE_SUBTLE),
            );
            painter.text(
                Pos2::new(lane.rect.left() + 8.0, lane.rect.top() + 4.0),
                Align2::LEFT_TOP,
                lane.track,
                theme.font(SIZE_CODE),
                TEXT_SECONDARY,
            );
            painter.text(
                Pos2::new(lane.rect.left() + 8.0, lane.rect.bottom() - 4.0),
                Align2::LEFT_BOTTOM,
                format!("{:.2}", value(&lane.keys, now)),
                theme.font(SIZE_CODE),
                TEXT_CODE,
            );
            painter.line_segment(
                [
                    Pos2::new(left, lane.y(lane.range.at(0.5))),
                    Pos2::new(right, lane.y(lane.range.at(0.5))),
                ],
                Stroke::new(STROKE, LINE_SUBTLE.gamma_multiply(0.6)),
            );
            let mut points = Vec::new();
            let mut px = left;
            while px <= right {
                points.push(Pos2::new(px, lane.y(value(&lane.keys, time(px)))));
                px += STEP_PX;
            }
            painter.line(points, Stroke::new(1.5, ACCENT.gamma_multiply(0.85)));
            for (index, key) in lane.keys.iter().enumerate() {
                let held = self
                    .curves
                    .selected
                    .as_ref()
                    .is_some_and(|(track, at)| track == lane.track && *at == index);
                let fill = if held { ACCENT } else { BG_DEEP };
                painter.circle(
                    Pos2::new(x(key[0]), lane.y(key[1])),
                    DOT,
                    fill,
                    Stroke::new(STROKE + 0.5, ACCENT),
                );
            }
        }
        if lanes.is_empty() {
            painter.text(
                Pos2::new(left, header.bottom() + ROW / 2.0),
                Align2::LEFT_CENTER,
                format!("{camera} has no keyed tracks; + adds one at its static value"),
                theme.font(SIZE_CODE),
                TEXT_MUTED,
            );
        }
        let lanes_top = header.bottom();
        let head = x(now);
        painter.line_segment(
            [Pos2::new(head, lanes_top), Pos2::new(head, rect.bottom())],
            Stroke::new(STROKE, ACCENT),
        );
        if let Some(track) = create {
            let _ = self.curve(
                CurveEdit {
                    camera: camera.clone(),
                    track: track.to_string(),
                    change: Change::Create,
                },
                None,
            );
            return;
        }
        let (origin, pointer, ctrl, shift) = ui.input(|input| {
            (
                input.pointer.press_origin(),
                input.pointer.interact_pos(),
                input.modifiers.ctrl || input.modifiers.command,
                input.modifiers.shift,
            )
        });
        let hit = |at: Pos2| -> Option<(usize, usize, Pos2)> {
            let mut best: Option<(f32, usize, usize, Pos2)> = None;
            for (row, lane) in lanes.iter().enumerate() {
                for (index, key) in lane.keys.iter().enumerate() {
                    let screen = Pos2::new(x(key[0]), lane.y(key[1]));
                    let gap = screen.distance(at);
                    if gap <= HIT && best.is_none_or(|(g, ..)| gap < g) {
                        best = Some((gap, row, index, screen));
                    }
                }
            }
            best.map(|(_, row, index, screen)| (row, index, screen))
        };
        if response.drag_started()
            && !ctrl
            && let Some(origin) = origin
            && let Some((row, index, screen)) = hit(origin)
        {
            let lane = &lanes[row];
            let key = lane.keys[index];
            self.curves.selected = Some((lane.track.to_string(), index));
            self.curves.drag = Some(crate::curves::Drag {
                track: lane.track.to_string(),
                key: index,
                start: key,
                at: key,
                join: self.curves.join(),
                range: lane.range,
                rows: [lane.rect.top() + INSET, lane.rect.bottom() - INSET],
                offset: [screen.x - origin.x, screen.y - origin.y],
            });
        }
        if response.dragged()
            && let Some(mut drag) = self.curves.drag.clone()
            && let Some(pointer) = pointer
        {
            let at = pointer + Vec2::new(drag.offset[0], drag.offset[1]);
            let fps = f64::from(span.fps);
            let t = if shift {
                drag.start[0]
            } else {
                (time(at.x) * fps).round() / fps
            };
            let along = f64::from((drag.rows[1] - at.y) / (drag.rows[1] - drag.rows[0]).max(1.0));
            let next = [
                crate::edit::round(t, 3),
                crate::edit::round(drag.range.at(along), 3),
            ];
            if next != drag.at {
                drag.at = next;
                let (track, key, join) = (drag.track.clone(), drag.key, drag.join);
                self.curves.drag = Some(drag);
                let moved = self.curve(
                    CurveEdit {
                        camera: camera.clone(),
                        track,
                        change: Change::Move { key, at: next },
                    },
                    Some(Join::Into(join)),
                );
                if moved.is_err() {
                    self.cancel_curve();
                }
            }
        }
        if response.drag_stopped() {
            self.curves.drag = None;
        }
        if response.clicked()
            && let Some(at) = pointer
        {
            if ctrl {
                let lane = lanes
                    .iter()
                    .find(|lane| lane.rect.contains(at) && at.x >= left && at.x <= right);
                if let Some(lane) = lane {
                    let fps = f64::from(span.fps);
                    let _ = self.curve(
                        CurveEdit {
                            camera: camera.clone(),
                            track: lane.track.to_string(),
                            change: Change::Add {
                                t: (time(at.x) * fps).round() / fps,
                            },
                        },
                        None,
                    );
                }
            } else {
                self.curves.selected =
                    hit(at).map(|(row, index, _)| (lanes[row].track.to_string(), index));
            }
        }
    }
}
