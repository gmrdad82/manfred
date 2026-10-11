# Changelog

Every release of Manfred, newest first. Earlier versions were developed privately; 0.19.0 is the first public one.

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
