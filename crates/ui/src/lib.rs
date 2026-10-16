mod animated;
mod app;
mod cameras;
mod curves;
mod document;
mod edit;
mod fixture;
mod follow;
mod gizmo;
mod open;
mod orbit;
mod overlay;
mod panel;
mod pick;
mod routes;
mod sample;
mod shell;
mod shots;
mod spans;
mod start;
mod tape;
mod theme;
mod timeline;
mod undo;
mod viewer;
mod viewport;
mod watched;
mod wire;

use pfx_editor_style::widgets;

pub use app::Manfred;
pub use curves::{Change, CurveEdit, TRACKS as CURVE_TRACKS};
pub use fixture::Fixture;
pub use follow::{Follow, Mode, Shape};
pub use pfx_editor_shell::{Error, Pulse, Script, shot, window};
pub use shell::Shell;
pub use start::{start_recipe, start_recipe_here};
pub use tape::{
    Act, Layout, MAX_FRAMES, Tape, View, frame_png, play, poster_png, steps as tape_steps,
};
pub use viewer::Viewer;
pub use watched::is_scene;
pub use wire::Look;

#[cfg(test)]
use pfx_editor_shell::{encode_png, gpu_turn};

#[cfg(test)]
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp/ui-tests")
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("scene.toml")
}
