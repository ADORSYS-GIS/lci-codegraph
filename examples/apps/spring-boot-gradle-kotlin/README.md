# Spring Boot Billing Service (Kotlin)

A complete Spring Boot 3 REST API for billing, written in Kotlin with Gradle Kotlin DSL, demonstrating what happens when no tree-sitter grammar exists for the input language.

## What this sample demonstrates

- **Intentional absence**: `lci-codegraph` ships no Kotlin tree-sitter grammar, and `.kt` is not in the language fallback list. Every Kotlin file is skipped—not parsed, not chunked, entirely absent from the graph.
- **No partial extraction**: the graph is not "sparse" or "limited." It is empty. Committed this way deliberately.
- **Why this matters**: Kotlin Spring Boot services are common. A developer pointing this tool at a Kotlin codebase needs to know immediately that extraction does not work. An empty, committed graph answers that question honestly, instead of leaving them wondering whether the tool is broken or whether they misconfigured it.

## The graph

**0 nodes, 0 edges.** Completely empty.

## What it does NOT show

Everything. The `InvoiceController` with `@GetMapping` and `@PostMapping`, the `InvoiceService` interface and `@Service` implementation, the `InvoiceRepository` extending `JpaRepository`—all of it is idiomatic Spring Boot Kotlin, and none of it appears. This is not a limitation to work around. It is a statement: "Grammar missing; extraction unavailable."

---

Regenerate with `UPDATE_GOLDEN=1 cargo test --test examples`.
