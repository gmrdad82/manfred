mod caller;
mod common;

use caller::{KEEP_REFLECTION, PLATE, SlotPainter, lighting, look};
use manfred_build::build;
use manfred_recipe::parse;
use manfred_runtime::content::{self, Slots};
use manfred_runtime::light::{self, Steam};
use manfred_runtime::raster::{Lens, Plan};
use manfred_runtime::{
    Fill, IDENTITY, Light, NoContent, Parts, Picker, Raster, Route, Scene, Source, load, sky,
};
use pfx::adapter::turn;
use pfx_gpu::{Gpu, wgpu};
use pfx_live::effects::VolumeKind;
use pfx_live::frame::{InstanceSurface, Matrix, MeshHandle};
use pfx_live::maps::ContentFormat;
use pfx_live::probes::ProbeLighting;
use pfx_live::renderer::Picked;
use pfx_materials::{Content, ContentLayer, Material};
use std::path::Path;

const SMALL: &str = "manfred = 1\nname = \"t\"\n\
[[object]]\nname = \"desk\"\nkind = \"box\"\nsize = [0.4, 0.02, 0.3]\nmaterial = \"wood\"\n\n\
[[object]]\nname = \"mv_lid__top\"\nkind = \"box\"\nsize = [0.1, 0.01, 0.1]\nat = [0.0, 0.05, 0.0]\nmaterial = \"film\"\n\n\
[[object]]\nname = \"screen\"\nkind = \"box\"\nsize = [0.1, 0.06, 0.01]\nat = [0.1, 0.05, -0.1]\nmaterial = \"glow\"\n\n\
[[mover]]\ngroup = \"lid\"\npivot = [0.0, 0.05, 0.0]\naxis = [0.0, 0.0, 1.0]\ntravel = 0.02\nclip = [0.0, 1.0, 0.0, 0.08]\nclip2 = [0.0, -1.0, 0.0, 0.0]\n";

const PLAIN: &str = "manfred = 1\nname = \"p\"\n\
[[object]]\nname = \"a\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\nmaterial = \"wood\"\n\n\
[[object]]\nname = \"b\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\nat = [0.2, 0.0, 0.0]\nmaterial = \"wood\"\n";

const SIDECAR: &str = r#"{
  "cameras": [{"position": [0.0, 0.3, 1.0], "target": [0.0, 0.0, 0.0], "fov_y_deg": 20.0}],
  "sun": {"toward": [0.3, 0.8, 0.5], "radius_deg": 0.5, "irradiance": 3.0},
  "motion": {"cup": [0.1, 0.0, 0.0], "radius": 0.02, "box_lo": [-0.1, 0.0, -0.1], "box_hi": [0.1, 0.1, 0.1], "density": 1.2, "anisotropy": 0.3, "dust": 1.0, "mote": 0.0006}
}"#;

fn scene_of(recipe: &str, sidecar: &str) -> Scene {
    let recipe = parse(recipe).unwrap();
    load(&build(&recipe, Path::new(".")).unwrap().glb(), sidecar).unwrap()
}

fn small_lookup(name: &str) -> Material {
    match name {
        "film" => Material {
            base: [0.2, 0.3, 0.4],
            ..Material::default()
        },
        "glow" => Material {
            content: Content::Screen,
            ..Material::default()
        },
        _ => Material::default(),
    }
}

fn index(scene: &Scene, name: &str) -> usize {
    let object = scene.objects.iter().position(|o| o == name).unwrap() as u32;
    scene
        .pieces
        .iter()
        .position(|p| p.object == object)
        .unwrap()
}

