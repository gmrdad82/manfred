# An app's own build binary

Manfred holds no app's things. An app keeps its generators, its sidecar writer, its material catalogue, its recipes and its bakes in its own repository, and drives Manfred through a small build binary that links the `manfred` library and registers them. That binary is `manfred` with the app's names added: every command, flag and output is the same, and `manfred` alone refuses a recipe that names something only the app registers.

Link the library by its tag:

```toml
[dependencies]
manfred = { git = "https://github.com/gmrdad82/manfred.git", tag = "vX.Y.Z" }
```

## The registry

`manfred::build::Registry` is what an app fills. `Registry::new()` already knows Manfred's own sidecar dialects; everything else is the app's.

- **Generators:** `register(name, fn(&Call) -> Result<Vec<Piece>, String>)`, or `register_with_facts(name, fn(&Call) -> Result<Generated, String>)` when the generator also returns sidecar facts, materials or lights. `Generated::lights` is a list of `GeneratedLight { name, position, direction, cone: [inner, outer] (degrees), range, color, intensity, shadow }` in the object's frame: the object's transform moves them with the pieces, and the scene writes each as a `[[light]]` with `kind = "spot"` while the engine has room. A recipe names it with `kind = "generator"`, `generator = "<name>"` and `params`. `Call` gives the object, its params, a seed and the recipe's folder. Names are the app's, usually `<app>/<thing>`.
- **Sidecar writers:** `register_sidecar(name, fn(&SceneFacts) -> Result<serde_json::Value, String>)`. A recipe picks one with `[output] sidecar_dialect = "<name>"`. The writer turns the facts the builder gathers into the JSON the app's loader reads, and may refuse a recipe with a sentence (for example, a dialect that needs a camera). It returns a JSON object; Manfred then deep-merges what generators returned and the recipe's `[sidecar]` table over it, and writes it pretty-printed with sorted keys.
  - `"none"` writes no sidecar and can't be registered.
  - `"facts"` is Manfred's own dialect, the facts as they are (below).
- **Materials:** `register_materials(iter of (name, serde_json::Value))`, or `register_materials_toml(text)` for a file of `[materials.<name>]` tables in the engine's material form (the same form as a recipe's own `[materials]`). A part's material name resolves through the app's catalogue first, then the recipe's own `[materials]` (with what generators returned under it). The engine holds no presets since v0.19.0, so a name found in neither is an error naming the object, the material and the names the recipe does define. When the recipe's own entry differs from the catalogue's, the catalogue's is used and `manfred build` prints a note.
- **Queries:** `names()`, `contains(name)`, `sidecar(name)`, `sidecar_names()`, `material(name)`, `material_names()`.

The catalogue reaches every consumer through `Built::recipe(&recipe)`: the GLB's material names, `manfred shot`, `manfred render`, `manfred clip` and the window all draw from it. The bake recipe takes it alone: `manfred build` writes the catalogue entries the GLB names as `bake-materials.toml`, the engine's material library the bake names, and refuses the recipe's own `[materials]` and any name the catalogue lacks.

## The facts

`SceneFacts` (`manfred::build::SceneFacts`) is what a sidecar writer reads, all in the recipe's own units and axes:

- `cameras`: `CameraFacts { name, orthographic, position, target, up, fov_y_deg, focal_mm, sensor_mm, shift, focus_m, fstop, ortho_height }`, in file order;
- `sun`: `SunFacts { toward, radius_deg, color, irradiance }`;
- `daylight`: `DaylightFacts { hour, day, latitude, heading }`;
- `sky`: `SkyFacts { intensity, rotation_deg, balance, room }`, with `RoomFacts { width, floor, wall, ceiling, lights }` and `LampFacts { name, az, el, power, color, width, height, soft, slats, open }`;
- `plate`: `[width, height]`;
- `movers`: `MoverFacts { group, pivot, axis, travel, clip, clip2 }`, in file order;
- `look`: `LookFacts { exposure, grade: GradeFacts { vignette, saturation, warmth, bloom, haze } }`;
- `extra`: what generators returned and the recipe's `[sidecar]`, merged in that order. Manfred merges it over the writer's output, so a writer doesn't need to.

`manfred::build::{merge, mover_json}` and `manfred::glb::{room_json, grade_json, sidecar_text}` are the pieces Manfred's own writer uses.

**The `facts` dialect** writes `cameras` (a list, each with `name`, `orthographic`, `position`, `target`, `up`, `shift`, `fstop` and whichever of `fov_y_deg`, `focal_mm`, `sensor_mm`, `focus_m` and `ortho_height` the recipe gives), `sun`, `daylight`, `sky` (with `balance` and `room` when set; a lamp writes `slats` and `open` only off 0 and 1, and `name` only when it has one), `plate` (`width`, `height`), `movers` (by group) and `look` (`exposure`, `grade`). A key the recipe doesn't set is left out. `manfred::runtime::facts` reads it, and `load` passes that reader (`docs/runtime.md`). An app passes its own reader to `load_read` for the keys its writer stored. The reader is an argument of the load, beside the writer on the registry.

## Commands

`manfred::cli::main_with(args, &registry, &host)` runs the CLI. `Host { beat, usage, commands }` lets the binary add its own subcommands: each `Command { name, usage, run: fn(&[String], &Registry) -> ExitCode }` runs when its name is the first argument and isn't one of Manfred's own, with the rest of the line as given, so it parses its own arguments; its `usage` line is that command's description and usage in `manfred --help`. `usage` on the host is text the help ends with. An app that would rather keep its bakes in a separate binary needs no command at all.

## The facade

What an app's binary needs is public under `manfred`:

