use manfred_build::Registry;
use manfred_recipe::{Recipe, load};
use manfred_render::{Film, IDENTITY, Staging, View, eyepiece, looked, trace};
use manfred_scene::{Eye, facing, headless, placeholder, premultiplied, still, write_png};
use pfx::adapter::{turn, turn_reporting};
use pfx_gpu::Gpu;
use pfx_materials::Blend;
use pfx_trace::detail::ContentFace;
use pfx_trace::stage::{ContentKind, Image, Placement, PlacementContent, Stage};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

const SIZE: [u32; 2] = [400, 400];
const HALF: f64 = 1.5;
const SAMPLES: u32 = 256;
const SEED: u32 = 9;
const TOLERANCE: f64 = 24.0;

const RECIPE: &str = r#"manfred = 1
name = "tracecards"
seed = 3

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

[[object]]
name = "ghost"
kind = "box"
size = [0.3, 0.3, 0.3]
at = [-1.2, 0.6, 0.0]
material = "slider"
shadow = "none"

[[object]]
name = "solid"
kind = "box"
size = [0.3, 0.3, 0.3]
at = [-1.2, -0.6, 0.0]
material = "slider"

[[object]]
name = "sign"
kind = "card"
size = [0.4, 0.4]
at = [1.2, 0.0, 0.0]
rotate = [0.0, 60.0, 0.0]
image = "sign.png"
face_camera = true
material = "sheet"

[[mover]]
group = "slide"
pivot = [0.5, -0.5, 0.0]
axis = [0.0, 1.0, 0.0]
travel = 0.01
clip = [0.0, 1.0, 0.0, -0.5]
"#;

const REGIONS: [(&str, [f64; 2]); 12] = [
    ("photo", [-0.5, 0.5]),
    ("sprite", [0.5, 0.5]),
    ("sprite's cut corner", [0.83, 0.83]),
    ("feed", [-0.5, -0.5]),
    ("backdrop", [0.0, 0.0]),
    ("slider below the plane", [0.5, -0.65]),
    ("slider above the plane", [0.5, -0.35]),
    ("under the ghost", [-1.25, 0.36]),
    ("under the solid box", [-1.25, -0.84]),
    ("open backdrop", [-1.25, 0.0]),
    ("sign", [1.2, 0.0]),
    ("sign's turned edge", [1.36, 0.0]),
];

fn image<'a>(content: &PlacementContent<'a>) -> Image<'a> {
    match content.kind {
        ContentKind::Image(image) => image,
        ContentKind::Text(_) => panic!("the content is text, not an image"),
    }
}

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
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

fn disc(x: u32, y: u32) -> [u8; 4] {
    let dx = f64::from(x) + 0.5 - 32.0;
    let dy = f64::from(y) + 0.5 - 32.0;
    if dx * dx + dy * dy < 24.0 * 24.0 {
        [20, 220, 20, 255]
    } else {
        [255, 255, 255, 0]
    }
}

fn scene(name: &str) -> PathBuf {
    let dir = scratch(name);
    png(&dir.join("red.png"), 32, |_, _| [230, 20, 20, 255]);
    png(&dir.join("disc.png"), 64, disc);
    png(&dir.join("sign.png"), 16, |_, _| [230, 200, 20, 255]);
    let file = dir.join("tracecards.toml");
    std::fs::write(&file, RECIPE).unwrap();
    file
}

fn staged(path: &Path, registry: &Registry) -> (Recipe, Staging) {
    let recipe = load(path).unwrap().recipe;
    let built = manfred_build::build_with(&recipe, path.parent().unwrap(), registry).unwrap();
    let recipe = built.recipe(&recipe);
    let staging = Staging::of(&recipe, &built.parts).unwrap();
    (recipe, staging)
}

fn piece(recipe: &Recipe, staging: &Staging, name: &str) -> usize {
    let object = recipe.objects.iter().position(|o| o.name == name).unwrap();
    staging
        .pieces
        .iter()
        .position(|piece| piece.object == object)
        .unwrap()
}

fn apply(m: &[[f32; 4]; 4], v: [f64; 3], w: f32) -> [f32; 3] {
    let v = [v[0] as f32, v[1] as f32, v[2] as f32, w];
    std::array::from_fn(|r| (0..4).map(|c| m[c][r] * v[c]).sum())
}

