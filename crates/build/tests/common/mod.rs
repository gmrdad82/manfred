#![allow(dead_code)]

use glam::DVec3;
use manfred_build::{Piece, build, write};
use manfred_mesh::{Mesh, bounds, rounded_box};
use manfred_recipe::{Recipe, validate};
use std::path::{Path, PathBuf};

pub fn golden(set: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(set)
}

pub fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp/build-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn matches_golden(r: &Recipe, folder: &Path, scratch_name: &str, files: [&str; 2]) {
    assert_eq!(validate(r), []);
    let b = build(r, folder).unwrap();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        write(&b, r, folder, None).unwrap();
    }
    let out = scratch(scratch_name);
    let written = write(&b, r, folder, Some(&out)).unwrap();
    assert_eq!(written, files.map(|name| out.join(name)));
    for name in files {
        let made = std::fs::read(out.join(name)).unwrap();
        let want = std::fs::read(folder.join(name)).unwrap();
        assert!(
            made == want,
            "{name} differs from its golden; UPDATE_GOLDEN=1 regenerates it"
        );
    }
}

pub fn builds_the_same_bytes_twice(r: &Recipe, folder: &Path) {
    let a = build(r, folder).unwrap();
    let b = build(r, folder).unwrap();
    assert!(a.glb() == b.glb());
    assert_eq!(a.sidecar(), b.sidecar());
}

pub fn cube(name: &str, material: &str) -> Piece {
    let mut mesh = rounded_box([1.0, 1.0, 1.0], 0.0, 1);
    mesh.set_material(material);
    Piece {
        name: name.to_string(),
        mesh,
    }
}

pub fn centre(m: &Mesh) -> DVec3 {
    let [lo, hi] = bounds(m).unwrap();
    (DVec3::from(lo) + DVec3::from(hi)) * 0.5
}
