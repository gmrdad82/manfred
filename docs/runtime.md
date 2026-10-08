# The runtime loader

`manfred-runtime` (`manfred::runtime` on the facade) is what an app or game links to read and draw Manfred's output. Its load half turns a GLB and its sidecar into a `Scene` of plain data. Manfred reads its own `facts` dialect (`docs/apps.md`); an app passes a reader for the keys its writer stored. It reads the GLB through `pfx_load::Mesh` and never touches wgpu. Its raster half (below) turns that `Scene` into the engine's live instances, lights and content, with every look-specific value a parameter of the caller's. It stays a library: no job record, no crash reporter, no window, no clock.

## Calls

- `load(glb: &[u8], sidecar: &str) -> Result<Scene, Error>`: the room is the sky; a blank sidecar gives every default, so a GLB with no sidecar loads. It reads with `facts`.
- `load_with(glb, sidecar, &Options)`: `Options { sky, fill, aspect }`. `Options::with_sky(choice, fill)` reads two strings (`daylight` or `hdr` for the HDR, `mix`, anything else the room; a fill that isn't a finite number of 0 or more is 0.25). `aspect` is the frame's when the sidecar has no `plate`, for a lens given in millimetres.
- `load_files(glb_path, sidecar_path)`, `load_dir(dir, name)` (`<name>.glb` beside `<name>.json`), each with a `_with` form. A sidecar whose `sky` names an `hdr` file (through the recipe's `[sidecar.sky]`, or whatever key the reader copies into `sky.hdr_file`) has that file read from beside the sidecar into `sky.hdr` and becomes the HDR choice.
- `Reader::read(&self, &serde_json::Value, &Options) -> Result<Sidecar, Error>`. A function of that shape is a reader. It fills a `Sidecar`: camera, sun, sky, room, exposure, grade, plate, motion, and the movers the sidecar names. `facts(json, &Options)` is Manfred's reader, and the calls above pass it. `load_read`, `load_files_read` and `load_dir_read` take the app's reader, each with a `_with` form for `Options`. The reader applies `Options` (`sky`, `fill`, and `aspect` when its lens is in millimetres). Movers in the GLB's node extras stay the runtime's, checked against the reader's list. A reader's `Err` reaches the caller; `Error::wanted("key")` is a sidecar error naming that key.
- `Error` is `Glb`, `Sidecar` or `Read { path, reason }`.
- `mover_group("mv_lid__handle") == Some("lid")`; `Scene::mover(group)` and `Scene::mover_of(object)` find the spec.

## The scene

- `tris`: world-space triangles, with the degenerate ones (cross product under 1e-14) dropped, flat normals where the GLB has none, inverse-transpose normals, `uv` zeros where it has none.
- `pieces`: one per primitive that keeps a triangle, welded as the GLB has it (positions, normals, uvs, indices) or unwelded with flat normals when the GLB has none. `part` counts every primitive in visit order, so it is stable for a given GLB; `Scene::parts` is the total.
- `materials` and `objects`: names interned in first-seen order of a depth-first visit of the roots. A node's name, else its mesh's, else `object`; a material's name, else `default`. Names are the contract: the material names are the app's and `mv_<group>__<part>` groups movers. Looking a name up in a material table is the caller's.
- `camera`: position, target, up, `fov_y` in degrees, `shift` (the lens shift, `[x, y]`), `aperture`, `focus`, and `orthographic` with `ortho_height`.
- `sun`, `exposure`, `grade` (vignette, saturation, warmth, bloom, haze), `plate`, `motion` (a steam source, its box and density, from the recipe's `[sidecar.motion]`; without `cup` the source is the box's centre) and `movers`, sorted by group name, each with pivot, axis, travel and two clip planes.
- `sky`: `choice` (`Room`, the default, `Hdr` or `Mix`), `fill`, `balance`, `intensity`, `rotation_deg` (and `rotation()` in radians), `hdr_file` and `hdr`. `room`: the lamp room from `sky.room`, its floor, wall and ceiling colours and its lamps, with the runtime's defaults. The room is read whatever the choice, since the mix and the engine's sky need it. Building the sky's pixels from them is the raster half's.

## Reading the facts

`facts` reads the keys below. An app's reader returns the same `Sidecar` from its own keys (`docs/apps.md`).

- **Camera:** the first of `cameras`. `fov_y` is `fov_y_deg`, or comes from `focal_mm` and `sensor_mm` (36 by default, the longest side) at the frame's aspect: the sidecar's `plate`, else `Options::aspect`, else 1586:992. `shift` is the recipe's, `[x, y]` in half-heights. `aperture` is `focal / 1000 / fstop` when `fstop` is above 0, the focal length worked back from the field of view when only that is given. `focus` is `focus_m`, else the distance to the target. `orthographic` and `ortho_height` as written.
- **Sun, sky, room, look:** `sun` (`toward`, `radius_deg`, `color`, `irradiance`), `sky` (`intensity`, `rotation_deg`, `balance`, `room`, `hdr`), `look.exposure` and `look.grade`. Missing keys take the runtime's defaults.
- **Movers** from `movers` and from the GLB's extras (`docs/recipe.md`, "Extras"). Only `extras.manfred` is read; a GLB with the old `extras.manf` is an `Error::Glb`.
- **Plate** from `plate.width` and `plate.height`; **motion** from `motion`.

## Checks

`tests/sidecar.rs` loads the toybox and the table goldens (`tests/golden/`) and pins every value of `toybox.json` against the scene, the four mover groups (one found by its `parts` through the extras), a room sky built and written by Manfred and read back lamp for lamp, a lens in millimetres read back as a field of view and an aperture, the sky choice, the defaults of a blank sidecar, and determinism by every door. `tests/reader.rs` checks that `facts` through `load_read` matches `load` on those two scenes, that a reader for another dialect loads the same scene (its movers included, with and without the GLB's extras), that a reader's error names the key and reaches every load door, and that a reader's `sky.hdr_file` is read from beside the sidecar. `tests/extras.rs` and `tests/groups.rs` check the movers from the extras against the sidecar's on the toybox.

## The raster half

Every look-specific value is a parameter. The material lookup, the content painter and the GPU turn are the caller's.

- **`Look`:** `fov` (degrees), `sun` (the sun's intensity), `sky` (the sky's gain), `turn` (its rotation in radians), `near` and `far`, the finish `chain` the renderer installs, and the per-frame `finish`. `Look::of(scene, near, far, chain, finish)` takes the first four from the scene.
- **`Lens`:** `Lens::rest(&scene.camera, &look)`, `viewed(&View)` with `View { yaw, pitch, roll, dolly, zoom, look }`, `tangents`, `view` and `projection` with the lens shift.
- **`Plan::new(scene, &look, lookup, keep)`** is the CPU half, with nothing on the GPU:
  - `meshes`: each piece as `pfx_geom::mesh::Mesh`, with the engine's tangents;
  - `instances`: `Instance::new(MeshHandle(k), IDENTITY, material, object + 1)`, so a pick id is the object index plus one (`content::id_of`, `object_of`), as Manfred's own stage does;
  - `materials`: `lookup(name)` for each of `scene.materials`, in order. The fallback for a name it doesn't know is the lookup's;
  - `groups`: the mover each piece belongs to (`Scene::group_of`, so extras groups count);
  - `surfaces`: a mover piece carries its mover's `clip` and `clip2`; a piece whose object or material is named in `keep` gets `reflection_occlusion = 0`;
  - `contents`, `parts` (`Piece::part`, for the detail atlas), `movers`, `camera` and `sun`.
- **`sky::pixels(scene, daylight)`:** the live sky's texels. The room is built from `scene.room`; `Hdr` and `Mix` need the caller's daylight HDR bytes and prepare them through the engine's daylight preparation, the mix adding the room's lamps at `fill`; a sidecar that names `sky.hdr` is decoded as it is.
- **`Light::new(scene, &look, &pixels, lighting)`** and `Lighting`: `hour` (the engine's reference hour, 16), `framing` (`caster_margin`, `framed`, `behind`, `before`: the shadow receiver box), `reflection_occlusion`, `wind`, `steam` (`color`, `rise`, `clock`), `motes` (`cell`, `share`, `spread`, `glow`) and `canopy: Option<Canopy { tree, sharpness, bird: Option<Bird> }>`. The tree and the bird's mesh and flight (`period`, `start`, `span`, `half_span`, `distance`) are the caller's. `light::receiver`, `quality`, `gobo`, `wind`, `anchors`, `detail_line` and `hash3` take those parameters.
- **`Bake::read(files, &Layout)`**, `Bake::from_dir` and `Bake::from_files`: the probes, the local volumes, the reflection cube and the detail atlas, from the folders `Layout { probes, locals, reflection, detail }` names. The detail parts are matched by `Piece::part` against the atlas's `parts[].index` (`light::layers`), and each bound instance gets its own copy of its material with the layer selected (`light::detailed`). `light::strays(scene, atlas)` names any atlas part that isn't exactly one static piece of its own name.
- **`Painter`**, the caller's content: `layer(material, filled)` dresses a material's content layer given the slots filled so far, `surface(content, base)` places the content on an instance, `warm(material)` lists the layers the warm step should compile, and `paint(Paint { gpu, encoder, slot, source, target })` draws a source into a slot. `NoContent` paints nothing. `Raster::feed(&[Source { texture, revision, format, fill }])` re-fills only the slots whose revision changed: `Fill::Paint` through the painter, then the slot's mips, `Fill::Copy` straight in. A caller folds anything else that dirties a slot (marks, a caret) into its revision.
- **`Raster<P: Painter>`:** `Raster::window(gpu, parts, size, format)`, `headless` and `headless_cached(…, cache)` from `Parts { plan, light, look, painter, turn }`, where `turn` is the caller's GPU turn (`pfx::adapter::turn`). It uploads the meshes, installs the light with a turn on either side, and has a whole live surface:
  - drawing: `draw`, `shoot`, `still(time, frames)` and `follow(time, frames, track, steps)` with a turn every `TURN_FRAMES` (8), `capture`, `ids`, `velocity`, `passes`, `jitter`;
  - the warm step: `warm`, `warming`, `ready`, `warm_and_wait`, `use_pipeline_cache`, `warmed_save`, `made_on_render_thread`, `dresses`;
  - the scene: `pose(&[Matrix])`, `set_view`, `reseed`, `resize`, `set_hour`, `bake`, `sky_settling`, `light`, `probes`, `set_probes` (the caller's `ProbeLighting`, at the lighting's `reflection_occlusion`, as `bake` installs its own), `set_reflection_occlusion`, `instances`, `materials`, `groups`, `surfaces`, `filled`, `plain`, `assign`, `restore`, `painter_mut`;
  - picking: `aim(cursor)`, `picked`, `fresh`, and `pick(cursor, &picker, ray)`, which takes the engine's id when it is for the cursor's pixel and the caller's `Picker` (`pick`, `pick_object`, `objects`) otherwise, with the `Route` it took.

### A caller's site

```rust
let scene = runtime::load_dir(&scene_dir, "toybox")?;
let look = runtime::Look::of(&scene, NEAR, FAR, chain, finish);
let pixels = runtime::sky::pixels(&scene, Some(&daylight_hdr))?;
let parts = runtime::Parts {
    plan: runtime::Plan::new(&scene, &look, app_material, &KEEP_REFLECTION),
    light: runtime::Light::new(&scene, &look, &pixels, app_lighting()),
    look,
    painter: AppContent::default(),
    turn: render::adapter::turn,
};
let mut raster = runtime::Raster::headless(&gpu, parts, PLATE)?;
raster.bake(runtime::Bake::from_files(BAKE, &APP_LAYOUT)?)?;
raster.feed(&sources)?;
raster.draw(time, &view)?;
```

The app keeps its material lookup with its fallback, its lighting constants (tree, bird, steam, motes), its content painter, its look and post chain, the names that keep their reflection, its near and far planes, its baked files and their layout. `tests/raster.rs` is such a caller as test code (`tests/caller/mod.rs`): a small hand-built scene plans its instances, surfaces, sky and light; a scene without movers, content or detail parts gives plain instances; the caller's painter dresses its slots; a pick reads the object plus one and falls back to the caller's picker.

Parity against an app's own renderer is the app's to prove now, with its own scenes and its own call site.