#[test]
fn the_placements_carry_each_cards_image_cutout_turn_and_shadow() {
    let file = scene("tracecards-cpu");
    let (recipe, staging) = staged(&file, &Registry::new());
    let camera = manfred_scene::camera(&recipe, "main").unwrap();
    let lens = eyepiece(camera, View::Ortho, SIZE).unwrap();
    let placements = staging.placements(&lens.eye);
    let at = |name: &str| placements[piece(&recipe, &staging, name)];
    for name in ["photo", "sprite", "feed", "sign"] {
        let placement = at(name);
        let material = &staging.materials[placement.material as usize];
        assert_eq!(material.content_layer.slot, 0, "{name}");
        assert_eq!(material.content_layer.blend, Blend::Over, "{name}");
        let content = placement.content.unwrap();
        assert_eq!(content.uv_offset, [0.0; 2], "{name}");
        assert_eq!(content.uv_scale, [1.0; 2], "{name}");
        assert_eq!(content.crop, [0.0, 0.0, 1.0, 1.0], "{name}");
        assert_eq!(content.face, ContentFace::Both, "{name}");
        assert!(image(&content).srgb && image(&content).valid(), "{name}");
        assert_eq!(content.cutout, name == "sprite", "{name}");
        assert!(placement.casts_shadow, "{name}");
        assert!(!placement.shadow_only, "{name}");
        assert_eq!(placement.alpha_cutoff, 0.5, "{name}");
    }
    assert_eq!(at("mv_slide").clip, [[0.0, 1.0, 0.0, -0.5], [0.0; 4]]);
    assert_eq!(at("backdrop").clip, [[0.0; 4]; 2]);
    assert!(!at("ghost").shadow_only);
    for name in ["backdrop", "mv_slide", "ghost", "solid"] {
        let placement = at(name);
        assert!(placement.content.is_none(), "{name}");
        assert_eq!(placement.model, IDENTITY, "{name}");
        assert_eq!(
            staging.materials[placement.material as usize]
                .content_layer
                .slot,
            -1,
            "{name}"
        );
        assert_eq!(placement.casts_shadow, name != "ghost", "{name}");
    }
    assert_ne!(at("photo").material, at("backdrop").material);
    assert_eq!(at("photo").material, at("feed").material);
    assert_eq!(at("mv_slide").material, at("ghost").material);
    let photo = image(&at("photo").content.unwrap());
    assert_eq!([photo.width, photo.height], [32, 32]);
    assert!(photo.texels.iter().all(|t| *t == [230, 20, 20, 255]));
    let sprite = image(&at("sprite").content.unwrap());
    assert_eq!([sprite.width, sprite.height], [64, 64]);
    assert_eq!(sprite.texels[0], [0, 0, 0, 0]);
    assert_eq!(sprite.texels[32 * 64 + 32], [20, 220, 20, 255]);
    let feed = image(&at("feed").content.unwrap());
    assert_eq!([feed.width, feed.height], [64, 64]);
    assert_eq!(feed.texels.as_flattened(), premultiplied(&placeholder()));
    let face = staging.cards[piece(&recipe, &staging, "sign")]
        .unwrap()
        .face
        .unwrap();
    assert!(
        staging.cards[piece(&recipe, &staging, "photo")]
            .unwrap()
            .face
            .is_none()
    );
    let sign = at("sign");
    assert_eq!(sign.model, facing(&face, &lens.eye));
    assert_ne!(sign.model, IDENTITY);
    let normal = apply(&sign.model, face.normal, 0.0);
    assert!((normal[2] - 1.0).abs() < 1e-5, "{normal:?}");
    let centre = apply(&sign.model, face.centre, 1.0);
    assert!((centre[0] - 1.2).abs() < 1e-5 && centre[2].abs() < 1e-5);
    let staged = staging.stage(lens).unwrap();
    let detail = &staged.detail;
    assert_eq!(detail.content_slots.len(), 1 + 4);
    assert!(detail.content_slots[0].is_none());
    assert!(detail.content_slots[1..].iter().all(Option::is_some));
    let surface = |name: &str| detail.instances[piece(&recipe, &staging, name)].surface;
    assert!(surface("sprite").cutout && !surface("photo").cutout);
    assert!(!surface("ghost").casts_shadow && surface("solid").casts_shadow);
    assert_eq!(surface("mv_slide").clip, [[0.0, 1.0, 0.0, -0.5], [0.0; 4]]);
    assert_eq!(surface("backdrop").clip, [[0.0; 4]; 2]);
    let slots: Vec<i32> = ["photo", "sprite", "feed", "sign"]
        .iter()
        .map(|name| surface(name).layer.slot)
        .collect();
    assert_eq!(slots, [1, 2, 3, 4]);
    assert_eq!(
        detail.instances[piece(&recipe, &staging, "sign")].transform,
        sign.model
    );
}

