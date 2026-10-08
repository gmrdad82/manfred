# The rig: running `manfred gui` hidden, for tests

`bin/rig` runs the window in a hidden compositor on wlroots' headless backend, takes screenshots of it, types into it and closes it, so a test or a script can drive the live window without it ever appearing on your own desktop. For a frame with no compositor at all, `manfred gui --shot` is quicker; the rig is for what only a live window shows: keys, focus, the pointer, the real swapchain.

It needs `cage` or `labwc`, `grim`, `wtype`, `unshare` with user namespaces, and `wlrctl` for the pointer verbs.

## What `up` sets up

- A rig folder `tmp/rig-<name>` in the repo, never a path outside `tmp/`. The name is letters, digits, dot, dash and underscore.
- A private `/tmp`: the compositor and the app run under `unshare -Urm` with `tmp/rig-<name>/tmp` bind-mounted over `/tmp`, so wlroots' Xwayland can never touch the host's `/tmp/.X11-unix` and the host `/tmp` holds nothing of the rig. `XDG_RUNTIME_DIR` is left alone: a socket path is capped at 108 bytes.
- Isolation: `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME` and `XDG_DATA_HOME` under the rig, and the mute on three legs: `PIPEWIRE_RUNTIME_DIR=/nonexistent`, `PULSE_SERVER=/nonexistent` and `ALSA_CONFIG_PATH` at a null config in the rig.
- At most four compositors at once on the machine (`RIG_SLOTS`); `up` polls every 5 s up to `RIG_SLOT_WAIT` seconds (600 by default), then refuses.
- It records the compositor's, the app's and the launcher's pids, and the app's `WAYLAND_DISPLAY` in `$RIG/wayland`, refusing one equal to the shell's own. Every process is found through the `RIG=<folder>` in its environment, never by name alone.

## The verbs

- `bin/rig up <recipe> <name>`: runs `target/release/manfred gui <recipe>` under `cage`, or under `labwc` with `RIG_COMPOSITOR=labwc` (`RIG_MODE=1520x960@60Hz`, `RIG_SCALE=1|2`). It refuses:
  - a recipe path that is not a file;
  - a name that would put the folder outside `tmp/`;
  - a missing release binary, or one older than any file under `src/` or `crates/`: build it first;
  - a rig that is already up under that name.
- `bin/rig shot <name> <file>`: `grim` from the rig's socket to `tmp/rig-<name>/tmp/<file>.png`; prints the path and the size.
- `bin/rig keys <name> <wtype args…>`: one `wtype` call, after `-k F24 -s 800` as the sacrificial first key. `bin/rig keys w2 -k b -k 1` presses B then 1.
  - A chord goes in one call. Every call opens with `F24`, and the window reads any other key as the end of a pending chord, so `keys w2 -k b` and then `keys w2 -k 1` never reaches B1.
  - Give the window a second after the call before the next `shot`; it redraws on input and idles at zero frames.
- `bin/rig pointer <name> <wlrctl pointer args…>`: labwc only; cage has no virtual pointer, and a bare `wlrctl pointer` outside a rig can segfault. `wlrctl` moves by a difference, clicks and scrolls, but cannot hold a button.
- `bin/rig press <name> <x> <y> [left|middle|right]`: labwc only. Moves the rig's pointer to the point, in points from the output's top left, and presses the button, which stays down. The point must sit inside the output, whose size `up` records in `tmp/rig-<name>/extent` (`RIG_MODE` over `RIG_SCALE`).
  - The hold lives in a small helper that talks labwc's `zwlr_virtual_pointer_v1` itself, because `wlrctl` has no verb for holding. The helper runs under `RIG=<folder>` like every other rig process, writes its pid to `pointer.pid`, takes its commands through `pointer.fifo`, and lets go by itself after five minutes.
  - While a button is held, `bin/rig drag-to <name> <x> <y>` moves the pointer to a point and `bin/rig pointer <name> move <dx> <dy>` moves it by a difference.
  - `press` refuses a second hold in the same rig.
- `bin/rig release <name> [<x> <y>]`: lets go of the held button, after moving to the point if there is one. `down` lets go and stops the helper if the caller forgot.
- `bin/rig down <name>`: the patient teardown. It kills the app, polls up to ten seconds until it is gone, only then `kill -9`s what is left, then stops the compositor and the launcher. A `kill -9` on a live app makes cage abort with a crash card on the host desktop.
- `bin/rig ls`: the live rigs, with their names, compositor, pids and recipe.

A gizmo drag in the live window is `press` on the handle, a few `drag-to` steps with a `shot` after the last, and `release` at the end point. Run it on a copy of a recipe, since the release writes the gesture into the file.

All verbs but `up` refuse unless `WAYLAND_DISPLAY` is the shell's own, as recorded at `up`, so a call from inside a rig never reaches your desktop by mistake.

## An example run

Build the release binary first, then run the rig in one step so `down` always runs:

```
cargo build --release

bin/rig up crates/recipe/tests/fixtures/flat.toml w2 && bin/rig shot w2 first && bin/rig keys w2 -k b -k 1 && sleep 1 && bin/rig shot w2 second; status=$?; bin/rig down w2; exit $status
```

The window draws on the GPU directly, not as a queued job; a rig run is short.

A gizmo drag, on a copy of a small table recipe (`RIG_COMPOSITOR=labwc`, a 15 s wait for the first stage): `press` on the outliner's `ball` row and `release` select it, `press 850 472` on the x handle, four `drag-to` steps to `910 472`, a `shot`, then `release 910 472`. The recipe gained one line, `at = [0.074726, 0.042, -0.03]`, and D1 read `move ball along x · object[ball].at`.

Look at `tmp/rig-w2/tmp/first.png` and `second.png`. The `down` runs whatever the earlier verbs did, so the compositor never outlives the step.

## When a rig fails

A report about a rig says which process died, if one did (the app, the compositor, a tool), whether the app came up (pid and socket), and whether the fault is Manfred's or the rig's, before any retry.

- `up` says `cage did not come up` or `labwc did not come up`: a rig fault before Manfred booted. Read `tmp/rig-<name>/rig.log`; it holds the compositor's output.
- `up` says `manfred did not come up`: a Manfred or launch fault. The compositor is up, Manfred is not, or has no socket yet. `rig.log` holds Manfred's own output; the window prints its errors to stderr.
- `shot`, `keys` or `pointer` says `manfred pid … is not running`: Manfred died after `up`. Read `rig.log` for a panic or a wgpu error.
- `rig: refusing caller Wayland display`: the call came from a shell whose `WAYLAND_DISPLAY` differs from the one `up` recorded. Run it from the same shell.
- `build first` or `is older than`: the release binary is missing or stale; run the build step.
- `teardown left … running`: do not `kill -9` it by hand. Wait, run `bin/rig down <name>` again, and only if the pid stays, look at what holds it.
- `grim` fails or writes nothing: the compositor has no output yet or the app has not drawn; take the shot again after a second.
- Captures and logs live under `tmp/rig-<name>/` and `tmp/rig-<name>/tmp/`, named. `tmp/` is ignored by git.
