# Architecture

This is the document to read before touching `src/graph/`. It describes how the pipeline actually
fits together, based on the code as it stands — not the aspirational shape.

## The one-parse design

Everything downstream of a source file starts from a single `tree_sitter::Tree` — and, since the
raw-inputs cutover (ADR-0086), everything upstream of that starts from a
[`RawInput`](../src/input.rs) rather than a file on disk. `src/input.rs`'s `Indexer::push` is where
the one-parse decision is actually made: given one `RawInput`, it parses the content **once**
(`lang::parse`) and hands that one tree to both:

- the chunker (`chunk::chunk_tree`), and
- the graph extractor (`graph::extract_file`), when `IndexOptions::build_graph` is set.

`Indexer::push` never touches a filesystem itself — it only ever sees a `RawInput` that something
else already produced. `src/walk.rs`'s `FsSource` is what turns a checkout into that stream of
`RawInput`s: it walks the tree, applies both ignore layers, reads each surviving file's bytes, and
yields one `RawInput` per file. `walk_checkout` is the thin driver that wires the two together —
build an `FsSource`, `push` everything it yields into an `Indexer`, record the pruned count, `finish`.
Nothing about the one-parse design is specific to the filesystem: any other reader that can produce
`RawInput`s (a git object store, a tarball, an HTTP fetch, editor buffers, DB rows) gets the same
one-parse behaviour for free by feeding the same `Indexer`. See the "reader/indexer seam" section
below for what belongs on which side of that line.

This one-parse path only happens on the fast path — a language with a real graph extractor
(`lang::has_graph(language)`). If graph extraction is off, or the language has no extractor, the input
is chunked via `chunk::chunk_file`, which re-parses internally if it needs to; there is no second
parse when both chunking and graph extraction run, which is the case the one-parse design exists for.

```mermaid
flowchart TD
    R["reader (e.g. FsSource)<br/>— other readers could sit here too"] --> RI["RawInput { path, content, language }"]
    RI --> P["Indexer::push"]
    P --> B{"has_graph(language)<br/>&& build_graph?"}
    B -- yes --> C["lang::parse (once)"]
    C --> D["chunk::chunk_tree"]
    C --> E["graph::extract_file"]
    B -- no --> F["chunk::chunk_file<br/>(chunks only)"]
    E --> G["FileSymbols<br/>(nodes, contains/method edges,<br/>unresolved call sites, callables)"]
    G -.->|"collected across all pushed inputs"| H["Indexer::finish"]
    H --> I["graph::resolve(Vec&lt;FileSymbols&gt;)"]
    I --> J["canonical Graph<br/>(sorted + deduped)"]
```

`Indexer` collects one `FileSymbols` per graph-eligible input into a `Vec<FileSymbols>` as `push` is
called, then `finish` calls `graph::resolve` exactly once at the end, over everything that was pushed
— cross-file resolution needs every input's definitions to exist before it can attribute a call
correctly.

## The reader/indexer seam

`src/input.rs` and `src/walk.rs` are split along a deliberate seam, and the module doc comments on
both sides say so explicitly — this section is the model to follow when reasoning about it, or when
deciding which side a new piece of logic belongs on.

`crate::input` is the source-agnostic core: `RawInput`, `IndexOptions`, `Indexer`, `index_inputs`. It
knows a path and some bytes and nothing else — not where they came from, not whether a filesystem was
ever involved. Everything it does is a function of the bytes it was handed: the byte cap, the
UTF-8/binary sniff, language detection, the one-parse chunk+graph dispatch, PDF extraction.

`crate::walk` is one *reader*: `FsSource`, a filesystem-specific `Iterator<Item = RawInput>`, plus the
`walk_checkout` convenience driver built on top of it. A reader's job is narrower than it looks from
the outside — it does not index anything itself. It only decides *which* inputs are worth producing
at all, and turns each surviving one into a `RawInput`.

**Path filtering — the ignore layers — sits in the reader, not the indexer, and this is the one rule
worth internalising about the seam.** It would be easy to imagine `Indexer::push` taking a path and
deciding whether to bother with it. That is exactly backwards: by the time a `RawInput` reaches
`push`, its bytes have already been read off disk (or fetched over HTTP, or pulled from a DB row) —
the expensive part is done. A reader that knows in advance it will discard a path (a `.gitignore`d
file, a `node_modules` subtree, an operator-configured glob) can skip producing the `RawInput` for it
entirely, and `FsSource` does exactly that: its `filter_entry` callback prunes a directory *before
walking into it*, so an ignored subtree's files are never even stat'd, let alone read. Putting that
decision in `Indexer::push` instead would mean every reader — including ones for which "path" barely
means anything, like a database row — pays to produce an input just to have it thrown away, and would
give the indexing core an ignore-list dependency it has no other reason to carry. `Indexer` does still
participate: a reader that pruned inputs before `push` ever saw them reports the count via
`Indexer::record_pruned`, so `IndexStats::paths_ignored` reflects reader-side pruning without the
indexer needing to know *why* those inputs never arrived.

