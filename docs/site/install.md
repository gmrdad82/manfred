---
title: "Install"
description: "Install Manfred on Linux, Windows or macOS from a release or from source, put manfred on the PATH, then open and render a scene in 30 seconds."
sidebar:
  order: 1
---

Each release carries Linux (x86_64), Windows (x86_64) and macOS (Apple Silicon) builds with one `SHA256SUMS` file. On Linux, Manfred needs a Vulkan GPU and its window needs Wayland.

## From the release binary

Each [release](https://github.com/gmrdad82/manfred/releases/latest) carries `manfred-vX.Y.Z-linux-x86_64.tar.gz`, `manfred-vX.Y.Z-windows-x86_64.zip`, `manfred-vX.Y.Z-macos-arm64.tar.gz` and a `SHA256SUMS` file. The Linux binary needs glibc 2.39 or newer (Ubuntu 24.04, Debian 13, Fedora 40 and later).

Linux and macOS, into `~/.local/bin`:

```sh
v=$(basename "$(curl -fsSLo /dev/null -w '%{url_effective}' https://github.com/gmrdad82/manfred/releases/latest)")
os=$([ "$(uname -s)" = Darwin ] && echo macos-arm64 || echo linux-x86_64)
mkdir -p ~/.local/bin && curl -fsSL "https://github.com/gmrdad82/manfred/releases/download/$v/manfred-$v-$os.tar.gz" | tar -xz -C ~/.local/bin manfred
manfred --version
```

`~/.local/bin` needs to be on your `PATH`.

Windows, in PowerShell, into `%LOCALAPPDATA%\Programs\manfred`, added to your user `PATH` (open a new terminal afterwards):

```powershell
$v = (Invoke-RestMethod https://api.github.com/repos/gmrdad82/manfred/releases/latest).tag_name; $d = "$env:LOCALAPPDATA\Programs\manfred"; Invoke-WebRequest "https://github.com/gmrdad82/manfred/releases/download/$v/manfred-$v-windows-x86_64.zip" -OutFile "$env:TEMP\manfred.zip"; Expand-Archive "$env:TEMP\manfred.zip" $d -Force; [Environment]::SetEnvironmentVariable("Path", [Environment]::GetEnvironmentVariable("Path", "User") + ";$d", "User")
```

Each archive also holds `LICENSE`, `NOTICE`, `README.md` and the bundled licence texts; the Linux and macOS line takes the binary alone.

To check the download, fetch `SHA256SUMS` from the same release and run `sha256sum -c --ignore-missing SHA256SUMS` beside the archive (`shasum -a 256 -c --ignore-missing SHA256SUMS` on macOS, `Get-FileHash` on Windows).

## Windows, from the setup

Run `Manfred-vX.Y.Z-Setup.exe` from the release. It is unsigned, so SmartScreen asks first (More info, then Run anyway). It installs per machine into `C:\Program Files\Manfred`, adds a Start-menu shortcut (and a desktop one, a checked option), registers `.manf`, and appends `C:\Program Files\Manfred` to the machine `Path`, so `manfred` runs in any terminal opened afterwards, Claude Code's included. `/S` installs silently; uninstalling takes the folder back off the `Path`. `manfred-vX.Y.Z-windows-x86_64.zip` holds `manfred.exe` alone, for a manual install.

## macOS, from the app

Unzip `manfred-vX.Y.Z-macos-arm64.app.zip` from the release and move `Manfred.app` to Applications. The release's app is unsigned, so the first open needs System Settings, Privacy & Security, Open Anyway. On its first launch it offers once to install the `manfred` command, the way an editor's "Install command in PATH" does: a `/usr/local/bin/manfred` link into the app, made through the administrator prompt. `/usr/local/bin` is on macOS's default `PATH`, so `manfred` then runs in any new terminal, Claude Code's included. Help, Install the manfred command, in the window does it later (or removes it), and so does `/Applications/Manfred.app/Contents/MacOS/manfred-desk --install-cli` (or `--remove-cli`). `manfred-vX.Y.Z-macos-arm64.tar.gz` holds `manfred` alone.

## `manfred` in a terminal

The Windows setup and the macOS app put `manfred` on the `PATH` themselves. By hand, when that step was skipped or declined:

- Linux: pci's installer links `~/.local/bin/manfred`; otherwise put `manfred` in `~/.local/bin` and keep that folder on your `PATH` (`export PATH="$HOME/.local/bin:$PATH"` in `~/.bashrc` or `~/.zshrc`).
- macOS: `sudo ln -sf /Applications/Manfred.app/Contents/MacOS/manfred /usr/local/bin/manfred`, or `export PATH="/Applications/Manfred.app/Contents/MacOS:$PATH"` in `~/.zshrc`.
- Windows: add `C:\Program Files\Manfred` to `Path` in Environment Variables (Start, "Edit the system environment variables", Environment Variables, `Path`, New), the safe way. `setx PATH "%PATH%;C:\Program Files\Manfred"` works too, but `setx` cuts what it writes at 1,024 characters and writes the machine and user entries together into the user `Path`, so a long `PATH` loses its end; prefer the dialog.

Check each with `manfred --version` in a new terminal.

## Register the .manf file type

The archive also carries a `share/` tree: the `application/x-manfred` type for `*.manf` files (matched by their `MANFRED\0` header too), a `manfred.desktop` launcher that opens `.manf` and `.glb` files with `manfred gui`, and Manfred's mark as the app and file icon (scalable SVG). Unpack the whole archive and run `bin/register-filetypes`, or place the files by hand:

```sh
d=${XDG_DATA_HOME:-~/.local/share}
mkdir -p "$d/mime/packages" "$d/applications" "$d/icons"
cp share/mime/packages/manfred.xml "$d/mime/packages/"
cp share/applications/manfred.desktop "$d/applications/"
cp -r share/icons/hicolor "$d/icons/"
update-mime-database "$d/mime"
update-desktop-database "$d/applications"
```

`bin/register-filetypes` does the same from a checkout or an unpacked archive. Gamer Dad's own copy is placed by pci's installer. On Windows, `Manfred-vX.Y.Z-Setup.exe` registers `.manf` instead, and on macOS `Manfred.app` declares it.

## From source

With Rust 1.97 or newer; on Linux also `pkg-config` and the Wayland client library (`libwayland-dev` on Debian and Ubuntu), on Windows the MSVC build tools, on macOS the Xcode command line tools:

```sh
cargo install --locked --git https://github.com/gmrdad82/manfred --tag vX.Y.Z manfred
```

Replace `vX.Y.Z` with a [release](https://github.com/gmrdad82/manfred/releases) tag.

## Optional tools

Manfred uses three command-line tools when they are on the `PATH`, and works without them:

- [pgpu](https://docs.pitomd.com/pgpu/), a GPU queue: renders and clips wait their turn on it; without it they run at once, paced the same way.
- [pcod](https://github.com/gmrdad82/pito-codecs), a video encoder for `manfred clip --video`: without it `--video` says pcod is needed and keeps the frames.
- [pfx](https://docs.pitomd.com/pfx/), the engine's tool, bakes the light probes from the bake recipe `manfred build` writes.

`manfred gui --clip`, which films the window, needs `pcod`, pito-codecs' CLI, on the `PATH` to encode its master; `--frames` needs nothing.

## Quick start

```sh
manfred gui                                   # the window, on Manfred's own mark; Ctrl+O opens a recipe
git clone --depth 1 https://github.com/gmrdad82/manfred && cd manfred
manfred gui showcase/workshop.toml            # the workshop, live
manfred render showcase/hero.toml --camera hero --draft --out hero.png
```

The last line path-traces the hero scene's `hero` camera at 1586x992 and prints `hero.png` when it is written. [Your first scene](../guides/first-scene/) starts a recipe of your own.
