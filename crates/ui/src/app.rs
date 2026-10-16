use std::path::{Path, PathBuf};
use std::time::Duration;

use egui_wgpu::Renderer;
use pfx_editor_shell::worker::Wake;
use pfx_editor_shell::{App, Pgpu, Timing, Watch};
use pfx_gpu::Gpu;

use crate::follow::{self, Follow, Mode};
use crate::theme::Theme;
use crate::{Shell, Viewer};

const SCENE_EVERY: Duration = Duration::from_millis(50);

enum Seat<'a> {
    Own(Option<Box<Viewer>>),
    Lent(&'a mut Viewer),
}

impl Seat<'_> {
    fn get(&self) -> Option<&Viewer> {
        match self {
            Seat::Own(viewer) => viewer.as_deref(),
            Seat::Lent(viewer) => Some(viewer),
        }
    }

    fn get_mut(&mut self) -> Option<&mut Viewer> {
        match self {
            Seat::Own(viewer) => viewer.as_deref_mut(),
            Seat::Lent(viewer) => Some(viewer),
        }
    }

    fn put(&mut self, next: Viewer) {
        match self {
            Seat::Own(viewer) => *viewer = Some(Box::new(next)),
            Seat::Lent(viewer) => **viewer = next,
        }
    }
}

pub struct Manfred<'a> {
    shell: &'a mut Shell,
    viewer: Seat<'a>,
    wake: Option<Wake>,
    steady: bool,
}

impl<'a> Manfred<'a> {
    pub fn new(shell: &'a mut Shell, viewer: Option<Viewer>) -> Manfred<'a> {
        Manfred {
            shell,
            viewer: Seat::Own(viewer.map(Box::new)),
            wake: None,
            steady: false,
        }
    }

    pub fn lent(shell: &'a mut Shell, viewer: &'a mut Viewer) -> Manfred<'a> {
        Manfred {
            shell,
            viewer: Seat::Lent(viewer),
            wake: None,
            steady: false,
        }
    }

    pub fn wireframe(&self) -> crate::wire::Look {
        self.shell.wireframe()
    }

    pub fn set_wireframe(&mut self, amount: f32) {
        self.shell.set_wireframe(amount);
    }

    pub fn set_xray(&mut self, on: bool) {
        self.shell.set_xray(on);
    }

    pub fn act(&mut self, act: &crate::tape::Act) -> Result<(), String> {
        use crate::tape::Act;
        match act {
            Act::Stage(stage) => {
                let viewer = self.viewer.get_mut().ok_or("stage wants a recipe open")?;
                viewer.set_stage(stage)?;
            }
            Act::Reveal {
                object,
                goal,
                before,
                now,
            } => {
                let viewer = self.viewer.get_mut().ok_or("reveal wants a recipe open")?;
                viewer.reveal(object, *goal, *before, *now)?;
            }
            _ => return self.shell.direct(act),
        }
        if let (Some(viewer), Some(wake)) = (self.viewer.get_mut(), self.wake.as_ref()) {
            let built = wake.clone();
            viewer.request(self.shell, move || built());
        }
        Ok(())
    }

    pub fn set_clay(&mut self, amount: f32) {
        self.shell.set_clay(amount);
    }

    pub fn building(&self) -> bool {
        self.viewer
            .get()
            .is_some_and(|viewer| viewer.follow().busy() || viewer.staging())
    }

    pub fn curve_key(
        &mut self,
        camera: &str,
        track: &str,
        key: usize,
        at: [f64; 2],
    ) -> Result<(), String> {
        self.shell.curve_step(camera, track, key, at)
    }

    pub fn curve(&mut self, edit: crate::CurveEdit) -> Result<(), String> {
        self.shell.curve(edit, None)
    }

    pub fn rendering(&self) -> bool {
        self.shell.rendering()
    }

    pub fn steady(&mut self) {
        self.steady = true;
        if let Some(viewer) = self.viewer.get_mut() {
            viewer.steady();
        }
    }
}

fn start(viewer: &mut Viewer, shell: &mut Shell, wake: &Wake) {
    let staged = wake.clone();
    viewer.background(move || staged());
    let built = wake.clone();
    viewer.request(shell, move || built());
}

