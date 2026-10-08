use glam::DVec3;
use manfred_recipe::{Camera, Key, Patch, Projection, numbers};
use manfred_scene::Eye;
use toml_edit::{Item, Table, value};

use crate::edit::round;

const NEAR: f64 = 1e-5;
const STRAIGHT: f64 = 1e-6;

fn number(at: f64) -> Item {
    manfred_recipe::number(round(at, 6))
}

fn list(values: &[f64]) -> Item {
    let fine: Vec<f64> = values.iter().map(|at| round(*at, 6)).collect();
    numbers(&fine)
}

fn field(name: &str, key: &str) -> Vec<Key> {
    vec![
        Key::Table("camera".to_string()),
        Key::Object(name.to_string()),
        Key::Table(key.to_string()),
    ]
}

fn put(name: &str, key: &str, item: Item) -> Patch {
    Patch {
        path: field(name, key),
        value: Some(item),
    }
}

pub fn level_up(eye: &Eye) -> [f64; 3] {
    let forward = (DVec3::from_array(eye.target) - DVec3::from_array(eye.position))
        .try_normalize()
        .unwrap_or(DVec3::NEG_Z);
    let level = DVec3::Y - forward * DVec3::Y.dot(forward);
    if level.length() < STRAIGHT {
        eye.up
    } else {
        level.normalize().to_array()
    }
}

pub fn set_from_view(camera: &Camera, eye: &Eye) -> Vec<Patch> {
    let name = camera.name.as_str();
    let mut patches = vec![
        put(name, "position", list(&eye.position)),
        put(name, "target", list(&eye.target)),
        put(name, "up", list(&level_up(eye))),
    ];
    match (camera.kind, eye.ortho_height) {
        (Projection::Orthographic, Some(height)) => {
            patches.push(put(name, "ortho_height", number(height)));
        }
        (Projection::Perspective, _) if camera.focal_mm.is_none() => {
            patches.push(put(name, "fov_y_deg", number(eye.fov_y_deg)));
        }
        _ => {}
    }
    patches
}

pub fn unique_name<'a>(known: impl IntoIterator<Item = &'a str>) -> String {
    let known: Vec<&str> = known.into_iter().collect();
    (1..)
        .map(|n| format!("view{n}"))
        .find(|name| !known.contains(&name.as_str()))
        .unwrap_or_default()
}

pub fn new_camera(known: &[Camera], eye: &Eye) -> (String, Vec<Patch>) {
    let name = unique_name(known.iter().map(|camera| camera.name.as_str()));
    let mut table = Table::new();
    table.decor_mut().set_prefix("\n");
    table.insert("name", value(name.as_str()));
    match eye.ortho_height {
        Some(_) => table.insert("kind", value("orthographic")),
        None => table.insert("kind", value("perspective")),
    };
    table.insert("position", list(&eye.position));
    table.insert("target", list(&eye.target));
    table.insert("up", list(&level_up(eye)));
    match eye.ortho_height {
        Some(height) => table.insert("ortho_height", number(height)),
        None => table.insert("fov_y_deg", number(eye.fov_y_deg)),
    };
    if eye.shift != [0.0, 0.0] {
        table.insert("shift", list(&eye.shift));
    }
    let patch = if known.is_empty() {
        let mut tables = toml_edit::ArrayOfTables::new();
        tables.push(table);
        Patch {
            path: vec![Key::Table("camera".to_string())],
            value: Some(Item::ArrayOfTables(tables)),
        }
    } else {
        Patch {
            path: vec![Key::Table("camera".to_string()), Key::Object(name.clone())],
            value: Some(Item::Table(table)),
        }
    };
    (name, vec![patch])
}

pub fn delete(name: &str) -> Vec<Patch> {
    vec![Patch {
        path: vec![
            Key::Table("camera".to_string()),
            Key::Object(name.to_string()),
        ],
        value: None,
    }]
}

