# Spring Boot Billing Service (Kotlin)

A complete Spring Boot 3 REST service — controller, service interface and `@Service` implementation,
`JpaRepository`, `@Entity` — written in Kotlin with the Gradle Kotlin DSL. It is here to show what
the extractor does when it has no grammar for the input language.

## What this sample demonstrates

- **No structural graph, at all.** `lci-codegraph` ships no Kotlin tree-sitter grammar, so nothing in
  this app is parsed into definitions or calls. The graph is not sparse or partial. It is empty.
- **But the source is still chunked and searchable.** `.kt`/`.kts` carry a language tag, so these
  files go through the windowed-text fallback: 8 files chunked, 0 skipped. They produce chunks and
  embeddings, and semantic search finds them.
- **The difference between those two sentences is the point.** A language tag is not a graph
  extractor. Being indexed for retrieval and being understood structurally are separate capabilities,
  and this sample is the case where a repository has exactly one of them.

## The graph

**0 nodes, 0 edges.**

For contrast, the same application written in Java —
[`spring-boot-maven-java`](../spring-boot-maven-java) — produces 47 nodes, 47 edges, 5 route nodes
and a `@FeignClient` service boundary from a comparable amount of source.

## What it does NOT show

Everything structural. `InvoiceController`'s `@GetMapping`/`@PostMapping` handlers, the
`InvoiceService` interface and its `@Service` implementation, `InvoiceRepository extends
JpaRepository` — all idiomatic Spring Boot Kotlin, and none of it appears as a node, an edge, a route,
or a call. Asking `graph_get_callers` about anything in this app returns nothing, because there is
nothing.

This is not a limitation to work around. It is the honest answer to "does this work on my Kotlin
service?", and the sample is committed *because* the answer is "not structurally", not despite it.
It also gives whoever adds a Kotlin grammar a ready-made fixture whose expected output is already
wired into `tests/examples.rs` — the day that grammar lands, this file is what tells you the gap
closed.

---

Regenerate with `UPDATE_GOLDEN=1 cargo test --test examples`.
