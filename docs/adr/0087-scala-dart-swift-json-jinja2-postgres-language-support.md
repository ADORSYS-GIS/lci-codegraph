# ADR-0087: Extend language coverage — Scala, Dart, Swift, JSON, Jinja2, Postgres

- **Status:** Accepted — Implemented
- **Date:** 2026-09-02
- **Deciders:** @leghadjeu-christian
- **Related:** [ADR-0086](0086-in-house-code-graph-crate.md) — the `LanguageSupport`/`REGISTRY` pattern
  this ADR uses unchanged, and the "language expansion" line of work every `src/lang/<language>.rs`
  file's doc comment already cites. This ADR is the first batch of new languages added after that
  pattern was established, and records the choices specific to *this* batch — the trait/registry
  design itself is not revisited here.

## Context and Problem Statement

ADR-0086 established `lci-codegraph` as the sole graph engine and, as part of that, a single
extension point for adding a language: one `LanguageSupport` impl per `src/lang/<language>.rs`,
registered in `REGISTRY`. At the time this ADR was written, six languages were registered: Rust
(native extractor), Python, JavaScript, TypeScript/TSX, Java, and CrateStack — all six with a real
structural-graph extractor (`GraphStrategy::Tags` or `RustNative`).

Seven more languages were requested: Kotlin, Scala, JSON, Jinja2, Dart, Swift, Postgres. The pattern
itself didn't need to change to add them — but three things did need a decision, because they hadn't
come up yet: (1) what to do with a language whose grammar has **no bundled `tags.scm` at all**
(`GraphStrategy` supported returning `None` in its type signature since ADR-0086, but no registered
language had ever actually exercised that branch — every prior addition had a usable tags query);
(2) what to do when a grammar's `tags.scm` exists upstream but the published crate doesn't re-export
it as `TAGS_QUERY`; and (3) what to do when a requested language's grammar crate is flatly
incompatible with this workspace's pinned `tree-sitter` version.

## Decision Drivers

- Reuse the existing `LanguageSupport`/`REGISTRY` extension point exactly as-is — no new
  abstractions for this batch.
- Never claim graph support (`GraphStrategy::Tags`) a grammar doesn't actually have. A language with
  no `tags.scm` upstream is still worth registering (real parsing beats a bare text tag), but must
  honestly report `graph_strategy() -> None`, not a fabricated or hand-invented query.
- Prefer a grammar's own exported `TAGS_QUERY` when available; vendor only when necessary, and only
  against the exact pinned tag (never `main`, which can be ahead of what's actually published) — with
  provenance recorded in the vendored file itself.
- A language that cannot compile into this workspace at all (a real `cargo build` failure, not a
  theoretical concern) does not get registered, full stop — no partial/fake registration to satisfy a
  request list.

## Considered Options

- **Option A — treat every requested language uniformly, hand-writing a `tags.scm`-equivalent query
  for any grammar that lacks one.** Rejected: this is bespoke, unverified-against-upstream logic to
  write and maintain for JSON/Jinja2/Postgres, none of which have a meaningful call-graph concept
  upstream grammar authors thought worth encoding. It would also set a bad precedent — inventing
  classifier logic no one asked this crate to own, instead of the crate's actual job (running what
  the grammar already ships).
- **Option B — only register languages whose crate exports `TAGS_QUERY` directly, skip the rest.**
  Rejected: this would have covered Dart and Swift only, leaving Scala (grammar-capable, just not
  crate-exported), and JSON/Jinja2/Postgres (grammar-capable, no `tags.scm` at all) with nothing —
  including no real parse — despite all five having genuinely usable grammars.
- **Option C — chosen. Tiered inclusion, decided per language by what its grammar actually ships:**
  full graph support where a real `tags.scm` exists and compiles (vendored or crate-exported), grammar
  registration with `graph_strategy() -> None` where no `tags.scm` exists upstream at all, and no
  registration where the grammar itself cannot be added to this workspace.

## Decision Outcome

Chosen option: **C**. Six of the seven requested languages land in one of the tiers below; one
(Kotlin) does not land in this workspace at all.

### Language tiers, this batch

| Language | Grammar registered | `GraphStrategy` | Why |
|---|---|---|---|
| Scala | Yes | `Tags` — **vendored** | upstream ships `queries/tags.scm`, but the published crate doesn't re-export it as `TAGS_QUERY` (unlike Python/Java/JS) |
| Dart | Yes | `Tags` — crate-exported | same shape as Python/Java/JavaScript |
| Swift | Yes | `Tags` — crate-exported | same shape as Python/Java/JavaScript |
| JSON | Yes | `None` | no `tags.scm` upstream at all — no functions/classes/calls to classify |
| Jinja2 | Yes | `None` | no `tags.scm` upstream at all |
| Postgres | Yes | `None` | no `tags.scm` upstream (only `highlights`/`injections`/`outline`) |
| Kotlin | **No** | — | see below |

### Why Kotlin is not included

Every `tree-sitter-kotlin` version published to crates.io, through `0.3.8`, declares a runtime
`tree-sitter` dependency (`>=0.21, <0.23`, or `0.20` on older releases). This workspace pins
`tree-sitter = "0.26"`. Cargo's `links = "tree-sitter"` uniqueness rule means only one version of the
native tree-sitter C library may be linked into the final binary — the two requirements are mutually
exclusive, and `cargo build` fails to resolve a dependency graph at all with `tree-sitter-kotlin`
added. This was verified directly (the dependency was added, `cargo build` was run, the resolver
error was observed), not inferred from reading the manifest alone.