#[test]
fn a_small_scene_plans_its_instances_surfaces_and_lights() {
    let scene = scene_of(SMALL, SIDECAR);
    let look = look(&scene);
    let plan = Plan::new(&scene, &look, small_lookup, &["film"]);
    assert_eq!(scene.materials, ["wood", "film", "glow"]);
    assert_eq!(
        plan.materials,
        scene
            .materials
            .iter()
            .map(|n| small_lookup(n))
            .collect::<Vec<_>>()
    );
    assert_eq!(plan.instances.len(), scene.pieces.len());
    for (k, (instance, piece)) in plan.instances.iter().zip(&scene.pieces).enumerate() {
        assert_eq!(instance.mesh, MeshHandle(k as u32));
        assert_eq!(instance.material, piece.material);
        assert_eq!(instance.id, piece.object + 1);
        assert_eq!(instance.model, IDENTITY);
        assert!(instance.casts_shadow && !instance.shadow_only);
    }
    for (mesh, piece) in plan.meshes.iter().zip(&scene.pieces) {
        assert_eq!(mesh.positions, piece.positions);
        assert_eq!(mesh.indices, piece.indices);
        assert_eq!(mesh.tangents.len(), piece.positions.len());
    }
    let (desk, lid, screen) = (
        index(&scene, "desk"),
        index(&scene, "mv_lid__top"),
        index(&scene, "screen"),
    );
    assert_eq!(plan.groups[desk], None);
    assert_eq!(plan.groups[lid], Some(0));
    assert_eq!(plan.groups[screen], None);
    assert_eq!(plan.surfaces[desk], InstanceSurface::default());
    assert_eq!(
        plan.surfaces[lid],
        InstanceSurface {
            clip: [[0.0, 1.0, 0.0, 0.08], [0.0, -1.0, 0.0, 0.0]],
            reflection_occlusion: 0.0,
            ..Default::default()
        }
    );
    assert_eq!(plan.contents[screen], Content::Screen);
    assert_eq!(plan.contents[desk], Content::None);
    assert_eq!(
        plan.parts,
        scene.pieces.iter().map(|p| p.part).collect::<Vec<_>>()
    );
    assert_eq!(plan.movers, 1);
    let toward = [0.3f32, 0.8, 0.5];
    let l = (toward[0] * toward[0] + toward[1] * toward[1] + toward[2] * toward[2]).sqrt();
    assert_eq!(plan.sun.direction, toward.map(|v| v / l));
    assert_eq!(plan.sun.intensity, 3.0);

    let pixels = sky::pixels(&scene, None).unwrap();
    assert_eq!((pixels.width, pixels.height), (1024, 512));
    assert_eq!(pixels.texels, sky::room(&scene.room).texels);
    let light = Light::new(&scene, &look, &pixels, lighting());
    assert_eq!(light.hour, 16.0);
    assert_eq!(light.target, [0.0; 3]);
    assert_eq!(light.radius_deg, 0.5);
    assert!(light.canopy);
    let m = scene.motion.as_ref().unwrap();
    assert_eq!(
        light.steam,
        Some(Steam {
            lo: [
                m.cup[0] - m.radius * 5.0,
                m.cup[1],
                m.cup[2] - m.radius * 3.0
            ],
            hi: [
                m.cup[0] + m.radius * 3.0,
                m.cup[1] + 0.2,
                m.cup[2] + m.radius * 3.0
            ],
            density: 1.2,
            anisotropy: 0.3,
            color: [0.9, 0.9, 0.92],
            clock: 0.5,
        })
    );
    let volume = light.steam.unwrap().volume();
    let VolumeKind::Plume(plume) = volume.kind else {
        panic!("the steam is a plume");
    };
    for k in 0..3 {
        assert!((plume.source[k] - m.cup[k]).abs() < 1e-6, "{plume:?}");
    }
    assert!((plume.radius - m.radius).abs() < 1e-6, "{plume:?}");
    assert_eq!(plume.depth_per_width, 9.0);
    assert_eq!(plume.fade, [0.02, 0.07, 0.2]);
    let motes = light.motes.as_ref().unwrap();
    assert!(!motes.cells.is_empty());
    for p in motes.at(1.0) {
        for k in 0..3 {
            assert!((m.lo[k]..=m.hi[k]).contains(&p.position_size[k]));
        }
    }
    let lens = Lens::rest(&scene.camera, &look);
    let receiver = light::receiver(&lens, PLATE, &lighting().framing);
    for k in 0..3 {
        assert!(receiver.min[k] < lens.pivot[k] && lens.pivot[k] < receiver.max[k]);
    }
}

