use manfred_build::Registry;
use manfred_recipe::{Projection as CameraProjection, Recipe, load};
use manfred_render::{
    Eyepiece, IDENTITY, Lettering, Overlay, Staging, View, Way, eyepiece, looked, looked_with,
    trace,
};
use manfred_scene::{headless, still, write_png};
use pfx::adapter::{turn, turn_reporting};
use pfx_materials::{Blend, ContentLayer, Material};
use pfx_trace::detail::ContentFace;
use pfx_trace::stage::{ContentKind, Mesh, Placement, PlacementContent};
use pfx_trace::text;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

const SIZE: [u32; 2] = [800, 500];
const SAMPLES: u32 = 128;
const SEED: u32 = 11;
const CHANGED: u8 = 24;
const IOU_FLOOR: f64 = 0.94;
const COLOUR_TOLERANCE: f64 = 12.0;

const SCENE: &str = r#"manfred = 1
name = "tracetext"
seed = 5

[[camera]]
name = "main"
position = [0.0, 0.9, 2.0]
target = [0.0, 0.2, 0.0]
fov_y_deg = 38.0

[sun]
model = "authored"
toward = [0.3, 0.8, 0.6]
irradiance = 3.0

[materials.floor]
base = [0.2, 0.2, 0.22]
roughness = 1.0
specular = 0.0

[materials.crate]
base = [0.1, 0.25, 0.6]
roughness = 1.0
specular = 0.0

[materials.plate]
base = [0.75, 0.75, 0.7]
roughness = 1.0
specular = 0.0

[[object]]
name = "floor"
kind = "box"
size = [4.0, 0.1, 4.0]
at = [0.0, -0.05, 0.0]
material = "floor"

[[object]]
name = "crate"
kind = "box"
size = [0.5, 0.3, 0.3]
at = [-0.45, 0.15, 0.25]
material = "crate"

[[object]]
name = "plate"
kind = "plane"
size = [0.6, 0.4]
at = [0.45, 0.16, 0.3]
rotate = [40.0, 0.0, 0.0]
material = "plate"
"#;

const TEXTS: &str = r#"
[[object]]
name = "sign"
kind = "text"
at = [-0.42, 0.62, -0.3]
spans = [{ text = "UNLIT", size = 0.13, weight = 700, color = [1.0, 0.8, 0.1] }]

[[object]]
name = "poster"
kind = "text"
space = "lit"
at = [0.42, 0.62, -0.3]
spans = [{ text = "LIT", size = 0.15, weight = 700, color = [0.95, 0.3, 0.3] }]

[[object]]
name = "label"
kind = "text"
space = "lit"
on = "crate"
spans = [{ text = "BOX", size = 0.11, weight = 700, color = [1.0, 1.0, 1.0] }]

[[object]]
name = "mark"
kind = "text"
space = "lit"
on = "plate"
spans = [{ text = "PLATE", size = 0.09, weight = 700, color = [0.05, 0.05, 0.05] }]

[[object]]
name = "title"
kind = "text"
space = "overlay"
at = [16.0, 12.0, 0.0]
spans = [{ text = "manfred render", size = 32.0, weight = 700, color = [1.0, 1.0, 1.0] }]
"#;

const CARD: &str = r#"
[[object]]
name = "badge"
kind = "card"
size = [0.2, 0.2]
at = [0.0, 0.3, 0.6]
image = "grey.png"
material = "plate"

[[object]]
name = "stamp"
kind = "text"
space = "lit"
on = "badge"
spans = [{ text = "OK", size = 0.06, weight = 700, color = [0.9, 0.1, 0.1] }]
"#;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn scene(name: &str, text: &str) -> PathBuf {
    let dir = root().join("tmp").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("tracetext.toml");
    std::fs::write(&file, text).unwrap();
    file
}

fn staged(path: &Path, registry: &Registry) -> (Recipe, Staging) {
    let recipe = load(path).unwrap().recipe;
    let built = manfred_build::build_with(&recipe, path.parent().unwrap(), registry).unwrap();
    let recipe = built.recipe(&recipe);
    let staging = Staging::of(&recipe, &built.parts).unwrap();
    (recipe, staging)
}

fn lens(recipe: &Recipe) -> Eyepiece {
    eyepiece(
        manfred_scene::camera(recipe, "main").unwrap(),
        View::Lens,
        SIZE,
    )
    .unwrap()
}

