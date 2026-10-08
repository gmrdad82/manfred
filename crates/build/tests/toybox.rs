mod common;

use manfred_build::build;
use manfred_mesh::{Mesh, area, bounds};
use manfred_recipe::{Kind, Recipe, load, parse, validate};
use std::path::{Path, PathBuf};

const NAMES: [&str; 18] = [
    "backdrop",
    "block",
    "body",
    "cable",
    "card",
    "coin",
    "drawer_front",
    "drawer_pull",
    "lip_dial",
    "lip_tray",
    "mv_knob",
    "mv_knob__dot",
    "mv_lid",
    "mv_lid__handle",
    "mv_roller",
    "pill",
    "shade",
    "vase",
];

const MATERIALS: [&str; 10] = [
    "wood",
    "felt",
    "lacquer_red",
    "rubber",
    "card",
    "brass",
    "lacquer_cream",
    "steel",
    "glaze",
    "occluder",
];

fn golden() -> PathBuf {
    common::golden("toybox")
}

fn toybox() -> Recipe {
    load(&golden().join("toybox.toml")).unwrap().recipe
}

fn recipe(text: &str) -> Recipe {
    parse(&format!(
        "manfred = 1\nname = \"t\"\n[plate]\nwidth = 100\nheight = 50\nmetres_per_pixel = 0.01\norigin = \"top_left\"\n{text}"
    ))
    .unwrap()
}

fn mesh(text: &str) -> Mesh {
    let b = build(&recipe(text), Path::new(".")).unwrap();
    b.parts[0].node.mesh.clone()
}

fn parse_error(text: &str) -> String {
    parse(&format!("manfred = 1\nname = \"t\"\n{text}"))
        .unwrap_err()
        .to_string()
}

