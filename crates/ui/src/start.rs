use std::io;
use std::path::{Path, PathBuf};

const RECIPE: &str = include_str!("../../../brand/start.toml");
const MESHES: [(&str, &[u8]); 2] = [
    (
        "start/badge.glb",
        include_bytes!("../../../brand/start/badge.glb"),
    ),
    (
        "start/wordmark.glb",
        include_bytes!("../../../brand/start/wordmark.glb"),
    ),
];

pub const MARK: &str = "manfred";
const SHIPPED_VERSION: &str = "text = \"vX.Y.Z\"";

pub fn start_text() -> String {
    RECIPE.replacen(
        SHIPPED_VERSION,
        &format!("text = \"v{}\"", env!("CARGO_PKG_VERSION")),
        1,
    )
}

pub fn start_recipe(state: &Path) -> io::Result<PathBuf> {
    let folder = state.join("start");
    std::fs::create_dir_all(folder.join("start"))?;
    for (path, bytes) in MESHES {
        std::fs::write(folder.join(path), bytes)?;
    }
    let recipe = folder.join("start.toml");
    std::fs::write(&recipe, start_text())?;
    Ok(recipe)
}

pub fn start_recipe_here() -> io::Result<PathBuf> {
    start_recipe(&crate::open::state_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{DMat4, DVec3};
    use manfred_build::Built;
    use manfred_recipe::{Kind, Projection, Recipe};

    const VERSION_TEXT: &str = "version";

    fn state(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tmp/start-tests")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn brand() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../brand")
    }

    fn opened(name: &str) -> (Recipe, Built) {
        let loaded = manfred_recipe::load(&start_recipe(&state(name)).unwrap()).unwrap();
        let built = manfred_build::build(&loaded.recipe, loaded.path.parent().unwrap()).unwrap();
        (loaded.recipe, built)
    }

    fn world(recipe: &Recipe, name: &str) -> DMat4 {
        let index = recipe.objects.iter().position(|o| o.name == name).unwrap();
        manfred_build::world(recipe, index).unwrap()
    }

    fn under(recipe: &Recipe, object: usize, group: &str) -> bool {
        let mut name = recipe.objects[object].name.clone();
        loop {
            if name == group {
                return true;
            }
            match recipe.objects.iter().find(|o| o.name == name) {
                Some(o) if o.parent.is_some() => name = o.parent.clone().unwrap(),
                _ => return false,
            }
        }
    }

    fn points<'a>(
        recipe: &'a Recipe,
        built: &'a Built,
        group: &'a str,
    ) -> impl Iterator<Item = DVec3> + 'a {
        built
            .parts
            .iter()
            .filter(move |p| !p.hidden && under(recipe, p.object, group))
            .flat_map(|part| {
                part.node.mesh.positions.iter().map(|p| {
                    part.node
                        .transform
                        .transform_point3(DVec3::from_array(p.map(f64::from)))
                })
            })
    }

    fn bounds(recipe: &Recipe, built: &Built, group: &str) -> (DVec3, DVec3) {
        points(recipe, built, group)
            .fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(min, max), p| {
                (min.min(p), max.max(p))
            })
    }

    fn front(recipe: &Recipe, built: &Built, group: &str) -> f64 {
        let mut planes: std::collections::BTreeMap<i64, f64> = Default::default();
        for part in built
            .parts
            .iter()
            .filter(|p| !p.hidden && under(recipe, p.object, group))
        {
            let mesh = &part.node.mesh;
            let world = |index: u32| {
                part.node.transform.transform_point3(DVec3::from_array(
                    mesh.positions[index as usize].map(f64::from),
                ))
            };
            for corners in mesh.indices.chunks_exact(3) {
                let [a, b, c] = [corners[0], corners[1], corners[2]].map(world);
                let normal = (b - a).cross(c - a);
                if normal.normalize_or_zero().z > 0.999 {
                    let z = ((a.z + b.z + c.z) / 3.0 * 1e5).round() as i64;
                    *planes.entry(z).or_default() += normal.length() / 2.0;
                }
            }
        }
        planes
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(f64::NAN, |(z, _)| z as f64 / 1e5)
    }

    #[test]
    fn the_outliner_lists_the_ground_and_the_mark_with_its_two_meshes() {
        let dir = state("reads");
        let path = start_recipe(&dir).unwrap();
        assert_eq!(path, dir.join("start/start.toml"));
        let loaded = manfred_recipe::load(&path).unwrap();
        let document = crate::document::Document::from_recipe(&loaded.recipe, "start.toml");
        let rows: Vec<(usize, &str)> = document
            .nodes
            .iter()
            .map(|node| (node.depth, node.label.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                (0, "ground"),
                (0, MARK),
                (1, "badge"),
                (1, "wordmark"),
                (1, VERSION_TEXT)
            ]
        );
        assert_eq!(loaded.recipe.cameras.len(), 1);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("[[use]]") && !text.contains("include"));
    }

    #[test]
    fn the_badge_and_the_wordmark_are_meshes_of_the_shipped_glbs_in_their_materials() {
        let (recipe, built) = opened("meshes");
        for (name, file) in [
            ("badge", "start/badge.glb"),
            ("wordmark", "start/wordmark.glb"),
        ] {
            let object = recipe.objects.iter().find(|o| o.name == name).unwrap();
            assert!(
                matches!(&object.kind, Kind::Mesh { path, node: None } if path == file),
                "{name}: {:?}",
                object.kind
            );
            assert_eq!(object.parent.as_deref(), Some(MARK));
            assert!(object.material.is_none() && object.materials.is_empty());
        }
        let slots = |name: &str| -> Vec<String> {
            let index = recipe.objects.iter().position(|o| o.name == name).unwrap();
            let part = built.parts.iter().find(|p| p.object == index).unwrap();
            let mut names: Vec<String> = Vec::new();
            for slot in &part.node.mesh.slots {
                if !names.contains(&slot.material) {
                    names.push(slot.material.clone());
                }
            }
            names
        };
        assert_eq!(slots("badge"), ["blueprint", "amber"]);
        assert_eq!(slots("wordmark"), ["amber"]);
        let shown: Vec<&str> = built
            .parts
            .iter()
            .filter(|p| !p.hidden && !p.node.mesh.positions.is_empty())
            .map(|p| recipe.objects[p.object].name.as_str())
            .collect();
        assert_eq!(shown, ["ground", "badge", "wordmark"]);
    }

    #[test]
    fn the_shipped_glbs_rebuild_from_their_recipes_byte_for_byte() {
        for (name, bytes) in MESHES {
            let recipe_path = brand().join(name).with_extension("toml");
            let loaded = manfred_recipe::load(&recipe_path).unwrap();
            assert!(!loaded.recipe.output.hierarchy, "{name}");
            let built =
                manfred_build::build(&loaded.recipe, recipe_path.parent().unwrap()).unwrap();
            assert!(
                built.glb() == bytes,
                "{name} differs from its recipe's build"
            );
            assert_eq!(
                loaded.recipe.output.glb.as_deref(),
                Path::new(name).file_name().and_then(|f| f.to_str())
            );
        }
    }

    #[test]
    fn the_badge_is_the_part_file_s_badge_standing_up() {
        let loaded = manfred_recipe::load(&brand().join("start/badge.toml")).unwrap();
        let recipe = &loaded.recipe;
        let part = std::fs::read_to_string(brand().join("parts/c.toml")).unwrap();
        let theirs: Vec<&str> = part
            .lines()
            .filter_map(|line| line.strip_prefix("name = \""))
            .filter_map(|rest| rest.strip_suffix('"'))
            .collect();
        let names: Vec<&str> = recipe.objects.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, theirs);
        assert_eq!(recipe.uses.len(), 1);
        let face = world(recipe, "c").transform_vector3(DVec3::Y);
        assert!(face.normalize().dot(DVec3::Z) > 0.999, "{face}");
    }

    #[test]
    fn the_badge_and_the_wordmark_face_forward_on_one_plane() {
        let (recipe, built) = opened("plane");
        let badge = front(&recipe, &built, "badge");
        let word = front(&recipe, &built, "wordmark");
        assert!(badge.is_finite() && word.is_finite(), "{badge} {word}");
        assert!(
            (badge - word).abs() < 1e-4,
            "badge {badge}, wordmark {word}"
        );
    }

    #[test]
    fn every_part_of_the_mark_hangs_from_the_one_parent() {
        let (recipe, _) = opened("parent");
        let mark = recipe.objects.iter().find(|o| o.name == MARK).unwrap();
        assert_eq!(mark.parent, None);
        assert!(mark.hidden);
        for (index, object) in recipe.objects.iter().enumerate() {
            assert_eq!(
                under(&recipe, index, MARK),
                object.name != "ground",
                "{}",
                object.name
            );
        }
    }

    #[test]
    fn the_mark_stands_on_the_floor_with_its_origin_at_its_centre() {
        let (recipe, built) = opened("floor");
        let origin = world(&recipe, MARK).transform_point3(DVec3::ZERO);
        assert!(origin.x.abs() < 1e-9 && origin.z.abs() < 1e-9, "{origin}");
        for group in ["badge", "wordmark"] {
            let (min, _) = bounds(&recipe, &built, group);
            assert!(min.y.abs() < 2e-4, "{group}: {min}");
        }
        let (min, max) = bounds(&recipe, &built, MARK);
        let centre = (min + max) / 2.0;
        assert!((centre - origin).length() < 1e-4, "{centre} {origin}");
        let solids = crate::viewport::solids(&recipe, &built);
        let poses = crate::gizmo::Poses::of(&recipe).centred(&solids);
        let mark = recipe.objects.iter().position(|o| o.name == MARK).unwrap();
        assert_eq!(poses.get(mark).unwrap().pivot, DVec3::ZERO);
    }

    #[test]
    fn the_camera_looks_down_at_a_three_quarter_angle_in_perspective() {
        let (recipe, _) = opened("camera");
        let camera = &recipe.cameras[0];
        assert_eq!(camera.kind, Projection::Perspective);
        let look =
            (DVec3::from_array(camera.target) - DVec3::from_array(camera.position)).normalize();
        assert!(look.y < -0.2 && look.y > -0.6, "{look}");
        let level = DVec3::new(look.x, 0.0, look.z).normalize();
        let off_axis = level.x.abs().min(level.z.abs());
        assert!(off_axis > 0.3, "{level}");
    }

    #[test]
    fn three_files_are_written_and_the_copy_starts_fresh_each_time() {
        let dir = state("fresh");
        let path = start_recipe(&dir).unwrap();
        let folder = dir.join("start");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), start_text());
        for (name, bytes) in MESHES {
            assert!(std::fs::read(folder.join(name)).unwrap() == bytes, "{name}");
        }
        assert!(!folder.join("parts").exists());
        std::fs::write(&path, "manfred = 1\nname = \"edited\"\n").unwrap();
        std::fs::write(folder.join(MESHES[0].0), b"not a glb").unwrap();
        std::fs::remove_file(folder.join(MESHES[1].0)).unwrap();
        start_recipe(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), start_text());
        for (name, bytes) in MESHES {
            assert!(std::fs::read(folder.join(name)).unwrap() == bytes, "{name}");
        }
    }

    #[test]
    fn the_copy_carries_the_running_version_at_the_wordmark_s_baseline() {
        let (recipe, built) = opened("version");
        assert!(RECIPE.matches(SHIPPED_VERSION).count() == 1);
        assert!(!start_text().contains(SHIPPED_VERSION));
        let object = recipe
            .objects
            .iter()
            .find(|o| o.name == VERSION_TEXT)
            .unwrap();
        assert_eq!(object.parent.as_deref(), Some(MARK));
        let Kind::Text { spans, .. } = &object.kind else {
            panic!("{:?}", object.kind)
        };
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, format!("v{}", env!("CARGO_PKG_VERSION")));
        assert_eq!(
            crate::shell::status_version(),
            format!("manfred {}", spans[0].text)
        );
        let (min, max) = bounds(&recipe, &built, "wordmark");
        let at = world(&recipe, VERSION_TEXT).transform_point3(DVec3::ZERO);
        assert!(at.x > max.x && at.x - max.x < 0.02, "{at} {max}");
        let baseline = at.y + (1.25 - 1.02 + 0.30) / 2.0 * spans[0].size;
        assert!((baseline - min.y).abs() < 1e-4, "{baseline} {min}");
        assert!(
            (at.z - front(&recipe, &built, "wordmark")).abs() < 1e-6,
            "{at}"
        );
    }
}
