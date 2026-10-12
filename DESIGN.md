# Manfred: design

Manfred is a scene tool on the pfx engine. An AI agent such as Claude, or a person, describes objects and scenes in recipe files. Manfred builds them headless into GLB and sidecar files, and shows the same recipe live in a window where you look around, arrange, set cameras, lights and the sky, edit text and render. Your edits go back into the recipe, so whoever edits it next sees them. It takes the place of Blender scripts. It is not a modeller: no sculpting, no hand mesh editing.

Manfred holds no app's things. An app keeps its generators, its sidecar writer, its material catalogue, its recipes, goldens and bakes in its own repo, and drives Manfred through a build binary of its own that links the `manfred` library and registers them (`docs/apps.md`).

The recipe schema is in `docs/recipe.md`, an app's side in `docs/apps.md`, the look in `docs/theme.md`, the window in `docs/editing.md`, `docs/gizmos.md` and `docs/cameras.md`, renders and clips in `docs/render.md`, the runtime loader in `docs/runtime.md`.

## 1. The pieces in plain words

- **The engine** is pfx, the set of Rust libraries that every app compiles in: live rendering, path tracing, text, materials, physics, sound. Its command-line program, also **pfx**, renders and bakes. Manfred writes the bake recipe; baking it needs pfx (`pfx bake`).
- **A recipe** is a TOML file describing a scene: what the objects are, how they are built, where they sit, cameras, lights, sky, text, movers. An agent or a person writes it; Manfred reads it; edits in the window patch it.
- **A GLB** is the standard binary mesh file the apps load.
- **A sidecar** is the JSON beside the GLB holding what a GLB can't carry: the camera with its shift, the sun, the sky, the movers. Manfred gathers these facts from the recipe, and a writer turns them into the JSON: Manfred's own `facts` dialect, or an app's writer registered by its build binary.
- **Builder versus viewer.** The builder is the headless command that turns a recipe into GLB and sidecar. The viewer is the window that shows the same recipe live.
- **A bake** precomputes indirect light into probes so the live view stays fast. pfx does it from a bake recipe; Manfred writes that bake recipe from the scene recipe.
- **pgpu** is an optional GPU queue command. When it is on the PATH, Manfred's long GPU jobs run under it and report pacing turns to it, so they share the GPU with whatever else is using it. Without it, Manfred paces its GPU work the same way and runs directly. The window itself never goes through it.
- **The rig** runs the window in a hidden nested compositor, with screenshots and scripted input, for tests.
- **Deterministic** means the same input gives the same bytes every time, so golden files and diffs are possible.

## 2. Who uses it

- **An agent or a script, headless:** writes and edits recipes, runs `manfred build`, checks the output with `manfred diff` and `manfred shot`.
- **The apps' build binaries, headless:** each app's own `manfred` with its generators, writer and catalogue registered; the app commits its outputs.
- **A person, in the window:** opens a recipe, sees it live, arranges, renders. The window doesn't model; new objects come from the recipe.

The recipe is the single truth for all three. Tool state that isn't scene content (the orbit camera, selection, panel layout, the undo stack) is never in the recipe, so the app's diffs stay clean, and none of it is saved: only the recent list is kept, under `$XDG_STATE_HOME/manfred`.

## 3. What an app needs from Manfred

The toybox scene (`tests/golden/toybox/`) keeps every one of these kinds covered, and no app's scene lives in Manfred:

- **Meshes from code:** some apps build every mesh from explicit vertex and face lists (grids, tubes, fans, rings) with seeded randomness and noise. They need a mesh library and generators in Rust, registered by the app (`docs/apps.md`), with Manfred's kit: gradient noise, CPython's `random` and numpy's `RandomState`, and Blender's normal and export-order rules.
- **2.5D objects:** a desk-like object is 2D outlines extruded along the depth axis, with bevels and pockets whose cutters are outlines too: 2D polygon booleans and offsets with a profiled extrusion build it; a 3D boolean library stays a fallback. Also lathes, swept tubes, boxes and Blender's cylinder, smooth-by-angle normals, and one UV layer projected from the camera onto the plate with per-object overrides.
- **Skies and truths for the tracer:** an equirect of an emitter room, glass and thin film, rendered by `manfred render`.
- **Every app** reads glTF Y-up with modifiers applied, names only on materials, one root node per object in Blender's natural, case-insensitive name order, one primitive per material slot.

Matching Blender vertex for vertex is not a goal and not possible for bevels and normals. Done means: the same names, order and structure; bounds, area and triangle counts within stated tolerances; and a render diff on the live renderer under a PSNR floor. After an app switches, its own golden files are the reference, kept in its own repo.

## 4. Crates

Linux with Vulkan, Wayland for the window. Every crate has one job.

