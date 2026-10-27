# 002 — Desktop app stack

- **Status:** Accepted
- **Date:** 2026-10-24
- **Milestone:** 9 (v0.5.0)
- **Deciders:** project owner

## Context

Milestone 9 adds a desktop app with an interactive call graph, search, impact
analysis, and an English/Arabic UI with RTL support. Atlas is local-first: the
app must make no network calls at runtime, must not execute repository code,
and must work from the same SQLite index the CLI uses.

The repository has no Node.js/npm toolchain available, and adding a JavaScript
build step would add a second dependency ecosystem to a Rust project. The app
still needs a real desktop window, a webview, and a small IPC surface.

## Options considered

### A. Electron + a JavaScript frontend

- **Benefits:** Mature ecosystem, familiar UI tooling.
- **Costs:** Bundles Chromium and Node.js; large binary; separate runtime
  security surface; requires npm and a frontend build pipeline; awkward to
  call the Rust crates without a sidecar or native addon.
- **Fit:** Poor.

### B. Tauri v2 + a JavaScript framework and npm build

- **Benefits:** Small native shell, good Rust integration, mature webview.
- **Costs:** Requires Node/npm, a framework toolchain, and a generated asset
  pipeline. That is extra infrastructure for a UI that only needs a handful of
  views.
- **Fit:** Fair, but heavier than necessary.

### C. Tauri v2 + static vanilla HTML/CSS/JavaScript

- **Benefits:** Rust backend calls the existing `atlas-index`/`atlas-scan`
  crates directly; no Node/npm; no runtime network; assets are embedded at
  compile time; `withGlobalTauri` exposes the IPC and dialog APIs; a strict
  CSP can block network access; the UI remains small and auditable.
- **Costs:** The frontend is hand-written; no framework or component tooling;
  tests are mostly backend tests plus a manual GUI checklist; Linux builds
  need the WebKitGTK/GTK system libraries.
- **Fit:** Good.

### D. Native Rust UI (egui/iced)

- **Benefits:** Single language, no webview.
- **Costs:** A second UI stack with different layout, accessibility, text
  shaping and RTL support; the graph view would be custom drawing; slower to
  deliver a polished bilingual UI.
- **Fit:** Fair, but a poor use of the existing web assets and Tauri's
  accessibility support.

## Decision

Use **Tauri v2** with a **static vanilla HTML/CSS/JavaScript frontend**:

- The app lives in `crates/atlas-tauri` and is a workspace member.
- `tauri.conf.json` points `frontendDist` at the checked-in `ui/` directory.
- `withGlobalTauri` is enabled, so the UI uses `window.__TAURI__.core.invoke`
  and `window.__TAURI__.dialog` without an npm package or bundler.
- The native folder picker comes from `tauri-plugin-dialog`; no other plugins.
- The CSP allows only local assets and Tauri IPC. No remote fonts, scripts,
  styles, images, or `fetch` calls are used.
- The Rust backend opens `<repository>/.atlas/atlas.db` and calls the shared
  crates. It never runs repository code.
- English and Arabic strings live in `ui/i18n.js`; the UI switches
  `lang`/`dir` and persists the choice in `localStorage`.
- Packaging/installers are deferred to Milestone 11. Milestone 9 verifies a
  development build and a launched window, not signed installers.

## Consequences

- `cargo build -p atlas-tauri` requires the Tauri Linux prerequisites
  (`webkit2gtk-4.1`, `gtk+-3.0`, `javascriptcoregtk-4.1`, `libsoup-3.0`,
  `librsvg-2.0`). CI must install them on Ubuntu before building the app.
- The app has no Node/npm build step. Changing UI files requires rebuilding the
  Rust binary because assets are embedded by `tauri-build`.
- Backend behavior is tested with ordinary Rust tests against `AtlasState`.
  The webview UI is verified with a build plus a launch smoke test and a
  manual checklist; it is not covered by a browser test runner.
- A future framework migration is possible without changing the Rust command
  layer, but is not planned.
- Tauri v3 is alpha at the time of this decision; v2 stable is used.