#[test]
fn a_scene_without_movers_content_or_detail_parts_gives_plain_instances() {
    let scene = scene_of(PLAIN, "");
    let look = look(&scene);
    let plan = Plan::new(&scene, &look, small_lookup, &KEEP_REFLECTION);
    assert_eq!(plan.instances.len(), 2);
    assert!(plan.groups.iter().all(Option::is_none));
    assert!(
        plan.surfaces
            .iter()
            .all(|s| *s == InstanceSurface::default())
    );
    assert!(plan.contents.iter().all(|c| *c == Content::None));
    assert_eq!(plan.movers, 0);
    for (instance, piece) in plan.instances.iter().zip(&scene.pieces) {
        assert_eq!(instance.model, IDENTITY);
        assert_eq!(instance.previous_model, IDENTITY);
        assert_eq!(instance.id, piece.object + 1);
        assert_eq!(instance.material, 0);
    }
    let slots = Slots::default();
    let dressed = slots.materials(&NoContent, &plan.materials);
    assert_eq!(dressed, plan.materials);
    assert_eq!(
        content::dresses(&NoContent, dressed.clone(), &plan.materials),
        vec![dressed]
    );
    let light = Light::new(
        &scene,
        &look,
        &sky::pixels(&scene, None).unwrap(),
        lighting(),
    );
    assert!(light.steam.is_none() && light.motes.is_none());
    assert!(light.particles(1.0).is_empty());
    let mut instances = plan.instances.clone();
    let mut materials = plan.materials.clone();
    let bound = light::detailed(&[None, None], &mut materials, &mut instances).unwrap();
    assert_eq!(bound, 0);
    assert_eq!(materials, plan.materials);
}

#[test]
fn a_callers_painter_dresses_content_by_its_slots() {
    let screen = Material {
        content: Content::Screen,
        clearcoat: 0.4,
        ..Material::default()
    };
    let plain = Material::default();
    let painter = SlotPainter {
        dress: caller::Dress { gain: 1.6 },
    };
    let materials = [plain, screen];
    let mut slots = Slots::default();
    let dark = slots.materials(&painter, &materials);
    assert_eq!(dark[1].content_layer, ContentLayer::default());
    assert_eq!(slots.dirty(&[]), Vec::<bool>::new());
    slots = Slots::default();
    let dresses = content::dresses(&painter, dark.clone(), &materials);
    assert_eq!(dresses.len(), 2);
    assert_eq!(dresses[0], dark);
    assert_eq!(
        dresses[1][1].content_layer,
        ContentLayer {
            strength: 1.0,
            ..ContentLayer::from_kind(Content::Screen, 0.4, 1, &caller::LOOK)
        }
    );
    assert_eq!(slots.filled(), &[] as &[bool]);
}

struct Boxes {
    objects: usize,
    hit: Option<u32>,
}

impl Picker for Boxes {
    type Hit = u32;

    fn objects(&self) -> usize {
        self.objects
    }

    fn pick(&self, _: [f32; 3], _: [f32; 3]) -> Option<u32> {
        self.hit
    }

    fn pick_object(&self, _: [f32; 3], _: [f32; 3], object: u32) -> Option<u32> {
        (Some(object) == self.hit).then_some(object)
    }
}

#[test]
fn a_pick_reads_object_plus_one_and_falls_back_to_the_callers_picker() {
    assert_eq!(content::id_of(0), 1);
    assert_eq!(content::object_of(0), None);
    assert_eq!(content::object_of(5), Some(4));
    assert_eq!(content::pixel((10.7, 3.2), (20, 10)), Some((10, 3)));
    assert_eq!(content::pixel((-0.2, 3.0), (20, 10)), None);
    assert_eq!(content::pixel((20.0, 3.0), (20, 10)), None);
    let picker = Boxes {
        objects: 3,
        hit: Some(2),
    };
    let ray = ([0.0; 3], [0.0, 0.0, -1.0]);
    let picked = |id| {
        Some(Picked {
            x: 4,
            y: 5,
            id,
            depth: 0.5,
        })
    };
    let resolve = |p, at| content::resolve(p, at, &picker, ray.0, ray.1);
    assert_eq!(resolve(picked(3), Some((4, 5))), (Some(2), Route::Ids));
    assert_eq!(resolve(picked(2), Some((4, 5))), (Some(2), Route::Missed));
    assert_eq!(resolve(picked(0), Some((4, 5))), (None, Route::Ids));
    assert_eq!(resolve(picked(9), Some((4, 5))), (None, Route::Ids));
    assert_eq!(resolve(picked(3), Some((1, 1))), (Some(2), Route::Stale));
    assert_eq!(resolve(None, Some((4, 5))), (Some(2), Route::Stale));
}

