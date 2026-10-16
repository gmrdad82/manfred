use std::path::PathBuf;

use manfred_recipe::{
    Kind, Marker, add_patch, duplicate_patch, entries, load, parse, remove_patch, validate,
    write_checked,
};

const RECIPE: &str = r#"manfred = 1
name = "edit"

# the table, under everything
[[object]]
name = "table"
kind = "box"
size = [1.0, 0.1, 1.0]

# a cup on the table
[[object]]
name = "cup"
kind = "cylinder"
radius = 0.04
depth = 0.1
parent = "table"

# the only camera
[[camera]]
name = "main"
position = [0.0, 1.0, 3.0]
target = [0.0, 0.0, 0.0]

[timeline]
fps = 10
seconds = 2.0

# the moment it tips
[[marker]]
name = "tip"
t = 1.0
"#;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp/recipe-tests/entries")
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("edit.toml");
    std::fs::write(&path, RECIPE).unwrap();
    path
}

fn pairs(list: &[&str]) -> Vec<String> {
    list.iter().map(|p| p.to_string()).collect()
}

#[test]
fn add_duplicate_and_remove_write_the_file_keeping_its_comments() {
    let path = scratch("round-trip");
    let mut l = load(&path).unwrap();
    let patch = add_patch(
        &l,
        "object",
        "lamp",
        &pairs(&["kind=sphere", "radius=0.1", "at=[0,2,0]"]),
    )
    .unwrap();
    write_checked(&mut l, &[patch]).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    for comment in [
        "# the table, under everything",
        "# a cup on the table",
        "# the only camera",
        "# the moment it tips",
    ] {
        assert!(text.contains(comment), "{text}");
    }
    let lamp = text.find("name = \"lamp\"").unwrap();
    assert!(text.find("name = \"cup\"").unwrap() < lamp, "{text}");
    assert!(lamp < text.find("# the only camera").unwrap(), "{text}");
    let added = l.recipe.objects.iter().find(|o| o.name == "lamp").unwrap();
    assert!(matches!(added.kind, Kind::Sphere { radius, .. } if radius == 0.1));
    assert_eq!(added.at, [0.0, 2.0, 0.0]);

    let patch = duplicate_patch(&l, "object", "cup", "mug", &pairs(&["radius=0.05"])).unwrap();
    write_checked(&mut l, &[patch]).unwrap();
    let mug = l.recipe.objects.iter().find(|o| o.name == "mug").unwrap();
    assert_eq!(mug.parent.as_deref(), Some("table"));
    assert!(matches!(mug.kind, Kind::Cylinder { radius, .. } if radius == 0.05));
    let names: Vec<String> = entries(&l, Some("object"))
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["table", "cup", "lamp", "mug"]);
    assert!(add_patch(&l, "object", "mug", &[]).is_err());
    assert!(add_patch(&l, "nope", "x", &[]).is_err());

    for name in ["mug", "lamp"] {
        let patch = remove_patch(&l, "object", name).unwrap();
        write_checked(&mut l, &[patch]).unwrap();
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), RECIPE);
}

#[test]
fn a_remove_names_what_refers_to_it_and_a_write_that_breaks_the_check_is_undone() {
    let path = scratch("refused");
    let mut l = load(&path).unwrap();
    let refused = remove_patch(&l, "object", "table").unwrap_err();
    assert!(
        refused.text.contains("object[cup].parent"),
        "{}",
        refused.text
    );
    let late = add_patch(&l, "marker", "late", &pairs(&["t=9"])).unwrap();
    let undone = write_checked(&mut l, &[late]).unwrap_err();
    assert_eq!(undone.path, "marker[late].t");
    assert!(
        undone.text.ends_with("the recipe is left as it was"),
        "{}",
        undone.text
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), RECIPE);
    assert!(l.recipe.markers.iter().all(|m| m.name != "late"));
}

#[test]
fn markers_name_times_within_the_timeline() {
    let r = parse(RECIPE).unwrap();
    assert_eq!(
        r.markers,
        [Marker {
            name: "tip".into(),
            t: 1.0
        }]
    );
    let twice = format!("{RECIPE}\n[[marker]]\nname = \"tip\"\nt = 3.0\n");
    let problems = validate(&parse(&twice).unwrap());
    let paths: Vec<&str> = problems.iter().map(|p| p.path.as_str()).collect();
    assert!(paths.contains(&"marker[tip].name"), "{paths:?}");
    assert!(paths.contains(&"marker[tip].t"), "{paths:?}");
    let clockless = RECIPE.replace("[timeline]\nfps = 10\nseconds = 2.0\n", "");
    let problems = validate(&parse(&clockless).unwrap());
    assert!(
        problems.iter().any(|p| p.path == "marker[tip].t"),
        "{problems:?}"
    );
}