- `manfred::cli`: `main`, `main_with`, `Host`, `Command`, `Run`, `Beat`, `Pulse`, `VERSION`, `version_line`.
- `manfred::build` (`crates/build`): `Registry`, `Call`, `Piece`, `Generated`, `Generator`, `FactsGenerator`, `SidecarWriter`, `SceneFacts` and its parts (`CameraFacts`, `SunFacts`, `DaylightFacts`, `SkyFacts`, `RoomFacts`, `LampFacts`, `MoverFacts`, `LookFacts`, `GradeFacts`), `FACTS`, `NO_SIDECAR`, `facts_sidecar`, `merge`, `mover_json`, `build`, `build_with`, `write`, `Built`, `seed`, `world`, `local_matrix`, `rotation`, `detail_uv`.
- `manfred::mesh` (`crates/mesh`): `Mesh`, `Slot`, the kinds' builders (`rounded_box`, `cylinder`, `extrude`, `pocket`, `lathe`, `sweep`, `grid`, `disc`, …), `transform`, `flat`, `smooth`, `smooth_by_angle`, `natural_cmp`, and the surface-nets polygonizer (`Field`, `Lattice`, `surface_nets`).
- `manfred::generators` (`crates/generators`), the kit for an app's generators and bakes, every item with neutral inputs in its tests:
  - `noise`: Manfred's gradient noise, `perlin` and `fractal` (`docs/noise.md`);
  - `pyrandom::Random`: CPython's `random`, call for call (`random`, `uniform`, `randrange`, `gauss`, `init_genrand`, …), checked against 1,400 recorded calls;
  - `numpy_random::RandomState`: numpy's legacy `RandomState` over the same Mersenne Twister;
  - `params`: `read_json`, `source_or_own` and `Table`, for a generator's `params` or a `source` file;
  - `shape`: a face-list mesh as Blender holds one (`Vector`, `Face`, `Shape`, `grid`, `tube`) turned into a `Piece` with Blender's normals and triangulation (`piece`, `exported`, `flips`, `unit`), Blender's exporter's node order (`export_cmp`), and `smoothstep`, `interp`, `noise`, `fractal` on a `Vector`.
- `manfred::recipe`, `manfred::glb` (`summary`, `diff`, `write`, `sidecar_text`, `room_json`, `grade_json`) and `manfred::runtime` (`Reader`, `Sidecar`, `facts`, `Error::wanted`, `load_read`, `load_read_with`, `load_files_read`, `load_files_read_with`, `load_dir_read`, `load_dir_read_with`; `docs/runtime.md`).

A generator or a bake written inside an app compiles against these paths alone (`manfred::generators::noise`, `manfred::generators::shape` and so on); a bake's own PNG writing is the app's.

## Adopting

An app moves its scenes onto Manfred with a build binary of its own:

1. Put its generators, its sidecar writer and its bakes in its own crate, with their paths pointed at `manfred::generators::…` as above.
2. Register them: `registry.register(…)` for each generator, `registry.register_sidecar("<its dialect>", writer)` with the writer reading `SceneFacts` (the room, the grade and the daylight are typed facts, and a writer refuses a recipe with `Err`), and `register_materials_toml` for its material library.
3. In its recipes: the sky, sun and look take the spellings `docs/recipe.md` gives, and a `[bake]` names its `product`. A dialect name is the app's own, as long as the binary registers a writer under it.
4. Its bakes and its commands either join the binary through `Host::commands` or ship as a binary of their own.

## An example

`examples/app_build.rs` is a complete app build binary, compiled and tested with the gate:

```rust
use manfred::build::{Call, Piece, Registry, SceneFacts};
use manfred::cli::{Command, Host};

fn cube(call: &Call) -> Result<Vec<Piece>, String> { … }
fn sidecar(facts: &SceneFacts) -> Result<serde_json::Value, String> { … }

fn read_sidecar(
    json: &serde_json::Value,
    options: &manfred::runtime::Options,
) -> Result<manfred::runtime::Sidecar, manfred::runtime::Error> {
    let mut read = manfred::runtime::facts(&serde_json::Value::Null, options)?;
    let eye = json.get("eye").ok_or_else(|| manfred::runtime::Error::wanted("eye"))?;
    let at = json.get("look_at").ok_or_else(|| manfred::runtime::Error::wanted("look_at"))?;
    let xyz = |v: &serde_json::Value, i: usize| {
        v.get(i).and_then(|n| n.as_f64()).unwrap_or(0.0) as f32
    };
    read.camera.position = [xyz(eye, 0), xyz(eye, 1), xyz(eye, 2)];
    read.camera.target = [xyz(at, 0), xyz(at, 1), xyz(at, 2)];
    Ok(read)
}

fn hello(args: &[String], registry: &Registry) -> ExitCode { … }

const HOST: Host = Host {
    beat: None,
    usage: "",
    commands: &[Command { name: "hello", usage: "app hello <words>...", run: hello }],
};

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.register("app/cube", cube);
    registry.register_sidecar("app", sidecar);
    registry.register_materials_toml(include_str!("materials.toml")).unwrap();
    registry
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    manfred::cli::main_with(&args, &registry(), &HOST)
}

fn open(glb: &[u8], text: &str) -> Result<manfred::runtime::Scene, manfred::runtime::Error> {
    manfred::runtime::load_read(glb, text, read_sidecar)
}
```

`sidecar` writes the camera as `eye` and `look_at`. `read_sidecar` reads those two keys back into a `Sidecar`, starting from `facts` on empty JSON so every other field keeps the runtime's default, and returns `Error::wanted` naming a key it needs. `open` passes that reader to `load_read`.