What *does* belong on the indexer side, correspondingly, is anything that can only be decided from the
content itself, because a reader — by construction — has not looked at the bytes yet when it decides
whether to hand them over. The byte cap, the UTF-8 decode, the binary content sniff, and language
detection are all content-level judgment calls `Indexer::push` makes once it has the bytes in hand;
`MAX_INPUT_BYTES` is the one exception that leans the other way on purpose — it is a content-level
constant, but reader-facing, specifically so a reader can bound its own read at the I/O level (as
`FsSource` does, reading at most `MAX_INPUT_BYTES + 1` bytes) rather than pulling an oversized input
into memory whole only to have `push` reject it after the fact.

## The `Classifier` seam

`src/graph/emit.rs` runs one shared depth-first walk (`walk`) that emits definition nodes,
`contains`/`method` edges, and unresolved call sites. What differs per language is *how a tree-sitter
node is recognised as a definition or a call site* — that recognition is factored out behind
`emit::Classifier`:

```rust,ignore
enum Classifier<'a> {
    Rust,
    Tagged(&'a tags::TaggedSymbols),
}
```

- **`Classifier::Rust`** delegates to the same `interesting_node` function the chunker uses
  (`chunk::interesting_node`, matching on tree-sitter node kinds like `function_item`, `impl_item`,
  `struct_item`) plus a dedicated `call_expression` navigation (`graph::callee::callee_ref_of`) for
  call sites. This is Rust's own hand-written node-kind extractor. It exists so chunk and graph
  symbols stay in lock-step for Rust specifically, and so the committed golden
  (`tests/golden/sample-repo.graph.json`) is byte-stable — a property a query-driven extractor
  running someone else's grammar-authored query would not guarantee across grammar upgrades.
- **`Classifier::Tagged`** wraps a `tags::TaggedSymbols` — the result of running the language's
  bundled `tags.scm` query (`tags::extract`) once per file, *before* the DFS starts. Every other
  supported language (Python, JavaScript, TypeScript, TSX, Java) goes through this path. The tags
  query gives definitions (`@definition.function`, `@definition.class`, …) and call references
  (`@reference.call`) keyed by tree-sitter node id, which `Classifier::Tagged::classify`/`call_site`
  look up by node id during the walk.

Which classifier a language uses is decided once, per file, by `lang::LanguageSupport::graph_strategy`
(`GraphStrategy::RustNative` vs. `GraphStrategy::Tags(&'static Query)`) — see `src/lang/mod.rs`. This
is the single place that determines Rust-native vs. tags-based; the rest of `emit.rs`,
`graph::callee`, and `graph::resolve` are written against the `Classifier` abstraction and do not
know or care which source produced it.

What the tags query does **not** give — and what `emit.rs` still derives from the tree itself,
identically for both classifier variants — is containment (which definition is nested inside which)
and the call qualifier (the receiver of a qualified call, e.g. the `Foo` in `Foo.bar()`). Both are
positional facts about where a node sits in the tree, not something a flat tag list carries.
`Classifier::container_scope` and `Classifier::call_site`'s qualifier handling exist specifically to
recover those two facts uniformly, regardless of classifier source.

## Per-file extraction: recording, not resolving

`graph::extract_file` produces a `FileSymbols` for one file:

- `nodes: Vec<GraphNode>` — the file node itself (`node_id` = the file path, 1-based `start_line: 1`)
  plus one node per definition found by the walk.
- `contains: Vec<GraphEdge>` — the `contains`/`method` edges, fully resolved *within this file* (a
  definition's parent is always known at emission time — it is the top of the `stack` the DFS
  maintains).
- `calls: Vec<CallSite>` — **unresolved**. Each is `{ caller: String, name: String, qualifier:
  Option<String> }`: the enclosing definition's node id, the bare callee name, and an optional
  qualifier. Nothing here is a graph edge yet — resolving a `CallSite` to an actual target definition
  requires knowing about definitions in *other* files, which isn't available until every file has
  been walked.
