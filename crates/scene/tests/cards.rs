use manfred_recipe::{Recipe, load};
use manfred_scene::{FRAME_RATE, Stage, UNMEASURED_MS, camera, headless, still, write_png};
use pfx::adapter::{TurnState, turn, turn_soon};
use std::path::{Path, PathBuf};

const SIZE: [u32; 2] = [400, 400];

const RECIPE: &str = r#"manfred = 1
name = "surfaces"
seed = 3

[[camera]]
name = "main"
kind = "orthographic"
position = [0.0, 0.0, 5.0]
target = [0.0, 0.0, 0.0]
up = [0.0, 1.0, 0.0]
ortho_height = 2.0

[sun]
model = "authored"
toward = [0.2, 0.3, 1.0]
irradiance = 3.0

[materials.backdrop]
base = [0.05, 0.1, 0.7]
roughness = 1.0
specular = 0.0

[materials.sheet]
base = [0.8, 0.8, 0.8]
roughness = 1.0
specular = 0.0

[materials.slider]
base = [0.8, 0.6, 0.05]
roughness = 1.0
specular = 0.0

[[object]]
name = "backdrop"
kind = "box"
size = [4.0, 4.0, 0.1]
at = [0.0, 0.0, -0.5]
material = "backdrop"

[[object]]
name = "photo"
kind = "card"
size = [0.8, 0.8]
at = [-0.5, 0.5, 0.0]
image = "red.png"
material = "sheet"

[[object]]
name = "sprite"
kind = "card"
size = [0.8, 0.8]
at = [0.5, 0.5, 0.0]
image = "disc.png"
alpha_cutoff = 0.5
material = "sheet"

[[object]]
name = "feed"
kind = "card"
size = [0.8, 0.8]
at = [-0.5, -0.5, 0.0]
slot = "page"
material = "sheet"

[[object]]
name = "mv_slide"
kind = "box"
size = [0.6, 0.6, 0.2]
at = [0.5, -0.5, 0.0]
material = "slider"

[[mover]]
group = "slide"
pivot = [0.5, -0.5, 0.0]
axis = [0.0, 1.0, 0.0]
travel = 0.01
clip = [0.0, 1.0, 0.0, -0.5]
"#;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp")
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn png(path: &Path, side: u32, texel: impl Fn(u32, u32) -> [u8; 4]) {
    let mut texels = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            texels.extend(texel(x, y));
        }
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, side, side);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&texels).unwrap();
    writer.finish().unwrap();
    std::fs::write(path, out).unwrap();
}

fn scene(dir: &Path, red_side: u32) -> PathBuf {
    png(&dir.join("red.png"), red_side, |_, _| [230, 20, 20, 255]);
    png(&dir.join("disc.png"), 64, |x, y| {
        let dx = f64::from(x) + 0.5 - 32.0;
        let dy = f64::from(y) + 0.5 - 32.0;
        if dx * dx + dy * dy < 24.0 * 24.0 {
            [20, 220, 20, 255]
        } else {
            [255, 255, 255, 0]
        }
    });
    let file = dir.join("surfaces.toml");
    std::fs::write(&file, RECIPE).unwrap();
    file
}

fn pixel(image: &[u8], at: [f64; 2]) -> [u8; 3] {
    let x = ((at[0] + 1.0) * 0.5 * f64::from(SIZE[0])) as usize;
    let y = ((1.0 - at[1]) * 0.5 * f64::from(SIZE[1])) as usize;
    let k = (y * SIZE[0] as usize + x) * 4;
    [image[k], image[k + 1], image[k + 2]]
}

fn staged(path: &Path) -> (Recipe, manfred_build::Built) {
    let recipe = load(path).unwrap().recipe;
    let built = manfred_build::build(&recipe, path.parent().unwrap()).unwrap();
    (recipe, built)
}

#[test]
#[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
fn cards_show_their_images_cut_out_and_movers_clip() {
    let dir = scratch("surfaces");
    let file = scene(&dir, 32);
    let image = still(&file, "main", SIZE).unwrap();
    let out = scratch("").join("surfaces-cards.png");
    write_png(&out, SIZE, &image).unwrap();
    let probes = [
        ("photo", [-0.5, 0.5]),
        ("sprite", [0.5, 0.5]),
        ("sprite's corner", [0.83, 0.83]),
        ("feed", [-0.5, -0.5]),
        ("feed's border", [-0.895, -0.5]),
        ("slider below the plane", [0.5, -0.65]),
        ("slider above the plane", [0.5, -0.35]),
        ("backdrop", [0.0, 0.0]),
    ];
    for (name, at) in probes {
        println!("{name}: {:?}", pixel(&image, at));
    }
    let [r, g, b] = pixel(&image, [-0.5, 0.5]).map(i32::from);
    assert!(r > 2 * g && r > 2 * b, "photo {:?}", [r, g, b]);
    let [r, g, b] = pixel(&image, [0.5, 0.5]).map(i32::from);
    assert!(g > 2 * r && g > 2 * b, "sprite {:?}", [r, g, b]);
    let blue = |at: [f64; 2]| {
        let [r, g, b] = pixel(&image, at).map(i32::from);
        b > 2 * r && b > g
    };
    assert!(blue([0.0, 0.0]), "the backdrop");
    assert!(blue([0.83, 0.83]), "the sprite's cut corner");
    let [r, g, b] = pixel(&image, [-0.5, -0.5]).map(i32::from);
    assert!(
        r > 20 && (r - g).abs() * 5 < r && b < 2 * r,
        "feed {:?}",
        [r, g, b]
    );
    let border = pixel(&image, [-0.895, -0.5]).map(i32::from);
    assert!(
        border[0] < r,
        "the placeholder's frame {border:?} against {r}"
    );
    let [r, g, b] = pixel(&image, [0.5, -0.65]).map(i32::from);
    assert!(r > 2 * b && g > b, "slider {:?}", [r, g, b]);
    assert!(blue([0.5, -0.35]), "the slider above its plane");
    assert_eq!(still(&file, "main", SIZE).unwrap(), image);
    turn();
}