fn object(recipe: &Recipe, name: &str) -> usize {
    recipe.objects.iter().position(|o| o.name == name).unwrap()
}

fn piece(recipe: &Recipe, staging: &Staging, name: &str) -> usize {
    let object = object(recipe, name);
    staging
        .pieces
        .iter()
        .position(|piece| piece.object == object)
        .unwrap()
}

fn same(a: &Mesh<'_>, b: &Mesh<'_>) -> bool {
    a.positions == b.positions
        && a.normals == b.normals
        && a.tangents == b.tangents
        && a.uvs == b.uvs
        && a.alpha == b.alpha
        && a.indices == b.indices
}

fn model(staging: &Staging, recipe: &Recipe, name: &str) -> [[f32; 4]; 4] {
    let object = object(recipe, name);
    staging
        .text
        .items
        .iter()
        .find(|item| item.object == object)
        .and_then(|item| item.world())
        .unwrap()
        .0
}

#[test]
fn each_kind_of_text_takes_the_engines_way() {
    let file = scene("tracetext-cpu", &format!("{SCENE}{TEXTS}"));
    let (recipe, staging) = staged(&file, &Registry::new());
    let lens = lens(&recipe);
    let lettering = Lettering::of(&staging).unwrap();
    let names: Vec<&str> = lettering.letters.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["sign", "poster", "label", "mark"]);
    for letter in &lettering.letters {
        let glyphs = match &letter.way {
            Way::Carrier { lettering, .. }
            | Way::Fit { lettering, .. }
            | Way::Over { lettering, .. } => lettering.glyphs.len(),
        };
        println!(
            "{}: {glyphs} glyphs, {} bytes of lettering",
            letter.name,
            letter.bytes()
        );
    }
    let way = |name: &str| {
        &lettering
            .letters
            .iter()
            .find(|l| l.name == name)
            .unwrap()
            .way
    };
    for (name, unlit) in [("sign", true), ("poster", false), ("label", false)] {
        let Way::Carrier {
            lettering,
            model,
            lit,
            ..
        } = way(name)
        else {
            panic!("{name} is staged on a carrier");
        };
        assert_eq!(*lit, !unlit, "{name}");
        assert_eq!(*model, self::model(&staging, &recipe, name), "{name}");
        assert!(
            matches!(lettering.content().kind, ContentKind::Text(_)),
            "{name}"
        );
    }
    let crate_piece = piece(&recipe, &staging, "crate");
    let meshes = staging.meshes();
    let fit = text::fit(
        &meshes[crate_piece],
        IDENTITY,
        model(&staging, &recipe, "label"),
    )
    .unwrap();
    let Way::Carrier { lettering: ink, .. } = way("label") else {
        unreachable!()
    };
    let shared = fit.elsewhere(&meshes[crate_piece], &fit.content(ink));
    assert_eq!(shared.len(), 10, "the box's other triangles share its UVs");
    let plate = piece(&recipe, &staging, "plate");
    let Way::Fit {
        piece,
        lettering: ink,
        fit,
    } = way("mark")
    else {
        panic!("the plate's text is fitted to the plate");
    };
    assert_eq!(*piece, plate);
    assert!(fit.elsewhere(&meshes[plate], &fit.content(ink)).is_empty());
    assert!(matches!(fit.content(ink).kind, ContentKind::Text(_)));

    let mut meshes = staging.meshes();
    let mut placements = staging.placements(&lens.eye);
    let mut materials = staging.materials.clone();
    let before = placements.clone();
    lettering.stage(&mut meshes, &mut placements, &mut materials);
    let pieces = staging.pieces.len();
    assert_eq!(meshes.len(), pieces + 3);
    assert_eq!(placements.len(), pieces + 3);
    for at in (0..pieces).filter(|&at| at != plate) {
        assert_eq!(placements[at], before[at]);
    }
    let fitted = placements[plate];
    let content = fitted.content.unwrap();
    assert_eq!(content, fit.content(ink));
    assert!(!content.cutout && content.face == ContentFace::Both);
    assert_eq!(
        materials[fitted.material as usize],
        Material {
            content_layer: ContentLayer::default(),
            ..staging.materials[before[plate].material as usize]
        }
    );
    assert_eq!(
        materials[fitted.material as usize].content_layer.blend,
        Blend::Over
    );
    for (at, name) in (pieces..).zip(["sign", "poster", "label"]) {
        let carrier = placements[at];
        let Way::Carrier { lettering, lit, .. } = way(name) else {
            unreachable!()
        };
        assert_eq!(carrier.mesh as usize, at, "{name}");
        assert_eq!(carrier.model, model(&staging, &recipe, name), "{name}");
        assert!(!carrier.casts_shadow, "{name}");
        assert_eq!(carrier.content, Some(lettering.content()), "{name}");
        assert!(carrier.content.unwrap().cutout, "{name}");
        assert_eq!(
            carrier.alpha_cutoff,
            Placement::new(0, IDENTITY, 0).alpha_cutoff,
            "{name}"
        );
        assert_eq!(
            materials[carrier.material as usize],
            text::material(*lit),
            "{name}"
        );
        assert!(same(&meshes[at], &lettering.carrier().mesh()), "{name}");
    }
    let staged = staging.stage(lens).unwrap();
    assert_eq!(staged.detail.instances.len(), pieces + 3);
    assert_eq!(
        staged
            .detail
            .content_slots
            .iter()
            .filter(|slot| slot.is_some())
            .count(),
        0
    );
    assert_eq!(
        staged
            .detail
            .text_slots
            .iter()
            .filter(|slot| slot.is_some())
            .count(),
        4
    );
}