| Crate | Package | Job | Leans on |
|---|---|---|---|
| `crates/recipe` | manfred-recipe | parse, validate, patch and write recipes; the schema version | toml_edit, serde |
| `crates/mesh` | manfred-mesh | outlines, fillets, offsets, 2D booleans, profiled extrusion, pockets, lathe, sweep, grid, tube, fan, Blender-like primitives, split normals, weld, natural sort | i_overlay, glam |
| `crates/glb` | manfred-glb | the deterministic GLB writer, the facts and the `facts` dialect, a reader for diffs | gltf-json, gltf |
| `crates/build` | manfred-build | the builder and the `Registry` of generators, sidecar writers and materials: a recipe to GLB nodes, sidecar facts and output files, behind `manfred build`; pure and headless | recipe, mesh, glb |
| `crates/scene` | manfred-scene | recipe to engine scene: cameras with shift and ortho, sun, sky, lights, materials by name, text; the headless still | pfx-live, -load, -geom, -materials, -text, -post, -core |
| `crates/generators` | manfred-generators | the kit for an app's generators: noise, CPython's and numpy's random, params, Blender's face-list rules | build, mesh |
| `crates/render` | manfred-render | path-traced stills, equirect, ortho; PNG, EXR and HDR out; pacing and GPU turns | pfx-trace, -gpu, png, exr |
| `crates/runtime` | manfred-runtime | an app's loader: a GLB and a facts sidecar to a scene, and the scene to live instances, surfaces, lights, baked detail and content | pfx-load, -live, -gpu, -geom, -materials, -post, -bake |
| `crates/ui` | manfred-ui | the egui shell: panels, viewport, gizmos; the window host and the offscreen shot come from pfx-editor-shell, the widgets and the theming from pfx-editor-style; its own look is `theme.rs`, and `tape.rs` plays a tape on the offscreen window, frame by frame, for a clip | egui, egui-wgpu 0.36.2, pfx-editor-shell, pfx-editor-style |
| root | manfred | the `manfred` binary and the CLI it shares with an app's build binary: commands, the window, and the calls into the reporter | the crates above, pfx-report |

The binary reports its crashes, the progress of long jobs and the window's heartbeat through pfx-report, whose every call does nothing: `main` starts the reporter and names the command, the CLI's long commands keep a job record, and the window beats once a frame. A build that wants real reports patches in a reporter with the same API, with no change here. A build of this tree sends nothing anywhere.

A library facade, `manfred` as a lib target with the recipe, mesh, glb, build, generators and runtime crates re-exported, is what an app links in its own build binary to register its generators, sidecar writer and material catalogue by name, and to add commands of its own (`docs/apps.md`). The recipe references a generator and its params; the window treats a generator's output as read-only geometry it can still place. No scripting language, and no Python.

The engine crates are pinned by one ref in the workspace and move in one commit, egui with them. The engine is on wgpu 30, and egui 0.36.2 is the egui release on that wgpu. The tracer takes the GPU's ray queries when the adapter offers them and its own BVH otherwise. `scene`, `render`, `runtime`, `ui` and `glb` call the engine. The root `Cargo.toml` holds the pin.

## 5. Process model

- **`manfred build`** is CPU only, pure and fast: a recipe plus its inputs give a GLB, a sidecar and, when asked, a bake recipe. No GPU, no clock, no randomness beyond the recipe's seed.
- **`manfred shot`** draws one still of a recipe on the live renderer, headless, through the engine's offscreen device.
- **`manfred render`** path-traces. When `pgpu` is on the PATH, it re-executes itself under `pgpu run --class clip --as manfred`, inside a `systemd-run` user scope with a memory cap and at a lower priority when `systemd-run` is there, and reports pacing turns to pgpu as it goes. Without pgpu it says so and renders directly, paced the same way (`docs/render.md`).
- **`manfred gui`** is the window. It redraws on input or on a change of the recipe file, presents with vsync, and idles at zero frames. It never calls `pgpu turn`: it is not a queued job. When pgpu is there and `pgpu status --json` reports play, it pauses its live view. Renders started from the window are `manfred render` subprocesses, so the window stays responsive; the result appears when the subprocess ends. The window starts no bake: Manfred has none, and the caller runs `pfx bake` on the bake recipe `manfred build` writes.
- **`manfred gui --shot`** draws one frame of the whole interface offscreen to a PNG, with scripted input, and no compositor. The interface tests use it. A test that needs a live compositor goes through `bin/rig` (`docs/rig.md`).

## 6. Document model

- One recipe file, with a `manfred` schema version key, objects in file order, cameras, lights, sky, text, movers and variants. A `params` reference pulls numbers from an app's own JSON, so an app's solver keeps writing its file.
- The window holds the recipe as a toml_edit document and **never holds a dirty copy**. Each finished gesture is a patch, a key path and a value, applied to the document and written at once, with the author's formatting and comments preserved. Undo is the inverse patch. An external change on disk is re-read and wins, so an agent editing the file and a person in the window never clobber each other.
- The builder caches each object by the hash of its subtree, so one dragged knob rebuilds one object. The window keeps one stage for its life and swaps only the changed meshes (`replace_mesh`, `release_mesh`); its rebuild worker keeps the prepared sky and decoded card images between edits, so an edit that leaves `[sky]` and the images alone neither reloads nor decodes them. The status panel D1 shows the GPU time of the last frame and the engine's dropped-timing count when above zero.
- Instance ids are object index plus one. `pick` from the engine selects by id. Glass surfaces and text carry that same id, so a click on either selects the object. The outliner selects what has no pixels.

