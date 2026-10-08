use manfred_recipe::{Camera, Projection, Recipe, SkyKind};

use crate::sample;

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub name: String,
    pub material: String,
    pub location: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub object: Option<usize>,
    pub label: String,
    pub depth: usize,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lamp {
    Sun,
    Daylight,
    Sky,
    Bulb,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub name: String,
    pub path: String,
    pub objects: Vec<Item>,
    pub nodes: Vec<Node>,
    pub cameras: Vec<(String, String)>,
    pub views: Vec<Camera>,
    pub lights: Vec<(Lamp, String, String)>,
    pub materials: Vec<String>,
    pub hour: Option<f32>,
    pub aspect: f32,
    pub patch: String,
    pub source: u64,
}

fn transform(index: usize) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let step = |n: usize, m: usize| ((index * n) % m) as f32;
    (
        [
            step(37, 23) * 0.06 - 0.66,
            step(11, 7) * 0.02 - 0.06,
            step(5, 9) * -0.002,
        ],
        [0.0, 0.0, step(13, 4) * 90.0],
        [1.0, 1.0, 1.0],
    )
}

fn first_seen<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for name in names {
        if !seen.iter().any(|known| known == name) {
            seen.push(name.to_string());
        }
    }
    seen
}

fn narrow(values: [f64; 3]) -> [f32; 3] {
    values.map(|value| value as f32)
}

fn camera_detail(camera: &Camera) -> String {
    let lens = match (camera.kind, camera.focal_mm, camera.fov_y_deg) {
        (Projection::Orthographic, _, _) => {
            format!("ortho  {:.2} m", camera.ortho_height.unwrap_or(0.0))
        }
        (Projection::Perspective, Some(focal), _) => format!("{focal:.0} mm"),
        (Projection::Perspective, None, Some(fov)) => format!("fov {fov:.1}°"),
        (Projection::Perspective, None, None) => String::new(),
    };
    if camera.shift == [0.0, 0.0] {
        lens
    } else {
        format!(
            "{lens}  shift {:.2} {:.2}",
            camera.shift[0], camera.shift[1]
        )
    }
}

fn sky_kind(kind: SkyKind) -> &'static str {
    match kind {
        SkyKind::Hdr => "hdr",
        SkyKind::Analytic => "analytic",
        SkyKind::Room => "room",
        SkyKind::Mix => "mix",
    }
}

fn lights(recipe: &Recipe) -> Vec<(Lamp, String, String)> {
    let mut lights = Vec::new();
    if let Some(sun) = &recipe.sun {
        lights.push((
            Lamp::Sun,
            "sun".to_string(),
            format!("{:.1} W/m²", sun.irradiance),
        ));
    }
    if let Some(daylight) = &recipe.daylight {
        lights.push((
            Lamp::Daylight,
            "daylight".to_string(),
            format!("lat {:.1}°  day {}", daylight.latitude, daylight.day),
        ));
    }
    if let Some(sky) = &recipe.sky {
        lights.push((
            Lamp::Sky,
            "sky".to_string(),
            match sky.rotation_deg {
                0.0 => format!("{}  ×{:.2}", sky_kind(sky.kind), sky.intensity),
                turn => format!("{}  ×{:.2}  {turn:.0}°", sky_kind(sky.kind), sky.intensity),
            },
        ));
    }
    for light in &recipe.lights {
        lights.push((
            Lamp::Bulb,
            light.name.clone(),
            format!("{:.2}", light.intensity),
        ));
    }
    lights
}

fn parents(names: &[String], above: &[Option<String>]) -> Vec<Option<usize>> {
    let index = |name: &str| names.iter().position(|known| known == name);
    let parents: Vec<Option<usize>> = above
        .iter()
        .map(|parent| parent.as_deref().and_then(index))
        .collect();
    parents
        .iter()
        .enumerate()
        .map(|(start, parent)| {
            let mut at = (*parent)?;
            for _ in 0..parents.len() {
                if at == start {
                    return None;
                }
                match parents[at] {
                    Some(next) => at = next,
                    None => return *parent,
                }
            }
            None
        })
        .collect()
}

pub fn tree(recipe: &Recipe) -> Vec<Node> {
    let names: Vec<String> = recipe.objects.iter().map(|o| o.name.clone()).collect();
    let above: Vec<Option<String>> = recipe.objects.iter().map(|o| o.parent.clone()).collect();
    tree_of(&names, &above)
}

pub fn tree_of(names: &[String], above: &[Option<String>]) -> Vec<Node> {
    let parents = parents(names, above);
    let mut nodes = Vec::new();
    let mut stack: Vec<(usize, Option<usize>, usize)> = parents
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, parent)| parent.is_none())
        .map(|(object, _)| (object, None, 0))
        .collect();
    while let Some((object, parent, depth)) = stack.pop() {
        let name = &names[object];
        let label = parent
            .and_then(|parent: usize| {
                let above = &names[nodes_object(&nodes, parent)];
                name.strip_prefix(&format!("{above}__"))
            })
            .unwrap_or(name)
            .to_string();
        let index = nodes.len();
        nodes.push(Node {
            object: Some(object),
            label,
            depth,
            parent,
            children: Vec::new(),
        });
        if let Some(parent) = parent {
            nodes[parent].children.push(index);
        }
        for child in (0..parents.len()).rev() {
            if parents[child] == Some(object) {
                stack.push((child, Some(index), depth + 1));
            }
        }
    }
    nodes
}

