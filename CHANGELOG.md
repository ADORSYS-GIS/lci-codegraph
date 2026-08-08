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

### Changed

- `src/walk.rs` is now a thin filesystem-reader driver: `walk_checkout` builds an `FsSource`, pushes
  every input it yields into an `Indexer`, records the pruned count, and calls `finish` — the one-parse
  chunk/graph logic itself now lives in `Indexer::push`, not in `walk.rs`.
- `IndexStats::files_skipped_binary` now also covers content that fails UTF-8 decoding outright;
  previously that path incremented no counter at all.
- The operator/gitignore path-filtering layer now lives entirely in the reader (`FsSource`), not in the
  indexing core — deciding which inputs are worth handing over is the reader's job, because only the
  reader can avoid paying to produce an input that would just be discarded.

### BREAKING

- `WalkOutput` and `WalkStats` are renamed to `IndexOutput` and `IndexStats`. Migration: replace
  `WalkOutput`/`WalkStats` with `IndexOutput`/`IndexStats` wherever they're named (the field shapes are
  unchanged, plus the two new counters above) — `walk_checkout`/`walk_checkout_from_env`'s signatures
  are otherwise unchanged.

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
