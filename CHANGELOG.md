# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Source-agnostic indexing core (`src/input.rs`, ADR-0086): `RawInput` (a logical path plus bytes,
  with an optional explicit `language` override), `IndexOptions`, a push-based `Indexer`
  (`new`/`push`/`record_pruned`/`finish`), and the `index_inputs(iter, &options)` convenience. A
  filesystem checkout is now one *reader* among several possible ones — a caller with content already
  in memory (a git object store, a tarball stream, an HTTP fetch, editor buffers, DB rows) can index it
  directly, with no checkout materialised on disk.
- `FsSource`: the filesystem reader, now public and directly usable as an `Iterator<Item = RawInput>`,
  so filesystem inputs can be mixed with in-memory ones in a single `Indexer`.
- `IndexStats::files_skipped_too_large` and `IndexStats::files_skipped_unsupported` counters, so a
  too-large input and an input with no determinable language are each individually diagnosable instead
  of both looking like "nothing got indexed."
- Semantic embeddings against an OpenAI-compatible `/embeddings` endpoint (`src/embed/`), enabled by
  `OPENAI_BASE_URL` (`EmbedConfig::from_env`) or a caller-supplied `WalkOptions::embed`. No local/
  in-process model and no Cargo feature gate — embedding is the live path whenever configured, not a
  dormant one. The embedded text is graph-aware: a small deterministic header (enclosing container,
  callees, callers) derived from the resolved `Graph` is prepended to each chunk's own content before
  it's sent. `embed::embed_output` runs the step over any `IndexOutput` — not just `walk_checkout`'s —
  after the graph is resolved, since the header needs cross-file callers/callees that don't exist until
  every input has been indexed (see `docs/architecture.md`).
- `IndexStats::chunks_embedded` and `IndexStats::embed_batches` counters, logged at the end of a run
  that embeds.
- `Chunk::embedding: Option<Vec<f32>>` and `Chunk::embed_input: Option<String>` (both
  `#[serde(skip_serializing_if = "Option::is_none")]`, so existing JSON output is unchanged when
  embedding is off).

### Changed

- `src/walk.rs` is now a thin filesystem-reader driver: `walk_checkout` builds an `FsSource`, pushes
  every input it yields into an `Indexer`, records the pruned count, calls `finish`, and — when
  `WalkOptions::embed` is set — calls `embed::embed_output` on the result. The one-parse chunk/graph
  logic itself now lives in `Indexer::push`, not in `walk.rs`; embedding never did and still doesn't,
  because it's fallible network I/O and the indexing core is infallible CPU work.
- `IndexStats::files_skipped_binary` now also covers content that fails UTF-8 decoding outright;
  previously that path incremented no counter at all.
- The operator/gitignore path-filtering layer now lives entirely in the reader (`FsSource`), not in the
  indexing core — deciding which inputs are worth handing over is the reader's job, because only the
  reader can avoid paying to produce an input that would just be discarded.
- **Breaking:** `Chunk` no longer derives `Eq` (only `PartialEq`) — a chunk carrying an
  `embedding: Vec<f32>` has no total equality, since floats aren't `Eq`. Downstream code relying on
  `Chunk: Eq` (a `HashSet<Chunk>`/`BTreeSet<Chunk>`, an `Ord`/`dedup` bound requiring it, etc.) will
  fail to compile. Verified none of that exists in this crate's own `src/`, `tests/`, or `examples/`.
- **Breaking:** `Chunk` gained the two fields above. Any downstream construction site using an
  exhaustive `Chunk { .. }` struct literal (rather than `..Default::default()` or a builder) will fail
  to compile until it sets `embedding` and `embed_input` too.
- The crate's dependency boundary is revised: `ureq` is now a dependency, used **solely** for the
  optional embeddings HTTP call — the one part of this crate's job that is inherently a network
  round-trip to a model endpoint. Every other part of the boundary (no `kube`/`sqlx`/forge client) is
  unchanged.

### BREAKING

- `WalkOutput` and `WalkStats` are renamed to `IndexOutput` and `IndexStats`. Migration: replace
  `WalkOutput`/`WalkStats` with `IndexOutput`/`IndexStats` wherever they're named (the field shapes are
  unchanged, plus the new counters above) — `walk_checkout`/`walk_checkout_from_env`'s signatures are
  otherwise unchanged.

## [0.1.0] - 2026-08-07

### Added

- Initial standalone release, exported from the Lightbridge Code Intelligence monorepo
- Semantic chunking for source code with bounded token window support
- Cross-file structural call graph extraction for:
  - Rust
  - Python
  - TypeScript/JavaScript (including TSX/JSX)
  - Java
- Composable gitignore-style ignore-list that layers on top of the repository's own `.gitignore`
  rather than replacing it
- Bounded PDF text extraction, guarded for untrusted input (byte cap before parse, pre-flight
  decompression-bomb check, `catch_unwind`, parse timeout)
- Test suite of 182 tests: 130 unit (93.8% line coverage), 47 integration across per-language
  fixtures with committed goldens, and 5 Docker-backed container tests — a Neo4j round-trip
  asserting the downstream retrieval queries, glibc and musl build/run containers, and pinned
  real-world repository clones
- Governance-based contribution workflow with AI usage declarations
- CI covering fmt, clippy, tests, MSRV, coverage floor, rustdoc, `cargo-deny` and a publish
  dry-run, plus a tag-driven crates.io release workflow

### Fixed

- Rust trait methods declared without a default body (`fn greet(&self);`) are now extracted. They
  parse as `function_signature_item` rather than `function_item` and were previously classified as
  nothing at all, so a trait interface produced no graph node and no chunk — invisible to both
  structural and semantic search. Such declarations are indexed as definitions but deliberately are
  **not** call targets: a call dispatches to an implementation, and letting declarations compete for
  the same name would have made every single-impl trait method call ambiguous, and therefore dropped.
- Binary content can no longer reach chunk output. The NUL sniff lived only in `chunk_file`, which
  the graph-enabled walk never calls, so a binary blob that happens to be valid UTF-8 was windowed
  with raw NUL bytes in `Chunk::content`. The guard now sits where a chunk is produced and in the
  walk ahead of both consumers, so the graph no longer ingests binary content either. Skipped files
  are reported via the new `WalkStats::files_skipped_binary` counter instead of vanishing.

### Security

- `pdf-extract` is floored at 0.12, the first release depending on `lopdf >= 0.42`, which patches
  [RUSTSEC-2026-0187](https://rustsec.org/advisories/RUSTSEC-2026-0187) — unbounded recursion on
  deeply nested PDF objects that aborts the process with `SIGABRT`. Because that is an abort and
  not a panic, the crate's `catch_unwind` guard could not contain it.

[0.1.0]: https://github.com/vymalo/lci-codegraph/releases/tag/v0.1.0