fn nodes_object(nodes: &[Node], node: usize) -> usize {
    nodes[node].object.unwrap_or(0)
}

impl Document {
    pub fn sample() -> Document {
        let mut nodes = Vec::new();
        for row in sample::tree() {
            let parent = nodes.len();
            nodes.push(Node {
                object: row.object,
                label: row.name.clone(),
                depth: 0,
                parent: None,
                children: Vec::new(),
            });
            for child in row.children {
                let name = sample::OBJECTS[child].0;
                let index = nodes.len();
                nodes.push(Node {
                    object: Some(child),
                    label: name
                        .split_once("__")
                        .map_or(name, |(_, part)| part)
                        .to_string(),
                    depth: 1,
                    parent: Some(parent),
                    children: Vec::new(),
                });
                nodes[parent].children.push(index);
            }
        }
        let objects = sample::OBJECTS
            .iter()
            .enumerate()
            .map(|(index, (name, material))| {
                let (location, rotation, scale) = transform(index);
                Item {
                    name: name.to_string(),
                    material: material.to_string(),
                    location,
                    rotation,
                    scale,
                }
            })
            .collect();
        let lamps = [Lamp::Sun, Lamp::Sky, Lamp::Bulb];
        Document {
            name: sample::RECIPE_NAME.to_string(),
            path: sample::RECIPE_PATH.to_string(),
            objects,
            nodes,
            cameras: sample::CAMERAS
                .iter()
                .map(|(name, detail)| (name.to_string(), detail.to_string()))
                .collect(),
            views: Vec::new(),
            lights: sample::LIGHTS
                .iter()
                .zip(lamps)
                .map(|((name, detail), lamp)| (lamp, name.to_string(), detail.to_string()))
                .collect(),
            materials: first_seen(sample::OBJECTS.iter().map(|(_, material)| *material)),
            hour: Some(14.5),
            aspect: crate::viewport::ASPECT,
            patch: sample::LAST_PATCH.to_string(),
            source: 0,
        }
    }