fn near(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

fn edges_shared_twice(m: &Mesh) -> bool {
    let key = |i: u32| m.positions[i as usize].map(f32::to_bits);
    let mut count = std::collections::BTreeMap::new();
    for t in m.indices.chunks_exact(3) {
        for k in 0..3 {
            let (a, b) = (key(t[k]), key(t[(k + 1) % 3]));
            let e = if a < b { (a, b) } else { (b, a) };
            *count.entry(e).or_insert(0u32) += 1;
        }
    }
    count.values().all(|&c| c == 2)
}

fn volume(m: &Mesh) -> f64 {
    m.indices
        .chunks_exact(3)
        .map(|t| {
            let [a, b, c] =
                [0, 1, 2].map(|k| glam::Vec3::from(m.positions[t[k] as usize]).as_dvec3());
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

#[test]
fn the_toybox_matches_its_golden_glb_and_sidecar() {
    common::matches_golden(
        &toybox(),
        &golden(),
        "toybox",
        ["toybox.glb", "toybox.json"],
    );
}

#[test]
fn the_bare_toybox_matches_its_golden_glb_and_sidecar() {
    let bare = manfred_recipe::apply_variant(&toybox(), "bare").unwrap();
    common::matches_golden(
        &bare,
        &golden(),
        "toybox-bare",
        ["toybox-bare.glb", "toybox-bare.json"],
    );
}

#[test]
fn the_table_matches_its_golden_glb_and_sidecar() {
    let dir = common::golden("table");
    let table = load(&dir.join("table.toml")).unwrap().recipe;
    common::matches_golden(&table, &dir, "table", ["table.glb", "table.json"]);
    common::builds_the_same_bytes_twice(&table, &dir);
}

#[test]
fn the_toybox_builds_the_same_bytes_twice() {
    common::builds_the_same_bytes_twice(&toybox(), &golden());
}

#[test]
fn the_toybox_has_its_names_in_natural_order() {
    let b = build(&toybox(), &golden()).unwrap();
    let glb = manfred_glb::summary(&b.glb()).unwrap();
    let names: Vec<&str> = glb.nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, NAMES);
    assert!(
        glb.nodes
            .iter()
            .all(|n| n.slots.len() == 1 && n.triangles > 0)
    );
}

#[test]
fn the_toybox_has_its_own_materials_in_first_seen_order() {
    let b = build(&toybox(), &golden()).unwrap();
    let glb = manfred_glb::summary(&b.glb()).unwrap();
    assert_eq!(glb.materials, MATERIALS);
    let r = toybox();
    for name in MATERIALS {
        assert!(r.materials.contains_key(name), "{name}");
    }
}

#[test]
fn the_toybox_keeps_its_cutters_and_hidden_guide_out_of_the_glb() {
    let r = toybox();
    let pockets: Vec<&str> = r
        .objects
        .iter()
        .filter(|o| matches!(o.kind, Kind::Pocket { .. }))
        .map(|o| o.name.as_str())
        .collect();
    assert_eq!(pockets, ["well_tray", "well_dial", "well_slot"]);
    assert_eq!(r.objects.len(), 22);
    let b = build(&r, &golden()).unwrap();
    assert!(b.nodes().iter().all(|n| n.name != "guide"));
    assert!(b.parts.iter().any(|p| p.node.name == "guide" && p.hidden));
}

#[test]
fn the_bare_variant_drops_its_objects_motion_and_sun() {
    let bare = manfred_recipe::apply_variant(&toybox(), "bare").unwrap();
    assert_eq!(validate(&bare), []);
    let b = build(&bare, &golden()).unwrap();
    let names: Vec<String> = b.nodes().into_iter().map(|n| n.name).collect();
    for gone in ["vase", "card", "coin"] {
        assert!(!names.iter().any(|n| n == gone), "{gone}");
    }
    let sidecar: serde_json::Value = serde_json::from_str(&b.sidecar().unwrap()).unwrap();
    assert!(sidecar.get("motion").is_none());
    assert_eq!(sidecar["sun"]["irradiance"], serde_json::json!(0.0));
}

#[test]
fn a_sweep_along_an_outline_is_a_closed_outward_bead() {
    let text = "[[object]]\nname = \"lip\"\nkind = \"sweep\"\nalong = { outline = { circle_px = [50, 25, 10], segments = 48 }, offset = 0.01, depth = 0.02 }\nradius = 0.005\nring = 12\nmaterial = \"m\"\n";
    let m = mesh(text);
    assert_eq!(m.triangle_count(), 48 * 12 * 2);
    assert!(edges_shared_twice(&m));
    assert!(volume(&m) > 0.0);
    let [lo, hi] = bounds(&m).unwrap();
    let reach = 0.1 + 0.01 + 0.005;
    assert!(near(lo, [-reach, -reach, -0.025], 1e-6), "{lo:?}");
    assert!(near(hi, [reach, reach, -0.015], 1e-6), "{hi:?}");
    assert!(mesh(text) == m);
}

#[test]
fn a_sweep_along_an_outline_takes_one_radius_and_no_path() {
    let along = "along = { outline = { circle = [0.0, 0.0, 1.0] } }";
    let both = parse_error(&format!(
        "[[object]]\nname = \"a\"\nkind = \"sweep\"\n{along}\npath = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]\nradius = 0.1\n"
    ));
    assert!(both.contains("`path` and `along` are both set"), "{both}");
    let caps = parse_error(&format!(
        "[[object]]\nname = \"a\"\nkind = \"sweep\"\n{along}\ncaps = true\nradius = 0.1\n"
    ));
    assert!(
        caps.contains("a sweep along an outline is closed"),
        "{caps}"
    );
    let r = parse(&format!(
        "manfred = 1\nname = \"t\"\n[[object]]\nname = \"a\"\nkind = \"sweep\"\n{along}\nradii = [0.1, 0.2]\n"
    ))
    .unwrap();
    assert!(
        validate(&r)
            .iter()
            .any(|p| p.text.contains("takes one `radius`"))
    );
}

#[test]
fn a_lifted_plane_keeps_its_edges_and_raises_its_middle() {
    let m = mesh(
        "[[object]]\nname = \"card\"\nkind = \"plane\"\nsize = [0.2, 0.4]\ngrid = [2, 4]\nbend = { radius = 0.5, form = \"lift\" }\nmaterial = \"m\"\n",
    );
    let sag = 0.5 - (0.25f64 - 0.04).sqrt();
    let [lo, hi] = bounds(&m).unwrap();
    assert!(near(lo, [-0.1, 0.0, -0.2], 1e-7), "{lo:?}");
    assert!(near(hi, [0.1, sag, 0.2], 1e-7), "{hi:?}");
    assert_eq!(m.triangle_count(), 16);
}

#[test]
fn curl_cup_and_jag_shape_a_strip_card() {
    let text = |extra: &str| {
        format!(
            "[[object]]\nname = \"card\"\nkind = \"plane\"\nsize = [0.4, 0.8]\ngrid = [10, 20]\n{extra}material = \"m\"\n"
        )
    };
    let flat = mesh(&text(""));
    let curled = mesh(&text("curl = { width_px = 4, depth = 0.003 }\n"));
    let [lo, _] = bounds(&curled).unwrap();
    assert!((lo[1] + 0.003).abs() < 1e-7);
    let row = |m: &Mesh, z: f32| -> Vec<f32> {
        m.positions
            .iter()
            .filter(|p| (p[2] - z).abs() < 1e-6)
            .map(|p| p[1])
            .collect()
    };
    assert!(row(&curled, 0.4).iter().all(|y| (y + 0.003).abs() < 1e-7));
    assert!(row(&curled, 0.0).iter().all(|y| y.abs() < 1e-9));
    let cupped = mesh(&text("cup = { from = 0.6, depth = 0.002 }\n"));
    let side = |m: &Mesh, x: f32| -> Vec<f32> {
        m.positions
            .iter()
            .filter(|p| (p[0] - x).abs() < 1e-6)
            .map(|p| p[1])
            .collect()
    };
    assert!(side(&cupped, 0.2).iter().all(|y| (y + 0.002).abs() < 1e-7));
    assert!(side(&cupped, 0.0).iter().all(|y| y.abs() < 1e-9));
    let jagged = mesh(&text("jag_px = [0.5, 1.0]\n"));
    let [lo, hi] = bounds(&jagged).unwrap();
    assert!((lo[0] + 0.21).abs() < 1e-6 && (hi[0] - 0.21).abs() < 1e-6);
    let left: Vec<(f32, f32)> = jagged
        .positions
        .iter()
        .filter(|p| p[0] < -0.19)
        .map(|p| (p[2], p[0]))
        .collect();
    let at = |z: f32, x: f32| {
        left.iter()
            .any(|&(lz, lx)| (lz - z).abs() < 1e-6 && (lx - x).abs() < 1e-6)
    };
    assert!(at(0.4, -0.195) && at(0.36, -0.21) && at(0.32, -0.195));
    for m in [&flat, &curled, &cupped, &jagged] {
        assert_eq!(m.triangle_count(), 400);
        let up = m.normals.iter().map(|n| n[1]).sum::<f32>();
        assert!(up > 0.0);
    }
    let metres = mesh(&text(
        "curl = { width = 0.04, depth = 0.003 }\ncup = { from = 0.6, depth = 0.002 }\njag = [0.005]\n",
    ));
    let pixels = mesh(&text(
        "curl = { width_px = 4, depth = 0.003 }\ncup = { from = 0.6, depth = 0.002 }\njag_px = [0.5]\n",
    ));
    assert_eq!(metres.positions.len(), pixels.positions.len());
    assert!(
        metres
            .positions
            .iter()
            .zip(&pixels.positions)
            .all(|(a, b)| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-7))
    );
    assert!(mesh(&text("jag_px = [0.5, 1.0]\n")) == jagged);
}

#[test]
fn plane_shaping_keys_are_checked() {
    let both = parse_error(
        "[[object]]\nname = \"a\"\nkind = \"plane\"\nsize = [1.0, 1.0]\ncurl = { width = 0.1, width_px = 2, depth = 0.1 }\n",
    );
    assert!(
        both.contains("`width` and `width_px` are both set"),
        "{both}"
    );
    let form = parse_error(
        "[[object]]\nname = \"a\"\nkind = \"plane\"\nsize = [1.0, 1.0]\nbend = { radius = 1.0, form = \"fold\" }\n",
    );
    assert!(form.contains("\"wrap\" or \"lift\""), "{form}");
    let r = parse(
        "manfred = 1\nname = \"t\"\n[[object]]\nname = \"a\"\nkind = \"plane\"\nsize = [1.0, 1.0]\ncup = { from = 1.0, depth = 0.1 }\n",
    )
    .unwrap();
    assert!(validate(&r).iter().any(|p| p.path.ends_with("cup.from")));
}

#[test]
fn a_cylinder_reads_its_radius_and_length_in_plate_pixels() {
    let m = mesh(
        "[[object]]\nname = \"axle\"\nkind = \"cylinder\"\nat_px = [50, 25, -0.01]\nrotate = [0.0, 0.0, 90.0]\nradius_px = 2\ndepth_px = 30\nsegments = 32\nbevel = { width = 0.002, segments = 3 }\nmaterial = \"m\"\n",
    );
    let [lo, hi] = bounds(&m).unwrap();
    assert!(near(lo, [-0.15, -0.02, -0.03], 1e-6), "{lo:?}");
    assert!(near(hi, [0.15, 0.02, 0.01], 1e-6), "{hi:?}");
    assert!(edges_shared_twice(&m));
    assert!(volume(&m) > 0.0);
}

#[test]
fn a_profiled_outline_rounds_its_sharp_corners_too() {
    let text = "[[object]]\nname = \"key\"\nkind = \"outline\"\noutline = { rect = [0.0, 0.0, 0.1, 0.1] }\nfront = 0.0\nback = 0.05\nprofile = { kind = \"round\", width = 0.01, segments = 4 }\nmaterial = \"m\"\n";
    let m = mesh(text);
    assert!(edges_shared_twice(&m));
    assert!(volume(&m) > 0.0);
    let [lo, hi] = bounds(&m).unwrap();
    assert!(near(lo, [0.0, 0.0, -0.05], 1e-7) && near(hi, [0.1, 0.1, 0.0], 1e-7));
    assert!(!m.positions.iter().any(|p| p[0] == 0.0 && p[1] == 0.0));
    let arc = 0.01 - 0.01 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        m.positions
            .iter()
            .any(|p| (p[0] - arc).abs() < 1e-6 && (p[1] - arc).abs() < 1e-6)
    );
    assert!(area(&m) < 0.1 * 0.1 * 2.0 + 0.4 * 0.05);
}
