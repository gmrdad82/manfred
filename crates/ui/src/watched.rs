use std::path::{Path, PathBuf};

use glam::{DMat4, DVec3};
use manfred_recipe::Camera as Shot;
use manfred_scene::Eye;
use pfx_gpu::{Gpu, wgpu};
use pfx_live::frame::{Camera, MeshStats, multiply};
use pfx_live::renderer::{Exposure, Renderer, Text};
use pfx_live::scene::{Applied, Staged};
use pfx_load::scene::{Projection, Reload, Scene, SceneError, SceneWatch};

use crate::animated::Animated;
use crate::document::{Document, Item, Lamp, tree_of};
use crate::follow::{Stamp, stamp};
use crate::pick::{Ray, triangle};

const OUTPUT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const IN_FLIGHT: u64 = 2;
const EMPTY: [[f64; 3]; 2] = [[-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]];
pub const CAMERA: &str = "camera";

type Looked = ([[f32; 4]; 4], [[f32; 4]; 4]);

fn worded(error: &SceneError, scene: &Path) -> String {
    let folder = scene.parent().unwrap_or(Path::new(""));
    let file = error.file.strip_prefix(folder).unwrap_or(&error.file);
    match error.line {
        Some(line) => format!("{}:{line}: {}", file.display(), error.message),
        None => format!("{}: {}", file.display(), error.message),
    }
}

pub fn is_scene(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".scene.toml"))
}

#[derive(Clone, Debug)]
pub enum Event {
    Reloaded(Box<Reload>),
    Broken,
}

pub struct Watched {
    path: PathBuf,
    watch: Option<SceneWatch>,
    opened: Option<Option<Stamp>>,
    error: Option<String>,
    generation: u64,
    reloads: u64,
}

impl Watched {
    pub fn open(path: impl Into<PathBuf>) -> Watched {
        let path = path.into();
        let mut watched = Watched {
            path,
            watch: None,
            opened: None,
            error: None,
            generation: 0,
            reloads: 0,
        };
        watched.reopen();
        watched
    }

    fn reopen(&mut self) -> bool {
        let now = stamp(&self.path);
        if self.opened.as_ref() == Some(&now) {
            return false;
        }
        self.opened = Some(now);
        match SceneWatch::open(&self.path) {
            Ok(watch) => {
                self.watch = Some(watch);
                self.error = None;
                self.generation += 1;
                true
            }
            Err(error) => {
                self.error = Some(worded(&error, &self.path));
                false
            }
        }
    }

    pub fn scene(&self) -> Option<&Scene> {
        self.watch.as_ref().map(SceneWatch::scene)
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn reloads(&self) -> u64 {
        self.reloads
    }

    pub fn files(&self) -> Vec<PathBuf> {
        match &self.watch {
            Some(watch) => watch.files().map(Path::to_path_buf).collect(),
            None => vec![self.path.clone()],
        }
    }

    pub fn check(&mut self) -> Option<Event> {
        let result = match self.watch.as_mut() {
            Some(watch) => watch.check(),
            None => return self.reopen().then(|| self.opened_event()).flatten(),
        };
        self.take(result)
    }

    pub fn poll(&mut self) -> Option<Event> {
        let result = match self.watch.as_mut() {
            Some(watch) => watch.poll(),
            None => return self.reopen().then(|| self.opened_event()).flatten(),
        };
        self.take(result)
    }

    fn opened_event(&self) -> Option<Event> {
        let scene = self.scene()?.clone();
        Some(Event::Reloaded(Box::new(Reload {
            diff: Scene::diff(&scene, &scene),
            scene,
            recovered: true,
        })))
    }

    fn take(&mut self, result: Option<Result<Reload, SceneError>>) -> Option<Event> {
        match result? {
            Ok(reload) => {
                self.error = None;
                self.generation += 1;
                self.reloads += 1;
                Some(Event::Reloaded(Box::new(reload)))
            }
            Err(error) => {
                self.error = Some(worded(&error, &self.path));
                Some(Event::Broken)
            }
        }
    }
}

fn wide(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::from(value))
}