## 7. The window

- **egui**, themed to `docs/theme.md`: one monospace family, deep navy, thin borders, amber selection, panel codes. The theme is a `Theme` value Manfred builds in `crates/ui/src/theme.rs` (its tokens and the two fonts from `assets/fonts/`); pfx's `pfx-editor-style`, which pfx's own editor uses too and which the root `Cargo.toml` pins at the same rev as every other pfx crate, turns it into the `Visuals` and `Style` and paints the custom widgets from it, and bundles no look of its own. The window host, the offscreen shot and its input script, the worker, the pgpu status reader and the panel focus keys are pfx's `pfx-editor-shell`, behind its `App` trait: `Manfred` in `crates/ui/src/app.rs` is Manfred's `App`, its `Shell` with its `Viewer`, named `manfred` for the pgpu job and the log lines. No egui type leaks into `recipe`, `mesh`, `glb` or `scene`.
- **The viewport is a texture.** The engine renders the scene offscreen; egui shows that texture in its central panel and routes input by the widget's response. Gizmos are 2D lines in egui's painter over the viewport, hit-tested in 2D, projected from the scene's camera. The engine has no line drawing and needs none.
- **Panels:** an outliner with search, properties for the selection, cameras, lights and sky with an hour scrubber, a render panel with progress, a status bar with the recipe path, the engine's frame time and the pgpu state. Panels carry codes, A, B1 to B3, C1 to C3 and D1, and keys jump focus to them.
- **Every control acts.** The four tabs in A jump focus to their panels as the codes do, Scene to B1, Cameras to B2, Lights to B3, Render to C3, and the tab of the focused panel is the lit one (Scene for B1 and C2; none for A, C1 and D1). A panel's header focuses it. Key caps are legends of the keys, not buttons, except the hint bar's `ctrl o`, `ctrl z` and `ctrl ⇧ z` and the picker's `enter` and `esc`, which do what their keys do. Pills and dotted separators are readouts. A control that cannot act now is disabled, with the reason on hover, and a value the window cannot write shows read only. A test walks every control the shell draws, clicks it, and fails on one that changes nothing.
- **egui is 2D.** Its widgets are flat chrome around the viewport; they hover, focus, tooltip and click, and the theme turns their shadows off. Nothing in egui lives in the 3D scene. Anything in-world, labels on surfaces or a diegetic control, is the engine's surface text and content slots, drawn by the scene, not by egui.

## 8. 2D and 2.5D

Manfred is 3D, and 2D is 3D with two switches: an orthographic camera and flat cards facing it. The recipe has `kind = "orthographic"` on a camera, `card` objects that carry an image slot and can face the camera, and the plate for pixel-exact placement. Cards cast shadows the way the engine's leaf cards do. The live renderer and the tracer both take an orthographic camera, and `manfred render` takes an equirectangular one. A sprite-heavy scene packs its images into one `ContentAtlas`, as Manfred's cards do.

## 9. Determinism and tests

- The GLB writer formats every number the same way, orders nodes by file order or natural sort as the recipe says, interns materials first-seen, and writes one primitive per slot. The same recipe gives the same bytes on any machine.
- Manfred's own scenes (`table`, `toybox`, `cards`) have golden GLBs and sidecars in `tests/golden/`, checked byte for byte; no Manfred test reads an app's repo.
- `manfred diff a.glb b.glb` prints names, order, node and triangle counts, bounds, area and materials, per object, with the differences marked; `--render` adds a live-renderer still of each and a PSNR. An app migrating from Blender runs it against its Blender export.
- Live-renderer stills are byte-identical on the same GPU and driver and compared with a tolerance otherwise. Path-traced output is compared by PSNR only.
- GPU tests are `#[ignore]`d; run them with `cargo test -- --ignored` on a machine with a Vulkan GPU (optionally under `pgpu run`). `bin/gate` runs no ignored tests.

## 10. What Manfred never does

- Model by hand: no sculpting, no vertex editing, no UV unwrapping, no rigging, no animation authoring. Blender stays the escape hatch for a one-off, and its result comes in as glTF.
- Read `.blend` files. Import is glTF 2.0 and Radiance HDR.
- Write outside the caller's repo: outputs go where the recipe says, scratch under `tmp/`, caches under the machine cache folder.
- Link the reporter from a library crate under `crates/`. Only the root package calls pfx-report, and a test keeps the others off it.
- Spawn pfx from a library. The app's build script runs `pfx bake` on the bake recipe Manfred wrote.

## 11. What is in place

The recipe, the mesh library, the GLB writer and reader, the builder and the registry, the live scene and `manfred shot`, the window, the path tracer (`manfred render`, equirect, orthographic), clips, the runtime's load and raster halves, and the generator kit. An app adopts through its own build binary (`docs/apps.md`).

## 12. Another machine

Nothing in the design changes for a second machine. `manfred build` is a pure function of a folder; `manfred render` and `manfred shot` are GPU jobs that run with or without a queue. A different GPU gives renders that differ in the low bits, so render comparisons use tolerances; GLBs are identical everywhere.
