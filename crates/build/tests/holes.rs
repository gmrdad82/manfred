use manfred_build::{build, outlines};
use manfred_mesh::Mesh;
use manfred_mesh::outline::Outline;
use manfred_recipe::{Kind, Outline as Shape, Recipe, parse, validate};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/holes/glyph_p.toml")
}

fn recipe(text: &str) -> Recipe {
    parse(&format!("manfred = 1\nname = \"t\"\n{text}")).unwrap()
}

fn ids(m: &Mesh) -> Vec<u32> {
    let mut seen = BTreeMap::<[u32; 3], u32>::new();
    m.positions
        .iter()
        .map(|p| {
            let n = seen.len() as u32;
            *seen.entry(p.map(|c| (c + 0.0).to_bits())).or_insert(n)
        })
        .collect()
}

fn solid(m: &Mesh) -> i64 {
    let id = ids(m);
    let mut directed = BTreeMap::<(u32, u32), u32>::new();
    for t in m.indices.chunks_exact(3) {
        for e in 0..3 {
            let (a, b) = (id[t[e] as usize], id[t[(e + 1) % 3] as usize]);
            assert_ne!(a, b, "a collapsed edge");
            *directed.entry((a, b)).or_default() += 1;
        }
    }
    for (&(a, b), &n) in &directed {
        assert_eq!(n, 1, "edge {a}-{b} is used {n} times one way");
        assert_eq!(directed.get(&(b, a)), Some(&1), "edge {a}-{b} is open");
    }
    let volume: f64 = m
        .indices
        .chunks_exact(3)
        .map(|t| m.point(t[0]).dot(m.point(t[1]).cross(m.point(t[2]))))
        .sum::<f64>()
        / 6.0;
    assert!(volume > 0.0, "signed volume {volume}");
    let vertices: BTreeSet<u32> = m.indices.iter().map(|&i| id[i as usize]).collect();
    vertices.len() as i64 - (directed.len() / 2) as i64 + m.triangle_count() as i64
}

fn only_mesh(r: &Recipe) -> Mesh {
    let b = build(r, Path::new(".")).unwrap();
    assert_eq!(b.parts.len(), 1);
    b.parts[0].node.mesh.clone()
}

fn depths_near(m: &Mesh, ring: &Outline, reach: f64) -> BTreeSet<i64> {
    let n = ring.0.len();
    m.positions
        .iter()
        .filter(|p| {
            let q = [p[0] as f64, p[1] as f64];
            (0..n).any(|i| {
                let (a, b) = (ring.0[i], ring.0[(i + 1) % n]);
                let e = [b[0] - a[0], b[1] - a[1]];
                let len = e[0] * e[0] + e[1] * e[1];
                let t = (((q[0] - a[0]) * e[0] + (q[1] - a[1]) * e[1]) / len).clamp(0.0, 1.0);
                let d = [a[0] + e[0] * t - q[0], a[1] + e[1] * t - q[1]];
                (d[0] * d[0] + d[1] * d[1]).sqrt() <= reach
            })
        })
        .map(|p| (p[2] as f64 * 1e5).round() as i64)
        .filter(|&z| z > -3500)
        .collect()
}

fn points(o: &Shape) -> Outline {
    let Shape::Points { points, .. } = o else {
        panic!("points");
    };
    Outline(points.iter().map(|p| [p[0], p[1]]).collect())
}

#[test]
fn pfx_p_is_one_watertight_solid_with_its_bowl_rounded_like_its_outside() {
    let text = std::fs::read_to_string(fixture()).unwrap();
    let r = parse(&text).unwrap();
    assert_eq!(validate(&r), []);
    assert_eq!(outlines(&r), []);
    let m = only_mesh(&r);
    assert_eq!(solid(&m), 0, "one solid with one hole through it");
    let Kind::Outline { outline, holes, .. } = &r.objects[0].kind else {
        panic!("an outline");
    };
    let outer = depths_near(&m, &points(outline), 0.0085);
    let bowl = depths_near(&m, &points(&holes[0]), 0.0085);
    assert!(bowl.len() >= 7, "the bowl's wall rows {bowl:?}");
    assert_eq!(bowl, outer);
}

#[test]
fn pfx_p_as_a_difference_builds_the_same_solid() {
    let text = std::fs::read_to_string(fixture()).unwrap();
    let start = text.find("outline = { points = ").unwrap();
    let end = text.find("front = 0.07").unwrap();
    let block = &text[start..end];
    let (outer, hole) = block.split_once("\nholes = [").unwrap();
    let outer = outer.trim_start_matches("outline = ").trim();
    let hole = hole.trim().trim_end_matches(']');
    let rewritten = text.replace(
        block,
        &format!("outline = {{ difference = [{outer}, {hole}] }}\n"),
    );
    let a = only_mesh(&parse(&text).unwrap());
    let b = only_mesh(&parse(&rewritten).unwrap());
    assert_eq!(solid(&b), 0);
    assert_eq!(a.triangle_count(), b.triangle_count());
}