#[test]
fn overlay_text_is_painted_after_the_finish_only_where_it_lies() {
    let file = scene("tracetext-overlay", &format!("{SCENE}{TEXTS}"));
    let (_, staging) = staged(&file, &Registry::new());
    let overlay = Overlay::of(&staging.text);
    assert_eq!(overlay.laid.items.len(), 1);
    assert_eq!(overlay.laid.items[0].name, "title");
    let boxes = staging.text.overlay_boxes(SIZE);
    let [left, top, right, bottom] = boxes[0].1;
    let mut pixels = vec![[0.1, 0.2, 0.3, 1.0]; (SIZE[0] * SIZE[1]) as usize];
    overlay.paint(&mut pixels, SIZE).unwrap();
    let mut painted = 0;
    for (index, pixel) in pixels.iter().enumerate() {
        if *pixel == [0.1, 0.2, 0.3, 1.0] {
            continue;
        }
        painted += 1;
        let x = (index as u32 % SIZE[0]) as f32 + 0.5;
        let y = (index as u32 / SIZE[0]) as f32 + 0.5;
        assert!(
            x > left - 1.0 && x < right + 1.0 && y > top - 1.0 && y < bottom + 1.0,
            "{x} {y}"
        );
    }
    assert!(painted > 500, "{painted}");
    let film = manfred_render::Film {
        size: SIZE,
        color: vec![[0.1, 0.2, 0.3, 1.0]; (SIZE[0] * SIZE[1]) as usize],
        albedo: Vec::new(),
        normal: Vec::new(),
        samples: 1,
    };
    let bare = manfred_render::looked_with(&film, &staging.finish, false).unwrap();
    let empty = Overlay::default();
    assert_eq!(
        manfred_render::looked_with(&film, (&staging.finish, &empty), false).unwrap(),
        bare
    );
    let titled = manfred_render::looked_with(&film, (&staging.finish, &overlay), false).unwrap();
    let differ = titled
        .chunks_exact(4)
        .zip(bare.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert!(differ > 500 && differ <= painted, "{differ} {painted}");
}

fn grey(dir: &Path, side: u32) {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, side, side);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer
        .write_image_data(&[128u8, 128, 128, 255].repeat((side * side) as usize))
        .unwrap();
    writer.finish().unwrap();
    std::fs::write(dir.join("grey.png"), out).unwrap();
}

fn inked(content: PlacementContent<'_>) -> usize {
    let ContentKind::Text(text) = content.kind else {
        panic!("the stamp is lettered over the picture");
    };
    let steps = 64;
    (0..steps)
        .flat_map(|y| (0..steps).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let uv = [
                (x as f32 + 0.5) / steps as f32,
                (y as f32 + 0.5) / steps as f32,
            ];
            let pixel = text.at(uv);
            pixel[0] > pixel[1] + 0.15 && pixel[3] > 0.2
        })
        .count()
}