Upstream's unreleased `main` branch has already decoupled the crate from a hard-pinned `tree-sitter`
version, via `tree-sitter-language` — the same binding style every grammar in this ADR uses — but no
crates.io release carries that fix yet. `.kt`/`.kts` keeps the pre-existing windowed-text-only
treatment it already had before this ADR (a language *tag*, no grammar, no graph); nothing regresses,
nothing new is gained. This is a revisit-later item, not a permanent exclusion — see Unresolved
questions.

### The `GraphStrategy::None` tier, exercised for the first time

`LanguageSupport::graph_strategy() -> Option<GraphStrategy>` has always been able to return `None` —
ADR-0086's own doc comment on the trait says so ("or `None` for a grammar we chunk but have no graph
extractor for") — but no registered language had actually returned it before this batch; every prior
addition had a usable `tags.scm`. JSON, Jinja2, and Postgres are the first real exercise of that
branch: registered with a grammar (so they are parsed and chunked, not treated as opaque text), while
honestly reporting no structural-graph capability rather than fabricating one to make the language
list look more complete than it is.

### Vendoring vs. composing

Where a grammar exports `TAGS_QUERY` directly (Dart, Swift), it is referenced as-is — no local copy
to maintain, no drift risk. Where it doesn't (Scala), the upstream query is vendored byte-for-byte
under `src/lang/queries/`, verified against the exact pinned tag (not `main`), with a header recording
the source commit and an explicit re-sync obligation on every version bump.

A real gap was found in the vendored Scala query during review of this PR: it has no pattern for a
*qualified* call (`Foo.helper()`, `x.foo()`), only a bare one (`helper()`) — confirmed empirically, a
qualified call produced zero `calls` edges before the fix. Rather than hand-editing the vendored file
(which would break the "byte-identical to upstream" guarantee the provenance header makes), the fix is
a separate local supplement file, composed with the vendored query at query-compile time the same way
`typescript.rs` already composes the JavaScript and TypeScript queries into one.

## Consequences

- **Good:** six new languages get real tree-sitter parsing; three of them (Scala, Dart, Swift) get
  full structural-graph support, including cross-file call resolution.
- **Good:** the `GraphStrategy::None` tier is now a proven, tested path — the next language with no
  `tags.scm` upstream needs no new design, just a registration.
- **Good:** `.json` graduates from the generic `text` tag to a real grammar (still windowed-chunked,
  since it has no `interesting_node` entries — see Unresolved questions).
- **Bad:** Kotlin, explicitly requested, is not available in this workspace. This is a real gap for
  any Kotlin-heavy repo, blocked on an upstream release this crate does not control.
- **Neutral:** the vendored Scala query (plus its local supplement) is a maintenance liability future
  `tree-sitter-scala` bumps must account for by hand — documented, not hidden, but real.

## Alternatives considered

- **Registering Kotlin anyway, pinned to an older `tree-sitter`.** Not viable: `tree-sitter` itself
  has `links = "tree-sitter"`, so the workspace can only ever link one version of it across every
  dependency — there is no per-crate override that lets one grammar use 0.21 while the rest use 0.26.
- **Hand-writing a hypothetical `tags.scm` for JSON/Jinja2/Postgres.** Rejected per Option A above —
  these grammars' own authors chose not to ship one, because there is no meaningful call-graph concept
  for JSON at all, and Jinja2/SQL's call-like constructs (template macros, SQL function calls) were
  judged not worth this ADR inventing unverified classifier logic for.

## Unresolved questions

- **Kotlin:** revisit once `tree-sitter-kotlin` publishes a crates.io release compatible with
  `tree-sitter >= 0.26` (upstream's `main` branch already has the fix, just not a release).
- **Postgres's `LANGUAGE_PLPGSQL` grammar** is registered nowhere — PL/pgSQL is not written to its own
  files in practice (it lives dollar-quoted inside a `CREATE FUNCTION` in an ordinary `.sql` file), so
  there is no standalone file extension to route a second `LanguageSupport` by. Revisit only if a real
  need for parsing *extracted* PL/pgSQL bodies shows up.
- **`chunk.rs::interesting_node`** (symbol-level chunking) was not extended for any language in this
  batch — tracked separately as its own architectural question, not folded into this ADR: see
  [issue #47](https://github.com/ADORSYS-GIS/lci-codegraph/issues/47).

## More Information

- [ADR-0086](0086-in-house-code-graph-crate.md) — the `LanguageSupport`/`REGISTRY` pattern this ADR
  extends.
- [`docs/architecture.md`](../architecture.md), "Adding a language" — the mechanical how-to this ADR's
  decisions were made within, unchanged by this batch.
- PR [#44](https://github.com/ADORSYS-GIS/lci-codegraph/pull/44) — the implementation.
- Issue [#45](https://github.com/ADORSYS-GIS/lci-codegraph/issues/45) — the tracking ticket.
- Issue [#47](https://github.com/ADORSYS-GIS/lci-codegraph/issues/47) — the `interesting_node`/
  chunk-boundary follow-up this ADR defers.
