pub mod cli;

pub use manfred_build as build;
pub use manfred_generators as generators;
pub use manfred_glb as glb;
pub use manfred_mesh as mesh;
pub use manfred_recipe as recipe;
pub use manfred_runtime as runtime;

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn the_library_crates_never_link_the_reporter() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates");
        let mut checked = 0;
        for entry in std::fs::read_dir(crates).unwrap().flatten() {
            let manifest = entry.path().join("Cargo.toml");
            let Ok(text) = std::fs::read_to_string(&manifest) else {
                continue;
            };
            checked += 1;
            assert!(
                !text.contains("pfx-report"),
                "{} links pfx-report",
                manifest.display()
            );
        }
        assert!(checked >= 6);
    }
}
