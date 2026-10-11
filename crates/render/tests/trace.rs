use manfred_recipe::{Recipe, parse};
use manfred_render::{Film, Paced, Staging, View, eyepiece, looked, trace, write};
use pfx::adapter::turn_reporting;
use pfx_gpu::Gpu;
use pfx_load::Sky;
use pfx_trace::detail::Bounces;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

const BOX: &str = r#"manfred = 1
name = "box"

[sun]
model = "authored"
toward = [0.4, 0.8, 0.3]
radius_deg = 0.5
color = [1.0, 0.95, 0.9]
irradiance = 3.0

[[camera]]
name = "main"
position = [0.0, 1.2, 3.0]
target = [0.0, 0.2, 0.0]
fov_y_deg = 40.0

[materials.grey]
base = [0.5, 0.5, 0.5]
roughness = 0.8

[materials.orange]
family = "lacquer"
base = [0.73, 0.06, 0.0]
roughness = 0.45
clearcoat = 1.0
clearcoat_roughness = 0.07

[[object]]
name = "floor"
kind = "plane"
size = [4.0, 4.0]
material = "grey"

[[object]]
name = "cube"
kind = "box"
at = [0.0, 0.3, 0.0]
size = [0.6, 0.6, 0.6]
material = "orange"
"#;

const FURNACE: &str = r#"manfred = 1
name = "furnace"

[sun]
model = "authored"
toward = [0.0, 1.0, 0.0]
radius_deg = 0.5
color = [1.0, 1.0, 1.0]
irradiance = 0.0

[sky]
kind = "analytic"
intensity = 0.0

[[camera]]
name = "inside"
position = [0.0, 0.0, 0.0]
target = [0.0, 0.0, -1.0]
fov_y_deg = 90.0

[materials.glow]
base = [0.5, 0.5, 0.5]
roughness = 1.0
specular = 0.0
emission = [0.25, 0.5, 0.75]

[[object]]
name = "room"
kind = "box"
size = [2.0, 2.0, 2.0]
two_sided = true
material = "glow"
"#;

const WALLS: &str = r#"manfred = 1
name = "walls"

[sun]
model = "authored"
toward = [0.0, 1.0, 0.0]
radius_deg = 0.5
color = [1.0, 1.0, 1.0]
irradiance = 0.0

[sky]
kind = "analytic"
intensity = 0.0

[[camera]]
name = "inside"
position = [0.0, 0.0, 0.0]
target = [0.0, 0.0, -1.0]
fov_y_deg = 90.0

[[camera]]
name = "plan"
kind = "orthographic"
position = [0.0, 5.0, 0.0]
target = [0.0, 0.0, 0.0]
up = [0.0, 0.0, -1.0]
ortho_height = 2.0

[materials.red]
base = [0.0, 0.0, 0.0]
specular = 0.0
emission = [1.0, 0.0, 0.0]

[materials.green]
base = [0.0, 0.0, 0.0]
specular = 0.0
emission = [0.0, 1.0, 0.0]

[materials.blue]
base = [0.0, 0.0, 0.0]
specular = 0.0
emission = [0.0, 0.0, 1.0]

[materials.white]
base = [0.0, 0.0, 0.0]
specular = 0.0
emission = [1.0, 1.0, 1.0]

[materials.dim]
base = [0.0, 0.0, 0.0]
specular = 0.0
emission = [0.1, 0.1, 0.1]

[[object]]
name = "ahead"
kind = "plane"
at = [0.0, 0.0, -1.0]
rotate = [90.0, 0.0, 0.0]
size = [2.0, 2.0]
two_sided = true
material = "red"

[[object]]
name = "right"
kind = "plane"
at = [1.0, 0.0, 0.0]
rotate = [0.0, 0.0, 90.0]
size = [2.0, 2.0]
two_sided = true
material = "green"

[[object]]
name = "behind"
kind = "plane"
at = [0.0, 0.0, 1.0]
rotate = [90.0, 0.0, 0.0]
size = [2.0, 2.0]
two_sided = true
material = "blue"

[[object]]
name = "left"
kind = "plane"
at = [-1.0, 0.0, 0.0]
rotate = [0.0, 0.0, 90.0]
size = [2.0, 2.0]
two_sided = true
material = "dim"

[[object]]
name = "ceiling"
kind = "plane"
at = [0.0, 1.0, 0.0]
size = [2.0, 2.0]
two_sided = true
material = "white"

[[object]]
name = "floor"
kind = "plane"
at = [0.0, -1.0, 0.0]
size = [2.0, 2.0]
two_sided = true
material = "dim"
"#;

const SQUARE: &str = r#"manfred = 1
name = "square"

