use manfred_build::Part;
use manfred_glb::{DMat4, Node};
use manfred_mesh::{Mesh, Slot};
use manfred_recipe::{Recipe, Shadow, load};
use manfred_scene::{Eye, Stage, camera, headless, shoot, write_png};
use pfx::adapter::turn;
use pfx_gpu::wgpu;
use std::path::PathBuf;

const SIZE: [u32; 2] = [640, 400];

fn fixture(name: &str) -> Recipe {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    load(&path).unwrap().recipe
}

fn scratch(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp")
        .join(name)
}

fn face(mesh: &mut Mesh, corners: [[f32; 3]; 4], normal: [f32; 3]) {
    let base = mesh.positions.len() as u32;
    mesh.positions.extend(corners);
    mesh.normals.extend([normal; 4]);
    if let Some(uvs) = &mut mesh.uvs {
        uvs.extend([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    }
    mesh.indices
        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn part(name: &str, object: usize, mut mesh: Mesh, material: &str) -> Part {
    mesh.slots = vec![Slot {
        material: material.into(),
        start: 0,
        count: mesh.indices.len() as u32,
    }];
    Part {
        node: Node {
            name: name.into(),
            mesh,
            transform: DMat4::IDENTITY,
            keep_transform: false,
        },
        object,
        shadow: Shadow::Cast,
        two_sided: false,
        hidden: false,
    }
}

fn empty() -> Mesh {
    Mesh {
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Some(Vec::new()),
        uvs1: None,
        indices: Vec::new(),
        slots: Vec::new(),
    }
}

fn cube(centre: [f32; 3], half: f32) -> Mesh {
    let mut mesh = empty();
    let [x, y, z] = centre;
    let (lo, hi) = (
        [x - half, y - half, z - half],
        [x + half, y + half, z + half],
    );
    face(
        &mut mesh,
        [
            [lo[0], lo[1], hi[2]],
            [hi[0], lo[1], hi[2]],
            [hi[0], hi[1], hi[2]],
            [lo[0], hi[1], hi[2]],
        ],
        [0.0, 0.0, 1.0],
    );
    face(
        &mut mesh,
        [
            [hi[0], lo[1], lo[2]],
            [lo[0], lo[1], lo[2]],
            [lo[0], hi[1], lo[2]],
            [hi[0], hi[1], lo[2]],
        ],
        [0.0, 0.0, -1.0],
    );
    face(
        &mut mesh,
        [
            [hi[0], lo[1], hi[2]],
            [hi[0], lo[1], lo[2]],
            [hi[0], hi[1], lo[2]],
            [hi[0], hi[1], hi[2]],
        ],
        [1.0, 0.0, 0.0],
    );
    face(
        &mut mesh,
        [
            [lo[0], lo[1], lo[2]],
            [lo[0], lo[1], hi[2]],
            [lo[0], hi[1], hi[2]],
            [lo[0], hi[1], lo[2]],
        ],
        [-1.0, 0.0, 0.0],
    );
    face(
        &mut mesh,
        [
            [lo[0], hi[1], hi[2]],
            [hi[0], hi[1], hi[2]],
            [hi[0], hi[1], lo[2]],
            [lo[0], hi[1], lo[2]],
        ],
        [0.0, 1.0, 0.0],
    );
    face(
        &mut mesh,
        [
            [lo[0], lo[1], lo[2]],
            [hi[0], lo[1], lo[2]],
            [hi[0], lo[1], hi[2]],
            [lo[0], lo[1], hi[2]],
        ],
        [0.0, -1.0, 0.0],
    );
    mesh
}

fn floor(width: f32, depth: f32) -> Mesh {
    let mut mesh = empty();
    let (w, d) = (width * 0.5, depth * 0.5);
    face(
        &mut mesh,
        [[-w, 0.0, d], [w, 0.0, d], [w, 0.0, -d], [-w, 0.0, -d]],
        [0.0, 1.0, 0.0],
    );
    mesh
}

fn box_parts(paint: &str) -> Vec<Part> {
    vec![
        part("box", 0, cube([0.0, 0.1, 0.0], 0.1), paint),
        part("floor", 1, floor(1.6, 1.2), "wood"),
    ]
}

fn colours(rgba: &[u8]) -> usize {
    let mut seen: Vec<[u8; 3]> = rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2]]).collect();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

fn pixel(rgba: &[u8], size: [u32; 2], x: u32, y: u32) -> [u8; 3] {
    let at = ((y * size[0] + x) * 4) as usize;
    [rgba[at], rgba[at + 1], rgba[at + 2]]
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn a_one_box_recipe_stages_and_renders() {
    let recipe = fixture("box.toml");
    let gpu = headless().unwrap();
    let parts = box_parts("orange");
    let stage = Stage::with_parts(gpu.clone(), &recipe, &parts, SIZE).unwrap();
    assert_eq!(stage.materials(), ["orange", "wood"]);
    assert_eq!(stage.instance_count(), 2);
    assert_eq!(stage.output_format(), wgpu::TextureFormat::Rgba8UnormSrgb);
    drop(stage);
    let rgba = shoot(&gpu, &recipe, &parts, "main", SIZE).unwrap();
    assert_eq!(rgba.len(), (SIZE[0] * SIZE[1] * 4) as usize);
    write_png(&scratch("scene-box.png"), SIZE, &rgba).unwrap();
    assert!(colours(&rgba) > 200, "{} colours", colours(&rgba));
    let middle = pixel(&rgba, SIZE, SIZE[0] / 2, SIZE[1] / 2);
    assert!(middle[0] > middle[2], "the box is orange: {middle:?}");
    let plan = shoot(&gpu, &recipe, &parts, "plan", SIZE).unwrap();
    write_png(&scratch("scene-box-plan.png"), SIZE, &plan).unwrap();
    assert!(colours(&plan) > 50, "{} colours", colours(&plan));
    turn();
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn an_unknown_material_is_refused() {
    let recipe = fixture("box.toml");
    let gpu = headless().unwrap();
    let error = Stage::with_parts(gpu, &recipe, &box_parts("no_such_paint"), SIZE)
        .err()
        .unwrap();
    assert!(error.0.contains("object box"), "{error}");
    assert!(error.0.contains("\"no_such_paint\""), "{error}");
    turn();
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn the_same_still_twice_gives_the_same_bytes() {
    let recipe = fixture("box.toml");
    let gpu = headless().unwrap();
    let parts = box_parts("orange");
    let first = shoot(&gpu, &recipe, &parts, "main", [320, 200]).unwrap();
    turn();
    let second = shoot(&headless().unwrap(), &recipe, &parts, "main", [320, 200]).unwrap();
    assert!(first == second, "two stills of one recipe differ");
    turn();
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn the_hour_and_the_size_follow_the_window() {
    let recipe = fixture("box.toml");
    let gpu = headless().unwrap();
    let parts = box_parts("orange");
    let mut stage = Stage::with_parts(gpu.clone(), &recipe, &parts, [320, 200]).unwrap();
    let eye = Eye::from_camera(camera(&recipe, "main").unwrap(), 1.6);
    let draw = |stage: &mut Stage, size: [u32; 2]| {
        let target = gpu
            .offscreen(size[0], size[1], stage.output_format())
            .unwrap();
        for frame in 0..8 {
            stage
                .render(&eye, frame as f32 / 60.0, &target.view)
                .unwrap();
        }
        gpu.readback_rgba8(&target).unwrap()
    };
    let afternoon = draw(&mut stage, [320, 200]);
    stage.set_hour(Some(19.5)).unwrap();
    let evening = draw(&mut stage, [320, 200]);
    assert!(afternoon != evening, "the hour changed nothing");
    assert!(stage.set_hour(Some(25.0)).is_err());
    stage.resize([200, 120]).unwrap();
    assert_eq!(stage.size(), [200, 120]);
    assert_eq!(draw(&mut stage, [200, 120]).len(), 200 * 120 * 4);
    turn();
}

#[test]
#[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
fn a_shallow_focus_blurs_off_the_focal_plane_and_fstop_zero_does_not() {
    let recipe = fixture("box.toml");
    let gpu = headless().unwrap();
    let parts = box_parts("orange");
    let main = camera(&recipe, "main").unwrap();
    let eye = Eye::from_camera(main, 1.6);
    let draw = |through: Option<&manfred_recipe::Camera>| {
        let mut stage = Stage::with_parts(gpu.clone(), &recipe, &parts, SIZE).unwrap();
        stage.look_through(through);
        let target = gpu
            .offscreen(SIZE[0], SIZE[1], stage.output_format())
            .unwrap();
        for frame in 0..16 {
            stage
                .render(&eye, frame as f32 / 60.0, &target.view)
                .unwrap();
        }
        turn();
        gpu.readback_rgba8(&target).unwrap()
    };
    let pinhole = draw(None);
    let mut shut = main.clone();
    shut.fstop = 0.0;
    assert!(draw(Some(&shut)) == pinhole, "fstop 0 changed the shot");
    let mut open = main.clone();
    open.fstop = 1.4;
    let shallow = draw(Some(&open));
    write_png(&scratch("scene-dof-pinhole.png"), SIZE, &pinhole).unwrap();
    write_png(&scratch("scene-dof-shallow.png"), SIZE, &shallow).unwrap();
    let apart = |x: u32, y: u32| {
        let (a, b) = (pixel(&pinhole, SIZE, x, y), pixel(&shallow, SIZE, x, y));
        a.iter().zip(b).map(|(a, b)| a.abs_diff(b)).max().unwrap()
    };
    let (cx, cy) = (SIZE[0] / 2, SIZE[1] / 2);
    let focal = (cy - 4..=cy + 4)
        .flat_map(|y| (cx - 4..=cx + 4).map(move |x| (x, y)))
        .map(|(x, y)| apart(x, y))
        .max()
        .unwrap();
    assert!(focal <= 6, "the focal plane moved by {focal}");
    let blurred = (0..SIZE[1])
        .flat_map(|y| (0..SIZE[0]).map(move |x| (x, y)))
        .filter(|&(x, y)| apart(x, y) > 8)
        .count();
    assert!(
        blurred > 1000,
        "{blurred} pixels changed off the focal plane"
    );
}
