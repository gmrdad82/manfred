use glam::{DMat4, DVec3};
use manfred_glb::{Lod, Node, Order, Targets, sidecar_text};
use manfred_mesh::Morph;
use manfred_recipe::{self as recipe, Kind, Object, Recipe, Shadow};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod bake;
pub mod builtin;
pub mod detail_uv;
mod extras;
mod facts;
mod holes;
mod lod;
mod maps;
mod normal_map;
mod scene;
mod shape;
mod timeline;
mod uv;

pub use manfred_glb::{
    CameraFacts, DaylightFacts, FACTS, GradeFacts, LampFacts, LookFacts, MoverFacts, RoomFacts,
    SceneFacts, SidecarWriter, SkyFacts, SunFacts, facts as facts_sidecar, merge, mover_json,
};
pub use manfred_glb::{Image, Look, Looks, Map, Mime};
pub use manfred_glb::{Lens, Track};
pub use manfred_motion::{Route, Timeline, Trs, matrix, sample, trs_from_matrix};
pub use manfred_recipe::NO_SIDECAR;
pub use maps::{LARGEST_IMAGE, image, image_fits};
pub use scene::object_ids;
pub use timeline::{Animates, Bake, Baker, camera_track, lens, pose, time_refusal};

pub const GENERATOR: &str = concat!("manfred ", env!("CARGO_PKG_VERSION"));

pub const BUILTIN: &str = "manfred/";

pub struct Call<'a> {
    pub object: &'a Object,
    pub params: &'a BTreeMap<String, serde_json::Value>,
    pub seed: u64,
    pub base: &'a Path,
    pub routes: &'a BTreeMap<String, Route>,
}

impl Call<'_> {
    pub fn route(&self, name: &str) -> Option<&Route> {
        self.routes.get(name)
    }
}