[sun]
model = "authored"
toward = [0.0, 1.0, 0.0]
radius_deg = 0.5
color = [1.0, 1.0, 1.0]
irradiance = 0.0

[sky]
kind = "analytic"
intensity = 0.0

[[camera]]
name = "plan"
kind = "perspective"
position = [0.0, 5.0, 0.0]
target = [0.0, 0.0, 0.0]
up = [0.0, 0.0, -1.0]
fov_y_deg = 30.0
ortho_height = 2.0

[materials.white]
base = [0.0, 0.0, 0.0]
specular = 0.0
emission = [1.0, 1.0, 1.0]

[[object]]
name = "square"
kind = "plane"
size = [1.0, 1.0]
material = "white"
"#;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn scratch(name: &str) -> PathBuf {
    let dir = root().join("tmp");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn staging(text: &str) -> (Recipe, Staging) {
    let recipe = parse(text).unwrap();
    let built = manfred_build::build(&recipe, Path::new("")).unwrap();
    let staging = Staging::of(&recipe, &built.parts).unwrap();
    (recipe, staging)
}

fn traced(
    gpu: &Gpu,
    (recipe, staging): (&Recipe, &Staging),
    camera: &str,
    view: View,
    size: [u32; 2],
    samples: u32,
    seed: u32,
) -> (Film, Paced) {
    let camera = manfred_scene::camera(recipe, camera).unwrap();
    let staged = staging
        .stage(eyepiece(camera, view, size).unwrap())
        .unwrap();
    trace(
        gpu,
        &staged,
        size,
        samples,
        seed,
        &mut |_| (),
        &mut turn_reporting,
    )
    .unwrap()
}

fn gpu() -> Gpu {
    manfred_scene::headless().unwrap()
}

fn luminance(pixel: [f32; 4]) -> f32 {
    0.2126 * pixel[0] + 0.7152 * pixel[1] + 0.0722 * pixel[2]
}

fn at(film: &Film, u: f32, v: f32) -> [f32; 4] {
    let [width, height] = film.size;
    let x = ((u * width as f32) as u32).min(width - 1);
    let y = ((v * height as f32) as u32).min(height - 1);
    film.color[(y * width + x) as usize]
}

