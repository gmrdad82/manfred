# Cameras and renders from the window

The cameras panel (B2) and the render panel (C3) of `manfred gui`. Both write nothing themselves: B2 pushes gestures onto `Shell::edits()`, which the patch task drains and writes to the recipe, and C3 starts a subprocess.

## The view: free, or through a camera

The 3D view fills C1, as Blender's does, with no bands above or below.

- **The free view** is the default: orbit, pan and dolly move it, and it is drawn at C1's own size and aspect. Its field of view covers a frame of the recipe's aspect (its `[plate]`'s, else 16:10) fitted inside C1, so a recipe opened from its camera shows at least what that camera frames, and C1's longer side shows more. A recipe opens in the free view from its first camera's place, and C1's header says `orbit from <camera>`.
- **Through a camera:** `0`, or choosing a camera in B2, looks through it, as Blender's camera view does. The render's frame, at the aspect of C3's size, sits in C1 with a 24-point margin at least, the camera's field of view spans it exactly, and the view outside it is dimmed in `bg.deep` (a passepartout) with a thin border round the frame. The header says the camera's name. Orbiting, panning or dollying leaves it for the free view, and so does opening another recipe. A camera set from the view or made new looks through once its patch lands.
- **Everything follows the view:** picking, the gizmo and its handles, the floor grid, the axes and the overlays are drawn through the same eye the stage renders C1 with.

## B2: set a camera from the view

Orbit, pan or dolly the viewport, then save what you see.

| Key (B2 focused) | Button | What it does |
| --- | --- | --- |
| `↑` `↓` | a row | choose a camera and look through it |
| `s` | Set from view | write the orbit eye into the chosen camera |
| `n` | New | append a camera named from the eye |
| `del` | Delete | ask, then remove the chosen camera |
| `0` | | look through the chosen camera again (any panel) |

- **Set from view** pushes one gesture, `set camera <name> from view`, with a patch per key of `[[camera]]`:
  - `position`, `target` and `up` always. The `up` is level: the world's up made perpendicular to the view, or the orbit's own up when looking straight down;
  - `fov_y_deg` for a perspective camera, `ortho_height` for an orthographic one;
  - a perspective camera that sets `focal_mm` keeps its lens: orbiting never changes the field of view, and a camera with both `fov_y_deg` and `focal_mm` is a problem;
  - `kind`, `shift`, `sensor_mm`, `focus_m`, `fstop` and every other key stay as they are, with their comments and formatting.
- **New** pushes `new camera view<N>`, one patch appending a `[[camera]]` table named `view1`, `view2`, … (the lowest free number). It is perspective with `fov_y_deg`, or orthographic with `ortho_height` when the view is. A non-zero `shift` is copied. A recipe with no camera yet gets its `[[camera]]` array created by the same patch.
- **Delete** asks first: B2 shows `delete <name>?` with `Delete ⏎` and `Keep esc`. `⏎` or the button pushes `delete camera <name>`; `esc`, the other button, or any other key cancels. The gesture is refused by the recipe's own validation if something still names the camera, and the patch task reports it.
- Numbers are written to 6 decimals, so the recipe holds `0.3`, not `0.30000000000000004`.
- After the patch lands, the window selects the camera and snaps the eye to what the file now holds, so the viewport says `front`, not `orbit from front`. If the patch never lands, or the view moves in between, the window does nothing.
- Each gesture is one undo step: the inverse patches restore the file byte for byte.

## C3: render a still

C3 holds the camera, the size, the samples and a Render button, which runs `manfred render`, the path tracer. `docs/render.md` has the details.

Render (or `⏎` in C3) runs, on a thread, with the window's own executable:

```
<manfred> render <recipe> --camera <name> --size WxH --spp N --out tmp/manfred/renders/<recipe>-<camera>-<W>x<H>.png
```

- `<recipe>` is the recipe file's name without its extension; characters other than letters, digits, `-`, `_` and `.` in a name become `_`. The folder is made if it is missing, relative to where the window was started.
- `manfred render` decides for itself whether to run under the optional GPU queue, `pgpu` (`docs/render.md`, the process line), so the window wraps nothing; it never waits on the subprocess and draws at once.
- One render at a time. While one is live the button reads Cancel, and `esc` in C3 does the same: it sends TERM to the child's own pid, never SIGKILL.

### States

The thread polls the process every 100 ms and, when `pgpu` is on the PATH, `pgpu status --json` every 400 ms (1.5 s timeout; a failure leaves the state as it was). The job is found by the child's pid, or by its `--out` path in the job's label. Without pgpu, or before pgpu lists the job, a render turns from queued to running as soon as it reports its first progress.

| State | When |
| --- | --- |
| queued | started, or pgpu lists it waiting with no reason (behind another job) |
| waiting | pgpu gives a reason: `desk`, `paused`, `vram`, or `yielded` when it gave its turn away |
| running | pgpu says `running` or `lent`, or the render reports its progress |
| done | the process exited 0 and the PNG exists |
| failed | any other exit: the reason is `exit <code>` and the last line of its stderr, or `stopped by a signal`; also a clean exit with no PNG, and a command that could not start |
| cancelled | the process ended after Cancel |

A render that finishes while Cancel is being sent is `done`.

The state shows as a pill with the camera and size in C3, and as `render <camera> <W>×<H> · <state>` in D1 until the next render starts.

### The result

When a render is done, C3 shows its thumbnail (click it to enlarge) and its path. `v` in C3 (or a click on the thumbnail) shows the PNG full-size over the viewport, fitted to the viewport, with a band naming it; any key closes it and does nothing else. The PNG is decoded on the render's thread, and boxed down to 2048 on its longer side for the full view and 640 for the thumbnail, so a large render never stalls a frame.

## Wiring

`Shell::set_waker(f)` takes a function that the render thread calls whenever the job changes. The window passes one that sends its event loop a redraw (`Wake::Staged`), so C3 updates as the state changes; without one it updates on the next redraw, which the pgpu poll, when pgpu is there, causes within a second when its state changes.

## Tests

- CPU: the patches for set from view, new camera and delete (applied to a recipe and compared with the file, and undone); the command line; the state machine through queued, waiting, running, done, failed and cancelled with fake pgpu status texts and a fake process; the thread through to a picture, and to a cancel; the PNG decode and shrink.
- `--shot`: B2 after set from view, and C3 in each state, written under `tmp/ui-tests/` by `shell`'s tests.