#[test]
fn a_frame_a_washer_and_four_bars_build_as_one_solid_each() {
    let r = recipe(
        r#"
[[object]]
name = "frame"
kind = "outline"
front = 0.0
back = 0.02
outline = { difference = [{ rect = [-0.1, -0.1, 0.1, 0.1] }, { rect = [-0.05, -0.05, 0.05, 0.05] }] }
profile = { kind = "round", width = 0.004, segments = 4 }

[[object]]
name = "washer"
kind = "outline"
front = 0.0
back = 0.01
outline = { circle = [0.0, 0.0, 0.05] }
holes = [{ circle = [0.0, 0.0, 0.02] }]
profile = { kind = "chamfer", width = 0.002 }

[[object]]
name = "bars"
kind = "outline"
front = 0.0
back = 0.01
outline = { union = [{ rect = [0.0, 0.0, 0.1, 0.02] }, { rect = [0.0, 0.06, 0.1, 0.08] }, { rect = [0.0, 0.0, 0.02, 0.08] }, { rect = [0.08, 0.0, 0.1, 0.08] }] }
profile = { kind = "round", width = 0.003 }
"#,
    );
    assert_eq!(outlines(&r), []);
    let b = build(&r, Path::new(".")).unwrap();
    assert_eq!(b.parts.len(), 3);
    for part in &b.parts {
        assert_eq!(solid(&part.node.mesh), 0, "{}", part.node.name);
    }
}

#[test]
fn a_ring_in_a_ring_is_two_solids_by_even_odd() {
    let r = recipe(
        r#"
[[object]]
name = "rings"
kind = "outline"
front = 0.0
back = 0.01
outline = { circle = [0.0, 0.0, 0.05] }
holes = [{ circle = [0.0, 0.0, 0.04] }, { circle = [0.0, 0.0, 0.03] }, { circle = [0.0, 0.0, 0.02] }]
profile = { kind = "round", width = 0.002 }

[[object]]
name = "islands"
kind = "outline"
front = 0.0
back = 0.01
outline = { union = [{ difference = [{ circle = [0.0, 0.0, 0.05] }, { circle = [0.0, 0.0, 0.04] }] }, { circle = [0.0, 0.0, 0.03] }] }
"#,
    );
    assert_eq!(outlines(&r), []);
    let b = build(&r, Path::new(".")).unwrap();
    assert_eq!(solid(&b.parts[0].node.mesh), 0, "two rings");
    assert_eq!(solid(&b.parts[1].node.mesh), 2, "a ring and a disc");
}

#[test]
fn a_pocket_cuts_into_a_holed_outline_and_not_over_its_hole() {
    let base = r#"
[[object]]
name = "plate"
kind = "outline"
front = 0.0
back = 0.02
outline = { rect = [0.0, 0.0, 0.2, 0.1], radius = 0.01 }
holes = [{ circle = [0.05, 0.05, 0.02] }]
profile = { kind = "round", width = 0.004 }

[[object]]
name = "well"
kind = "pocket"
into = "plate"
depth = 0.01
rim = { width = 0.002 }
"#;
    let inside = recipe(&format!(
        "{base}outline = {{ rect = [0.1, 0.02, 0.18, 0.08], radius = 0.008 }}\n"
    ));
    assert_eq!(outlines(&inside), []);
    let m = only_mesh(&inside);
    assert_eq!(solid(&m), 0);
    let over = recipe(&format!(
        "{base}outline = {{ circle = [0.05, 0.05, 0.01] }}\n"
    ));
    let refused = outlines(&over);
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused[0].path, "object[well].outline");
    assert_eq!(
        refused[0].text,
        "the pocket reaches past the edge of `plate`, with 100.0% of its area outside it"
    );
}

fn refused(text: &str) -> (String, Option<usize>, String) {
    let r = recipe(text);
    assert_eq!(validate(&r), []);
    let problems = outlines(&r);
    assert_eq!(problems.len(), 1, "{problems:?}");
    let p = &problems[0];
    let built = build(&r, Path::new(".")).unwrap_err().to_string();
    assert_eq!(built, format!("{}: {}", p.path, p.text));
    (p.path.clone(), p.line, p.text.clone())
}

const SLAB: &str = "\n[[object]]\nname = \"s\"\nkind = \"outline\"\nfront = 0.0\nback = 0.02\noutline = { rect = [-0.1, -0.1, 0.1, 0.1] }\n";

