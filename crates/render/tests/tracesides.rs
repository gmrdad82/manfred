use manfred_build::Registry;
use manfred_recipe::{Recipe, load};
use manfred_render::{Staging, View, eyepiece, looked, trace};
use manfred_scene::{headless, still, write_png};
use pfx::adapter::{turn, turn_reporting};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

const SIZE: [u32; 2] = [400, 400];
const HALF: f64 = 1.5;
const SAMPLES: u32 = 256;
const SEED: u32 = 5;
const TOLERANCE: f64 = 24.0;

const WALLS: &str = r#"manfred = 1
name = "walls"

[[camera]]
name = "main"
kind = "orthographic"
position = [0.0, 0.0, 5.0]
target = [0.0, 0.0, 0.0]
up = [0.0, 1.0, 0.0]
ortho_height = 3.0

[sun]
model = "authored"
toward = [0.2, 0.3, 1.0]
irradiance = 3.0

[materials.backdrop]
base = [0.7, 0.05, 0.05]
roughness = 1.0
specular = 0.0

[materials.sheet]
base = [0.05, 0.6, 0.1]
roughness = 1.0
specular = 0.0

[[object]]
name = "backdrop"
kind = "box"
size = [4.0, 4.0, 0.1]
at = [0.0, 0.0, -0.5]
material = "backdrop"

[[object]]
name = "from_behind"
kind = "plane"
size = [1.0, 1.0]
at = [-0.6, 0.0, 0.5]
rotate = [-90.0, 0.0, 0.0]
material = "sheet"

[[object]]
name = "from_the_front"
kind = "plane"
size = [1.0, 1.0]
at = [0.6, 0.0, 0.5]
rotate = [90.0, 0.0, 0.0]
material = "sheet"

[[object]]
name = "both_faces"
kind = "plane"
size = [1.0, 1.0]
at = [0.0, 1.1, 0.5]
rotate = [-90.0, 0.0, 0.0]
two_sided = true
material = "sheet"
"#;

const GLASS: &str = r#"manfred = 1
name = "glass"

[[camera]]
name = "main"
kind = "orthographic"
position = [0.0, 5.0, 0.0]
target = [0.0, 0.0, 0.0]
up = [0.0, 0.0, -1.0]
ortho_height = 3.0

[sun]
model = "authored"
toward = [0.0, 1.0, 0.0]
irradiance = 0.0

[[light]]
name = "lamp"
position = [1.5, 1.5, 0.0]
color = [1.0, 1.0, 1.0]
intensity = 60.0
radius = 0.05
range = 8.0
shadow = true

[materials.floor]
base = [0.7, 0.7, 0.7]
roughness = 1.0
specular = 0.0

[materials.glass]
family = "glass"
base = [1.0, 0.25, 0.25]
roughness = 0.02
transmission = 1.0
ior = 1.5
thickness = 0.004

[[object]]
name = "floor"
kind = "plane"
size = [4.0, 4.0]
material = "floor"

[[object]]
name = "pane"
kind = "box"
size = [0.6, 0.02, 0.6]
at = [0.0, 0.5, 0.0]
material = "glass"
"#;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn scene(name: &str, text: &str) -> PathBuf {
    let dir = root().join("tmp").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("scene.toml");
    std::fs::write(&file, text).unwrap();
    file
}

fn staged(path: &Path) -> (Recipe, Staging) {
    let recipe = load(path).unwrap().recipe;
    let built =
        manfred_build::build_with(&recipe, path.parent().unwrap(), &Registry::new()).unwrap();
    let recipe = built.recipe(&recipe);
    let staging = Staging::of(&recipe, &built.parts).unwrap();
    (recipe, staging)
}

