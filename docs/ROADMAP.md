# Atlas Roadmap

## v1.0 Definition of Done
- [x] Rust, Python and TypeScript indexing with measured accuracy numbers published in the README
- [x] Desktop app with interactive graph, search, impact analysis, English and Arabic UI
- [x] Incremental re-indexing, migrations, and no panics on hostile input
- [x] Install docs for the prebuilt artifacts, checksum verification and the desktop app's runtime libraries
- [x] CI green on Linux and the release artifacts downloadable: the CI run on
  `main` is green (ubuntu/macos/windows checks + `cargo deny`), and the
  `v1.0.0` release artifacts are live and verified — every checksum validates,
  the downloaded CLI runs, and the desktop tarball unpacks. Two Windows-only
  fixes landed after the first red run: a multi-resolution `icon.ico`
  required by `tauri-build`, and a separator-agnostic `db_path` assertion in
  a desktop-session test
- [x] Honest limitations documented, with the "type-accurate resolution" question settled by decision record

## Milestones
| Milestone | Title | Target Tag | Status |
|-----------|-------|------------|--------|
| R0 | Re-baseline and scope update | - | Done |
| 5 | Release hygiene | v0.1.0 | Done |
| 6 | Language abstraction + Python + TypeScript | v0.2.0 | Done |
| 7 | Import-aware resolution and an accuracy report | v0.3.0 | Done |
| 8 | Docs, signatures and text search | v0.4.0 | Done |
| 9 | Desktop app | v0.5.0 | Done |
| 10 | Semantic search | v0.9.0-beta | Done |
| 11 | Packaging and the v1.0 release | v1.0.0 | Done |