- `callables: Vec<Callable>` — every function/method definition found, `{ name, node_id, scope }`,
  where `scope` is the enclosing type name (a Rust `impl S` or a `class C`) used only to disambiguate
  same-named callables later. This is the table `resolve` matches call sites against.

The DFS (`emit::walk`) threads three pieces of state as it descends: a `stack` of enclosing definition
node ids (for `contains` parenting and for attributing a call site to its caller), a `scope` (the
nearest enclosing type name, for method disambiguation), and an `enclosing_kind` (to decide whether a
callable directly inside it should be a `method` edge — `is_type_container` matches `impl`/`trait`/
`struct`/`enum`/`class`/`interface` — or a plain `contains`).

Node ids are deterministic and content-derived: `def_node_id` formats them as
`<file>#<line>:<name-or-kind>` (e.g. `src/shapes.rs#9:new`), so the same input always produces the
same id — no counters, no UUIDs.

## Cross-file resolution

`graph::resolve::resolve(files: Vec<FileSymbols>) -> Graph` is the only place that looks across files.
It builds two lookup tables keyed by bare callee name:

1. a **global** table (`HashMap<&str, Vec<&Callable>>`) covering every callable in every file, and
2. for each file in turn, a **local** table covering only that file's callables.

For each unresolved `CallSite`, resolution tries the local table first (same-file definitions win),
falling back to the global table only when the local table has no match, or when the local match is
itself ambiguous and the global table can still narrow it down. This local-first policy means a
same-named definition in another file never shadows an unambiguous local one.

### How ambiguity is handled

The core of `pick()` is a three-way outcome, not a boolean:

```rust,ignore
enum Pick<'a> {
    One(&'a str),   // exactly one candidate — emit the edge
    Ambiguous,      // several same-named candidates, qualifier didn't narrow it — drop, count
    None,           // no candidate in this table — try the next one
}
```

- **A single candidate** resolves, unless a *type* qualifier is present and positively contradicts it
  (`only.scope.as_deref() != Some(q)`) — a *module*-style qualifier that isn't a type scope still
  matches, so `math::add()` resolves to a free function `add` with no type scope.
- **Several same-named candidates** resolve only if a qualifier narrows the set to exactly one match
  on `scope`. Otherwise the call is **ambiguous** — and an ambiguous call is *dropped*, not resolved
  to an arbitrary candidate and not fanned out to every candidate. This is the deliberate,
  precision-favouring choice documented directly in `resolve.rs`: "a bare name matching several
  same-named defs is dropped and counted, not fanned out."

Concretely: if `foo()` is defined in two files and the call site has no qualifier that can tell them
apart, `resolve` produces **no** `calls` edge for that call — not one arbitrary one, and not two. This
trades recall for never mis-attributing a call to the wrong definition. `ambiguous`/`unresolved`
counts are logged (`tracing::debug!`) so this is visible, not silent.

Qualifiers themselves are recovered structurally, not through type inference:
`graph::callee::qualifier_from_callee_node` looks at the callee name node's *immediate* parent in the
tree (`member_expression`/`attribute`/`method_invocation`) and takes its `object`/receiver, but only
when that receiver is a plain identifier that isn't `self`/`cls`/`this`/`super` (those carry no type
information and are deliberately treated as "no qualifier" rather than a bogus one). There is no
attempt to resolve a receiver's actual type — a call through a variable of unknown type resolves the
same as an unqualified call.

## Canonicalisation

After every file's call sites are resolved, `resolve` sorts and dedups both `nodes` and `edges`
(`GraphNode`/`GraphEdge` derive `Ord`) before returning the `Graph`. This is what makes the output
stable enough to snapshot as a golden file and stable enough to submit downstream without spurious
diffs between runs over the same input.

## Adding a language

A language with a real graph extractor is one `LanguageSupport` implementation in
`src/lang/<language>.rs`, registered in the `REGISTRY` slice in `src/lang/mod.rs`. For a
tags-driven language that means: point `ts_language()` at the grammar, and return
`GraphStrategy::Tags(&query)` where `query` is the grammar's bundled `TAGS_QUERY` (composed with
another language's `TAGS_QUERY` first, if the grammar's own tags file doesn't cover concrete
class/function/method/call nodes on its own — see `src/lang/typescript.rs` for the composed-query
pattern). Nothing in `emit.rs`, `graph::callee`, or `graph::resolve` needs to change: they are written
against `Classifier`/`GraphStrategy`, not against a per-language `match`. A language with no
`graph_strategy` (i.e. no grammar in this crate at all) is still chunked via the windowed-text
fallback — it is simply absent from `graph::resolve`'s input.
