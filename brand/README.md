# Manfred's marks

Everything in `brand/` is the Manfred brand: © Catalin Ilinca, all rights reserved, and not covered by the MIT licence.

Manfred's own marks, made with Manfred: three variants of a dog-like mark of Manfred, the Jack Russell the tool is named after, each modelled in 3D and rendered by `manfred` with an orthographic camera. C, the badge, is the mark; A and B are the drafts it was chosen from. The look is blueprint: amber relief (`ACCENT`, `#F2BF58`) on a blueprint-blue tile (`BG_BASE`, `#02264A`).

## Files

| File | What it is |
|---|---|
| `rig.toml` | The shared rig: the key light, a lamp-room sky with a softbox and a fill, the look, the orthographic cameras `front` (square) and `sheet` (the same view, rendered three across), the materials and the ground |
| `parts/a.toml` | A, the head tilt |
| `parts/b.toml` | B, the cheeky one, in profile |
| `parts/c.toml` | C, the badge, face on |
| `mark-a.toml`, `mark-b.toml`, `mark-c.toml` | One recipe per variant: the rig and a use of its part at the origin |
| `start.toml` | The start scene: the badge standing upright beside its wordmark "Manfred", two `mesh` objects under one parent `manfred`, on the rig's light and ground, with its own perspective camera at a three-quarter view. It carries its rig inline, so the window writes it with its two GLBs and can move the whole mark |
| `start/badge.toml`, `start/badge.glb` | The badge of `parts/c.toml` standing up, facing +z with its tile's front face at z = 0, centred on its bounds; the recipe builds the GLB |
| `start/wordmark.toml`, `start/wordmark.glb` | The wordmark's extruded letters with holes for the counters, its front face at z = 0, centred on its bounds; the recipe builds the GLB |
| `sheet.toml` | The three variants side by side under the same rig, 0.2 apart |

Each recipe is a few lines: `include = ["rig.toml"]` and a `[[use]]` of a part. Edit the rig or a part and every recipe that brings it in follows.

## The start scene

`manfred gui` opened with nothing shows `start.toml`. The badge and the wordmark are the brand, all rights reserved and outside the MIT licence, and they ship in the binary as the app's own mark: the window writes `start.toml` from it into its state folder to open a copy. The scene is Manfred's own 3D model of the badge, like `mark-c.toml`; the flat marks it was drawn from are in `assets/marks/`.

In the start scene the badge and the wordmark are one mesh each, built by `start/badge.toml` and `start/wordmark.toml` and placed by `start.toml` side by side, their front faces on one plane and the parent's origin at the centre of their bounds. `start/badge.toml` places the part with a `[[use]]` turned 90° about x, so the tile stands on its edge facing +z. The wordmark's letters were outlined once from `assets/marks/wordmark.svg` (JetBrains Mono Nerd Font Bold at 1000 px, its quadratic curves flattened to points): one `outline` per letter, 160 deep with a round profile 10 wide, the counters of a, e and d as `holes`, under the hidden anchor `wordmark` scaled by 0.00007, so its cap height is 51 mm and the word is 285 mm long. Rebuild both GLBs after changing either recipe or `parts/c.toml`:

```
manfred build brand/start/badge.toml
manfred build brand/start/wordmark.toml
```

## Shape, units and colours

- The mark lies on the ground facing up, and the cameras look straight down at it, with up along -z, so the picture reads as the flat drawing does.
- Each variant hangs from one hidden anchor scaled by 0.0004, so a part's units are the drawing's pixels and 160 of them are 64 mm. The tile is a superellipse of exponent 5 and half-width 160, sampled at 128 points, 28 deep, with a round profile 10 wide.
- The parts were sampled once from three flat line-art drawings of the mark, centred on their bounds and scaled by 1.08. Each stroke is a `sweep` tube along the drawing's path, its radius half the stroke width, its centre 0.3 radii above the tile, so about two thirds of it stands proud; a path is cut into separate tubes at its sharp corners, and every open end and corner carries a sphere of the tube's radius, the drawing's round caps and joins. Each fill (nose, eyes, speckles) is an outline 8 high (3 for the speckles) with a rounded profile, and a hole in a fill (a pupil's highlight, a nostril, C's pupils) is a pocket with a blue inlay in it.
- His speckled ear is his left, the picture's right. A and B were drawn with it on the picture's left, so their parts are the drawings mirrored; C already had it on the right.
- Colours are linear RGB: amber `#F2BF58` is `[0.8879, 0.521, 0.0976]`, blueprint blue `#02264A` is `[0.0006, 0.0194, 0.0685]`, and the ground is `BG_DEEP`, `#021A39`, `[0.0006, 0.0103, 0.0409]`, matte. Both the tile and the relief are lacquers whose gloss is their own specular.
- The room's ceiling is dark, so the tile's flat top, which mirrors what is straight above the camera, stays a deep blue; the softbox and the fill sit off to the sides at 50° and 38°, where the bevels and the tubes catch them.

## Render

From the repo root, check first, then render; `manfred render` runs itself under the optional GPU queue `pgpu` when that is on the PATH:

```
manfred check brand/mark-a.toml
manfred shot brand/mark-a.toml --camera front --size 768x768 --out tmp/brand/a-shot.png
manfred render brand/mark-a.toml --camera front --size 1024x1024 --spp 64 --out tmp/brand/manfred-a-3d.png
manfred render brand/sheet.toml --camera sheet --size 3072x1024 --spp 64 --out tmp/brand/manfred-sheet-3d.png
```

The drafts are 64 samples. The readability checks render the sheet at 600x200 and 300x100, where each tile is 128 and 64 pixels across. A proper render of the chosen variant is 2048x2048 at 512 samples:

```
manfred render brand/mark-a.toml --camera front --size 2048x2048 --spp 512 --out tmp/brand/manfred-a-2048.png
```

`manfred` writes opaque PNGs, so a mark sits on the rig's ground.
