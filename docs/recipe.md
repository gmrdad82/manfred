# The recipe, schema 1

A recipe is one TOML file in the app's repo, written by an AI agent such as Claude or by a person, patched by Manfred's window, read by `manfred build` or by the app's own build binary (`docs/apps.md`). The `recipe` crate is its one implementation and its tests are the spec.

## Conventions

- Paths are relative to the recipe file. Outputs go where `[output]` says; scratch goes under the repo's `tmp/`.
- Units are metres. Axes are the engine's and glTF's: Y up, right-handed, a default camera looks along -Z. "Depth" in a pocket grows away from the camera, along -Z. Blender's Z-up never appears in a recipe.
- Angles are degrees. Colours are linear RGB triplets in 0..1, or a `"#rrggbb"` string where a key says so. The key is `color`.
- Keys are snake_case. Objects are an array of tables, `[[object]]`, in the order they appear. An unknown key is an error. A missing name, an unknown material, a reference to an object that doesn't exist, and a degenerate result are errors that name the key.
- `manfred = 1` is the schema version. A reader refuses a newer schema and migrates an older one. An older header, `manf = 1`, has no alias: a recipe or an included file that still starts with `manf = 1` is refused at that line, "`manf = 1` is the old header; Manfred recipes start with `manfred = 1`".
- Nothing reads the clock or the machine. The only randomness comes from `seed`, which generators consume.

## Top level

```toml
manfred = 1
include = ["rig.toml"]
name = "toybox"
seed = 7
params = "scene/toybox.params.json"

[plate]
width = 1600
height = 1000
metres_per_pixel = 0.0004
origin = "centre"

[output]
glb = "assets/scene/toybox.glb"
sidecar = "assets/scene/toybox.json"
sidecar_dialect = "facts"
order = "natural"
bake = "assets/bake.toml"
scene = "assets/scene/toybox.scene.toml"
scene_camera = "main"
scene_format = 1
```

- `include`: other files whose cameras, lights, sky, look, materials, routes, particles, flare, motion blur, haze and objects merge into this recipe, under "Includes and uses" below.
- `sound`: an optional `*.sound.toml` file, relative to the recipe, which the scene names as its top-level `sound` (format 1 only; see "Shot effects").
- `params`: an optional JSON or TOML file whose values can be substituted into numeric fields as `"${params.camera.focal_mm}"`. An app's own solver writes the file; the recipe consumes it.
- `[plate]`: declares plate pixels, and fixes the `plate` UV projection. `camera` names the camera that projection looks through; without it, the first `[[camera]]`.
  - **Twins:** with a plate, these keys have a `_px` twin read in pixels: `at_px`, `size_px`, `rect_px`, `radius_px`, `circle_px`, `points_px`, and a mover's `pivot_px`, `travel_px`, `clip_px` and `clip2_px`. A key and its twin together are an error, and a twin without a `[plate]` is a problem.
  - **Origin:** `origin` is where pixel 0,0 sits, `"centre"` or `"top_left"`. Either way the plate is centred on the world origin.
  - **Axes:** pixel y grows down and world y up: `y = -(py - oy) * metres_per_pixel`. `rect_px` is normalised to min and max.
  - **Depth:** `at_px` takes x and y in pixels, plus an optional z in metres. A box's `size_px` takes three values, the last in metres; a card's or plane's takes two.
- `order`: `file` (the default) writes nodes in recipe order; `natural` sorts them the way Blender's exporter did, for migration. An app that depends on order says which.
- `sidecar_dialect`: which writer turns the scene's facts into the sidecar: `none` (the default; `[output]` itself may be absent), `facts`, Manfred's own dialect (`docs/apps.md`, "The facts"), or the name of a writer the app's build binary registers. The recipe carries one camera and sun description; the writer turns it into the keys that app reads. A dialect other than `none` needs a `sidecar` path, and a `sidecar` with dialect `none` is a problem. A name no writer is registered under is a build error that lists the registered ones; a writer may refuse a recipe with a sentence of its own, reported under `output.sidecar_dialect`.
- `scene`: when set, `manfred build` also writes the engine's `scene.toml` for this recipe, and its material library beside it, under "The scene" below. The path ends in `.toml`; `.scene.toml` is the convention. `scene_camera` names the `[[camera]]` it carries; without it, the first. A `scene_camera` without a `scene`, or naming no camera, is a problem. `scene_format` is 1 (the default: scene format 1, pfx-scene's, under "Format 1" below, which the engine and Manfred's window open) or 0 (the engine's older `scene.toml`, which the engine still reads, migrating it in memory with one warning a file); any other number, or a `scene_format` without a `scene`, is a problem.
- `bake`: when set, `manfred build` also writes a `pfx bake` recipe for this scene, in the engine's `bake.toml` schema, with the same sky, sun and files. The path ends in `bake.toml`, since `pfx bake --scene <folder>` reads that file of the folder. The keys only a bake needs come from the `[bake]` table, under "Variants, look and bake".

## Spelling shared with `scene.toml`

Where the recipe and the engine's `scene.toml` (pfx's `docs/scenes.md`) name the same thing, the same table reads the same in both. The recipe is Manfred's authoring layer and the scene file is the engine's runtime data; `manfred build` writes the second from the first. An old spelling is refused with a problem that names the new one, and there are no aliases.

| Old recipe spelling | Now |
|---|---|
| `[sun]` with `toward`, `color`, `irradiance` | `[sun]` with `model = "authored"` and the same keys |
| `[daylight]` with `hour`, `day`, `latitude`, `heading` | `[sun]` with `model = "daylight"` and the same keys |
| `prepare = "capped"` in `[sky]` | `[sky.prepare]` with `cap = 60.0` and `mean = 0.3` |
| `balance = [r, g, b]` in `[sky]` | `balance` in `[sky.prepare]` |
| `--set` and variant paths `daylight.hour`, `sky.balance` | `sun.hour`, `sky.prepare.balance` |
| `colour` | never a recipe key; `color` everywhere, and `colour` is refused as any unknown key |