pub fn shot(scene: &Scene) -> Shot {
    let camera = scene.camera_or_default();
    let (kind, fov, height) = match camera.projection {
        Projection::Perspective { fov } => (
            manfred_recipe::Projection::Perspective,
            Some(wide(fov)),
            None,
        ),
        Projection::Orthographic { height } => (
            manfred_recipe::Projection::Orthographic,
            None,
            Some(wide(height)),
        ),
    };
    Shot {
        name: CAMERA.to_string(),
        kind,
        position: camera.at.map(wide),
        target: camera.look_at.map(wide),
        up: camera.up.map(wide),
        fov_y_deg: fov,
        focal_mm: None,
        sensor_mm: None,
        shift: [0.0, 0.0],
        focus_m: None,
        fstop: 0.0,
        ortho_height: height,
        path: None,
        rig: None,
    }
}

fn first_seen(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for name in names {
        if !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen
}

fn lights(scene: &Scene) -> Vec<(Lamp, String, String)> {
    let mut lights = Vec::new();
    if let Some(sun) = &scene.sun {
        let lamp = if sun.daylight.is_some() {
            Lamp::Daylight
        } else {
            Lamp::Sun
        };
        lights.push((
            lamp,
            "sun".to_string(),
            format!("{:.1} W/m²  hour {:.1}", sun.intensity, sun.hour),
        ));
    }
    if let Some(sky) = &scene.sky {
        lights.push((Lamp::Sky, "sky".to_string(), sky.kind.clone()));
    }
    for (name, light) in &scene.lights {
        lights.push((Lamp::Bulb, name.clone(), format!("{:.2}", light.intensity)));
    }
    for (name, emitter) in &scene.emitters {
        lights.push((
            Lamp::Bulb,
            name.clone(),
            format!("{:.2}  r {:.2}", emitter.intensity, emitter.radius),
        ));
    }
    lights
}

pub fn document(scene: &Scene, path: &str) -> Document {
    let objects: Vec<Item> = scene
        .objects
        .iter()
        .map(|object| Item {
            name: object.name.clone(),
            material: match (&object.material, object.materials.is_empty()) {
                (Some(material), _) => material.clone(),
                (None, false) => object
                    .materials
                    .values()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" "),
                (None, true) => String::new(),
            },
            location: object.at,
            rotation: object.rotate,
            scale: object.scale,
        })
        .collect();
    let names: Vec<String> = scene.objects.iter().map(|o| o.name.clone()).collect();
    let parents: Vec<Option<String>> = scene.objects.iter().map(|o| o.parent.clone()).collect();
    let materials = first_seen(
        scene
            .objects
            .iter()
            .flat_map(|object| object.material.iter().chain(object.materials.values()))
            .cloned()
            .chain(scene.library.names().map(str::to_string)),
    );
    let camera = shot(scene);
    let detail = match (camera.fov_y_deg, camera.ortho_height) {
        (_, Some(height)) => format!("ortho  {height:.2} m"),
        (Some(fov), None) => format!("fov {fov:.1}°"),
        (None, None) => String::new(),
    };
    Document {
        name: Path::new(path)
            .file_name()
            .map(|name| {
                name.to_string_lossy()
                    .trim_end_matches(".scene.toml")
                    .to_string()
            })
            .unwrap_or_default(),
        path: path.to_string(),
        objects,
        nodes: tree_of(&names, &parents),
        cameras: vec![(CAMERA.to_string(), detail)],
        views: vec![camera],
        lights: lights(scene),
        materials,
        hour: None,
        aspect: crate::viewport::ASPECT,
        patch: "none".to_string(),
        source: 0,
        timeline: crate::animated::span(scene),
    }
}

pub fn object_of(scene: &Scene, id: u32) -> Option<usize> {
    scene.objects.iter().position(|object| object.id == id)
}

fn matrix(model: [[f32; 4]; 4]) -> DMat4 {
    DMat4::from_cols_array_2d(&model.map(|column| column.map(f64::from)))
}

