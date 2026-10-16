# Chase film

A 9:16 chase shot made from Manfred's CLI end to end: a sports coupe racing down a wet city street at golden hour, buildings on both sides, crosswalks, traffic, a chase camera with lag and shake, and road spray. Before the chase, five build beats show how the street was made: each beat is a Claude prompt, the Manfred CLI running, then a jump cut into Manfred's window. The last beat ends on the chase camera's first frame in clay and cuts straight into the rendered chase. Manfred writes the recipe and builds the scene, pfx renders it on its live renderer, and pcod encodes the film.

Run everything from the repo's root. Rendered frames and videos go to your video folder, never into the repo:

```
FILM="${XDG_VIDEOS_DIR:-$HOME/Videos}/chase-film"
```

## 1. Write the recipe

`manfred new street` writes a starter chase recipe: a 9:16 timeline, a route, the street along it, a hero car, traffic and a chase camera. The sample's own recipe is `showcase/chase.toml`; to start a fresh one:

```
manfred new street --seed 1 --out tmp/street.toml
```

Shape the route by hand in the window (`manfred gui showcase/chase.toml`) or with `manfred route showcase/chase.toml list|set|add|remove ...`, and check the timing with `manfred timeline showcase/chase.toml`.

## 2. The five build beats

The recipe's `[[stage]]` tables split the street into the order it is built in: `street`, `car`, `city`, `camera` and `look`. `manfred build --stage <name>` builds every stage up to that one. Each beat runs its CLI step, then cuts to its window clip, a tape of quick shots (0.3 to 0.7 s each, 2 to 4 s a beat, at 60 fps) in `breakdown/`, mostly in clay with the selection outlined in the accent:

1. **Street**: `manfred build showcase/chase.toml --stage street`. The S route over the grid in wireframe, then the road laid down and seen from the top (`breakdown/1-street.tape`).
2. **Car**: `manfred build showcase/chase.toml --stage car`. The coupe with its lights' gizmos and its tail lights selected, then the rear left wheel with the spray emitter's dots (`breakdown/2-car.tape`).
3. **City**: `manfred build showcase/chase.toml --stage city`. The road strip, the buildings arraying along it, the plan with the crosswalk stripes selected, the props, then the power poles and their wires (`breakdown/3-city.tape`).
4. **Look**: `manfred build showcase/chase.toml --stage look`, then `manfred set` tuning of the grade and the lights. A street lamp's spot cone, then a render region on the tail lights through the chase camera, rendered inside and clay outside (`breakdown/4-look.tape`).
5. **Camera**: `manfred shoot showcase/chase.toml --final --out "$FILM/chase"`. The chase camera's frustum and baked path, then through the camera at t = 0 in clay: the match cut into the chase (`breakdown/5-camera.tape`).

One beat's clip, as PNG frames in its own folder under `$FILM/breakdown`, with the viewport over the whole frame and the timeline strip under it:

```
manfred gui showcase/chase.toml --clip "$FILM/breakdown" --tape showcase/chase-film/breakdown/1-street.tape --fps 60 --size 2160x3840 --scale 2 --bare --strip --frames --json
```

All five in order, each its own GPU job:

```
bin/breakdown "$FILM/breakdown"
```

## 3. Render the chase

`manfred shoot` (beat 5) builds the scene, then has pfx render it through the chase camera, frame n at t = n / 60, into numbered PNG frames, the scene's sound as a WAV and a manifest. The same render by hand, from the built scene file in `showcase/out/`:

```
pgpu run --class clip --as manfred -- pfx shot showcase/out/street.scene.toml --seconds 10 --fps 60 --size 2160x3840 --camera chase --final --out "$FILM/chase" --audio "$FILM/chase.wav" --json
```

## 4. Record the Claude Code part

prec records the terminal session that sends each beat's prompt and runs its CLI step, at the film's size and rate:

```
prec clip <app> <shot> --app-dir . --size 2160x3840 --fps 60 --no-archive --out "$FILM/prec"
```

## 5. Assemble and encode the film

`pfx film` lays the beats (prompt, CLI, window clip), the chase and the cards end to end into PNG frames and a WAV, with a manifest naming the pcod command that encodes them:

```
pfx film showcase/chase-film/chase.film.toml --out tmp/chase-film --json
```

Then run the `pcod frames` line the manifest names, with `"$FILM/chase-film.mp4"` as its output.

The film script, its score and the prec tapes are added to this folder later.