#[test]
fn a_perspective_eye_turns_a_card_toward_the_cameras_position() {
    let file = scene("tracecards-lens");
    let text = RECIPE.replace(
        "kind = \"orthographic\"\nposition = [0.0, 0.0, 5.0]",
        "kind = \"perspective\"\nfov_y_deg = 40.0\nposition = [3.0, 0.0, 5.0]",
    );
    std::fs::write(&file, text).unwrap();
    let (recipe, staging) = staged(&file, &Registry::new());
    let camera = manfred_scene::camera(&recipe, "main").unwrap();
    let lens = eyepiece(camera, View::Lens, SIZE).unwrap();
    assert_eq!(lens.eye, Eye::from_camera(camera, 1.0));
    let at = piece(&recipe, &staging, "sign");
    let face = staging.cards[at].unwrap().face.unwrap();
    let model = staging.placements(&lens.eye)[at].model;
    assert_eq!(model, facing(&face, &lens.eye));
    let normal = apply(&model, face.normal, 0.0);
    let toward = [3.0 - 1.2, 0.0, 5.0];
    let length = ((toward[0] * toward[0] + toward[2] * toward[2]) as f32).sqrt();
    assert!(
        (normal[0] - toward[0] as f32 / length).abs() < 1e-5,
        "{normal:?}"
    );
    assert!(
        (normal[2] - toward[2] as f32 / length).abs() < 1e-5,
        "{normal:?}"
    );
    let round = eyepiece(camera, View::Equirect, SIZE).unwrap();
    assert_eq!(round.eye.ortho_height, None);
}

#[test]
fn a_card_cuts_only_where_live_cuts_it() {
    let file = scene("tracecards-cutoff");
    for (cutoff, cut) in [("0.0", false), ("0.25", true), ("0.9", true)] {
        let text = RECIPE.replace("alpha_cutoff = 0.5", &format!("alpha_cutoff = {cutoff}"));
        std::fs::write(&file, text).unwrap();
        let (recipe, staging) = staged(&file, &Registry::new());
        let at = piece(&recipe, &staging, "sprite");
        let lens = eyepiece(
            manfred_scene::camera(&recipe, "main").unwrap(),
            View::Ortho,
            SIZE,
        )
        .unwrap();
        let placement = staging.placements(&lens.eye)[at];
        let content = placement.content.unwrap();
        assert_eq!(content.cutout, cut, "alpha_cutoff = {cutoff}");
        assert_eq!(
            placement.alpha_cutoff,
            cutoff.parse::<f32>().unwrap(),
            "alpha_cutoff = {cutoff}"
        );
    }
}

fn plain<'a>(staging: &'a Staging) -> Vec<Placement<'a>> {
    staging
        .slots
        .iter()
        .enumerate()
        .map(|(mesh, &material)| Placement {
            two_sided: false,
            ..Placement::new(mesh as u32, IDENTITY, material)
        })
        .collect()
}