#[test]
fn a_lit_text_on_a_card_is_lettered_over_its_picture() {
    let file = scene("tracetext-card", &format!("{SCENE}{CARD}"));
    let side = 1024u32;
    grey(file.parent().unwrap(), side);
    let (recipe, staging) = staged(&file, &Registry::new());
    let lens = lens(&recipe);
    let lettering = Lettering::of(&staging).unwrap();
    let badge = piece(&recipe, &staging, "badge");
    let Way::Over {
        piece: on,
        lettering: ink,
        fit,
    } = &lettering.letters[0].way
    else {
        panic!("the stamp is lettered over the picture");
    };
    assert_eq!(*on, badge);
    let mut meshes = staging.meshes();
    let mut placements = staging.placements(&lens.eye);
    let mut materials = staging.materials.clone();
    let before = placements[badge];
    lettering.stage(&mut meshes, &mut placements, &mut materials);
    assert_eq!(placements.len(), staging.pieces.len());
    assert_eq!(meshes.len(), staging.pieces.len());
    let picture = before.content.unwrap();
    let content = placements[badge].content.unwrap();
    assert_eq!(content, fit.over(&picture, ink).unwrap());
    let ContentKind::Text(text) = content.kind else {
        panic!("the stamp is lettered over the picture");
    };
    assert_eq!(text.picture.unwrap().width, side);
    let hits = inked(content);
    assert!(hits > 20, "{hits}");
    assert_eq!(
        Placement {
            content: Some(content),
            ..before
        },
        placements[badge]
    );

    let small = scene("tracetext-card-small", &format!("{SCENE}{CARD}"));
    grey(small.parent().unwrap(), 16);
    let (recipe, staging) = staged(&small, &Registry::new());
    let lens = self::lens(&recipe);
    let lettering = Lettering::of(&staging).unwrap();
    let Way::Over {
        lettering: ink,
        fit,
        ..
    } = &lettering.letters[0].way
    else {
        panic!("a coarse picture still carries the text over it");
    };
    let mut meshes = staging.meshes();
    let mut placements = staging.placements(&lens.eye);
    let mut materials = staging.materials.clone();
    let badge = piece(&recipe, &staging, "badge");
    let before = placements[badge];
    lettering.stage(&mut meshes, &mut placements, &mut materials);
    assert_eq!(placements.len(), staging.pieces.len());
    let content = placements[badge].content.unwrap();
    assert_eq!(content, fit.over(&before.content.unwrap(), ink).unwrap());
    let ContentKind::Text(text) = content.kind else {
        panic!("a coarse picture still carries the text over it");
    };
    assert_eq!(text.picture.unwrap().width, 16);
}

#[test]
fn a_recipe_without_text_stages_as_before() {
    let registry = Registry::new();
    for (path, camera, view) in [
        (
            root().join("tests/golden/table/table.toml"),
            "main",
            View::Lens,
        ),
        (
            root().join("tests/golden/table/table.toml"),
            "main",
            View::Equirect,
        ),
    ] {
        let (recipe, staging) = staged(&path, &registry);
        assert!(staging.text.items.is_empty(), "{path:?}");
        let lens = eyepiece(
            manfred_scene::camera(&recipe, camera).unwrap(),
            view,
            [200, 100],
        )
        .unwrap();
        let lettering = Lettering::of(&staging).unwrap();
        assert!(lettering.letters.is_empty());
        let mut meshes = staging.meshes();
        let mut placements = staging.placements(&lens.eye);
        let mut materials = staging.materials.clone();
        lettering.stage(&mut meshes, &mut placements, &mut materials);
        let plain = staging.meshes();
        assert_eq!(meshes.len(), plain.len(), "{path:?}");
        assert!(
            meshes.iter().zip(&plain).all(|(a, b)| same(a, b)),
            "{path:?}"
        );
        assert_eq!(placements, staging.placements(&lens.eye), "{path:?}");
        assert_eq!(materials, staging.materials, "{path:?}");
        assert!(Overlay::of(&staging.text).is_empty());
        let staged = staging.stage(lens).unwrap();
        assert_eq!(staged.detail.instances.len(), staging.pieces.len());
        assert!(staged.detail.text_slots.is_empty(), "{path:?}");
    }
}

fn changed(a: &[u8], b: &[u8]) -> Vec<bool> {
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .map(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > CHANGED))
        .collect()
}