- **`[sun]`** takes `model`, required: `"daylight"` (`hour` and `latitude` and `heading` required, `day` 172, `reference_hour`) or `"authored"` (`toward` and `irradiance` required, `color` white, `radius_deg`). Each model takes only its own keys.
- **`[sky.prepare]`** takes `cap`, `balance` and `mean` as the engine's bake does, and the table needs one of them.
- **`[look]`** stays. The engine's `[finish]` is the post chain's own keys (`style`, `bloom`, `vignette`, `tone` and the rest), which `manfred` does not hold: `[look]` is what an author says (`view`, `exposure` and a `grade` of five amounts from 0 to 1, haze among them, which is the room's and not a post pass), and the writer turns it into a `[finish]` of numbers. A `[finish]` in a recipe is refused with a pointer to `[look]`. The finish's colour lookup table is an author's grade too, so it is `[look]`'s `lut` and `lut_amount`, which the writer passes to the scene's `[finish]` under the same names.
- **The same, unchanged:** `[sky]`'s `kind` (`analytic`, `hdr`, `room`), `path`, `rotation_deg` and `intensity`; `[sky.room]` with its `[[sky.room.lights]]`; and `[[light]]`'s `name`, `position`, `color`, `intensity` and `radius`.

**Left different on purpose:**
- **Daylight defaults.** The scene file defaults `latitude` to 45 and `heading` to 180, and a recipe that left them out would read differently in the two. The recipe requires both, so it never does. `reference_hour` is Manfred's 16, where the scene file's default is 12: the recipe accepts only 16.0 or the key left out, and the writer writes `reference_hour = 16.0` into the scene.
- **An authored sun's `hour`** is refused: Manfred's authored light follows the hour of the shot or the window, which no scene key says. `sky_fill` is Manfred's alone; `radius_deg` is the scene's `radius`.
- **`[[light]]`'s `shadow` and `range`.** The recipe's defaults are `false` and 0 (reach derived from the intensity, as `light.rs` does); the scene file's are `true` and 10. The writer writes the resolved values.
- **The sky.** `kind = "mix"`, `source`, `fill`, `[sky.shade]` and `ambient = "authored"` (the room scaled by the sun's hour) are Manfred's. The scene's `ambient` is a number on the analytic sky, `turbidity` and `ground_albedo` are not recipe keys, and a `mix` is written as two layers whose weights carry `fill`.
- **Cameras.** The recipe's `[[camera]]` list with `position`, `target` and `fov_y_deg` is not the scene's single `[camera]` with `at`, `look_at` and `fov`; the writer picks one and converts.
- **Movers, variants, `[sidecar]` and `[bake]`** have no scene spelling and stay on the authoring side: the scene's `[[mover]]` is a preview motion with a kind, a period and a curve, and a recipe mover has none.
- **`[trace]`** is the scene's own table, with `transmissive_shadows`, `clamp_indirect` and `filter_glossy`; `spp`, `threshold` and `min_samples` are `manfred render`'s alone and stay out of the scene.

## Objects

Common keys on every `[[object]]`:

| Key | Meaning |
|---|---|
| `name` | exact, unique, the glTF node name the app looks up |
| `id` | optional: 10 lowercase Crockford base32 characters (`0-9`, `a-z` without `i`, `l`, `o` and `u`), unique among the objects. A format 1 scene derives the ids of the object's entries from it in place of the recipe's path and the object's name, so they survive a rename. The window writes it on the first save of a recipe that writes a scene (`[output] scene`), as the id the writer derives for the object's first node today; `manfred_build::object_ids` gives it to both |
| `kind` | one of the kinds below |
| `at`, `rotate`, `scale` | the node transform, applied into the mesh unless `keep_transform = true`. They compose as T·R·S. `rotate` is XYZ Euler in degrees in the recipe's Y-up frame, applied X, then Y, then Z (the matrix Rz·Ry·Rx). From Blender's Z-up: a position (x, y, z) becomes [x, z, −y], and a rotation Rx(θ), Ry(θ), Rz(θ) becomes Rx(θ), Rz(−θ), Ry(θ) |
| `parent` | another object's name; transforms compose, the GLB stays flat unless `[output] hierarchy = true` |
| `material` | a name resolved through the app's registered catalogue, then the recipe's own `[materials]`; or `materials = ["a", "b"]` for slots, assigned by face groups the kind defines. Only `cylinder` defines groups today: `[side, caps]`. Every other kind takes one name |
| `smooth_angle` | split normals on edges sharper than this; absent means flat shading; `smooth = true` means smooth everywhere |
| `flat_faces` | `true` (default `false`) keeps a large flat face's own normal at all its vertices instead of smoothing it into its bevel; needs `smooth_angle`; see [Flat faces](#flat-faces) |
| `bevel` | `{ width, segments, angle = 30, clamp = true }` for kinds that take an edge profile |
| `uv` | `"local"` (the default: the builder's own UVs, or a box projection where the kind has none), `"plate"`, `"none"`, or an override table, `{ kind = "rect", … }` or `{ kind = "cylinder", … }`; see [UVs](#uvs) |
| `detail_uv` | `{ texels_per_m, gutter = 16 }` adds a second UV set, TEXCOORD_1: a unique layout for detail bakes; see [Detail UVs](#detail-uvs-texcoord_1) |
| `shadow` | `"cast"` (default), `"only"` (an occluder), `"none"` |
| `two_sided` | draw both faces |
| `hidden` | built, not exported; useful for cutters you want to see in the window |
| `dynamic` | `true` or `false`: whether the object moves or changes at run time, so a baked plate leaves it out and the live view draws it over the plate. Without it, a mover's part, a card that faces the camera, and an object whose `parent` is dynamic are dynamic, and everything else is static, as pfx derives it. `true` marks what the app moves itself; `false` keeps a mover's part in the plate. A text takes it too, under "Text" below; a pocket doesn't |
| `morph` | `{ <target> = <weight>, … }` on a `mesh` object only: the weights of the file's morph targets; see [Morph targets](#morph-targets) |
| `proxy` | `"box"` (the default) or `"hull"`: the invisible stand-in of a static object in the scene's proxies file, under "The scene". Only with a plate folder (`[bake.plates] dir`), never on a dynamic object, a text, a pocket or a hidden object |

A part whose material the live renderer draws as glass (`transmission` above 0, or the glass or liquid family) is a glass surface, and it is not also an opaque instance. `shadow = "cast"` sets `glass::Surface::casts_shadow`, so the surface draws its own tinted sun shadow: transmission × base, or transmission × Beer–Lambert over the thickness when both are set (`glass::cast_tint`). `shadow = "none"` draws the glass and casts nothing. `shadow = "only"` keeps the surface with `shadow_only: true` and `casts_shadow: true`: it casts its tinted shadow and is not drawn. A part of a mover carries the mover's `clip` and `clip2` planes on its surface, so glass is cut at them as the opaque parts are. Glass tints the sun's shadow and, with engine v0.19.0, the shadowed local lights.

### Flat faces

`flat_faces = true`, with `smooth_angle`, stops a large flat face from being smoothed into its bevel. Without it the face's corner vertices share a normal with the bevel's first segment (18° apart is under a `smooth_angle` of 35°), so the interpolated normal tilts across the face by up to a few degrees, the way the triangulation falls. With it:

- A **planar region** is a connected set of triangles, joined edge to edge, whose normals are all within 0.5° of the normal of the region's first triangle (the lowest triangle index it contains). It is the unit the rule works on, so a face split into any number of triangles is one region, and a curved surface of small facets is not.
- A region is **larger** when its area is at least twice that of every neighbouring region it would otherwise be smoothed with (neighbours across edges under `smooth_angle`; sharper edges are split anyway). The ratio of 2 is a fixed number so that a bevel's segments, each a thin strip beside the face, never rival it, while a low-poly curved surface, whose facets are all alike, never has a face of its own.
- A larger region keeps one normal at every vertex, its area-weighted mean, constant across the face whatever the triangulation. Its edges to every other region are split. Everything else smooths by `smooth_angle` as before: the bevel's segments stay smooth among themselves and into the object's other curved faces, and only the bevel's vertices on the face's boundary stop averaging in the face.
- Deterministic; without the key every mesh is byte for byte as before. A pocket's floor, a slab's front and back, a box's faces and a cylinder's caps are regions of this kind.

Kinds:

Every count that sizes a mesh (`segments`, `rings`, `ring`, `grid`, `bevel.segments`, `profile.segments`, `detail_uv.gutter`) is a whole number from 0 to 4096, the same cap as a generator's counts; a larger one is refused when the recipe is read.

- **`box`**: `size = [x, y, z]`. With `bevel`, a rounded box.
- **`cylinder`**: `radius`, `depth` (or `radius_px` and `depth_px` under a `[plate]`), `segments = 64`, `caps = "ngon" | "none"`, axis Y. Blender's topology, so a Blender cylinder's counts match.
- **`sphere`**: `radius`, `segments = 96`, `rings = 48`.
- **`plane`**: `size = [x, z]`, a single quad; `grid = [nu, nv]` subdivides it. A card bends with the shaping keys:
  - `bend = { radius, form = "wrap" | "lift" }` puts it on a drum. `wrap` (the default) wraps it round; `lift` keeps the edges across v in the plane and raises the middle toward the face by the drum's sag.
  - `curl = { width | width_px, depth }`: the two edges across v curl back by `depth·(1 − cos θ)` over `width`.
  - `cup = { from, depth }`: the sides past `from` (a fraction of the half width) bend back by `depth·t²`.
  - `jag = [...]` or `jag_px`: the two side columns move by these amounts, cycling by grid row, inward on even rows and outward on odd ones.
- **`disc`**: `radius`, `segments`; an n-gon fan with no centre vertex, as Blender's.
- **`torus`**: `major`, `minor`, `segments = 96`, `rings = 24`.
- **`capsule`**: `radius`, `length` (the straight section between the two hemispheres' centres, not the total length); built with 32 segments and 8 rings.
- **`outline`**: a 2D shape extruded between `front` and `back` depths with an edge `profile`: slabs, discs, keys, buttons, frames, washers and glyphs.
  - **Holes:** an outline whose shape has holes extrudes as one closed solid: the front and back faces are cut around the holes, an outer wall runs along the outer contour and an inner wall along each hole, and the `profile` rounds both, outward on the outer wall and into the hole on each inner wall, its sharp corners included. The normals face out of the solid, an inner wall's into its hole. Separate pieces, each with its own holes, are one object, as a `union` of separate shapes is.
  - A hole comes from a `difference` whose cutter lies inside the outer contour, from a `union` that closes round a gap, or from `holes = [outline, …]` beside the outer `outline`.
  - `difference` and `union` are set operations (nonzero): a cutter removes everything inside its outer contour, and a part of a `union` that lies inside another part's hole stays, as an island. `holes` is even-odd: the outline and each entry are contours, and a contour inside a hole is solid again, so `outline = { circle = [0, 0, 0.05] }` with `holes = [{ circle = [0, 0, 0.04] }, { circle = [0, 0, 0.03] }, { circle = [0, 0, 0.02] }]` is two rings, one inside the other.
  - **Refused, at the key:** an entry of `holes` that touches or crosses the outline's edge or another entry, or lies outside the outline (`object[frame].holes[1]`); a cutter or a union part that touches the edge or another hole, naming both (`` `outline.difference[1]` touches `outline.difference[0]` at [0.1, 0] ``); and a `profile` wider than half the narrowest wall between a hole and the outer contour or between two holes, at `profile.width`, naming the width, the wall and the widest width it takes. Within the last 5% below half the wall the profile is built at 95% of half, so the faces never meet. A cutter that crosses the edge is a notch, not a hole, and stays allowed.
  - `outline = { rect = [x0, y0, x1, y1], radius = r }`, `{ rect = …, radius = r, turned = deg }`, `{ circle = [cx, cy, r], segments = 64 }`, `{ points = [[x, y], …] }`, `{ points = [[x, y, r], …] }` with a fillet radius per corner, `{ superellipse = { half = [a, b], exponent = n, segments = 64, centre = [0, 0] } }`, `{ union = [outline, …] }`, `{ difference = [a, b] }`.
  - **`superellipse`** is the closed curve `|x/a|^n + |y/b|^n = 1` about `centre`: 2 is an ellipse (with `a = b`, the circle of the same segments), 4 a squircle, and higher exponents square it off. Its points run counter-clockwise from `+x` as `(a·cos^(2/n) t, b·sin^(2/n) t)`, built a quarter at a time and mirrored, so it is exact at the four axes (`[cx + a, cy]`, `[cx, cy + b]` and so on), symmetric about its diagonals, and the same bytes every time. `half` is above 0 on both axes, `exponent` above 0, and `segments` a multiple of 4 from 8 up. It reads as a `points` outline of those corners with no fillets, so a profile, a union, a difference and a pocket take it as they take any other outline (`manfred_mesh::outline::superellipse`).
  - `profile = { kind = "round" | "chamfer" | "none", width, segments }` rounds the front and back rims, and also the outline's corners that turn more than 40°, as Blender's angle-limited bevel did on vertical edges. This is what Blender's bevel did on these parts.
  - `turned` is in degrees, counter-clockwise as seen from the camera.
- **`pocket`**: a cut into another object's front face: `into = "body"`, `outline`, `depth` (the floor's absolute depth, on the same axis as the slab's `front` and `back`), `rim = { width, segments }` rounds the pocket's top edge, `floor = { width, segments }` fillets its floor. Pockets take the place of Blender's boolean cutters; they are built in 2D and never through a 3D boolean. A pocket's outline is in its target's outline frame; its own `at`, `rotate` and `scale` are ignored, and its target must be an `outline` that resolves to one shape, holes and all. A pocket that reaches past its target's outer edge or into one of its holes is refused, naming the pocket. A pocket's own outline has no hole.
- **`lathe`**: `profile = [[r, y], …]` revolved around Y, `segments = 128`, `weld = 1e-7`.
- **`sweep`**: `path = [[x, y, z], …]`, `radius` or `radii = [r, …]`, `ring = 16`, `closed`, `caps`. Beads and tubes. Caps are n-gon fans, as Blender's.
  - `along = { outline = <outline>, offset, depth }` replaces `path`. The ring is swept along an outline in the plate frame, pushed out by `offset` along the outline's outward normal, at `depth`. It is closed and capless, with one `radius`. The ring starts on the outward normal and turns toward the camera: a lip round a pocket, a bezel round a slot. Its outline is one contour, with no hole.
- **`mesh`**: `path = "file.glb"`, optional `node`. Imported geometry, glTF only. Without `node`, every node of the file's scene is read into the one object, with its transforms applied, so a GLB of many parts is one mesh. It keeps the file's material names, one slot per material, unless the object sets `material`; its normals come from the object's `smooth` or `smooth_angle`, and it is flat without either. It keeps the file's morph targets, and `morph` sets their weights; see [Morph targets](#morph-targets). It keeps the file's material texture maps, and the recipe's `[materials]` set or replace them; see [Texture maps](#texture-maps).
- **`generator`**: `generator = "app/birds"`, `params = { … }`. Built by a function an app registers under that name, in the app's build binary, which calls `manfred::cli::main` with its registry and behaves as `manfred` does (`docs/apps.md`). Manfred itself registers only its built-ins, named `manfred/…`: [`manfred/road`](#manfredroad), [`manfred/buildings`](#manfredbuildings) and [`manfred/furniture`](#manfredfurniture). The window builds it through the same registry and lets you place it.
  - The generator gets the object, its `params`, the recipe's folder and a seed: the recipe's `seed` mixed with the object's index in the file (`manfred::build::seed`), so two objects never share one.
  - It returns one or more pieces, each a named mesh; each becomes a node with the piece's name. A piece may share its own object's name, never another object's or another piece's.
  - The object's transform, `parent`, `keep_transform`, `shadow`, `two_sided` and `hidden` apply to every piece. The pieces keep their own material slots, normals and UVs unless the object sets `material` (or one name in `materials`), `smooth` or `smooth_angle`, or `uv`.
  - It may also return named materials in the engine's material form, which merge under the recipe's `[materials]`: a recipe entry of the same name wins, and `manfred build` prints a line when the entry differs from the one the generator returned. Two generators returning one name with different values is an error naming both objects. They reach the GLB's material names, `manfred shot`, `manfred render` and the window as the recipe's own do, but never the recipe file. A recipe with a `[bake]` refuses them as it refuses its own.
  - An unknown name is an error that lists the registered generators, or, when no app generator is registered and the name isn't a `manfred/` one, says that the app's own build binary builds it; a generator's own error is reported under `object[<name>].generator`.
- **`card`**: `size = [w, h]`, `image = "file.png"` or `slot = "name"`, `face_camera = false`, `alpha_cutoff = 0.5`. A flat surface with content; it casts shadows. The 2D and 2.5D building block.
  - **The quad** lies in the object's XY plane facing +Z, and its UVs span the image: (0, 0) at the top left, (1, 1) at the bottom right. A card keeps these UVs whatever its `uv` says. The GLB carries the plain quad in the card's `material`.
  - **`image`** is a PNG or JPEG path relative to the recipe, read by its first bytes. `manfred shot` and the window decode every card image once per build, pack them with the engine's `ContentAtlas` into content slot 0, and give each card its crop, so the whole image lands on the quad. Cards that name one file share one cell. The image is the card's albedo, laid `Over` its material: where it is opaque it replaces the material's colour, where it is clear the material shows. A missing or unreadable image is an error naming the card.
  - **The atlas** is square, `Rgba8UnormSrgb` with premultiplied alpha and the engine's 16-texel gutters (level 4, clean to 16× minification), and as small as holds the images: 256² first, doubling to at most 16384². No cards, no atlas. The window uploads it again only when an image's texels change, not on every edit.
  - **`slot`** names an app's live content, which Manfred cannot produce. The card shows a placeholder from the atlas instead: a 64² checker of 8-texel squares in two mid greys inside a 2-texel dark grey frame, opaque, so the card is seen and picked.
  - **`alpha_cutoff`** goes onto the engine instance (0.5 unless set). When it is above 0 and the image has a texel below it, the card stays one quad and `InstanceSurface::cutout` discards where the content's bilinear alpha, read at level 0, is below the cutoff, in the prepass, the opaque pass and the sun and local-light casters. The cut edge follows the image to the texel, and the cut part casts no shadow. A cutoff of 0, or an image with no texel below it, keeps the plain quad with no cutout. `manfred render` cuts at the same authored cutoff.
  - **`face_camera = true`** turns the card each frame, in the window and in `manfred shot`, about its own centre and its own up, so its front faces the camera: the eye's position for a perspective camera, the view direction for an orthographic one. Its up stays the recipe's; a camera straight above or below leaves it as authored.
- **`text`**: `spans = [{ text, family, size, weight, italic, color }]`, `wrap`, `anchor`, `space = "surface" | "overlay" | "lit"`, `on = "object"`. Static text only; what the apps print live stays in the apps. The scene draws it through the engine's text, in `manfred shot`, `manfred render` and the window; the GLB carries only an empty node by its name, with no mesh.
  - **Spans** run on in one paragraph. `size` is required and above 0: metres for `surface` and `lit`, pixels of the still for `overlay`; lines are 1.25 sizes apart. `color` is linear RGB, white by default. `weight` runs from 1 to 1000, 400 by default and 700 bold. `italic` slants the face.
  - **`family`** is `"mono"` by default, JetBrains Mono, embedded in Manfred with its regular and bold faces. Anything else is a TrueType or OpenType file's path, relative to the recipe; the font's own family name picks it.
  - **`wrap`** is the paragraph's width, in metres or pixels as `size`. Without it each line is as long as its text and lines start flush left; with it, lines break at words and align by the anchor's column.
  - **`anchor`** is the point of the paragraph's box that sits at the text's place: `"top_left"`, `"top"`, `"top_right"`, `"left"`, `"centre"`, `"right"`, `"bottom_left"`, `"bottom"` or `"bottom_right"`. `"centre"` by default, and `"top_left"` for `overlay`.
  - **`"surface"`** text is flat and unlit, depth-tested against the scene. Without `on`, it lies in its own XY plane at its transform (with `parent`), reading along +X with +Y up, facing +Z.
  - **`on = "object"`** lays it on that object's front face: the +Z face of its bounding box in its own frame, centred, reading along the object's +X. A `plane` or `disc` lies flat facing +Y, so its text lies on its top, reading along +X with the top of the text toward −Z. The text's own `at`, `rotate` and `scale` then move it within that face, x across, y up and z out, and it is lifted 0.5 mm off the face. `on` and `parent` are not set together.
  - **`"lit"`** is surface text shaded by the scene's sun, sky and lights.
  - **`"overlay"`** text is drawn over the finished image, in pixels. The anchor names both the screen's point and the paragraph's: `"bottom_right"` puts the paragraph's bottom-right corner at the screen's. `at`'s x and y move it in pixels, x right and y down, as plate pixels do; its z, `rotate` and `scale` are not used.
  - **`dynamic`** marks live text: what the app changes or moves is drawn over a plate, and static text is baked into it. Text on a dynamic object (by `parent` or `on`) is dynamic too; without the key, the rest is static.
  - **In the window,** C2 shows a selected text object's spans: each span's text, size and colour. A finished edit, Enter or the focus leaving a field, or the release of a dragged number, is one gesture; Esc in a span's text puts it back.

### Morph targets

A `mesh` object keeps the morph targets of the file's triangle primitives:

- **What is kept:** each target's POSITION deltas, its name from the glTF mesh's `extras.targetNames` (a target without a name is called by its index, `"0"`, `"1"`, …), and the mesh's `weights` as the default weights. The file's NORMAL and TANGENT deltas are not read: Manfred computes the normals from `smooth` or `smooth_angle` on the neutral positions, and each target's NORMAL deltas are the normals the same smoothing gives on the fully morphed positions, minus those. The deltas follow every vertex Manfred splits for UVs and normals, and vertices that sit together but move apart under a target are never merged.
- **Many nodes:** without `node`, the deltas take each node's transform without its translation, and targets of the same name are one target; a primitive without it moves by nothing. A name first seen takes its default weight from the first mesh that has it.
- **`morph = { smile = 0.6, angry = 0.2 }`** sets a target's weight by name; a target not named keeps the file's weight, and a target without one 0. A weight is any finite number: glTF allows below 0 and above 1.
- **Variants and `--set`** reach it by its dotted path, and both may add a target the object's `morph` doesn't name yet, or `morph` itself: `set = { "object.face.morph.smile" = 0.0 }`, `--set object.face.morph.angry=0.5`.
- **Refused:** `morph` on any kind but `mesh` when the recipe is read; a weight that is not a finite number; and at build, a name the file has no target of, in one line naming the object, the file and its targets: `` object[face].morph.smile: head.glb has no morph target `smile`, expected one of: up, wide ``.
- **The GLB** keeps the positions and normals neutral, gives every primitive of the mesh one target per name, in the same order, with its POSITION deltas (with `min` and `max`, as glTF asks) and NORMAL deltas, and carries the object's weights as the mesh's `weights` and the names as its `extras.targetNames`, so a runtime blends them live.
- **`manfred shot`, `manfred render` and the window** draw the mesh posed at those weights, applied on the CPU when it is built; bakes, bounds and proxies read the posed mesh too. A recipe without morph targets builds the same bytes as before.

### manfred/road

`generator = "manfred/road"` builds a street along a `[[route]]`, in world coordinates: leave the object's `at`, `rotate` and `scale` unset, since they would move the road off its route. Offsets across the road are metres to the right of travel; the asphalt sits at the route's height.

```toml
[[object]]
name = "main"
kind = "generator"
generator = "manfred/road"
params = { route = "street", lanes = 2, every = 80, offset = 30 }
```

- **`params`:** `route` (a route's name, required); `lanes` (each way, 1 to 8, default 2); `lane_width` (3.2 m); `sidewalk` (3.0 m from the curb's face to its outer edge); `curb` (its height, 0.15 m); `curb_width` (its top, 0.3 m); `line` (a marking's width, 0.15 m); `dash` and `dash_gap` (3 m and 6 m); `centre` (`"double"`, the default, `"dashed"` or `"solid"`); `edge` (the edge line's centre from the curb, 0.3 m, 0 for none); `crosswalks = [distances along the route, m]`, or `every` (m) with `offset` (default half of `every`); `crosswalk_depth` (4 m along the route); `stripe` and `stripe_gap` (a zebra stripe's width and the gap between, 0.5 m and 0.6 m); `step` (the longest step along the route, 1 m: curves step finer, 0.05 rad a step, down to 0.1 m); `seed` (default the object's). Any other key is refused. A listed crosswalk off an open route is refused; `every` keeps the ones that fit.
- **Pieces:** `<object>.asphalt` (one ribbon, UVs in metres: u across from the left curb, v along the route), `<object>.curbs` (each curb's face and top), `<object>.sidewalks` (their tops and outer faces), `<object>.markings` (lane lines dashed, the centre line and the solid edge lines, 2 mm above the asphalt and broken at crosswalks), `<object>.crosswalks` (zebra stripes, 2 mm up; absent without crosswalks).
- **Materials:** `road_asphalt` (dark, roughness 0.6), `road_paint` (white, slightly worn through `ageing`) and `road_concrete` (curbs and sidewalks). A recipe `[materials.road_asphalt]` replaces it, for a wet look among others. The seed moves the dashes' phase and wears each zebra stripe's ends by up to 12 cm; the same seed builds the same bytes.
- **Facts:** `road = { route, length, width, lanes, lane_width, lane_centres, curb, sidewalk = [inner, outer], outer, crosswalks, crosswalk_depth }`. `width` is curb to curb; `lanes` is each way; `lane_centres` are the lanes' lateral offsets from left to right, the positive ones driving along the route; `sidewalk` gives its inner and outer offsets and `outer` where buildings may start; `crosswalks` are the distances built.

### manfred/buildings

`generator = "manfred/buildings"` lines a `[[route]]` with city blocks, built in world coordinates (leave the object's `at` and `rotate` at their defaults). It walks the route from its start, one block after another with a gap between, and stands each block square to the route at its middle, its street front `setback` metres from the centre line. A block that a bend or another stretch of the route would bring nearer than `setback` is pushed back, and dropped after four tries, so nothing stands in the road. Each building has a taller ground storey with shopfronts and a fascia, recessed upper walls between corner piers, a band at every floor, sills under its windows, windows on the front and both sides, a cornice, and one to three plant boxes on a flat roof. One of five palettes per building: concrete, brick, sandstone, white render, or a dark glass curtain wall whose windows run in ribbons.

| param | default | meaning |
|---|---|---|
| `route` | required | the `[[route]]` it lines |
| `side` | `"both"` | `"both"`, `"left"` or `"right"` of the direction of travel |
| `setback` | `11.0` | metres from the centre line to the street front (a 2x2-lane road and its sidewalk) |
| `block` | `[8.0, 24.0]` | frontage of a building, metres, at least 4 |
| `height` | `[12.0, 60.0]` | height, metres, rounded to whole storeys |
| `depth` | `[12.0, 20.0]` | depth back from the street front, metres |
| `gap` | `[0.0, 4.0]` | space between neighbours, metres |
| `storey` | `3.2` | floor height, metres, 2 to 10; the ground storey is 1.25 of it |
| `window` | `{ width = 1.4, spacing = 3.0 }` | window width and centre spacing, metres; the width is at least 0.2 m less than the spacing; a window is 0.55 of a storey tall |
| `lit` | `0.25` | the share of windows lit (twice that for shopfronts), 0 to 1 |
| `seed` | the object's | a whole number; the same seed and params build the same bytes |

A range is a number or `[min, max]`. An unknown param is refused.

- **Pieces:** `<object>.facades` (walls, piers, bands, sills, cornices), `<object>.windows` (glass), `<object>.lit` (the lit windows, emissive), `<object>.roofs` (roof slabs and plant boxes).
- **Materials** it returns: `buildings_concrete`, `buildings_brick`, `buildings_sandstone`, `buildings_render`, `buildings_curtain`, `buildings_trim_stone`, `buildings_trim_cream`, `buildings_trim_metal`, `buildings_glass`, `buildings_lit` (warm emission), `buildings_roof` and `buildings_plant`; a recipe `[materials]` entry of the same name restyles them.
- **Facts:** `buildings = { count, setback }`, the buildings placed and the setback used.
- **Weight:** about 500 to 1,000 triangles a building; a 600 m street lined on both sides stays well under 200,000.

### manfred/furniture

`generator = "manfred/furniture"` lines a route with street furniture: lamp poles on the sidewalk, the cables between them, and a few extras. Its pieces are in world coordinates, laid along the route, so leave the object at the origin (its `at`, `rotate` and `scale` still move every piece and light).

```toml
[[object]]
name = "furniture"
kind = "generator"
generator = "manfred/furniture"
params = { route = "main", spacing = [22, 30], crosswalks = [120.0, 310.0] }
```

| Param | Default | Meaning |
|---|---|---|
| `route` | needed | the name of a `[[route]]` |
| `offset` | `8.2` | the pole line's lateral distance from the route's centre, in metres, on the sidewalk |
| `spacing` | `[22, 30]` | the gap between poles on a side, drawn in `[min, max]` metres; min is 2 or more |
| `sides` | `"both"` | `"both"`, `"left"` or `"right"` of travel; each side draws its own gaps |
| `height` | `8` | the pole's height in metres |
| `arm` | `1.8` | the arm's reach over the road, below `offset` |
| `lamp` | `true` | lit heads: an emissive lens and a spot light each; `false` leaves plain lenses and no lights |
| `wires` | `true` | a cable between each two neighbouring poles of a side, from top to top, hanging `sag` metres at the middle (and from the last pole to the first on a closed route) |
| `sag` | `0.4` | the cables' drop at their middle |
| `extras` | `true` | at each crosswalk, a traffic signal and four bollards on each side; signs between poles |
| `signs` | `0.2` | the chance of a sign between two neighbouring poles, away from crosswalks |
| `crosswalks` | `[]` | distances along the route, as numbers or tables with `at` (or `distance`), such as a street generator's facts; a pole within 3.5 m of one moves past it |
| `max_lights` | `24` | the most spot lights it returns, the ones nearest the route's start first |
| `intensity` | `60` | each lamp's intensity |
| `range` | `24` | each lamp's range in metres |
| `color` | `[1.0, 0.72, 0.42]` | each lamp's linear colour, a warm sodium white |
| `cone` | `[35, 60]` | each lamp's inner and outer cone in degrees |
| `seed` | the object's seed | the seed for the gaps and the signs |

- **Pieces:** `<object>.poles` (posts, plinths, arms and head shells), `<object>.wires`, `<object>.lamps` (the lenses, emissive warm when lit) and `<object>.extras` (signals, bollards and signs); a piece with nothing in it is left out.
- **Materials** it returns: `furniture_metal`, `furniture_wire`, `furniture_lamp` (emissive) or `furniture_lamp_off`, `furniture_sign` and `furniture_signal` (the lit red lamp); a recipe entry of the same name wins.
- **Lights:** each lit lamp is a spot light under its head, `<object>.lamp<n>` counted from the route's start, pointing down and a little over the road, with no shadow. The scene writes them as `[[light]]` tables with `kind = "spot"`, `position`, `direction`, `cone`, `color`, `intensity`, `range` and `shadow`, after the recipe's own lights, while the engine has room (4 local lights with pfx today); `manfred build` prints a line naming the ones left out.
- **Deterministic:** the same route, params and seed build the same bytes.

### Texture maps

A material carries texture maps, kept from a `mesh` object's file or set under `[materials.<name>]`. A recipe without them builds the same bytes as before.

- **What is kept from a mesh file:** for each glTF material the read primitives use, its `baseColorTexture`, `normalTexture` with its `scale`, `metallicRoughnessTexture` and `occlusionTexture` with its `strength`, each with its `texCoord` set, its sampler (filters and wrap) and, on the base and metallic-roughness textures, its `KHR_texture_transform`; and with them the material's `baseColorFactor`, `metallicFactor` and `roughnessFactor`. An image is kept when it is a PNG or a JPEG, embedded in the file's buffers or a file beside it, and its bytes are kept as they are, never decoded or encoded again. A texture whose image is neither, or a `data:` URI, is not kept. A material with no kept map writes as before, factors included. The maps follow the file's material names, so an object that sets `material` draws its own material and leaves them behind.
- **The keys,** beside the material's engine keys (`base`, `roughness`, `metalness` and the rest), spelled after the engine's own names:

  | key | glTF slot | default |
  |---|---|---|
  | `base_map = "tex/face.png"` | `baseColorTexture` | |
  | `normal_map` | `normalTexture` | |
  | `normal_scale` | `normalTexture.scale` | 1.0 |
  | `roughness_map` | `metallicRoughnessTexture`: roughness in G, metalness in B, as glTF packs them | |
  | `occlusion_map` | `occlusionTexture` | |
  | `occlusion_strength` | `occlusionTexture.strength`, from 0 to 1 | 1.0 |
  | `detail_normal_map` | the material's `extras.detailNormalTexture`, on TEXCOORD_1 | |
  | `detail_normal_scale` | its `scale` | 1.0 |

  A path is relative to the recipe; a map the recipe sets is on TEXCOORD_0 with no sampler (glTF's repeat and the viewer's filters), except the detail normal map on TEXCOORD_1, which `detail_uv` makes. A map the recipe sets replaces the file's map of that slot; a slot it doesn't set keeps the file's. `normal_scale`, `occlusion_strength` and `detail_normal_scale` set the scale of the slot's map, the file's or the recipe's. The keys never reach the engine's material: `manfred check` and the scene file read the rest of the table as before.
- **Refused:** a key whose value is not a path or a number (a strength outside 0 to 1) when the recipe is read; and at build, at the key (`materials.skin.base_map`), a file that cannot be read, a file that is not a PNG (read by its first bytes), and an image too large for a card, read from its header alone as a card's is: a side whose 16-texel cell with its gutters passes 16384. A mesh file's image past that size is refused at the object's `path`.
- **The GLB** embeds every image once in its binary chunk, in a buffer view of its own, with its `mimeType`; images are read once per build, and two maps of the same bytes share one image. Samplers and textures are shared the same way. A material with maps has the file's factors, or without a file 1.0 base colour under a base map, metalness and roughness 1.0 under a roughness map, and Manfred's 0.8 grey, 0 and 0.5 otherwise. `KHR_texture_transform` is listed in `extensionsUsed` when a map carries it. The same inputs give the same bytes.
- **The views:** `manfred shot` and the window draw the base, normal and roughness maps through the engine's material maps (`Renderer::set_maps`), which multiply the material's `base`, `roughness` and `metalness` (glTF's factor times texture, with the recipe's material as the factor) and bend the normal toward the map's by `normal_scale`, clamped to 0..1. They read TEXCOORD_0, repeat as glTF's default sampler does, and ignore a map's sampler and transform. Each kind's maps are resized to the largest of them, at most 4096 texels a side. `manfred render` draws the base map alone, as content multiplied over the material's `base`, once over UV 0..1. The occlusion and detail normal maps are only carried in the GLB: no view draws them, and `manfred render` draws no normal or roughness map, as the engine's tracer reads none.

## Includes and uses

Recipes that share a camera, a sky, lights or parts keep them in files of their own: a **rig file** that recipes include, and **part files** that recipes place copies of. Both are plain TOML files beside the recipes, read on every build, so a change to the rig reaches every recipe that includes it.

```toml
manfred = 1
name = "panel"
include = ["rig.toml"]

[sky]
intensity = 1.5

[[object]]
name = "board"
kind = "box"
size = [0.2, 0.1, 0.01]

[[use]]
file = "parts/knob.toml"
prefix = "left_"
at = [-0.05, 0.05, 0.0]

[[use]]
file = "parts/knob.toml"
prefix = "right_"
parent = "board"
at = [0.05, 0.0, 0.005]
rotate = [0.0, 0.0, 45.0]
scale = 1.5
```

The rules follow the engine's `scene.toml` `include` where they fit (pfx's `docs/scenes.md`):

- **Paths** are relative to the file that names them: an `include` or a use's `file`, and a sky's `path`, a card's `image`, a mesh's `path` and a span's font `family` inside an included or used file. An absolute path is refused. A `"${params.…}"` inside one reads the recipe's own params, and a `_px` key the recipe's own `[plate]`.
- **`include = ["rig.toml", …]`** merges each file's tables into the recipe, in order and depth first: an included file's own includes come before it, and every included entry comes before the recipe's own. A file is included once per recipe; a second include of it is refused, and so is a cycle, naming the chain (`a.toml → b.toml → a.toml is a cycle`). A missing file is an error at the include that names it: `include: cannot read nope.toml: no such file`.
- **What an included file holds:** `[[object]]`, `[[camera]]`, `[[light]]`, `[[particles]]`, `[[mover]]`, `[[route]]`, `[[use]]`, `[sun]`, `[sky]`, `[look]`, `[flare]`, `[motion_blur]`, `[haze]`, `[materials.<name>]`, its own `include`, and optionally `manfred`. `name`, `seed`, `params`, `sound`, `[plate]`, `[output]`, variants, `[sidecar]` and `[bake]` belong to the recipe and are refused in an included file.
- **Names:** an object, camera, light, mover group, route, non-empty use prefix or material is defined once across a recipe and the files it includes and uses. A second definition in another file is an error naming both files: ``light[lamp]: `lamp` is defined in rig.toml and in panel.toml``. The recipe overrides an entry by changing it in the file that holds it, never by defining it again.
- **Tables:** `[sun]`, `[sky]` and `[look]` are set by one included file at most (two siblings setting one is refused, naming both), and the including file's own keys win over the included ones key by key, through nested tables such as `[sky.prepare]` and `[sky.room]`. Above, the panel keeps the rig's whole sky and changes only its intensity. An array or an inline table is one value and is replaced whole.
- **A problem in another file** is reported at the recipe's line that brings the file in, with that file's own name and line before the key: `manfred check panel.toml` prints `panel.toml:3: rig.toml:24: light[lamp].colour: unknown key …`, and `Recipe::origin_of` gives the file and line of any key.

**`[[use]]`** places a copy of another recipe file's objects. It takes the place of concatenating part files with a shell script and renaming objects with `sed`.

| Key | Meaning |
|---|---|
| `file` | the part file, relative to the file that holds the use. It may be a whole recipe that opens and builds on its own: only its objects are copied, after its own includes and uses; its cameras, lights, sky, movers, variants and the rest stay its own |
| `prefix` | put before every copied object's name, and before every `parent`, pocket `into` and text `on` inside the copy, so the copy's references stay inside the copy. Without it (or `""`), the copy keeps the part's names: one placement of a part whose names are already its own |
| `at`, `rotate`, `scale` | the copy's placement, as an object's (`at_px` under a `[plate]` too). It composes with each copied root object's own placement as T·R·S times the object's, so a root at `at = [0.1, 0, 0]` in a use at `[1, 0, 0]` turned 90° about Y lands at `[1, 0, −0.1]`. A turn about the same single axis adds degrees; any other pair of turns is converted back to XYZ Euler degrees. A scale that differs between axes places only unrotated objects and is refused otherwise, since the result would shear |
| `parent` | an object of the using recipe: every copied root object takes it as its parent, so the placement is in that object's frame |

- A **root** is a copied object without a `parent` in the part; children keep their own placement in their parent's frame. Pockets, text `on` an object and overlay text follow their target and take no placement.
- **Order:** the copies follow the recipe's own objects, use by use, each in the part's order. `Recipe::uses` lists every use with the names of the objects it placed; `Recipe::copy_of(name)` finds the use behind a copy.
- **Uses nest:** a part may use other parts; their copies are copied again with the outer prefix before the inner one (`m_a_bit`), and the placements compose level by level. A part that uses itself, directly or not, is a cycle.
- **After the copy**, the copies are objects like any other: variants `drop` them by glob, `--set object.left_cap.radius=0.01` changes one copy, and `--set use.left_.at=[0, 0, 0]` moves a whole copy. A variant or `--set` replays on the merged recipe, includes and copies included.
- **Refused:** a use without `file`, a part with no `[[object]]`, a copy whose name another object already has (naming both files), and a `parent` that names no object of the recipe or one of the use's own copies (`manfred check`).

**In the window**, a recipe with includes and uses opens, edits and saves as one recipe:

- **Write-back:** each edit goes to the file that holds the key. A light from `rig.toml` moved in the window is written into `rig.toml`; `[sky] intensity`, which the panel overrides, goes into the panel; a key new to an included table goes to the file that holds the table. A new object, camera or light goes into the recipe itself. Undo puts an entry deleted from an included file back into that file at its place. A file the window did not change is never written, and an included file changed on disk since the window read it is re-read, and the outside change wins, as the recipe's own does.
- **Copies** show in the outliner as the objects they are, under their prefixed names (`left_base`, `left_cap`), and are picked, framed and shown like any other. They are read-only: an edit to one is refused with a sentence that names the use and its file (`` `left_cap` is a copy that use[left_] places from parts/knob.toml; change the part in parts/knob.toml, or move every copy of it by the use's at, rotate and scale ``). The `[[use]]` entry itself is editable by its prefix (`use.left_.at`), or by its place when it has none, and moving it moves the whole copy.
- `Files::paths` lists every file a recipe read, the recipe first, for a caller that watches them.

## UVs

An object carries one UV set, written as TEXCOORD_0 exactly as the app reads it: u grows right, v grows down the image, as glTF stores it. Blender's v is flipped on export, so a Blender UV `(u, v)` is `(u, 1 − v)` here. An object that asks for a [detail layout](#detail-uvs-texcoord_1) carries a second set, TEXCOORD_1, in the same convention.

- **`"plate"`** projects every face's corners through the plate's camera onto the `[plate]`: `u` across its width, `v` down its height, with the camera's `shift`. A face whose normal points away from the camera (seen from its centre) takes the sentinel `[0.0005, 0.0005]`, Blender's `(0.0005, 0.9995)` after the flip. Vertices split where neighbouring faces disagree.
- **`{ kind = "rect", rect = [x0, y0, x1, y1] }`**, or `rect_px` under a `[plate]`, maps a rectangle of the plate frame onto the atlas: `s` runs from x0 to x1, `t` from the top edge down, and the UV is `atlas = [u0, v0, u1, v1]` at `(s, t)`: a photo, a screen, a printed card.
  - `atlas` defaults to `[0.0, 0.0, 1.0, 1.0]`.
  - `facing = 0.5`: a face takes the rect when its normal's z, toward the camera's side, is above this; every other face takes `away`.
  - `clamp = false`: true clamps `s` and `t` to 0..1.
  - `away = [-1.0, -1.0]`: Blender's `(-1.0, 2.0)` after the flip.
- **`{ kind = "cylinder", degrees = 360, atlas = [u0, v0, u1, v1] }`** wraps the object around its own Y axis, in its own frame before its transform: `s` is the angle from +Z toward +X, with `degrees` at the middle of the atlas, and `t` runs from the top down: a printed vase or mug wall.
  - `band = [y0, y1]`: `t` runs from 0 at y1 to 1 at y0, clamped. Without it, over the object's height, unclamped.
  - `wall = [y0, y1]`: only faces whose normal points away from the axis and whose centre lies strictly between these heights take the wrap; every other face takes `away`. Without it, every face does.
  - `away = [-1.0, -1.0]`.

```toml
uv = { kind = "rect", rect_px = [1300, 160, 1500, 960], atlas = [0.78125, 0.0, 0.9765625, 0.9765625] }
uv = { kind = "rect", rect_px = [1100, 570, 1400, 710], facing = 0.0, clamp = true, atlas = [0.0, 0.0, 0.25, 0.125] }
uv = { kind = "cylinder", degrees = -30.0, atlas = [0.0, 0.5, 0.5, 0.25], band = [0.004, 0.09], wall = [0.002, 0.098] }
```

## Detail UVs (TEXCOORD_1)

`detail_uv = { texels_per_m = 400 }` on an object adds a second UV set, written as TEXCOORD_1 beside its TEXCOORD_0, which stays exactly as it was. It is a unique layout: no two points of the surface share a texel, so the engine's detail bake (`pfx bake --detail`, the engine's `docs/bake.md`) can bake the object's procedural layers into maps. An object without the key writes no TEXCOORD_1, and its bytes don't change.

- **Charts:** the faces are grown into planar charts in face order. A chart takes a neighbouring face across a shared edge while that face's normal is within 8° of the chart's first face and its projection overlaps none of the chart's faces. Each chart is projected along that first normal, turned to its smallest bounding rectangle and laid wide.
- **Scale:** `texels_per_m` is texels per metre of surface, in the world frame after `at`, `rotate`, `scale` and `parent`. The map is the smallest power of two from 64 to 4096 texels across that the charts pack into, and one metre spans `texels_per_m / size` of the UV tile. The 8° limit keeps every face within 1% of the asked density. That map size is the texels across the tile that the engine's `[detail.view]` sizing counts, so it can be compared with the size `pfx bake --detail` prints for the part.
- **Packing:** charts are packed in shelves, tallest first, with `gutter` texels (16 by default, at most 64) between charts and around the edge of the tile. A layout that doesn't fit 4096 texels across is an error naming `detail_uv`, which asks for a lower `texels_per_m`.
- **Seams:** a vertex splits where two charts meet. TEXCOORD_0 is unchanged at every corner.
- **Refused:** `detail_uv` with `uv = "none"`, on a `pocket` (it goes on the pocket's target) and on `text`. `texels_per_m` is above 0.
- **Checked:** `manfred diff` marks a node whose TEXCOORD_1 appears or goes (`uvs1 false -> true`) and a change in its layout's area use, the share of the tile its charts cover (`uv1 area use 0.00% -> 70.28%`).

The toybox's backdrop (`tests/golden/toybox/`) carries `detail_uv = { texels_per_m = 400 }`: its 1.6 by 1.0 m plane is one 640 by 400 texel chart in a 1024 map. The 16-texel gutter is the engine's rule: the detail bake dilates 8 texels past each chart and keeps five mip levels, so 16 keeps mips 0 to 4 from mixing neighbouring charts.

## Cameras, sun, sky, lights, movers

```toml
[[camera]]
name = "main"
kind = "perspective"
position = [0.0, 0.12, 1.6]
target = [0.0, 0.12, 0.0]
up = [0.0, 1.0, 0.0]
fov_y_deg = 13.3778
shift = [0.0, -0.6395]
focus_m = 1.6
fstop = 0.0

[[camera]]
name = "plan"
kind = "orthographic"
position = [0.0, 10.0, 0.0]
target = [0.0, 0.0, 0.0]
up = [0.0, 0.0, -1.0]
ortho_height = 2.2
```

- A perspective camera takes `fov_y_deg`, or `focal_mm` with `sensor_mm`, the sensor's longest side in millimetres, fitted to the frame's longer side as Blender's automatic sensor fit does. `shift` is a fraction of the half-height, positive up and right; an app's writer converts it to whatever its loader reads.
- The window's own orbit camera is not in the recipe.

A camera may carry a path, which `manfred clip` follows (`docs/render.md`, clips). It is a table under its `[[camera]]`: `[camera.path]` belongs to the camera written last above it.

```toml
[camera.path]
kind = "orbit"
target = [0.0, 0.03, -0.02]
turns = 1.0
rise_deg = 10.0
radius_scale = 1.0
ease = "linear"
```

```toml
[camera.path]
kind = "keys"
ease = "smooth"

[[camera.path.keys]]
t = 0.5
position = [0.4, 0.3, 0.5]
target = [0.0, 0.03, -0.02]
focal_mm = 35.0

[[camera.path.keys]]
t = 1.0
position = [0.0, 0.6, 0.1]
target = [0.0, 0.0, -0.05]
```

- `kind` is `"orbit"` or `"keys"`; each takes only its own keys. `ease` is `"linear"` (the default) or `"smooth"` (in and out, `3t² − 2t³`), and shapes the whole path's time.
- **Orbit:** the camera turns about the vertical line through `target` and looks at `target`. `turns` (any number; positive turns counter-clockwise seen from above, `0.5` is half a turn) is required. `rise_deg` (0) raises the camera's elevation above the target by that many degrees over the path, from where the camera stands; the elevation at the end stays between −90 and 90. `radius_scale` (1, above 0) scales the camera's distance from the target. With `radius_scale = 1` and the camera's own target, the path starts on the camera's own pose. A camera straight above or below `target` has nothing to turn from and is a problem.
- **Keys:** each key is a time `t` from 0 to 1, after the key before it, a `position` and a `target`, and optionally `focal_mm`. Position, target and focal length run straight from key to key; past the last key the camera holds it. Without a key at 0, the camera's own position, target and lens are the key at 0. A key's `focal_mm` needs a perspective camera with its own `focal_mm` (a key without one takes the camera's).
- Time runs from 0 to 1 over the clip whatever its length. An orbit's frames share the loop, frame `i` of `N` at `i / N`, so a whole number of turns loops without a repeated frame; keys end on their last key, frame `i` at `i / (N − 1)`.
- `--set` reaches the path's values: `camera.main.path.turns=2`.

```toml
[sun]
model = "authored"
toward = [0.3, 0.8, 0.5]
radius_deg = 2.5
color = [1.0, 0.97, 0.92]
irradiance = 3.0

[sky]
kind = "hdr"
path = "assets/sky/window.hdr"
source = "made by the app, no licence"
rotation_deg = 169.4
intensity = 4.2
[sky.prepare]
cap = 60.0
balance = [1.0, 0.92, 0.8]
mean = 0.3
[sky.shade]
hours = { 6 = [0.6, 0.6, 0.7], 12 = [1.0, 1.0, 1.0], 20 = [0.8, 0.6, 0.4] }
[sky.room]
width = 1024
floor = [0.2197, 0.1352, 0.0676]
wall = [0.19, 0.13, 0.07]
ceiling = [0.2845, 0.2276, 0.1593]
[[sky.room.lights]]
name = "window"
az = -64.0
el = 14.0
width = 37.5
height = 29.0
power = 10.585
color = [1.0, 0.86, 0.66]
soft = 0.05
slats = 9.0
open = 0.62
```

- **`[sun]`** is one table with a `model`, as the scene file's is:
  - `model = "authored"`: `toward`, `color` and `irradiance` are the light itself, and `radius_deg` (0.27) its disc. The window and the stills turn and cool it with the hour they light at; the sidecar's `sun` carries it as written.
  - `model = "daylight"`: the sun of a place and a time, `hour` (0 to 24), `day` (172), `latitude`, `heading`, and `reference_hour` (16.0 only, or left out):
    ```toml
    [sun]
    model = "daylight"
    hour = 9.5
    day = 172
    latitude = 44.4
    heading = 0.0
    ```
  - Without a `[sun]` the stills use the solar sun of the reference hour. The sidecar's `daylight` is written as before.
- `[sky]` mirrors the engine's bake recipe keys, so the bake sees the sky the stills see. Every HDR has a `source` line. `kind` is one of four:
  - **`hdr`** draws the file at `path`. A `[sky.prepare]` of `cap = 60.0` and `mean = 0.3` prepares it as the engine's daylight preparation does: luminance capped at 60, balanced toward `balance` at constant luminance when `balance` is set, and the mean scaled to 0.3. A `[sky.prepare]` of `balance` alone balances the file. `cap` and `mean` go together and are those numbers, which is the one preparation Manfred has; any other value is refused naming `sky.prepare`. The table needs one of its three keys.
  - **`room`** draws a lamp room from `[sky.room]` through the engine's room sky. It needs `[sky.room]` and refuses `path` and `[sky.prepare]`. By default the room is the same at every hour. `ambient = "authored"` scales it by the authored sun's hourly ambient, from the recipe's authored `[sun]` (Manfred's default authored sun when there is none), as `pfx bake` scales each anchor: live, the room exposed by that ambient. Only `kind = "room"` takes `ambient`, and `"authored"` is its only value. No sidecar key carries it.
  - **`mix`**: the prepared HDR plus the room's lamps alone, at `fill` (0.25) of the HDR's mean luminance. It needs `path` and `[sky.room]`. Only a mix takes `fill`. A mix is never baked.
  - **`analytic`** is the engine's clear sky with the sun; `intensity` scales its ambient.
- Then, for every kind, the sky turns by the negative of `rotation_deg` (default 0) and is scaled by `intensity` (default 1). Live, a room sky is the engine's room turned by `-rotation` and exposed by `intensity`, as `pfx bake` bakes it.
- `[sky.shade]` tints the sky by hour, interpolated between the hours given.
- `[sky.room]` is the lamp room, in the engine's room keys and defaults. An unknown key is refused.
  - **The room:** `width` is the room texture's width in texels, not metres (1024, from 2 to 8192; the sky is `width` × `width / 2`). `floor` ([0.16, 0.10, 0.06]), `wall` ([0.30, 0.24, 0.18]) and `ceiling` ([0.55, 0.47, 0.38]) are linear RGB: a blend from wall to floor below the horizon and from wall to ceiling above it.
  - **The lamps:** each `[[sky.room.lights]]` takes `name` (optional), `az` (0) and `el` (30) in degrees, with azimuth 0 along +Z and positive turning towards +X. `power` (4) multiplies `color` ([1.0, 0.9, 0.78]). `width` (30) and `height` (20) are angular sizes in degrees, above 0 and below 180. `soft` (0.08) is the edge falloff as a fraction of the half extent. `slats` (0, none) bands the lamp across its height like blinds, with `open` (1) of each band lit.
  - **Beside an HDR:** a recipe may carry `[sky.room]` with `kind = "hdr"`. It is not drawn then, and still reaches the facts as `sky.room`, for an app's loader that draws its own room.
  - **The draft's `[[sky.light]]`** is gone. It is refused with a pointer to `[[sky.room.lights]]`.

```toml
[[light]]
name = "lamp"
position = [0.2, 0.3, 0.1]
color = [1.0, 0.8, 0.6]
intensity = 20.0
radius = 0.02
range = 3.0
shadow = true

[[mover]]
group = "knob"
pivot = [0.1, 0.05, 0.0]
axis = [0.0, 1.0, 0.0]
travel = 0.003
clip = [0.0, 0.0, 1.0, 0.0]
```

- Lights are the engine's point lights, or spot lights with `kind = "spot"`, four at most today; "Shot effects" below has the spot and parent keys.
- A mover's parts are the objects named `mv_<group>` and `mv_<group>__<part>`, or listed in `parts`. The facts carry pivot, axis, travel and clip planes as written. Two movers may not share a `group`.
- **A mover's keys:** `group`, `pivot`, `axis` and `travel` are required; `clip` and `clip2` are optional; `parts` is optional. `axis` is not [0, 0, 0].
  - **`pivot`** is in metres, in the recipe's own axes. `pivot_px = [x, y, z]` reads x and y as plate pixels, as `at_px` does, and z in metres, the depth along the recipe's z (0 when left out).
  - **`travel`** is the distance in metres. `travel_px = 1` is one plate pixel, which is `metres_per_pixel`.
  - **`clip` and `clip2`** are planes `[nx, ny, nz, d]`: a point `p` with `dot(n, p) > d` is cut away while the part moves. `clip_px = [0, 1, 0, row]` and `clip2_px = [0, -1, 0, row]` put the plane on a plate row: the normal must be [0, ±1, 0], and `d` is the normal's y times the row's height in metres. So `clip_px = [0, 1, 0, 151]` cuts everything above row 151, and `clip2_px = [0, -1, 0, 738]` everything below row 738.
  - **In the live view** (`manfred shot` and the window) every piece of every part of a mover carries its mover's `clip` and `clip2` as the engine's per-instance clip planes, in world space as written: the pieces are drawn at their rest pose, so the recipe's axes are the world's. A part is cut, its shadow too. A part two movers claim takes the first mover's planes, as the extras do. `manfred render` clips movers too (`docs/render.md`). The glass parts of a mover are not clipped live until an engine release clips glass surfaces per instance.
  - **In the sidecar:** the facts dialect writes `movers.<group>` with `pivot`, `axis`, `travel`, and `clip` and `clip2` when the mover has them; an app's writer writes them as its loader reads them.

## Routes

A route is a smooth path through the scene, for anything that travels along it: a car that follows a street, a camera rig, a generator that lays a road beside it.

```toml
[[route]]
name = "main"
points = [
  [0.0, 0.0, 0.0],
  [0.0, 0.0, 40.0],
  [12.0, 0.0, 80.0],
]
closed = false
```

- `name` is required and no other route uses it. `points` is required: at least 2 points `[x, y, z]` in metres, Y up (a closed route needs 3), every number finite, and no point at the same place as the next one (for a closed route, the last and the first too). A problem names the route, as `route[main].points`.
- `closed` (false by default) joins the last point back to the first.
- **The curve** passes through every point: a centripetal Catmull-Rom spline (alpha 0.5), which never loops or cusps between points. An open route's ends continue straight past the first and last points' neighbours. Distances along the route are in metres, from an arc-length table built once when the route is read; a distance past an open route's ends holds at the end, and a closed route's wraps around.
- **The frame** at a distance is its position, its tangent (the way of travel), its right (the tangent crossed with world up) and its up. A route that runs straight up or down takes world +Z as its reference instead of world up. Curvature is signed, positive where the route turns right.
- **Editing:** `--set route.main.points=[[0, 0, 0], [0, 0, 50]]` and a variant's `set` replace the points; `closed` the same way. `manfred route <recipe> list|set|add|remove --route <name> [--at <i>] [--point x,y,z] [--json]` lists the routes with their points and lengths, moves point `--at` to `--point`, inserts `--point` before point `--at` (or at the end without `--at`), or removes point `--at`, and writes the recipe file through the same edits the window makes, keeping its comments; a route never goes below its 2 points (3 when closed). Routes in an included file are edited in that file.

## Shot effects

The keys pfx-scene 1.1.0 added for a rendered shot. The recipe spells them as the scene file does (pfx-scene's `docs/format.md`), the writer passes them through, and Manfred validates them by type and range, naming the key. They need `[output] scene_format = 1`: a format 0 scene refuses them at the key. Manfred's own views (`manfred shot`, `render`, `clip` and the window) ignore them, and leave a light with a `parent` out, since it rides an object; pfx draws them when it renders the scene. `--set` and variants reach every key, as `particles.spray.rate`, `light.headlight.cone`, `flare.amount` or `materials.asphalt.wet.puddles`. A colour is `"#rrggbb"` or `[r, g, b]`, each at least 0; the key is `color`, never `colour`.

```toml
sound = "street.sound.toml"

[[light]]
name = "headlight"
kind = "spot"
parent = "hero.body"
at = [0.6, 0.1, 2.1]
direction = [0.0, -0.1, 1.0]
cone = [15.0, 30.0]
intensity = 40.0

[[particles]]
name = "spray"
parent = "hero"
at = [0.0, -0.4, -2.1]
rate_keys = [[0.0, 0.0], [1.0, 200.0]]
lifetime = [0.4, 0.9]
alpha = [[0.0, 0.6], [1.0, 0.0]]
colors = [[0.0, "#d8dde4"], [1.0, [0.5, 0.5, 0.55]]]

[look]
lut = "grade.cube"
lut_amount = 0.75

[flare]
amount = 0.4

[motion_blur]
shutter = 0.5

[haze]
amount = 0.3
height = 0.0
falloff = 0.15

[materials.asphalt.wet]
amount = 0.8
```

- **A parent** (`parent` on a light or a particle system) names an object or a piece's node (`hero`, `hero.wheel_rl`). The writer writes the scene object that holds it: the node itself, or an object's first shown node; one that names nothing shown in the built scene is refused at build, at `light[<name>].parent` or `particles[<name>].parent`.
- **`[[light]]`** also takes `kind`: `"point"` (the default) or `"spot"`. `direction` (`[x, y, z]`, not zero; pfx's default `[0, -1, 0]`) and `cone` (`[inner, outer]` degrees, 0 <= inner <= outer <= 180; pfx's default `[30, 45]`) belong to a spot and are refused on a point light. `parent` with `at` (`[x, y, z]` in the parent's frame, default `[0, 0, 0]`) places a light on an object in place of `position`: a parented light refuses `position`, an unparented one needs it and refuses `at`. A light without these keys writes the same scene as before.
- **`[[particles]]`:** `name` (needed, unique), `parent` and `at` (where particles are born, in the parent's frame or the world's), `rate` (>= 0, per second), `rate_keys` (`[[seconds, rate], ...]`, each >= 0, times increasing; they drive the rate over `rate`), `burst` (whole number), `max` (whole number >= 1), `lifetime` (`[low, high]` seconds, 0 < low <= high), `speed` (`[low, high]`, 0 <= low <= high), `direction` (not zero), `cone` (0 to 180 degrees), `inherit` (0 to 1), `gravity` (`[x, y, z]`), `drag` (>= 0), `size` (`[start, end]`, each >= 0), `alpha` (`[[t, alpha], ...]`, each 0 to 1, t increasing), `color` (one colour) or `colors` (`[[t, colour], ...]`, t 0 to 1), not both, `spin` (two numbers, degrees per second), `stretch` (>= 0), `blend` (`"alpha"` or `"add"`), `lit` (bool) and `seed` (whole number). Every key but `name` is optional and takes pfx's default.
- **`[look]`** `lut`: a `.cube` file, relative to the recipe; `lut_amount`: 0 to 1, only with `lut` (pfx's default when left out). The scene's finish gets one last `[[finish.pass]]` with both, after the tone.
- **`[flare]`:** `amount` (0 to 1), `ghosts` (whole number), `ghost_spacing`, `halo`, `halo_radius`, `starburst` and `threshold` (each >= 0). Every key optional.
- **`[motion_blur]`:** `shutter` (0 to 1, the share of a frame the shutter is open), `max_px` (>= 0) and `samples` (whole number >= 1). Every key optional.
- **`[haze]`:** the outdoor haze: `amount` (0 to 1), `height` (a number), `falloff` (>= 0), `reach` (>= 0, metres the march goes), `shafts` (0 to 1), `steps` (whole number >= 1) and `color`. It needs `height` or `falloff`, unless `look.grade.haze` gives the room's haze, whose `[haze]` it then joins; `amount` and `look.grade.haze` together are refused at `haze.amount`.
- **`[materials.<name>.wet]`:** `amount` and `puddles` (0 to 1), `puddle_scale` (> 0, metres) and `seed` (whole number). Manfred's own materials ignore it; the scene's library writes it on that material.
- **`sound`:** a path ending in `.sound.toml`, relative to the recipe, written as the scene's `sound`.
- **Paths:** `look.lut` and `sound` must be files when the scene is written, and are written relative to the project root, as the scene's other paths are.

## Timeline

```toml
[timeline]
fps = 60
seconds = 6.0
size = [1080, 1920]
seed = 7
```

`[timeline]` gives the recipe a clock: frame `n` is at `t = n / fps`, from frame 0 to the last.

- `fps`: frames per second, a whole number from 1 to 240; 60 when absent.
- `seconds` or `frames`: the length, one of them. `seconds` is above 0 and rounds to the nearest whole frame at `fps`; `frames` is a whole number from 1 up. Both are an error, and so is neither. The frames run from 0 to the last, so `seconds = 6.0` at 60 fps keys 361 frames, at 0 to 6 seconds; a million frames at most.
- `size`: `[width, height]` in pixels, each side from 1 to 16384; `[1080, 1920]`, 9:16, when absent. It is the aspect of the timeline's cameras.
- `seed`: the seed of what the timeline animates (drift, shake); the recipe's own `seed` when absent.

Every key takes `--set` and a variant's `set`, as `--set timeline.fps=30`. A recipe without `[timeline]` has no animation, and builds the same bytes it always did.

- **What animates:** Manfred's own bakers key the objects and cameras the recipe animates, one key a frame. An animated object keeps its mesh in its own frame, as `keep_transform` does, so its key is its whole world transform.
- **The GLB:** the keys are one glTF animation named `timeline`, with one LINEAR sampler a channel: the input is the frame times `n / fps` with their `min` and `max`, the output the translation, rotation (a unit quaternion, x, y, z, w) or scale. A channel whose keys never change is left out. Every animated node is at the top level of the GLB, named after its object or piece (`hero.body`), and starts at its key at 0. A camera the timeline moves is a top-level node `camera.<name>` holding a glTF perspective camera: `yfov` from the camera's `fov_y_deg`, or its `focal_mm` and `sensor_mm` at the timeline's aspect, `aspectRatio` the timeline's width over its height, `znear` 0.05 and no `zfar`. It looks down its own -Z, as glTF's cameras do.
- **Posing:** `manfred shot` and `manfred render` take `--time <seconds>` and draw the scene at that time: each key is interpolated as glTF's LINEAR is, the rotation by slerp, and a camera the timeline moves looks from its pose there. `--time` without a `[timeline]`, or outside 0 to its length, is refused before anything is drawn. `manfred clip <recipe> --timeline` films the timeline: frames 0 to the last at its `fps`, through `--camera`, posed by the timeline when it moves that camera, in place of `--frames`, `--fps` and a camera path.

## Variants, look and bake

```toml
[variant.bare]
drop = ["slat*", "wall_*", "vase", "coin"]
set = { "sun.irradiance" = 0.0 }
remove = ["light.lamp"]

[look]
view = "graded"
exposure = 1.0
grade = { vignette = 0.2, saturation = 1.0, warmth = 0.1, bloom = 0.1, haze = 0.0 }
```

`manfred build --variant bare` applies the variant before building.
- `drop` removes objects by glob.
- `set` and `remove` take dotted paths, with entries of an array of tables addressed by name: `camera.main.fov_y_deg`, `light.lamp`.
- `set` may add a missing last key but not a missing table; a mesh object's `morph` is added when a `set` names one of its targets.
- `remove` of a key that isn't there is an error.
- Sidecar keys are reachable like any other: `remove = ["sidecar.motion"]` drops the `[sidecar.motion]` table below, and `set = { "sidecar.motion.density" = 2.0 }` changes a number in it.

**A variant's files.** A variant writes its own GLB and sidecar, so building it never overwrites the recipe's.
- **The naming rule:** the variant's name goes before the last extension of the file name, in the same folder: `scene/toybox.glb` and `scene/toybox.json` give `scene/toybox-bare.glb` and `scene/toybox-bare.json`. A recipe with no `output.glb` writes `<name>.glb`, so its variant writes `<name>-<variant>.glb`. A variant has a sidecar only when the recipe does.
- **The scene:** a variant writes its own scene when the recipe does, with its name before `.scene.toml` (`scene/toybox.scene.toml` gives `scene/toybox-bare.scene.toml`), or before the last extension of another name.
- **`[variant.bare.output]`** overrides the rule with `glb`, `sidecar`, `bake` and `scene`, each a path as in `[output]`.
- **The bake:** a variant writes no bake unless `[variant.<name>.output]` gives `bake`. The recipe's `[bake]` table stays in that case and goes with it.
- **Checked:** two files of the recipe and its variants may not be the same path; the later one is the problem, reported as `variant.<name>.output`.

`manfred build <recipe> --all-variants` builds the recipe and then every variant, in the variants' name order, and prints each path it writes. `--variant` and `--all-variants` together are a line it cannot parse, and exit 2. `--out <dir>` puts every file in one folder, so the names have to differ there too.

**Overrides on the command line.** `manfred build`, `manfred shot` and `manfred render` take `--set <key>=<value>`, as often as needed, so a caller renders another input without writing a recipe: `--set object.liquid.params.dump=/abs/tmp/lab/a.liquid --set sky.intensity=0.6`.
- **The key** is a dotted path as a variant's `set` takes it, with an array of tables entry addressed by its `name`: `sky.intensity`, `camera.eye.fov_y_deg`, `object.liquid.params.width`.
- **Strict:** it replaces a value the recipe already has, except a mesh object's morph weight, `object.<name>.morph.<target>`, which it may add. A key the recipe does not carry or a table instead of a value is refused in one line that names the key, and the command exits 1: ``manfred build: scene.toml: sky.intensty: --set: the recipe has no value at `sky.intensty` ``. A missing `=` is a line it cannot parse, and exits 2.
- **The value** takes the kind of the one it replaces. A string is the text as given, with no quotes, so a path needs none; a number, a boolean, an array or an inline table is read as TOML and must be of that same kind (an integer and a float are both numbers). A `"${params.…}"` string may be replaced by a number.
- **Paths** in a value are relative to the recipe, as in the file; a caller in another folder passes absolute ones.
- **Order:** the variant first, then each `--set` in the order given, then the usual checks. The recipe file is never written: the override lives only in that run (`manfred_recipe::apply_set`).

## The sidecar table

```toml
[sidecar.motion]
cup = [-0.2, -0.07, 0.12]
radius = 0.02
box_lo = [-0.3, -0.2, 0.0]
box_hi = [0.3, 0.2, 0.2]
density = 1.2
```

`[sidecar]` holds keys the recipe has no table for, written into the sidecar as they are. It is a passthrough rather than a typed table on purpose: such a block is an app's own contract with its loader, its keys change with the app's code, and a typed table here would make every new key a Manfred release. `manfred::runtime` reads `motion` (a steam source, its box and density) from it. The numbers are in the recipe's units and axes, the same ones the sidecar uses: nothing is converted.
- It deep-merges into what the writer wrote, after what generators returned, so it can add a key to `sun` and a key of its own replaces a key the writer wrote.
- Tables and arrays keep their shape; TOML integers stay integers. Like every Manfred sidecar, the keys come out sorted.
- A recipe whose `sidecar_dialect` is `none` and which has a `[sidecar]` is a problem: there is no file for it.

`[look]` is Manfred's own table (see "Spelling shared with `scene.toml`"). It reaches the facts, and the live renderer's post chain reads the same keys. `view` is `"graded"` (the default) or `"standard"`. `exposure` is above 0. `grade` takes `vignette` (0), `saturation` (1), `warmth` (0), `bloom` (0) and `haze` (0), an amount from 0 to 1. Above 0, the live renderer draws the room's haze from it through the engine's `RoomHaze::from_amount`.

`[trace]` holds the tracer's own settings, which `manfred render` and a traced `manfred clip` pass to the engine's `Detail`, and the scene's `[trace]` carries; the recipe's other tools ignore them.
- `transmissive_shadows` (false by default): true passes the sun's, the sky's and the local lights' shadow rays through glass and liquid, tinted as live tints them (`Detail::transmissive_shadows`).
- `clamp_indirect` (0, off): 0 to 10000, the luminance (Rec. 709 weights) that caps each light contribution reached after the first bounce, scaled down to it keeping its hue, as Cycles' Clamp Indirect (`Detail::clamp_indirect`). Light at the first hit is never clamped.
- `filter_glossy` (0, off): 0 to 1, the least roughness of every vertex after a diffuse bounce, as Cycles' Filter Glossy (`Detail::filter_glossy`), so a sharp lobe reached through a diffuse bounce stops throwing fireflies under a small sun while the chrome and glass the camera sees stay crisp. pfx suggests `0.25`, with `clamp_indirect` as the backstop (pfx's `docs/trace.md`, "Fireflies").
- A value outside its range is a problem at its line. Both off, every render keeps its bytes.
- `spp`, `threshold` and `min_samples` (unset): `manfred render`'s adaptive sampling for this recipe, the cap (at least 1), the noise under which a pixel stops (0 or more; 0 runs every pixel to `spp`) and the samples every pixel takes first (at least 2). Each one unset takes the command's default (64, 0.01, 16), and `--spp`, `--threshold` and `--min-samples` override them; `--draft` ignores them. With the denoiser after the trace, a scene whose sunlit surfaces never reach 0.01 sets its own, such as `threshold = 0.05` with `spp = 128`. A traced `manfred clip` and the window's C3 keep their fixed samples.

**The view.** `graded` finishes a frame with the graded chain: exposure, bloom, vignette, saturation, warmth and the ACES tone curve, then sRGB. `standard` finishes it as Blender's Standard view does, so a Manfred PNG can stand beside a Cycles one: linear radiance times `exposure`, clamped to [0, 1], then the sRGB curve, and nothing else (no tone curve, bloom, vignette, saturation, warmth or haze). It applies to `manfred shot`, the window and `manfred render`'s PNG alike; EXR and HDR stay raw.
- **The grade is refused, not ignored.** Under `standard`, a `grade` key off its neutral value (`vignette`, `warmth`, `bloom` and `haze` above 0, a `saturation` other than 1) is a problem at that key, so a recipe never carries a grade that does nothing; `grade = {}` or the neutral values are fine, and `view = "graded"` takes them as before.
- **The facts** carry `look.exposure` and `look.grade`; `view` stays in the recipe.

```toml
[bake]
anchors = [8.0, 12.0, 16.0, 20.0]
samples = 16
[[bake.volume]]
name = "desk"
min = [-0.5, 0.0, -0.5]
max = [0.5, 0.5, 0.5]
spacing = 0.25
```

`[bake]` holds the keys the bake recipe needs and the rest of the recipe can't fill. It is required when `[output] bake` is set, and refused without it.
- `anchors` (required): the hours to bake, rising and distinct, from 0 to 24.
- `samples` (required): above 0.
- `product` (required): the name of the app the bake is for, any lowercase slug of letters, digits and `-` that starts with a letter or digit, as the engine names apps. Manfred keeps no list of apps, and `pfx bake` takes any slug; it names the bake and changes nothing in the light.
- `[[bake.volume]]`: at least one, each with a `name` of ASCII letters, digits, `-` and `_`, unique, plus `min`, `max` and `spacing`. `max − min` must be a whole number of spacings on every axis.

```toml
[bake.plates]
size = [3840, 2160]
overscan = 0.1
scale = 1.25
anchors = [12.0, 16.0, 19.0]
layers = ["color", "depth", "normal", "id", "sun", "transfer"]
key = "desk-v3"
dir = "bakes/desk/plates/desk"
casters = "proxies"

[[bake.plates.view]]
name = "desk"
camera = "main"
```

`[bake.plates]` asks `pfx bake` for baked plates: the static part of the scene path-traced once per anchor at the camera's rest pose, with depth, normal and id layers, which the live renderer composites under what moves (pfx's `docs/bake.md`, "Plates", and `docs/plates.md`). It needs `[output] scene`, since the plate traces the scene file `manfred build` writes.
- `size` (required): `[width, height]`, the live frame in pixels, each 16 to 16384.
- `overscan` (0.1): 0 to 1, the margin the plate adds round the view for drift. `scale` (1): 0.25 to 4 plate texels per live pixel.
- `anchors` (the bake's `anchors`): rising, distinct hours from 0 to 24; the scene's sun is set to each.
- `layers` (all six): some of `"color"`, `"depth"`, `"normal"`, `"id"`, `"sun"` and `"transfer"`, each once. `color` and `depth` are required, and `transfer` needs `sun` and `normal`.
- `threshold` (0.01, above 0), `min_samples` (16, at least 2), `max_samples` (1024, at least `min_samples` and at most 65536) and `growth` (2, above 1): the tracer's adaptive sampling. `seed` (the bake's seed): a whole number.
- `key`: Manfred's own plate key, any string; pfx records it and hashes it into its key, so a new key re-bakes.
- `clamp_indirect` and `filter_glossy`: the `[trace]` keys of the same names and ranges, for the plates' colour passes alone. Either one given here replaces the scene's (0 turns it off for the plates); left out, the plates take the scene's.
- `dir`: the plate folder `pfx bake` writes (`<output>/plates/<view>`), relative to the recipe. With it, the scene carries `[plates]` and a proxies file. `casters` (`"meshes"`): `"proxies"` or `"none"`, what shadows the moving parts in the static objects' place; it needs `dir`.
- `[[bake.plates.view]]`: the views to bake, each a `name` (up to 64 ASCII letters, digits, `-` and `_`, unique) and any of the scene camera's `at`, `look_at`, `up`, `fov` (or `focal` with an optional `sensor`, not both), `shift`, `near` and `far`. `camera` names a perspective `[[camera]]` that fills the keys the view leaves out, converted as the scene's camera is, with `near` and `far` fitted to the scene's bounds. Without a view, pfx bakes one named `main` from the scene's camera, which must then be perspective.

The bake recipe `manfred build` writes:
- **Top-level keys:** `scene`, the GLB; `app`, the recipe's `product`; `materials = "bake-materials.toml"` when the GLB names a material; `anchors`, `samples`, and `seed`, the recipe's seed cut to 32 bits. A daylight `[sun]`'s `day`, `latitude` and `heading` are written when it is set, and `shadow_only_nodes` lists the objects with `shadow = "only"`.
- **The materials:** `bake-materials.toml`, beside the bake file, is the engine's material library (`pfx_materials::Library`) of every material the GLB's shown parts name, each from the app's registered catalogue. A name the catalogue lacks is an error naming `materials.<name>`, so pfx never bakes a part in its default grey.
- **The sky and the sun:** `[sky]` comes from the recipe's sky in the engine's bake keys of v0.19: `kind` is `daylight` for an analytic sky, `hdr` or `room`. A recipe's `[sky.prepare]` of `cap = 60.0` and `mean = 0.3` is written as it stands with its `balance` in the order `cap`, `balance`, `mean`, the chain the stills prepare the HDR with, and one of `balance` alone is a `[sky.prepare]` of `balance`. `[sky.shade]` gives each anchor its interpolated tint, and `[sky.room]` is written only for a room sky. `[sun]` is `model = "authored"` with the sun's `toward`, `color` and `irradiance`; the bake has no sun radius. Without an authored `[sun]` the bake takes the solar sun of the recipe's daylight `[sun]`, as the stills do.
- **The hourly ambient:** `ambient = "authored"` scales each anchor's sky by the authored sun's ambient, as the stills scale it. It is written for an HDR sky under a `[sun]`, and for a room sky with `ambient = "authored"`, which then needs a `[sun]`.
- **Volumes and emitters:** `[[volumes]]` come from `[[bake.volume]]`, and each `[[light]]` becomes a sphere `[[emitter]]` with its position, radius, colour and intensity.
- **Plates:** `[bake.plates]` is written last as `[plates]` in pfx's keys and order, with `scene`, the scene file, and every other key only where it differs from pfx's default (`anchors` and `seed` where they differ from the bake's own, `clamp_indirect` and `filter_glossy` whenever set). Each view is a `[[plates.view]]`; a view with a `camera` writes that camera's `at`, `look_at`, `up`, `fov`, a `shift` other than zero, `near` and `far`, and its own keys win. `dir`, `casters` and the proxies are the scene's, not the bake's.
- **Paths:** every path is relative to the bake file's folder, and pfx reads nothing outside it. So the bake file goes in a folder that holds the GLB, the HDR and the plates' scene: `assets/bake.toml` beside `scene/toybox.glb` and `sky/window.hdr`, not in `assets/scene/`. A path outside the folder is an error naming `output.glb`, `sky.path` or `output.scene`.
- **Refused:** what the bake schema can't express is an error that names the key:
  - a `mix` sky (`sky.kind`). A mix is never baked: the bake's emitters are glTF nodes or spheres, not lamps at infinity, so bake under `hdr` or `room`;
  - an analytic sky with an `intensity` other than 1 or a `rotation_deg` other than 0;
  - a `[[light]]` with radius 0;
  - an object with `shadow = "none"`;
  - a recipe's own `[materials.<name>]`, and a material the app's catalogue lacks (`materials.<name>`): the bake's library holds the app's catalogue alone;
  - a room sky with `ambient = "authored"` and no `[sun]` (`sky.ambient`): the stills then scale it by Manfred's default authored sun under the daylight sun, which the bake can't express.

## The scene

The recipe is Manfred's authoring layer; `scene.toml` is the engine's runtime data, which a viewer opens with `pfx_load::scene::Scene::open`, stages live and traced, and watches (the engine's `docs/scenes.md`). `[output] scene` writes it beside the GLB, with the materials in a library named after it: `toybox.scene.toml` names `toybox.materials.toml`. It is scene format 1 by default ("Format 1" below); `scene_format = 0` writes the engine's older format, which the engine reads through pfx-scene's migration. The bullets below say what both formats hold, in format 0's words; "Format 1" says how format 1 writes the same. In format 0 every path is relative to the scene file, and stays inside the scene's project root (the nearest folder up from the scene that holds a `project.toml`, else the scene's own folder), since pfx-scene refuses a path that leaves it; Manfred refuses one naming its key. The same recipe writes the same bytes. Manfred's own `table` and `toybox` scenes have goldens in both formats: format 1 in `crates/build/tests/scene/format1/`, format 0 in `crates/build/tests/scene/`.

- **Meshes and objects:** each node of the GLB is a `[mesh.<node>]` with `file` and `node`, and an `[[object]]` of the same name. `id` is the recipe object's place plus one, the pick id Manfred's window uses, shared by a generator's pieces. Manfred bakes an object's transform into its mesh, so the object writes `at`, `rotate` and `scale` only for `keep_transform`: the recipe's own, or with a `parent` the composed placement (a shear is refused at `object[<name>].parent`). Under `[output] hierarchy = true` a node's descendants are hidden in its mesh, since they are objects of their own. An object of one material writes `material`, `shadow` when not `"cast"`, `two_sided` when set, and a mover's part its `clip` and `clip2` as `clip`. Hidden objects are not in the GLB, so not in the scene.
- **Materials:** every material the GLB's shown parts name, from the app's catalogue, then the recipe's `[materials]` and the generators' (as the scene draws them). A name in none is an error naming `materials.<name>`.
- **Cards:** a card's image is a `[content.<card>]`, and the card's material is a copy named `<material>+content<slot>` with the engine's content layer on that slot, laid `Over`, one slot per image, four at most. A slot card keeps its plain material: the app paints its live content.
- **Text:** a `text` object with one span is a `[text.<name>]`: the span's words, its font (the embedded mono, written beside the scene as `JetBrainsMonoNerdFont-Regular.ttf` or `-Bold.ttf` by weight, or the span's own file), its size times the placement's uniform scale, its colour with alpha 1, and `at` and `rotate` from its placement, `on` included.
- **The sun:** an authored `[sun]` is `model = "authored"` with `toward`, `color`, `irradiance` and `hour = 16.0`, the sun as the still draws it at its own hour. `[daylight]` is `model = "daylight"` with `hour`, `day`, `latitude`, `heading` and `reference_hour = 16.0`, Manfred's reference. With neither, the daylight of Manfred's default hour, 16, at the engine's day, latitude and heading. `radius` is the recipe's `radius_deg` for an authored sun, 0 to 10 (refused at `sun.radius_deg` outside it), and 0.27 for a daylight sun. An authored sun also writes `reference_hour = 16.0`, so it is exactly as authored at hour 16 and follows the hour as the still's does.
- **The sky:** `analytic` writes the sky's `intensity` as the analytic sky's `ambient` (times the daylight's ambient under `[daylight]`); its `rotation_deg` is not used, as live. `hdr` writes `path`, `rotation_deg`, and `[sky.prepare]` as the bake does (cap 60, the balance, mean 0.3). `room` writes `rotation_deg` and every `[sky.room]` and lamp key. For `hdr` and `room`, `intensity` is the recipe's times what the still multiplies the texels by at its hour: the `[sky.shade]` tint and the hourly ambient (under `[daylight]` for an HDR, and for a room with `ambient = "authored"`). Without `[sky]`, an analytic sky, as the still draws. A `mix` writes `kind = "mix"` with two `[[sky.layer]]`: the prepared HDR (`path`, `rotation_deg`, `intensity` as for `hdr`, and its `[sky.layer.prepare]`) and the room's lamps alone, a `room` layer with `floor`, `wall` and `ceiling` black, its `width` the HDR's, the same `rotation_deg` and `intensity`, and a `weight` of `fill` (0.25 unless set) times the prepared HDR's mean luminance over the lamps' own, the gain the still's mix applies, worked out from the file when the scene is written. The engine bakes the layers into one HDR at the HDR's width: Manfred's own mix. A mix with no lamps writes `weight = 0.0`, the HDR alone.
- **Lights:** each `[[light]]` with every key: `range` resolved (`sqrt(intensity / 0.01)` when 0) and `shadow` written, since the scene's defaults differ. A spot writes `kind`, `direction` and `cone`; a parented light `parent` and `at` in place of `position`. In format 1 each light has an `id`, derived under the kind `light`.
- **Shot effects:** `[[particles]]` (each with an `id` under the kind `particles`, after the lights), `[flare]`, `[motion_blur]`, the outdoor `[haze]` keys, a material's `wet`, the LUT pass and `sound`, as "Shot effects" says.
- **The camera:** `projection`, `at`, `look_at`, `up`, `fov` (vertical, from `focal_mm` at the plate's aspect, or at 1586 × 992 without a plate) or `height`, `near` and `far` as the still fits them to the scene's bounds, and `shift` when it isn't `[0, 0]`. A perspective camera with `fstop` above 0 writes `focal`, `sensor` (the vertical height, so the engine's fov is the camera's), `fstop` and, when set, `focus` (`focus_m`, the distance from the eye along the view, which is the engine's `focus`) in place of `fov`; a `focus_m` at or below the focal length is refused at `camera[<name>].focus_m`.
- **The haze:** `look.grade.haze` above 0, under `view = "graded"`, is `[haze]` with `lo`, `hi` (Manfred's room), `amount`, `phase = 0.35` and `back = 0.7`: the engine's room haze, the same one the still draws. The tracer draws none.
- **The finish:** `[look]` as the still's chain, in the still's own order: `exposure` alone for `view = "standard"`; for `graded` (and without `[look]`) one `[[finish.pass]]` each for `exposure`, a box `bloom` with the still's shape, a power `vignette` (2.4, 0.8, 1.28), `saturation`, linear `warmth` and the ACES `tone`. The engine orders inline keys by name, which puts the warmth after the tone where the still has it before, so the passes are listed in `[[finish.pass]]`, which the engine applies in the order written.
- **Trace:** `[trace]` is written when one of its keys is on: `transmissive_shadows = true`, and `clamp_indirect` and `filter_glossy` when above 0. Off is the engine's default and is not written, so a recipe without them writes no `[trace]`.
- **Dynamic:** an object writes `dynamic` with the recipe's own value when it sets one. Without it, a mover's part and the child of a dynamic object write `dynamic = true`, since the scene carries neither the recipe's movers nor its parents; a card that faces the camera writes nothing, as pfx derives it, and every other object is static. A text writes `dynamic = true` when it is dynamic, its own key or its carrier's.
- **Plates:** with `[bake.plates] dir`, `[plates]` follows `[trace]` with `dir` (relative to the scene), `proxies` and `casters` when not `"meshes"`. The proxies file is written beside the scene, named after it (`desk.scene.toml` names `desk.proxies.toml`): one `[[proxy]]` per static object of the scene, in its order, with `object`, the scene object's name. A `box` is the object's world bounds, `at` their centre and `size` their extent, at least 1 mm on each axis, with no `rotate`; a `hull` is the convex hull of the object's world vertices, `points` and `triangles` counter-clockwise from outside, and an object whose hull holds no volume is refused at `object[<name>].proxy`. Proxies are never drawn: they shadow the moving parts under `casters = "proxies"` and answer CPU picks with the object's pick id.
- **Objects:** a card with an image writes `alpha_cutoff` (the engine's cutout, as the still cuts: the texels of the image below it are dropped); a card that faces the camera writes `face_camera = true`. The engine turns the card about its object's origin, with its `+Y` along the camera's up, where the still turns it about its own centre and keeps the recipe's up, so a card that faces the camera needs `keep_transform = true` (the card's own origin is its centre) and is refused at `object[<name>].keep_transform` without it; a card that is not upright to the camera's up turns differently in the two.
- **Left on the authoring side,** and listed in one note of `manfred build`: movers, which say where a part can go in the app's own axes and units with no kind, unit or motion, so the scene's preview `[[mover]]` has nothing to take (their clip planes are written on their objects); variants, `[sidecar]` and its motion, camera paths, hidden objects (built and not exported, so the GLB has no node for them, and writing them would draw them in the apps), and slot cards' live content.
- **Format 1** (the default, `[output] scene_format = 1`) writes the same scene as pfx-scene's format 1 (pfx-scene's `docs/format.md`, at the rev the root `Cargo.toml` pins), through its types. The engine's `Scene::open`, `SceneWatch` and `Staged` read it through pfx-scene's `Project::scene`, and so does Manfred's window.
  - `format = 1` heads the scene and its material library, and each key is in the order pfx-scene's tables list them: meshes by name, then objects, contents and texts, then the sun, sky, haze, camera, finish and trace. Materials write every key, as format 0 does.
  - `pick` is the object's pick id, where format 0 wrote `id`.
  - Every mesh, object, content, text and material has an `id`: `Id::derive` of `"<recipe>\n<kind>\n<name>"`, the recipe's project-relative path, the entry's table key (`mesh`, `object`, `content`, `text` or `materials`) and its name. An entry made from an object with its own `id` derives from `"<id>\n<kind>\n<n>"` instead, where `n` counts the object's parts from 0, so its ids keep across a rename of the object. An id already taken gets `"\n1"`, `"\n2"`, … appended to its key, as pfx-scene's migration does. The same recipe writes the same ids. The first save in the window gives an object without an `id` its own, the object entry's derived id, so the object entry and its pick keep across the save; the object's mesh, content and text entries then derive from that `id` and change once, and keep across every rename after it.
  - `[authoring]` holds `recipe`, the recipe's path from the project root, and `variant` when the build applied one, in the scene, its library and its proxies file.
  - The plate keys are pfx-scene's, written through its types: an object's `dynamic` and a text's `dynamic` after their other keys, and `[plates]` after every other table but `[authoring]`, with `dir` and `proxies` relative to the root. The proxies file starts with `format = 1`, and each proxy has an `id`, derived as the object's other entries' are, under the kind `proxy`.
  - Every path (the library, the GLB, an HDR, a card's image, a font) is relative to the project root: the nearest folder up from the scene that holds a `project.toml`, else the scene's own folder. A path that would leave the root is refused, naming the key (`output.glb`, `sky.path`, `object[<name>].image`, `object[<name>].spans`) and the file. The recipe's own path may leave it: it is provenance, not a path the scene reads.
- **Refused,** naming the key: a `[sky.shade]` coloured at the drawn hour (`sky.shade`); more than four lights (`light`); a card that faces the camera without `keep_transform` (`keep_transform`), shows a JPEG or a fifth image (`image`); overlay text (`space`); text of more than one span (`spans`) or of an uneven scale (`scale`).

## Extras

The GLB carries what a loader needs about movers and shadows in glTF `extras` on its nodes, so a loader can read them without the sidecar. The extras are additive: the sidecar is written exactly as before, and a GLB without extras loads as it always did.

```json
{"name": "mv_knob", "extras": {"manfred": {"axis": [0.0, 0.0, -1.0], "group": "knob", "pivot": [0.2682219419924338, 0.15170239596469104, 0.008], "travel": 0.003}}}
{"name": "mv_knob__cap", "extras": {"manfred": {"group": "knob"}}}
{"name": "shade", "extras": {"manfred": {"shadow": "only"}}}
```

- **Where:** only on nodes, and only inside one object named `manfred`, so an app's own extras keys never collide with them. A node with nothing to say has no `extras` at all. Meshes and materials carry none. Keys are written sorted, numbers as the sidecar writes them.
- **`group`:** on every part of a mover, the mover's `group`. A part is an object the sidecar rule counts: `mv_<group>`, `mv_<group>__<part>`, or one listed in the mover's `parts`; every piece of a generator object counts. An object two movers claim goes to the first mover in the recipe.
- **A mover's root:** the first visible part of the mover, in the recipe's file order, also carries `pivot`, `axis` and `travel`, and `clip` and `clip2` when the mover has them. The root is a convention and not a parent node, even when `[output] hierarchy = true` nests the GLB; it is the one node per group that has `pivot`. The numbers are the ones `movers.<group>` gets in the facts, in the recipe's own axes and units: nothing is converted. A mover whose parts are all hidden writes nothing.
- **`shadow`:** `"only"` or `"none"` when the object's `shadow` is not the default `cast`, on each of its nodes. The GLB has no other place for it. `two_sided` is not written, because every Manfred material is already double sided in the GLB; `hidden` objects are not in the GLB at all; `keep_transform` is the node's own matrix.
- **Not written:** anything that is only the builder's: seeds, generator names and params, UV and bevel settings, `parent`.
- **Dialects:** the extras are written whatever the dialect, `none` included.
- **`manfred diff`:** compares extras key by key, with nested keys joined by dots, and marks a difference under the node's name: `extras manfred.travel 0.003 -> 0.004`, or `extras manfred.group only in a`. A node that has extras on one side and none on the other lists every key as only on the side that has it. When one file carries no extras at all, as a Blender export doesn't, extras are left out of the diff.
- **The runtime library:** `manfred_runtime` reads only `extras.manfred`, and a GLB a node of which still carries the older key `extras.manf` is an `Error::Glb` that names the new key; rebuild it with `manfred build`. It takes the movers from nodes whose `manfred` has a `pivot`. When the sidecar has movers too, the two must agree group for group, number for number, and a group that differs or is on one side only is an `Error::Sidecar` naming it. With no sidecar movers the extras are used alone; with no extras the sidecar is. A root without a `group`, or two roots of one group, is an `Error::Glb`. `Scene::groups` keeps each object's `extras.manfred.group` beside `Scene::objects`, and `Scene::group_of(object)` gives that group, or `mover_group(name)` when the GLB has none for the object, so `Scene::mover_of` finds a part the recipe lists in a mover's `parts` without the `mv_` prefix; a GLB without extras finds parts by their names as before.

## Defaults and requirements

Where a key is absent, the recipe crate takes these, and its tests pin them:
- **Outline:** `front` and `back` are required; `holes` is empty. `segments` is 8 on rect and points outlines and 64 on circles and superellipses; a superellipse's `centre` is the origin.
- **Use:** `file` is required; `prefix` is empty, `at` the origin, `rotate` none, `scale` 1, and no `parent`. `profile.kind` is `round`, with 4 segments for round and 1 for chamfer.
- **Pocket:** `rim` and `floor` take `{ width, segments = 4 }`.
- **Bevel:** box and cylinder only; `segments` is 1.
- **Disc:** `segments` is 64.
- **Sweep:** `caps` is true; exactly one of `radius` and `radii`.
- **Card:** exactly one of `image` and `slot`.
- **Scale:** `scale` may be one number, for a uniform scale.
- **Materials:** `material` or `materials`, never both.
- **Material tables:** every `[materials.<name>]`, the recipe's own and its includes', is read as the engine's `Material`, which refuses unknown fields: `manfred check` names the bad field, as `manfred shot` and `manfred render` do for a material a part uses (`materials.<name>.emissive: unknown field \`emissive\`, expected one of …`) at its file and line, even for a material no part uses, so `emission`, not `emissive`, passes.
- **Authored sun:** `toward` and `irradiance` are required, `radius_deg` is 0.27 and `color` white.
- **Daylight sun:** `hour`, `latitude` and `heading` are required; `day` is 172 and `reference_hour` 16.0.
- **Light:** `position` and `intensity` are required; `color` is white, `radius` and `range` 0, and `shadow` false.
- **Text:** a span's `size` is required and above 0, its `color` is white, its `weight` from 1 to 1000; `wrap` is above 0; `anchor` is one of the nine names; `on` names another object, and not with `parent`.
- **Sky:** `[sky.prepare]` takes `cap` and `mean` together or neither, and needs one of its keys. An HDR `path` needs a `source` line, and `kind = "hdr"` and `kind = "mix"` need a `path`. `kind = "room"` and `kind = "mix"` need `[sky.room]`; every room and lamp key has a default.
- **Bake:** `product` is required.
- **Plates:** `size` is required; `overscan` 0.1, `scale` 1, all six `layers`, `threshold` 0.01, `min_samples` 16, `max_samples` 1024, `growth` 2, `casters` `"meshes"`, and `anchors` and `seed` the bake's. A view needs `name`.
- **Proxy:** `"box"`.

A material name is checked by `manfred check`, not by `validate`: an object with no `material` (which draws as `"default"`), or one naming a material that neither the recipe's `[materials]` (with its includes), a generator's returned materials nor the run's registered catalogue defines, is a problem at the object's file and line (`object[<name>].material`), in the words `manfred shot` and `manfred render` refuse it with, naming the material and the names the recipe defines. Plain `manfred check` has no catalogue, so a name the recipe doesn't define is a problem there; an app's build binary, which registers its catalogue, passes the names that catalogue defines. Pockets, text, hidden objects, and generator and mesh objects that name no material are exempt, since they draw no piece of their own or carry their own materials. The engine holds no presets since v0.19.0.

## What is not in the recipe

- The window's orbit camera, selection, panel layout and undo stack: tool state, never saved. Only the recent list is kept, under `$XDG_STATE_HOME/manfred`.
- Live content: a page, a print, a simulation. Apps paint those at runtime, into the slots `card` objects name.
- Behaviour: poses, motion, simulation. Movers describe where a part can go, never where it is now.
- An app's material library: a name resolves through the app's registered catalogue, then the recipe's own `[materials.<name>]` in the engine's material TOML. The app decides whether a recipe's material joins its catalogue.
