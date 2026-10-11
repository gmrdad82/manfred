use manfred_scene::{FRAME_RATE, Stage, UNMEASURED_MS, camera, headless, still, write_png};
use pfx::adapter::{TurnState, turn, turn_soon};
use std::path::{Path, PathBuf};

const GLASS: &str = r#"manfred = 1
name = "glass-shadow"

[[camera]]
name = "main"
position = [0.0, 0.0, 4.0]
target = [0.0, 0.0, 0.0]
fov_y_deg = 45.0

[sun]
model = "authored"
toward = [0.3, 0.5, 0.8]
irradiance = 2.0

[materials.floor]
base = [0.6, 0.6, 0.6]
roughness = 0.6
specular = 0.0

[materials.pane]
base = [0.5, 0.62, 0.95]
transmission = 1.0
roughness = 0.02
ior = 1.45

[[object]]
name = "floor"
kind = "box"
size = [6.0, 6.0, 0.2]
at = [0.0, 0.0, -1.0]
material = "floor"

[[object]]
name = "pane"
kind = "box"
size = [1.0, 1.0, 0.4]
at = [-0.8, 0.0, 0.0]
material = "pane"
"#;

const CARD: &str = r#"manfred = 1
name = "cutout-card"

[[camera]]
name = "main"
kind = "orthographic"
position = [0.0, 0.0, 4.0]
target = [0.0, 0.0, 0.0]
up = [0.0, 1.0, 0.0]
ortho_height = 2.0

[sun]
model = "authored"
toward = [0.2, 0.4, 1.0]
irradiance = 3.0

[materials.floor]
base = [0.15, 0.2, 0.55]
roughness = 1.0
specular = 0.0

[materials.sheet]
base = [0.85, 0.85, 0.85]
roughness = 1.0
specular = 0.0

[[object]]
name = "floor"
kind = "box"
size = [4.0, 4.0, 0.1]
at = [0.0, 0.0, -0.4]
material = "floor"

[[object]]
name = "card"
kind = "card"
size = [1.2, 1.2]
image = "ring.png"
alpha_cutoff = 0.5
material = "sheet"
"#;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch(name: &str) -> PathBuf {
    let dir = root().join("tmp").join(name);
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

fn ring(x: u32, y: u32) -> [u8; 4] {
    let u = (f64::from(x) + 0.5) / 512.0 - 0.5;
    let v = (f64::from(y) + 0.5) / 512.0 - 0.5;
    let r = (u * u + v * v).sqrt();
    let alpha = if (0.22..=0.42).contains(&r) { 255 } else { 0 };
    [220, 40, 30, alpha]
}

fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn glass_shadow_bytes() {
    let dir = scratch("engine19-glass");
    let file = dir.join("glass.toml");
    std::fs::write(&file, GLASS).unwrap();
    let size = [256, 256];
    let image = still(&file, "main", size).unwrap();
    let out = root().join("tmp/engine19-glass-after.png");
    write_png(&out, size, &image).unwrap();
    println!(
        "glass shadow bytes {} hash {:016x}",
        image.len(),
        hash(&image)
    );
    let before_path = root().join("tmp/engine19-glass-before.png");
    if let Some(before) = decode(&before_path) {
        let mut worst = 0u8;
        for (a, b) in image.chunks_exact(4).zip(before.chunks_exact(4)) {
            for k in 0..4 {
                worst = worst.max(a[k].abs_diff(b[k]));
            }
        }
        println!(
            "glass shadow against the copy path: worst channel {worst}, same {}",
            worst == 0
        );
    }
    for (name, at) in [
        ("left", [80u32, 128]),
        ("centre", [128, 128]),
        ("right", [180, 128]),
        ("low", [128, 180]),
        ("high", [128, 70]),
    ] {
        let k = ((at[1] * size[0] + at[0]) * 4) as usize;
        println!("{name} {at:?}: {:?}", &image[k..k + 4]);
    }
    turn();
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn a_cutout_card_frame_time() {
    let dir = scratch("engine19-card");
    png(&dir.join("ring.png"), 512, ring);
    let file = dir.join("card.toml");
    std::fs::write(&file, CARD).unwrap();
    let recipe = manfred_recipe::load(&file).unwrap().recipe;
    let built = manfred_build::build(&recipe, &dir).unwrap();
    let size = [3840, 2160];
    let gpu = headless().unwrap();
    let mut stage = Stage::with_parts(gpu, &recipe, &built.parts, size).unwrap();
    let eye = manfred_scene::Eye::from_camera(
        camera(&recipe, "main").unwrap(),
        f64::from(size[0]) / f64::from(size[1]),
    );
    let target = stage
        .gpu()
        .offscreen(size[0], size[1], stage.output_format())
        .unwrap();
    let mut turns = TurnState::default();
    let mut times = Vec::new();
    for frame in 0..36u32 {
        let measured = stage
            .render_measured(&eye, frame as f32 / FRAME_RATE, &target.view)
            .unwrap();
        if frame >= 8 {
            times.extend(measured);
        }
        turn_soon(&mut turns, measured.unwrap_or(UNMEASURED_MS));
    }
    times.sort_by(f64::total_cmp);
    let median = times[times.len() / 2];
    println!(
        "512 cutout card at {}x{}, median {:.3} ms over {} frames, instances {}",
        size[0],
        size[1],
        median,
        times.len(),
        stage.instance_count()
    );
    let image = stage.gpu().readback_rgba8(&target).unwrap();
    write_png(&root().join("tmp/engine19-card-after.png"), size, &image).unwrap();
    turn();
}

fn decode(path: &Path) -> Option<Vec<u8>> {
    let file = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    Some(buf)
}
