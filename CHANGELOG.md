# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
