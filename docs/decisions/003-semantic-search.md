# 003 — Semantic search

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** 10 (v0.9.0-beta)
- **Deciders:** project owner

## Context

Milestone 10 adds semantic search: a user describes what they are looking for
in natural language and Atlas ranks symbols by meaning rather than by exact
tokens. Atlas is local-first, makes no network calls at runtime, and must keep
working offline. The only allowed network operation is an explicit,
user-initiated, one-time model download with a visible prompt.

The index already stores symbol names, qualified names, kinds, signatures,
documentation, file paths and languages. A semantic index needs an embedding
model, a place to store vectors, and a query path. The desktop app and the CLI
must share the same database and model cache.

## Options considered

### A. ONNX Runtime via `fastembed`

- **Benefits:** Mature embedding library; many models; CPU and GPU support.
- **Costs:** Large native ONNX Runtime dependency, large binary, build-time
  binary downloads, and an inference stack that is much heavier than the rest
  of Atlas. Model download is normally implicit, which conflicts with the
  explicit-download rule unless wrapped carefully.
- **Fit:** Workable, but disproportionate for a beta feature.

### B. Transformer inference via Candle

- **Benefits:** Pure Rust, no native runtime, good quality with MiniLM/BGE.
- **Costs:** Adds a transformer runtime, tokenizer and ~90 MB model; slower CPU
  indexing; more code to maintain (BERT loading, pooling, batching).
- **Fit:** Good quality, but more machinery and download size than needed for
  the first semantic-search release.

### C. Static Model2Vec embeddings via `model2vec-rs`

- **Benefits:** Official Rust implementation; no transformer inference at query
  time; a code-specific model (`minishlab/potion-code-16M-v2`) is available;
  model files are ~33.5 MB; encoding is fast; the crate can be built with
  `local-only`, so loading never touches the network. The model is MIT
  licensed.
- **Costs:** Static embeddings are less accurate than a full transformer;
  English/code-focused; 256 dimensions; model quality must be evaluated and
  documented honestly.
- **Fit:** Best balance for a local-first beta.

### D. External local embedding service (Ollama, llama.cpp server, etc.)

- **Benefits:** Access to larger models without embedding a runtime.
- **Costs:** A second process and installation, environment-dependent results,
  lifecycle management, and a poor fit for a self-contained desktop app.
- **Fit:** Poor for v1.0.

### E. Lexical search only

- **Benefits:** No new dependency or download.
- **Costs:** Does not satisfy Milestone 10; natural-language queries such as
  "send an HTTP request with a timeout" cannot match code that never uses those
  exact words.
- **Fit:** Rejected as the milestone deliverable.

## Comparison

| Criterion | A. fastembed | B. Candle | C. Model2Vec | D. Local service |
|---|---|---|---|---|
| Offline after download | Yes | Yes | Yes | Yes |
| Extra runtime | ONNX Runtime | None (Rust) | None (Rust) | External process |
| Model size | ~90 MB+ | ~90 MB+ | ~33.5 MB | Depends |
| Query latency | Low | Medium | Very low | Process-dependent |
| Indexing speed | Medium | Slow on CPU | Very fast | Process-dependent |
| Implementation effort | Low–medium | High | Low | Medium |
| Fit for a beta | Fair | Good | Good | Poor |

## Decision

Use **Model2Vec `minishlab/potion-code-16M-v2`** through `model2vec-rs` with the
`local-only` and `fancy-regex` features:

- Pin the HuggingFace revision and the SHA-256 of `config.json`,
  `tokenizer.json` and `model.safetensors`.
- Download only through an explicit `atlas model download` command (and the
  equivalent desktop button). The command prints the model, size, source and
  cache path, requires confirmation, and verifies hashes. No search or indexing
  command downloads anything.
- Store the model in the platform cache directory
  (`ATLAS_MODEL_DIR` overrides it), never in the repository.
- Store normalized `f32` vectors as SQLite BLOBs in a new schema v4 table,
  keyed by `(symbol_id, model)`. Brute-force cosine similarity is sufficient
  for the symbol counts Atlas targets; no ANN index is added.
- Embed symbol name, qualified name, kind, signature, documentation, file path
  and language. Use a content hash so `atlas embed` only recomputes stale
  symbols.
- Keep lexical FTS5 search as the default. Semantic search is opt-in in the
  CLI (`atlas search --semantic`) and in the desktop app (a search-mode switch).
- Document that the model is code-focused English, that semantic ranking is
  approximate, and that callers/impact queries remain the authoritative
  structural answers.

## Consequences

- Users who want semantic search download ~33.5 MB once; after that the model
  is local and all commands work offline.
- Re-indexing invalidates embeddings for changed symbols. `atlas embed` must be
  run again; the app shows stale counts and a build button.
- The model and its cache are not part of the repository or the SQLite file.
  The database only stores vectors and the model ID.
- Schema v4 upgrades v1/v2/v3 databases in place. Old databases without
  embeddings remain fully usable for lexical search and graph queries.
- The model is MIT licensed. Its HuggingFace card and pinned hashes are the
  provenance record.
