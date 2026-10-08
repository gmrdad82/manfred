mod common;

use common::scratch;
use manfred_build::outlines;
use manfred_recipe::load;

const FRAME: &str = "\n[[object]]\nname = \"frame\"\nkind = \"outline\"\nfront = 0.0\nback = 0.02\noutline = { difference = [{ rect = [-0.1, -0.1, 0.1, 0.1] }, { points = [[0.0, -0.1], [0.05, -0.05], [0.0, 0.0], [-0.05, -0.05]] }] }\n";

#[test]
fn a_shape_refused_in_an_included_file_names_that_file_and_line() {
    let dir = scratch("outlines-included");
    std::fs::write(dir.join("rig.toml"), format!("manfred = 1\n{FRAME}")).unwrap();
    std::fs::write(
        dir.join("panel.toml"),
        "manfred = 1\nname = \"panel\"\ninclude = [\"rig.toml\"]\n",
    )
    .unwrap();
    let loaded = load(&dir.join("panel.toml")).unwrap();
    let problems = outlines(&loaded.recipe);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(
        problems[0].path,
        "rig.toml:8: object[frame].outline.difference[1]"
    );
    assert_eq!(problems[0].line, Some(3));
}

#[test]
fn a_shape_refused_in_a_used_file_names_that_file_and_line() {
    let dir = scratch("outlines-used");
    std::fs::write(dir.join("part.toml"), format!("manfred = 1\n{FRAME}")).unwrap();
    std::fs::write(
        dir.join("panel.toml"),
        "manfred = 1\nname = \"panel\"\n\n[[use]]\nfile = \"part.toml\"\nprefix = \"p_\"\n",
    )
    .unwrap();
    let loaded = load(&dir.join("panel.toml")).unwrap();
    let problems = outlines(&loaded.recipe);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].path.starts_with("part.toml:"),
        "{}",
        problems[0].path
    );
    assert!(
        problems[0]
            .path
            .ends_with("object[p_frame].outline.difference[1]")
    );
}