fn projected(lens: &Eyepiece, p: [f64; 3]) -> [f32; 2] {
    let camera = lens.camera;
    let d: [f32; 3] = std::array::from_fn(|k| p[k] as f32 - camera.origin[k]);
    let dot = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>();
    let depth = dot(d, camera.forward);
    let x = dot(d, camera.right) / (dot(camera.right, camera.right) * depth);
    let y = dot(d, camera.up) / (dot(camera.up, camera.up) * depth);
    [
        (x * 0.5 + 0.5) * SIZE[0] as f32,
        (0.5 - y * 0.5) * SIZE[1] as f32,
    ]
}

fn regions(staging: &Staging, lens: &Eyepiece) -> Vec<(usize, [f32; 4])> {
    let mut out: Vec<(usize, [f32; 4])> = staging
        .text
        .surface_boxes()
        .into_iter()
        .map(|(object, corners)| {
            let points = corners.map(|c| projected(lens, c));
            let low = [0, 1].map(|k| points.iter().map(|p| p[k]).fold(f32::INFINITY, f32::min));
            let high = [0, 1].map(|k| {
                points
                    .iter()
                    .map(|p| p[k])
                    .fold(f32::NEG_INFINITY, f32::max)
            });
            (object, [low[0], low[1], high[0], high[1]])
        })
        .collect();
    out.extend(staging.text.overlay_boxes(SIZE));
    out.into_iter()
        .map(|(object, [l, t, r, b])| (object, [l - 6.0, t - 6.0, r + 6.0, b + 6.0]))
        .collect()
}

fn traced(gpu: &pfx_gpu::Gpu, path: &Path) -> Vec<u8> {
    let (recipe, staging) = staged(path, &Registry::new());
    let lens = lens(&recipe);
    let staged = staging.stage(lens).unwrap();
    let (film, paced) = trace(
        gpu,
        &staged,
        SIZE,
        SAMPLES,
        SEED,
        &mut |_| (),
        &mut turn_reporting,
    )
    .unwrap();
    println!(
        "{}: {} submissions, the longest {:.2} ms, {:.1} ms on the GPU, {:.3} ms a sample",
        path.parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("trace"),
        paced.submissions,
        paced.longest_ms,
        paced.gpu_ms,
        paced.gpu_ms / f64::from(SAMPLES)
    );
    looked(&film, (&staging.finish, &Overlay::of(&staging.text))).unwrap()
}

#[test]
#[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
fn traced_text_covers_what_the_live_shot_draws() {
    let _gpu = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    turn();
    let with = scene("tracetext-gpu", &format!("{SCENE}{TEXTS}"));
    let without = scene("tracetext-gpu-bare", SCENE);
    let gpu = headless().unwrap();
    let traced_text = traced(&gpu, &with);
    turn();
    let traced_bare = traced(&gpu, &without);
    turn();
    write_png(&root().join("tmp/tracetext-traced.png"), SIZE, &traced_text).unwrap();
    let live_text = still(&with, "main", SIZE).unwrap();
    turn();
    let live_bare = still(&without, "main", SIZE).unwrap();
    turn();
    write_png(&root().join("tmp/tracetext-live.png"), SIZE, &live_text).unwrap();
    let ours = changed(&traced_text, &traced_bare);
    let theirs = changed(&live_text, &live_bare);
    let (recipe, staging) = staged(&with, &Registry::new());
    let lens = lens(&recipe);
    let mut worst_iou: f64 = 1.0;
    let mut worst_colour: f64 = 0.0;
    for (object, [left, top, right, bottom]) in regions(&staging, &lens) {
        let name = &recipe.objects[object].name;
        let (mut both, mut either, mut count) = (0usize, 0usize, 0.0);
        let mut sums = [[0.0f64; 3]; 2];
        for y in (top.max(0.0) as u32)..(bottom.min(SIZE[1] as f32) as u32) {
            for x in (left.max(0.0) as u32)..(right.min(SIZE[0] as f32) as u32) {
                let at = (y * SIZE[0] + x) as usize;
                if ours[at] || theirs[at] {
                    either += 1;
                }
                if ours[at] && theirs[at] {
                    both += 1;
                    count += 1.0;
                    for k in 0..3 {
                        sums[0][k] += f64::from(traced_text[at * 4 + k]);
                        sums[1][k] += f64::from(live_text[at * 4 + k]);
                    }
                }
            }
        }
        let iou = both as f64 / either.max(1) as f64;
        let [t, l] = sums.map(|s| s.map(|v| v / f64::max(count, 1.0)));
        let off = (0..3).map(|k| (t[k] - l[k]).abs()).fold(0.0, f64::max);
        println!(
            "{name}: {either} pixels of text, IoU {iou:.3}, mean traced {t:.1?}, live {l:.1?}, off by {off:.1}"
        );
        assert!(either > 300, "{name}: {either} pixels");
        worst_iou = worst_iou.min(iou);
        worst_colour = worst_colour.max(off);
    }
    println!("the worst IoU is {worst_iou:.3}, the worst colour {worst_colour:.1} of 255");
    assert!(worst_iou >= IOU_FLOOR, "{worst_iou}");
    assert!(worst_colour <= COLOUR_TOLERANCE, "{worst_colour}");
    turn();
}