fn goldens() -> [(PathBuf, &'static str, View, [u32; 2]); 2] {
    [
        (
            root().join("tests/golden/table/table.toml"),
            "main",
            View::Lens,
            [200, 125],
        ),
        (
            root().join("tests/golden/table/table.toml"),
            "main",
            View::Equirect,
            [256, 128],
        ),
    ]
}

#[test]
fn the_table_through_a_lens_and_as_an_equirect_places_as_before() {
    let registry = Registry::new();
    for (path, camera, view, size) in goldens() {
        let (recipe, staging) = staged(&path, &registry);
        assert!(staging.cards.iter().all(Option::is_none), "{path:?}");
        assert!(staging.images.is_empty(), "{path:?}");
        assert!(
            staging.clips.iter().all(|clip| *clip == [[0.0; 4]; 2]),
            "{path:?}"
        );
        let lens = eyepiece(manfred_scene::camera(&recipe, camera).unwrap(), view, size).unwrap();
        assert_eq!(staging.placements(&lens.eye), plain(&staging), "{path:?}");
        let staged = staging.stage(lens).unwrap();
        assert!(staged.detail.content_slots.is_empty(), "{path:?}");
    }
}

fn traced_film(
    gpu: &Gpu,
    (recipe, staging): (&Recipe, &Staging),
    camera: &str,
    view: View,
    size: [u32; 2],
    samples: u32,
) -> Film {
    let lens = eyepiece(manfred_scene::camera(recipe, camera).unwrap(), view, size).unwrap();
    let staged = staging.stage(lens).unwrap();
    trace(
        gpu,
        &staged,
        size,
        samples,
        SEED,
        &mut |_| (),
        &mut turn_reporting,
    )
    .unwrap()
    .0
}

fn film_bytes(film: &Film) -> Vec<u8> {
    film.color
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn the_table_through_a_lens_and_as_an_equirect_traces_to_the_same_bytes() {
    let _gpu = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let registry = Registry::new();
    let gpu = headless().unwrap();
    for (path, camera, view, size) in goldens() {
        let (recipe, staging) = staged(&path, &registry);
        let now = traced_film(&gpu, (&recipe, &staging), camera, view, size, 8);
        let lens = eyepiece(manfred_scene::camera(&recipe, camera).unwrap(), view, size).unwrap();
        let meshes = staging.meshes();
        let placements = plain(&staging);
        let mut before = Stage {
            meshes: &meshes,
            instances: &placements,
            materials: &staging.materials,
            sky: staging.sky.clone(),
            sun: staging.sun,
            camera: lens.camera,
            projection: lens.projection,
            lens: lens.lens,
        }
        .build()
        .unwrap();
        for (instance, piece) in before.detail.instances.iter_mut().zip(&staging.pieces) {
            instance.shadow_only = piece.shadow == manfred_recipe::Shadow::Only;
        }
        before.detail.lights = staging.lights.clone();
        before.detail.sun_radius_deg = staging.sun_radius_deg;
        let (then, _) = trace(
            &gpu,
            &before,
            size,
            8,
            SEED,
            &mut |_| (),
            &mut turn_reporting,
        )
        .unwrap();
        println!(
            "{}: {} pixels, the same bytes",
            path.display(),
            now.color.len()
        );
        assert_eq!(film_bytes(&now), film_bytes(&then), "{path:?}");
        turn();
    }
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

fn luma(c: [f64; 3]) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn a_traced_card_scene_matches_its_live_shot() {
    let _gpu = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let file = scene("tracecards-gpu");
    let (recipe, staging) = staged(&file, &Registry::new());
    let film = traced_film(
        &headless().unwrap(),
        (&recipe, &staging),
        "main",
        View::Ortho,
        SIZE,
        SAMPLES,
    );
    let traced = looked(&film, &staging.finish).unwrap();
    write_png(&root().join("tmp/tracecards-traced.png"), SIZE, &traced).unwrap();
    turn();
    let live = still(&file, "main", SIZE).unwrap();
    write_png(&root().join("tmp/tracecards-live.png"), SIZE, &live).unwrap();
    let mut worst: f64 = 0.0;
    for (name, at) in REGIONS {
        let [t, l] = [mean(&traced, at), mean(&live, at)];
        let off = (0..3).map(|c| (t[c] - l[c]).abs()).fold(0.0, f64::max);
        worst = worst.max(off);
        println!("{name}: traced {t:.1?}, live {l:.1?}, off by {off:.1}");
    }
    let t = |name: &str| mean(&traced, REGIONS.iter().find(|r| r.0 == name).unwrap().1);
    let [r, g, b] = t("photo");
    assert!(r > 2.0 * g && r > 2.0 * b, "photo {:?}", [r, g, b]);
    let [r, g, b] = t("sprite");
    assert!(g > 2.0 * r && g > 2.0 * b, "sprite {:?}", [r, g, b]);
    let blue = |c: [f64; 3]| c[2] > 2.0 * c[0] && c[2] > c[1];
    assert!(
        blue(t("sprite's cut corner")),
        "{:?}",
        t("sprite's cut corner")
    );
    assert!(
        blue(t("slider above the plane")),
        "{:?}",
        t("slider above the plane")
    );
    let [r, g, b] = t("slider below the plane");
    assert!(r > 2.0 * b && g > b, "slider {:?}", [r, g, b]);
    let [r, g, b] = t("feed");
    assert!(
        r > 20.0 && (r - g).abs() * 5.0 < r && b < 2.0 * r,
        "feed {:?}",
        [r, g, b]
    );
    let [r, g, b] = t("sign's turned edge");
    assert!(
        r > 2.0 * b && g > 2.0 * b,
        "the sign turned to the camera {:?}",
        [r, g, b]
    );
    let open = luma(t("open backdrop"));
    let ghost = luma(t("under the ghost"));
    let solid = luma(t("under the solid box"));
    println!(
        "backdrop luma: open {open:.1}, under the ghost {ghost:.1}, under the solid box {solid:.1}"
    );
    assert!((ghost - open).abs() < 0.1 * open, "{ghost} {open}");
    assert!(solid < 0.75 * open, "{solid} {open}");
    println!("the worst region is off by {worst:.1} of 255");
    assert!(worst <= TOLERANCE, "{worst}");
    turn();
}

const SURFACES: &str = r#"manfred = 1
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

fn surfaces_file() -> PathBuf {
    let dir = scratch("engine19-cards");
    png(&dir.join("red.png"), 32, |_, _| [230, 20, 20, 255]);
    png(&dir.join("disc.png"), 64, disc);
    let file = dir.join("surfaces.toml");
    std::fs::write(&file, SURFACES).unwrap();
    file
}

fn region(image: &[u8], size: [u32; 2], span: f64, at: [f64; 2]) -> [f64; 3] {
    let x = ((at[0] / span + 0.5) * f64::from(size[0])) as i64;
    let y = ((0.5 - at[1] / span) * f64::from(size[1])) as i64;
    let mut sum = [0.0; 3];
    let mut count = 0.0;
    for dy in -3..=3 {
        for dx in -3..=3 {
            let px = x + dx;
            let py = y + dy;
            if px < 0 || py < 0 || px >= size[0] as i64 || py >= size[1] as i64 {
                continue;
            }
            let k = (py as usize * size[0] as usize + px as usize) * 4;
            for c in 0..3 {
                sum[c] += f64::from(image[k + c]);
            }
            count += 1.0;
        }
    }
    sum.map(|v| v / count)
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn the_cards_scene_traces_beside_its_live_shot() {
    let _gpu = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let file = surfaces_file();
    let (recipe, staging) = staged(&file, &Registry::new());
    let size = [400, 400];
    let film = traced_film(
        &headless().unwrap(),
        (&recipe, &staging),
        "main",
        View::Ortho,
        size,
        SAMPLES,
    );
    let traced = looked(&film, &staging.finish).unwrap();
    write_png(&root().join("tmp/engine19-cards-traced.png"), size, &traced).unwrap();
    turn();
    let live = still(&file, "main", size).unwrap();
    write_png(&root().join("tmp/engine19-cards-live.png"), size, &live).unwrap();
    let probes = [
        ("photo", [-0.5, 0.5]),
        ("sprite", [0.5, 0.5]),
        ("sprite's cut corner", [0.83, 0.83]),
        ("feed", [-0.5, -0.5]),
        ("slider below the plane", [0.5, -0.65]),
        ("slider above the plane", [0.5, -0.35]),
        ("backdrop", [0.0, 0.0]),
    ];
    let mut worst = 0.0f64;
    for (name, at) in probes {
        let t = region(&traced, size, 2.0, at);
        let l = region(&live, size, 2.0, at);
        let off = (0..3).map(|c| (t[c] - l[c]).abs()).fold(0.0, f64::max);
        worst = worst.max(off);
        println!("{name}: traced {t:.1?}, live {l:.1?}, off by {off:.1}");
    }
    println!(
        "the cards scene, 7x7 means, worst {worst:.1} of 255 against a tolerance of {TOLERANCE}"
    );
    assert!(worst <= TOLERANCE, "{worst}");
    turn();
}

const RAMP: &str = r#"manfred = 1
name = "ramp"
seed = 3

[[camera]]
name = "main"
kind = "orthographic"
position = [0.0, 0.0, 4.0]
target = [0.0, 0.0, 0.0]
up = [0.0, 1.0, 0.0]
ortho_height = 1.2

[sun]
model = "authored"
toward = [0.0, 0.2, 1.0]
irradiance = 3.0

[materials.floor]
base = [0.05, 0.1, 0.7]
roughness = 1.0
specular = 0.0

[materials.sheet]
base = [0.9, 0.15, 0.1]
roughness = 1.0
specular = 0.0

[[object]]
name = "floor"
kind = "box"
size = [4.0, 4.0, 0.1]
at = [0.0, 0.0, -0.3]
material = "floor"

[[object]]
name = "card"
kind = "card"
size = [1.0, 1.0]
image = "ramp.png"
alpha_cutoff = 0.25
material = "sheet"
"#;

fn ramp_file(cutoff: &str) -> PathBuf {
    let dir = scratch("engine19-ramp");
    png(&dir.join("ramp.png"), 64, |x, _| {
        let alpha = (x * 255 / 63) as u8;
        [220, 30, 20, alpha]
    });
    let file = dir.join("ramp.toml");
    std::fs::write(
        &file,
        RAMP.replace("alpha_cutoff = 0.25", &format!("alpha_cutoff = {cutoff}")),
    )
    .unwrap();
    file
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn a_traced_card_matches_live_at_authored_cutoffs() {
    let _gpu = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let size = [200, 200];
    let span = 1.2;
    let probes = [(-0.45, "clear"), (0.0, "middle"), (0.46, "solid")];
    let mut shots = Vec::new();
    for cutoff in ["0.25", "0.9"] {
        let file = ramp_file(cutoff);
        let (recipe, staging) = staged(&file, &Registry::new());
        let eye = eyepiece(
            manfred_scene::camera(&recipe, "main").unwrap(),
            View::Ortho,
            size,
        )
        .unwrap()
        .eye;
        let (authored, cuts) = {
            let placed = staging.placements(&eye);
            let card = placed.iter().find(|p| p.content.is_some()).unwrap();
            (card.alpha_cutoff, card.content.unwrap().cutout)
        };
        assert_eq!(authored, cutoff.parse::<f32>().unwrap());
        assert!(cuts);
        let film = traced_film(
            &headless().unwrap(),
            (&recipe, &staging),
            "main",
            View::Ortho,
            size,
            64,
        );
        let traced = looked(&film, &staging.finish).unwrap();
        turn();
        let live = still(&file, "main", size).unwrap();
        let mut worst = 0.0f64;
        let mut middle = [0.0; 3];
        for (at, name) in probes {
            let t = region(&traced, size, span, [at, 0.0]);
            let l = region(&live, size, span, [at, 0.0]);
            let off = (0..3).map(|c| (t[c] - l[c]).abs()).fold(0.0, f64::max);
            worst = worst.max(off);
            if name == "middle" {
                middle = l;
            }
            println!("cutoff {cutoff} {name}: traced {t:.1?}, live {l:.1?}, off by {off:.1}");
        }
        println!(
            "cutoff {cutoff}: worst region {worst:.1} of 255, tolerance {TOLERANCE} on a 7x7 mean"
        );
        assert!(worst <= TOLERANCE, "cutoff {cutoff}: {worst}");
        shots.push(middle);
        turn();
    }
    let apart = (0..3)
        .map(|c| (shots[0][c] - shots[1][c]).abs())
        .fold(0.0, f64::max);
    println!(
        "the middle of the ramp differs by {apart:.1} on a channel between cutoffs 0.25 and 0.9"
    );
    assert!(apart > TOLERANCE, "{apart}");
}
