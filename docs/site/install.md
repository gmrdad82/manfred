---
title: "Install"
description: "Install Manfred on Linux from the release binary or from source, then open and render a scene in 30 seconds."
sidebar:
  order: 1
---

Manfred runs on Linux (x86_64) with a Vulkan GPU, and its window needs Wayland. Windows and macOS are not supported today.

## Linux, from the release binary

Each [release](https://github.com/gmrdad82/manfred/releases/latest) carries `manfred-vX.Y.Z-linux-x86_64.tar.gz` and a `SHA256SUMS` file. The binary needs glibc 2.39 or newer (Ubuntu 24.04, Debian 13, Fedora 40 and later).

```sh
v=$(basename "$(curl -fsSLo /dev/null -w '%{url_effective}' https://github.com/gmrdad82/manfred/releases/latest)")
mkdir -p ~/.local/bin && curl -fsSL "https://github.com/gmrdad82/manfred/releases/download/$v/manfred-$v-linux-x86_64.tar.gz" | tar -xz -C ~/.local/bin manfred
manfred --version
```

`~/.local/bin` needs to be on your `PATH`. The archive also holds `LICENSE`, `NOTICE`, `README.md` and the bundled licence texts; the line above takes the binary alone.

To check the download, fetch `SHA256SUMS` from the same release and run `sha256sum -c --ignore-missing SHA256SUMS` beside the archive.

## From source

With Rust 1.97 or newer, `pkg-config` and the Wayland client library (`libwayland-dev` on Debian and Ubuntu):

```sh
cargo install --locked --git https://github.com/gmrdad82/manfred --tag vX.Y.Z manfred
```

Replace `vX.Y.Z` with a [release](https://github.com/gmrdad82/manfred/releases) tag.

## Optional tools

Manfred uses three command-line tools when they are on the `PATH`, and works without them:

- [pgpu](https://docs.pitomd.com/pgpu/), a GPU queue: renders and clips wait their turn on it; without it they run at once, paced the same way.
- [pconv](https://docs.pitomd.com/pconv/), a video encoder for `manfred clip --video`: without it the frames and their ffconcat list are written, and any encoder makes the video, such as `ffmpeg -f concat -i frames.ffconcat clip.mp4`.
- [pfx](https://docs.pitomd.com/pfx/), the engine's tool, bakes the light probes from the bake recipe `manfred build` writes.

`manfred gui --clip`, which films the window, needs `ffmpeg` on the `PATH`.

## Quick start

```sh
manfred gui                                   # the window, on Manfred's own mark; Ctrl+O opens a recipe
git clone --depth 1 https://github.com/gmrdad82/manfred && cd manfred
manfred gui showcase/workshop.toml            # the workshop, live
manfred render showcase/hero.toml --camera hero --draft --out hero.png
```

The last line path-traces the hero scene's `hero` camera at 1586x992 and prints `hero.png` when it is written. [Your first scene](../guides/first-scene/) starts a recipe of your own.