#[test]
fn a_hole_on_the_edge_is_refused_at_its_key() {
    let (path, line, text) = refused(&format!(
        "{SLAB}holes = [\n  {{ circle = [0.0, 0.0, 0.02] }},\n  {{ rect = [0.05, -0.02, 0.1, 0.02] }},\n]\n"
    ));
    assert_eq!(path, "object[s].holes[1]");
    assert_eq!(line, Some(12));
    assert_eq!(
        text,
        "the hole touches or crosses the edge of `outline`; a hole lies inside the outline, clear of its edge"
    );
    let (path, _, _) = refused(&format!(
        "{SLAB}holes = [{{ circle = [0.09, 0.0, 0.02] }}]\n"
    ));
    assert_eq!(path, "object[s].holes[0]");
}

#[test]
fn a_hole_outside_the_outline_is_refused() {
    let (path, _, text) = refused(&format!(
        "{SLAB}holes = [{{ circle = [0.3, 0.0, 0.02] }}]\n"
    ));
    assert_eq!(path, "object[s].holes[0]");
    assert_eq!(
        text,
        "the hole lies outside `outline`; a hole lies inside the outline, clear of its edge"
    );
}

#[test]
fn holes_that_touch_or_cross_are_refused_naming_both() {
    let (path, line, text) = refused(&format!(
        "{SLAB}holes = [\n  {{ circle = [-0.03, 0.0, 0.02] }},\n  {{ circle = [0.0, 0.0, 0.02] }},\n]\n"
    ));
    assert_eq!(path, "object[s].holes[1]");
    assert_eq!(line, Some(12));
    assert_eq!(
        text,
        "the hole touches or crosses `holes[0]`; holes stay clear of each other, and one inside another makes an island"
    );
}

#[test]
fn a_cutter_touching_the_edge_from_inside_is_refused_naming_both() {
    let (path, _, text) = refused(
        "\n[[object]]\nname = \"s\"\nkind = \"outline\"\nfront = 0.0\nback = 0.02\noutline = { difference = [{ rect = [0.0, 0.0, 0.4, 0.4] }, { points = [[0.1, 0.0], [0.2, 0.1], [0.1, 0.2], [0.05, 0.1]] }] }\n",
    );
    assert_eq!(path, "object[s].outline.difference[1]");
    assert_eq!(
        text,
        "`outline.difference[1]` touches `outline.difference[0]` at [0.1, 0]; a hole lies clear of the edge and of other holes"
    );
}

#[test]
fn two_cutters_meeting_at_a_corner_are_refused_naming_both() {
    let (path, line, text) = refused(
        "\n[[object]]\nname = \"s\"\nkind = \"outline\"\nfront = 0.0\nback = 0.02\noutline = { difference = [{ rect = [0.0, 0.0, 0.4, 0.4] }, { union = [{ rect = [0.1, 0.1, 0.2, 0.2] }, { rect = [0.2, 0.2, 0.3, 0.3] }] }] }\n",
    );
    assert_eq!(path, "object[s].outline.difference[1].union[1]");
    assert_eq!(line, Some(9));
    assert_eq!(
        text,
        "`outline.difference[1].union[1]` touches `outline.difference[1].union[0]`; a hole lies clear of the edge and of other holes"
    );
}

#[test]
fn a_profile_wider_than_half_a_wall_is_refused_naming_the_width_and_the_wall() {
    let (path, line, text) = refused(
        "\n[[object]]\nname = \"w\"\nkind = \"outline\"\nfront = 0.0\nback = 0.02\noutline = { circle = [0.0, 0.0, 0.05], segments = 64 }\nholes = [{ circle = [0.0, 0.0, 0.044], segments = 64 }]\nprofile = { kind = \"round\", width = 0.004 }\n",
    );
    assert_eq!(path, "object[w].profile.width");
    assert_eq!(line, Some(11));
    assert_eq!(
        text,
        "the profile's width 0.004 is wider than half the 0.005993 wall between `outline` and `holes[0]`, which takes a width up to 0.002996"
    );
    let (path, _, text) = refused(
        "\n[[object]]\nname = \"w\"\nkind = \"outline\"\nfront = 0.0\nback = 0.02\noutline = { rect = [0.0, 0.0, 0.2, 0.1] }\nholes = [{ rect = [0.02, 0.02, 0.09, 0.08] }, { rect = [0.095, 0.02, 0.18, 0.08] }]\nprofile = { kind = \"chamfer\", width = 0.003 }\n",
    );
    assert_eq!(path, "object[w].profile.width");
    assert_eq!(
        text,
        "the profile's width 0.003 is wider than half the 0.005 wall between `holes[0]` and `holes[1]`, which takes a width up to 0.0025"
    );
}

#[test]
fn a_profile_that_fits_its_walls_builds() {
    let r = recipe(
        "\n[[object]]\nname = \"w\"\nkind = \"outline\"\nfront = 0.0\nback = 0.02\noutline = { circle = [0.0, 0.0, 0.05], segments = 64 }\nholes = [{ circle = [0.0, 0.0, 0.044], segments = 64 }]\nprofile = { kind = \"round\", width = 0.0025 }\n",
    );
    assert_eq!(outlines(&r), []);
    assert_eq!(solid(&only_mesh(&r)), 0);
}