impl App for Manfred<'_> {
    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.wake.is_some()
            && let Some(viewer) = self.viewer.get_mut()
        {
            viewer.settle(self.shell);
            if !self.steady {
                self.shell
                    .set_frame_ms(viewer.frame_ms().map(|ms| ms as f32));
                self.shell.set_timing_drops(viewer.timing_drops());
            }
        }
        self.shell.ui(ui);
    }

    fn viewport(&self) -> bool {
        self.viewer.get().is_some()
    }

    fn frame(&mut self, gpu: &Gpu, renderer: &mut Renderer, pixels_per_point: f32) -> bool {
        match self.viewer.get_mut() {
            Some(viewer) => viewer.frame(gpu, renderer, self.shell, pixels_per_point),
            None => false,
        }
    }

    fn forget(&mut self) {
        if let Some(viewer) = self.viewer.get_mut() {
            viewer.forget();
            viewer.sync(self.shell);
        }
    }

    fn name(&self) -> &str {
        "manfred"
    }

    fn theme(&self) -> Theme {
        crate::theme::manfred()
    }

    fn label(&self) -> Option<String> {
        self.shell.label()
    }

    fn watched(&self) -> Watch {
        let Some(viewer) = self.viewer.get() else {
            return Watch::default();
        };
        let path = PathBuf::from(viewer.path());
        let files = viewer.watched();
        let every = if files.len() > 1 || files.first() != Some(&path) {
            SCENE_EVERY
        } else {
            follow::EVERY
        };
        Watch { files, every }
    }

    fn changed(&mut self, _files: &[PathBuf]) {
        if let (Some(viewer), Some(wake)) = (self.viewer.get_mut(), self.wake.as_ref()) {
            let built = wake.clone();
            viewer.request(self.shell, move || built());
        }
    }

    fn waker(&mut self, wake: Wake) {
        let staged = wake.clone();
        self.shell.set_waker(move || staged());
        if let Some(viewer) = self.viewer.get_mut() {
            start(viewer, self.shell, &wake);
        }
        self.wake = Some(wake);
    }

    fn opened(&mut self) -> bool {
        let Some(path) = self.shell.take_opened() else {
            return false;
        };
        let mut next = if crate::watched::is_scene(Path::new(&path)) {
            Viewer::scene(PathBuf::from(&path), Mode::Stage)
        } else {
            Viewer::with(Follow::idle_with(
                PathBuf::from(&path),
                Mode::Stage,
                self.shell.registry().clone(),
            ))
        };
        if self.steady {
            next.steady();
        }
        if let Some(wake) = &self.wake {
            start(&mut next, self.shell, wake);
        }
        self.viewer.put(next);
        true
    }

    fn pgpu(&mut self, state: Pgpu) {
        self.shell.set_pgpu(state);
    }

    fn timing(&self) -> Timing {
        match self.viewer.get() {
            Some(viewer) => Timing {
                gpu_ms: viewer.frame_ms(),
                drops: viewer.timing_drops(),
                drawn: viewer.showing(),
            },
            None => Timing::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Script;
    use manfred_build::Registry;
    use pfx_editor_shell::{drive, encode_png, pixels};

    fn recipe(name: &str) -> PathBuf {
        let path = crate::scratch(name);
        std::fs::write(
            &path,
            format!(
                "manfred = 1\nname = \"{name}\"\n\n[[object]]\nname = \"box\"\nkind = \"box\"\nsize = [0.2, 0.2, 0.2]\n"
            ),
        )
        .unwrap();
        path
    }

    #[test]
    fn opening_a_second_recipe_watches_it_instead_of_the_first() {
        let first = recipe("app-open-first");
        let second = recipe("app-open-second");
        let mut shell = Shell::open(&first.display().to_string()).without_recent();
        let viewer = Viewer::with(Follow::idle_with(&first, Mode::Stage, Registry::new()));
        let mut app = Manfred::new(&mut shell, Some(viewer));
        assert_eq!(app.name(), "manfred");
        assert_eq!(app.label().as_deref(), Some("scene.toml"));
        assert_eq!(app.watched().files, vec![first.clone()]);
        assert_eq!(app.watched().every, follow::EVERY);
        assert!(!app.opened());
        let script = format!("key ctrl+o\ntext {}\nkey Enter\nframe", second.display());
        let ctx = egui::Context::default();
        crate::theme::apply_theme(&ctx, &crate::theme::manfred());
        drive(&ctx, [1586, 992], &Script::parse(&script).unwrap(), |ui| {
            app.ui(ui)
        });
        assert!(app.opened());
        assert!(!app.opened());
        assert!(app.viewport());
        assert_eq!(app.watched().files, vec![second]);
    }

    #[test]
    fn without_a_viewer_nothing_is_watched_or_drawn() {
        let mut shell = Shell::new().without_recent();
        let app = Manfred::new(&mut shell, None);
        assert!(!app.viewport());
        assert_eq!(app.watched(), Watch::default());
        assert_eq!(app.timing(), Timing::default());
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_script_shoots_a_gizmo_held_mid_drag() {
        let since = std::time::Instant::now();
        let size = [1586, 992];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = crate::scratch("polish-gizmo");
        std::fs::copy(root.join("tests/golden/table/table.toml"), &path).unwrap();
        let mut viewer = Viewer::new(&path, crate::Mode::Stage);
        let mut shell = Shell::open(&path.display().to_string());
        viewer.sync(&mut shell);
        pixels(
            &mut Manfred::lent(&mut shell, &mut viewer),
            size,
            &Script::default(),
        )
        .unwrap();
        crate::gpu_turn(since);
        let lens = shell.lens().unwrap();
        let ball = shell.document().object("ball").unwrap();
        let centre = glam::DVec3::new(0.03, 0.042, -0.03);
        let at = lens.project(centre).unwrap();
        let mpp = lens.metres_per_point(centre);
        let from = lens.project(centre + glam::DVec3::X * mpp * 60.0).unwrap();
        let moves: String = (1..=6)
            .map(|step| format!("move {} {}\nframe\n", from.x + 10.0 * step as f32, from.y))
            .collect();
        let script = Script::parse(&format!(
            "click {} {}\nframe\nframe\npress {} {}\n{moves}",
            at.x, at.y, from.x, from.y
        ))
        .unwrap();
        let since = std::time::Instant::now();
        let rgba = pixels(&mut Manfred::lent(&mut shell, &mut viewer), size, &script).unwrap();
        assert_eq!(shell.selected(), Some(ball));
        assert!(shell.gizmo().dragging().is_some());
        let (moved, motion) = shell.preview().unwrap();
        assert_eq!(moved, ball);
        assert!(
            (f64::from(motion[3][0]) - 60.0 * mpp).abs() < 1e-4,
            "{motion:?}"
        );
        assert!(shell.edits().is_empty());
        std::fs::write(
            root.join("tmp/polish-gizmo-drag.png"),
            encode_png(size, &rgba).unwrap(),
        )
        .unwrap();
        crate::gpu_turn(since);
    }
}
