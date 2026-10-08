# Showcase

Manfred's own scenes, built only from generic kinds and their own materials, to show what Manfred and the engine can do. Open one in the window, play with it, then render it big. An app's scenes live in the app's own repo, opened with its own build binary, which registers its generators (`docs/apps.md`).

- `workshop.toml`: a workbench before a plaster wall, under a lamp room whose window throws striped light: a teal case with two pockets, a brass lip and a dial and lid that move, a glazed vase, a glass jar, a cable, a spool, a mirror ball, a cupped note and the word `manfred` on the case. Cameras `hero`, `top`, `detail` and `orbit`, which carries an orbit path for `manfred clip`.
- `hero.toml`: the README's picture: a still life on a blueprint-blue cove under a low warm sun and a cool fill, with a flag above the set, out of view, shading the upper wall: a lacquer case with two pockets, a brass lip, an amber knob and a glowing `manfred = 1`, a swept cable, a gold ring, a chrome ball, an amber capsule, a lathed porcelain vase on an oak plinth, a lathed wine glass and a cupped note. Camera `hero`. Its `[trace]` keeps the chrome a mirror and the glass crisp with no fireflies under the small sun: `filter_glossy = 0.25`, with `clamp_indirect = 4.0` as the backstop.
- `gallery.toml`: the 2.5D case: three cards with slot placeholders in a wooden frame behind a glass pane, a caption, and an overlay title. Cameras `flat` (orthographic) and `angle`.

Run everything from the repo's root.

## Open one in the window

```
manfred gui showcase/workshop.toml
manfred gui showcase/gallery.toml
```

## What to try

- **Orbit:** drag in the viewport to turn the view, hold Shift or drag with the middle button to pan, scroll to dolly. In the cameras panel, `↑` and `↓` jump between the cameras, and `0` goes back to the chosen one.
- **The hour scrubber:** the slider in the sky panel. The sun and the lamp room's ambient follow the hour.
- **Picking:** click a part, the glass jar or the word on the case. The outliner selects it and the properties panel shows where it sits.
- **Moving a part:** with a part picked, `g` moves it, `r` rotates, `s` scales (hold Ctrl to snap). The new place is written straight into the recipe file, and Ctrl+Z puts it back.
- **Setting a camera:** orbit to a view you like and press `s` in the cameras panel; the camera is rewritten in the recipe.

## A hi-res still

The live look at 1920x1080, a few seconds:

```
manfred shot showcase/workshop.toml --camera hero --size 1920x1080 --out tmp/workshop-hero.png
```

The traced still; the GPU time grows with the size and the samples, so it runs under the optional GPU queue `pgpu` when that is on the PATH, and directly otherwise:

```
manfred render showcase/workshop.toml --camera hero --size 3840x2160 --spp 1024 --out tmp/workshop-hero-4k.png
manfred render showcase/gallery.toml --camera flat --size 2400x1600 --spp 256 --out tmp/gallery-flat.png
```

## A clip

```
manfred clip showcase/workshop.toml --camera orbit --frames 240 --fps 30 --size 1920x1080 --out tmp/workshop-orbit --video tmp/workshop-orbit.mp4
```

`--trace --spp 128` gives the traced look.
