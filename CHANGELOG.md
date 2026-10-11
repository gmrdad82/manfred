# Changelog

Every release of Manfred, newest first. Earlier versions were developed privately; 0.19.0 is the first public one.

## v0.20.0 — 2026-10-11

### Added

- Manfred runs on Windows x86_64 and macOS on Apple Silicon as well as Linux, and CI tests all three.
- Each release carries archives for Linux x86_64, Windows x86_64 and macOS arm64 with a SHA256SUMS file, and the README installs them.

### Changed

- A recipe builds to the same bytes on every system: the geometry math takes its sines, arcs, logs and powers from the libm crate.
- On Windows the recent list and the start scene live under `%LOCALAPPDATA%\manfred`, and tools on the PATH are found by their `.exe`.

### Fixed

- A recipe path that starts at the root is refused as absolute on Windows too.

## v0.19.0 — 2026-10-08

### Added

- The code is open source under MIT, with NOTICE, TRADEMARKS.md, a code of conduct and a security policy.
- `manfred gui --clip` films the offscreen window playing a tape into an x264 master, a poster and a manifest.
- `bin/gate --public` and CI run the gate that needs no GPU, a licence check included.

### Changed

- Command-line errors follow clap's form: a short error on stderr and exit 2, and a bare `manfred` prints that error.
- Every dependency comes from a public source over https, the pfx crates from pfx `v0.25.0`.
- Scenes are written and opened through pfx-scene, and crash reports and job progress go through pfx-report.
- The root package's `jobs` and `report` features are gone.

### Fixed

- A recipe opened from a folder's picker saves its edits.
