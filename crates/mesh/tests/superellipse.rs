mod common;

use common::*;
use manfred_mesh::outline::{Shape, circle, difference, superellipse, union};
use manfred_mesh::*;

#[test]
fn a_superellipse_is_exact_at_the_axes() {
    let o = superellipse([0.1, -0.2], [0.03, 0.02], 4.0, 64);
    assert_eq!(o.0.len(), 64);
    assert_eq!(o.0[0], [0.1 + 0.03, -0.2]);
    assert_eq!(o.0[16], [0.1, -0.2 + 0.02]);
    assert_eq!(o.0[32], [0.1 - 0.03, -0.2]);
    assert_eq!(o.0[48], [0.1, -0.2 - 0.02]);
}

#[test]
fn a_superellipse_meets_its_formula_on_the_diagonal_and_everywhere() {
    let (a, b, n) = (0.05, 0.03, 5.0);
    let o = superellipse([0.0, 0.0], [a, b], n, 128);
    let diagonal = o.0[16];
    let expected = 2f64.powf(-1.0 / n);
    assert!((diagonal[0] / a - expected).abs() < 1e-14, "{diagonal:?}");
    assert!((diagonal[1] / b - expected).abs() < 1e-14, "{diagonal:?}");
    for p in &o.0 {
        let on = (p[0] / a).abs().powf(n) + (p[1] / b).abs().powf(n);
        assert!((on - 1.0).abs() < 1e-12, "{p:?} gives {on}");
    }
    assert!(o.area() > 0.0);
    let unit = superellipse([0.0, 0.0], [1.0, 1.0], n, 128);
    for k in 1..32 {
        let (p, q) = (unit.0[k], unit.0[32 - k]);
        assert_eq!(p, [q[1], q[0]]);
    }
}

#[test]
fn exponent_two_is_a_circle() {
    let o = superellipse([0.2, 0.1], [0.04, 0.04], 2.0, 64);
    let c = circle([0.2, 0.1], 0.04, 64);
    assert_eq!(o.0.len(), c.0.len());
    for (p, q) in o.0.iter().zip(&c.0) {
        assert!((p[0] - q[0]).abs() < 1e-14 && (p[1] - q[1]).abs() < 1e-14);
    }
}

#[test]
fn a_superellipse_extrudes_with_a_profile_and_joins_unions_and_holes() {
    let tile = superellipse([0.0, 0.0], [0.02, 0.02], 4.0, 64);
    for profile in [Profile::round(0.002, 4), Profile::NONE] {
        let m = extrude(&tile, 0.0, 0.005, &profile, UvMode::Local);
        assert_eq!(
            bytes(&m),
            bytes(&extrude(&tile, 0.0, 0.005, &profile, UvMode::Local))
        );
        assert!(min_area(&m) > 0.0);
        assert_sane(&m);
        assert_closed(&m);
        assert_outward(&m);
    }
    let pair = union(&[tile.clone(), tile.map(|[x, y]| [x + 0.03, y])]);
    assert_eq!(pair.len(), 1);
    assert!(pair[0].holes.is_empty());
    let holed = difference(
        &Shape::new(tile.clone()),
        &[superellipse([0.0, 0.0], [0.01, 0.01], 4.0, 32)],
    );
    assert_eq!(holed.len(), 1);
    assert_eq!(holed[0].holes.len(), 1);
    assert!(holed[0].area() < tile.area());
}