pub fn cast(scene: &Scene, ray: &Ray) -> Option<usize> {
    let mut nearest: Option<(f64, u32)> = None;
    for draw in &scene.draws().items {
        if !draw.shadow.drawn() {
            continue;
        }
        let world = matrix(draw.model);
        let corner = |index: u32| {
            let p = draw.geometry.positions[index as usize];
            world.transform_point3(DVec3::new(
                f64::from(p[0]),
                f64::from(p[1]),
                f64::from(p[2]),
            ))
        };
        for face in draw.geometry.indices.chunks_exact(3) {
            if let Some(distance) =
                triangle(ray, [corner(face[0]), corner(face[1]), corner(face[2])])
                && nearest.is_none_or(|(best, _)| distance < best)
            {
                nearest = Some((distance, draw.id));
            }
        }
    }
    nearest.and_then(|(_, id)| object_of(scene, id))
}

pub fn bounds(scene: &Scene) -> [[f64; 3]; 2] {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for draw in &scene.draws().items {
        let world = matrix(draw.model);
        for p in &draw.geometry.positions {
            let at = world.transform_point3(DVec3::new(
                f64::from(p[0]),
                f64::from(p[1]),
                f64::from(p[2]),
            ));
            for axis in 0..3 {
                low[axis] = low[axis].min(at[axis]);
                high[axis] = high[axis].max(at[axis]);
            }
        }
    }
    if low.iter().chain(&high).all(|value| value.is_finite()) {
        [low, high]
    } else {
        EMPTY
    }
}

pub struct Live {
    renderer: Renderer,
    staged: Staged,
    size: [u32; 2],
    bounds: [[f64; 3]; 2],
    previous: Option<[[f32; 4]; 4]>,
    animated: Option<Animated>,
    time: f32,
    looked: Option<Looked>,
}

impl Live {
    pub fn new(gpu: Gpu, scene: &Scene, size: [u32; 2]) -> Result<Live, String> {
        let mut renderer = Renderer::new_with_output_format(gpu, size[0], size[1], OUTPUT)
            .map_err(|error| format!("the live renderer could not start: {error}"))?;
        renderer.set_exposure(Exposure::Fixed(1.0))?;
        let staged = renderer.stage(scene)?;
        Ok(Live {
            renderer,
            staged,
            size,
            bounds: bounds(scene),
            previous: None,
            animated: Animated::of(scene),
            time: 0.0,
            looked: None,
        })
    }

    pub fn apply(&mut self, reload: &Reload) -> Result<Applied, String> {
        let applied = self
            .staged
            .apply(&mut self.renderer, &reload.scene, &reload.diff)?;
        if applied.draws {
            self.bounds = bounds(&reload.scene);
        }
        self.animated = Animated::of(&reload.scene);
        Ok(applied)
    }

    pub fn set_time(&mut self, time: Option<f64>) -> bool {
        let time = time.unwrap_or(0.0) as f32;
        let moved = time != self.time;
        self.time = time;
        moved && self.animated.is_some()
    }

    pub fn chase(&self, time: f64) -> Option<Eye> {
        self.animated.as_ref()?.chase(time as f32)
    }