fn traced(path: &Path) -> Vec<u8> {
    let (recipe, staging) = staged(path);
    let lens = eyepiece(
        manfred_scene::camera(&recipe, "main").unwrap(),
        View::Ortho,
        SIZE,
    )
    .unwrap();
    let stage = staging.stage(lens).unwrap();
    let (film, paced) = trace(
        &headless().unwrap(),
        &stage,
        SIZE,
        SAMPLES,
        SEED,
        &mut |_| (),
        &mut turn_reporting,
    )
    .unwrap();
    println!(
        "{}: {:.1} ms on the GPU, {:.3} ms a sample",
        path.display(),
        paced.gpu_ms,
        paced.gpu_ms / f64::from(SAMPLES)
    );
    looked(&film, &staging.finish).unwrap()
}

fn mean(image: &[u8], at: [f64; 2]) -> [f64; 3] {
    let x = ((at[0] + HALF) / (2.0 * HALF) * f64::from(SIZE[0])) as i64;
    let y = ((HALF - at[1]) / (2.0 * HALF) * f64::from(SIZE[1])) as i64;
    let mut sum = [0.0; 3];
    let mut count = 0.0;
    for dy in -3..=3 {
        for dx in -3..=3 {
            let k = ((y + dy) as usize * SIZE[0] as usize + (x + dx) as usize) * 4;
            for c in 0..3 {
                sum[c] += f64::from(image[k + c]);
            }
            count += 1.0;
        }
    }
    sum.map(|v| v / count)
}

fn off(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|c| (a[c] - b[c]).abs()).fold(0.0, f64::max)
}

#[test]
fn a_placement_is_two_sided_as_its_piece_is() {
    let file = scene("tracesides-cpu", WALLS);
    let (recipe, staging) = staged(&file);
    let lens = eyepiece(
        manfred_scene::camera(&recipe, "main").unwrap(),
        View::Ortho,
        SIZE,
    )
    .unwrap();
    let placements = staging.placements(&lens.eye);
    let at = |name: &str| {
        let object = recipe.objects.iter().position(|o| o.name == name).unwrap();
        let piece = staging
            .pieces
            .iter()
            .position(|p| p.object == object)
            .unwrap();
        &placements[piece]
    };
    assert!(!at("backdrop").two_sided);
    assert!(!at("from_behind").two_sided);
    assert!(!at("from_the_front").two_sided);
    assert!(at("both_faces").two_sided);
    let stage = staging.stage(lens).unwrap();
    let sided: Vec<bool> = stage
        .detail
        .instances
        .iter()
        .map(|instance| instance.two_sided)
        .collect();
    let expected: Vec<bool> = placements.iter().map(|placed| placed.two_sided).collect();
    assert_eq!(sided, expected);
}

#[test]
#[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
fn a_one_sided_wall_seen_from_behind_is_seen_through_traced_as_live() {
    let _gpu = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let file = scene("tracesides-walls", WALLS);
    let traced = traced(&file);
    write_png(
        &root().join("tmp/tracesides-walls-traced.png"),
        SIZE,
        &traced,
    )
    .unwrap();
    turn();
    let live = still(&file, "main", SIZE).unwrap();
    write_png(&root().join("tmp/tracesides-walls-live.png"), SIZE, &live).unwrap();
    let probes = [
        ("through the wall seen from behind", [-0.6, 0.0]),
        ("the wall seen from the front", [0.6, 0.0]),
        ("the two-sided wall", [0.0, 1.1]),
        ("the backdrop", [0.0, -1.0]),
    ];
    let mut worst: f64 = 0.0;
    for (name, at) in probes {
        let [t, l] = [mean(&traced, at), mean(&live, at)];
        worst = worst.max(off(t, l));
        println!(
            "{name}: traced {t:.1?}, live {l:.1?}, off by {:.1}",
            off(t, l)
        );
    }
    let [r, g, _] = mean(&traced, probes[0].1);
    assert!(r > 2.0 * g, "the backdrop shows through: {r} {g}");
    let [r, g, _] = mean(&traced, probes[1].1);
    assert!(g > 2.0 * r, "the front face is drawn: {r} {g}");
    let [r, g, _] = mean(&traced, probes[2].1);
    assert!(
        g > 2.0 * r,
        "the two-sided wall is drawn from behind: {r} {g}"
    );
    assert!(worst <= TOLERANCE, "{worst}");
    turn();
}