#[test]
#[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
fn the_atlas_is_rebuilt_only_when_an_image_changes() {
    let dir = scratch("surfaces-update");
    let file = scene(&dir, 32);
    let (recipe, built) = staged(&file);
    let mut stage = Stage::new(headless().unwrap(), &recipe, &built, SIZE).unwrap();
    let first = stage.atlas().unwrap();
    assert_eq!(first.0, [256, 256]);
    let moved = RECIPE.replace("at = [-0.5, 0.5, 0.0]", "at = [-0.4, 0.5, 0.0]");
    std::fs::write(&file, moved).unwrap();
    let (recipe, built) = staged(&file);
    let change = stage.update(&recipe, &built).unwrap();
    assert_eq!(change.replaced, 1);
    assert_eq!(stage.atlas(), Some(first));
    scene(&dir, 300);
    let (recipe, built) = staged(&file);
    stage.update(&recipe, &built).unwrap();
    let grown = stage.atlas().unwrap();
    assert_eq!(grown.0, [512, 512]);
    println!("atlas: {first:?} then {grown:?}");
    let target = stage
        .gpu()
        .offscreen(SIZE[0], SIZE[1], stage.output_format())
        .unwrap();
    let eye = manfred_scene::Eye::from_camera(camera(&recipe, "main").unwrap(), 1.0);
    stage.render(&eye, 0.0, &target.view).unwrap();
    let bare = RECIPE
        .split("[[object]]\nname = \"photo\"")
        .next()
        .unwrap()
        .to_string();
    std::fs::write(&file, bare).unwrap();
    let (recipe, built) = staged(&file);
    stage.update(&recipe, &built).unwrap();
    assert_eq!(stage.atlas(), None);
    stage.render(&eye, 0.0, &target.view).unwrap();
    turn();
}

fn frame_times(stage: &mut Stage, recipe: &Recipe, frames: u32) -> Vec<f64> {
    let size = stage.size();
    let eye = manfred_scene::Eye::from_camera(
        camera(recipe, "main").unwrap(),
        f64::from(size[0]) / f64::from(size[1]),
    );
    let target = stage
        .gpu()
        .offscreen(size[0], size[1], stage.output_format())
        .unwrap();
    let mut turns = TurnState::default();
    let mut times = Vec::new();
    for frame in 0..frames {
        let measured = stage
            .render_measured(&eye, frame as f32 / FRAME_RATE, &target.view)
            .unwrap();
        times.extend(measured);
        turn_soon(&mut turns, measured.unwrap_or(UNMEASURED_MS));
    }
    times.sort_by(f64::total_cmp);
    times
}

#[test]
#[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
fn the_toyboxs_frame_time_with_and_without_its_clip_planes() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/toybox/toybox.toml");
    let (mut recipe, built) = staged(&path);
    if recipe
        .sky
        .as_ref()
        .and_then(|sky| sky.path.as_ref())
        .is_some_and(|sky| !path.parent().unwrap().join(sky).exists())
    {
        recipe.sky = None;
    }
    let mut bare = recipe.clone();
    bare.movers.clear();
    let size = [1600, 1000];
    let gpu = headless().unwrap();
    let report = |label: &str, times: &[f64]| {
        let at = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];
        println!(
            "the toybox at {}x{} {label}: median {:.3} ms, p90 {:.3} ms over {} frames",
            size[0],
            size[1],
            at(0.5),
            at(0.9),
            times.len()
        );
        at(0.5)
    };
    let mut rounds = Vec::new();
    for _ in 0..2 {
        let mut before = Stage::new(gpu.clone(), &bare, &built, size).unwrap();
        let without = report("without clip planes", &frame_times(&mut before, &bare, 240));
        drop(before);
        turn();
        let mut after = Stage::new(gpu.clone(), &recipe, &built, size).unwrap();
        let with = report("with clip planes", &frame_times(&mut after, &recipe, 240));
        drop(after);
        turn();
        rounds.push((without, with));
    }
    println!("rounds (without, with): {rounds:?}");
}
