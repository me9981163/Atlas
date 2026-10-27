# AGENTS.md — Atlas

## Project principles
- **Local-first**: No network calls at runtime. Any download (e.g. model) must be explicit, user-initiated, one-time, visible, and work offline after.
- **Honest about uncertainty**: Never guess. Name-based resolution is ambiguous; unresolved edges must be reported clearly. Never mistake silence for "no callers".
- **Deterministic**: Output order must be deterministic (source order). All query results sorted deterministically.
- **Safe by default**: Never execute repository code. Validate paths to avoid traversal issues.
- **Small and readable**: Simple code over clever code. Comments explain *why*, not *what*.

## Target data model
- **Symbols**: functions, methods, structs, enums, traits, impls, modules, consts, statics, type aliases (language-agnostic concepts where applicable)
- **RawEdge** (parser output): `{ from: String (qualified name of enclosing function/method), to_name: String (callee as written), kind: EdgeKind::Calls, line: usize }` - parser does not know symbol IDs; resolution happens later in index
- **FileSymbols/FileAnalysis**: contains symbols and edges per file
- **Edges in DB**: linked via `dst_symbol_id` after resolution; `resolution` (TEXT) records how resolved: `same_file`, `import`, `unique_name`, or NULL. Calls classified as `external` when import points outside repo.

## Schema
- Use SQLite with `rusqlite` + `bundled` feature
- `PRAGMA foreign_keys = ON` on every connection
- Schema version via `PRAGMA user_version` - all DB changes behind migrations
- Old databases must upgrade cleanly or be rebuilt with a clear message

## Hard rules
- **Iterative traversal only**: Walking AST/expression trees must be iterative (explicit stack or TreeCursor), never recursive over AST depth. Must have deep-nesting tests proving no stack overflow.
- **Transactional updates**: `replace_file` must be atomic - delete old symbols/edges and insert new ones atomically; failure must leave previous data intact.
- **Resolution**: Never guess. Rules in order: (1) exactly one match on callee name/suffix → link; (2) if several match, prefer same file if exactly one → link; (3) else leave NULL. Return counts of linked/ambiguous/unresolved.
- **Impact/cycles**: `impact_of` uses recursive CTE, must terminate on cycles, must not return duplicates, returns depth of first discovery.
- **Library code is silent**: Warnings collected and returned, not printed from library code.
- **Tests required**: Every claimed behavior must be backed by a test or command actually run.
- **Process**: One milestone at a time, run verification gate, report in format below, STOP and wait for approval. Decision checkpoints require decision record in `docs/decisions/NNN-*.md`. Human checkpoints require exact run instructions + checklist, wait for feedback.
- **No commits/tags**: Propose Conventional Commits message and version tag at end of each milestone.
- **No runtime network**: Except explicit user-initiated one-time model download with visible prompt; feature must work offline after.

## Planned crates (updated)
- `atlas-core` - shared types
- `atlas-parser` - language abstraction + extractors (Rust, Python, TypeScript) with iterative traversal
- `atlas-index` - SQLite storage, migrations, resolution, queries (callers/callees/impact), FTS5 search
- `atlas-scan` - repo walking, incremental indexing, parallel parsing
- `atlas-cli` - CLI interface
- `atlas-tauri` / `src-tauri` - Tauri v2 desktop app (thin backend calling crates)

## Out of scope (original) — OVERRIDDEN
UI, additional languages, search and semantic features are now IN SCOPE per milestones 5-11 below.

## Report format (use after every milestone)
```
### Milestone X — <title> (target tag: vX.Y.Z)
**Status:** Done / In Progress / Blocked

**What changed:**
- ...

**Decisions made:**
- ... (or link to docs/decisions/NNN-*.md)

**Tests added/updated:**
- ...

**Verification gate:**
- cargo fmt --all --check: ...
- cargo clippy --workspace --all-targets -- -D warnings: ...
- cargo test --workspace: ...
- (other commands run): ...

**Measured results (if applicable):**
- ...

**Uncertain / could not verify:**
- ...

**Next:** STOP — awaiting approval before Milestone X+1
**Proposed commit:** <conventional commit message>
**Proposed tag:** vX.Y.Z
```
