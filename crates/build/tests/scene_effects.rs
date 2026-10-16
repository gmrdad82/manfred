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

#[test]
fn a_timeline_scene_is_one_animated_object_whose_lights_and_particles_ride_its_nodes() {
    let root = project(
        "effects-timeline",
        &[(
            "[[camera]]\nname = \"chase\"\nposition = [0.0, 2.0, -8.0]\ntarget = [0.0, 0.5, 0.0]\nfov_y_deg = 50.0\n",
            "[timeline]\nfps = 30\nframes = 6\nsize = [1080, 1920]\n\n[[route]]\nname = \"main\"\npoints = [[0.0, 0.0, 0.0], [0.0, 0.0, 20.0], [6.0, 0.0, 40.0]]\n\n[[follow]]\nobject = \"car\"\nroute = \"main\"\nspeed = [[0.0, 10.0]]\n\n[[camera]]\nname = \"chase\"\nfov_y_deg = 50.0\nfstop = 2.8\n\n[camera.rig]\nfollow = \"car\"\nlag = 0.25\nfocus = \"follow\"\n",
        )],
    );
    emit(&root, &[]).unwrap();
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scene/format1");
    let made = std::fs::read_to_string(root.join("scenes/street.scene.toml")).unwrap();
    let golden = here.join("timeline.scene.toml");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &made).unwrap();
    }
    let want = std::fs::read_to_string(&golden).unwrap();
    assert!(
        made == want,
        "the timeline scene differs from its format 1 golden; UPDATE_GOLDEN=1 regenerates it\n{made}"
    );
    let diagnostics = Project::open(&root).unwrap().check();
    let lines: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
    assert!(lines.is_empty(), "{}", lines.join("\n"));
}

#[test]
fn a_timeline_car_writes_its_lights_first_riding_its_body_and_pfx_scene_finds_nothing_wrong() {
    let root = project(
        "effects-car-lights",
        &[
            (
                "kind = \"box\"\nat = [0.0, 0.5, 0.0]\nsize = [1.8, 0.8, 4.2]\nmaterial = \"paint\"\n",
                "kind = \"generator\"\ngenerator = \"manfred/car\"\nparams = { style = \"sports\", seed = 1 }\n",
            ),
            (
                "[[camera]]\nname = \"chase\"\nposition = [0.0, 2.0, -8.0]\ntarget = [0.0, 0.5, 0.0]\nfov_y_deg = 50.0\n",
                "[timeline]\nfps = 30\nframes = 6\nsize = [1080, 1920]\n\n[[route]]\nname = \"main\"\npoints = [[0.0, 0.0, 0.0], [0.0, 0.0, 20.0], [6.0, 0.0, 40.0]]\n\n[[follow]]\nobject = \"car\"\nroute = \"main\"\nspeed = [[0.0, 10.0]]\n\n[[camera]]\nname = \"chase\"\nfov_y_deg = 50.0\n\n[camera.rig]\nfollow = \"car\"\n",
            ),
        ],
    );
    emit(&root, &[]).unwrap();
    let made = std::fs::read_to_string(root.join("scenes/street.scene.toml")).unwrap();
    let scene: manfred_recipe::toml_edit::DocumentMut = made.parse().unwrap();
    let lights = scene["light"].as_array_of_tables().unwrap();
    let names: Vec<&str> = lights.iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "lamp",
            "headlight",
            "car.headlight_l",
            "car.headlight_r",
            "car.taillight_l",
            "car.taillight_r"
        ]
    );
    let riding: Vec<_> = lights.iter().skip(2).collect();
    for light in &riding {
        assert_eq!(light["parent"].as_str(), Some("street"), "{light}");
        assert_eq!(light["node"].as_str(), Some("car.body"), "{light}");
        assert_eq!(light["kind"].as_str(), Some("spot"), "{light}");
        assert!(
            light.contains_key("at") && !light.contains_key("position"),
            "{light}"
        );
    }
    let z = |light: &manfred_recipe::toml_edit::Table| {
        light["at"]
            .as_array()
            .unwrap()
            .get(2)
            .unwrap()
            .as_float()
            .unwrap()
    };
    assert!(z(riding[0]) > 2.0 && z(riding[2]) < -2.0);
    let diagnostics = Project::open(&root).unwrap().check();
    let lines: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
    assert!(lines.is_empty(), "{}", lines.join("\n"));
}