#[test]
#[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
fn a_word_at_eight_times_zoom_keeps_a_hard_edge() {
    let _gpu = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    turn();
    let file = scene(
        "tracetext-zoom",
        &format!("{SCENE}{TEXTS}\n[look]\nview = \"standard\"\nexposure = 1.0\n"),
    );
    let (recipe, staging) = staged(&file, &Registry::new());
    let lens = lens(&recipe);
    let sign = object(&recipe, "sign");
    let corners = staging
        .text
        .surface_boxes()
        .into_iter()
        .find(|(id, _)| *id == sign)
        .unwrap()
        .1;
    let points = corners.map(|corner| projected(&lens, corner));
    let px = |axis: usize, most: bool| {
        points.iter().map(|point| point[axis]).fold(
            if most {
                f32::NEG_INFINITY
            } else {
                f32::INFINITY
            },
            |a, b| {
                if most { a.max(b) } else { a.min(b) }
            },
        )
    };
    let world = |axis: usize, most: bool| {
        corners.iter().map(|point| point[axis]).fold(
            if most {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            },
            |a, b| {
                if most { a.max(b) } else { a.min(b) }
            },
        )
    };
    let px_h = px(1, true) - px(1, false);
    let world_w = world(0, true) - world(0, false);
    let world_h = world(1, true) - world(1, false);
    assert!(px_h > 10.0 && px_h < 160.0, "{px_h}");
    assert!(world_w > 0.01 && world_h > 0.01, "{world_w} {world_h}");
    let margin = 1.35;
    let height = (f64::from(px_h) * 8.0 * margin).round() as u32;
    let width = (f64::from(height) * world_w / world_h).round() as u32;
    assert!((64..1400).contains(&height) && (64..1800).contains(&width));
    let centre = [0, 1, 2].map(|axis| (world(axis, false) + world(axis, true)) * 0.5);
    let mut camera = recipe.cameras[0].clone();
    camera.name = "close".into();
    camera.kind = CameraProjection::Orthographic;
    camera.position = [centre[0], centre[1], centre[2] + 0.4];
    camera.target = centre;
    camera.up = [0.0, 1.0, 0.0];
    camera.shift = [0.0, 0.0];
    camera.ortho_height = Some(world_h * margin);
    let size = [width, height];
    let close = eyepiece(&camera, View::Ortho, size).unwrap();
    println!(
        "zoom {}x{}, the word was {px_h:.0} px tall in the shot, ortho height {:.3} m",
        size[0],
        size[1],
        world_h * margin
    );
    let gpu = headless().unwrap();
    let staged = staging.stage(close).unwrap();
    let samples = 64;
    let (film, paced) = trace(
        &gpu,
        &staged,
        size,
        samples,
        SEED,
        &mut |_| (),
        &mut turn_reporting,
    )
    .unwrap();
    println!(
        "zoom: {} submissions, the longest {:.2} ms, {:.3} ms a sample",
        paced.submissions,
        paced.longest_ms,
        paced.gpu_ms / f64::from(samples)
    );
    let png = looked_with(&film, &staging.finish, false).unwrap();
    write_png(&root().join("tmp/lettering-zoom.png"), size, &png).unwrap();
    let background = [png[0], png[1], png[2]];
    let ink = png
        .chunks_exact(4)
        .filter(|pixel| (0..3).any(|channel| pixel[channel].abs_diff(background[channel]) > 40))
        .count();
    let total = (size[0] * size[1]) as usize;
    println!("zoom: {ink} ink pixels of {total}");
    assert!(ink > total / 30 && ink < total * 3 / 4, "{ink} {total}");
    turn();
}
