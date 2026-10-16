mod common;

use common::{golden, scratch};
use manfred_build::{Registry, write};
use manfred_render::{Request, View, looked_with, trace};
use manfred_scene::{SETTLE_FRAMES, difference, headless, psnr, shoot, write_png};
use pfx::adapter::{TurnState, turn_reporting, turn_soon};
use pfx_gpu::{Gpu, wgpu};
use pfx_live::renderer::{Exposure, Renderer};
use pfx_load::scene::Scene;
use std::path::{Path, PathBuf};

const SIZE: [u32; 2] = [640, 400];
const SAMPLES: u32 = 16;
const SEED: u32 = 7;
const BOX: u32 = 8;

struct Case {
    set: &'static str,
    camera: &'static str,
    overrides: &'static [&'static str],
    live: f64,
    traced: f64,
}

const TABLE: Case = Case {
    set: "table",
    camera: "main",
    overrides: &[],
    live: 40.0,
    traced: 44.0,
};

const TOYBOX: Case = Case {
    set: "toybox",
    camera: "high",
    overrides: &["camera.high.fstop=0.0"],
    live: 30.0,
    traced: 37.0,
};

fn emitted(case: &Case) -> (PathBuf, PathBuf) {
    let folder = golden(case.set);
    let recipe = folder.join(format!("{}.toml", case.set));
    let set: Vec<String> = case.overrides.iter().map(|s| s.to_string()).collect();
    let (mut r, built) = manfred_scene::load(&recipe, None, &set, &Registry::new()).unwrap();
    r.output.scene = Some(format!("{}.scene.toml", case.set));
    r.output.scene_camera = Some(case.camera.to_string());
    assert_eq!(r.output.scene_format, 1);
    let out = scratch(&format!("scene-gpu-{}", case.set));
    write(&built, &r, &folder, Some(&out)).unwrap();
    (recipe, out.join(format!("{}.scene.toml", case.set)))
}

fn overrides(case: &Case) -> Vec<String> {
    case.overrides.iter().map(|s| s.to_string()).collect()
}

fn engine_live(gpu: &Gpu, scene: &Scene) -> Vec<u8> {
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let target = gpu.offscreen(SIZE[0], SIZE[1], format).unwrap();
    let mut renderer = Renderer::new_with_output_format(gpu.clone(), SIZE[0], SIZE[1], format)
        .map_err(|e| format!("{e:?}"))
        .unwrap();
    renderer.set_exposure(Exposure::Fixed(1.0)).unwrap();
    let mut staged = renderer.stage(scene).unwrap();
    let finish = staged.finish();
    let mut turns = TurnState::default();
    for frame in 0..SETTLE_FRAMES {
        let drawn = staged.frame(
            SIZE[0] as f32 / SIZE[1] as f32,
            frame as f32 / manfred_scene::FRAME_RATE,
            SEED,
        );
        let timings = renderer
            .render(
                &drawn.scene,
                &drawn.text,
                &drawn.effects,
                finish,
                &target.view,
            )
            .unwrap();
        let measured: f64 = timings.iter().map(|t| t.milliseconds).sum();
        let measured = if timings.is_empty() {
            manfred_scene::UNMEASURED_MS
        } else {
            measured
        };
        turn_soon(&mut turns, measured);
    }
    gpu.readback_rgba8(&target).unwrap()
}

fn manfred_live(gpu: &Gpu, case: &Case, recipe: &Path) -> Vec<u8> {
    let (r, built) = manfred_scene::load(recipe, None, &overrides(case), &Registry::new()).unwrap();
    shoot(gpu, &r, &built.parts, case.camera, SIZE).unwrap()
}

fn manfred_traced(case: &Case, recipe: &Path) -> Vec<u8> {
    let set = overrides(case);
    let rendered = manfred_render::render(
        &Request {
            recipe,
            camera: case.camera,
            size: SIZE,
            samples: SAMPLES,
            noise: None,
            seed: Some(SEED),
            view: View::Lens,
            variant: None,
            set: &set,
            crop: None,
            denoise: false,
            time: None,
        },
        &Registry::new(),
        &mut |_| {},
    )
    .unwrap();
    looked_with(&rendered.film, &rendered.finish, false).unwrap()
}

fn engine_traced(gpu: &Gpu, scene: &Scene) -> Vec<u8> {
    let staged = pfx_trace::stage::scene(scene, SIZE[0] as f32 / SIZE[1] as f32).unwrap();
    let mut turn = turn_reporting;
    let (film, _) = trace(gpu, &staged, SIZE, SAMPLES, SEED, &mut |_| {}, &mut turn).unwrap();
    let finish = scene.finish.as_ref().map(|f| f.chain.clone()).unwrap();
    looked_with(&film, &finish, false).unwrap()
}

fn boxed(rgba: &[u8]) -> Vec<u8> {
    let [width, height] = SIZE.map(|v| v / BOX);
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0u32; 3];
            for dy in 0..BOX {
                for dx in 0..BOX {
                    let at = (((y * BOX + dy) * SIZE[0] + x * BOX + dx) * 4) as usize;
                    for k in 0..3 {
                        sum[k] += u32::from(rgba[at + k]);
                    }
                }
            }
            let area = BOX * BOX;
            out.extend(sum.map(|v| ((v + area / 2) / area) as u8));
            out.push(255);
        }
    }
    out
}

fn compare(case: &Case, way: &str, manfred: &[u8], engine: &[u8], floor: f64) {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp/scene-gpu")
        .join(case.set);
    write_png(&out.join(format!("{way}-manfred.png")), SIZE, manfred).unwrap();
    write_png(&out.join(format!("{way}-scene.png")), SIZE, engine).unwrap();
    write_png(
        &out.join(format!("{way}-difference.png")),
        SIZE,
        &difference(manfred, engine),
    )
    .unwrap();
    let measured = psnr(manfred, engine).unwrap();
    let smooth = psnr(&boxed(manfred), &boxed(engine)).unwrap();
    println!(
        "{} {way}: PSNR {measured:.2} dB, {smooth:.2} dB over {BOX}x{BOX} boxes",
        case.set
    );
    let measured = if way == "traced" { smooth } else { measured };
    assert!(
        measured >= floor,
        "{} {way}: the scene draws {measured:.2} dB from Manfred's stage, under {floor} dB",
        case.set
    );
}

fn live_and_traced(case: &Case) {
    let (recipe, file) = emitted(case);
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .starts_with("format = 1\n")
    );
    let scene = Scene::open(&file).unwrap();
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    let gpu = headless().unwrap();
    let manfred = manfred_live(&gpu, case, &recipe);
    turn_reporting(Default::default());
    let engine = engine_live(&gpu, &scene);
    turn_reporting(Default::default());
    compare(case, "live", &manfred, &engine, case.live);
    let manfred = manfred_traced(case, &recipe);
    turn_reporting(Default::default());
    let engine = engine_traced(&gpu, &scene);
    turn_reporting(Default::default());
    compare(case, "traced", &manfred, &engine, case.traced);
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn the_tables_scene_staged_by_the_engine_draws_as_manfred_draws_the_recipe() {
    live_and_traced(&TABLE);
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn the_toybox_scene_staged_by_the_engine_draws_as_manfred_draws_the_recipe() {
    live_and_traced(&TOYBOX);
}
