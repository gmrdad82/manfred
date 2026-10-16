use super::*;

pub(super) const HEIGHT: f32 = 30.0;
const READOUT: f32 = 230.0;
const KNOB: f32 = 5.0;
const PREVIOUS: &str = "\u{f048}";
const NEXT: &str = "\u{f051}";
const LOOP: &str = "\u{f01e}";

fn toggle(ui: &mut Ui, text: &str, on: bool) -> Response {
    let (fill, colour) = if on {
        (ACCENT, ACCENT_TEXT)
    } else {
        (BG_CHIP, TEXT_SECONDARY)
    };
    ui.add(
        Button::new(code(ui, text, colour))
            .fill(fill)
            .stroke(Stroke::new(STROKE, LINE_BORDER))
            .corner_radius(CornerRadius::same(RADIUS)),
    )
}

impl Shell {
    pub(super) fn tick(&mut self, ctx: &Context) {
        let Some(span) = self.span() else {
            return;
        };
        let now = ctx.input(|input| input.time);
        self.clock.tick(&span, now);
        if self.clock.playing {
            ctx.request_repaint();
        }
    }

    pub(super) fn transport_key(&mut self, key: Key) {
        let Some(span) = self.span() else {
            return;
        };
        match key {
            Key::Space => self.clock.toggle(&span),
            Key::Comma => self.clock.step(&span, -1),
            Key::Period => self.clock.step(&span, 1),
            Key::F => self.clock.chase = !self.clock.chase,
            Key::OpenBracket => self.jump_marker(false),
            Key::CloseBracket => self.jump_marker(true),
            _ => {}
        }
    }

    fn jump_marker(&mut self, forward: bool) {
        let Some(span) = self.span() else {
            return;
        };
        let now = self.clock.time;
        let mut times: Vec<f64> = self.document.markers.iter().map(|(_, t)| *t).collect();
        times.sort_by(f64::total_cmp);
        let next = if forward {
            times.into_iter().find(|t| *t > now + 1e-6)
        } else {
            times.into_iter().rev().find(|t| *t < now - 1e-6)
        };
        if let Some(t) = next {
            self.clock.seek(&span, t);
        }
    }

    pub fn seek_to(&mut self, to: &str) -> Result<f64, String> {
        let span = self
            .span()
            .ok_or_else(|| "seek needs a scene with a timeline".to_string())?;
        let time = match to.parse::<f64>() {
            Ok(t) if t.is_finite() && t >= 0.0 => t,
            Ok(_) => {
                return Err(format!(
                    "seek wants seconds from 0 up or a marker, not {to}"
                ));
            }
            Err(_) => match self.document.markers.iter().find(|(name, _)| name == to) {
                Some((_, t)) => *t,
                None => {
                    let names: Vec<&str> = self
                        .document
                        .markers
                        .iter()
                        .map(|(n, _)| n.as_str())
                        .collect();
                    let known = if names.is_empty() {
                        "the scene has no markers".to_string()
                    } else {
                        format!("expected one of: {}", names.join(", "))
                    };
                    return Err(format!("no marker named {to}, {known}"));
                }
            },
        };
        self.clock.seek(&span, time);
        Ok(self.clock.time)
    }

    pub(super) fn transport(&mut self, ui: &mut Ui) {
        let Some(span) = self.span() else {
            return;
        };
        let size = Vec2::new(ui.available_width(), HEIGHT);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let mut bar = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(0.0, 4.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        bar.spacing_mut().item_spacing.x = 4.0;
        let playing = self.clock.playing;
        let play = if playing {
            format!("{PAUSE}  space")
        } else {
            format!("{PLAY}  space")
        };
        if toggle(&mut bar, &play, playing).clicked() {
            self.clock.toggle(&span);
        }
        if chip(&mut bar, &format!("{PREVIOUS}  ,"), true).clicked() {
            self.clock.step(&span, -1);
        }
        if chip(&mut bar, &format!("{NEXT}  ."), true).clicked() {
            self.clock.step(&span, 1);
        }
        if !self.document.markers.is_empty() {
            if chip(&mut bar, &format!("{PREVIOUS}  ["), true).clicked() {
                self.jump_marker(false);
            }
            if chip(&mut bar, &format!("{NEXT}  ]"), true).clicked() {
                self.jump_marker(true);
            }
        }
        if toggle(&mut bar, &format!("{LOOP}  loop"), self.clock.looping).clicked() {
            self.clock.looping = !self.clock.looping;
        }
        if toggle(
            &mut bar,
            &format!("{CAMERA}  Chase cam  f"),
            self.clock.chase,
        )
        .clicked()
        {
            self.clock.chase = !self.clock.chase;
        }
        if self.has_curves()
            && toggle(
                &mut bar,
                &format!("{}  curves  k", curves::CURVES),
                self.curves.open,
            )
            .clicked()
        {
            self.toggle_curves();
        }
        bar.add_space(8.0);
        let width = (bar.available_width() - READOUT).max(40.0);
        let (track, response) =
            bar.allocate_exact_size(Vec2::new(width, HEIGHT - 8.0), Sense::click_and_drag());
        if response.is_pointer_button_down_on()
            && let Some(at) = response.interact_pointer_pos()
        {
            let along = ((at.x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0);
            self.clock.seek(&span, f64::from(along) * span.seconds());
        }
        self.scrubber = Some(track);
        self.scrubber(&bar, track, &span);
        let frame = span.frame(self.clock.time);
        bar.add_space(8.0);
        bar.label(code(
            &bar,
            format!(
                "{:.2} s · {frame} / 0–{} · {} fps",
                self.clock.time, span.frames, span.fps
            ),
            TEXT_CODE,
        ));
    }

    fn scrubber(&self, ui: &Ui, track: Rect, span: &Span) {
        let painter = ui.painter();
        let y = track.center().y;
        let x = |time: f64| track.left() + track.width() * (time / span.seconds().max(1e-9)) as f32;
        painter.line_segment(
            [Pos2::new(track.left(), y), Pos2::new(track.right(), y)],
            Stroke::new(2.0, LINE_STRONG),
        );
        let seconds = span.seconds().floor() as u32;
        let every = (seconds / 20).max(1);
        for second in (0..=seconds).step_by(every as usize) {
            let at = x(f64::from(second));
            painter.line_segment(
                [Pos2::new(at, y - 5.0), Pos2::new(at, y + 5.0)],
                Stroke::new(STROKE, LINE_BORDER),
            );
        }
        for (name, time) in &self.document.markers {
            let at = x(*time);
            painter.line_segment(
                [Pos2::new(at, track.top()), Pos2::new(at, y + 5.0)],
                Stroke::new(1.5, TEXT_SECONDARY),
            );
            painter.text(
                Pos2::new(at + 2.0, track.top() - 1.0),
                Align2::LEFT_TOP,
                name,
                Theme::of(painter.ctx()).font(SIZE_CODE - 2.0),
                TEXT_SECONDARY,
            );
        }
        let now = x(self.clock.time);
        painter.line_segment(
            [Pos2::new(track.left(), y), Pos2::new(now, y)],
            Stroke::new(2.0, ACCENT),
        );
        painter.line_segment(
            [Pos2::new(now, track.top()), Pos2::new(now, track.bottom())],
            Stroke::new(STROKE, ACCENT),
        );
        painter.circle_filled(Pos2::new(now, y), KNOB, ACCENT);
    }
}
