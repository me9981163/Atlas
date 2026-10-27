# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-10-26
### Added
- CI on every push to `master`/`main` and every pull request: fmt, clippy and
  the full test suite on Linux/macOS/Windows (`fail-fast` off), plus an
  enforcing cargo-deny gate (licenses, RustSec advisories, bans, sources) via
  `cargo-deny-action@v2`
- Release pipeline triggered by `v*` tags: CLI archives for Linux/macOS/Windows
  with standard-format SHA-256 checksum files, a post-build smoke test of each
  binary, and a Linux-only desktop tarball containing `atlas-tauri` plus a
  `DESKTOP-README.txt` with its runtime-library requirements
- README install documentation for the prebuilt artifacts, checksum
  verification and the desktop app's runtime libraries
- `deny.toml` rewritten for the cargo-deny 0.20 schema: permissive-license
  allow list (including MPL-2.0, CDLA-Permissive-2.0 and Zlib for tauri's
  transitive trees), three documented unmaintained-advisory ignores, and
  enforcing source checks; `cargo deny check` passes locally on the full
  lockfile

### Fixed
- `atlas report | head` (any piped truncation) no longer panics with an EPIPE
  message: the CLI restores the default SIGPIPE disposition on Unix, so it dies
  silently like other Unix filters (`atlas report | head -c 20` now exits 141
  with no stderr output)
- The old audit job swallowed failures (`|| echo`); the cargo-deny gate is now
  enforcing, and the redundant non-enforcing `cargo audit` step is gone

### Changed
- The desktop footer reports `v1.0.0`
- README semantic-search screenshots recaptured from the v1.0.0 release build
- CI previously watched only `main`; the repository's branch is `master`, so it
  would never have run. Now watches both.

## [0.9.0-beta] - 2026-10-25
### Added
- `atlas-embed` crate: a pinned `minishlab/potion-code-16M-v2` Model2Vec
  model (MIT, 256 dimensions, ~32 MB), an explicit downloader with SHA-256
  verification, a local-only loader, and a cached embedder
- `atlas model download [--yes]` and `atlas model status`; the download prints
  the model, revision, source, size and cache path, verifies hashes, and is the
  only network operation Atlas performs
- Schema v4 with a `symbol_embeddings` table keyed by `(symbol_id, model)`,
  automatic v1/v2/v3 → v4 migrations, and `ON DELETE CASCADE`
- `atlas embed` with incremental content hashing, batched encoding, normalized
  `f32` vectors stored as SQLite BLOBs, and stale-symbol reporting
- `atlas search --semantic` with brute-force cosine ranking, deterministic
  ordering, and an optional `--model` selector
- Desktop semantic search mode: model status, in-app download confirmation,
  download and embedding progress, build/refresh controls, and similarity
  percentages in results
- English and Arabic strings for the semantic panel, download modal and
  progress messages, with RTL-safe layout
- Decision record `docs/decisions/003-semantic-search.md`
- Semantic search screenshots in English and Arabic

### Changed
- Lexical FTS5 search remains the default; semantic search is opt-in in both
  the CLI and the desktop app
- The desktop app reads and writes the same schema v4 database as the CLI;
  opening an older database migrates it in place and leaves embeddings empty
- The desktop footer reports `v0.9.0-beta`

### Fixed
- Semantic status, model sizes and progress values keep left-to-right number
  and unit order inside Arabic RTL text
- The semantic build button is disabled when every embedding is current
- The model download confirmation uses an in-app modal instead of a native GTK
  dialog, so Arabic text shaping, RTL layout and button localization are
  consistent

## [0.5.0] - 2026-10-24
### Added
- Tauri v2 desktop app (`crates/atlas-tauri`) with an interactive SVG call
  graph, FTS5 search and browse, symbol details, callers/callees/impact tabs,
  and an English/Arabic UI with RTL support
- Native folder picker through `tauri-plugin-dialog`
- ID-scoped index APIs: `symbol_details`, `list_symbols`, `callers_of_symbol`,
  `callees_of_symbol`, `impact_of_symbol` and `graph_for_symbol`