fn bytes(film: &Film) -> Vec<u8> {
    film.color
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn a_box_under_a_sun_renders_and_one_seed_gives_the_same_bytes() {
    let _gpu = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (recipe, staging) = staging(BOX);
    let gpu = gpu();
    let size = [160, 100];
    let (first, paced) = traced(&gpu, (&recipe, &staging), "main", View::Lens, size, 16, 3);
    let (again, _) = traced(&gpu, (&recipe, &staging), "main", View::Lens, size, 16, 3);
    let (other, _) = traced(&gpu, (&recipe, &staging), "main", View::Lens, size, 16, 4);
    println!("a box under a sun at 160x100, 16 samples: {paced:?}");
    assert!(first.color.iter().flatten().all(|v| v.is_finite()));
    assert_eq!(bytes(&first), bytes(&again));
    assert_ne!(bytes(&first), bytes(&other));
    assert!(paced.longest_ms < 300.0, "{paced:?}");
    let cube = at(&first, 0.5, 0.55);
    let floor = at(&first, 0.5, 0.95);
    let sky = at(&first, 0.5, 0.02);
    assert!(
        cube[0] > cube[2] * 1.5,
        "the lacquer reads orange: {cube:?}"
    );
    assert!(luminance(floor) > 0.05, "{floor:?}");
    assert!(luminance(sky) > 0.05, "{sky:?}");
    let path = scratch("render-box.png");
    write(&path, &first, &staging.finish).unwrap();
    pfx::adapter::turn();
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn an_emitter_only_room_keeps_its_mean() {
    let _gpu = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (recipe, staging) = staging(FURNACE);
    assert!(staging.lights.is_empty());
    assert_eq!(staging.sun.intensity, 0.0);
    let gpu = gpu();
    let (film, paced) = traced(
        &gpu,
        (&recipe, &staging),
        "inside",
        View::Lens,
        [64, 64],
        64,
        11,
    );
    let mean = film.mean();
    let albedo: f64 = 0.5;
    let emission = [0.25, 0.5, 0.75];
    let hits = Bounces::REFERENCE.diffuse as i32 + 2;
    let expected = emission.map(|e| e * (1.0 - albedo.powi(hits)) / (1.0 - albedo));
    println!(
        "an emitter-only room of albedo {albedo}: mean {mean:.5?} against Le(1-albedo^{hits})/(1-albedo) = {expected:?}, {paced:?}"
    );
    for (got, want) in mean.iter().zip(expected) {
        assert!((got - want).abs() < 0.01 * want, "{mean:?} {expected:?}");
    }
    pfx::adapter::turn();
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn an_orthographic_view_covers_the_square_at_its_size() {
    let _gpu = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (recipe, staging) = staging(SQUARE);
    let gpu = gpu();
    let (film, _) = traced(
        &gpu,
        (&recipe, &staging),
        "plan",
        View::Ortho,
        [64, 64],
        8,
        1,
    );
    let lit = film.color.iter().filter(|p| luminance(**p) > 0.5).count();
    println!("a 1 m square in a 2 m orthographic view over 64 pixels lights {lit} of 4096");
    assert!((lit as i64 - 32 * 32).abs() <= 64, "{lit}");
    assert!(luminance(at(&film, 0.5, 0.5)) > 0.9);
    assert!(luminance(at(&film, 0.1, 0.1)) < 0.01);
    let (lens, _) = traced(
        &gpu,
        (&recipe, &staging),
        "plan",
        View::Lens,
        [64, 64],
        8,
        1,
    );
    let seen = lens.color.iter().filter(|p| luminance(**p) > 0.5).count();
    println!("the same camera in perspective at 30 degrees lights {seen} of 4096");
    let side = 64.0 / (2.0 * 5.0 * 15f64.to_radians().tan());
    println!("a 30 degree view from 5 m covers the square with {side:.2} pixels a side");
    assert!((seen as f64 - side * side).abs() <= 64.0, "{seen} {side}");
    let (recipe, walls) = self::staging(WALLS);
    let (plan, _) = traced(&gpu, (&recipe, &walls), "plan", View::Lens, [64, 32], 4, 1);
    assert!(
        luminance(at(&plan, 0.5, 0.5)) > 0.9,
        "{:?}",
        at(&plan, 0.5, 0.5)
    );
    pfx::adapter::turn();
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn an_equirect_sees_the_room_around_the_camera_and_reads_back_as_a_sky() {
    let _gpu = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (recipe, staging) = staging(WALLS);
    let gpu = gpu();
    let (film, _) = traced(
        &gpu,
        (&recipe, &staging),
        "inside",
        View::Equirect,
        [128, 64],
        4,
        1,
    );
    let near = |pixel: [f32; 4], want: [f32; 3]| {
        pixel
            .iter()
            .zip(want)
            .all(|(got, want)| (got - want).abs() < 0.02)
    };
    assert!(near(at(&film, 0.5, 0.5), [1.0, 0.0, 0.0]), "ahead is red");
    assert!(
        near(at(&film, 0.75, 0.5), [0.0, 1.0, 0.0]),
        "right is green"
    );
    assert!(
        near(at(&film, 0.02, 0.5), [0.0, 0.0, 1.0]),
        "behind is blue"
    );
    assert!(near(at(&film, 0.25, 0.5), [0.1, 0.1, 0.1]), "left is dim");
    assert!(near(at(&film, 0.5, 0.02), [1.0, 1.0, 1.0]), "up is white");
    assert!(near(at(&film, 0.5, 0.98), [0.1, 0.1, 0.1]), "down is dim");
    let path = scratch("render-walls.hdr");
    write(&path, &film, &staging.finish).unwrap();
    let sky = Sky::parse(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!([sky.width, sky.height], film.size);
    for (back, traced) in sky.texels.iter().zip(&film.color) {
        for k in 0..3 {
            let step = traced[0].max(traced[1]).max(traced[2]) / 128.0;
            assert!(
                (back[k] - traced[k]).abs() <= step + 1e-7,
                "{back:?} {traced:?}"
            );
        }
    }
    pfx::adapter::turn();
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn the_table_equirect_previews_from_its_hdr() {
    let _gpu = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let path = root().join("tests/golden/table/table.toml");
    let loaded = manfred_recipe::load(&path).unwrap();
    let built = manfred_build::build(&loaded.recipe, path.parent().unwrap()).unwrap();
    let staging = Staging::of(&loaded.recipe, &built.parts).unwrap();
    let gpu = gpu();
    let size = [1024, 512];
    let (film, paced) = traced(
        &gpu,
        (&loaded.recipe, &staging),
        "main",
        View::Equirect,
        size,
        32,
        5,
    );
    println!("the table as an equirect at 1024x512, 32 samples: {paced:?}");
    assert!(paced.longest_ms < 300.0, "{paced:?}");
    let hdr = scratch("render-equirect.hdr");
    write(&hdr, &film, &staging.finish).unwrap();
    let sky = Sky::parse(&std::fs::read(&hdr).unwrap()).unwrap();
    let preview = Film {
        color: sky.texels,
        ..film
    };
    let rgba = looked(&preview, &staging.finish).unwrap();
    manfred_scene::write_png(&scratch("render-equirect.png"), size, &rgba).unwrap();
    pfx::adapter::turn();
}
