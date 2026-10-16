mod common;

use common::scratch;
use manfred_build::{Error, build, write};
use manfred_recipe::{apply_set, load, validate};
use pfx_scene::Project;
use std::path::{Path, PathBuf};

const RECIPE: &str = r##"manfred = 1
name = "street"
sound = "street.sound.toml"

[output]
glb = "street.glb"
scene = "street.scene.toml"

[[object]]
name = "road"
kind = "box"
size = [8.0, 0.1, 20.0]
material = "asphalt"

[[object]]
name = "car"
kind = "box"
at = [0.0, 0.5, 0.0]
size = [1.8, 0.8, 4.2]
material = "paint"

[[camera]]
name = "chase"
position = [0.0, 2.0, -8.0]
target = [0.0, 0.5, 0.0]
fov_y_deg = 50.0

[[light]]
name = "lamp"
position = [3.0, 4.0, 0.0]
intensity = 20.0

[[light]]
name = "headlight"
kind = "spot"
parent = "car"
at = [0.6, 0.1, 2.1]
direction = [0.0, -0.1, 1.0]
cone = [15.0, 30.0]
color = [1.0, 0.95, 0.85]
intensity = 40.0
range = 25.0

[[particles]]
name = "spray"
parent = "car"
at = [0.0, -0.4, -2.1]
rate = 120.0
rate_keys = [[0.0, 0.0], [1.0, 200.0]]
burst = 10
max = 400
lifetime = [0.4, 0.9]
speed = [2.0, 4.0]
direction = [0.0, 0.3, -1.0]
cone = 25.0
inherit = 0.5
gravity = [0.0, -9.8, 0.0]
drag = 0.8
size = [0.05, 0.3]
alpha = [[0.0, 0.6], [1.0, 0.0]]
colors = [[0.0, "#d8dde4"], [1.0, [0.5, 0.5, 0.55]]]
spin = [-90.0, 90.0]
stretch = 0.4
blend = "alpha"
lit = true
seed = 7

[[particles]]
name = "sparks"
at = [0.0, 0.0, 5.0]
color = "#ffb347"
blend = "add"

[look]
exposure = 1.1
lut = "grade.cube"
lut_amount = 0.75

[flare]
amount = 0.4
ghosts = 4
ghost_spacing = 0.3
halo = 0.2
halo_radius = 0.6
starburst = 0.5
threshold = 1.5

[motion_blur]
shutter = 0.5
max_px = 40.0
samples = 8

[haze]
amount = 0.3
height = 0.0
falloff = 0.15
reach = 120.0
shafts = 0.6
steps = 24
color = [0.95, 0.8, 0.6]

[materials.asphalt]
base = [0.05, 0.05, 0.05]
roughness = 0.9

[materials.asphalt.wet]
amount = 0.8
puddles = 0.4
puddle_scale = 2.5
seed = 3

[materials.paint]
base = [0.6, 0.02, 0.02]
roughness = 0.3
"##;

const CUBE: &str = "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";

fn project(name: &str, edit: &[(&str, &str)]) -> PathBuf {
    let root = scratch(name);
    std::fs::write(
        root.join("project.toml"),
        "format = 1\n\n[project]\nname = \"street\"\n",
    )
    .unwrap();
    let recipes = root.join("recipes");
    std::fs::create_dir_all(&recipes).unwrap();
    let mut text = RECIPE.to_string();
    for (from, to) in edit {
        assert!(text.contains(from), "{from}");
        text = text.replace(from, to);
    }
    std::fs::write(recipes.join("street.toml"), text).unwrap();
    std::fs::write(recipes.join("grade.cube"), CUBE).unwrap();
    std::fs::write(recipes.join("street.sound.toml"), "format = 1\n").unwrap();
    root
}

fn emit(root: &Path, sets: &[String]) -> Result<Vec<PathBuf>, Error> {
    let base = root.join("recipes");
    let r = load(&base.join("street.toml")).unwrap().recipe;
    let r = apply_set(&r, sets).unwrap();
    assert_eq!(validate(&r), []);
    let built = build(&r, &base)?;
    write(&built, &r, &base, Some(&root.join("scenes")))
}

#[test]
fn every_new_scene_key_reaches_its_format_1_golden_and_pfx_scene_finds_nothing_wrong() {
    let root = project("effects-golden", &[]);
    emit(&root, &[]).unwrap();
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scene/format1");
    for name in ["street.scene.toml", "street.materials.toml"] {
        let made = std::fs::read_to_string(root.join("scenes").join(name)).unwrap();
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::write(here.join(name), &made).unwrap();
        }
        let want = std::fs::read_to_string(here.join(name)).unwrap();
        assert!(
            made == want,
            "{name} differs from its format 1 golden; UPDATE_GOLDEN=1 regenerates it\n{made}"
        );
    }
    let diagnostics = Project::open(&root).unwrap().check();
    let lines: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
    assert!(lines.is_empty(), "{}", lines.join("\n"));
    let root = project("effects-set", &[]);
    emit(
        &root,
        &[
            "particles.spray.rate=60".into(),
            "materials.asphalt.wet.amount=0.2".into(),
        ],
    )
    .unwrap();
    let scene = std::fs::read_to_string(root.join("scenes/street.scene.toml")).unwrap();
    assert!(scene.contains("rate = 60.0"), "{scene}");
    let library = std::fs::read_to_string(root.join("scenes/street.materials.toml")).unwrap();
    assert!(library.contains("amount = 0.2"), "{library}");
}

#[test]
fn a_bad_particles_key_is_refused_naming_it() {
    for (from, to, key) in [
        ("rate = 120.0", "rate = -1.0", "particles[spray].rate"),
        (
            "blend = \"alpha\"",
            "blend = \"screen\"",
            "particles[spray].blend",
        ),
        (
            "lit = true",
            "lit = true\nglow = 1.0",
            "particles[spray].glow",
        ),
        (
            "alpha = [[0.0, 0.6], [1.0, 0.0]]",
            "alpha = [[0.5, 0.6], [0.2, 0.0]]",
            "particles[spray].alpha[1]",
        ),
    ] {
        let root = project("effects-bad-key", &[(from, to)]);
        let error = load(&root.join("recipes/street.toml")).unwrap_err();
        assert_eq!(error.path, key, "{error:?}");
    }
}

#[test]
fn a_parent_that_names_nothing_is_refused_at_build() {
    let root = project(
        "effects-no-parent",
        &[(
            "parent = \"car\"\nat = [0.0, -0.4",
            "parent = \"bus\"\nat = [0.0, -0.4",
        )],
    );
    let error = emit(&root, &[]).unwrap_err();
    assert_eq!(error.path, "particles[spray].parent");
    assert!(error.text.contains("\"bus\""), "{}", error.text);
}