    pub fn empty(path: &str) -> Document {
        Document {
            name: std::path::Path::new(path)
                .file_stem()
                .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()),
            path: path.to_string(),
            objects: Vec::new(),
            nodes: Vec::new(),
            cameras: Vec::new(),
            views: Vec::new(),
            lights: Vec::new(),
            materials: Vec::new(),
            hour: None,
            aspect: crate::viewport::ASPECT,
            patch: "none".to_string(),
            source: 0,
        }
    }

    pub fn from_recipe(recipe: &Recipe, path: &str) -> Document {
        let objects = recipe
            .objects
            .iter()
            .map(|object| Item {
                name: object.name.clone(),
                material: match (&object.material, object.materials.is_empty()) {
                    (Some(material), _) => material.clone(),
                    (None, false) => object.materials.join(" "),
                    (None, true) => String::new(),
                },
                location: narrow(object.at),
                rotation: narrow(object.rotate),
                scale: narrow(object.scale),
            })
            .collect();
        let materials = first_seen(
            recipe
                .objects
                .iter()
                .flat_map(|object| {
                    object
                        .material
                        .iter()
                        .chain(&object.materials)
                        .map(String::as_str)
                })
                .chain(recipe.materials.keys().map(String::as_str)),
        );
        let aspect = match &recipe.plate {
            Some(plate) if plate.width > 0.0 && plate.height > 0.0 => {
                (plate.width / plate.height) as f32
            }
            _ => crate::viewport::ASPECT,
        };
        Document {
            name: recipe.name.clone(),
            path: path.to_string(),
            objects,
            nodes: tree(recipe),
            cameras: recipe
                .cameras
                .iter()
                .map(|camera| (camera.name.clone(), camera_detail(camera)))
                .collect(),
            views: recipe.cameras.clone(),
            lights: lights(recipe),
            materials,
            hour: recipe
                .daylight
                .as_ref()
                .map(|daylight| daylight.hour as f32),
            aspect,
            patch: "none".to_string(),
            source: crate::follow::hash(&recipe.source),
        }
    }

    pub fn node_of(&self, object: usize) -> Option<usize> {
        self.nodes
            .iter()
            .position(|node| node.object == Some(object))
    }

    pub fn object(&self, name: &str) -> Option<usize> {
        self.objects.iter().position(|item| item.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NESTED: &str = r#"
manfred = 1
name = "nest"

[[object]]
name = "table"
kind = "box"
size = [1.0, 0.1, 1.0]
material = "oak"

[[object]]
name = "lamp"
kind = "sphere"
radius = 0.1
parent = "table"
material = "brass"

[[object]]
name = "floor"
kind = "plane"
size = [4.0, 4.0]
material = "felt"

[[object]]
name = "lamp__shade"
kind = "disc"
radius = 0.05
parent = "lamp"
materials = ["card", "brass"]

[[object]]
name = "cup"
kind = "box"
size = [0.1, 0.1, 0.1]
parent = "table"

[[camera]]
name = "main"
position = [0.0, 1.0, 3.0]
target = [0.0, 0.0, 0.0]
focal_mm = 50.0
sensor_mm = 36.0
shift = [0.0, 0.04]

[[camera]]
name = "plan"
kind = "orthographic"
position = [0.0, 10.0, 0.0]
target = [0.0, 0.0, 0.0]
up = [0.0, 0.0, -1.0]
ortho_height = 2.2
"#;

    fn nested() -> Recipe {
        manfred_recipe::parse(NESTED).unwrap()
    }

    #[test]
    fn children_nest_under_their_parent_in_file_order() {
        let nodes = tree(&nested());
        let rows: Vec<(usize, &str)> = nodes
            .iter()
            .map(|node| (node.depth, node.label.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                (0, "table"),
                (1, "lamp"),
                (2, "shade"),
                (1, "cup"),
                (0, "floor")
            ]
        );
        assert_eq!(nodes[0].children, [1, 3]);
        assert_eq!(nodes[1].children, [2]);
        assert_eq!(nodes[2].parent, Some(1));
        assert_eq!(nodes[4].parent, None);
        let objects: Vec<Option<usize>> = nodes.iter().map(|node| node.object).collect();
        assert_eq!(objects, [Some(0), Some(1), Some(3), Some(4), Some(2)]);
    }

    #[test]
    fn a_parent_loop_or_a_missing_parent_stays_at_the_root() {
        let mut recipe = nested();
        recipe.objects[1].parent = Some("lamp__shade".to_string());
        recipe.objects[4].parent = Some("nobody".to_string());
        let nodes = tree(&recipe);
        let rows: Vec<(usize, &str)> = nodes
            .iter()
            .map(|node| (node.depth, node.label.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                (0, "table"),
                (0, "lamp"),
                (0, "floor"),
                (0, "lamp__shade"),
                (0, "cup")
            ]
        );
    }

    #[test]
    fn the_document_carries_materials_and_cameras() {
        let document = Document::from_recipe(&nested(), "scenes/nest.toml");
        assert_eq!(document.name, "nest");
        assert_eq!(document.objects[3].material, "card brass");
        assert_eq!(document.objects[4].material, "");
        assert_eq!(document.materials, ["oak", "brass", "felt", "card"]);
        assert_eq!(
            document.cameras,
            [
                ("main".to_string(), "50 mm  shift 0.00 0.04".to_string()),
                ("plan".to_string(), "ortho  2.20 m".to_string()),
            ]
        );
        assert_eq!(document.views.len(), 2);
    }

    #[test]
    fn the_lights_list_the_sun_sky_and_bulbs_with_their_values() {
        let text = format!(
            "{NESTED}\n[sun]\nmodel = \"authored\"\ntoward = [0.0, 1.0, 0.0]\nirradiance = 3.0\n\n[sky]\nkind = \"analytic\"\nintensity = 0.5\nrotation_deg = 90.0\n\n[[light]]\nname = \"lamp\"\nposition = [0.0, 1.0, 0.0]\nintensity = 20.0\n"
        );
        let recipe = manfred_recipe::parse(&text).unwrap();
        let document = Document::from_recipe(&recipe, "scenes/nest.toml");
        assert_eq!(
            document.lights,
            [
                (Lamp::Sun, "sun".to_string(), "3.0 W/m²".to_string()),
                (
                    Lamp::Sky,
                    "sky".to_string(),
                    "analytic  ×0.50  90°".to_string()
                ),
                (Lamp::Bulb, "lamp".to_string(), "20.00".to_string()),
            ]
        );
        let daylight = text.replace(
            "[sun]\nmodel = \"authored\"\ntoward = [0.0, 1.0, 0.0]\nirradiance = 3.0",
            "[sun]\nmodel = \"daylight\"\nhour = 9.0\nlatitude = 52.0\nheading = 0.0",
        );
        let recipe = manfred_recipe::parse(&daylight).unwrap();
        let document = Document::from_recipe(&recipe, "scenes/nest.toml");
        assert_eq!(document.lights[0].0, Lamp::Daylight);
    }

    #[test]
    fn the_sample_keeps_wave_one_rows() {
        let document = Document::sample();
        let knob = document
            .nodes
            .iter()
            .find(|node| node.label == "mv_knob")
            .unwrap();
        let labels: Vec<&str> = knob
            .children
            .iter()
            .map(|child| document.nodes[*child].label.as_str())
            .collect();
        assert_eq!(labels, ["face", "cap", "dot"]);
        assert_eq!(
            document
                .nodes
                .iter()
                .filter(|node| node.object.is_some())
                .count(),
            sample::OBJECTS.len()
        );
    }
}