#[test]
fn the_look_keys_reach_the_lamps_the_road_the_sky_and_the_haze_through_set() {
    let root = project(
        "effects-looks",
        &[
            (
                "kind = \"box\"\nat = [0.0, 0.5, 0.0]\nsize = [1.8, 0.8, 4.2]\nmaterial = \"paint\"\n",
                "kind = \"generator\"\ngenerator = \"manfred/car\"\nparams = { seed = 1, head_emission = [8.0, 7.0, 6.0], tail_emission = [4.0, 0.1, 0.0], head_light = { intensity = 120.0, cone = [10.0, 30.0], range = 60.0, color = [1.0, 1.0, 1.0] }, tail_light = { intensity = 9.0 } }\n\n[[route]]\nname = \"main\"\npoints = [[0.0, 0.0, 0.0], [0.0, 0.0, 60.0]]\n\n[[object]]\nname = \"main\"\nkind = \"generator\"\ngenerator = \"manfred/road\"\nparams = { route = \"main\", asphalt = { base = [0.03, 0.03, 0.035], roughness = 0.3, wet = { amount = 0.7, puddles = 0.3, puddle_scale = 3.0, seed = 2 } }, concrete = { roughness = 0.7 } }\n",
            ),
            (
                "[look]\n",
                "[sky]\nkind = \"analytic\"\nturbidity = 6.0\nground_albedo = [0.1, 0.1, 0.1]\n\n[look]\n",
            ),
            (
                "color = [0.95, 0.8, 0.6]\n",
                "color = [0.95, 0.8, 0.6]\nfog = 0.02\nsmoke = 0.5\nmist = 0.01\nfloor = 0.1\nphase = 0.6\nback = 0.4\ngold = [1.0, 0.9, 0.7]\nambient = [0.05, 0.04, 0.03]\nunmapped = 0.1\nseed = 4\n",
            ),
        ],
    );
    emit(
        &root,
        &[
            "object.car.params.head_light.intensity=150".into(),
            "object.car.params.head_emission=[9.0, 8.0, 7.0]".into(),
            "object.main.params.asphalt.wet.amount=0.9".into(),
            "sky.turbidity=7".into(),
            "haze.phase=0.5".into(),
        ],
    )
    .unwrap();
    let made = std::fs::read_to_string(root.join("scenes/street.scene.toml")).unwrap();
    let scene: manfred_recipe::toml_edit::DocumentMut = made.parse().unwrap();
    assert_eq!(scene["sky"]["turbidity"].as_float(), Some(7.0), "{made}");
    assert!(
        scene["sky"]
            .as_table()
            .unwrap()
            .contains_key("ground_albedo")
    );
    let haze = scene["haze"].as_table().unwrap();
    assert_eq!(haze["phase"].as_float(), Some(0.5));
    for key in [
        "fog", "smoke", "mist", "floor", "back", "gold", "ambient", "unmapped", "seed",
    ] {
        assert!(haze.contains_key(key), "{key}: {made}");
    }
    let lights = scene["light"].as_array_of_tables().unwrap();
    let light = |name: &str| {
        lights
            .iter()
            .find(|l| l["name"].as_str() == Some(name))
            .unwrap()
    };
    let head = light("car.headlight_l");
    assert_eq!(head["intensity"].as_float(), Some(150.0));
    assert_eq!(head["range"].as_float(), Some(60.0));
    assert_eq!(head["cone"].to_string().trim(), "[10.0, 30.0]");
    let tail = light("car.taillight_r");
    assert_eq!(tail["intensity"].as_float(), Some(9.0));
    assert_eq!(tail["range"].as_float(), Some(6.0));
    let text = std::fs::read_to_string(root.join("scenes/street.materials.toml")).unwrap();
    let library: manfred_recipe::toml_edit::DocumentMut = text.parse().unwrap();
    let materials = library["materials"].as_table().unwrap();
    let named = |prefix: &str| {
        materials
            .iter()
            .find(|(name, _)| name.starts_with(prefix))
            .map(|(name, item)| (name.to_string(), item.clone()))
            .unwrap_or_else(|| panic!("{prefix}: {text}"))
    };
    let (asphalt, item) = named("road_asphalt_");
    assert_eq!(item["wet"]["amount"].as_float(), Some(0.9), "{text}");
    assert_eq!(item["wet"]["seed"].as_integer(), Some(2), "{text}");
    assert_eq!(item["roughness"].as_float(), Some(0.3), "{text}");
    assert!(!materials.contains_key("road_asphalt"), "{asphalt}: {text}");
    assert!(materials.contains_key("road_paint"), "{text}");
    named("road_concrete_");
    let (_, head) = named("car_head_");
    assert_eq!(
        head["emission"].to_string().trim(),
        "[9.0, 8.0, 7.0]",
        "{text}"
    );
    named("car_tail_");
    let diagnostics = Project::open(&root).unwrap().check();
    let lines: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
    assert!(lines.is_empty(), "{}", lines.join("\n"));
}