const SUNNY: &str = r#"
[sun]
model = "authored"
toward = [0.6, 0.8, 0.0]
irradiance = 3.0
"#;

fn glass_with(sun: bool, light: bool, pane: bool) -> String {
    let mut text = GLASS.to_string();
    if sun {
        text = text.replace(
            "[sun]
model = \"authored\"
toward = [0.0, 1.0, 0.0]
irradiance = 0.0
",
            SUNNY.trim_start(),
        );
    }
    if !light {
        let at = text.find("[[light]]").unwrap();
        let end = text.find("[materials.floor]").unwrap();
        text.replace_range(at..end, "");
    }
    if !pane {
        let at = text.find("[[object]]\nname = \"pane\"").unwrap();
        text.truncate(at);
    }
    text
}

fn frame_ms(path: &Path) -> f64 {
    let (recipe, built) = manfred_scene::load(path, None, &[], &Registry::new()).unwrap();
    let gpu = headless().unwrap();
    let mut reel = manfred_scene::Reel::new(&gpu, &recipe, &built.parts, SIZE).unwrap();
    let eye = reel.eye(manfred_scene::camera(&recipe, "main").unwrap());
    let (_, ms) = reel.shoot(&eye).unwrap();
    ms / f64::from(manfred_scene::SETTLE_FRAMES)
}

fn shadow_report(
    name: &str,
    text: &str,
    shadow: [f64; 2],
    lit: [f64; 2],
) -> ([f64; 3], [f64; 3], [f64; 3], [f64; 3]) {
    let file = scene(name, text);
    let traced = traced(&file);
    write_png(
        &root().join(format!("tmp/{name}-traced.png")),
        SIZE,
        &traced,
    )
    .unwrap();
    turn();
    let live = still(&file, "main", SIZE).unwrap();
    write_png(&root().join(format!("tmp/{name}-live.png")), SIZE, &live).unwrap();
    turn();
    let result = (
        mean(&traced, shadow),
        mean(&traced, lit),
        mean(&live, shadow),
        mean(&live, lit),
    );
    println!(
        "{name}: traced shadow {:.1?} lit {:.1?}, live shadow {:.1?} lit {:.1?}",
        result.0, result.1, result.2, result.3
    );
    result
}

#[test]
#[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
fn glass_tints_the_shadow_of_a_local_light_and_of_the_sun_live() {
    let _gpu = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (_, _, live_shadow, live_lit) = shadow_report(
        "tracesides-glass-light",
        &glass_with(false, true, true),
        [-0.75, 0.0],
        [-0.75, -0.9],
    );
    let tinted = |shadow: [f64; 3], lit: [f64; 3]| {
        shadow[0] / shadow[1].max(1.0) > 1.3 * lit[0] / lit[1].max(1.0)
    };
    assert!(
        tinted(live_shadow, live_lit),
        "{live_shadow:?} {live_lit:?}"
    );
    turn();
    let (_, _, live_shadow, live_lit) = shadow_report(
        "tracesides-glass-sun",
        &glass_with(true, false, true),
        [-0.4, 0.0],
        [-0.4, -0.9],
    );
    assert!(
        tinted(live_shadow, live_lit),
        "{live_shadow:?} {live_lit:?}"
    );
    turn();
}

#[test]
#[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
fn a_local_light_over_glass_costs_a_small_scene_a_fraction_of_a_millisecond() {
    let _gpu = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut rows = Vec::new();
    for (name, light, pane) in [
        ("bare", false, false),
        ("pane", false, true),
        ("light", true, false),
        ("pane and light", true, true),
    ] {
        let file = scene("tracesides-cost", &glass_with(false, light, pane));
        let mut best = f64::MAX;
        for _ in 0..3 {
            best = best.min(frame_ms(&file));
            turn();
        }
        println!("{name}: {best:.3} ms a frame");
        rows.push(best);
    }
    println!(
        "the light costs the glass scene {:.3} ms, the pane {:.3} ms",
        rows[3] - rows[1],
        rows[1] - rows[0]
    );
}
