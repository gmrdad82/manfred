use glam::{DMat4, DVec3};
use manfred_build::{Built, Track};
use manfred_recipe::Recipe;
use manfred_scene::Eye;
pub use manfred_scene::poses;

const LONGEST_STEP: f64 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub fps: u32,
    pub frames: u32,
    pub aspect: f32,
}

impl Span {
    pub fn of(recipe: &Recipe) -> Option<Span> {
        let timeline = recipe.timeline.as_ref()?;
        let [width, height] = timeline.size;
        Some(Span {
            fps: timeline.fps.max(1),
            frames: timeline.frames.max(1),
            aspect: width.max(1) as f32 / height.max(1) as f32,
        })
    }

    pub fn seconds(&self) -> f64 {
        self.time(self.frames)
    }

    pub fn time(&self, frame: u32) -> f64 {
        f64::from(frame) / f64::from(self.fps)
    }

    pub fn frame(&self, time: f64) -> u32 {
        ((time * f64::from(self.fps) + 1e-6).floor().max(0.0) as u32).min(self.frames)
    }

    fn clamp(&self, time: f64) -> f64 {
        if time.is_finite() {
            time.clamp(0.0, self.seconds())
        } else {
            0.0
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clock {
    pub time: f64,
    pub playing: bool,
    pub looping: bool,
    pub chase: bool,
    run: f64,
    last: Option<f64>,
}

impl Clock {
    pub fn fit(&mut self, span: Option<Span>) {
        match span {
            Some(span) => {
                self.time = span.clamp(self.time);
                self.run = span.clamp(self.run);
            }
            None => {
                self.playing = false;
                self.last = None;
            }
        }
    }

    pub fn tick(&mut self, span: &Span, now: f64) -> bool {
        let last = self.last.replace(now);
        if !self.playing {
            return false;
        }
        let step = last.map_or(0.0, |last| (now - last).clamp(0.0, LONGEST_STEP));
        let end = span.seconds();
        let mut run = self.run + step;
        if run >= end {
            if self.looping && end > 0.0 {
                run %= end;
            } else {
                run = end;
                self.playing = false;
            }
        }
        self.run = run;
        let time = span.time(span.frame(run));
        let moved = time != self.time;
        self.time = time;
        moved
    }

    pub fn toggle(&mut self, span: &Span) {
        if self.playing {
            self.playing = false;
            return;
        }
        if span.frame(self.time) >= span.frames {
            self.time = 0.0;
        }
        self.run = self.time;
        self.playing = true;
    }

    pub fn seek(&mut self, span: &Span, time: f64) {
        self.playing = false;
        self.time = span.clamp(time);
        self.run = self.time;
    }

    pub fn step(&mut self, span: &Span, frames: i64) {
        let now = i64::from(span.frame(self.time));
        let next = if frames < 0 && span.time(now as u32) < self.time - 1e-9 {
            now
        } else {
            now + frames
        };
        let next = if self.looping {
            next.rem_euclid(i64::from(span.frames) + 1)
        } else {
            next.clamp(0, i64::from(span.frames))
        };
        self.seek(span, span.time(next as u32));
    }
}

fn at(track: &Track, fps: u32, time: f64) -> Option<DMat4> {
    (!track.keys.is_empty())
        .then(|| manfred_build::matrix(&manfred_build::sample(&track.keys, fps, time)))
}

pub fn chase(built: &Built, fps: u32, time: f64) -> Option<Eye> {
    let track = built.tracks.iter().find(|track| track.camera.is_some())?;
    let lens = track.camera?;
    let world = at(track, fps, time)?;
    let position = world.transform_point3(DVec3::ZERO);
    let forward = world.transform_vector3(DVec3::NEG_Z).normalize_or_zero();
    let up = world.transform_vector3(DVec3::Y).normalize_or_zero();
    if forward == DVec3::ZERO || up == DVec3::ZERO {
        return None;
    }
    Some(Eye {
        position: position.to_array(),
        target: (position + forward).to_array(),
        up: up.to_array(),
        fov_y_deg: lens.yfov.to_degrees(),
        shift: [0.0, 0.0],
        ortho_height: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Span {
        Span {
            fps: 10,
            frames: 20,
            aspect: 0.5625,
        }
    }

    #[test]
    fn playback_never_runs_ahead_of_the_clock_and_stops_or_loops_at_the_end() {
        let span = span();
        let mut clock = Clock::default();
        clock.tick(&span, 0.0);
        clock.toggle(&span);
        clock.tick(&span, 0.0);
        assert!(clock.tick(&span, 0.25));
        assert!((clock.time - 0.2).abs() < 1e-9);
        assert!(!clock.tick(&span, 0.25));
        clock.tick(&span, 10.0);
        assert!((clock.time - 0.5).abs() < 1e-9);
        for now in 1..40 {
            clock.tick(&span, 10.0 + f64::from(now) * 0.1);
        }
        assert_eq!(clock.time, 2.0);
        assert!(!clock.playing);
        clock.looping = true;
        clock.tick(&span, 20.0);
        clock.toggle(&span);
        assert_eq!(clock.time, 0.0);
        for now in 1..=25 {
            clock.tick(&span, 20.0 + f64::from(now) * 0.1);
        }
        assert!(clock.playing);
        assert!((clock.time - 0.5).abs() < 1e-9, "{}", clock.time);
    }

    fn trs(x: f64, turn: f64) -> manfred_motion::Trs {
        let rotation = glam::DQuat::from_rotation_y(turn).to_array();
        manfred_motion::Trs {
            translation: [x, 0.0, 0.0],
            rotation,
            scale: [1.0; 3],
        }
    }

    #[test]
    fn a_tracked_part_moves_from_its_rest_and_the_camera_track_gives_the_chase_eye() {
        let recipe = manfred_recipe::parse(
            "manfred = 1\nname = \"t\"\n\n[[object]]\nname = \"car\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n",
        )
        .unwrap();
        let mut built = manfred_build::build(&recipe, std::path::Path::new(".")).unwrap();
        let name = built.parts[0].node.name.clone();
        built.parts[0].node.keep_transform = true;
        built.parts[0].node.transform = DMat4::from_translation(DVec3::new(1.0, 0.0, 0.0));
        built.tracks = vec![
            Track {
                node: name.clone(),
                keys: vec![trs(1.0, 0.0), trs(3.0, 0.0)],
                camera: None,
                focus: None,
            },
            Track {
                node: "camera.chase".into(),
                keys: vec![trs(0.0, 0.0), trs(0.0, std::f64::consts::PI)],
                camera: Some(manfred_build::Lens {
                    yfov: 0.5,
                    aspect: 0.5625,
                    znear: 0.05,
                }),
                focus: None,
            },
        ];
        let posed = poses(&built, 1, 0.5);
        assert_eq!(posed.len(), 1);
        assert_eq!(posed[&name][3][0], 1.0);
        assert_eq!(poses(&built, 1, 0.0)[&name][3][0], 0.0);
        let start = chase(&built, 1, 0.0).unwrap();
        assert_eq!(start.target, [0.0, 0.0, -1.0]);
        assert!((start.fov_y_deg - 0.5f64.to_degrees()).abs() < 1e-9);
        let half = chase(&built, 1, 0.5).unwrap();
        assert!((half.target[0] + 1.0).abs() < 1e-9, "{:?}", half.target);
    }

    #[test]
    fn steps_land_on_frames_and_a_rebuild_keeps_the_time() {
        let span = span();
        let mut clock = Clock::default();
        clock.seek(&span, 0.73);
        clock.step(&span, -1);
        assert!((clock.time - 0.7).abs() < 1e-9);
        clock.step(&span, 1);
        assert!((clock.time - 0.8).abs() < 1e-9);
        clock.step(&span, -100);
        assert_eq!(clock.time, 0.0);
        clock.seek(&span, 1.5);
        clock.fit(Some(span));
        assert_eq!(clock.time, 1.5);
        clock.fit(Some(Span { frames: 10, ..span }));
        assert_eq!(clock.time, 1.0);
    }
}
