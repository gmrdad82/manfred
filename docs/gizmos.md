# Gizmos: select, move, rotate and scale in the viewport

The viewport (C1) is where you pick things up and move them. A click selects, a gizmo drags, and the release writes one gesture into the recipe. This page says how; `docs/editing.md` says how a gesture reaches the file and how undo works.

## Selecting

- A click in C1 that isn't a drag picks the object under the cursor and selects it in the outliner. A click on nothing clears the selection.
- On the live stage the pick is the engine's: `Stage::pick(x, y)` asks for a pixel, and `Stage::picked()` answers a frame or two later with the id the frame wrote there, which is the object's index plus one. The window keeps drawing until the answer lands.
- **What is pickable:** every visible mesh, glass, and text in every space. All carry the same id, the object's index plus one, and 0 is nothing.
  - **Glass:** a part whose material the engine draws in its glass pass, one with `transmission` above 0 or of the `glass` or `liquid` family, is a glass surface, and it writes its id where its front face is nearest. A click through clear glass picks the glass, not what is behind it.
  - **Text:** surface and lit text write their id where a glyph's ink covers the pixel, in front of the object they sit on; overlay text writes it over everything. A click on a label selects the label; a click beside it selects the object under it.
- The fake viewport, which tests use, picks on the CPU: a ray from the eye through the pixel, against every visible triangle of the built recipe, glass included, and against each surface text's laid box, the rectangle around its glyphs lying on its face; nearest wins. Overlay text is hit by its box on screen first, the last one written on top, as the engine's `text_at` does. Shadow-only occluders and hidden objects are never picked.
- The outliner stays the way to select what has no pixels: hidden and shadow-only objects, and anything behind the selected object's gizmo, which takes a click first.

## The gizmo

- **Modes:** `g` move, `r` rotate, `s` scale, as Blender does, with C1 focused. The hint bar lists them, and a chip in the viewport's top right says the mode and the space.
  - Move: three axis arrows and three plane squares. A square moves in the plane of the two axes it spans and takes the colour of the third.
  - Rotate: three rings, one about each axis.
  - Scale: three axis handles and a centre box for a uniform scale.
- **The space:** world axes, or the object's own with `l`. Scale always works on the object's own axes, because the recipe's `scale` is along them.
- **Colours:** x `danger`, y `ok`, z `info`; the handle under the cursor or being dragged is `accent`; a gizmo that can't be used is `text.dim`, with the reason beside it.
- **The pivot:** an object with geometry of its own keeps its origin as the pivot, as Blender does. A group, an object with children and no visible geometry of its own (hidden, as the start scene's `manfred` is), has its gizmo and its pivot at the centre of its subtree's world bounds: the box round every visible part under it, from the built parts. An origin within 0.1 mm of that centre is the centre, so the pivot stays the origin there. A group whose `at` comes from params keeps its origin. The pivot is fixed in the group's own frame for the length of a drag, and found again from the rebuilt parts after it.
- **Drawing:** 2D lines in egui's painter, clipped to the image, projected through the same eye the stage renders with, lens shift and orthographic included. A handle is 80 points long whatever the distance, and is hit within 6 points.

## Dragging

- **Move along an axis:** the pointer is first brought onto the axis's line on screen, then the axis point under it is found in 3D. A drag of N points along an axis parallel to the screen moves the object by N times the metres per point at its depth.
- **Move in a plane:** the pointer's ray meets the plane through the pivot.
- **Rotate:** the angle the pointer sweeps around the gizmo's centre on screen, turned into a rotation about the ring's axis; it is signed by whether that axis faces the eye. The new rotation is written back as the recipe's XYZ Euler angles, each kept within 180° of where it was.
- **Scale:** an axis handle grows by the share of its own length the pointer moves along it; the centre box grows by the pointer's move right and up over the handle length. Scale never goes below 0.001.
- **The preview:** while dragging, `Stage::preview(object, motion)` gives the object's instances, and its children's, the motion matrix from its recipe pose to the dragged one. No mesh is rebuilt or uploaded. `Stage::restore()` puts every instance back, and a rebuild resets them anyway.
- **Snapping:** hold Ctrl. Moves go in steps of 1 mm, or of one plate pixel in x and y when the object is placed with `at_px`; rotations in steps of 5°; scales to multiples of 0.05. A snapped move keeps the object's own offset: an object at 37.5 mm moves to 38.5 mm, not to 38 mm.
- **Esc** cancels: the preview goes, and the rest of the drag does nothing.

## What a release writes

One gesture, labelled like `move ball along x`, through `shell.edits().push(label, patches)`. The window never writes the file itself; the patch code does.

- Only the keys that changed: `at` for a move, `rotate` for a rotation, `scale` for a scale.
- **About a group's centre:** a move writes the group's `at`, shifted by the drag. A rotation or a scale turns about the pivot, so it writes `rotate` or `scale` and the `at` that keeps the pivot where it was: `at` becomes `at + R·S·pivot − R′·S′·pivot`, with R and S the rotation and scale before the drag and R′ and S′ after it. Both keys go in one gesture, `at` first, labelled as the rotation is (`rotate pair about y`), so D1 shows `rotate pair about y · object[pair].at +1`. One undo puts both back, and the file is byte for byte as it was; redo writes both again. A group whose origin is its centre writes `rotate` or `scale` alone.
- In the key the recipe uses:
  - `at` in metres, rounded to a micrometre;
  - `at_px` in plate pixels under a `[plate]`, rounded to a thousandth of a pixel, with the depth in metres. An `at_px` written with two numbers keeps two while the depth stays 0;
  - an object with neither key gets `at`;
  - `rotate` in degrees, rounded to 0.0001°;
  - `scale` as one number if the recipe wrote one and the result is still uniform, else three.
- In the parent's frame: a child's new `at` is its dragged world position taken back through its parent's transform, and its rotation through its parent's rotation, so the build composes it to the place it was dropped.
- After the release the preview holds until the rebuilt recipe arrives, so the object never jumps back while the file is written and rebuilt. If the patch code refuses the gesture, it calls `shell.drop_preview()`.

## When the gizmo is greyed out

- A key written as `"${params.…}"`: the params file stays its truth, so that mode is greyed with the params path. The other modes still work.
- A pocket: its own `at`, `rotate` and `scale` are ignored by the builder.
- A generator's object can be placed like any other: its transform applies to all its pieces, and a pick on any piece selects the object.