fn routes(recipe: &Recipe) -> Result<BTreeMap<String, Route>, Error> {
    recipe
        .routes
        .iter()
        .map(|spec| {
            Route::new(&spec.points, spec.closed)
                .map(|route| (spec.name.clone(), route))
                .map_err(|text| Error::new(format!("route[{}]", spec.name), text))
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct Piece {
    pub name: String,
    pub mesh: manfred_mesh::Mesh,
}

pub type Generator = fn(&Call) -> Result<Vec<Piece>, String>;

#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedLight {
    pub name: String,
    pub position: [f64; 3],
    pub direction: [f64; 3],
    pub cone: [f64; 2],
    pub range: f64,
    pub color: [f64; 3],
    pub intensity: f64,
    pub shadow: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Generated {
    pub pieces: Vec<Piece>,
    pub facts: serde_json::Map<String, serde_json::Value>,
    pub materials: BTreeMap<String, serde_json::Value>,
    pub lights: Vec<GeneratedLight>,
}

pub type FactsGenerator = fn(&Call) -> Result<Generated, String>;

#[derive(Clone, Copy, Debug)]
enum Entry {
    Pieces(Generator),
    Facts(FactsGenerator),
}

impl Entry {
    fn run(self, call: &Call) -> Result<Generated, String> {
        match self {
            Entry::Pieces(generator) => generator(call).map(|pieces| Generated {
                pieces,
                ..Generated::default()
            }),
            Entry::Facts(generator) => generator(call),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Registry {
    generators: BTreeMap<String, Entry>,
    sidecars: BTreeMap<String, SidecarWriter>,
    materials: BTreeMap<String, serde_json::Value>,
    bakers: Vec<(String, Baker)>,
    animates: Vec<Animates>,
}

impl Default for Registry {
    fn default() -> Registry {
        Registry::new()
    }
}

impl Registry {
    pub fn new() -> Registry {
        let mut sidecars = BTreeMap::new();
        sidecars.insert(FACTS.to_string(), manfred_glb::facts as SidecarWriter);
        let mut registry = Registry {
            generators: BTreeMap::new(),
            sidecars,
            materials: BTreeMap::new(),
            bakers: bake::BAKERS
                .iter()
                .map(|&(name, baker)| (name.to_string(), baker))
                .collect(),
            animates: bake::ANIMATES.to_vec(),
        };
        builtin::builtins(&mut registry);
        registry
    }

    pub fn register_baker(&mut self, name: &str, baker: Baker, animates: Animates) {
        self.bakers.retain(|(have, _)| have != name);
        self.bakers.push((name.to_string(), baker));
        self.animates.push(animates);
    }

    pub fn baker_names(&self) -> impl Iterator<Item = &str> {
        self.bakers.iter().map(|(name, _)| name.as_str())
    }

    pub fn register(&mut self, name: &str, generator: Generator) {
        self.generators
            .insert(name.to_string(), Entry::Pieces(generator));
    }

    pub fn register_with_facts(&mut self, name: &str, generator: FactsGenerator) {
        self.generators
            .insert(name.to_string(), Entry::Facts(generator));
    }

    pub fn get(&self, name: &str) -> Option<Generator> {
        match self.generators.get(name) {
            Some(Entry::Pieces(generator)) => Some(*generator),
            _ => None,
        }
    }

    pub fn get_with_facts(&self, name: &str) -> Option<FactsGenerator> {
        match self.generators.get(name) {
            Some(Entry::Facts(generator)) => Some(*generator),
            _ => None,
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.generators.contains_key(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.generators.keys().map(String::as_str)
    }

    pub fn register_sidecar(&mut self, name: &str, writer: SidecarWriter) {
        if name != NO_SIDECAR {
            self.sidecars.insert(name.to_string(), writer);
        }
    }

    pub fn sidecar(&self, name: &str) -> Option<SidecarWriter> {
        self.sidecars.get(name).copied()
    }

    pub fn sidecar_names(&self) -> impl Iterator<Item = &str> {
        self.sidecars.keys().map(String::as_str)
    }

    pub fn register_materials<I, K>(&mut self, catalogue: I)
    where
        I: IntoIterator<Item = (K, serde_json::Value)>,
        K: Into<String>,
    {
        for (name, value) in catalogue {
            self.materials.insert(name.into(), value);
        }
    }

    pub fn register_materials_toml(&mut self, text: &str) -> Result<(), String> {
        let catalogue = recipe::materials(text).map_err(|e| e.to_string())?;
        self.register_materials(catalogue);
        Ok(())
    }

    pub fn material(&self, name: &str) -> Option<&serde_json::Value> {
        self.materials.get(name)
    }

    pub fn material_names(&self) -> impl Iterator<Item = &str> {
        self.materials.keys().map(String::as_str)
    }
}

#[derive(Clone, Debug)]
pub struct Part {
    pub node: Node,
    pub object: usize,
    pub shadow: Shadow,
    pub two_sided: bool,
    pub hidden: bool,
    pub looks: Arc<Looks>,
}

#[derive(Clone, Debug)]
pub struct Built {
    pub parts: Vec<Part>,
    pub order: Order,
    pub facts: SceneFacts,
    pub sidecar: Option<serde_json::Value>,
    pub extras: Vec<Option<serde_json::Value>>,
    pub materials: BTreeMap<String, serde_json::Value>,
    pub catalogue: BTreeMap<String, serde_json::Value>,
    pub notes: Vec<String>,
    pub timeline: Option<Timeline>,
    pub tracks: Vec<Track>,
    facts_by_object: serde_json::Map<String, serde_json::Value>,
    pub lights: Vec<GeneratedLight>,
    pub looks: Arc<Looks>,
    pub lods: Vec<Lod>,
    nest: Vec<Option<(usize, DMat4)>>,
}

impl Built {
    pub fn recipe(&self, recipe: &Recipe) -> Recipe {
        let mut merged = self.recipe_without_catalogue(recipe);
        for (name, value) in &self.catalogue {
            merged.materials.insert(name.clone(), value.clone());
        }
        merged
    }

    fn recipe_without_catalogue(&self, recipe: &Recipe) -> Recipe {
        let mut merged = recipe.clone();
        for (name, value) in &self.materials {
            merged
                .materials
                .entry(name.clone())
                .or_insert_with(|| value.clone());
        }
        merged
    }

    pub fn nodes(&self) -> Vec<Node> {
        self.parts
            .iter()
            .filter(|part| !part.hidden)
            .map(|part| part.node.clone())
            .collect()
    }

    pub fn glb(&self) -> Vec<u8> {
        self.try_glb().unwrap_or_default()
    }

    pub fn try_glb(&self) -> Result<Vec<u8>, Error> {
        let visible: Vec<usize> = (0..self.parts.len())
            .filter(|&at| !self.parts[at].hidden)
            .collect();
        let extras: Vec<Option<serde_json::Value>> = visible
            .iter()
            .map(|&at| self.extras.get(at).cloned().flatten())
            .collect();
        let mut placed = vec![None; self.parts.len()];
        for (position, &at) in visible.iter().enumerate() {
            placed[at] = Some(position);
        }
        let tracked = |at: usize| {
            self.tracks
                .iter()
                .any(|t| t.camera.is_none() && t.node == self.parts[at].node.name)
        };
        let mut nodes = Vec::with_capacity(visible.len());
        let mut parents = Vec::with_capacity(visible.len());
        for &at in &visible {
            let mut node = self.parts[at].node.clone();
            let nested = self
                .nest
                .get(at)
                .copied()
                .flatten()
                .filter(|&(parent, _)| !tracked(at) && !tracked(parent));
            let parent = match nested {
                Some((parent, local)) => {
                    node.transform = local;
                    node.keep_transform = true;
                    placed[parent]
                }
                None => None,
            };
            nodes.push(node);
            parents.push(parent);
        }
        let lods: Vec<Lod> = self
            .lods
            .iter()
            .filter_map(|lod| {
                let of = placed.get(lod.of).copied().flatten()?;
                Some(Lod { of, ..lod.clone() })
            })
            .collect();
        let tracks: Vec<Track> = self
            .tracks
            .iter()
            .filter(|t| t.camera.is_some() || nodes.iter().any(|n| n.name == t.node))
            .cloned()
            .collect();
        let animation = self
            .timeline
            .filter(|_| !tracks.is_empty())
            .map(|timeline| manfred_glb::Animation {
                fps: timeline.fps,
                tracks: &tracks,
            });
        manfred_glb::write_animated(
            &nodes,
            &parents,
            &extras,
            manfred_glb::Content {
                looks: &self.looks,
                lods: &lods,
            },
            self.order,
            GENERATOR,
            animation,
        )
        .map_err(|e| Error::new("", e.0))
    }

    pub fn sidecar(&self) -> Option<String> {
        self.sidecar.as_ref().map(sidecar_text)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub path: String,
    pub text: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str(&self.text)
        } else {
            write!(f, "{}: {}", self.path, self.text)
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    pub fn new(path: impl Into<String>, text: impl Into<String>) -> Error {
        Error {
            path: path.into(),
            text: text.into(),
        }
    }
}

pub(crate) fn fail(object: &Object, key: &str, text: impl Into<String>) -> Error {
    Error::new(format!("object[{}].{key}", object.name), text)
}

fn sin_cos(degrees: f64) -> (f64, f64) {
    let quarter = degrees / 90.0;
    if quarter.fract() == 0.0 {
        match quarter.rem_euclid(4.0) as u8 {
            0 => (0.0, 1.0),
            1 => (1.0, 0.0),
            2 => (0.0, -1.0),
            _ => (-1.0, 0.0),
        }
    } else {
        degrees.to_radians().sin_cos()
    }
}

pub fn rotation(degrees: [f64; 3]) -> DMat4 {
    let (sx, cx) = sin_cos(degrees[0]);
    let (sy, cy) = sin_cos(degrees[1]);
    let (sz, cz) = sin_cos(degrees[2]);
    let x = DMat4::from_cols_array(&[
        1.0, 0.0, 0.0, 0.0, 0.0, cx, sx, 0.0, 0.0, -sx, cx, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);
    let y = DMat4::from_cols_array(&[
        cy, 0.0, -sy, 0.0, 0.0, 1.0, 0.0, 0.0, sy, 0.0, cy, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);
    let z = DMat4::from_cols_array(&[
        cz, sz, 0.0, 0.0, -sz, cz, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);
    z * y * x
}

pub fn local_matrix(object: &Object) -> DMat4 {
    DMat4::from_translation(DVec3::from(object.at))
        * rotation(object.rotate)
        * DMat4::from_scale(DVec3::from(object.scale))
}

pub fn world(recipe: &Recipe, index: usize) -> Result<DMat4, Error> {
    let mut matrix = DMat4::IDENTITY;
    let mut at = index;
    for _ in 0..=recipe.objects.len() {
        let object = &recipe.objects[at];
        matrix = local_matrix(object) * matrix;
        match &object.parent {
            None => return Ok(matrix),
            Some(parent) => match recipe.objects.iter().rposition(|o| o.name == *parent) {
                Some(next) => at = next,
                None => {
                    return Err(fail(
                        object,
                        "parent",
                        format!("no object is named `{parent}`"),
                    ));
                }
            },
        }
    }
    Err(fail(
        &recipe.objects[index],
        "parent",
        "the parents form a loop",
    ))
}

fn nest(recipe: &Recipe, parts: &[Part]) -> Vec<Option<(usize, DMat4)>> {
    if !recipe.output.hierarchy {
        return Vec::new();
    }
    let mut first = vec![None; recipe.objects.len()];
    for (at, part) in parts.iter().enumerate() {
        if !part.hidden && first[part.object].is_none() {
            first[part.object] = Some(at);
        }
    }
    let parent = |at: usize| {
        let name = recipe.objects[at].parent.as_ref()?;
        recipe.objects.iter().rposition(|o| o.name == *name)
    };
    parts
        .iter()
        .map(|part| {
            let mut chain = DMat4::IDENTITY;
            let mut at = part.object;
            for _ in 0..=recipe.objects.len() {
                chain = local_matrix(&recipe.objects[at]) * chain;
                at = parent(at)?;
                if let Some(above) = first[at] {
                    let node = &parts[above].node;
                    let local = match (node.keep_transform, part.node.keep_transform) {
                        (true, true) => chain,
                        (false, true) => part.node.transform,
                        (true, false) => node.transform.inverse(),
                        (false, false) => DMat4::IDENTITY,
                    };
                    return Some((above, local));
                }
            }
            None
        })
        .collect()
}

fn glb_order(order: recipe::Order) -> Order {
    match order {
        recipe::Order::File => Order::File,
        recipe::Order::Natural => Order::Natural,
    }
}

fn sidecar(
    recipe: &Recipe,
    registry: &Registry,
    facts: &SceneFacts,
) -> Result<Option<serde_json::Value>, Error> {
    let name = recipe.output.sidecar_dialect.as_str();
    if !recipe.output.writes_sidecar() {
        return Ok(None);
    }
    let Some(writer) = registry.sidecar(name) else {
        let known: Vec<&str> = registry.sidecar_names().collect();
        return Err(Error::new(
            "output.sidecar_dialect",
            format!(
                "no sidecar writer is named `{name}`, expected \"{NO_SIDECAR}\" or one of: {}; an app registers its own through the Manfred library",
                known.join(", ")
            ),
        ));
    };
    let written = writer(facts)
        .map_err(|text| Error::new("output.sidecar_dialect", format!("{name}: {text}")))?;
    let serde_json::Value::Object(mut root) = written else {
        return Err(Error::new(
            "output.sidecar_dialect",
            format!("{name}: a sidecar writer returns a JSON object"),
        ));
    };
    merge(&mut root, &facts.extra);
    Ok(Some(serde_json::Value::Object(root)))
}

fn catalogue(
    parts: &[Part],
    registry: &Registry,
    recipe: &Recipe,
    notes: &mut Vec<String>,
) -> BTreeMap<String, serde_json::Value> {
    let mut used = BTreeMap::new();
    for part in parts {
        for name in part
            .node
            .mesh
            .slots
            .iter()
            .map(|slot| slot.material.as_str())
        {
            if used.contains_key(name) {
                continue;
            }
            if let Some(value) = registry.material(name) {
                if recipe.materials.get(name).is_some_and(|own| own != value) {
                    notes.push(format!(
                        "materials.{name}: the app's catalogue entry is used, the recipe's own is not"
                    ));
                }
                used.insert(name.to_string(), value.clone());
            }
        }
    }
    used
}

fn pockets_by_target(recipe: &Recipe) -> Result<BTreeMap<&str, Vec<&Object>>, Error> {
    let mut out: BTreeMap<&str, Vec<&Object>> = BTreeMap::new();
    for object in &recipe.objects {
        let Kind::Pocket { into, .. } = &object.kind else {
            continue;
        };
        match recipe.objects.iter().find(|o| &o.name == into) {
            Some(Object {
                kind: Kind::Outline { .. },
                ..
            }) => out.entry(into.as_str()).or_default().push(object),
            Some(target) => {
                return Err(fail(
                    object,
                    "into",
                    format!(
                        "a pocket cuts only into an outline, `{into}` is a {}",
                        target.kind.name()
                    ),
                ));
            }
            None => {
                return Err(fail(object, "into", format!("no object is named `{into}`")));
            }
        }
    }
    Ok(out)
}

pub fn outlines(recipe: &Recipe) -> Vec<recipe::Problem> {
    let pockets = match pockets_by_target(recipe) {
        Ok(pockets) => pockets,
        Err(e) => return vec![problem(recipe, e)],
    };
    recipe
        .objects
        .iter()
        .flat_map(|object| {
            let cuts = pockets
                .get(object.name.as_str())
                .map_or(&[][..], Vec::as_slice);
            shape::refusals(object, cuts)
        })
        .map(|e| problem(recipe, e))
        .collect()
}

pub fn missing_materials(
    recipe: &Recipe,
    base: &Path,
    registry: &Registry,
) -> Vec<recipe::Problem> {
    let mut defined: std::collections::BTreeSet<String> = recipe
        .materials
        .keys()
        .cloned()
        .chain(registry.material_names().map(str::to_string))
        .collect();
    let generated = recipe.objects.iter().any(|object| {
        matches!(&object.kind, Kind::Generator { generator, .. } if registry.generators.contains_key(generator))
    });
    if generated && let Ok(built) = build_with(recipe, base, registry) {
        defined.extend(built.materials.keys().cloned());
    }
    let mut out = Vec::new();
    for (i, object) in recipe.objects.iter().enumerate() {
        if object.hidden || matches!(object.kind, Kind::Pocket { .. } | Kind::Text { .. }) {
            continue;
        }
        let names: Vec<&str> = match &object.material {
            Some(name) => vec![name.as_str()],
            None if !object.materials.is_empty() => {
                object.materials.iter().map(String::as_str).collect()
            }
            None if matches!(object.kind, Kind::Mesh { .. } | Kind::Generator { .. }) => continue,
            None => vec!["default"],
        };
        let mut seen: Vec<&str> = Vec::new();
        for name in names {
            if defined.contains(name) || seen.contains(&name) {
                continue;
            }
            seen.push(name);
            out.push(recipe.problem(
                format!("object[{}].material", object.name),
                &format!("object#{i}.material"),
                recipe::missing_material_text(defined.iter().map(String::as_str), name),
            ));
        }
    }
    out
}

fn problem(recipe: &Recipe, e: Error) -> recipe::Problem {
    let ipath = recipe
        .objects
        .iter()
        .enumerate()
        .find_map(|(i, o)| {
            let key = e.path.strip_prefix(&format!("object[{}]", o.name))?;
            let key = key.replace('[', "#").replace(']', "");
            Some(format!("object#{i}{key}"))
        })
        .unwrap_or_default();
    recipe.problem(e.path, &ipath, e.text)
}

pub fn seed(recipe: u64, index: usize) -> u64 {
    splitmix(recipe ^ splitmix(index as u64))
}

fn splitmix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn generate(
    recipe: &Recipe,
    index: usize,
    base: &Path,
    registry: &Registry,
    routes: &BTreeMap<String, Route>,
) -> Result<Generated, Error> {
    let object = &recipe.objects[index];
    let Kind::Generator { generator, params } = &object.kind else {
        return Ok(Generated::default());
    };
    let Some(&entry) = registry.generators.get(generator) else {
        let builtin = generator.starts_with(BUILTIN);
        let known: Vec<&str> = registry
            .names()
            .filter(|name| name.starts_with(BUILTIN) == builtin)
            .collect();
        let text = if known.is_empty() {
            format!(
                "`{generator}` is built by the app's own build binary, which registers it through the Manfred library"
            )
        } else {
            format!(
                "no generator is named `{generator}`, expected one of: {}",
                known.join(", ")
            )
        };
        return Err(fail(object, "generator", text));
    };
    let call = Call {
        object,
        params,
        seed: seed(recipe.seed, index),
        base,
        routes,
    };
    entry
        .run(&call)
        .map_err(|text| fail(object, "generator", format!("{generator}: {text}")))
}

fn claim<'a>(
    claimed: &mut BTreeMap<String, &'a str>,
    names: &BTreeMap<&str, usize>,
    object: &'a Object,
    piece: &str,
) -> Result<(), Error> {
    let clash = if piece != object.name && names.contains_key(piece) {
        Some(format!(
            "the piece `{piece}` has the name of the object `{piece}`"
        ))
    } else {
        claimed.get(piece).map(|owner| {
            format!("the piece `{piece}` has the name of a piece of the object `{owner}`")
        })
    };
    match clash {
        Some(text) => Err(fail(object, "generator", text)),
        None => {
            claimed.insert(piece.to_string(), &object.name);
            Ok(())
        }
    }
}

fn offer<'a>(
    offered: &mut BTreeMap<String, (&'a str, serde_json::Value)>,
    object: &'a Object,
    materials: BTreeMap<String, serde_json::Value>,
) -> Result<(), Error> {
    for (name, value) in materials {
        match offered.get(&name) {
            Some((owner, have)) if *have != value => {
                return Err(fail(
                    object,
                    "generator",
                    format!(
                        "the material `{name}` differs from the one the object `{owner}` returned"
                    ),
                ));
            }
            Some(_) => {}
            None => {
                offered.insert(name, (&object.name, value));
            }
        }
    }
    Ok(())
}

pub fn build(recipe: &Recipe, base: &Path) -> Result<Built, Error> {
    build_with(recipe, base, &Registry::new())
}

pub fn build_with(recipe: &Recipe, base: &Path, registry: &Registry) -> Result<Built, Error> {
    build_cached(recipe, base, registry, &mut Cache::default())
}

#[derive(Clone, Debug)]
struct Made {
    nodes: Vec<Node>,
    lods: Vec<Lod>,
    facts: serde_json::Map<String, serde_json::Value>,
    materials: BTreeMap<String, serde_json::Value>,
    lights: Vec<GeneratedLight>,
    looks: Looks,
}

#[derive(Clone, Debug, Default)]
pub struct Cache {
    made: BTreeMap<String, Made>,
    rebuilt: Vec<usize>,
}

impl Cache {
    pub fn rebuilt(&self) -> &[usize] {
        &self.rebuilt
    }

    pub fn len(&self) -> usize {
        self.made.len()
    }

    pub fn is_empty(&self) -> bool {
        self.made.is_empty()
    }
}

fn key(
    recipe: &Recipe,
    index: usize,
    base: &Path,
    registry: &Registry,
    cuts: &[&Object],
    matrix: &Result<DMat4, Error>,
) -> Option<String> {
    let object = &recipe.objects[index];
    let entry = match &object.kind {
        Kind::Mesh { .. } => return None,
        Kind::Generator { generator, .. } => Some((
            registry.generators.get(generator),
            seed(recipe.seed, index),
            &recipe.routes,
        )),
        _ => None,
    };
    let eye =
        matches!(object.uv, Some(recipe::Uv::Plate)).then_some((&recipe.plate, &recipe.cameras));
    Some(format!("{:?}", (object, base, entry, cuts, matrix, eye)))
}

fn placed(light: GeneratedLight, matrix: DMat4) -> GeneratedLight {
    let direction = matrix
        .transform_vector3(DVec3::from(light.direction))
        .normalize_or(DVec3::NEG_Y);
    GeneratedLight {
        position: matrix
            .transform_point3(DVec3::from(light.position))
            .to_array(),
        direction: direction.to_array(),
        ..light
    }
}

fn make(
    recipe: &Recipe,
    index: usize,
    (base, routes): (&Path, &BTreeMap<String, Route>),
    registry: &Registry,
    cuts: &[&Object],
    matrix: Result<DMat4, Error>,
    taken: impl FnOnce(Vec<&str>, &Made) -> Result<(), Error>,
) -> Result<Made, Error> {
    let object = &recipe.objects[index];
    let mut morph: Option<Morph> = None;
    let mut looks = Looks::new();
    let generated = if matches!(object.kind, Kind::Generator { .. }) {
        generate(recipe, index, base, registry, routes)?
    } else {
        let mesh = match &object.kind {
            Kind::Mesh { path, node, .. } => {
                let (mesh, mut found, kept) = shape::imported(object, base, path, node.as_deref())?;
                shape::weights(object, &mut found)?;
                morph = (!found.is_empty()).then_some(found);
                looks = kept;
                mesh
            }
            _ => shape::local(object, cuts, base)?,
        };
        Generated {
            pieces: vec![Piece {
                name: object.name.clone(),
                mesh,
            }],
            ..Generated::default()
        }
    };
    let Generated {
        pieces,
        facts,
        materials,
        lights,
    } = generated;
    let mut made = Made {
        nodes: Vec::with_capacity(pieces.len()),
        lods: Vec::new(),
        facts,
        materials,
        lights: Vec::new(),
        looks,
    };
    taken(
        pieces.iter().map(|piece| piece.name.as_str()).collect(),
        &made,
    )?;
    let matrix = matrix?;
    made.lights = lights
        .into_iter()
        .map(|light| placed(light, matrix))
        .collect();
    for Piece { name, mut mesh } in pieces {
        let mut morph = morph.take();
        let follow = |morph: &mut Option<Morph>, sources: Option<Vec<u32>>| {
            if let (Some(morph), Some(sources)) = (morph.as_mut(), sources) {
                morph.remap(&sources);
            }
        };
        follow(&mut morph, shape::uvs(recipe, object, matrix, &mut mesh)?);
        follow(&mut morph, detail_uv::apply(object, matrix, &mut mesh)?);
        shape::materials(object, &mut mesh)?;
        let transform = if object.keep_transform {
            matrix
        } else {
            manfred_mesh::transform(&mut mesh, matrix);
            if let Some(morph) = morph.as_mut() {
                morph.transform(matrix);
            }
            DMat4::IDENTITY
        };
        shape::normals(object, &mut mesh, morph.as_mut());
        let targets = morph.map(|morph| {
            let base = mesh.clone();
            morph.pose(&mut mesh);
            Box::new(Targets { base, morph })
        });
        let node = Node {
            name,
            mesh,
            transform,
            keep_transform: object.keep_transform,
            targets,
        };
        let of = made.nodes.len();
        made.lods.extend(
            lod::levels(object, &node)?
                .into_iter()
                .map(|lod| Lod { of, ..lod }),
        );
        made.nodes.push(node);
    }
    Ok(made)
}

pub fn build_cached(
    recipe: &Recipe,
    base: &Path,
    registry: &Registry,
    cache: &mut Cache,
) -> Result<Built, Error> {
    cache.rebuilt.clear();
    let mut used = std::collections::BTreeSet::new();
    let animated = timeline::animated(recipe, registry);
    let kept;
    let staged = if animated.is_empty() {
        recipe
    } else {
        kept = timeline::kept(recipe, &animated);
        &kept
    };
    let routes = routes(recipe)?;
    let mut built = assemble(staged, base, registry, cache, &mut used, &routes);
    if let Ok(made) = &mut built {
        made.tracks = timeline::bake(staged, made, &routes, registry)?;
        cache.made.retain(|key, _| used.contains(key));
    }
    built
}

fn assemble(
    recipe: &Recipe,
    base: &Path,
    registry: &Registry,
    cache: &mut Cache,
    used: &mut std::collections::BTreeSet<String>,
    routes: &BTreeMap<String, Route>,
) -> Result<Built, Error> {
    let names_of: BTreeMap<&str, usize> = recipe
        .objects
        .iter()
        .enumerate()
        .map(|(i, o)| (o.name.as_str(), i))
        .collect();
    let pockets = pockets_by_target(recipe)?;
    let mut claimed = BTreeMap::new();
    let mut parts = Vec::new();
    let mut generated = serde_json::Map::new();
    let mut by_object = serde_json::Map::new();
    let mut offered: BTreeMap<String, (&str, serde_json::Value)> = BTreeMap::new();
    let mut lights = Vec::new();
    let mut imported = Looks::new();
    let mut lods = Vec::new();
    for (index, object) in recipe.objects.iter().enumerate() {
        if matches!(object.kind, Kind::Pocket { .. }) {
            continue;
        }
        let cuts = pockets
            .get(object.name.as_str())
            .map_or(&[][..], Vec::as_slice);
        let matrix = world(recipe, index);
        let key = key(recipe, index, base, registry, cuts, &matrix);
        let mut take = |names: Vec<&str>, made: &Made| -> Result<(), Error> {
            if matches!(object.kind, Kind::Generator { .. }) {
                for name in names {
                    claim(&mut claimed, &names_of, object, name)?;
                }
                offer(&mut offered, object, made.materials.clone())?;
                manfred_glb::merge(&mut generated, &made.facts);
                if !made.facts.is_empty() {
                    by_object.insert(
                        object.name.clone(),
                        serde_json::Value::Object(made.facts.clone()),
                    );
                }
            }
            Ok(())
        };
        let made = match key.as_ref().and_then(|key| cache.made.get(key)) {
            Some(made) => {
                take(
                    made.nodes.iter().map(|node| node.name.as_str()).collect(),
                    made,
                )?;
                made.clone()
            }
            None => {
                cache.rebuilt.push(index);
                let made = make(recipe, index, (base, routes), registry, cuts, matrix, take)?;
                if let Some(key) = &key {
                    cache.made.insert(key.clone(), made.clone());
                }
                made
            }
        };
        if let Some(key) = key {
            used.insert(key);
        }
        if !object.hidden {
            lights.extend(made.lights);
        }
        for (name, look) in made.looks {
            imported.entry(name).or_insert(look);
        }
        let first = parts.len();
        lods.extend(made.lods.into_iter().map(|lod| Lod {
            of: first + lod.of,
            ..lod
        }));
        for node in made.nodes {
            parts.push(Part {
                node,
                object: index,
                shadow: object.shadow,
                two_sided: object.two_sided,
                hidden: object.hidden,
                looks: Arc::default(),
            });
        }
    }
    let looks = Arc::new(maps::resolve(recipe, base, imported)?);
    for part in &mut parts {
        part.looks = looks.clone();
    }
    let facts = facts::facts(recipe, &generated);
    let sidecar = sidecar(recipe, registry, &facts)?;
    let mut materials = BTreeMap::new();
    let mut notes = Vec::new();
    for (name, (owner, value)) in offered {
        match recipe.materials.get(&name) {
            Some(own) if *own != value => notes.push(format!(
                "materials.{name}: the recipe's entry replaces the one the object `{owner}` returned"
            )),
            Some(_) => {}
            None => {
                materials.insert(name, value);
            }
        }
    }
    let catalogue = catalogue(&parts, registry, recipe, &mut notes);
    notes.extend(scene::left_out(recipe));
    notes.extend(scene::lights_left_out(recipe, &lights));
    Ok(Built {
        materials,
        catalogue,
        notes,
        lights,
        looks,
        lods,
        extras: extras::of(recipe, &parts, &facts.movers),
        nest: nest(recipe, &parts),
        timeline: recipe.timeline,
        tracks: Vec::new(),
        facts_by_object: by_object,
        parts,
        order: glb_order(recipe.output.order),
        sidecar,
        facts,
    })
}

fn target(recipe_path: Option<&str>, fallback: String, base: &Path, out: Option<&Path>) -> PathBuf {
    match (out, recipe_path) {
        (Some(dir), Some(path)) => {
            let name = Path::new(path)
                .file_name()
                .map_or_else(|| fallback.clone().into(), PathBuf::from);
            dir.join(name)
        }
        (Some(dir), None) => dir.join(fallback),
        (None, Some(path)) => base.join(path),
        (None, None) => base.join(fallback),
    }
}

fn put(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let fail = |text: String| Error::new(path.display().to_string(), text);
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| fail(format!("cannot create the folder: {e}")))?;
    }
    let name = path
        .file_name()
        .ok_or_else(|| fail("expected a file name".into()))?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(name);
    temp_name.push(".tmp");
    let temp = path.with_file_name(temp_name);
    std::fs::write(&temp, bytes).map_err(|e| fail(format!("cannot write: {e}")))?;
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        fail(format!("cannot rename into place: {e}"))
    })
}

pub fn write(
    built: &Built,
    recipe: &Recipe,
    base: &Path,
    out: Option<&Path>,
) -> Result<Vec<PathBuf>, Error> {
    let mut written = Vec::new();
    let glb = target(
        recipe.output.glb.as_deref(),
        format!("{}.glb", recipe.name),
        base,
        out,
    );
    let scene_path = recipe.output.scene.as_ref().map(|file| {
        target(
            Some(file),
            format!("{}{}", recipe.name, recipe::SCENE_SUFFIX),
            base,
            out,
        )
    });
    let bake = match &recipe.output.bake {
        Some(file) => {
            let path = target(Some(file), "bake.toml".to_string(), base, out);
            let text = bake::text(built, recipe, &glb, &path, scene_path.as_deref(), base)?;
            Some((text, path))
        }
        None => None,
    };
    let scene = match scene_path {
        Some(path) => Some((scene::text(built, recipe, &path, &glb, base)?, path)),
        None => None,
    };
    put(&glb, &built.try_glb()?)?;
    written.push(glb);
    if let Some(text) = built.sidecar() {
        let path = target(
            recipe.output.sidecar.as_deref(),
            format!("{}.json", recipe.name),
            base,
            out,
        );
        put(&path, text.as_bytes())?;
        written.push(path);
    }
    if let Some((bake, path)) = bake {
        if let Some(library) = &bake.library {
            let file = path.with_file_name(bake::LIBRARY);
            put(&file, library.as_bytes())?;
            written.push(file);
        }
        put(&path, bake.text.as_bytes())?;
        written.push(path);
    }
    if let Some((scene, path)) = scene {
        if let Some((file, text)) = &scene.library {
            put(file, text.as_bytes())?;
            written.push(file.clone());
        }
        for (file, bytes) in &scene.files {
            put(file, bytes)?;
            written.push(file.clone());
        }
        put(&path, scene.text.as_bytes())?;
        written.push(path);
    }
    Ok(written)
}
