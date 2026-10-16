---
title: "Shoot and film the window headless"
description: "Draw one frame of Manfred's window offscreen, drive it with a script of clicks and keys, and film it playing a tape, byte for byte the same on every run."
sidebar:
  order: 6
---

The window can be drawn offscreen, with no compositor and nothing on your desktop: one frame with `--shot`, or a whole clip with `--clip`. A program or an AI agent sees what the window shows this way, and every window clip in these pages was made by it.

![Manfred's window on its start scene: the amber badge of a Jack Russell's head and the word Manfred beside it, selected with a move gizmo over a floor grid.](../images/start.png)

## One frame

```sh
manfred gui showcase/workshop.toml --shot workshop.png
manfred gui --start --shot start.png --size 1280x800
```

`--size` sets the window (1586x992 by default). The frame shows the settled window: the scene built, the stage drawn, every panel laid out. `--bare` draws the viewport alone instead, filling the whole frame (see below).

## Play a script first

`--input <script>` plays a script of one step a line before the frame. Coordinates are points from the window's top left, and a line starting with `#` is a comment.

```
# pick the vase and look at its properties
click 995 480
frame
key g
```

| Step | Does |
| --- | --- |
| `key <name>` | one key by egui's name (`g`, `Escape`, `Enter`, `ArrowDown`); `ctrl+`, `shift+`, `alt+` hold a modifier: `key ctrl+z` |
| `text <words>` | types the rest of the line |
| `move x y` | moves the pointer |
| `click x y` | presses and releases |
| `press x y` / `release x y` | holds the button down until the release, for a drag a frame can stop inside |
| `drag x y x y [middle]` | a whole drag in one line |
| `scroll x y lines` | the wheel, positive to come nearer |
| `frame` | draws one pass |

```sh
manfred gui showcase/workshop.toml --input pick.txt --shot picked.png
```

## Film a tape

A tape is a script with time: `wait <seconds>` holds still, the pointer glides to each point, a drag takes its time, text types at 15 characters a second.

```
wait 1
click 673 547
wait 0.6
press 720 556
move 690 551
move 660 546
release 630 541
wait 0.8
key ctrl+z
wait 1.5
```

```sh
manfred gui showcase/workshop.toml --clip clips --tape move.tape --poster last
```

It writes three files into `clips/`, each named `<tape>-<W>x<H>-<fps>fps`: an H.264 `.master.mp4`, a `.poster.png` and a `.json` manifest ([file formats](../../reference/formats/)). `--fps` is 30, 60 or 120. The frames are drawn as PNGs into a staging folder, `clips/<stem>.partial/`, encoded by pito-codecs' `pcod frames` on the CPU at CRF 14, and removed; the manifest names pcod and its version. It needs `pcod` on the `PATH` and an even `--size`, and refuses in one line without them.

- **It plays on a copy:** the recipe's folder is copied into the clip folder and played there, so the tape's edits never reach your files, and the copy is removed at the end.
- **It is deterministic:** between frames it waits for every build, pick and render the window started, so the same tape gives the same frames byte for byte.

### Frames, scale and a framed window

- `--frames` keeps the PNG frames instead of encoding a master: `clips/<tape>-<W>x<H>-<fps>fps/frame-00000.png` on, and the `.json` manifest beside the folder, with each frame's tape step and line. It needs no `pcod`, takes no `--poster`, and the same tape gives the same PNG bytes.
- `--bare` draws the viewport alone: no panel, header, hint bar or timeline bar, so the viewport fills the whole frame at any size, a portrait `--size 1080x1920` or `--size 2160x3840 --scale 2` included. The pointer still draws, the viewport's own marks (the grid, the axes, a camera's frame) stay, and the tape's coordinates are the viewport's points. The keys still work: `key Space` plays a timeline, `key f` looks through its chase camera. It needs a recipe, a scene or `--start`, and doesn't go with `--framed`. `--shot` takes it too.
- `--scale <factor>` lays the window out at that many pixels a point, from 1 to 4: `--size 2160x3840 --scale 2` reads like the window at 1080x1920, text, panels and the pointer twice as large. The tape's coordinates stay in points.
- `--framed` lays the window out at `--window` (1586x992 points by default) and centres it in the `--size` frame on the editor's ground, so a 9:16 film shows the real landscape window. The window is drawn at `--scale`, or at the largest scale that fits the frame when that one doesn't: with `--size 2160x3840 --scale 2` it is drawn 2160x1351, at 1.36 pixels a point, sharp, never scaled down after drawing.
- `--json` writes one JSON line a frame on stderr, `{"stage":"frame","frame":n,"done":n+1,"total":N}`, and one line on stdout at the end, `{"stage":"done","kind":…,"frames":N,"seconds":…,"manifest":…,"files":[…]}`, with the folder (or the master and poster) and the manifest. A refusal stays one line on stderr, `manfred gui --clip: …`, exit 1.

```sh
manfred gui showcase/workshop.toml --clip film --tape tweak.tape --frames --size 2160x3840 --scale 2 --framed --fps 60 --json
manfred gui out/street.scene.toml --clip film --tape chase.tape --bare --size 2160x3840 --scale 2 --fps 60
```

To make a GIF of it, [pconv](https://docs.pitomd.com/pconv/) converts the master:

```sh
pconv clips/move-1586x992-30fps.master.mp4 move.gif --size 1280 --fps 15
```