    pub fn output_format(&self) -> wgpu::TextureFormat {
        self.renderer.output_format()
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn resize(&mut self, size: [u32; 2]) -> Result<(), String> {
        if size == self.size {
            return Ok(());
        }
        self.renderer.resize(size[0], size[1])?;
        self.size = size;
        self.previous = None;
        Ok(())
    }

    pub fn settling(&self) -> bool {
        self.renderer.sky_settling()
    }

    pub fn mesh_stats(&self) -> MeshStats {
        self.renderer.mesh_stats()
    }

    pub fn frame_ms(&self) -> Option<f64> {
        self.renderer
            .timings()
            .filter(|timings| !timings.passes.is_empty())
            .map(|timings| timings.passes.iter().map(|t| t.milliseconds).sum())
    }

    pub fn timing_drops(&self) -> u64 {
        let drops = self.renderer.timing_drops();
        drops.ring + drops.late
    }

    pub fn looked(&self) -> Option<Looked> {
        self.looked
    }

    pub fn depth(&self) -> &wgpu::TextureView {
        &self.renderer.frame().targets.depth_view
    }

    pub fn pick(&mut self, x: u32, y: u32) -> Result<(), String> {
        self.renderer.pick(x, y)
    }

    pub fn picked(&mut self) -> Option<u32> {
        self.renderer.picked().map(|picked| picked.id)
    }

    fn camera(&mut self, eye: &Eye) -> Camera {
        let view = eye.view();
        let (near, far) = eye.depth_range(self.bounds);
        let projection = eye.projection(self.size, near, far);
        let current = multiply(projection, view);
        let camera = Camera {
            view,
            projection,
            previous_view_projection: self.previous.unwrap_or(current),
            position: eye.position.map(|v| v as f32),
        };
        self.previous = Some(current);
        self.looked = Some((view, projection));
        camera
    }

    pub fn render(
        &mut self,
        eye: &Eye,
        output: &wgpu::TextureView,
        live: bool,
    ) -> Result<(), String> {
        let camera = self.camera(eye);
        let aspect = self.size[0].max(1) as f32 / self.size[1].max(1) as f32;
        let finish = self.staged.finish();
        let time = self.time;
        if let Some(animated) = self.animated.as_mut() {
            animated.pose(&self.staged, time);
        }
        let mut frame = self.staged.frame(aspect, time, 0);
        frame.scene.camera = camera;
        if let Some(animated) = &self.animated {
            frame.scene.instances = &animated.instances;
            frame.effects.glass = &animated.glass;
            frame.effects.sprays = &animated.spray_instances;
            if animated.lights.is_some() {
                frame.effects.lights = animated.lights.as_deref();
            }
        }
        if live {
            self.renderer.submit(
                &frame.scene,
                &Text::default(),
                &frame.effects,
                finish,
                output,
            )?;
            if self.renderer.frames_in_flight() > IN_FLIGHT {
                self.renderer.wait_frames(IN_FLIGHT)?;
            }
        } else {
            self.renderer.render(
                &frame.scene,
                &Text::default(),
                &frame.effects,
                finish,
                output,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub fn fixture(name: &str) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/scene");
    let to = crate::scratch(name).with_file_name("scene");
    std::fs::create_dir_all(&to).unwrap();
    for entry in std::fs::read_dir(&from).unwrap().flatten() {
        std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
    }
    to.join("stand.scene.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened(name: &str) -> (PathBuf, Watched) {
        let path = fixture(name);
        let watched = Watched::open(&path);
        assert_eq!(watched.error(), None);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .starts_with("format = 1\n")
        );
        let warnings = &watched.scene().unwrap().warnings;
        assert!(warnings.is_empty(), "{warnings:?}");
        (path, watched)
    }

    #[test]
    fn a_scene_file_is_named_scene_toml() {
        assert!(is_scene(Path::new("rooms/stand.scene.toml")));
        assert!(!is_scene(Path::new("rooms/stand.toml")));
        assert!(!is_scene(Path::new("rooms/scene.toml")));
    }

    #[test]
    fn the_document_lists_the_objects_under_their_parents_and_the_scenes_camera() {
        let (path, watched) = opened("watched-document");
        let document = document(watched.scene().unwrap(), &path.display().to_string());
        assert_eq!(document.name, "stand");
        let rows: Vec<(usize, &str)> = document
            .nodes
            .iter()
            .map(|node| (node.depth, node.label.as_str()))
            .collect();
        assert_eq!(
            rows,
            [(0, "floor"), (0, "plinth"), (1, "ball"), (0, "crate")]
        );
        assert_eq!(document.objects[2].material, "brass");
        assert_eq!(document.objects[1].location, [-0.5, 0.25, 0.0]);
        assert_eq!(
            document.cameras,
            [("camera".to_string(), "fov 40.0°".to_string())]
        );
        assert_eq!(document.views[0].position, [0.0, 1.2, 3.2]);
        assert_eq!(document.materials, ["felt", "clay", "brass"]);
        let lamps: Vec<(Lamp, &str)> = document
            .lights
            .iter()
            .map(|(lamp, name, _)| (*lamp, name.as_str()))
            .collect();
        assert_eq!(
            lamps,
            [(Lamp::Sun, "sun"), (Lamp::Sky, "sky"), (Lamp::Bulb, "lamp")]
        );
    }

    #[test]
    fn check_reloads_a_changed_material_and_nothing_else() {
        let (path, mut watched) = opened("watched-material");
        assert!(watched.check().is_none());
        let library = path.with_file_name("stand.materials.toml");
        let text = std::fs::read_to_string(&library).unwrap();
        std::fs::write(
            &library,
            text.replace("base = [0.70, 0.32, 0.22]", "base = [0.20, 0.40, 0.70]"),
        )
        .unwrap();
        let Some(Event::Reloaded(reload)) = watched.check() else {
            panic!("the material edit did not reload");
        };
        assert_eq!(reload.diff.materials.changed, ["clay"]);
        assert!(reload.diff.objects.is_empty() && reload.diff.meshes.is_empty());
        assert!(!reload.diff.sky && !reload.diff.sun && !reload.diff.camera);
        assert!(!reload.recovered);
        assert_eq!(watched.reloads(), 1);
        assert!(watched.check().is_none());
    }

    #[test]
    fn a_broken_save_names_the_line_keeps_the_scene_and_recovers() {
        let (path, mut watched) = opened("watched-broken");
        let good = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            good.replace("intensity = 4.0", "intensity = 4.0\nwatts = 2"),
        )
        .unwrap();
        let Some(Event::Broken) = watched.check() else {
            panic!("the broken save was not reported");
        };
        let error = watched.error().unwrap().to_string();
        let line = good
            .lines()
            .position(|line| line == "intensity = 4.0")
            .unwrap()
            + 2;
        assert!(
            error.starts_with(&format!("stand.scene.toml:{line}:")),
            "{error}"
        );
        assert!(error.contains("'watts'"), "{error}");
        assert_eq!(watched.scene().unwrap().objects.len(), 4);
        assert!(watched.check().is_none());
        std::fs::write(&path, good.replace("intensity = 4.0", "intensity = 5.0")).unwrap();
        let Some(Event::Reloaded(reload)) = watched.check() else {
            panic!("the good save did not recover");
        };
        assert!(reload.recovered);
        assert_eq!(reload.diff.lights.changed, ["lamp"]);
        assert_eq!(watched.error(), None);
    }

    #[test]
    fn a_broken_library_names_its_own_file_beside_the_scene() {
        let (path, mut watched) = opened("watched-library");
        let library = path.with_file_name("stand.materials.toml");
        let text = std::fs::read_to_string(&library).unwrap();
        std::fs::write(
            &library,
            text.replace("roughness = 0.3", "roughness = \"rough\""),
        )
        .unwrap();
        let Some(Event::Broken) = watched.check() else {
            panic!("the broken library was not reported");
        };
        let error = watched.error().unwrap();
        assert!(error.starts_with("stand.materials.toml:"), "{error}");
        assert_eq!(watched.scene().unwrap().objects.len(), 4);
    }

    #[test]
    fn a_scene_broken_when_opened_opens_once_it_reads() {
        let path = fixture("watched-late");
        let good = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, good.replace("[sky]", "[sky]\nshade = 1")).unwrap();
        let mut watched = Watched::open(&path);
        assert!(watched.scene().is_none());
        assert!(watched.error().unwrap().contains("stand.scene.toml:"));
        assert!(watched.check().is_none());
        std::fs::write(&path, &good).unwrap();
        let Some(Event::Reloaded(reload)) = watched.check() else {
            panic!("the scene did not open once it read");
        };
        assert!(reload.recovered && reload.diff.is_empty());
        assert_eq!(watched.error(), None);
        assert!(
            watched
                .files()
                .iter()
                .any(|file| file.ends_with("ball.gltf"))
        );
    }

    #[test]
    fn a_ray_picks_the_nearest_object_by_its_id() {
        let (_, watched) = opened("watched-cast");
        let scene = watched.scene().unwrap();
        let down = |x: f64, z: f64| Ray {
            origin: DVec3::new(x, 5.0, z),
            direction: DVec3::new(0.0, -1.0, 0.0),
        };
        assert_eq!(cast(scene, &down(-0.5, 0.0)), Some(2));
        assert_eq!(cast(scene, &down(0.7, 0.3)), Some(3));
        assert_eq!(cast(scene, &down(1.8, -1.8)), Some(0));
        assert_eq!(cast(scene, &down(3.0, 3.0)), None);
        let [low, high] = bounds(scene);
        assert!(
            (low[0] + 2.0).abs() < 1e-6 && (high[1] - 0.9).abs() < 1e-6,
            "{low:?} {high:?}"
        );
    }
}
