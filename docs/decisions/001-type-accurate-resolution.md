# 001 — Type-accurate call resolution

- **Status:** Proposed (decision checkpoint — nothing implemented)
- **Date:** 2026-10-24
- **Milestone:** 7 (v0.3.0)
- **Deciders:** project owner

## Context

Atlas resolves call edges from syntax plus import information. It does not have
type information, so it cannot always tell which definition a call refers to.
The current pipeline is:

1. Extract symbols, calls and imports with tree-sitter (Rust, Python,
   TypeScript).
2. Resolve calls through imports when the import names the target module and
   the target symbol exists in the index.
3. Classify calls through imports that point outside the repository as
   `external`.
4. Otherwise fall back to conservative name matching (`same_file`, then a
   unique name), and leave everything else unresolved.

This is fast, deterministic, fully offline, and has no toolchain dependencies.
Measured on three real repositories with the v0.3.0 build:

| Repository | Language | Edges | Resolved | External | Unresolved | Index time |
|---|---|---:|---:|---:|---:|---:|
| ripgrep | Rust | 13,351 | 4,659 (34.9%) | 829 (6.2%) | 7,863 (58.9%) | 0.57 s |
| requests | Python | 2,510 | 710 (28.3%) | 775 (30.9%) | 1,025 (40.8%) | 0.12 s |
| zod | TypeScript | 7,621 | 3,140 (41.2%) | 572 (7.5%) | 3,909 (51.3%) | 0.91 s |

Most remaining unresolved edges are method calls on values whose type Atlas
does not know (`unwrap`, `isinstance`, `issue.maximum.toString`), dynamic
language features, builtins, and re-exports. Type information would address a
large part of this, but at a significant cost.

## Options considered

### A. Embed rust-analyzer (`ra_ap_*` crates)

Use rust-analyzer's crates as a library to build a semantic model and ask for
definitions/references.

- **Benefits:** Best possible Rust accuracy, including generics, traits,
  macros and method resolution. Fully local. No separate process.
- **Costs:** Large dependency tree and build time; the `ra_ap_*` crates are
  not a stable public API and change frequently; significant integration and
  maintenance effort. Rust only — Python and TypeScript would still need a
  different solution. Macro expansion may execute build scripts/proc macros,
  which conflicts with "never execute repository code".
- **Effort:** High (weeks), ongoing upgrade work.
- **Risk:** High (API churn, compile-time and binary-size growth, safety
  review of build-script execution).

### B. Consume SCIP indexes

Require or invoke language-specific SCIP indexers (`scip-rust`,
`scip-python`, `scip-typescript`) and read the resulting protobuf.

- **Benefits:** Language-agnostic model (symbols, definitions, references,
  implementations). Designed for code intelligence and cross-repository
  navigation. Deterministic index artifacts. Keeps Atlas' parser for
  structure and uses SCIP only for resolution.
- **Costs:** Adds external indexer binaries/toolchains that users must install
  or Atlas must download (explicit, one-time, visible — allowed by the
  project rules but a UX burden). Indexers themselves may need a working
  build environment. Protobuf schema and indexer versions must be tracked.
  Accuracy depends on the indexer.
- **Effort:** Medium–high (weeks): dependency management, invocation,
  mapping SCIP symbol IDs to Atlas symbols, storage schema, fallback when an
  indexer is missing.
- **Risk:** Medium (external tool availability, version drift, larger
  installation, but no runtime network requirement once installed).

### C. LSP client

Launch `rust-analyzer`, `pyright`/`basedpyright`, and `tsserver` and query
`textDocument/definition` and `textDocument/references`.

- **Benefits:** Uses the same engines editors use; good accuracy; no need to
  embed language internals; supports all three languages.
- **Costs:** Runtime dependency on three language servers, process lifecycle
  management, project configuration discovery, slow cold starts, large
  installs. Results can depend on the environment and server version.
  Servers may run build scripts or import project code (safety concern).
  Hard to make deterministic and to run in CI.
- **Effort:** High (weeks), plus ongoing compatibility work.
- **Risk:** High (process management, environment sensitivity, security
  review, poor fit for batch indexing).

### D. Keep syntax + imports, add targeted improvements

Stay with the current approach and improve the common failure modes:
re-export following, `tsconfig` path aliases, Python package roots,
`__all__`, and better handling of `self`/`cls` method calls.

- **Benefits:** No new dependencies, remains fast/offline/deterministic,
  incremental and low-risk. Each improvement can be measured with
  `atlas report`.
- **Costs:** Cannot resolve genuinely type-dependent calls; accuracy will
  plateau well below a compiler-backed index.
- **Effort:** Low–medium (days per improvement).
- **Risk:** Low.

### E. Hybrid: parser-first, optional deep index

Keep the current pipeline as the default, and add an optional, explicitly
requested "deep index" that imports a SCIP index (or LSP results) into
separate tables. Queries prefer deep results and fall back to syntax results.

- **Benefits:** Best of both: zero-dependency default, high accuracy for users
  who opt in, language-agnostic via SCIP. Keeps the fast path fast.
- **Costs:** Two resolution paths to maintain; schema and precedence rules;
  UI must explain which edges are type-accurate; still needs indexer
  installation.
- **Effort:** High (several weeks), but can be staged after v1.0.
- **Risk:** Medium.

## Comparison

| Criterion | A. rust-analyzer | B. SCIP | C. LSP | D. Status quo+ | E. Hybrid |
|---|---|---|---|---|---|
| Rust accuracy | Excellent | Very good | Very good | Fair | Very good |
| Python accuracy | None | Very good | Very good | Fair | Very good |
| TypeScript accuracy | None | Very good | Very good | Fair | Very good |
| Offline after setup | Yes | Yes | Yes | Yes | Yes |
| Extra install | Large | Medium | Large | None | Medium (opt-in) |
| Determinism | Good | Good | Poor | Good | Good |
| Safety review needed | Yes | Medium | High | No | Medium |
| Effort | High | Medium–high | High | Low | High |
| Fit for v1.0 | Poor | Fair | Poor | Good | Fair |

## Recommendation

Adopt **D now, E later**:

1. Finish v1.0 with syntax + import-aware resolution. Publish the measured
   numbers and limitations (this record and the README).
2. Before v1.0, spend a small, bounded effort on the highest-value syntax
   improvements (re-exports, TypeScript path aliases, Python package roots).
3. After v1.0, evaluate **E** with SCIP as the first deep-index backend. SCIP
   is the best fit for a local-first, multi-language tool: it produces a
   deterministic artifact and does not require embedding three language
   servers. Treat `rust-analyzer` embedding (A) and LSP (C) as rejected for
   now because of maintenance cost, safety exposure, and poor batch
   determinism.

## Decision

**No type-accurate backend is implemented in v0.3.0.** The schema keeps
`edges.resolution` extensible so a future deep index can add values such as
`scip` without breaking old databases. This record is a decision checkpoint:
the project owner must choose between D, E (SCIP), or another option before
any implementation work starts.

## Consequences

- Accuracy numbers in the README are explicitly syntax/import-based and must
  not be presented as compiler-accurate.
- `atlas report` remains the tool for measuring any future improvement.
- If E is chosen, a new decision record must define the SCIP symbol mapping,
  precedence rules, storage schema, and the offline installation flow.