#[test]
fn the_mix_and_the_daylight_need_the_callers_hdr() {
    let mut scene = scene_of(PLAIN, "");
    scene.sky.choice = manfred_runtime::SkyChoice::Hdr;
    assert!(sky::pixels(&scene, None).is_err());
    scene.sky.choice = manfred_runtime::SkyChoice::Mix;
    assert!(sky::pixels(&scene, None).is_err());
}

const DRAFT: (u32, u32) = (640, 400);

fn red() -> Material {
    Material {
        base: [0.6, 0.05, 0.04],
        ..Material::default()
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::yield_now();
    }
}

fn toybox_lookup(name: &str) -> Material {
    match name {
        "lacquer_red" => red(),
        _ => small_lookup(name),
    }
}

fn source(gpu: &Gpu, colour: [u8; 4]) -> wgpu::Texture {
    let size = wgpu::Extent3d {
        width: 16,
        height: 16,
        depth_or_array_layers: 1,
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("runtime test source"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: ContentFormat::Srgb8.texture_format(),
        usage: wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let texels: Vec<u8> = colour.repeat(16 * 16);
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(16 * 4),
            rows_per_image: Some(16),
        },
        size,
    );
    texture
}

fn apply(m: Matrix, p: [f32; 4]) -> [f32; 4] {
    [0, 1, 2, 3].map(|row| (0..4).map(|col| m[col][row] * p[col]).sum())
}

fn pixel_of(lens: &Lens, size: (u32, u32), point: [f32; 3]) -> (f32, f32) {
    let clip = apply(
        lens.projection(size.0, size.1),
        apply(lens.view(), [point[0], point[1], point[2], 1.0]),
    );
    let (x, y) = (clip[0] / clip[3], clip[1] / clip[3]);
    (
        (x * 0.5 + 0.5) * size.0 as f32,
        (0.5 - y * 0.5) * size.1 as f32,
    )
}

fn on_body(scene: &Scene, body: u32) -> [f32; 3] {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for piece in scene.pieces.iter().filter(|p| p.object == body) {
        for p in &piece.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
    }
    let at = |k: usize, t: f32| lo[k] + (hi[k] - lo[k]) * t;
    [at(0, 600.0 / 1600.0), at(1, 1.0 - 760.0 / 1000.0), hi[2]]
}

fn draft(name: &str, rgb: &[u8], size: (u32, u32)) {
    let path = common::scratch("raster").join(name);
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size.0, size.1);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(rgb)
        .unwrap();
}