- Bounded, deterministic graph neighborhoods with caller/callee depth and node
  limits, a truncation flag, and resolved-edge-only drawing
- Desktop backend session tests and index query/graph tests
- Decision record `docs/decisions/002-desktop-app-stack.md` (Tauri v2 with
  static vanilla HTML/CSS/JavaScript)
- Desktop screenshots and run instructions in the README

### Changed
- `SearchResult` includes the symbol `id`; `ImpactEntry` includes `symbol_id`
- `impact_of` merges same-named symbols by ID and uses a cycle-safe path
  sentinel instead of relying on the depth limit alone
- Callers, callees, impact and graph queries can be scoped to one symbol ID, so
  same-named definitions are no longer merged
- The desktop app reads and writes the same schema v3 database as the CLI; no
  migration is required

### Fixed
- Impact counts and entries render from the selected symbol's ID-scoped results
- Arabic RTL layout, graph mirroring and centered graph banners
- Language switches preserve the current graph zoom level
- Result and graph labels use explicit count labels instead of pluralized
  English/Arabic strings

## [0.4.0] - 2026-10-24
### Added
- Declaration signatures for Rust, Python and TypeScript symbols
- Documentation extraction: Rust `///` and `/**` comments, Python docstrings
  and TypeScript `/**` JSDoc, stored on `symbols.signature` and `symbols.doc`
- FTS5 full-text search over names, qualified names, signatures and docs
- `atlas search <QUERY> [--limit N] [--json]` with deterministic bm25 ordering
- Schema v3 with automatic v1/v2 → v3 migrations and FTS index rebuild
- Databases with a newer schema version are rejected with a clear message

### Changed
- Signatures collapse whitespace and are truncated at 500 characters;
  documentation is truncated at 4,000 characters
- Python docstrings are dedented using `inspect.cleandoc` semantics
- FTS queries treat user input as plain terms, so FTS operators cannot change
  the meaning of a search

### Fixed
- FTS rows are kept in sync when files are re-indexed or removed

## [0.3.0] - 2026-10-24
### Added
- Import extraction for Rust (`use`), Python (`import`/`from`) and TypeScript
  (`import`/`require`), including aliases, nested Rust use trees, Python
  relative imports and TypeScript default/namespace/destructured requires
- Import-aware call resolution, with `edges.resolution` recording `import`,
  `same_file`, `unique_name` or `external`
- `atlas report` (and `--json`) with per-resolution counts, external vs
  unresolved edges and the top 20 unresolved names
- Schema v2 with an `imports` table and an automatic v1 → v2 migration
- Root-relative file paths, stale-file removal on re-index and external edge
  counts in `atlas index` / `atlas stats`
- Decision record `docs/decisions/001-type-accurate-resolution.md` comparing
  rust-analyzer, SCIP and LSP (no backend implemented)

### Changed
- Rust method calls keep a simple receiver path (`x.method`) so they are not
  mistaken for imported free functions; complex receivers keep only the field
  name
- Python and TypeScript callees keep their full receiver path for import
  matching while name matching still uses the final segment
- Resolution now runs against an in-memory symbol map instead of one SQL query
  per edge, and filters imports by binding before matching

### Fixed
- Glob imports (`use super::*`, `from x import *`) no longer shadow concrete
  same-file or unique-name definitions
- Explicit imports that point at a missing internal target stay unresolved
  instead of falling back to an unrelated global name
- Name matching is case-sensitive (the previous SQL `LIKE` fallback was not)
- TypeScript/JavaScript extension handling for `./foo.js` imports that map to
  `foo.ts`

## [0.0.1] - 2026-10-24
### Added
- Initial workspace with atlas-core, atlas-parser, atlas-cli
- Call edge extraction for Rust (iterative traversal)
- atlas-index with SQLite storage and graph queries
- atlas-scan for repository scanning with incremental indexing
- CLI commands: index, stats, callers, callees, impact