pub fn near(a: &Eye, b: &Eye) -> bool {
    let close = |a: &[f64], b: &[f64]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < NEAR);
    close(&a.position, &b.position)
        && close(&a.target, &b.target)
        && (close(&a.up, &b.up) || close(&level_up(a), &b.up) || close(&a.up, &level_up(b)))
        && (a.fov_y_deg - b.fov_y_deg).abs() < NEAR
        && a.ortho_height
            .zip(b.ortho_height)
            .is_none_or(|(a, b)| (a - b).abs() < NEAR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_recipe::{Loaded, apply, parse, write};

    const TEXT: &str = r#"manfred = 1
name = "pair"

[[object]]
name = "table"
kind = "box"
size = [1.0, 0.1, 1.0]
material = "oak"

[[camera]]
name = "front"
position = [0.0, 0.5, 2.0]
target = [0.0, 0.0, 0.0]
up = [0.0, 1.0, 0.0]
fov_y_deg = 30.0
shift = [0.0, -0.25]
focus_m = 2.0

[[camera]]
name = "lens"
position = [2.0, 0.5, 0.0]
target = [0.0, 0.0, 0.0]
focal_mm = 50.0
sensor_mm = 36.0

[[camera]]
name = "plan"
kind = "orthographic"
position = [0.0, 10.0, 0.0]
target = [0.0, 0.0, 0.0]
up = [0.0, 0.0, -1.0]
ortho_height = 2.2
"#;

    fn loaded(text: &str) -> Loaded {
        Loaded {
            recipe: parse(text).unwrap(),
            doc: text.parse().unwrap(),
            path: std::path::PathBuf::from("recipe.toml"),
        }
    }

    fn eye() -> Eye {
        Eye {
            position: [0.30000000000000004, 1.25, 3.0],
            target: [0.0, 0.1, -0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 41.5,
            shift: [0.0, 0.0],
            ortho_height: None,
        }
    }

    fn run(loaded: &mut Loaded, patches: Vec<Patch>) {
        for patch in patches {
            apply(loaded, &patch).unwrap();
        }
    }

    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a.iter().zip(b).map(|(a, b)| a * b).sum()
    }

    fn forward(eye: &Eye) -> [f64; 3] {
        let mut f = [0.0; 3];
        for (i, f) in f.iter_mut().enumerate() {
            *f = eye.target[i] - eye.position[i];
        }
        let length = dot(f, f).sqrt();
        f.map(|x| x / length)
    }

    #[test]
    fn a_level_up_is_the_worlds_up_perpendicular_to_the_view() {
        let rolled = Eye {
            up: [0.6, 0.5, -0.62],
            ..eye()
        };
        let up = level_up(&rolled);
        assert!(dot(up, forward(&rolled)).abs() < 1e-12, "{up:?}");
        assert!((dot(up, up) - 1.0).abs() < 1e-12);
        let f = forward(&rolled);
        let right = [-f[2], 0.0, f[0]];
        assert!(dot(up, right).abs() < 1e-12, "{up:?} {right:?}");
        assert!(up[1] > 0.0);
        let flat = Eye {
            position: [0.0, 0.0, 3.0],
            target: [0.0, 0.0, 0.0],
            up: [0.3, 0.8, 0.1],
            ..eye()
        };
        assert_eq!(level_up(&flat), [0.0, 1.0, 0.0]);
    }

    #[test]
    fn looking_straight_down_keeps_the_eyes_own_up() {
        let down = Eye {
            position: [0.0, 10.0, 0.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 0.0, -1.0],
            ..eye()
        };
        assert_eq!(level_up(&down), [0.0, 0.0, -1.0]);
        let mut file = loaded(TEXT);
        let plan = file.recipe.cameras[2].clone();
        run(&mut file, set_from_view(&plan, &down));
        assert_eq!(file.recipe.cameras[2].up, [0.0, 0.0, -1.0]);
    }

    #[test]
    fn a_tilted_up_and_its_level_one_are_the_same_view() {
        let rolled = Eye {
            up: [0.6, 0.5, -0.62],
            ..eye()
        };
        let level = Eye {
            up: level_up(&rolled),
            ..rolled
        };
        assert!(near(&rolled, &level));
        assert!(near(&level, &rolled));
        let other = Eye {
            up: [-0.6, 0.5, 0.62],
            ..rolled
        };
        assert!(!near(&rolled, &other));
    }

    #[test]
    fn set_from_view_writes_the_eye_and_keeps_the_rest() {
        let mut file = loaded(TEXT);
        let front = file.recipe.cameras[0].clone();
        let patches = set_from_view(&front, &eye());
        assert_eq!(patches.len(), 4);
        run(&mut file, patches);
        let after = write(&file);
        let expect = TEXT.replace(
            "position = [0.0, 0.5, 2.0]\ntarget = [0.0, 0.0, 0.0]\nup = [0.0, 1.0, 0.0]\nfov_y_deg = 30.0",
            "position = [0.3, 1.25, 3.0]\ntarget = [0.0, 0.1, 0.0]\nup = [-0.035462, 0.934339, -0.354617]\nfov_y_deg = 41.5",
        );
        assert_eq!(after, expect);
        let camera = &file.recipe.cameras[0];
        assert_eq!(camera.shift, [0.0, -0.25]);
        assert_eq!(camera.focus_m, Some(2.0));
        assert_eq!(camera.position, [0.3, 1.25, 3.0]);
    }

    #[test]
    fn set_from_view_keeps_a_focal_length_and_an_orthographic_kind() {
        let mut file = loaded(TEXT);
        let lens = file.recipe.cameras[1].clone();
        let patches = set_from_view(&lens, &eye());
        assert_eq!(patches.len(), 3);
        run(&mut file, patches);
        let camera = &file.recipe.cameras[1];
        assert_eq!(camera.focal_mm, Some(50.0));
        assert_eq!(camera.fov_y_deg, None);
        assert_eq!(camera.position, [0.3, 1.25, 3.0]);
        let plan = file.recipe.cameras[2].clone();
        let from = Eye {
            ortho_height: Some(1.5),
            ..eye()
        };
        run(&mut file, set_from_view(&plan, &from));
        let camera = &file.recipe.cameras[2];
        assert_eq!(camera.kind, Projection::Orthographic);
        assert_eq!(camera.ortho_height, Some(1.5));
        assert_eq!(camera.fov_y_deg, None);
    }

    #[test]
    fn set_from_view_undoes_with_the_inverse_patches() {
        let mut file = loaded(TEXT);
        let front = file.recipe.cameras[0].clone();
        let mut undo = Vec::new();
        for patch in set_from_view(&front, &eye()) {
            undo.push(apply(&mut file, &patch).unwrap());
        }
        for patch in undo.into_iter().rev() {
            apply(&mut file, &patch).unwrap();
        }
        assert_eq!(write(&file), TEXT);
    }

    #[test]
    fn a_new_camera_is_appended_under_a_unique_name() {
        let mut file = loaded(TEXT);
        let (name, patches) = new_camera(&file.recipe.cameras, &eye());
        assert_eq!(name, "view1");
        run(&mut file, patches);
        let (again, patches) = new_camera(&file.recipe.cameras, &eye());
        assert_eq!(again, "view2");
        run(&mut file, patches);
        let names: Vec<&str> = file
            .recipe
            .cameras
            .iter()
            .map(|camera| camera.name.as_str())
            .collect();
        assert_eq!(names, ["front", "lens", "plan", "view1", "view2"]);
        let made = &file.recipe.cameras[3];
        assert_eq!(made.kind, Projection::Perspective);
        assert_eq!(made.position, [0.3, 1.25, 3.0]);
        assert_eq!(made.fov_y_deg, Some(41.5));
        let tail = "\n[[camera]]\nname = \"view1\"\nkind = \"perspective\"\nposition = [0.3, 1.25, 3.0]\ntarget = [0.0, 0.1, 0.0]\nup = [-0.035462, 0.934339, -0.354617]\nfov_y_deg = 41.5\n";
        assert!(write(&file).contains(tail), "{}", write(&file));
    }

    #[test]
    fn a_free_name_skips_taken_ones() {
        assert_eq!(unique_name(["view1", "view3", "main"]), "view2");
        assert_eq!(unique_name([]), "view1");
        assert_eq!(unique_name(["view2"]), "view1");
    }

    #[test]
    fn a_new_orthographic_camera_keeps_its_height_and_shift() {
        let mut file = loaded(TEXT);
        let from = Eye {
            ortho_height: Some(1.8),
            shift: [0.1, 0.2],
            ..eye()
        };
        let (_, patches) = new_camera(&file.recipe.cameras, &from);
        run(&mut file, patches);
        let made = file.recipe.cameras.last().unwrap();
        assert_eq!(made.kind, Projection::Orthographic);
        assert_eq!(made.ortho_height, Some(1.8));
        assert_eq!(made.shift, [0.1, 0.2]);
    }

    #[test]
    fn a_first_camera_makes_the_table() {
        let text = "manfred = 1\nname = \"bare\"\n\n[[object]]\nname = \"a\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\nmaterial = \"m\"\n";
        let mut file = loaded(text);
        let (name, patches) = new_camera(&file.recipe.cameras, &eye());
        run(&mut file, patches);
        assert_eq!(file.recipe.cameras.len(), 1);
        assert_eq!(file.recipe.cameras[0].name, name);
    }

    #[test]
    fn delete_removes_one_camera_and_undoes() {
        let mut file = loaded(TEXT);
        let mut undo = Vec::new();
        for patch in delete("lens") {
            undo.push(apply(&mut file, &patch).unwrap());
        }
        let names: Vec<&str> = file
            .recipe
            .cameras
            .iter()
            .map(|camera| camera.name.as_str())
            .collect();
        assert_eq!(names, ["front", "plan"]);
        for patch in undo {
            apply(&mut file, &patch).unwrap();
        }
        assert_eq!(write(&file), TEXT);
    }

    #[test]
    fn the_table_changes_only_the_four_keys_of_the_camera() {
        let table = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/golden/table/table.toml");
        let mut file = manfred_recipe::load(&table).unwrap();
        let before = write(&file);
        let main = file.recipe.cameras[0].clone();
        let home = Eye::from_camera(&main, 1.6);
        let turned = crate::orbit::turn(&home, [90.0, 30.0]);
        run(&mut file, set_from_view(&main, &turned));
        let after = write(&file);
        let changed: Vec<(&str, &str)> = before
            .lines()
            .zip(after.lines())
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(before.lines().count(), after.lines().count());
        assert_eq!(changed.len(), 2, "{changed:?}");
        assert!(changed[0].1.starts_with("position = ["));
        assert!(changed[1].1.starts_with("up = ["));
        let out = crate::scratch("cameras-table").with_file_name("table-after.toml");
        std::fs::write(out, after).unwrap();
    }

    #[test]
    fn near_tells_a_saved_eye_from_a_moved_one() {
        let eye = eye();
        let saved = Eye {
            position: [0.3, 1.25, 3.0],
            ..eye
        };
        assert!(near(&eye, &saved));
        let moved = Eye {
            position: [0.4, 1.25, 3.0],
            ..eye
        };
        assert!(!near(&eye, &moved));
    }
}