fn shot<P: manfred_runtime::Painter>(raster: &mut Raster<P>, time: f32) -> Vec<u8> {
    raster.shoot(time).unwrap();
    raster.settle().unwrap();
    turn();
    raster.capture().unwrap()
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn the_raster_half_draws_feeds_restores_and_picks_the_toybox() {
    let dir = common::golden("toybox");
    let glb = std::fs::read(dir.join("toybox.glb")).unwrap();
    let sidecar = std::fs::read_to_string(dir.join("toybox.json")).unwrap();
    let scene = load(&glb, &sidecar).unwrap();
    let look = look(&scene);
    let pixels = sky::pixels(&scene, None).unwrap();
    let parts = Parts {
        plan: Plan::new(&scene, &look, toybox_lookup, &KEEP_REFLECTION),
        light: Light::new(&scene, &look, &pixels, lighting()),
        look,
        painter: SlotPainter::default(),
        turn,
    };
    let gpu = block_on(Gpu::headless()).unwrap();
    let mut raster = Raster::headless(&gpu, parts, DRAFT).unwrap();
    assert_eq!(raster.size(), DRAFT);

    let print = source(&gpu, [200, 180, 40, 255]);
    let screen = source(&gpu, [30, 90, 200, 255]);
    let sources = |revisions: [u64; 2]| {
        [
            Source {
                texture: &print,
                revision: revisions[0],
                format: ContentFormat::Srgb8,
                fill: Fill::Copy,
            },
            Source {
                texture: &screen,
                revision: revisions[1],
                format: ContentFormat::Srgb8,
                fill: Fill::Copy,
            },
        ]
    };
    assert_eq!(raster.filled(), (&[] as &[bool], 0));
    raster.feed(&sources([1, 1])).unwrap();
    assert_eq!(raster.filled(), (&[true, true][..], 1));
    raster.feed(&sources([1, 1])).unwrap();
    assert_eq!(raster.filled(), (&[true, true][..], 1));
    raster.feed(&sources([1, 2])).unwrap();
    assert_eq!(raster.filled(), (&[true, true][..], 2));
    raster.settle().unwrap();
    turn();

    let body = scene.objects.iter().position(|o| o == "body").unwrap() as u32;
    let cursor = pixel_of(raster.lens(), DRAFT, on_body(&scene, body));
    raster.aim(cursor);
    let first = shot(&mut raster, 1.0);
    draft("toybox.png", &first, DRAFT);
    assert_eq!(first.len(), (DRAFT.0 * DRAFT.1 * 3) as usize);
    let mean = first.iter().map(|&v| f64::from(v)).sum::<f64>() / first.len() as f64;
    assert!(mean > 20.0, "the frame is dark: mean {mean}");
    let lit = first
        .chunks_exact(3)
        .filter(|p| p.iter().any(|&v| v > 64))
        .count();
    assert!(
        lit * 2 > (DRAFT.0 * DRAFT.1) as usize,
        "only {lit} lit pixels"
    );
    let mut seen: Vec<[u8; 3]> = first.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
    seen.sort_unstable();
    seen.dedup();
    assert!(seen.len() > 256, "only {} colours", seen.len());

    let picked = raster.picked().expect("the aimed pixel is picked");
    assert_eq!(
        (picked.x, picked.y),
        (cursor.0.floor() as u32, cursor.1.floor() as u32)
    );
    assert_eq!(picked.id, content::id_of(body));
    let ids = raster.ids().unwrap();
    assert_eq!(ids[(picked.y * DRAFT.0 + picked.x) as usize], picked.id);
    assert!(raster.fresh());

    let red = raster.materials().iter().position(|m| *m == red()).unwrap() as u32;
    let lid = index(&scene, "mv_lid");
    let count = raster.materials().len();
    assert_eq!(raster.assign(lid, count as u32), None);
    let previous = raster.assign(lid, red).unwrap();
    assert_ne!(previous, red);
    assert_eq!(raster.instances()[lid].material, red);
    shot(&mut raster, 3.0);
    let assigned = shot(&mut raster, 1.0);
    assert_ne!(assigned, first, "the assigned material shows");
    assert_eq!(raster.assign(lid, previous), Some(red));
    shot(&mut raster, 3.0);
    assert_eq!(
        shot(&mut raster, 1.0),
        first,
        "assigning back gives the same bytes"
    );

    let plain = raster.plain(&[lid]);
    assert_eq!(plain, [previous]);
    assert_eq!(raster.materials().len(), count + 1);
    assert_eq!(raster.instances()[lid].material, count as u32);
    shot(&mut raster, 3.0);
    raster.restore(&[lid], &plain);
    assert_eq!(raster.materials().len(), count);
    assert_eq!(raster.instances()[lid].material, previous);
    let restored = shot(&mut raster, 1.0);
    assert_eq!(restored, first, "restore gives back the same bytes");

    let fallback = ProbeLighting::fallback(&gpu.device, &gpu.queue, [0.2; 3]);
    raster.set_probes(fallback).unwrap();
    assert_eq!(
        raster.probes().reflection_occlusion(),
        lighting().reflection_occlusion
    );
    raster.set_reflection_occlusion(0.25).unwrap();
    assert_eq!(raster.probes().reflection_occlusion(), 0.25);
    assert!(raster.set_reflection_occlusion(1.5).is_err());
    assert_eq!(raster.probes().reflection_occlusion(), 0.25);
    let probed = shot(&mut raster, 1.0);
    assert_eq!(probed.len(), first.len());
    turn();
}