#[test]
fn keyed_values_reach_the_scene_s_keys_tables_and_pfx_scene_finds_nothing_wrong() {
    let root = project(
        "effects-keys",
        &[
            (
                "sound = \"street.sound.toml\"\n",
                "sound = \"street.sound.toml\"\n\n[timeline]\nfps = 10\nframes = 10\n",
            ),
            (
                "intensity = 20.0\n",
                "intensity = 20.0\nkeys = { intensity = [[0.0, 20.0], [1.0, 40.0]], color = [[0.0, [1.0, 0.5, 0.2]], [1.0, [1.0, 1.0, 1.0]]] }\ninterp = { color = \"step\" }\n",
            ),
            (
                "range = 25.0\n",
                "range = 25.0\nkeys = { cone = [[0.0, [10.0, 20.0]], [1.0, [15.0, 30.0]]] }\n",
            ),
            (
                "fov_y_deg = 50.0\n",
                "fov_y_deg = 50.0\nkeys = { fov_y_deg = [[0.0, 50.0], [1.0, 35.0]] }\n",
            ),
            (
                "blend = \"add\"\n",
                "blend = \"add\"\nkeys = { rate = [[0.0, 0.0], [1.0, 50.0]], alpha = [[0.0, 1.0], [1.0, 0.5]] }\n",
            ),
            (
                "lut_amount = 0.75\n",
                "lut_amount = 0.75\nkeys = { exposure = [[0.0, 1.0], [1.0, 1.4]] }\n",
            ),
            (
                "color = [0.95, 0.8, 0.6]\n",
                "color = [0.95, 0.8, 0.6]\nkeys = { amount = [[0.0, 0.3], [1.0, 0.1]] }\n",
            ),
            (
                "roughness = 0.3\n",
                "roughness = 0.3\n\n[sun]\nmodel = \"daylight\"\nhour = 9.0\nlatitude = 52.0\nheading = 20.0\nkeys = { hour = [[0.0, 9.0], [1.0, 10.0]] }\n",
            ),
        ],
    );
    emit(&root, &[]).unwrap();
    let made = std::fs::read_to_string(root.join("scenes/street.scene.toml")).unwrap();
    let scene: manfred_recipe::toml_edit::DocumentMut = made.parse().unwrap();
    let keys = |table: &dyn manfred_recipe::toml_edit::TableLike, name: &str| -> usize {
        table
            .get("keys")
            .and_then(|k| k.get(name))
            .and_then(|k| k.as_array())
            .unwrap_or_else(|| panic!("no keys.{name} in {made}"))
            .len()
    };
    let table = |key: &str| scene[key].as_table_like().unwrap();
    let lights = scene["light"].as_array_of_tables().unwrap();
    assert_eq!(keys(lights.get(0).unwrap(), "intensity"), 2);
    assert_eq!(keys(lights.get(0).unwrap(), "color"), 3);
    assert_eq!(keys(lights.get(1).unwrap(), "cone"), 2);
    assert_eq!(keys(table("camera"), "fov"), 2);
    assert_eq!(keys(table("finish"), "exposure"), 2);
    assert_eq!(keys(table("haze"), "amount"), 2);
    assert_eq!(keys(table("sun"), "hour"), 2);
    let particles = scene["particles"].as_array_of_tables().unwrap();
    let sparks = particles.get(1).unwrap();
    assert_eq!((keys(sparks, "rate"), keys(sparks, "alpha")), (2, 2));
    let diagnostics = Project::open(&root).unwrap().check();
    let lines: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
    assert!(lines.is_empty(), "{}", lines.join("\n"));
}
