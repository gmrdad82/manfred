use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use manfred_build::{Built, Cache, Registry};
use manfred_recipe::{Kind, Recipe};
use toml_edit::DocumentMut;

use pfx_editor_shell::worker::{Poll, Worker};

pub const EVERY: Duration = Duration::from_millis(250);
const REMEMBERED: usize = 512;

pub fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    Current,
    Earlier,
    Outside,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Own {
    current: Option<u64>,
    written: Vec<u64>,
}

impl Own {
    pub fn read(&mut self, text: &str) {
        self.current = Some(hash(text));
    }

    pub fn wrote(&mut self, text: &str) {
        let hash = hash(text);
        self.current = Some(hash);
        self.written.retain(|known| *known != hash);
        self.written.push(hash);
        if self.written.len() > REMEMBERED {
            self.written.remove(0);
        }
    }

    pub fn seen(&self, hash: u64) -> Seen {
        if self.current == Some(hash) {
            Seen::Current
        } else if self.written.contains(&hash) {
            Seen::Earlier
        } else {
            Seen::Outside
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Stage,
    Fake,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

pub fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(Stamp {
        modified: meta.modified().ok(),
        len: meta.len(),
    })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub stage: Option<String>,
    pub reveal: BTreeMap<String, f64>,
}

impl Shape {
    pub fn apply(&self, recipe: Recipe) -> Result<Recipe, String> {
        let mut recipe = match &self.stage {
            Some(stage) => {
                manfred_recipe::apply_stage(&recipe, stage).map_err(|error| error.to_string())?
            }
            None => recipe,
        };
        for (name, fraction) in &self.reveal {
            if let Some(object) = recipe
                .objects
                .iter_mut()
                .find(|object| object.name == *name)
                && let Kind::Generator { params, .. } = &mut object.kind
            {
                params.insert("reveal".into(), serde_json::json!(fraction));
            }
        }
        Ok(recipe)
    }
}

pub fn revealed(recipe: &Recipe, name: &str) -> Result<f64, String> {
    let object = recipe
        .objects
        .iter()
        .find(|object| object.name == name)
        .ok_or_else(|| format!("reveal: no object {name}"))?;
    match &object.kind {
        Kind::Generator { params, .. } => Ok(params
            .get("reveal")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(1.0)),
        _ => Err(format!("reveal: {name} is not a generator's object")),
    }
}

#[derive(Debug)]
pub struct Follow {
    path: PathBuf,
    mode: Mode,
    recipe: Option<Arc<Recipe>>,
    doc: Option<Arc<DocumentMut>>,
    built: Option<Arc<Built>>,
    error: Option<String>,
    generation: u64,
    stamps: Vec<(PathBuf, Option<Stamp>)>,
    pending: Option<Worker<(Option<Stamp>, Outcome)>>,
    registry: Arc<Registry>,
    cache: Arc<Mutex<Cache>>,
    shape: Shape,
}

#[derive(Debug)]
enum Outcome {
    Ready(Box<Recipe>, Box<DocumentMut>, Option<Box<Built>>),
    Unbuilt(Box<Recipe>, Box<DocumentMut>, String),
    Unread(String),
}

fn base(path: &Path) -> PathBuf {
    path.parent().map_or_else(PathBuf::new, Path::to_path_buf)
}

impl Follow {
    pub fn new(path: impl Into<PathBuf>, mode: Mode) -> Follow {
        Follow::new_with(path, mode, Registry::new())
    }

    pub fn idle(path: impl Into<PathBuf>, mode: Mode) -> Follow {
        Follow::idle_with(path, mode, Registry::new())
    }

    pub fn new_with(path: impl Into<PathBuf>, mode: Mode, registry: Registry) -> Follow {
        let mut follow = Follow::idle_with(path, mode, registry);
        follow.reload();
        follow
    }

    pub fn idle_with(path: impl Into<PathBuf>, mode: Mode, registry: Registry) -> Follow {
        Follow {
            path: path.into(),
            mode,
            recipe: None,
            doc: None,
            built: None,
            error: None,
            generation: 0,
            stamps: Vec::new(),
            pending: None,
            registry: Arc::new(registry),
            cache: Arc::default(),
            shape: Shape::default(),
        }
    }

    pub fn shape(&self) -> &Shape {
        &self.shape
    }

    pub fn set_shape(&mut self, shape: Shape) {
        self.shape = shape;
    }

    pub fn busy(&self) -> bool {
        self.pending.as_ref().is_some_and(Worker::busy)
    }

    pub fn request(&mut self, done: impl Fn() + Send + Sync + 'static) {
        let path = self.path.clone();
        let mode = self.mode;
        let registry = Arc::clone(&self.registry);
        let cache = Arc::clone(&self.cache);
        let shape = self.shape.clone();
        let mut worker = Worker::new(Arc::new(done));
        worker.start(move || (stamp(&path), read(&path, mode, &registry, &cache, &shape)));
        self.pending = Some(worker);
    }

    pub fn settle(&mut self) -> bool {
        let Some(Poll::Done((stamped, outcome))) = self.pending.as_mut().map(Worker::poll) else {
            return false;
        };
        self.apply(stamped, outcome)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn recipe(&self) -> Option<&Recipe> {
        self.recipe.as_deref()
    }

    pub fn doc(&self) -> Option<&DocumentMut> {
        self.doc.as_deref()
    }

    pub fn built(&self) -> Option<&Built> {
        self.built.as_deref()
    }

    pub fn shared(&self) -> Option<(Arc<Recipe>, Option<Arc<Built>>)> {
        let recipe = self.recipe.clone()?;
        Some((recipe, self.built.clone()))
    }

    pub fn rebuilt(&self) -> Vec<usize> {
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .rebuilt()
            .to_vec()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn ready(&self) -> bool {
        self.recipe.is_some() && (self.mode == Mode::Fake || self.built.is_some())
    }

    pub fn watched(&self) -> Vec<PathBuf> {
        if self.stamps.is_empty() {
            return vec![self.path.clone()];
        }
        self.stamps.iter().map(|(file, _)| file.clone()).collect()
    }

    pub fn stale(&self) -> bool {
        if self.stamps.is_empty() {
            return stamp(&self.path).is_some();
        }
        self.stamps.iter().any(|(file, then)| stamp(file) != *then)
    }

    pub fn reload(&mut self) -> bool {
        self.pending = None;
        let first = stamp(&self.path);
        let outcome = read(
            &self.path,
            self.mode,
            &self.registry,
            &self.cache,
            &self.shape,
        );
        self.apply(first, outcome)
    }

    fn apply(&mut self, first: Option<Stamp>, outcome: Outcome) -> bool {
        let before = (self.generation, self.error.clone());
        let files = match &outcome {
            Outcome::Ready(recipe, ..) | Outcome::Unbuilt(recipe, ..) => {
                let folder = base(&self.path);
                let mut files = vec![self.path.clone()];
                for name in recipe.files.names().into_iter().skip(1) {
                    let file = folder.join(name);
                    if !files.contains(&file) {
                        files.push(file);
                    }
                }
                files
            }
            Outcome::Unread(_) => self.watched(),
        };
        let files = if files.is_empty() {
            vec![self.path.clone()]
        } else {
            files
        };
        self.stamps = files
            .into_iter()
            .map(|file| {
                let then = if file == self.path {
                    first
                } else {
                    stamp(&file)
                };
                (file, then)
            })
            .collect();
        match outcome {
            Outcome::Ready(recipe, doc, built) => {
                self.recipe = Some(Arc::from(recipe));
                self.doc = Some(Arc::from(doc));
                self.built = built.map(Arc::from);
                self.error = None;
                self.generation += 1;
            }
            Outcome::Unbuilt(recipe, doc, error) => {
                if self.recipe.is_none() {
                    self.recipe = Some(Arc::from(recipe));
                    self.doc = Some(Arc::from(doc));
                    self.generation += 1;
                }
                self.error = Some(error);
            }
            Outcome::Unread(error) => self.error = Some(error),
        }
        before != (self.generation, self.error.clone())
    }
}

fn read(
    path: &Path,
    mode: Mode,
    registry: &Registry,
    cache: &Mutex<Cache>,
    shape: &Shape,
) -> Outcome {
    let loaded = match manfred_recipe::load(path) {
        Ok(loaded) => loaded,
        Err(error) if error.path == path.display().to_string() => {
            return Outcome::Unread(error.text);
        }
        Err(error) => return Outcome::Unread(error.to_string()),
    };
    let doc = Box::new(loaded.doc);
    let recipe = match shape.apply(loaded.recipe.clone()) {
        Ok(recipe) => Box::new(recipe),
        Err(error) => return Outcome::Unbuilt(Box::new(loaded.recipe), doc, error),
    };
    if mode == Mode::Fake {
        return Outcome::Ready(recipe, doc, None);
    }
    let mut cache = cache.lock().unwrap_or_else(PoisonError::into_inner);
    match manfred_build::build_cached(&recipe, &base(path), registry, &mut cache) {
        Ok(built) => Outcome::Ready(recipe, doc, Some(Box::new(built))),
        Err(error) => Outcome::Unbuilt(recipe, doc, format!("build: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch;
    use std::sync::mpsc;

    const ONE: &str = "manfred = 1\nname = \"one\"\n\n[[object]]\nname = \"a\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n";
    const TWO: &str = "manfred = 1\nname = \"two\"\n\n[[object]]\nname = \"a\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n\n[[object]]\nname = \"b\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n";
    const GENERATED: &str = "manfred = 1\nname = \"gen\"\n\n[[object]]\nname = \"stack\"\nkind = \"generator\"\ngenerator = \"test/crate\"\n";

    #[test]
    fn a_change_on_disk_reloads() {
        let path = scratch("follow-change");
        std::fs::write(&path, ONE).unwrap();
        let mut follow = Follow::new(&path, Mode::Fake);
        assert!(follow.ready());
        assert_eq!(follow.recipe().unwrap().name, "one");
        assert!(!follow.stale());
        let first = follow.generation();
        std::fs::write(&path, TWO).unwrap();
        assert!(follow.stale());
        assert!(follow.reload());
        assert_eq!(follow.recipe().unwrap().objects.len(), 2);
        assert_eq!(follow.generation(), first + 1);
        assert_eq!(follow.error(), None);
        assert!(!follow.stale());
    }

    #[test]
    fn a_failed_reload_keeps_the_last_good_recipe() {
        let path = scratch("follow-broken");
        std::fs::write(&path, TWO).unwrap();
        let mut follow = Follow::new(&path, Mode::Fake);
        let good = follow.generation();
        std::fs::write(&path, "manfred = 1\nname = \"two\"\n[[object]\n").unwrap();
        assert!(follow.reload());
        assert_eq!(follow.generation(), good);
        assert_eq!(follow.recipe().unwrap().name, "two");
        assert!(
            follow.error().unwrap().contains("TOML"),
            "{:?}",
            follow.error()
        );
        std::fs::write(&path, ONE).unwrap();
        assert!(follow.reload());
        assert_eq!(follow.error(), None);
        assert_eq!(follow.recipe().unwrap().name, "one");
    }

    #[test]
    fn a_build_failure_still_shows_the_first_recipe() {
        let path = scratch("follow-build");
        std::fs::write(&path, GENERATED).unwrap();
        let follow = Follow::new(&path, Mode::Stage);
        assert!(follow.built().is_none());
        assert!(!follow.ready());
        assert_eq!(follow.recipe().unwrap().name, "gen");
        assert!(follow.error().unwrap().starts_with("build: "));
        assert_eq!(follow.generation(), 1);
    }

    #[test]
    fn a_requested_reload_arrives_off_the_calling_thread() {
        let path = scratch("follow-request");
        std::fs::write(&path, ONE).unwrap();
        let mut follow = Follow::idle(&path, Mode::Fake);
        assert!(follow.recipe().is_none());
        let (send, receive) = mpsc::channel();
        follow.request(move || send.send(()).unwrap());
        assert!(follow.busy());
        receive.recv().unwrap();
        assert!(follow.settle());
        assert!(!follow.busy());
        assert_eq!(follow.recipe().unwrap().name, "one");
        assert!(!follow.stale());
        std::fs::write(&path, "manfred = 1\n[[object]\n").unwrap();
        let (send, receive) = mpsc::channel();
        follow.request(move || send.send(()).unwrap());
        receive.recv().unwrap();
        assert!(follow.settle());
        assert_eq!(follow.recipe().unwrap().name, "one");
        assert!(follow.error().is_some());
        assert!(!follow.settle());
    }

    fn crate_piece(_: &manfred_build::Call) -> Result<Vec<manfred_build::Piece>, String> {
        Ok(vec![manfred_build::Piece {
            name: "crate".into(),
            mesh: Default::default(),
        }])
    }

    #[test]
    fn the_worker_builds_generator_objects_through_the_registry() {
        let path = scratch("follow-generator");
        std::fs::write(&path, GENERATED).unwrap();
        let mut registry = Registry::new();
        registry.register("test/crate", crate_piece);
        let mut follow = Follow::idle_with(&path, Mode::Stage, registry);
        let (send, receive) = mpsc::channel();
        follow.request(move || send.send(()).unwrap());
        receive.recv().unwrap();
        assert!(follow.settle());
        assert_eq!(follow.error(), None);
        let names: Vec<&str> = follow
            .built()
            .unwrap()
            .parts
            .iter()
            .map(|p| p.node.name.as_str())
            .collect();
        assert_eq!(names, ["crate"]);
        let plain = Follow::new(&path, Mode::Stage);
        assert!(
            plain
                .error()
                .unwrap()
                .contains("`test/crate` is built by the app's own build binary"),
            "{:?}",
            plain.error()
        );
    }

    #[test]
    fn a_one_object_edit_rebuilds_only_that_object() {
        let path = scratch("follow-cache");
        std::fs::write(&path, TWO).unwrap();
        let mut follow = Follow::new(&path, Mode::Stage);
        assert_eq!(follow.error(), None);
        assert_eq!(follow.rebuilt(), [0, 1]);
        let edited = TWO.replace(
            "name = \"b\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]",
            "name = \"b\"\nkind = \"box\"\nsize = [2.0, 1.0, 1.0]",
        );
        assert_ne!(edited, TWO);
        std::fs::write(&path, &edited).unwrap();
        let (send, receive) = mpsc::channel();
        follow.request(move || send.send(()).unwrap());
        receive.recv().unwrap();
        assert!(follow.settle());
        assert_eq!(follow.error(), None);
        assert_eq!(follow.rebuilt(), [1]);
        let full = manfred_build::build(follow.recipe().unwrap(), &base(&path)).unwrap();
        assert!(follow.built().unwrap().glb() == full.glb());
    }

    #[test]
    fn the_hash_is_fnv_and_tells_texts_apart() {
        assert_eq!(hash(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(hash("a"), 0xaf63_dc4c_8601_ec8c);
        assert_ne!(hash(ONE), hash(TWO));
    }

    #[test]
    fn own_writes_are_told_from_outside_ones_by_their_hash() {
        let mut own = Own::default();
        assert_eq!(own.seen(hash(ONE)), Seen::Outside);
        own.read(ONE);
        assert_eq!(own.seen(hash(ONE)), Seen::Current);
        own.wrote(TWO);
        assert_eq!(own.seen(hash(TWO)), Seen::Current);
        assert_eq!(own.seen(hash(ONE)), Seen::Outside);
        own.wrote("three");
        assert_eq!(own.seen(hash(TWO)), Seen::Earlier);
        own.wrote(TWO);
        assert_eq!(own.seen(hash(TWO)), Seen::Current);
        assert_eq!(own.seen(hash("three")), Seen::Earlier);
    }

    #[test]
    fn a_missing_file_is_an_error_with_nothing_shown() {
        let path = scratch("follow-missing").with_file_name("absent.toml");
        let follow = Follow::new(&path, Mode::Fake);
        assert!(follow.recipe().is_none());
        assert!(follow.error().unwrap().contains("cannot read"));
        assert_eq!(follow.generation(), 0);
    }

    const RIG: &str =
        "manfred = 1\n\n[[light]]\nname = \"lamp\"\nposition = [0.0, 1.0, 0.0]\nintensity = 5.0\n";
    const PART: &str =
        "manfred = 1\n\n[[object]]\nname = \"knob\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n";
    const HOST: &str = "manfred = 1\nname = \"host\"\ninclude = [\"rig.toml\"]\n\n[[object]]\nname = \"a\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n\n[[use]]\nfile = \"part.toml\"\nprefix = \"p_\"\n";

    #[test]
    fn an_outside_edit_to_an_included_or_used_file_makes_the_recipe_stale() {
        let path = scratch("follow-includes");
        let dir = path.parent().unwrap().to_path_buf();
        std::fs::write(&path, HOST).unwrap();
        std::fs::write(dir.join("rig.toml"), RIG).unwrap();
        std::fs::write(dir.join("part.toml"), PART).unwrap();
        let mut follow = Follow::new(&path, Mode::Fake);
        assert!(follow.ready(), "{:?}", follow.error());
        let watched = follow.watched();
        assert_eq!(watched[0], path);
        assert!(watched.iter().any(|file| file.ends_with("rig.toml")));
        assert!(watched.iter().any(|file| file.ends_with("part.toml")));
        assert!(!follow.stale());
        std::fs::write(dir.join("rig.toml"), RIG.replace("1.0, 0.0", "1.5, 0.0")).unwrap();
        assert!(follow.stale());
        assert!(follow.reload());
        assert!(!follow.stale());
        std::fs::write(
            dir.join("part.toml"),
            PART.replace("[1.0, 1.0, 1.0]", "[2.0, 1.0, 1.0]"),
        )
        .unwrap();
        assert!(follow.stale());
        assert!(follow.reload());
        assert!(!follow.stale());
        std::fs::write(dir.join("rig.toml"), "manfred = 1\n[[light]\n").unwrap();
        assert!(follow.stale());
        follow.reload();
        assert!(follow.error().is_some());
        assert!(!follow.stale());
    }
}
